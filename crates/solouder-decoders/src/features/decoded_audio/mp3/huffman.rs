// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! Layer 3 Huffman decoding and dequantization (minimp3's
//! `drmp3_L3_pow_43` and `drmp3_L3_huffman`).

use super::bits::Bs;
use super::huffman_tables::{LINBITS, TAB32, TAB33, TABINDEX, TABS};
use super::side_info::GrInfo;
use super::tables::POW43;

/// `g_drmp3_pow43[16 + x]`, 0 outside the table.
#[inline]
fn pow43_at(x: i32) -> f32 {
    usize::try_from(16 + x)
        .ok()
        .and_then(|i| POW43.get(i))
        .copied()
        .unwrap_or(0.0)
}

/// `drmp3_L3_pow_43`: `x^(4/3)` for 0 to 8206, interpolated above 128.
fn pow_43(mut x: i32) -> f32 {
    let mut mult = 256;
    if x < 129 {
        return pow43_at(x);
    }
    if x < 1024 {
        mult = 16;
        x <<= 3;
    }
    let sign = (2 * x) & 64;
    let frac = ((x & 63) - sign) as f32 / ((x & !63) + sign) as f32;
    pow43_at((x + sign) >> 6)
        * (1.0 + frac * ((4.0_f32 / 3.0) + frac * (2.0_f32 / 9.0)))
        * mult as f32
}

/// The bit cache of `drmp3_L3_huffman` (`bs_cache`, `bs_sh`,
/// `bs_next_ptr`), over the main data in `buf`; bytes past its end read
/// as 0.
struct Cache<'a> {
    buf: &'a [u8],
    cache: u32,
    sh: i32,
    next: usize,
}

impl Cache<'_> {
    #[inline]
    fn byte(&self, i: usize) -> u32 {
        u32::from(self.buf.get(i).copied().unwrap_or(0))
    }

    /// `DRMP3_PEEK_BITS`; a shift by 32 behaves as on x86 (by 0).
    #[inline]
    fn peek(&self, n: u32) -> u32 {
        self.cache.wrapping_shr(32 - n)
    }

    /// `DRMP3_FLUSH_BITS`.
    #[inline]
    fn flush(&mut self, n: u32) {
        self.cache = self.cache.wrapping_shl(n);
        self.sh += n as i32;
    }

    /// `DRMP3_CHECK_BITS`: refills the cache a byte at a time.
    #[inline]
    fn check(&mut self) {
        while self.sh >= 0 {
            self.cache |= self.byte(self.next).wrapping_shl(self.sh as u32);
            self.next += 1;
            self.sh -= 8;
        }
    }

    /// `DRMP3_BSPOS`.
    #[inline]
    fn pos(&self) -> i64 {
        self.next as i64 * 8 - 24 + i64::from(self.sh)
    }

    /// Whether the next bit is set (`(drmp3_int32)bs_cache < 0`).
    #[inline]
    fn sign(&self) -> bool {
        (self.cache as i32) < 0
    }
}

/// `drmp3_L3_huffman`: the 576 lines of one granule of one channel into
/// `dst`, scaled by the band gains in `scf`, reading the main data `buf`
/// up to bit `layer3gr_limit`, where it leaves `bs`.
pub(super) fn huffman(
    dst: &mut [f32],
    bs: &mut Bs,
    buf: &[u8],
    gr_info: &GrInfo,
    scf: &[f32],
    layer3gr_limit: i32,
) {
    let sfb = |i: usize| i32::from(gr_info.sfbtab.get(i).copied().unwrap_or(0));
    let mut one = 0.0_f32;
    let (mut sfb_i, mut scf_i, mut d) = (0_usize, 0_usize, 0_usize);
    let mut big_val_cnt = i32::from(gr_info.big_values);
    let start = (bs.pos.max(0) / 8) as usize;
    let mut c = Cache {
        buf,
        cache: 0,
        sh: (bs.pos & 7) - 8,
        next: start + 4,
    };
    c.cache = (((c.byte(start) * 256 + c.byte(start + 1)) * 256 + c.byte(start + 2)) * 256
        + c.byte(start + 3))
    .wrapping_shl((bs.pos & 7) as u32);
    let mut store = |d: usize, value: f32| {
        if let Some(out) = dst.get_mut(d) {
            *out = value;
        }
    };

    let mut ireg = 0;
    while big_val_cnt > 0 {
        let tab_num = usize::from(gr_info.table_select.get(ireg).copied().unwrap_or(0) & 31);
        let mut sfb_cnt = i32::from(gr_info.region_count.get(ireg).copied().unwrap_or(0));
        ireg += 1;
        let codebook = TABINDEX[tab_num] as i32;
        let linbits = u32::from(LINBITS[tab_num]);
        loop {
            let np = sfb(sfb_i) / 2;
            sfb_i += 1;
            let mut pairs_to_decode = big_val_cnt.min(np);
            one = scf.get(scf_i).copied().unwrap_or(0.0);
            scf_i += 1;
            // Band tables add up to 576 lines and big_values is at most
            // 288, so a band of 0 is never reached; C would run away.
            while pairs_to_decode > 0 {
                let mut w = 5;
                let code = |i: i64| i32::from(TABS.get(i as usize).copied().unwrap_or(0));
                let mut leaf = code(i64::from(codebook) + i64::from(c.peek(w)));
                while leaf < 0 {
                    c.flush(w);
                    w = (leaf & 7) as u32;
                    leaf = code(i64::from(codebook) + i64::from(c.peek(w)) - i64::from(leaf >> 3));
                }
                c.flush((leaf >> 8) as u32);

                for _ in 0..2 {
                    let mut lsb = leaf & 0x0F;
                    if linbits != 0 && lsb == 15 {
                        lsb += c.peek(linbits) as i32;
                        c.flush(linbits);
                        c.check();
                        let sign = if c.sign() { -1.0 } else { 1.0 };
                        store(d, one * pow_43(lsb) * sign);
                    } else {
                        store(d, pow43_at(lsb - 16 * (c.cache >> 31) as i32) * one);
                    }
                    c.flush(u32::from(lsb != 0));
                    d += 1;
                    leaf >>= 4;
                }
                c.check();
                pairs_to_decode -= 1;
            }
            big_val_cnt -= np;
            if big_val_cnt <= 0 {
                break;
            }
            sfb_cnt -= 1;
            if sfb_cnt < 0 {
                break;
            }
        }
    }

    let codebook_count1: &[u8] = if gr_info.count1_table != 0 {
        &TAB33
    } else {
        &TAB32
    };
    let count1 = |i: u32| i32::from(codebook_count1.get(i as usize).copied().unwrap_or(0));
    let mut np = 1 - big_val_cnt;
    'quads: loop {
        let mut leaf = count1(c.peek(4));
        if leaf & 8 == 0 {
            let more = (c.cache << 4).wrapping_shr(32 - (leaf & 3) as u32);
            leaf = count1((leaf >> 3) as u32 + more);
        }
        c.flush((leaf & 7) as u32);
        if c.pos() > i64::from(layer3gr_limit) {
            break;
        }
        for half in 0..2 {
            // DRMP3_RELOAD_SCALEFACTOR
            np -= 1;
            if np == 0 {
                np = sfb(sfb_i) / 2;
                sfb_i += 1;
                if np == 0 {
                    break 'quads;
                }
                one = scf.get(scf_i).copied().unwrap_or(0.0);
                scf_i += 1;
            }
            // DRMP3_DEQ_COUNT1
            for s in half * 2..half * 2 + 2 {
                if leaf & (128 >> s) != 0 {
                    store(d + s, if c.sign() { -one } else { one });
                    c.flush(1);
                }
            }
        }
        c.check();
        d += 4;
    }

    bs.pos = layer3gr_limit;
}
