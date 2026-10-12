//! SoLouder mixer core: a Rust port of the SoLoud audio engine.
//!
//! This crate has no Bevy or audio-device dependency. It owns the mixer and
//! exposes an imperative interface shaped after SoLoud's C API, which is where
//! behavioral parity with the C++ reference is tested. Device output and the
//! Bevy integration live in separate crates.
//!
//! Status: milestone B. Prepared PCM sounds play, stop, pause and loop with
//! volume, pan, speed and resampling, mixed offline into 1 to 8 channels.

pub mod contracts;
pub mod dsp;
pub mod features;

pub use contracts::{MAX_CHANNELS, Pcm, PcmError, SAMPLE_GRANULARITY};
pub use dsp::{Clipper, Resampler};
pub use features::playback::{
    ConfigError, ControlError, Handle, Mixer, MixerConfig, PlayOptions, Sound, VOICE_COUNT,
};
