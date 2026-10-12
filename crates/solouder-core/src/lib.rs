//! SoLouder mixer core: a Rust port of the SoLoud audio engine.
//!
//! This crate has no Bevy or audio-device dependency. It owns the mixer and
//! exposes an imperative interface shaped after SoLoud's C API, which is where
//! behavioral parity with the C++ reference is tested. Device output and the
//! Bevy integration live in separate crates.
//!
//! Status: milestone D in progress. Prepared PCM sounds play, stop, pause,
//! seek and loop with volume, pan, speed, fades and resampling, alone or in
//! voice groups, mixed into 1 to 8 channels, either offline through
//! [`Mixer`] or from an audio callback through the [`realtime`]
//! controller/renderer pair.

pub mod contracts;
pub mod dsp;
pub mod features;

pub use contracts::{MAX_CHANNELS, Pcm, PcmError, SAMPLE_GRANULARITY};
pub use dsp::{Clipper, Resampler};
pub use features::playback::{
    ConfigError, ControlError, Handle, Mixer, MixerConfig, PlayOptions, Sound, SoundId, VOICE_COUNT,
};

/// The SoLoud release whose behavior this crate reproduces
/// (`SOLOUD_VERSION`, returned by `Soloud::getVersion`).
pub const SOLOUD_VERSION: u32 = 202002;
pub use features::realtime;
pub use features::realtime::{
    Controller, ControllerError, Event, FinishReason, RealtimeConfig, Renderer, Status, VoiceId,
};
