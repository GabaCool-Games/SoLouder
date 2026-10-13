// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! WAV files streamed from disk or memory (`WavStream` with dr_wav).

use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use solouder_core::{StreamFormat, StreamReader, StreamSource};

use super::wav_reader::WavReader;
use crate::DecodeError;
use crate::features::decoded_audio::riff::{self, Layout};

#[derive(Debug)]
enum Origin {
    /// Read from memory by every voice (`loadMem`, `loadToMem`).
    Memory(Arc<[u8]>),
    /// Opened again by every voice (`load`).
    File(PathBuf),
}

/// A WAV file decoded while it plays (SoLoud's `WavStream` for WAV data):
/// wrap it in a [`solouder_core::Stream`] to play it. Each voice reads its
/// own copy of the position, from memory or from the file opened again.
///
/// Formats are those of [`decode_wav`](crate::decode_wav), with the same
/// sample conversions, and the stream's length is the whole frames present:
/// a data chunk cut off early ends the stream there. Seeking jumps to the
/// frame instead of decoding up to it.
#[derive(Debug)]
pub struct WavStream {
    origin: Origin,
    layout: Layout,
    frames: u32,
}

impl WavStream {
    /// A stream of the WAV file held in `bytes` (`WavStream::loadMem`).
    /// Only the header is read now.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, DecodeError> {
        let bytes: Arc<[u8]> = bytes.into();
        let layout = riff::locate(&mut Cursor::new(&bytes[..]), bytes.len() as u64)?;
        Self::with_layout(Origin::Memory(bytes), layout)
    }

    /// A stream of the WAV file at `path`, which each voice opens again
    /// (`WavStream::load`). The header is read now; if the file is gone or
    /// no longer the same format when a voice starts, that voice ends at
    /// once.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        let path = path.as_ref().to_path_buf();
        let layout = layout_of_file(&path)?;
        Self::with_layout(Origin::File(path), layout)
    }

    /// Reads the whole file at `path` into memory and streams from there
    /// (`WavStream::loadToMem`).
    pub fn load_to_memory(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        let bytes = std::fs::read(path).map_err(DecodeError::Io)?;
        Self::from_bytes(bytes)
    }

    fn with_layout(origin: Origin, layout: Layout) -> Result<Self, DecodeError> {
        // SoLoud keeps the frame count in 32 bits (upstream #401).
        let frames = u32::try_from(layout.frames())
            .map_err(|_| DecodeError::Unsupported("more than 2^32 - 1 frames".to_owned()))?;
        Ok(Self {
            origin,
            layout,
            frames,
        })
    }
}

fn layout_of_file(path: &Path) -> Result<Layout, DecodeError> {
    let mut file = File::open(path).map_err(DecodeError::Io)?;
    let len = file.metadata().map_err(DecodeError::Io)?.len();
    riff::locate(&mut file, len)
}

impl StreamSource for WavStream {
    fn format(&self) -> StreamFormat {
        StreamFormat {
            channels: self.layout.channels,
            sample_rate: self.layout.sample_rate as f32,
            frames: self.frames,
        }
    }

    fn open(&self) -> Option<Box<dyn StreamReader>> {
        let frames = u64::from(self.frames);
        match &self.origin {
            Origin::Memory(bytes) => {
                let reader = WavReader::new(Cursor::new(Arc::clone(bytes)), self.layout, frames)?;
                Some(Box::new(reader))
            }
            Origin::File(path) => {
                let mut file = File::open(path).ok()?;
                let len = file.metadata().ok()?.len();
                let layout = riff::locate(&mut file, len).ok()?;
                let same = (layout.encoding, layout.channels, layout.sample_rate)
                    == (
                        self.layout.encoding,
                        self.layout.channels,
                        self.layout.sample_rate,
                    );
                if !same {
                    return None;
                }
                let reader = WavReader::new(file, layout, frames.min(layout.frames()))?;
                Some(Box::new(reader))
            }
        }
    }
}
