// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa) and dr_mp3 0.6.31 by David Reid
// (public domain or MIT No Attribution), based on minimp3 by Lion (lieff)
// (CC0); altered for SoLouder.

//! MP3 files to [`Pcm`] (`Wav::loadmp3` with dr_mp3).
//!
//! Also the MPEG audio reader for streams. Both are a port of the decoding
//! path of dr_mp3 0.6.31 as SoLoud builds it (`DR_MP3_FLOAT_OUTPUT`, layers
//! 1, 2 and 3), bit for bit with its SSE build: minimp3 (`bits`, `header`,
//! `layer12`, `side_info`, `huffman`, `stereo`, `imdct`, `dct`, `synth`, `layer3`,
//! `decoder`) decodes one frame at a time, and dr_mp3 (`drmp3`,
//! `callbacks`) feeds it from memory or from a reader.

mod bits;
mod callbacks;
mod dct;
mod decoder;
mod drmp3;
mod header;
mod huffman;
mod huffman_tables;
mod imdct;
mod layer12;
mod layer3;
mod side_info;
mod stereo;
mod synth;
mod tables;

use std::io::{Read, Seek};

use solouder_core::{MAX_CHANNELS, Pcm};

use self::callbacks::Callbacks;
use self::drmp3::{Drmp3, Memory};
use super::{DecodeError, DecodeLimits};

/// The frames `Wav::loadmp3` reads at a time.
const BLOCK: usize = 512;

/// An MPEG audio stream read through `R` (dr_mp3 opened with `drmp3_init`
/// and SoLoud's file callbacks), for `WavStream`.
///
/// `R` must start at the first byte of the file: [`Self::rewind`] and
/// [`Self::frame_count`] seek it to byte 0. Read errors end the data, as a
/// short read does. Every read returns [`Self::channels`] samples per frame,
/// interleaved, whatever later frames hold.
pub(crate) struct Mp3File<R> {
    mp3: Drmp3<Callbacks<R>>,
}

impl<R: Read + Seek> Mp3File<R> {
    /// Opens the stream and decodes its first frame (`drmp3_init`).
    pub(crate) fn open(reader: R) -> Result<Self, DecodeError> {
        let mp3 = Drmp3::init(Callbacks::new(reader)).ok_or(DecodeError::NotRecognized)?;
        Ok(Self { mp3 })
    }

    /// The channels of the first frame: 1 or 2.
    pub(crate) fn channels(&self) -> u32 {
        self.mp3.channels
    }

    /// The sample rate of the first frame.
    pub(crate) fn sample_rate(&self) -> u32 {
        self.mp3.sample_rate
    }

    /// `drmp3_get_pcm_frame_count`: decodes the whole stream to count its
    /// frames, then goes back to the frame the stream was on (from the
    /// start, decoding up to it). 0 if the reader cannot seek.
    pub(crate) fn frame_count(&mut self) -> u64 {
        self.mp3.pcm_frame_count()
    }

    /// `drmp3_read_pcm_frames_f32`: up to `frames` frames (as many as
    /// `out_interleaved` holds) into `out_interleaved`; fewer only at the
    /// end of the stream. Returns the frames read.
    pub(crate) fn read_f32(&mut self, out_interleaved: &mut [f32], frames: usize) -> usize {
        let channels = self.mp3.channels as usize;
        let frames = frames.min(out_interleaved.len() / channels.max(1));
        self.mp3
            .read_pcm_frames(Some(out_interleaved), frames as u64) as usize
    }

    /// `drmp3_seek_to_pcm_frame(0)`: back to the first frame. False if the
    /// reader cannot seek.
    pub(crate) fn rewind(&mut self) -> bool {
        self.mp3.seek_to_pcm_frame(0)
    }
}

/// Decodes a whole MPEG audio file (layers 1, 2 and 3) held in `bytes`
/// (SoLoud's `Wav` with dr_mp3).
///
/// The length is dr_mp3's frame count, which checks every frame without
/// decoding it; when frames then fail to decode, the audio ends early and
/// the rest of the counted length is silent, as in SoLoud. No input makes
/// this panic.
pub fn decode_mp3(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    if bytes.is_empty() {
        return Err(DecodeError::NotRecognized);
    }
    let mut mp3 = Drmp3::init(Memory::new(bytes)).ok_or(DecodeError::NotRecognized)?;
    let total = mp3.pcm_frame_count();
    let channels = mp3.channels;
    // validDecodedLayout
    if total == 0 {
        return Err(DecodeError::Empty);
    }
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(DecodeError::UnsupportedChannelCount(channels));
    }
    if mp3.sample_rate == 0 {
        return Err(DecodeError::InvalidSampleRate);
    }
    let samples = total.saturating_mul(u64::from(channels));
    if total > u64::from(u32::MAX) / u64::from(channels) {
        return Err(DecodeError::TooLarge {
            samples,
            limit: u64::from(u32::MAX),
        });
    }
    if samples > limits.max_samples {
        return Err(DecodeError::TooLarge {
            samples,
            limit: limits.max_samples,
        });
    }
    let total = usize::try_from(total).map_err(|_| DecodeError::OutOfMemory)?;
    let len = usize::try_from(samples).map_err(|_| DecodeError::OutOfMemory)?;
    let mut data = Vec::new();
    data.try_reserve_exact(len)
        .map_err(|_| DecodeError::OutOfMemory)?;
    data.resize(len, 0.0);
    mp3.seek_to_pcm_frame(0);

    let channels = channels as usize;
    let mut block = [0.0_f32; BLOCK * MAX_CHANNELS as usize];
    for first in (0..total).step_by(BLOCK) {
        let frames = (total - first).min(BLOCK);
        let read = mp3.read_pcm_frames(Some(&mut block), frames as u64) as usize;
        // clearUnreadFrames
        block[read * channels..frames * channels].fill(0.0);
        for (c, plane) in data.chunks_exact_mut(total).enumerate() {
            let frames_in = block.chunks_exact(channels);
            for (dst, frame) in plane[first..first + frames].iter_mut().zip(frames_in) {
                *dst = frame[c];
            }
        }
    }
    Ok(Pcm::from_planar(
        data,
        channels as u32,
        mp3.sample_rate as f32,
    )?)
}

#[cfg(test)]
#[path = "unit_tests.rs"]
mod unit_tests;
