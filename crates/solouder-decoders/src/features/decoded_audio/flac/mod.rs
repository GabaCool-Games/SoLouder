// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa) and dr_flac 0.12.31 by David Reid
// (public domain or MIT No Attribution); altered for SoLouder.

//! FLAC files to [`Pcm`] (`Wav::loadflac` with dr_flac), and the FLAC reader
//! that streams share: a port of the decoding path of dr_flac 0.12.31 as
//! SoLoud builds it, with reference patch 0057.

mod bits;
mod decorrelate;
mod frame;
mod metadata;
mod subframe;

use std::io::{Cursor, Read, Seek};

use solouder_core::{MAX_CHANNELS, Pcm};

use self::bits::{Bits, crc16};
use self::frame::Header;
use super::{DecodeError, DecodeLimits, convert};

/// A FLAC file being decoded one FLAC frame at a time, on one timeline:
/// each FLAC frame is placed by its number, a frame that starts before the
/// end of the previous one gives only the audio after it, and frames that
/// fail to decode or fail their checksums leave gaps.
pub(crate) struct FlacFile<R> {
    bits: Bits<R>,
    pub(crate) channels: u32,
    pub(crate) sample_rate: u32,
    /// The frame count STREAMINFO declares; 0 when it is unknown.
    pub(crate) frames: u64,
    /// The longest FLAC frame STREAMINFO allows, in audio frames, at least 1.
    pub(crate) max_block: usize,
    /// STREAMINFO's maximum block size, which frames may not exceed.
    block_limit: u32,
    /// Audio frames per FLAC frame when FLAC frames are numbered, as
    /// STREAMINFO gives it: 0 unless its minimum and maximum agree.
    fixed_block: u64,
    sample_bits: u32,
    first_frame: u64,
    /// The FLAC frame decoded last, planar with a stride of `max_block`.
    samples: Vec<i32>,
    /// The frame after the last one decoded.
    next: u64,
}

impl<R: Read + Seek> FlacFile<R> {
    /// Reads the metadata blocks of the FLAC file in `source`.
    pub(crate) fn open(mut source: R) -> Result<Self, DecodeError> {
        let info = metadata::read(&mut source)?;
        if info.channels > MAX_CHANNELS {
            return Err(DecodeError::UnsupportedChannelCount(info.channels));
        }
        if info.sample_rate == 0 {
            return Err(DecodeError::InvalidSampleRate);
        }
        let max_block = (info.max_block as usize).max(1);
        Ok(Self {
            bits: Bits::new(source),
            channels: info.channels,
            sample_rate: info.sample_rate,
            frames: info.frames,
            max_block,
            block_limit: info.max_block,
            fixed_block: if info.min_block == info.max_block {
                u64::from(info.max_block)
            } else {
                0
            },
            sample_bits: info.sample_bits,
            first_frame: info.first_frame,
            samples: vec![0; info.channels as usize * max_block],
            next: 0,
        })
    }

    /// Goes back to the first frame (dr_flac's seek to frame 0).
    pub(crate) fn rewind(&mut self) -> bool {
        self.next = 0;
        self.bits.seek_to(self.first_frame)
    }

    /// Decodes the next FLAC frame and hands `use_frame` the frames missing
    /// before it (a gap left by frames that failed to decode, a bad
    /// checksum among them) and its channels, left-justified in 32 bits.
    /// Returns `false` at the end of the data.
    pub(crate) fn next_frame(&mut self, use_frame: impl FnOnce(u64, &[&[i32]])) -> bool {
        loop {
            let Some(header) = frame::next_header(&mut self.bits, self.sample_bits) else {
                return false;
            };
            if self.decode(&header).is_none() {
                // Search again from just after this frame's sync code.
                self.bits.back_to_mark(2);
                continue;
            }
            let len = u64::from(header.block);
            let first = if header.variable {
                header.number
            } else if self.fixed_block > 0 {
                header.number.saturating_mul(self.fixed_block)
            } else {
                header.number.saturating_mul(len)
            };
            let overlap = self.next.saturating_sub(first);
            if overlap >= len {
                continue;
            }
            let stride = self.max_block;
            let block = header.block as usize;
            let mut planes: [&[i32]; MAX_CHANNELS as usize] = [&[]; MAX_CHANNELS as usize];
            for (c, plane) in planes.iter_mut().take(self.channels as usize).enumerate() {
                *plane = &self.samples[c * stride + overlap as usize..c * stride + block];
            }
            let gap = first.saturating_sub(self.next);
            self.next = first.saturating_add(len);
            use_frame(gap, &planes[..self.channels as usize]);
            return true;
        }
    }

    /// Decodes the frame after `header` into `samples`; `None` if it is
    /// invalid, fails its checksum or is cut off.
    fn decode(&mut self, header: &Header) -> Option<()> {
        if header.block > self.block_limit
            || frame::channel_count(header.assignment) != self.channels
        {
            return None;
        }
        let (stride, block) = (self.max_block, header.block as usize);
        let mut wasted = [0; MAX_CHANNELS as usize];
        let channels = self.channels as usize;
        for (c, wasted) in wasted.iter_mut().take(channels).enumerate() {
            let out = &mut self.samples[c * stride..c * stride + block];
            *wasted = subframe::decode(&mut self.bits, header, c, out)?;
        }
        self.bits.align();
        let expected = crc16(self.bits.marked());
        if self.bits.read(16)? != u32::from(expected) {
            return None;
        }
        decorrelate::justify(
            header.assignment,
            &mut self.samples,
            stride,
            block,
            &wasted[..channels],
            32 - self.sample_bits,
        );
        Some(())
    }
}

/// Decodes a whole FLAC file held in `bytes` (SoLoud's `Wav` with dr_flac).
///
/// 1 to 32 bits per sample, 1 to [`MAX_CHANNELS`] channels in the file's
/// order, converted as dr_flac does. The length is the frame count
/// STREAMINFO declares, which must be known; data that ends early yields the
/// frames present, and a frame that fails to decode or fails its checksum
/// is silent. FLAC can compress silence a thousandfold, so the one
/// allocation follows the declared length, within `limits`. No input makes
/// this panic.
pub fn decode_flac(bytes: &[u8], limits: &DecodeLimits) -> Result<Pcm, DecodeError> {
    let mut file = FlacFile::open(Cursor::new(bytes))?;
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
    let mut end = 0_usize;
    while file.next_frame(|gap, planes| {
        let first = usize::try_from(gap).map_or(total, |gap| end.saturating_add(gap).min(total));
        let count = planes[0].len().min(total - first);
        for (c, plane) in planes.iter().enumerate() {
            let out = &mut data[c * total + first..][..count];
            for (out, &sample) in out.iter_mut().zip(*plane) {
                *out = convert::from_i32(sample);
            }
        }
        if count > 0 {
            end = first + count;
        }
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

#[cfg(test)]
#[path = "unit_tests.rs"]
mod unit_tests;
