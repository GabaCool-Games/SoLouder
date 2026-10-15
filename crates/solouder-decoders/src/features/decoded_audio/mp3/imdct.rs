// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! The layer 3 inverse MDCT (minimp3's `drmp3_L3_dct3_9`,
//! `drmp3_L3_imdct36`, `drmp3_L3_idct3`, `drmp3_L3_imdct12`,
//! `drmp3_L3_imdct_short`, `drmp3_L3_change_sign` and `drmp3_L3_imdct_gr`).
//! The SSE path of `drmp3_L3_imdct36` does the same operations per lane as
//! the scalar loop written here.

// Float constants keep C's digits, so they round to the same `f32`.
#![allow(clippy::excessive_precision)]

const SHORT_BLOCK_TYPE: u8 = 2;
const STOP_BLOCK_TYPE: u8 = 3;

/// `drmp3_L3_dct3_9`.
fn dct3_9(y: &mut [f32; 9]) {
    let (mut s0, mut s2, mut s4, mut s6, mut s8) = (y[0], y[2], y[4], y[6], y[8]);
    let mut t0 = s0 + s6 * 0.5;
    s0 -= s6;
    let mut t4 = (s4 + s2) * 0.93969262;
    let mut t2 = (s8 + s2) * 0.76604444;
    s6 = (s4 - s8) * 0.17364818;
    s4 += s8 - s2;

    s2 = s0 - s4 * 0.5;
    y[4] = s4 + s0;
    s8 = t0 - t2 + s6;
    s0 = t0 - t4 + t2;
    s4 = t0 + t4 - s6;

    let (mut s1, mut s3, mut s5, mut s7) = (y[1], y[3], y[5], y[7]);

    s3 *= 0.86602540;
    t0 = (s5 + s1) * 0.98480775;
    t4 = (s5 - s7) * 0.34202014;
    t2 = (s1 + s7) * 0.64278761;
    s1 = (s1 - s5 - s7) * 0.86602540;

    s5 = t0 - s3 - t2;
    s7 = t4 - s3 - t0;
    s3 = t4 + s3 - t2;

    y[0] = s4 - s7;
    y[1] = s2 + s1;
    y[2] = s0 - s3;
    y[3] = s8 + s5;
    y[5] = s8 - s5;
    y[6] = s0 + s3;
    y[7] = s2 - s1;
    y[8] = s4 + s7;
}

/// `drmp3_L3_imdct36`: `nbands` long blocks from `at`, with their
/// overlaps from `overlap`.
fn imdct36(grbuf: &mut [f32], at: usize, overlap: &mut [f32], window: &[f32; 18], nbands: usize) {
    const TWID9: [f32; 18] = [
        0.73727734, 0.79335334, 0.84339145, 0.88701083, 0.92387953, 0.95371695, 0.97629601,
        0.99144486, 0.99904822, 0.67559021, 0.60876143, 0.53729961, 0.46174861, 0.38268343,
        0.30070580, 0.21643961, 0.13052619, 0.04361938,
    ];
    for j in 0..nbands {
        let g = &mut grbuf[at + j * 18..at + j * 18 + 18];
        let overlap = &mut overlap[j * 9..j * 9 + 9];
        let mut co = [0.0_f32; 9];
        let mut si = [0.0_f32; 9];
        co[0] = -g[0];
        si[0] = g[17];
        for i in 0..4 {
            si[8 - 2 * i] = g[4 * i + 1] - g[4 * i + 2];
            co[1 + 2 * i] = g[4 * i + 1] + g[4 * i + 2];
            si[7 - 2 * i] = g[4 * i + 4] - g[4 * i + 3];
            co[2 + 2 * i] = -(g[4 * i + 3] + g[4 * i + 4]);
        }
        dct3_9(&mut co);
        dct3_9(&mut si);

        si[1] = -si[1];
        si[3] = -si[3];
        si[5] = -si[5];
        si[7] = -si[7];

        for i in 0..9 {
            let ovl = overlap[i];
            let sum = co[i] * TWID9[9 + i] + si[i] * TWID9[i];
            overlap[i] = co[i] * TWID9[i] - si[i] * TWID9[9 + i];
            g[i] = ovl * window[i] - sum * window[9 + i];
            g[17 - i] = ovl * window[9 + i] + sum * window[i];
        }
    }
}

/// `drmp3_L3_idct3`.
fn idct3(x0: f32, x1: f32, x2: f32) -> [f32; 3] {
    let m1 = x1 * 0.86602540;
    let a1 = x0 - x2 * 0.5;
    [a1 + m1, x0 + x2, a1 - m1]
}

/// `drmp3_L3_imdct12`: one short window of `x` (every third line) with
/// the three overlap values in `overlap`; returns `dst`.
fn imdct12(x: &[f32], overlap: &mut [f32]) -> [f32; 6] {
    const TWID3: [f32; 6] = [
        0.79335334, 0.92387953, 0.99144486, 0.60876143, 0.38268343, 0.13052619,
    ];
    let co = idct3(-x[0], x[6] + x[3], x[12] + x[9]);
    let mut si = idct3(x[15], x[12] - x[9], x[6] - x[3]);
    si[1] = -si[1];

    let mut dst = [0.0; 6];
    for i in 0..3 {
        let ovl = overlap[i];
        let sum = co[i] * TWID3[3 + i] + si[i] * TWID3[i];
        overlap[i] = co[i] * TWID3[i] - si[i] * TWID3[3 + i];
        dst[i] = ovl * TWID3[2 - i] - sum * TWID3[5 - i];
        dst[5 - i] = ovl * TWID3[5 - i] + sum * TWID3[2 - i];
    }
    dst
}

/// `drmp3_L3_imdct_short`: `nbands` short blocks from `at`.
fn imdct_short(grbuf: &mut [f32], at: usize, overlap: &mut [f32], nbands: usize) {
    for band in 0..nbands {
        let g = &mut grbuf[at + band * 18..at + band * 18 + 18];
        let overlap = &mut overlap[band * 9..band * 9 + 9];
        let mut tmp = [0.0_f32; 18];
        tmp.copy_from_slice(g);
        g[..6].copy_from_slice(&overlap[..6]);
        let out = imdct12(&tmp, &mut overlap[6..]);
        g[6..12].copy_from_slice(&out);
        let out = imdct12(&tmp[1..], &mut overlap[6..]);
        g[12..18].copy_from_slice(&out);
        let out = imdct12(&tmp[2..], &mut overlap[6..]);
        overlap[..6].copy_from_slice(&out);
    }
}

/// `drmp3_L3_change_sign`: negates the odd lines of the odd bands from
/// `at`.
pub(super) fn change_sign(grbuf: &mut [f32], at: usize) {
    for b in (0..32).step_by(2) {
        let g = at + 18 + b * 18;
        for i in (1..18).step_by(2) {
            grbuf[g + i] = -grbuf[g + i];
        }
    }
}

/// `drmp3_L3_imdct_gr`: the granule of one channel from `at`, its first
/// `n_long_bands` bands long blocks whatever the block type.
pub(super) fn imdct_gr(
    grbuf: &mut [f32],
    at: usize,
    overlap: &mut [f32; 9 * 32],
    block_type: u8,
    n_long_bands: usize,
) {
    const MDCT_WINDOW: [[f32; 18]; 2] = [
        [
            0.99904822, 0.99144486, 0.97629601, 0.95371695, 0.92387953, 0.88701083, 0.84339145,
            0.79335334, 0.73727734, 0.04361938, 0.13052619, 0.21643961, 0.30070580, 0.38268343,
            0.46174861, 0.53729961, 0.60876143, 0.67559021,
        ],
        [
            1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.99144486, 0.92387953, 0.79335334, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.13052619, 0.38268343, 0.60876143,
        ],
    ];
    let n_long_bands = n_long_bands.min(32);
    imdct36(grbuf, at, overlap, &MDCT_WINDOW[0], n_long_bands);
    let at = at + 18 * n_long_bands;
    let overlap = &mut overlap[9 * n_long_bands..];
    if block_type == SHORT_BLOCK_TYPE {
        imdct_short(grbuf, at, overlap, 32 - n_long_bands);
    } else {
        let window = &MDCT_WINDOW[usize::from(block_type == STOP_BLOCK_TYPE)];
        imdct36(grbuf, at, overlap, window, 32 - n_long_bands);
    }
}
