// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! Layer 3 stereo processing, reordering and alias reduction (minimp3's
//! `drmp3_L3_midside_stereo`, `drmp3_L3_intensity_stereo*`,
//! `drmp3_L3_stereo_*`, `drmp3_L3_reorder` and `drmp3_L3_antialias`).
//!
//! All work on the granule buffer of the decoder: channel 0 at 0, channel 1
//! at 576 (the SSE paths of mid/side and alias reduction do the same
//! operations per lane as the scalar code).

// Float constants keep C's digits, so they round to the same `f32`.
#![allow(clippy::excessive_precision)]

use super::header::{test_mpeg1, test_ms_stereo};
use super::side_info::{GrInfo, ldexp_q2};

/// `drmp3_L3_midside_stereo`: `n` lines from `left` and 576 after it.
pub(super) fn midside_stereo(grbuf: &mut [f32], left: usize, n: usize) {
    for i in left..left + n {
        let (a, b) = (grbuf[i], grbuf[i + 576]);
        grbuf[i] = a + b;
        grbuf[i + 576] = a - b;
    }
}

/// `drmp3_L3_intensity_stereo_band`.
fn intensity_stereo_band(grbuf: &mut [f32], left: usize, n: usize, kl: f32, kr: f32) {
    for i in left..left + n {
        grbuf[i + 576] = grbuf[i] * kr;
        grbuf[i] *= kl;
    }
}

/// The width of band `i` of `sfb`, 0 past its end (C pads with zeros).
fn width(sfb: &[u8], i: usize) -> usize {
    usize::from(sfb.get(i).copied().unwrap_or(0))
}

/// `drmp3_L3_stereo_top_band`: the last band of the right channel (from 576)
/// holding a line other than 0, per short window.
fn stereo_top_band(grbuf: &[f32], sfb: &[u8], nbands: usize, max_band: &mut [i32; 3]) {
    *max_band = [-1; 3];
    let mut right = 576;
    for i in 0..nbands {
        let w = width(sfb, i);
        for k in (0..w).step_by(2) {
            if grbuf[right + k] != 0.0 || grbuf[right + k + 1] != 0.0 {
                max_band[i % 3] = i as i32;
                break;
            }
        }
        right += w;
    }
}

/// `drmp3_L3_stereo_process`.
fn stereo_process(
    grbuf: &mut [f32],
    ist_pos: &[u8; 39],
    sfb: &[u8],
    hdr: &[u8; 4],
    max_band: &[i32; 3],
    mpeg2_sh: u32,
) {
    const PAN: [f32; 7 * 2] = [
        0.0, 1.0, 0.21132487, 0.78867513, 0.36602540, 0.63397460, 0.5, 0.5, 0.63397460, 0.36602540,
        0.78867513, 0.21132487, 1.0, 0.0,
    ];
    let max_pos = if test_mpeg1(hdr) { 7 } else { 64 };
    let mut left = 0;
    let mut i = 0;
    while width(sfb, i) != 0 {
        let w = width(sfb, i);
        let ipos = u32::from(ist_pos.get(i).copied().unwrap_or(0));
        if i as i32 > max_band[i % 3] && ipos < max_pos {
            // C's digits (the same f32 as SQRT_2).
            #[allow(clippy::approx_constant)]
            let s = if test_ms_stereo(hdr) { 1.41421356 } else { 1.0 };
            let (kl, kr);
            if test_mpeg1(hdr) {
                kl = PAN[2 * ipos as usize];
                kr = PAN[2 * ipos as usize + 1];
            } else {
                let k = ldexp_q2(1.0, (((ipos + 1) >> 1) << mpeg2_sh) as i32);
                (kl, kr) = if ipos & 1 != 0 { (k, 1.0) } else { (1.0, k) };
            }
            intensity_stereo_band(grbuf, left, w, kl * s, kr * s);
        } else if test_ms_stereo(hdr) {
            midside_stereo(grbuf, left, w);
        }
        left += w;
        i += 1;
    }
}

/// `drmp3_L3_intensity_stereo`: `gr` holds the granule's two channels.
pub(super) fn intensity_stereo(
    grbuf: &mut [f32],
    ist_pos: &mut [u8; 39],
    gr: &[GrInfo],
    hdr: &[u8; 4],
) {
    let n_sfb = usize::from(gr[0].n_long_sfb) + usize::from(gr[0].n_short_sfb);
    let max_blocks = if gr[0].n_short_sfb != 0 { 3 } else { 1 };
    let mut max_band = [-1; 3];
    stereo_top_band(grbuf, gr[0].sfbtab, n_sfb, &mut max_band);
    if gr[0].n_long_sfb != 0 {
        max_band = [max_band[0].max(max_band[1]).max(max_band[2]); 3];
    }
    for (i, &band) in max_band.iter().enumerate().take(max_blocks) {
        let default_pos = if test_mpeg1(hdr) { 3 } else { 0 };
        let itop = n_sfb - max_blocks + i;
        let prev = itop - max_blocks;
        ist_pos[itop] = if band >= prev as i32 {
            default_pos
        } else {
            ist_pos[prev]
        };
    }
    let mpeg2_sh = u32::from(gr[1].scalefac_compress & 1);
    stereo_process(grbuf, ist_pos, gr[0].sfbtab, hdr, &max_band, mpeg2_sh);
}

/// `drmp3_L3_reorder`: the short blocks from `at`, window by window per
/// band, into band order, through `scratch`. Reads and writes may pass the
/// channel's 576 lines, as in C (MPEG-2.5 at 8 kHz with mixed blocks): the
/// buffer is laid out as C's scratch is.
pub(super) fn reorder(grbuf: &mut [f32], at: usize, scratch: &mut [f32], sfb: &[u8]) {
    let mut src = at;
    let mut dst = 0;
    let mut band = 0;
    loop {
        let len = width(sfb, band);
        if len == 0 {
            break;
        }
        for _ in 0..len {
            for window in 0..3 {
                if let Some(out) = scratch.get_mut(dst) {
                    *out = grbuf.get(src + window * len).copied().unwrap_or(0.0);
                }
                dst += 1;
            }
            src += 1;
        }
        band += 3;
        src += 2 * len;
    }
    let n = dst.min(scratch.len()).min(grbuf.len().saturating_sub(at));
    grbuf[at..at + n].copy_from_slice(&scratch[..n]);
}

/// `drmp3_L3_antialias`: the butterflies between the `nbands` + 1 first
/// bands from `at`.
pub(super) fn antialias(grbuf: &mut [f32], at: usize, nbands: i32) {
    const AA: [[f32; 8]; 2] = [
        [
            0.85749293, 0.88174200, 0.94962865, 0.98331459, 0.99551782, 0.99916056, 0.99989920,
            0.99999316,
        ],
        [
            0.51449576, 0.47173197, 0.31337745, 0.18191320, 0.09457419, 0.04096558, 0.01419856,
            0.00369997,
        ],
    ];
    for band in 0..nbands.max(0) as usize {
        let g = at + band * 18;
        for i in 0..8 {
            let u = grbuf[g + 18 + i];
            let d = grbuf[g + 17 - i];
            grbuf[g + 18 + i] = u * AA[0][i] - d * AA[1][i];
            grbuf[g + 17 - i] = u * AA[1][i] + d * AA[0][i];
        }
    }
}
