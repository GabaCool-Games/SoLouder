//! Playback: sounds, voices and their controls, and rendering of the mix.

mod config;
mod control;
mod fades;
mod filter_control;
mod groups;
mod handle;
mod mix_bus;
mod mixer;
mod queries;
mod retire;
mod sound;
mod voice;
mod voices;

pub use config::{ConfigError, MixerConfig};
pub use control::{ControlError, PlayOptions};
pub use filter_control::FilterTarget;
pub use handle::{Handle, VOICE_COUNT};
pub use mixer::Mixer;
pub use sound::{Sound, SoundId};
