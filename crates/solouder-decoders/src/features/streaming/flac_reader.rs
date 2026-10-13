// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! One voice's reader of a FLAC stream (`WavStreamInstance` with dr_flac).

use std::fmt;

use solouder_core::StreamReader;

use crate::features::decoded_audio::convert;
use crate::features::decoded_audio::flac::FlacFile;

/// Opens the voice's file again at its start: its bytes in memory, or the
/// file it opened, through a second handle.
pub(crate) type Reopen = Box<dyn FnMut() -> Option<FlacFile<'static>> + Send>;

/// Decodes a FLAC file a FLAC frame at a time, converting as
/// [`decode_flac`](crate::decode_flac) does. Reads continue past the length
/// STREAMINFO declares, as dr_flac's do; the voice ends at that length.
///
/// Rewinding reads the file again from its start, and skipping decodes and
/// discards, as SoLoud's generic seek does: Symphonia's FLAC seek panics on
/// some damaged seek tables. Frames that failed to decode are silent, up to
/// the declared length (or one FLAC frame when no length is declared).
pub(crate) struct FlacReader {
    file: FlacFile<'static>,
    reopen: Reopen,
    channels: usize,
    /// The decoded FLAC frame, planar with a stride of `stride`.
    block: Vec<f32>,
    /// The longest FLAC frame STREAMINFO allows.
    stride: usize,
    /// Frames in `block`, and how many of them were read.
    block_len: usize,
    block_read: usize,
    /// Silent frames owed before `block`, for FLAC frames that failed to
    /// decode.
    silence: u64,
    /// The frame following `block`.
    next: u64,
}

impl FlacReader {
    /// A reader at the first frame of `file`, which `reopen` opens again.
    pub fn new(file: FlacFile<'static>, reopen: Reopen) -> Self {
        let channels = file.channels as usize;
        let stride = file.max_block;
        Self {
            file,
            reopen,
            channels,
            block: vec![0.0; channels * stride],
            stride,
            block_len: 0,
            block_read: 0,
            silence: 0,
            next: 0,
        }
    }

    /// Decodes the next FLAC frame into `block`; false at the end.
    fn refill(&mut self) -> bool {
        let (block, stride) = (&mut self.block, self.stride);
        let mut decoded = None;
        let more = self.file.next_frame(|gap, planes| {
            let len = planes[0].len().min(stride);
            for (c, plane) in planes.iter().enumerate() {
                for (out, &sample) in block[c * stride..][..len].iter_mut().zip(*plane) {
                    *out = convert::from_i32(sample);
                }
            }
            decoded = Some((gap, len));
        });
        let Some((gap, len)) = decoded.filter(|_| more) else {
            return false;
        };
        let room = match self.file.frames {
            0 => stride as u64,
            frames => frames.saturating_sub(self.next),
        };
        self.silence = gap.min(room);
        self.block_len = len;
        self.block_read = 0;
        self.next += self.silence + len as u64;
        true
    }

    /// Takes up to `frames` frames, writing them to `out` if given.
    fn take(&mut self, mut out: Option<(&mut [f32], usize)>, frames: usize) -> usize {
        let mut done = 0;
        while done < frames {
            if self.silence == 0 && self.block_read == self.block_len && !self.refill() {
                break;
            }
            let silent = self.silence > 0;
            let left = if silent {
                usize::try_from(self.silence).unwrap_or(usize::MAX)
            } else {
                self.block_len - self.block_read
            };
            let n = left.min(frames - done);
            if let Some((out, stride)) = out.as_mut() {
                for c in 0..self.channels {
                    let to = &mut out[c * *stride + done..][..n];
                    if silent {
                        to.fill(0.0);
                    } else {
                        to.copy_from_slice(&self.block[c * self.stride + self.block_read..][..n]);
                    }
                }
            }
            if silent {
                self.silence -= n as u64;
            } else {
                self.block_read += n;
            }
            done += n;
        }
        done
    }

    /// The frame the next read starts at.
    fn position(&self) -> u64 {
        self.next - (self.block_len - self.block_read) as u64 - self.silence
    }
}

impl fmt::Debug for FlacReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlacReader")
            .field("channels", &self.channels)
            .field("position", &self.position())
            .finish_non_exhaustive()
    }
}

impl StreamReader for FlacReader {
    fn read(&mut self, out: &mut [f32], stride: usize, frames: usize) -> usize {
        self.take(Some((out, stride)), frames)
    }

    /// Fails if the file cannot be read again or no longer has the same
    /// layout.
    fn rewind(&mut self) -> bool {
        let Some(file) = (self.reopen)() else {
            return false;
        };
        let layout = |f: &FlacFile<'_>| (f.channels, f.sample_rate, f.max_block);
        if layout(&file) != layout(&self.file) {
            return false;
        }
        self.file = file;
        self.block_len = 0;
        self.block_read = 0;
        self.silence = 0;
        self.next = 0;
        true
    }

    fn skip(&mut self, frames: u64) -> u64 {
        let mut skipped = 0;
        while skipped < frames {
            let step = (frames - skipped).min(self.stride as u64) as usize;
            let got = self.take(None, step);
            skipped += got as u64;
            if got < step {
                break;
            }
        }
        skipped
    }
}
