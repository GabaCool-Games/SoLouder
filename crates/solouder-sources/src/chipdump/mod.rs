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
pub(crate) const MAX_FILE: u64 = 80 << 20;

/// Why a chip dump was refused.
#[derive(Debug)]
#[non_exhaustive]
pub enum ChipDumpError {
    /// Shorter than 34 bytes, or data shorter than the header says
    /// (reference patch 0085).
    Truncated,
    /// The file does not start with `CHIPTUNE`.
    NotChipTune,
    /// A chip type this source does not play.
    UnsupportedChip(u8),
    /// No chunks, a data offset past the file's end, or for `TedSid` a
    /// loop position at or past the data's end (reference patches 0085 and
    /// 0093; SoLoud read outside its data).
    BadHeader,
    /// A zx7 chunk reads past the data or writes past the ops (reference
    /// patch 0087).
    BadCompression,
    /// The loop never waits, so playing it would never return (reference
    /// patch 0086).
    NoWait,
    /// For `Ay`, a tick rate below 11 Hz or a chip clock below 352800 Hz,
    /// which its player cannot render (reference patch 0095; SoLoud divided
    /// by zero, spun, or overran its buffer), or a chip clock above 4 MHz,
    /// which no AY or YM takes (reference patch 0113; the chip's work grows
    /// with its clock).
    UnsupportedSpeed,
    /// More operations between waits than a voice can play in bounded
    /// time: for `TedSid` more than 8192 in any 1024 samples, for `Ay` more
    /// than 131072 in any two frames (reference patches 0112 and 0114;
    /// SoLoud ran them all on the audio thread, however many).
    TooManyOperations,
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
            Self::UnsupportedSpeed => {
                f.write_str("the chip dump's tick rate or chip clock is out of range")
            }
            Self::TooManyOperations => {
                f.write_str("the chip dump has too many operations between its waits")
            }
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChipDump {
    pub(crate) chip: u8,
    pub(crate) flags: u8,
    /// The data's length in bytes, at least 1.
    pub(crate) length: usize,
    /// Where `TedSid` continues after the end; `Ay` does not use it.
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
        if length == 0 {
            return Err(ChipDumpError::BadHeader);
        }
        let data = &bytes[data_offset..];
        // Stored data must be there before its room is allocated.
        if flags & 1 != 0 && data.len() < length {
            return Err(ChipDumpError::Truncated);
        }
        let mut raw = vec![0; length.div_ceil(2) * 2];
        if flags & 1 != 0 {
            raw[..length].copy_from_slice(&data[..length]);
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

    /// The most operations in any stretch of the dump before its waits
    /// reach `window`, reading from the start to the end and then, with
    /// `repeat_from`, from that position to the end again and again (as
    /// the players continue). Counts up to `limit + 1`; one pass over the
    /// operations.
    pub(crate) fn most_ops_before_waits(
        &self,
        repeat_from: Option<usize>,
        window: u64,
        limit: usize,
    ) -> usize {
        let first = self.length.div_ceil(2);
        let repeat = repeat_from.map_or(0, |from| (self.length - from).div_ceil(2));
        let op = |k: usize| match repeat_from {
            Some(from) if k >= first => self.op(from + 2 * ((k - first) % repeat)),
            _ => self.ops[k],
        };
        let wait = |op: u16| {
            if op & 0x8000 != 0 {
                u64::from(op & 0x7fff)
            } else {
                0
            }
        };
        let end = if repeat == 0 { first } else { usize::MAX };
        let (mut j, mut sum, mut most) = (0, 0_u64, 0);
        let mut i = 0;
        while i < first + repeat && most <= limit {
            while j < end && sum < window && j - i <= limit {
                sum += wait(op(j));
                j += 1;
            }
            most = most.max(j - i);
            sum -= wait(op(i));
            i += 1;
        }
        most
    }

    /// Whether the loop holds a wait of more than 0 (bit 15 set): TedSid's
    /// player reads operations until one waits.
    #[cfg(feature = "tedsid")]
    pub(crate) fn loop_waits(&self) -> bool {
        (self.loop_pos..self.length)
            .step_by(2)
            .any(|pos| self.op(pos) & 0x8000 != 0 && self.op(pos) & 0x7fff != 0)
    }
}

#[cfg(test)]
mod tests;
