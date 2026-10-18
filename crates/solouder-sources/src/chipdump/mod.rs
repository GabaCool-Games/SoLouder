// Ported from SoLoud src/audiosource/tedsid/soloud_tedsid.cpp and
// src/audiosource/ay/soloud_ay.cpp (zlib/libpng, Copyright (c) 2015-2020
// Jari Komppa); altered for SoLouder.

//! Chip register dumps (`CHIPTUNE` files), shared by `TedSid` and `Ay`.
//!
//! A 28-byte header: `CHIPTUNE`, the data's offset (16 bits), the chip
//! type and flags (a byte each), the data's length as a count of
//! 1024-byte chunks and the last chunk's length, the loop position as a
//! chunk and an offset (16 bits each), then the CPU and chip speeds (32
//! bits each). The data is 16-bit operations, one zx7 stream a chunk
//! unless flag bit 0 says it is stored as it is.

mod zx7;

use std::fmt;
use std::io::{self, Read};

/// SoLoud reads files of at least this many bytes.
const MIN_FILE: usize = 34;

/// The most bytes a dump is read up to: the longest data, compressed with
/// zx7's worst overhead, after the longest data offset.
const MAX_FILE: u64 = 80 << 20;

/// Why a chip dump was refused.
#[derive(Debug)]
#[non_exhaustive]
pub enum ChipDumpError {
    /// Shorter than 34 bytes, or data shorter than the header says
    /// (reference patch 0078).
    Truncated,
    /// The file does not start with `CHIPTUNE`.
    NotChipTune,
    /// A chip type this source does not play.
    UnsupportedChip(u8),
    /// No chunks, a loop position at or past the data's end, or a data
    /// offset past the file's end (reference patch 0078; SoLoud read
    /// outside its data).
    BadHeader,
    /// A zx7 chunk reads past the data or writes past the ops (reference
    /// patch 0080).
    BadCompression,
    /// The loop never waits, so playing it would never return (reference
    /// patch 0079).
    NoWait,
    /// Longer than 80 MiB.
    TooLarge,
    /// Reading failed.
    Io(io::Error),
}

impl fmt::Display for ChipDumpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the chip dump is cut short"),
            Self::NotChipTune => f.write_str("not a CHIPTUNE dump"),
            Self::UnsupportedChip(chip) => write!(f, "chip type {chip} is not supported here"),
            Self::BadHeader => {
                f.write_str("the chip dump's sizes or loop position are out of range")
            }
            Self::BadCompression => f.write_str("a compressed chunk of the chip dump is damaged"),
            Self::NoWait => f.write_str("the chip dump's loop never waits"),
            Self::TooLarge => f.write_str("the chip dump is longer than 80 MiB"),
            Self::Io(error) => write!(f, "reading the chip dump failed: {error}"),
        }
    }
}

impl std::error::Error for ChipDumpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// A loaded dump.
// `chip`, `cpu_speed` and `chip_speed` are for Ay (milestone F unit 5b).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChipDump {
    pub(crate) chip: u8,
    pub(crate) flags: u8,
    /// The data's length in bytes, at least 1.
    pub(crate) length: usize,
    /// Where playing continues after the end, below `length`.
    pub(crate) loop_pos: usize,
    pub(crate) cpu_speed: u32,
    pub(crate) chip_speed: u32,
    /// The operations, `(length + 1) / 2` of them; an odd last byte makes
    /// one with a high byte of 0.
    pub(crate) ops: Vec<u16>,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

impl ChipDump {
    /// Reads all of `reader` (up to 80 MiB) and parses it.
    pub(crate) fn read(reader: impl Read, chips: &[u8]) -> Result<Self, ChipDumpError> {
        let mut bytes = Vec::new();
        reader
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(ChipDumpError::Io)?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(ChipDumpError::TooLarge);
        }
        Self::parse(&bytes, chips)
    }

    /// Parses a dump of one of `chips` (`TedSid::loadFile`, `Ay::loadFile`).
    pub(crate) fn parse(bytes: &[u8], chips: &[u8]) -> Result<Self, ChipDumpError> {
        if bytes.len() < MIN_FILE {
            return Err(ChipDumpError::Truncated);
        }
        if &bytes[..8] != b"CHIPTUNE" {
            return Err(ChipDumpError::NotChipTune);
        }
        let data_offset = usize::from(u16_at(bytes, 8));
        let chip = bytes[10];
        if !chips.contains(&chip) {
            return Err(ChipDumpError::UnsupportedChip(chip));
        }
        let flags = bytes[11];
        let chunks = usize::from(u16_at(bytes, 12));
        let last_chunk = usize::from(u16_at(bytes, 14));
        let loop_pos = usize::from(u16_at(bytes, 16)) * 1024 + usize::from(u16_at(bytes, 18));
        if chunks == 0 || data_offset > bytes.len() {
            return Err(ChipDumpError::BadHeader);
        }
        let length = (chunks - 1) * 1024 + last_chunk;
        if length == 0 || loop_pos >= length {
            return Err(ChipDumpError::BadHeader);
        }
        let data = &bytes[data_offset..];
        let mut raw = vec![0; length.div_ceil(2) * 2];
        if flags & 1 != 0 {
            let stored = data.get(..length).ok_or(ChipDumpError::Truncated)?;
            raw[..length].copy_from_slice(stored);
        } else {
            let mut used = 0;
            for chunk in 0..chunks {
                used += zx7::decompress(&data[used..], &mut raw, chunk * 1024)
                    .ok_or(ChipDumpError::BadCompression)?;
            }
        }
        let ops = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect();
        Ok(Self {
            chip,
            flags,
            length,
            loop_pos,
            cpu_speed: u32_at(bytes, 20),
            chip_speed: u32_at(bytes, 24),
            ops,
        })
    }

    /// The operation at byte position `pos` (SoLoud reads `mOps[pos / 2]`,
    /// so an odd loop position reads the operations from the byte before).
    pub(crate) fn op(&self, pos: usize) -> u16 {
        self.ops[pos / 2]
    }

    /// Whether the loop holds a wait of more than 0 (bit 15 set): TedSid's
    /// player reads operations until one waits.
    pub(crate) fn loop_waits(&self) -> bool {
        (self.loop_pos..self.length)
            .step_by(2)
            .any(|pos| self.op(pos) & 0x8000 != 0 && self.op(pos) & 0x7fff != 0)
    }
}

#[cfg(test)]
mod tests;
