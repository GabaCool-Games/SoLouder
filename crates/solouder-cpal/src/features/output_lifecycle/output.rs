//! Opening the default output device and running a renderer on it.

use std::fmt;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SizedSample, StreamConfig};
use rtrb::Consumer;
use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{Clipper, ConfigError, Controller, Mixer, MixerConfig, Renderer, VOICE_COUNT};

use super::callback::{Callback, OutputShared, PanicPayload, ReturnOnDrop};
use super::health::{self, DISCONNECTED, FAILED, PLAYING, Watch};
use super::negotiate::{StreamPreferences, choose};

/// How to open the output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputConfig {
    /// Sample rate and channel preferences; unset values follow the device.
    pub stream: StreamPreferences,
    /// Device buffer in frames, 1 to [`MAX_BUFFER_FRAMES`]; `None` lets the
    /// device choose. The mixer renders at least 2048 frames per pass either
    /// way.
    pub buffer_frames: Option<u32>,
    /// How the final mix is limited.
    pub clipper: Clipper,
    /// Voices mixed at once (`setMaxActiveVoiceCount`), 1 to 1023.
    pub max_active_voices: u32,
    /// Queue sizes between the controller and the audio thread.
    pub realtime: RealtimeConfig,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            stream: StreamPreferences::default(),
            buffer_frames: None,
            clipper: Clipper::Roundoff,
            max_active_voices: 16,
            realtime: RealtimeConfig::default(),
        }
    }
}

/// The largest `buffer_frames` accepted: a little over a second at 48 kHz.
pub const MAX_BUFFER_FRAMES: u32 = 65_536;

/// Frames the mixer renders per pass when the device buffer is smaller.
const MIN_MIX_FRAMES: u32 = 2048;

/// How long closing waits for the backend to hand the renderer back.
const RETURN_WAIT: Duration = Duration::from_millis(500);

/// Why the output could not be opened.
#[derive(Debug)]
#[non_exhaustive]
pub enum OutputError {
    /// The system has no default output device.
    NoDevice,
    /// No configuration of the device suits both the preferences and the
    /// mixer (1, 2, 4, 6 or 8 channels; a convertible sample format).
    UnsupportedDevice,
    /// `max_active_voices` is outside 1 to `VOICE_COUNT - 1`.
    InvalidMaxActiveVoices(u32),
    /// `buffer_frames` is outside 1 to [`MAX_BUFFER_FRAMES`].
    InvalidBufferFrames(u32),
    /// The mixer rejected the settings.
    Config(ConfigError),
    /// The audio backend failed.
    Backend(cpal::Error),
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => write!(f, "no audio output device"),
            Self::UnsupportedDevice => write!(f, "the output device has no usable configuration"),
            Self::InvalidMaxActiveVoices(count) => {
                write!(
                    f,
                    "max_active_voices {count} is outside 1 to {}",
                    VOICE_COUNT - 1
                )
            }
            Self::InvalidBufferFrames(frames) => {
                write!(
                    f,
                    "buffer_frames {frames} is outside 1 to {MAX_BUFFER_FRAMES}"
                )
            }
            Self::Config(error) => write!(f, "invalid mixer settings: {error}"),
            Self::Backend(error) => write!(f, "audio backend error: {error}"),
        }
    }
}

impl std::error::Error for OutputError {}

impl From<cpal::Error> for OutputError {
    fn from(error: cpal::Error) -> Self {
        Self::Backend(error)
    }
}

/// What the output is doing, as far as the backend has reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputState {
    /// Rendering into the device.
    Playing,
    /// The device went away or the stream must be rebuilt; nothing plays.
    /// Final: open a new output to continue.
    Disconnected,
    /// The backend reported an error that ends the stream, or an error it
    /// could not classify (or a busy device) after which no callback came
    /// for half a second (or four callbacks, if longer). Underruns, device
    /// changes the stream followed and refused real-time priority never fail
    /// it, and an unclassified error is only counted
    /// ([`Output::watched_error_count`]) while callbacks keep arriving.
    /// Final: open a new output to continue.
    Failed,
    /// The renderer panicked; the output plays silence from then on. Final.
    /// See [`Output::panic_message`].
    Panicked,
}

/// Facts about the opened stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputInfo {
    /// The audio API in use, such as `WASAPI` or `ALSA` (`getBackendString`).
    pub backend: &'static str,
    /// The device's human-readable name, if the backend reports one.
    pub device_name: Option<String>,
    /// Output sample rate in Hz (the mixer's rate).
    pub sample_rate: u32,
    /// Output channels (the mixer's channel count).
    pub channels: u16,
    /// The device sample format the mix is converted to.
    pub sample_format: SampleFormat,
}

/// A running device stream that renders a [`Renderer`].
///
/// Dropping it stops the stream first and then frees the renderer on the
/// dropping thread. The stream type is not `Send` on every platform, so
/// neither is this: keep it on the thread that opened it.
///
/// The mix is converted to the device's format (`f32`, `f64`, `i8`, `i16`,
/// 24-bit, `i32`, `u8`, `u16` or `u32`) after clamping to `[-1, 1)`; NaN
/// samples play as silence.
///
/// A panic in the renderer is caught and the output goes silent, but this
/// is best-effort: the panic hook still runs on the audio thread first, and
/// may allocate or lock.
pub struct Output {
    stream: Option<cpal::Stream>,
    returned: Consumer<Renderer>,
    panics: Consumer<PanicPayload>,
    panic: Option<PanicPayload>,
    shared: Arc<OutputShared>,
    watch: Mutex<Watch>,
    info: OutputInfo,
}

impl fmt::Debug for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// Opens the default output device, creates a mixer for its configuration,
/// and starts playing. Returns the output and the controller to play with.
///
/// Settings that do not depend on the device are checked before it is
/// opened. On any error nothing keeps running and nothing needs cleanup.
pub fn open(config: &OutputConfig) -> Result<(Output, Controller), OutputError> {
    let max_active = config.max_active_voices;
    if max_active == 0 || max_active as usize >= VOICE_COUNT {
        return Err(OutputError::InvalidMaxActiveVoices(max_active));
    }
    if let Some(frames) = config.buffer_frames
        && !(1..=MAX_BUFFER_FRAMES).contains(&frames)
    {
        return Err(OutputError::InvalidBufferFrames(frames));
    }
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(OutputError::NoDevice)?;
    let default = device.default_output_config()?;
    let ranges: Vec<_> = device.supported_output_configs()?.collect();
    let supported =
        choose(&default, &ranges, config.stream).ok_or(OutputError::UnsupportedDevice)?;

    let mut mixer = Mixer::new(MixerConfig {
        channels: u32::from(supported.channels()),
        sample_rate: supported.sample_rate(),
        buffer_size: config.buffer_frames.unwrap_or(0).max(MIN_MIX_FRAMES),
        clipper: config.clipper,
        // 3D audio is not reachable through the device output yet (unit 8).
        ..MixerConfig::default()
    })
    .map_err(OutputError::Config)?;
    mixer
        .set_max_active_voice_count(max_active)
        .map_err(|_| OutputError::InvalidMaxActiveVoices(max_active))?;
    let (controller, renderer) = split(mixer, config.realtime);

    let stream_config = StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size: config
            .buffer_frames
            .map_or(BufferSize::Default, BufferSize::Fixed),
    };
    let info = OutputInfo {
        backend: host.id().name(),
        device_name: device.description().ok().map(|d| d.name().to_owned()),
        sample_rate: supported.sample_rate(),
        channels: supported.channels(),
        sample_format: supported.sample_format(),
    };
    let output = start(
        &device,
        stream_config,
        supported.sample_format(),
        renderer,
        info,
    )?;
    Ok((output, controller))
}

fn start(
    device: &cpal::Device,
    stream_config: StreamConfig,
    format: SampleFormat,
    renderer: Renderer,
    info: OutputInfo,
) -> Result<Output, OutputError> {
    let shared = Arc::new(OutputShared::default());
    let (back, returned) = rtrb::RingBuffer::new(1);
    let (panic_slot, panics) = rtrb::RingBuffer::new(1);
    let scratch_frames = renderer.config().buffer_size as usize;
    let holder = ReturnOnDrop::new(renderer, back);
    let callback = Callback::new(holder, scratch_frames, Arc::clone(&shared), panic_slot);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(device, stream_config, callback, &shared),
        SampleFormat::F64 => build::<f64>(device, stream_config, callback, &shared),
        SampleFormat::I8 => build::<i8>(device, stream_config, callback, &shared),
        SampleFormat::I16 => build::<i16>(device, stream_config, callback, &shared),
        SampleFormat::I24 => build::<cpal::I24>(device, stream_config, callback, &shared),
        SampleFormat::I32 => build::<i32>(device, stream_config, callback, &shared),
        SampleFormat::U8 => build::<u8>(device, stream_config, callback, &shared),
        SampleFormat::U16 => build::<u16>(device, stream_config, callback, &shared),
        SampleFormat::U32 => build::<u32>(device, stream_config, callback, &shared),
        _ => return Err(OutputError::UnsupportedDevice),
    }?;
    stream.play()?;
    Ok(Output {
        stream: Some(stream),
        returned,
        panics,
        panic: None,
        shared,
        watch: Mutex::new(Watch::new(Instant::now())),
        info,
    })
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut callback: Callback,
    shared: &Arc<OutputShared>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + cpal::FromSample<f32>,
{
    let errors = Arc::clone(shared);
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| callback.fill(data),
        move |error: cpal::Error| {
            health::report(&errors, error.kind());
        },
        None,
    )
}

impl Output {
    /// The negotiated stream.
    pub fn info(&self) -> &OutputInfo {
        &self.info
    }

    /// The stream's state as last reported by the backend.
    pub fn state(&self) -> OutputState {
        if self.shared.panicked.load(Ordering::Relaxed) {
            return OutputState::Panicked;
        }
        match self.shared.state.load(Ordering::Relaxed) {
            PLAYING if !health::stalled(&self.shared, &self.watch, self.info.sample_rate) => {
                OutputState::Playing
            }
            PLAYING => {
                let shared = &self.shared.state;
                let _ =
                    shared.compare_exchange(PLAYING, FAILED, Ordering::Relaxed, Ordering::Relaxed);
                self.state()
            }
            DISCONNECTED => OutputState::Disconnected,
            _ => OutputState::Failed,
        }
    }

    /// Backend errors that may or may not have stopped the stream (errors
    /// cpal could not classify, a busy device), reported so far. While
    /// callbacks keep arriving the output stays [`OutputState::Playing`].
    pub fn watched_error_count(&self) -> u64 {
        self.shared.watched_errors.load(Ordering::Relaxed)
    }

    /// Backend errors reported so far, including recoverable ones such as
    /// underruns.
    pub fn error_count(&self) -> u64 {
        self.shared.errors.load(Ordering::Relaxed)
    }

    /// Frames the device asked for in its most recent callback, the closest
    /// equivalent of `getBackendBufferSize`; 0 before the first callback.
    pub fn callback_frames(&self) -> u64 {
        self.shared.callback_frames.load(Ordering::Relaxed)
    }

    /// Device callbacks run so far.
    pub fn callback_count(&self) -> u64 {
        self.shared.callbacks.load(Ordering::Relaxed)
    }

    /// The message of the panic that silenced the output, if there was one
    /// and it carried text.
    pub fn panic_message(&mut self) -> Option<&str> {
        if self.panic.is_none() {
            self.panic = self.panics.pop().ok();
        }
        let payload = self.panic.as_ref()?;
        payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
    }

    /// Stops the stream and returns the renderer, with its voices. Waits up
    /// to half a second for the backend to release the callback; `None` if
    /// it has not, in which case the renderer is freed wherever the backend
    /// finally drops the callback.
    pub fn close(mut self) -> Option<Renderer> {
        self.stop()
    }

    fn stop(&mut self) -> Option<Renderer> {
        // Dropping the stream stops callbacks; the callback then sends the
        // renderer back, at once on most backends.
        drop(self.stream.take());
        let deadline = Instant::now() + RETURN_WAIT;
        loop {
            if let Ok(renderer) = self.returned.pop() {
                return Some(renderer);
            }
            if self.returned.is_abandoned() || Instant::now() >= deadline {
                return self.returned.pop().ok();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        drop(self.stop());
    }
}
