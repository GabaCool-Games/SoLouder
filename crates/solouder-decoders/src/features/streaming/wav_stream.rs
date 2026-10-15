// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Audio files streamed from disk or memory (`WavStream` with dr_wav,
//! dr_flac, stb_vorbis and dr_mp3).

use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use solouder_core::{StreamFormat, StreamReader, StreamSource};

use super::kind::{self, BoxedSource, Kind};
use crate::DecodeError;
use crate::features::decoded_audio::vorbis::FileKind;

#[derive(Debug)]
enum Origin {
    /// Read from memory by every voice (`loadMem`, `loadToMem`).
    Memory(Arc<[u8]>),
    /// Opened again by every voice (`load`).
    File(PathBuf),
}

/// An audio file decoded while it plays (SoLoud's `WavStream`): wrap it in
/// a [`solouder_core::Stream`] to play it. Each voice reads its own copy of
/// the position, from memory or from the file opened again. The format is
/// chosen by the first four bytes, as SoLoud does: `OggS` is Ogg Vorbis,
/// `RIFF` is WAV, `fLaC` is FLAC, and anything else is tried as MPEG audio.
///
/// Formats are those of [`decode`](crate::decode), with the same samples.
/// A WAV stream's length is the whole frames present: a data chunk cut off
/// early ends the stream there, and seeking jumps to the frame instead of
/// decoding up to it. A FLAC stream's length is the one STREAMINFO
/// declares, and a voice also ends where the data does; with no declared
/// length, a voice ends at once, as in SoLoud. An Ogg Vorbis stream's
/// length is the last page's sample position (a file whose length cannot
/// be found does not load), a Vorbis packet that fails to decode ends the
/// voice, and seeking finds the target's Vorbis frame instead of decoding
/// up to it (reference patch 0058). An MPEG stream's length is found by
/// decoding the whole file once, when it loads. Seeking in FLAC and MPEG
/// streams decodes up to the target, as SoLoud's generic seek does. A voice
/// keeps the file it opened, so a looping voice plays on when the file is
/// deleted.
#[derive(Debug)]
pub struct WavStream {
    origin: Origin,
    kind: Kind,
    format: StreamFormat,
}

impl WavStream {
    /// A stream of the file held in `bytes` (`WavStream::loadMem`). Only the
    /// header is read now, except for MPEG audio, which is decoded once to
    /// find its length.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, DecodeError> {
        let bytes: Arc<[u8]> = bytes.into();
        let (kind, format) =
            kind::probe(Box::new(Cursor::new(Arc::clone(&bytes))), FileKind::Memory)?;
        Ok(Self {
            origin: Origin::Memory(bytes),
            kind,
            format,
        })
    }

    /// A stream of the file at `path`, which each voice opens again
    /// (`WavStream::load`). The header is read now (MPEG audio is decoded
    /// once to find its length); if the file is gone or no longer the same
    /// format when a voice starts, that voice ends at once.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(DecodeError::Io)?;
        let (kind, format) = kind::probe(Box::new(file), FileKind::Disk)?;
        Ok(Self {
            origin: Origin::File(path),
            kind,
            format,
        })
    }

    /// Reads the whole file at `path` into memory and streams from there
    /// (`WavStream::loadToMem`).
    pub fn load_to_memory(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        let bytes = std::fs::read(path).map_err(DecodeError::Io)?;
        Self::from_bytes(bytes)
    }
}

impl StreamSource for WavStream {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn open(&self) -> Option<Box<dyn StreamReader>> {
        let (input, file): (BoxedSource, _) = match &self.origin {
            Origin::Memory(bytes) => (Box::new(Cursor::new(Arc::clone(bytes))), FileKind::Memory),
            Origin::File(path) => (Box::new(File::open(path).ok()?), FileKind::Disk),
        };
        kind::open(self.kind, self.format, input, file)
    }
}
