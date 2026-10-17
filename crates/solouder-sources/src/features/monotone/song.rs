// Ported from SoLoud src/audiosource/monotone/soloud_monotone.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Monotone song files (`MonotoneSong`, `Monotone::loadFile`).

use std::fmt;
use std::io::{self, Read};

/// Tracks a song may have, and hardware channels a [`super::Monotone`]
/// may mix: voices keep 12 of each.
pub const MONOTONE_CHANNELS: u32 = 12;

/// Rows of a pattern.
pub(crate) const ROWS: usize = 64;

/// The file's first bytes: a backspace, then `MONOTONE`.
const MAGIC: &[u8; 9] = b"\x08MONOTONE";

/// Why a Monotone song or its settings were refused.
#[derive(Debug)]
#[non_exhaustive]
pub enum MonotoneError {
    /// The file does not start with Monotone's magic bytes.
    NotMonotone,
    /// The file ends before its last cell (reference patch 0069; SoLoud
    /// loaded the missing bytes from earlier reads).
    Truncated,
    /// A file version other than 1, or cells of other than 2 bytes.
    Unsupported {
        /// The file's version.
        version: u8,
        /// The file's cell size.
        cell_size: u8,
    },
    /// More than [`MONOTONE_CHANNELS`] tracks (reference patch 0071; SoLoud
    /// overran its 12 channels).
    TooManyTracks(u8),
    /// Hardware channels outside 1 to [`MONOTONE_CHANNELS`], or a negative
    /// waveform (`setParams`'s `INVALID_PARAMETER`).
    InvalidParams {
        /// The hardware channels asked for.
        hardware_channels: u32,
        /// The waveform asked for.
        waveform: i32,
    },
    /// Reading failed.
    Io(io::Error),
}

impl fmt::Display for MonotoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotMonotone => f.write_str("not a Monotone song"),
            Self::Truncated => f.write_str("the Monotone song is cut short"),
            Self::Unsupported { version, cell_size } => write!(
                f,
                "Monotone version {version} with {cell_size}-byte cells is not supported (expected version 1, 2 bytes)"
            ),
            Self::TooManyTracks(tracks) => {
                write!(f, "{tracks} tracks (at most {MONOTONE_CHANNELS})")
            }
            Self::InvalidParams {
                hardware_channels,
                waveform,
            } => write!(
                f,
                "{hardware_channels} hardware channels (expected 1 to {MONOTONE_CHANNELS}) or waveform {waveform} (expected 0 or more)"
            ),
            Self::Io(error) => write!(f, "reading the Monotone song failed: {error}"),
        }
    }
}

impl std::error::Error for MonotoneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// A Monotone song (SoLoud's `MonotoneSong`): patterns of 64 rows, each row
/// a 16-bit cell per track, played in the order list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonotoneSong {
    title: Vec<u8>,
    comment: Vec<u8>,
    patterns: u8,
    tracks: u8,
    order: [u8; 256],
    cells: Vec<u16>,
}

impl Default for MonotoneSong {
    /// An empty song: no tracks, so it plays silence (reference patch
    /// 0068; SoLoud left the song uninitialised).
    fn default() -> Self {
        Self {
            title: Vec::new(),
            comment: Vec::new(),
            patterns: 0,
            tracks: 0,
            order: [0; 256],
            cells: Vec::new(),
        }
    }
}

impl MonotoneSong {
    /// The title's bytes (code page 437 in Monotone), at most 40, up to
    /// the first NUL.
    pub fn title(&self) -> &[u8] {
        &self.title
    }

    /// The comment's bytes, as for [`Self::title`].
    pub fn comment(&self) -> &[u8] {
        &self.comment
    }

    /// The number of patterns.
    pub fn patterns(&self) -> u8 {
        self.patterns
    }

    /// The number of tracks, at most [`MONOTONE_CHANNELS`].
    pub fn tracks(&self) -> u8 {
        self.tracks
    }

    /// The order list: the pattern each position plays. `0xff` ends the
    /// song, which then starts again; so does the 256th position.
    pub fn order(&self) -> &[u8; 256] {
        &self.order
    }

    /// The cell of `track` in `row` of `pattern`, 0 (an empty cell) for a
    /// pattern the song does not have (reference patch 0072).
    pub(crate) fn cell(&self, pattern: usize, row: usize, track: usize) -> u16 {
        if pattern >= usize::from(self.patterns) {
            return 0;
        }
        let tracks = usize::from(self.tracks);
        self.cells[(pattern * ROWS + row) * tracks + track]
    }

    /// Reads a song (`Monotone::loadFile`).
    pub(crate) fn read(mut reader: impl Read) -> Result<Self, MonotoneError> {
        let mut magic = [0; 9];
        read_exact(&mut reader, &mut magic)?;
        if &magic != MAGIC {
            return Err(MonotoneError::NotMonotone);
        }
        let title = pascal(&mut reader)?;
        let comment = pascal(&mut reader)?;
        let mut header = [0; 4];
        read_exact(&mut reader, &mut header)?;
        let [version, patterns, tracks, cell_size] = header;
        if version != 1 || cell_size != 2 {
            return Err(MonotoneError::Unsupported { version, cell_size });
        }
        if u32::from(tracks) > MONOTONE_CHANNELS {
            return Err(MonotoneError::TooManyTracks(tracks));
        }
        let mut order = [0; 256];
        read_exact(&mut reader, &mut order)?;
        let count = ROWS * usize::from(patterns) * usize::from(tracks);
        let mut bytes = vec![0; count * 2];
        read_exact(&mut reader, &mut bytes)?;
        let cells = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect();
        Ok(Self {
            title,
            comment,
            patterns,
            tracks,
            order,
            cells,
        })
    }
}

fn read_exact(reader: &mut impl Read, buffer: &mut [u8]) -> Result<(), MonotoneError> {
    reader
        .read_exact(buffer)
        .map_err(|error| match error.kind() {
            io::ErrorKind::UnexpectedEof => MonotoneError::Truncated,
            _ => MonotoneError::Io(error),
        })
}

/// A Pascal string of 40 bytes after its length: at most 40 bytes, up to
/// the first NUL (reference patch 0070; SoLoud took up to 255).
fn pascal(reader: &mut impl Read) -> Result<Vec<u8>, MonotoneError> {
    let mut field = [0; 41];
    read_exact(reader, &mut field)?;
    let text = &field[1..=usize::from(field[0].min(40))];
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    Ok(text[..end].to_vec())
}
