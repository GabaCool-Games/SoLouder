// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! One voice's reader of an MPEG audio stream (`WavStreamInstance` with
//! dr_mp3).

use std::fmt;

use solouder_core::{SAMPLE_GRANULARITY, StreamReader};

use super::kind::BoxedSource;
use crate::features::decoded_audio::mp3::Mp3File;

const BLOCK: usize = SAMPLE_GRANULARITY as usize;

/// Decodes MPEG audio frames in blocks of 512 frames, as SoLoud does. A
/// short read is the end of the audio. Seeking decodes up to the target, as
/// SoLoud's generic seek does.
pub(crate) struct Mp3Reader {
    file: Mp3File<BoxedSource>,
    channels: usize,
    /// One block of interleaved frames.
    block: Vec<f32>,
}

impl Mp3Reader {
    /// A reader at the first frame of `file`.
    pub fn new(file: Mp3File<BoxedSource>) -> Self {
        let channels = file.channels() as usize;
        Self {
            file,
            channels,
            block: vec![0.0; BLOCK * channels],
        }
    }
}

impl fmt::Debug for Mp3Reader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mp3Reader")
            .field("channels", &self.channels)
            .finish_non_exhaustive()
    }
}

impl StreamReader for Mp3Reader {
    fn read(&mut self, out: &mut [f32], stride: usize, frames: usize) -> usize {
        let channels = self.channels;
        let mut done = 0;
        while done < frames {
            let want = (frames - done).min(BLOCK);
            let got = self
                .file
                .read_f32(&mut self.block[..want * channels], want)
                .min(want);
            for (j, frame) in self.block[..got * channels]
                .chunks_exact(channels)
                .enumerate()
            {
                for (c, &sample) in frame.iter().enumerate() {
                    out[c * stride + done + j] = sample;
                }
            }
            done += got;
            if got < want {
                break;
            }
        }
        done
    }

    /// `drmp3_seek_to_pcm_frame(0)`.
    fn rewind(&mut self) -> bool {
        self.file.rewind()
    }
}
