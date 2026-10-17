// Ported from SoLoud src/core/soloud_audiosource.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Sources that compute their audio as they play: noise, chip emulators,
//! speech (SoLoud's `AudioSource` subclasses other than `Wav` and
//! `WavStream`). The core knows only the [`Generator`] and
//! [`GeneratorInstance`] traits; `solouder-sources` implements them.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use super::sound::SoundId;
use super::source::private::{Prepared, Sealed};
use super::source::{Playable, SourceSettings, source_settings_api};
use super::voice::{Voice, VoiceSource};
use crate::contracts::MAX_CHANNELS;
use crate::dsp::c_int;

/// What a generator's voice knows about the voice playing it, read at
/// every call.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct GeneratorContext {
    /// The voice loops (the instance's `LOOPING` flag); voices can change
    /// it while they play.
    pub looping: bool,
    /// The rate the voice plays at: the generator's rate times the play
    /// speed (`mSamplerate`).
    pub sample_rate: f32,
}

impl GeneratorContext {
    /// A context, for calling an instance outside a mixer (tests, tools).
    pub fn new(looping: bool, sample_rate: f32) -> Self {
        Self {
            looping,
            sample_rate,
        }
    }
}

/// A source whose audio is computed while it plays (SoLoud's
/// `AudioSource`). Each voice gets its own [`GeneratorInstance`]. An
/// `Arc<dyn Generator>` upcasts to `Arc<dyn Any + Send + Sync>`, so the
/// concrete generator behind a [`Synth`] can be recovered.
pub trait Generator: Any + Send + Sync + fmt::Debug {
    /// Channels of every instance's audio, 1 to [`MAX_CHANNELS`].
    fn channels(&self) -> u32;

    /// Frames per second the audio is computed at (`mBaseSamplerate`).
    fn sample_rate(&self) -> f32;

    /// A new instance at the start of the audio (`createInstance`). Called
    /// on the thread that plays, never on the audio thread: it may
    /// allocate.
    fn instance(&self) -> Box<dyn GeneratorInstance>;
}

/// One voice's state of a [`Generator`] (`AudioSourceInstance`). Its
/// methods run on the audio thread: they must not allocate, block or
/// panic.
pub trait GeneratorInstance: Send + fmt::Debug {
    /// Computes up to `frames` frames, channel `c` into
    /// `out[c * stride..][..frames]`, and returns how many it wrote: fewer
    /// only when the audio has ended (`getAudio`). Callers pass
    /// `frames <= stride` and an `out` holding every channel.
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        stride: usize,
        context: &GeneratorContext,
    ) -> usize;

    /// Whether the audio has ended (`hasEnded`). A voice that does not loop
    /// stops once it has played what was computed.
    fn has_ended(&self) -> bool;

    /// Goes back to the start of the audio; false (the default) if the
    /// instance cannot. Seeking back and looping need it.
    fn rewind(&mut self) -> bool {
        false
    }

    /// A value the instance reports under `key` (`getInfo`); 0 by default.
    fn info(&self, _key: u32) -> f32 {
        0.0
    }
}

/// Why a [`Synth`] cannot be made from a generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynthError {
    /// Generators have 1 to [`MAX_CHANNELS`] channels.
    UnsupportedChannelCount(u32),
    /// The sample rate is not finite and greater than 0.
    InvalidSampleRate,
}

impl fmt::Display for SynthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedChannelCount(n) => {
                write!(
                    f,
                    "unsupported channel count {n} (expected 1 to {MAX_CHANNELS})"
                )
            }
            Self::InvalidSampleRate => write!(f, "the sample rate must be finite and above 0"),
        }
    }
}

impl std::error::Error for SynthError {}

/// A generator with its playback defaults: something that can be played,
/// queued and placed in 3D like a [`Sound`](super::Sound). Cloning shares
/// the generator and keeps the [`SoundId`]. Each voice builds its own
/// instance when it is played, on the thread that plays it (through the
/// real-time [`Controller`](crate::Controller), the game thread), and
/// computes its audio on the audio thread without allocating.
#[derive(Clone, Debug)]
pub struct Synth {
    generator: Arc<dyn Generator>,
    channels: u32,
    sample_rate: f32,
    id: SoundId,
    settings: SourceSettings,
}

impl Synth {
    /// A synth at volume 1 that does not loop, with a new [`SoundId`]. The
    /// channels and rate are read once, here.
    pub fn new(generator: Arc<dyn Generator>) -> Result<Self, SynthError> {
        let channels = generator.channels();
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(SynthError::UnsupportedChannelCount(channels));
        }
        let sample_rate = generator.sample_rate();
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(SynthError::InvalidSampleRate);
        }
        Ok(Self {
            generator,
            channels,
            sample_rate,
            id: SoundId::next(),
            settings: SourceSettings::default(),
        })
    }

    /// The generator.
    pub fn generator(&self) -> &Arc<dyn Generator> {
        &self.generator
    }

    /// Channels of the audio.
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// The rate the audio is computed at.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Shared by this synth and its clones.
    pub fn id(&self) -> SoundId {
        self.id
    }

    source_settings_api!();

    /// A new instance's audio, for a voice or a queue.
    pub(crate) fn source(&self) -> VoiceSource {
        VoiceSource::Generator {
            instance: self.generator.instance(),
            channels: self.channels,
        }
    }
}

impl Playable for Synth {
    fn source_id(&self) -> SoundId {
        self.id
    }
}

impl Sealed for Synth {
    fn prepare(&self) -> Prepared {
        let voice = Voice::new(&self.settings, self.id, self.source(), self.sample_rate, 0);
        Prepared {
            voice,
            stop_others: self.settings.single_instance,
            default_volume: self.settings.volume,
        }
    }
}

/// Room to discard audio into for a seek: `buffer` holds at least
/// `scratch` floats, and each pass computes `scratch / channels` frames
/// (`mScratch` and `mScratchSize` of `AudioSourceInstance::seek`).
pub(crate) struct Discard<'a> {
    pub buffer: &'a mut [f32],
    pub scratch: usize,
}

/// `getAudio` on a generator's audio: up to `frames` frames into `out`
/// (channel `c` at `c * stride`). Returns the frames written.
pub(crate) fn read(
    source: &mut VoiceSource,
    out: &mut [f32],
    stride: usize,
    frames: u32,
    context: &GeneratorContext,
) -> u32 {
    let VoiceSource::Generator { instance, .. } = source else {
        return 0;
    };
    let wrote = instance.get_audio(out, frames as usize, stride, context);
    wrote.min(frames as usize) as u32
}

/// `hasEnded` of a generator's audio.
pub(crate) fn has_ended(source: &VoiceSource) -> bool {
    match source {
        VoiceSource::Generator { instance, .. } => instance.has_ended(),
        _ => false,
    }
}

/// `getInfo` of a generator's audio; 0 for other sources.
pub(crate) fn info(source: &VoiceSource, key: u32) -> f32 {
    match source {
        VoiceSource::Generator { instance, .. } => instance.info(key),
        _ => 0.0,
    }
}

/// SoLoud's generic seek (`AudioSourceInstance::seek`) on a generator's
/// audio, `position` seconds in: rewinds unless `seconds` is past the
/// position (false, and nothing changes, if the instance cannot), then
/// computes and discards frames at `base_rate` in passes of the scratch
/// size. The caller sets the stream position on success.
pub(crate) fn seek(
    source: &mut VoiceSource,
    position: f64,
    seconds: f64,
    base_rate: f32,
    discard: Discard<'_>,
    context: &GeneratorContext,
) -> bool {
    let VoiceSource::Generator { instance, channels } = source else {
        return false;
    };
    let channels = *channels as usize;
    let mut offset = seconds - position;
    if offset <= 0.0 {
        if !instance.rewind() {
            return false;
        }
        offset = seconds;
    }
    let pass = (discard.scratch / channels) as i32;
    // `(int)floor(...)`: INT_MIN (nothing to discard) outside the range.
    let mut remaining = c_int((f64::from(base_rate) * offset).floor());
    while remaining > 0 {
        let frames = pass.min(remaining);
        let buffer = &mut discard.buffer[..frames as usize * channels];
        instance.get_audio(buffer, frames as usize, frames as usize, context);
        remaining -= frames;
    }
    true
}

#[cfg(test)]
#[path = "generator_tests.rs"]
mod tests;
