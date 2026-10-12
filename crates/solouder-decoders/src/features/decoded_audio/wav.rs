// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! WAV files to [`Pcm`].

use std::io::{Cursor, ErrorKind};

use solouder_core::{MAX_CHANNELS, Pcm};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::default::codecs::PcmDecoder;
use symphonia::default::formats::WavReader;

use super::{DecodeError, DecodeLimits, convert};

/// Decodes a whole WAV file held in `bytes`.
///
/// Supports integer PCM (8, 16, 24 and 32 bits), IEEE float (32 and 64 bits),
/// A-law and mu-law, in plain or extensible headers, with 1 to
/// [`MAX_CHANNELS`] channels. Channels keep their order in the file. A data
/// chunk that ends early yields the frames that are present.
pub fn decode_wav(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(DecodeError::NotRecognized);
    }
    let source = MediaSourceStream::new(
        Box::new(Cursor::new(bytes)),
        MediaSourceStreamOptions::default(),
    );
    let mut reader = WavReader::try_new(source, FormatOptions::default())?;
    let track = reader
        .tracks()
        .first()
        .ok_or_else(|| DecodeError::Malformed("no audio track".to_owned()))?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err(DecodeError::Unsupported("not an audio track".to_owned()));
    };
    let channels = params
        .channels
        .as_ref()
        .map_or(0, |channels| channels.count());
    let channel_count = u32::try_from(channels).unwrap_or(u32::MAX);
    if channels == 0 || channel_count > MAX_CHANNELS {
        return Err(DecodeError::UnsupportedChannelCount(channel_count));
    }
    let rate = match params.sample_rate {
        Some(rate) if rate > 0 => rate,
        _ => return Err(DecodeError::InvalidSampleRate),
    };
    let frame_hint = track.num_frames.unwrap_or(0);
    let mut decoder = PcmDecoder::try_new(params, &AudioDecoderOptions::default())?;

    // Every frame takes at least one byte, so the file size bounds the
    // reservation whatever the header claims.
    let reserve = frame_hint
        .min(bytes.len() as u64)
        .min(limits.max_samples / channels as u64);
    let mut planes = Vec::new();
    for _ in 0..channels {
        let mut plane: Vec<f32> = Vec::new();
        plane
            .try_reserve(reserve as usize)
            .map_err(|_| DecodeError::OutOfMemory)?;
        planes.push(plane);
    }

    loop {
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        let buffer = decoder.decode(&packet)?;
        let samples = (planes[0].len() + buffer.frames()) as u64 * channels as u64;
        if samples > limits.max_samples {
            return Err(DecodeError::TooLarge {
                samples: frame_hint.max(samples / channels as u64) * channels as u64,
                limit: limits.max_samples,
            });
        }
        convert::append(&buffer, &mut planes)?;
    }

    let frames = planes[0].len();
    if frames == 0 {
        return Err(DecodeError::Empty);
    }
    let mut data = Vec::new();
    data.try_reserve_exact(frames * channels)
        .map_err(|_| DecodeError::OutOfMemory)?;
    for plane in planes {
        data.extend_from_slice(&plane);
    }
    Ok(Pcm::from_planar(data, channel_count, rate as f32)?)
}
