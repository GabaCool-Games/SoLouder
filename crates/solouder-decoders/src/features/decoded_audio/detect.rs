//! Choosing the decoder for a file.

use solouder_core::Pcm;

use super::{DecodeError, DecodeLimits, decode_flac, decode_wav};

/// Decodes a whole WAV or FLAC file held in `bytes`, chosen by its first
/// four bytes as SoLoud's `Wav::loadMem` does: `fLaC` is FLAC (see
/// [`decode_flac`]), anything else is tried as WAV (see [`decode_wav`]).
pub fn decode(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    if bytes.starts_with(b"fLaC") {
        decode_flac(bytes, limits)
    } else {
        decode_wav(bytes, limits)
    }
}
