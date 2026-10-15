// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! Layers 1 and 2 (minimp3's `drmp3_L12_*`).

// Float constants keep C's digits, so they round to the same `f32`.
#![allow(clippy::excessive_precision)]

use super::bits::Bs;
use super::header::{
    bitrate_kbps, get_sample_rate, get_stereo_mode, get_stereo_mode_ext, is_layer_1, test_mpeg1,
};

const MODE_MONO: u32 = 3;
const MODE_JOINT_STEREO: u32 = 1;

/// `drmp3_L12_scale_info`.
pub(super) struct ScaleInfo {
    pub scf: [f32; 3 * 64],
    total_bands: u8,
    stereo_bands: u8,
    bitalloc: [u8; 64],
    scfcod: [u8; 64],
}

impl Default for ScaleInfo {
    fn default() -> Self {
        Self {
            scf: [0.0; 3 * 64],
            total_bands: 0,
            stereo_bands: 0,
            bitalloc: [0; 64],
            scfcod: [0; 64],
        }
    }
}

/// `drmp3_L12_subband_alloc`: (offset into the code table, code width,
/// bands).
type SubbandAlloc = (u8, u8, u8);

/// `drmp3_L12_subband_alloc_table`: the bit allocation tables of the frame,
/// and its band counts.
fn subband_alloc_table(hdr: &[u8; 4], sci: &mut ScaleInfo) -> &'static [SubbandAlloc] {
    let mode = get_stereo_mode(hdr);
    let stereo_bands = if mode == MODE_MONO {
        0
    } else if mode == MODE_JOINT_STEREO {
        (get_stereo_mode_ext(hdr) << 2) + 4
    } else {
        32
    };
    let (alloc, nbands): (&'static [SubbandAlloc], u32) = if is_layer_1(hdr) {
        (&[(76, 4, 32)], 32)
    } else if !test_mpeg1(hdr) {
        (&[(60, 4, 4), (44, 3, 7), (44, 2, 19)], 30)
    } else {
        let sample_rate_idx = get_sample_rate(hdr);
        let mut kbps = bitrate_kbps(hdr) >> u32::from(mode != MODE_MONO);
        if kbps == 0 {
            kbps = 192; // free format
        }
        if kbps < 56 {
            (
                &[(44, 4, 2), (44, 3, 10)],
                if sample_rate_idx == 2 { 12 } else { 8 },
            )
        } else {
            let all: &'static [SubbandAlloc] = &[(0, 4, 3), (16, 4, 8), (32, 3, 12), (40, 2, 7)];
            (
                all,
                if kbps >= 96 && sample_rate_idx != 1 {
                    30
                } else {
                    27
                },
            )
        }
    };
    sci.total_bands = nbands as u8;
    sci.stereo_bands = stereo_bands.min(nbands) as u8;
    alloc
}

/// `drmp3_L12_read_scalefactors`.
fn read_scalefactors(bs: &mut Bs, buf: &[u8], sci: &mut ScaleInfo, bands: usize) {
    const fn deq() -> [f32; 18 * 3] {
        const DIV: [f32; 18] = [
            3.0, 7.0, 15.0, 31.0, 63.0, 127.0, 255.0, 511.0, 1023.0, 2047.0, 4095.0, 8191.0,
            16383.0, 32767.0, 65535.0, 3.0, 5.0, 9.0,
        ];
        let mut table = [0.0; 18 * 3];
        let mut i = 0;
        while i < 18 {
            // DRMP3_DQ(x): three constants divided by x in f32.
            table[i * 3] = 9.53674316e-07_f32 / DIV[i];
            table[i * 3 + 1] = 7.56931807e-07_f32 / DIV[i];
            table[i * 3 + 2] = 6.00777173e-07_f32 / DIV[i];
            i += 1;
        }
        table
    }
    static DEQ_L12: [f32; 18 * 3] = deq();
    let mut out = 0;
    for i in 0..bands {
        let mut s = 0.0_f32;
        let ba = i32::from(sci.bitalloc[i]);
        let mask = if ba != 0 {
            4 + ((19 >> sci.scfcod[i]) & 3)
        } else {
            0
        };
        let mut m = 4;
        while m != 0 {
            if mask & m != 0 {
                let b = bs.get_bits(buf, 6) as i32;
                let index = (ba * 3 - 6 + b % 3) as usize;
                let deq = DEQ_L12.get(index).copied().unwrap_or(0.0);
                s = deq * ((1_i32 << 21) >> (b / 3)) as f32;
            }
            sci.scf[out] = s;
            out += 1;
            m >>= 1;
        }
    }
}

/// `drmp3_L12_read_scale_info`: bit allocations and scale factors.
pub(super) fn read_scale_info(hdr: &[u8; 4], bs: &mut Bs, buf: &[u8], sci: &mut ScaleInfo) {
    const BITALLOC_CODE_TAB: [u8; 92] = [
        0, 17, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, //
        0, 17, 18, 3, 19, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, //
        0, 17, 18, 3, 19, 4, 5, 16, //
        0, 17, 18, 16, //
        0, 17, 18, 19, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, //
        0, 17, 18, 3, 19, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, //
        0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    ];
    let alloc = subband_alloc_table(hdr, sci);
    let (mut k, mut ba_bits, mut tab_offset, mut next) = (0_usize, 0_u32, 0_usize, 0_usize);
    let code = |offset: usize, bits: u32| {
        BITALLOC_CODE_TAB
            .get(offset + bits as usize)
            .copied()
            .unwrap_or(0)
    };
    for i in 0..usize::from(sci.total_bands) {
        if i == k {
            if let Some(&(offset, width, count)) = alloc.get(next) {
                k += usize::from(count);
                ba_bits = u32::from(width);
                tab_offset = usize::from(offset);
            }
            next += 1;
        }
        let mut ba = code(tab_offset, bs.get_bits(buf, ba_bits));
        sci.bitalloc[2 * i] = ba;
        if i < usize::from(sci.stereo_bands) {
            ba = code(tab_offset, bs.get_bits(buf, ba_bits));
        }
        sci.bitalloc[2 * i + 1] = if sci.stereo_bands != 0 { ba } else { 0 };
    }

    for i in 0..2 * usize::from(sci.total_bands) {
        sci.scfcod[i] = if sci.bitalloc[i] == 0 {
            6
        } else if is_layer_1(hdr) {
            2
        } else {
            bs.get_bits(buf, 2) as u8
        };
    }

    read_scalefactors(bs, buf, sci, usize::from(sci.total_bands) * 2);

    for i in usize::from(sci.stereo_bands)..usize::from(sci.total_bands) {
        sci.bitalloc[2 * i + 1] = 0;
    }
}

/// `drmp3_L12_dequantize_granule`: one group of `group_size` samples per
/// band and channel, four times, into `grbuf` from `at` (channels 576
/// apart, bands 18 apart). Returns the samples per band written.
pub(super) fn dequantize_granule(
    grbuf: &mut [f32],
    at: usize,
    bs: &mut Bs,
    buf: &[u8],
    sci: &ScaleInfo,
    group_size: usize,
) -> usize {
    for j in 0..4 {
        let mut dst = at + group_size * j;
        let mut channel_one = false;
        for i in 0..2 * usize::from(sci.total_bands) {
            let ba = u32::from(sci.bitalloc[i]);
            if ba != 0 {
                if ba < 17 {
                    let half = (1_i32 << (ba - 1)) - 1;
                    for k in 0..group_size {
                        let value = bs.get_bits(buf, ba) as i32 - half;
                        if let Some(out) = grbuf.get_mut(dst + k) {
                            *out = value as f32;
                        }
                    }
                } else {
                    let modulus = (2_u32 << (ba - 17)) + 1; // 3, 5, 9
                    let mut code = bs.get_bits(buf, modulus + 2 - (modulus >> 3)); // 5, 7, 10
                    for k in 0..group_size {
                        let value = (code % modulus) as i32 - (modulus / 2) as i32;
                        if let Some(out) = grbuf.get_mut(dst + k) {
                            *out = value as f32;
                        }
                        code /= modulus;
                    }
                }
            }
            // `dst += choff; choff = 18 - choff`: 576 to the other channel,
            // then back to the next band.
            if channel_one {
                dst -= 558;
            } else {
                dst += 576;
            }
            channel_one = !channel_one;
        }
    }
    group_size * 4
}

/// `drmp3_L12_apply_scf_384`: copies the bands coded once to the second
/// channel and applies the scale factors from `scf_at` to a granule of 384
/// samples.
pub(super) fn apply_scf_384(sci: &ScaleInfo, scf_at: usize, dst: &mut [f32]) {
    let stereo = usize::from(sci.stereo_bands) * 18;
    let total = usize::from(sci.total_bands) * 18;
    dst.copy_within(stereo..total, 576 + stereo);
    for i in 0..usize::from(sci.total_bands) {
        let scf = &sci.scf[scf_at + i * 6..];
        for k in 0..12 {
            dst[i * 18 + k] *= scf[0];
            dst[i * 18 + k + 576] *= scf[3];
        }
    }
}
