// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! One voice's reader of an Ogg Vorbis stream (`WavStreamInstance` with
//! stb_vorbis, and reference patch 0058).

use std::fmt;

use solouder_core::StreamReader;

use super::kind::BoxedSource;
use crate::features::decoded_audio::vorbis::VorbisFile;

/// Plays the Vorbis frames of a file one after another. A read at or past
/// the length reads nothing, and a Vorbis packet that fails to decode ends
/// the audio, as it ends [`decode_ogg`](crate::decode_ogg).
pub(crate) struct OggReader {
    file: VorbisFile<BoxedSource>,
    /// Channels played: the file's, at most `MAX_CHANNELS`.
    channels: usize,
    /// The length from the last page.
    length: u64,
    /// Frames read (`mOffset`), or `length` once the audio ended.
    offset: u64,
    /// Frames in the last decoded Vorbis frame, and how many were read.
    frame_len: usize,
    frame_read: usize,
}

impl OggReader {
    /// A reader at the first frame of `file`, playing `channels` channels.
    pub fn new(file: VorbisFile<BoxedSource>, channels: u32, length: u64) -> Self {
        Self {
            file,
            channels: channels as usize,
            length,
            offset: 0,
            frame_len: 0,
            frame_read: 0,
        }
    }

    /// `getOggData`: copies what is left of the decoded Vorbis frame, up to
    /// `frames` frames, to `out` at `at`.
    fn copy(&mut self, out: &mut Option<(&mut [f32], usize)>, at: usize, frames: usize) -> usize {
        let n = (self.frame_len - self.frame_read).min(frames);
        if let Some((out, stride)) = out.as_mut() {
            for c in 0..self.channels {
                let from = &self.file.output(c)[self.frame_read..][..n];
                out[c * *stride + at..][..n].copy_from_slice(from);
            }
        }
        self.frame_read += n;
        self.offset += n as u64;
        n
    }

    /// The Ogg case of `WavStreamInstance::getAudio`, writing to `out` if
    /// given.
    fn take(&mut self, mut out: Option<(&mut [f32], usize)>, frames: usize) -> usize {
        if self.offset >= self.length {
            return 0;
        }
        let mut done = self.copy(&mut out, 0, frames);
        while done < frames {
            self.frame_len = self.file.get_frame_float();
            self.frame_read = 0;
            let got = self.copy(&mut out, done, frames - done);
            done += got;
            if self.offset >= self.length || got == 0 {
                self.offset = self.offset.max(self.length);
                break;
            }
        }
        done
    }

    fn forget_frame(&mut self) {
        self.frame_len = 0;
        self.frame_read = 0;
    }
}

impl fmt::Debug for OggReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OggReader")
            .field("channels", &self.channels)
            .field("length", &self.length)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl StreamReader for OggReader {
    fn read(&mut self, out: &mut [f32], stride: usize, frames: usize) -> usize {
        self.take(Some((out, stride)), frames)
    }

    /// `stb_vorbis_seek_start`, forgetting the Vorbis frame being read.
    fn rewind(&mut self) -> bool {
        let rewound = self.file.seek_start();
        self.forget_frame();
        self.offset = 0;
        rewound
    }

    /// Finds the Vorbis frame holding the target with
    /// `stb_vorbis_seek_frame` and starts inside it, landing where decoding
    /// up to it would; decodes up to it if the frame cannot be found.
    fn skip(&mut self, frames: u64) -> u64 {
        let from = self.offset;
        let target = from.saturating_add(frames);
        self.forget_frame();
        if target >= self.length {
            self.offset = self.length;
            return self.length.saturating_sub(from);
        }
        let target32 = target as u32;
        if self.file.seek_frame(target32)
            && let Some(start) = self.file.sample_offset().filter(|&start| start <= target32)
        {
            self.frame_len = self.file.get_frame_float();
            self.frame_read = ((target32 - start) as usize).min(self.frame_len);
            self.offset = target;
            return frames;
        }
        if !self.rewind() {
            self.offset = self.length;
            return 0;
        }
        let mut left = target;
        while left > 0 {
            let step = left.min(1 << 16) as usize;
            let got = self.take(None, step);
            left -= got as u64;
            if got < step {
                break;
            }
        }
        (target - left).saturating_sub(from)
    }
}
