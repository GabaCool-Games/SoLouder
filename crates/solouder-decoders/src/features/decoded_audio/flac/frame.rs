// Ported from dr_flac 0.12.31 by David Reid (public domain or MIT No
// Attribution), as patched for SoLouder's C++ reference; altered for SoLouder.

//! FLAC frame headers (`drflac__read_next_flac_frame_header`).

use std::io::{Read, Seek};

use super::bits::{Bits, crc8};

pub(super) const LEFT_SIDE: u8 = 8;
pub(super) const RIGHT_SIDE: u8 = 9;
pub(super) const MID_SIDE: u8 = 10;

/// Bits per sample by code; 0 takes STREAMINFO's, `None` is reserved.
const SAMPLE_BITS: [Option<u32>; 8] = [
    Some(0),
    Some(8),
    Some(12),
    None,
    Some(16),
    Some(20),
    Some(24),
    None,
];

/// A FLAC frame header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Header {
    /// Variable blocking: `number` counts audio frames, not FLAC frames.
    pub variable: bool,
    pub number: u64,
    /// Audio frames in the FLAC frame.
    pub block: u32,
    pub assignment: u8,
    pub sample_bits: u32,
}

/// The channels of a channel assignment, 0 to 10.
pub(super) fn channel_count(assignment: u8) -> u32 {
    if assignment >= LEFT_SIDE {
        2
    } else {
        u32::from(assignment) + 1
    }
}

enum Parsed {
    Header(Header),
    Invalid,
}

/// Finds the next frame header, leaving the mark on its first byte. A
/// header with a reserved value is passed over, as dr_flac does, and so is
/// one whose checksum is wrong, which dr_flac as SoLoud builds it does not
/// check. `None` at the end of the data.
pub(super) fn next_header<R: Read + Seek>(bits: &mut Bits<R>, stream_bits: u32) -> Option<Header> {
    loop {
        bits.align();
        loop {
            bits.set_mark();
            if bits.read(8)? == 0xFF {
                if bits.read(6)? == 0x3E {
                    break;
                }
                bits.align();
            }
        }
        if let Parsed::Header(header) = parse(bits, stream_bits)? {
            return Some(header);
        }
    }
}

/// The header after a sync code, `Invalid` if a value is reserved or the
/// checksum is wrong.
fn parse<R: Read + Seek>(bits: &mut Bits<R>, stream_bits: u32) -> Option<Parsed> {
    if bits.read(1)? == 1 {
        return Some(Parsed::Invalid);
    }
    let variable = bits.read(1)? == 1;
    let block_code = bits.read(4)?;
    if block_code == 0 {
        return Some(Parsed::Invalid);
    }
    let rate_code = bits.read(4)?;
    let assignment = bits.read(4)? as u8;
    if assignment > MID_SIDE {
        return Some(Parsed::Invalid);
    }
    let Some(sample_bits) = SAMPLE_BITS[bits.read(3)? as usize] else {
        return Some(Parsed::Invalid);
    };
    if bits.read(1)? == 1 {
        return Some(Parsed::Invalid);
    }
    let Some(number) = coded_number(bits)? else {
        return Some(Parsed::Invalid);
    };
    let block = match block_code {
        1 => 192,
        2..=5 => 576 << (block_code - 2),
        6 => bits.read(8)? + 1,
        7 => bits.read(16)? + 1,
        _ => 256 << (block_code - 8),
    };
    // The frame's own sample rate is read past; STREAMINFO's is used.
    match rate_code {
        0..=11 => {}
        12 => {
            bits.read(8)?;
        }
        13 | 14 => {
            bits.read(16)?;
        }
        _ => return Some(Parsed::Invalid),
    }
    let expected = crc8(bits.marked());
    if bits.read(8)? != u32::from(expected) {
        return Some(Parsed::Invalid);
    }
    let sample_bits = if sample_bits == 0 {
        stream_bits
    } else {
        sample_bits
    };
    Some(Parsed::Header(Header {
        variable,
        // dr_flac keeps a FLAC frame number in 32 bits.
        number: if variable {
            number
        } else {
            u64::from(number as u32)
        },
        block,
        assignment,
        sample_bits,
    }))
}

/// A frame or sample number in FLAC's UTF-8-like coding, 1 to 7 bytes:
/// `Some(None)` if the first byte is not a valid lead byte.
pub(super) fn coded_number<R: Read + Seek>(bits: &mut Bits<R>) -> Option<Option<u64>> {
    let lead = bits.read(8)?;
    if lead & 0x80 == 0 {
        return Some(Some(u64::from(lead)));
    }
    let count = match lead {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        0xF8..=0xFB => 5,
        0xFC..=0xFD => 6,
        0xFE => 7,
        _ => return Some(None),
    };
    let mut number = u64::from(lead & (0xFF >> (count + 1)));
    for _ in 1..count {
        number = (number << 6) | u64::from(bits.read(8)? & 0x3F);
    }
    Some(Some(number))
}
