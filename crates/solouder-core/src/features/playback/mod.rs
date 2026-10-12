//! Playback: mixer configuration and rendering of interleaved output.

mod config;
mod mixer;

pub use config::{ConfigError, MAX_CHANNELS, MixerConfig, SAMPLE_GRANULARITY};
pub use mixer::Mixer;
