// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! WAV and FLAC files streamed from disk or memory (`WavStream` with dr_wav
//! and dr_flac).

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use solouder_core::{StreamFormat, StreamReader, StreamSource};
use symphonia::core::io::MediaSource;

use super::flac_reader::{FlacReader, Reopen};
use super::wav_reader::WavReader;
use crate::DecodeError;
use crate::features::decoded_audio::flac::FlacFile;
use crate::features::decoded_audio::riff::{self, Layout};

#[derive(Debug)]
enum Origin {
    /// Read from memory by every voice (`loadMem`, `loadToMem`).
    Memory(Arc<[u8]>),
    /// Opened again by every voice (`load`).
    File(PathBuf),
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Wav(Layout),
    Flac,
}

/// A WAV or FLAC file decoded while it plays (SoLoud's `WavStream`): wrap it
/// in a [`solouder_core::Stream`] to play it. Each voice reads its own copy
/// of the position, from memory or from the file opened again. The format
/// is chosen by the first four bytes, as SoLoud does: `fLaC` is FLAC,
/// anything else is tried as WAV.
///
/// Formats are those of [`decode_wav`](crate::decode_wav) and
/// [`decode_flac`](crate::decode_flac), with the same sample conversions.
/// A WAV stream's length is the whole frames present: a data chunk cut off
/// early ends the stream there, and seeking jumps to the frame instead of
/// decoding up to it. A FLAC stream's length is the one STREAMINFO
/// declares, and a voice also ends where the data does; with no declared
/// length, a voice ends after its first block, as in SoLoud. Seeking in a
/// FLAC stream decodes up to the target, as SoLoud's generic seek does.
#[derive(Debug)]
pub struct WavStream {
    origin: Origin,
    kind: Kind,
    format: StreamFormat,
}

impl WavStream {
    /// A stream of the file held in `bytes` (`WavStream::loadMem`). Only the
    /// header is read now.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, DecodeError> {
        let bytes: Arc<[u8]> = bytes.into();
        let (kind, format) = if bytes.starts_with(b"fLaC") {
            flac_format(FlacFile::open(Box::new(Cursor::new(Arc::clone(&bytes))))?)?
        } else {
            let layout = riff::locate(&mut Cursor::new(&bytes[..]), bytes.len() as u64)?;
            wav_format(layout)?
        };
        Ok(Self {
            origin: Origin::Memory(bytes),
            kind,
            format,
        })
    }

    /// A stream of the file at `path`, which each voice opens again
    /// (`WavStream::load`). The header is read now; if the file is gone or
    /// no longer the same format when a voice starts, that voice ends at
    /// once.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DecodeError> {
        let path = path.as_ref().to_path_buf();
        let (kind, format) = match open_file(&path).map_err(DecodeError::Io)? {
            (file, true) => flac_format(FlacFile::open(Box::new(file))?)?,
            (mut file, false) => {
                let len = file.metadata().map_err(DecodeError::Io)?.len();
                wav_format(riff::locate(&mut file, len)?)?
            }
        };
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

    fn open_flac(&self, mut reopen: Reopen) -> Option<Box<dyn StreamReader>> {
        let file = reopen()?;
        let same = (file.channels, file.sample_rate as f32)
            == (self.format.channels, self.format.sample_rate);
        same.then(|| Box::new(FlacReader::new(file, reopen)) as Box<dyn StreamReader>)
    }
}

/// The file at `path` at its start, and whether it starts with `fLaC`.
fn open_file(path: &Path) -> std::io::Result<(File, bool)> {
    let mut file = File::open(path)?;
    let mut tag = [0; 4];
    let flac = file.read_exact(&mut tag).is_ok() && &tag == b"fLaC";
    file.seek(SeekFrom::Start(0))?;
    Ok((file, flac))
}

/// SoLoud keeps the frame count in 32 bits (upstream #401).
fn frame_count(frames: u64) -> Result<u32, DecodeError> {
    u32::try_from(frames)
        .map_err(|_| DecodeError::Unsupported("more than 2^32 - 1 frames".to_owned()))
}

fn wav_format(layout: Layout) -> Result<(Kind, StreamFormat), DecodeError> {
    let format = StreamFormat {
        channels: layout.channels,
        sample_rate: layout.sample_rate as f32,
        frames: frame_count(layout.frames())?,
    };
    Ok((Kind::Wav(layout), format))
}

fn flac_format(file: FlacFile<'_>) -> Result<(Kind, StreamFormat), DecodeError> {
    let format = StreamFormat {
        channels: file.channels,
        sample_rate: file.sample_rate as f32,
        frames: frame_count(file.frames)?,
    };
    Ok((Kind::Flac, format))
}

impl StreamSource for WavStream {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn open(&self) -> Option<Box<dyn StreamReader>> {
        let frames = u64::from(self.format.frames);
        match (&self.origin, self.kind) {
            (Origin::Memory(bytes), Kind::Wav(layout)) => {
                let reader = WavReader::new(Cursor::new(Arc::clone(bytes)), layout, frames)?;
                Some(Box::new(reader))
            }
            (Origin::Memory(bytes), Kind::Flac) => {
                let bytes = Arc::clone(bytes);
                self.open_flac(Box::new(move || {
                    FlacFile::open(Box::new(Cursor::new(Arc::clone(&bytes)))).ok()
                }))
            }
            (Origin::File(path), Kind::Wav(expected)) => {
                let mut file = File::open(path).ok()?;
                let len = file.metadata().ok()?.len();
                let layout = riff::locate(&mut file, len).ok()?;
                let same = (layout.encoding, layout.channels, layout.sample_rate)
                    == (expected.encoding, expected.channels, expected.sample_rate);
                if !same {
                    return None;
                }
                let reader = WavReader::new(file, layout, frames.min(layout.frames()))?;
                Some(Box::new(reader))
            }
            (Origin::File(path), Kind::Flac) => {
                let path = path.clone();
                self.open_flac(Box::new(move || {
                    let (file, flac) = open_file(&path).ok()?;
                    let source: Box<dyn MediaSource> = Box::new(file);
                    flac.then(|| FlacFile::open(source).ok()).flatten()
                }))
            }
        }
    }
}
