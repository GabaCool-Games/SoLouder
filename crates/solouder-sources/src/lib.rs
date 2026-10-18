//! SoLouder sources: ports of SoLoud's generators for `solouder-core`.
//!
//! Each source implements [`solouder_core::Generator`]; wrap it in a
//! [`solouder_core::Synth`] to play, queue or place it like any sound.
//! Voices build their instance on the thread that plays them and compute
//! their audio on the audio thread without allocating. Each source has a
//! Cargo feature of its own.

#[cfg(feature = "chipdump")]
mod chipdump;
mod features;
#[cfg(feature = "prg")]
mod prg;

#[cfg(feature = "chipdump")]
pub use chipdump::ChipDumpError;
#[cfg(feature = "ay")]
pub use features::ay::Ay;
#[cfg(feature = "monotone")]
pub use features::monotone::{MONOTONE_CHANNELS, Monotone, MonotoneError, MonotoneSong};
#[cfg(feature = "noise")]
pub use features::noise::{Noise, NoiseType, OCTAVES};
#[cfg(feature = "sfxr")]
pub use features::sfxr::{Sfxr, SfxrError, SfxrParams, SfxrPreset};
#[cfg(feature = "tedsid")]
pub use features::tedsid::{SidModel, TedSid};
#[cfg(feature = "vic")]
pub use features::vic::{RegisterOutOfRange, VIC_REGISTERS, Vic, VicModel};
#[cfg(feature = "prg")]
pub use prg::Prg;
