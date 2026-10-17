//! Output lifecycle: opening the device (or a headless renderer), sharing
//! the controller with systems, and reporting the output's state.

use std::sync::{Mutex, PoisonError};

use bevy::prelude::*;
use solouder_core::realtime::split;
use solouder_core::{
    Clipper, ConfigError, Controller, Mixer, MixerConfig, RealtimeConfig, Renderer, StreamWorker,
};
use solouder_cpal::{Output, OutputConfig, OutputInfo, OutputState};

/// Where the mix goes.
#[derive(Clone, Debug, PartialEq)]
pub enum OutputMode {
    /// The default output device. If it cannot be opened the app keeps
    /// running, [`AudioOutput`] reports why, and plays fail.
    Device(OutputConfig),
    /// No device: the app renders through [`HeadlessRenderer`] itself, for
    /// tests, servers and offline rendering.
    Headless(MixerConfig),
}

impl Default for OutputMode {
    fn default() -> Self {
        Self::Device(OutputConfig::default())
    }
}

impl OutputMode {
    /// A headless stereo mixer at 48 kHz.
    pub fn headless() -> Self {
        Self::Headless(MixerConfig {
            channels: 2,
            sample_rate: 48_000,
            buffer_size: 2048,
            clipper: Clipper::Roundoff,
            ..MixerConfig::default()
        })
    }
}

/// What the output is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioOutputState {
    /// Rendering into a device.
    Playing,
    /// The device went away; nothing plays.
    Disconnected,
    /// The backend failed.
    Failed,
    /// The renderer panicked; the device plays silence.
    Panicked,
    /// Rendering is driven by [`HeadlessRenderer`].
    Headless,
    /// No output could be opened; the text says why.
    Unavailable(String),
}

/// The output's state and, for a device, the negotiated stream.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct AudioOutput {
    /// Current state, refreshed every frame.
    pub state: AudioOutputState,
    /// The device stream, when one is open.
    pub info: Option<OutputInfo>,
}

/// Sends commands to the mixer. Present whenever an output exists.
///
/// Systems normally leave playback to the plugin's components; this is for
/// global settings such as volume.
#[derive(Resource)]
pub struct SolouderController(Mutex<Controller>);

impl SolouderController {
    /// The controller. Never blocks: Bevy already gives this system exclusive
    /// access.
    pub fn get(&mut self) -> &mut Controller {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The renderer in [`OutputMode::Headless`]; call [`render`](Self::render)
/// to advance the mix.
#[derive(Resource)]
pub struct HeadlessRenderer(Mutex<Renderer>);

impl HeadlessRenderer {
    /// Renders interleaved frames, applying queued commands first.
    pub fn render(&mut self, output: &mut [f32]) {
        self.renderer().render(output);
    }

    /// The renderer itself.
    pub fn renderer(&mut self) -> &mut Renderer {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The stream worker in [`OutputMode::Headless`]; call
/// [`service`](Self::service) before rendering so streamed clips have their
/// audio decoded. With a device, the worker runs on its own thread.
#[derive(Resource)]
pub struct HeadlessStreamWorker(Mutex<StreamWorker>);

impl HeadlessStreamWorker {
    /// Decodes until every streaming voice's buffer is full; returns
    /// whether there was anything to do.
    pub fn service(&mut self) -> bool {
        self.worker().service()
    }

    /// The worker itself.
    pub fn worker(&mut self) -> &mut StreamWorker {
        self.0.get_mut().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The device stream. Not `Send` on every platform, so it lives in the
/// world's non-send storage and is dropped, stopping the callback, when the
/// world is.
pub(crate) struct DeviceOutput(Output);

fn headless(
    config: MixerConfig,
    realtime: RealtimeConfig,
) -> Result<(Controller, Renderer), ConfigError> {
    let mixer = Mixer::new(config)?;
    Ok(split(mixer, realtime))
}

pub(crate) fn build(app: &mut App, mode: &OutputMode) {
    let (controller, state, info) = match mode {
        OutputMode::Device(config) => match solouder_cpal::open(config) {
            Ok((output, controller)) => {
                let info = output.info().clone();
                app.insert_non_send(DeviceOutput(output));
                (Some(controller), AudioOutputState::Playing, Some(info))
            }
            Err(error) => (None, AudioOutputState::Unavailable(error.to_string()), None),
        },
        OutputMode::Headless(config) => match headless(*config, RealtimeConfig::default()) {
            Ok((controller, renderer)) => {
                app.insert_resource(HeadlessRenderer(Mutex::new(renderer)));
                (Some(controller), AudioOutputState::Headless, None)
            }
            Err(error) => (None, AudioOutputState::Unavailable(error.to_string()), None),
        },
    };
    if let Some(mut controller) = controller {
        if let Some(worker) = controller.take_stream_worker() {
            if matches!(mode, OutputMode::Headless(_)) {
                app.insert_resource(HeadlessStreamWorker(Mutex::new(worker)));
            } else {
                // Decoded clips do not need the thread. If it cannot start,
                // streamed clips fail with `OutputUnavailable`. It ends by
                // itself once the controller and renderer are gone, so it is
                // not joined: exiting would otherwise wait on file reads.
                let _ = worker.spawn();
            }
        }
        app.insert_resource(SolouderController(Mutex::new(controller)));
    }
    app.insert_resource(AudioOutput { state, info })
        .add_systems(First, refresh_output_state);
}

fn refresh_output_state(device: Option<NonSend<DeviceOutput>>, mut output: ResMut<AudioOutput>) {
    let Some(device) = device else { return };
    let state = match device.0.state() {
        OutputState::Playing => AudioOutputState::Playing,
        OutputState::Disconnected => AudioOutputState::Disconnected,
        OutputState::Failed => AudioOutputState::Failed,
        OutputState::Panicked => AudioOutputState::Panicked,
    };
    if output.state != state {
        output.state = state;
    }
}
