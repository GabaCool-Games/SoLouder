//! Resident audio: whole files decoded once into immutable [`Pcm`]
//! (SoLoud's `Wav`).
//!
//! [`Pcm`]: solouder_core::Pcm

mod convert;
mod error;
mod wav;

pub use error::{DecodeError, DecodeLimits};
pub use wav::decode_wav;

#[cfg(test)]
#[path = "wav_tests.rs"]
mod wav_tests;
