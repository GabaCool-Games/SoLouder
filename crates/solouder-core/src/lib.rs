//! SoLouder mixer core: a Rust port of the SoLoud audio engine.
//!
//! This crate has no Bevy or audio-device dependency. It owns the mixer and
//! exposes an imperative interface shaped after SoLoud's C API, which is where
//! behavioral parity with the C++ reference is tested. Device output and the
//! Bevy integration live in separate crates.
//!
//! Status: milestone A. The mixer validates its configuration and renders
//! silence; audio sources arrive in milestone B.

pub mod features;

pub use features::playback::{ConfigError, MAX_CHANNELS, Mixer, MixerConfig, SAMPLE_GRANULARITY};
