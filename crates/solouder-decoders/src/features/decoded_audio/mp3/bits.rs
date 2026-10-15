// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! minimp3's bit reader (`drmp3_bs`).

/// A position and a limit, both in bits, over a byte buffer that each read
/// is given (`drmp3_bs`; the buffer is passed in so the reader can live next
/// to the buffer it reads).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Bs {
    pub pos: i32,
    pub limit: i32,
}

impl Bs {
    /// `drmp3_bs_init`: `bytes` bytes, read from the start.
    pub fn new(bytes: i32) -> Self {
        Self {
            pos: 0,
            limit: bytes.wrapping_mul(8),
        }
    }

    /// `drmp3_bs_get_bits`: the next `n` bits (1 to 32), most significant
    /// first. A read past the limit returns 0 but still moves the position,
    /// as in C. Bytes past the end of `buf` read as 0.
    pub fn get_bits(&mut self, buf: &[u8], n: u32) -> u32 {
        let s = (self.pos & 7) as u32;
        let mut shl = n as i32 + s as i32;
        let mut p = (self.pos >> 3) as usize;
        self.pos = self.pos.wrapping_add(n as i32);
        if self.pos > self.limit {
            return 0;
        }
        let byte = |i: usize| u32::from(buf.get(i).copied().unwrap_or(0));
        let mut next = byte(p) & (255 >> s);
        p += 1;
        let mut cache = 0_u32;
        loop {
            shl -= 8;
            if shl <= 0 {
                break;
            }
            cache |= next.wrapping_shl(shl as u32);
            next = byte(p);
            p += 1;
        }
        cache | next.wrapping_shr((-shl) as u32)
    }
}
