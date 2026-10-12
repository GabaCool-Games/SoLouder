//! SoLouder decoders: turn encoded audio files into [`solouder_core::Pcm`].
//!
//! Decoding happens outside the audio callback; the mixer only ever sees
//! validated, immutable PCM. Status: WAV (milestone B). Ogg Vorbis, FLAC and
//! MP3 come with streaming in milestone E.

mod features;

pub use features::decoded_audio::{DecodeError, DecodeLimits, decode_wav};
