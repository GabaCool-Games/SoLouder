// Ported from dr_flac 0.12.31 by David Reid (public domain or MIT No
// Attribution), as patched for SoLouder's C++ reference; altered for SoLouder.

//! The big-endian bit reader under the FLAC decoder (dr_flac's `drflac_bs`),
//! and the CRCs of FLAC frames.

use std::io::{ErrorKind, Read, Seek, SeekFrom};

/// Bytes read from the source at a time.
const CHUNK: usize = 64 * 1024;

/// The most a frame may read: far more than any FLAC frame needs, so that a
/// damaged frame cannot make the reader hold the rest of a long file.
pub(super) const FRAME_LIMIT: usize = 16 * 1024 * 1024;

/// Reads bits from `source`, most significant first, buffering from the
/// start of the frame being read (its mark) so that the frame's bytes can be
/// checked and a damaged frame read again from just after its sync code.
pub(super) struct Bits<R> {
    source: R,
    buf: Vec<u8>,
    /// Position of the next bit in `buf`.
    bit: usize,
    /// Byte in `buf` that refills keep.
    mark: usize,
    /// Bytes dropped from `buf` since the reader started or last moved.
    dropped: u64,
    ended: bool,
}

impl<R: Read + Seek> Bits<R> {
    /// A reader at the source's current position.
    pub fn new(source: R) -> Self {
        Self {
            source,
            buf: Vec::new(),
            bit: 0,
            mark: 0,
            dropped: 0,
            ended: false,
        }
    }

    /// Moves to byte `offset` of the source.
    pub fn seek_to(&mut self, offset: u64) -> bool {
        self.buf.clear();
        (self.bit, self.mark, self.dropped, self.ended) = (0, 0, 0, false);
        self.source.seek(SeekFrom::Start(offset)).is_ok()
    }

    /// Ensures `buf` holds `bytes` bytes from the current byte, if the
    /// source has them.
    fn fill(&mut self, bytes: usize) -> bool {
        let want = (self.bit >> 3) + bytes;
        if want - self.mark > FRAME_LIMIT {
            return false;
        }
        if want <= self.buf.len() {
            return true;
        }
        if self.ended {
            return false;
        }
        if self.mark > 0 {
            self.buf.drain(..self.mark);
            self.dropped += self.mark as u64;
            self.bit -= self.mark << 3;
            self.mark = 0;
        }
        let want = (self.bit >> 3) + bytes;
        while self.buf.len() < want {
            let len = self.buf.len();
            self.buf.resize(len + CHUNK, 0);
            let read = loop {
                match self.source.read(&mut self.buf[len..]) {
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    result => break result.unwrap_or(0),
                }
            };
            self.buf.truncate(len + read);
            if read == 0 {
                self.ended = true;
                return false;
            }
        }
        true
    }

    /// Bytes from the current byte to the frame limit.
    #[inline]
    fn budget(&self) -> usize {
        (self.mark + FRAME_LIMIT).saturating_sub(self.bit >> 3)
    }

    /// The 64 bits from the current byte, zero past the end of the data.
    #[inline]
    fn word(&mut self) -> u64 {
        self.fill(self.budget().min(8));
        let at = self.bit >> 3;
        if let Some(bytes) = self.buf.get(at..at + 8) {
            return u64::from_be_bytes(bytes.try_into().unwrap_or_default());
        }
        let mut bytes = [0; 8];
        let have = self.buf.len().saturating_sub(at).min(8);
        bytes[..have].copy_from_slice(&self.buf[at..at + have]);
        u64::from_be_bytes(bytes)
    }

    /// Whether `bits` more bits are in the data and within the frame limit.
    #[inline]
    fn has(&mut self, bits: usize) -> bool {
        self.fill(((self.bit & 7) + bits + 7) >> 3)
    }

    /// Reads `count` bits, 0 to 32, as an unsigned number.
    #[inline]
    pub fn read(&mut self, count: u32) -> Option<u32> {
        if count == 0 {
            return Some(0);
        }
        let word = self.word();
        if !self.has(count as usize) {
            return None;
        }
        let value = (word << (self.bit & 7)) >> (64 - count);
        self.bit += count as usize;
        Some(value as u32)
    }

    /// Reads a two's complement number of `count` bits, 1 to 33. Of 33 bits
    /// (a side channel of 32-bit audio), the low 32 are kept, as dr_flac
    /// keeps them, with its quirk: it reads in 64-bit lines counted from
    /// where it started (the first frame), and a number with one bit left in
    /// a line is read in two parts, the high bit shifted by 32, which x86
    /// takes as a shift by 0.
    pub fn read_signed(&mut self, count: u32) -> Option<i32> {
        if count > 32 {
            let in_line = ((self.dropped << 3) + self.bit as u64) % 64;
            let high = self.read(count - 32)?;
            let low = self.read(32)?;
            let split = count == 33 && in_line == 63;
            return Some((if split { low | high } else { low }) as i32);
        }
        let value = self.read(count)?;
        if count == 0 || count == 32 {
            return Some(value as i32);
        }
        let shift = 32 - count;
        Some(((value << shift) as i32) >> shift)
    }

    /// Counts zero bits up to the next one bit and moves past it.
    #[inline]
    pub fn unary(&mut self) -> Option<u32> {
        let mut zeros = 0_u32;
        loop {
            let skip = (self.bit & 7) as u32;
            let word = self.word() << skip;
            let lead = word.leading_zeros();
            if lead < 64 - skip {
                if !self.has(lead as usize + 1) {
                    return None;
                }
                self.bit += lead as usize + 1;
                return Some(zeros.wrapping_add(lead));
            }
            let run = 64 - skip;
            if !self.has(run as usize) {
                return None;
            }
            self.bit += run as usize;
            zeros = zeros.wrapping_add(run);
        }
    }

    /// Reads a Rice code with parameter `param` (0 to 30) and undoes its
    /// zigzag, as dr_flac does: in 32 bits, the quotient's high bits lost.
    #[inline]
    pub fn rice(&mut self, param: u32) -> Option<u32> {
        let quotient = self.unary()?;
        let value = (quotient << param) | self.read(param)?;
        Some((value >> 1) ^ (value & 1).wrapping_neg())
    }

    /// Reads `out.len()` Rice codes with parameter `param`, as [`Self::rice`]
    /// does, straight from the buffer while it holds them.
    pub fn rice_run(&mut self, param: u32, out: &mut [i32]) -> Option<()> {
        let mut i = 0;
        while i < out.len() {
            self.fill(self.budget().min(4096));
            let end = self.buf.len().min(self.mark + FRAME_LIMIT);
            let mut bit = self.bit;
            while i < out.len() && (bit >> 3) + 8 <= end {
                let at = bit >> 3;
                let bytes = self.buf[at..at + 8].try_into().unwrap_or_default();
                // At least 57 bits of the word are left after the shift.
                let word = u64::from_be_bytes(bytes) << (bit & 7);
                let lead = word.leading_zeros();
                if lead + 1 + param > 57 {
                    break;
                }
                let low = (word << (lead + 1)).checked_shr(64 - param).unwrap_or(0) as u32;
                let value = (lead << param) | low;
                out[i] = ((value >> 1) ^ (value & 1).wrapping_neg()) as i32;
                bit += (lead + 1 + param) as usize;
                i += 1;
            }
            self.bit = bit;
            if i < out.len() {
                out[i] = self.rice(param)? as i32;
                i += 1;
            }
        }
        Some(())
    }

    /// Moves to the next byte boundary.
    pub fn align(&mut self) {
        self.bit = (self.bit + 7) & !7;
    }

    /// Marks the current byte: refills keep it and what follows.
    pub fn set_mark(&mut self) {
        self.mark = self.bit >> 3;
    }

    /// Goes back to `bytes` bytes after the mark.
    pub fn back_to_mark(&mut self, bytes: usize) {
        self.bit = (self.mark + bytes) << 3;
    }

    /// The bytes from the mark up to the current byte.
    pub fn marked(&self) -> &[u8] {
        &self.buf[self.mark..self.bit >> 3]
    }
}

const fn crc8_table() -> [u8; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// `CRC16[k][b]`: the checksum of byte `b` followed by `k` zero bytes, so
/// that eight bytes can be added at once.
const fn crc16_tables() -> [[u16; 256]; 8] {
    let mut tables = [[0; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
            bit += 1;
        }
        tables[0][i] = crc;
        i += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let previous = tables[k - 1][i];
            tables[k][i] = (previous << 8) ^ tables[0][(previous >> 8) as usize];
            i += 1;
        }
        k += 1;
    }
    tables
}

static CRC8: [u8; 256] = crc8_table();
static CRC16: [[u16; 256]; 8] = crc16_tables();

/// FLAC's frame header checksum (polynomial 0x07).
pub(super) fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |crc, &b| CRC8[usize::from(crc ^ b)])
}

/// FLAC's frame checksum (polynomial 0x8005).
pub(super) fn crc16(bytes: &[u8]) -> u16 {
    let (chunks, rest) = bytes.as_chunks::<8>();
    let mut crc = 0_u16;
    for chunk in chunks {
        let [hi, lo] = crc.to_be_bytes();
        crc = CRC16[7][usize::from(chunk[0] ^ hi)]
            ^ CRC16[6][usize::from(chunk[1] ^ lo)]
            ^ CRC16[5][usize::from(chunk[2])]
            ^ CRC16[4][usize::from(chunk[3])]
            ^ CRC16[3][usize::from(chunk[4])]
            ^ CRC16[2][usize::from(chunk[5])]
            ^ CRC16[1][usize::from(chunk[6])]
            ^ CRC16[0][usize::from(chunk[7])];
    }
    rest.iter().fold(crc, |crc, &b| {
        (crc << 8) ^ CRC16[0][usize::from((crc >> 8) as u8 ^ b)]
    })
}
