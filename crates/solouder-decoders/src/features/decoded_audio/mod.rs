//! Resident audio: whole files decoded once into immutable [`Pcm`]
//! (SoLoud's `Wav`).
//!
//! [`Pcm`]: solouder_core::Pcm

pub(crate) mod convert;
mod error;
pub(crate) mod riff;
mod wav;

pub use error::{DecodeError, DecodeLimits};
pub use wav::decode_wav;

#[cfg(test)]
#[path = "wav_tests.rs"]
mod wav_tests;
