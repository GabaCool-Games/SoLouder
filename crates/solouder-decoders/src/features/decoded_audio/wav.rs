// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! WAV files to [`Pcm`].

use solouder_core::Pcm;

use super::{DecodeError, DecodeLimits, convert, riff};

/// Decodes a whole WAV file held in `bytes`.
///
/// Supports integer PCM (8, 16, 24 and 32 bits), IEEE float (32 and 64 bits),
/// A-law and mu-law, in plain or extensible headers, with 1 to
/// [`MAX_CHANNELS`](solouder_core::MAX_CHANNELS) channels. Channels keep their order in the file. A data
/// chunk that ends early yields the whole frames that are present.
///
/// The header is validated before anything is allocated, and the one
/// allocation is sized from the bytes actually present, never from what the
/// header claims. No input makes this panic.
pub fn decode_wav(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    let wav = riff::parse(bytes)?;
    let channels = wav.channels as usize;
    let frames = wav.data.len() / (channels * wav.encoding.width());
    if frames == 0 {
        return Err(DecodeError::Empty);
    }
    let samples = frames as u64 * channels as u64;
    if samples > limits.max_samples {
        return Err(DecodeError::TooLarge {
            samples,
            limit: limits.max_samples,
        });
    }
    let len = usize::try_from(samples).map_err(|_| DecodeError::OutOfMemory)?;
    let mut data = Vec::new();
    data.try_reserve_exact(len)
        .map_err(|_| DecodeError::OutOfMemory)?;
    data.resize(len, 0.0);
    convert::deinterleave(wav.encoding, wav.data, channels, &mut data);
    Ok(Pcm::from_planar(
        data,
        wav.channels,
        wav.sample_rate as f32,
    )?)
}
