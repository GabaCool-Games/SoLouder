// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! FLAC files to [`Pcm`] (`Wav::loadflac` with dr_flac), and the FLAC reader
//! that streams share.

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

use solouder_core::{MAX_CHANNELS, Pcm};
use symphonia::core::audio::{Audio, GenericAudioBufferRef};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::default::codecs::FlacDecoder;
use symphonia::default::formats::FlacReader;

use super::{DecodeError, DecodeLimits, convert};

/// A FLAC file being decoded one FLAC frame at a time.
///
/// Symphonia's FLAC code does unchecked integer arithmetic that overflows on
/// some damaged files. Builds without overflow checks wrap, as dr_flac does;
/// builds with them (debug builds) panic, and that panic is caught here and
/// treated as damaged data. Under `panic = "abort"` it still aborts.
pub(crate) struct FlacFile<'s> {
    reader: FlacReader<'s>,
    decoder: FlacDecoder,
    pub(crate) channels: u32,
    pub(crate) sample_rate: u32,
    /// The frame count STREAMINFO declares; 0 when it is unknown.
    pub(crate) frames: u64,
    /// The longest FLAC frame STREAMINFO allows, in audio frames.
    pub(crate) max_block: usize,
}

impl<'s> FlacFile<'s> {
    /// Reads the metadata blocks of the FLAC file in `source`.
    pub(crate) fn open(source: Box<dyn MediaSource + 's>) -> Result<Self, DecodeError> {
        let stream = MediaSourceStream::new(source, Default::default());
        let reader = guarded(|| FlacReader::try_new(stream, FormatOptions::default()))
            .ok_or_else(|| DecodeError::Malformed("damaged FLAC metadata".to_owned()))?
            .map_err(malformed)?;
        let track = reader
            .tracks()
            .first()
            .ok_or_else(|| DecodeError::Malformed("no FLAC stream".to_owned()))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or_else(|| DecodeError::Malformed("no FLAC stream".to_owned()))?
            .clone();
        let channels = params.channels.as_ref().map_or(0, |c| c.count());
        let channels = u32::try_from(channels).unwrap_or(u32::MAX);
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(DecodeError::UnsupportedChannelCount(channels));
        }
        let sample_rate = params.sample_rate.unwrap_or(0);
        if sample_rate == 0 {
            return Err(DecodeError::InvalidSampleRate);
        }
        let frames = track.num_frames.unwrap_or(0);
        let decoder = guarded(|| FlacDecoder::try_new(&params, &AudioDecoderOptions::default()))
            .ok_or_else(|| DecodeError::Malformed("damaged FLAC metadata".to_owned()))?
            .map_err(malformed)?;
        let max_block = decoder.codec_params().max_frames_per_packet.unwrap_or(0);
        let max_block = usize::try_from(max_block).unwrap_or(0).clamp(1, 65_535);
        Ok(Self {
            reader,
            decoder,
            channels,
            sample_rate,
            frames,
            max_block,
        })
    }

    /// Decodes the next FLAC frame and hands `use_frame` the number of its
    /// first audio frame and its channels, left-justified in 32 bits.
    /// Frames that fail to decode (a bad CRC among them) are skipped.
    /// Returns `false` at the end of the data, or once reading it panicked.
    pub(crate) fn next_frame(&mut self, use_frame: impl FnOnce(u64, &[&[i32]])) -> bool {
        loop {
            let packet = match guarded(|| self.reader.next_packet()) {
                Some(Ok(Some(packet))) => packet,
                _ => return false,
            };
            let Ok(first) = u64::try_from(packet.pts.get()) else {
                continue;
            };
            let decoder = &mut self.decoder;
            let Some(Ok(GenericAudioBufferRef::S32(buffer))) = guarded(|| decoder.decode(&packet))
            else {
                continue;
            };
            let mut planes: [&[i32]; MAX_CHANNELS as usize] = [&[]; MAX_CHANNELS as usize];
            let channels = self.channels as usize;
            if buffer.num_planes() != channels {
                continue;
            }
            for (c, plane) in planes.iter_mut().take(channels).enumerate() {
                *plane = buffer.plane(c).unwrap_or(&[]);
            }
            use_frame(first, &planes[..channels]);
            return true;
        }
    }
}

/// Decodes a whole FLAC file held in `bytes` (SoLoud's `Wav` with dr_flac).
///
/// 4 to 32 bits per sample, 1 to [`MAX_CHANNELS`] channels in the file's
/// order, converted as dr_flac does. The length is the frame count
/// STREAMINFO declares, which must be known; data that ends early yields the
/// frames present, and a frame that fails to decode is silent. FLAC can
/// compress silence a thousandfold, so the one allocation follows the
/// declared length, within `limits`. No input makes this panic.
pub fn decode_flac(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    let mut file = FlacFile::open(Box::new(Cursor::new(bytes)))?;
    if file.frames == 0 {
        return Err(DecodeError::Unsupported(
            "FLAC without its total frame count".to_owned(),
        ));
    }
    let channels = file.channels as usize;
    let samples = file.frames.saturating_mul(channels as u64);
    if samples > limits.max_samples {
        return Err(DecodeError::TooLarge {
            samples,
            limit: limits.max_samples,
        });
    }
    let total = usize::try_from(file.frames).map_err(|_| DecodeError::OutOfMemory)?;
    let len = usize::try_from(samples).map_err(|_| DecodeError::OutOfMemory)?;
    let mut data = Vec::new();
    data.try_reserve_exact(len)
        .map_err(|_| DecodeError::OutOfMemory)?;
    data.resize(len, 0.0);
    let mut end = 0;
    while file.next_frame(|first, planes| {
        let Some(room) = usize::try_from(first)
            .ok()
            .and_then(|f| total.checked_sub(f))
        else {
            return;
        };
        let first = total - room;
        for (c, plane) in planes.iter().enumerate() {
            let out = &mut data[c * total + first..(c + 1) * total];
            for (out, &sample) in out.iter_mut().zip(*plane) {
                *out = convert::from_i32(sample);
            }
        }
        end = end.max(first + planes[0].len().min(room));
    }) {}
    if end == 0 {
        return Err(DecodeError::Empty);
    }
    for c in 1..channels {
        data.copy_within(c * total..c * total + end, c * end);
    }
    data.truncate(channels * end);
    Ok(Pcm::from_planar(
        data,
        file.channels,
        file.sample_rate as f32,
    )?)
}

/// Runs Symphonia code that may panic on damaged data.
fn guarded<T>(run: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(run)).ok()
}

fn malformed(error: symphonia::core::errors::Error) -> DecodeError {
    DecodeError::Malformed(error.to_string())
}
