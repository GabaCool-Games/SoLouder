//! Resident audio: whole files decoded once into immutable [`Pcm`]
//! (SoLoud's `Wav`).
//!
//! [`Pcm`]: solouder_core::Pcm

pub(crate) mod convert;
mod detect;
mod error;
pub(crate) mod flac;
pub(crate) mod riff;
mod wav;

pub use detect::decode;
pub use error::{DecodeError, DecodeLimits};
pub use flac::decode_flac;
pub use wav::decode_wav;

#[cfg(test)]
#[path = "flac_tests.rs"]
mod flac_tests;
#[cfg(test)]
#[path = "wav_tests.rs"]
mod wav_tests;
