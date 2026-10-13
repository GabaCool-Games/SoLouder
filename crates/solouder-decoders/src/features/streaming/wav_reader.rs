// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! One voice's reader of a WAV stream (`WavStreamInstance` with dr_wav).

use std::fmt;
use std::io::{ErrorKind, Read, Seek, SeekFrom};

use solouder_core::{SAMPLE_GRANULARITY, StreamReader};

use crate::features::decoded_audio::convert;
use crate::features::decoded_audio::riff::Layout;

/// Something a reader can read from; implemented for every `Read + Seek`
/// type that can move to the audio thread's owner.
pub(crate) trait Input: Read + Seek + Send {}

impl<T: Read + Seek + Send> Input for T {}

/// Reads the data chunk of `layout` from `input`, converting as
/// [`decode_wav`](crate::decode_wav) does.
pub(crate) struct WavReader<R> {
    input: R,
    layout: Layout,
    /// Whole frames in the stream.
    frames: u64,
    /// The next frame to read.
    position: u64,
    /// One block of encoded frames.
    buffer: Vec<u8>,
}

impl<R: Input> WavReader<R> {
    /// A reader at the first frame of `frames` frames, or `None` if
    /// `input` cannot seek there.
    pub fn new(input: R, layout: Layout, frames: u64) -> Option<Self> {
        let mut reader = Self {
            input,
            layout,
            frames,
            position: 0,
            buffer: vec![0; SAMPLE_GRANULARITY as usize * layout.frame_bytes()],
        };
        reader.rewind().then_some(reader)
    }

    fn frame_offset(&self, frame: u64) -> u64 {
        self.layout.data_start + frame * self.layout.frame_bytes() as u64
    }

    /// Reads into the buffer until `len` bytes or the end; returns the bytes.
    fn fill(&mut self, len: usize) -> usize {
        let mut filled = 0;
        while filled < len {
            match self.input.read(&mut self.buffer[filled..len]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        filled
    }
}

impl<R> fmt::Debug for WavReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WavReader")
            .field("layout", &self.layout)
            .field("frames", &self.frames)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl<R: Input> StreamReader for WavReader<R> {
    fn read(&mut self, out: &mut [f32], stride: usize, frames: usize) -> usize {
        let frame_bytes = self.layout.frame_bytes();
        let channels = self.layout.channels as usize;
        let mut done = 0;
        while done < frames && self.position < self.frames {
            let block = (frames - done)
                .min(SAMPLE_GRANULARITY as usize)
                .min((self.frames - self.position) as usize);
            let got = self.fill(block * frame_bytes);
            let whole = got / frame_bytes;
            convert::deinterleave_into(
                self.layout.encoding,
                &self.buffer[..whole * frame_bytes],
                channels,
                &mut out[done..],
                stride,
            );
            done += whole;
            self.position += whole as u64;
            if whole < block {
                // A partial frame leaves the input mid-frame; go back to the
                // frame boundary for the next read.
                if !got.is_multiple_of(frame_bytes) {
                    let at = self.frame_offset(self.position);
                    let _ = self.input.seek(SeekFrom::Start(at));
                }
                break;
            }
        }
        done
    }

    fn rewind(&mut self) -> bool {
        let at = self.frame_offset(0);
        if self.input.seek(SeekFrom::Start(at)).is_err() {
            return false;
        }
        self.position = 0;
        true
    }

    /// Jumps instead of decoding: the result is the frame decoding would
    /// reach, so seeking matches SoLoud's generic seek without its cost
    /// (#396).
    fn skip(&mut self, frames: u64) -> u64 {
        let skipped = frames.min(self.frames - self.position);
        let at = self.frame_offset(self.position + skipped);
        if self.input.seek(SeekFrom::Start(at)).is_err() {
            return 0;
        }
        self.position += skipped;
        skipped
    }
}
