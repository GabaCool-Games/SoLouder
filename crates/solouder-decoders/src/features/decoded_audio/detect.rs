//! Choosing the decoder for a file.

use solouder_core::Pcm;

use super::{DecodeError, DecodeLimits, decode_flac, decode_mp3, decode_ogg, decode_wav};

/// Decodes a whole audio file held in `bytes`, chosen by its first four
/// bytes as SoLoud's `Wav::loadMem` does: `OggS` is Ogg Vorbis (see
/// [`decode_ogg`]), `RIFF` is WAV (see [`decode_wav`]), `fLaC` is FLAC (see
/// [`decode_flac`]), and anything else is tried as MPEG audio (see
/// [`decode_mp3`]).
pub fn decode(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    match bytes.get(..4) {
        Some(b"OggS") => decode_ogg(bytes, limits),
        Some(b"RIFF") => decode_wav(bytes, limits),
        Some(b"fLaC") => decode_flac(bytes, limits),
        _ => decode_mp3(bytes, limits),
    }
}
