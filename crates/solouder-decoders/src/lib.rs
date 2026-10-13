//! SoLouder decoders: turn encoded audio files into [`solouder_core::Pcm`],
//! or stream them while they play.
//!
//! Decoding happens outside the audio callback; the mixer only ever sees
//! validated, immutable PCM or a [`solouder_core::StreamReader`]. Status: WAV
//! (milestone B) and streamed WAV (milestone E). Ogg Vorbis, FLAC and MP3
//! come later in milestone E.

mod features;

pub use features::decoded_audio::{DecodeError, DecodeLimits, decode_wav};
pub use features::streaming::WavStream;
