// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! The polyphase synthesis filter bank (minimp3's `drmp3d_DCT_II`,
//! `drmp3d_synth_pair`, `drmp3d_synth` and `drmp3d_synth_granule`) with
//! `DR_MP3_FLOAT_OUTPUT`.
//!
//! On x64 and arm64 dr_mp3 only compiles its SIMD path (`DR_MP3_ONLY_SIMD`),
//! which computes four outputs per instruction. This is that path with four
//! lanes in arrays, each lane computed as an SSE lane is (here the same
//! operations in the same order as minimp3's scalar code; `drmp3d_DCT_II`,
//! where they differ, is in `dct`).

// Float constants keep C's digits, so they round to the same `f32`.
#![allow(clippy::excessive_precision)]

use super::dct::dct_ii;

/// `drmp3d_scale_pcm` with `DR_MP3_FLOAT_OUTPUT` (the SSE path multiplies
/// by the same constant).
#[inline]
fn scale_pcm(sample: f32) -> f32 {
    sample * (1.0_f32 / 32768.0_f32)
}

/// `drmp3d_synth_pair`: the two outputs of a column pair that need no
/// window pair, from `lins[z..]`.
fn synth_pair(pcm: &mut [f32], at: usize, nch: usize, lins: &[f32], z: usize) {
    let mut a = (lins[z + 14 * 64] - lins[z]) * 29.0;
    a += (lins[z + 64] + lins[z + 13 * 64]) * 213.0;
    a += (lins[z + 12 * 64] - lins[z + 2 * 64]) * 459.0;
    a += (lins[z + 3 * 64] + lins[z + 11 * 64]) * 2037.0;
    a += (lins[z + 10 * 64] - lins[z + 4 * 64]) * 5153.0;
    a += (lins[z + 5 * 64] + lins[z + 9 * 64]) * 6574.0;
    a += (lins[z + 8 * 64] - lins[z + 6 * 64]) * 37489.0;
    a += lins[z + 7 * 64] * 75038.0;
    pcm[at] = scale_pcm(a);

    let z = z + 2;
    let mut a = lins[z + 14 * 64] * 104.0;
    a += lins[z + 12 * 64] * 1567.0;
    a += lins[z + 10 * 64] * 9727.0;
    a += lins[z + 8 * 64] * 64019.0;
    a += lins[z + 6 * 64] * -9975.0;
    a += lins[z + 4 * 64] * -45.0;
    a += lins[z + 2 * 64] * 146.0;
    a += lins[z] * -5.0;
    pcm[at + 16 * nch] = scale_pcm(a);
}

#[rustfmt::skip]
static WIN: [f32; 240] = [
    -1.0,26.0,-31.0,208.0,218.0,401.0,-519.0,2063.0,2000.0,4788.0,-5517.0,7134.0,5959.0,35640.0,-39336.0,74992.0,
    -1.0,24.0,-35.0,202.0,222.0,347.0,-581.0,2080.0,1952.0,4425.0,-5879.0,7640.0,5288.0,33791.0,-41176.0,74856.0,
    -1.0,21.0,-38.0,196.0,225.0,294.0,-645.0,2087.0,1893.0,4063.0,-6237.0,8092.0,4561.0,31947.0,-43006.0,74630.0,
    -1.0,19.0,-41.0,190.0,227.0,244.0,-711.0,2085.0,1822.0,3705.0,-6589.0,8492.0,3776.0,30112.0,-44821.0,74313.0,
    -1.0,17.0,-45.0,183.0,228.0,197.0,-779.0,2075.0,1739.0,3351.0,-6935.0,8840.0,2935.0,28289.0,-46617.0,73908.0,
    -1.0,16.0,-49.0,176.0,228.0,153.0,-848.0,2057.0,1644.0,3004.0,-7271.0,9139.0,2037.0,26482.0,-48390.0,73415.0,
    -2.0,14.0,-53.0,169.0,227.0,111.0,-919.0,2032.0,1535.0,2663.0,-7597.0,9389.0,1082.0,24694.0,-50137.0,72835.0,
    -2.0,13.0,-58.0,161.0,224.0,72.0,-991.0,2001.0,1414.0,2330.0,-7910.0,9592.0,70.0,22929.0,-51853.0,72169.0,
    -2.0,11.0,-63.0,154.0,221.0,36.0,-1064.0,1962.0,1280.0,2006.0,-8209.0,9750.0,-998.0,21189.0,-53534.0,71420.0,
    -2.0,10.0,-68.0,147.0,215.0,2.0,-1137.0,1919.0,1131.0,1692.0,-8491.0,9863.0,-2122.0,19478.0,-55178.0,70590.0,
    -3.0,9.0,-73.0,139.0,208.0,-29.0,-1210.0,1870.0,970.0,1388.0,-8755.0,9935.0,-3300.0,17799.0,-56778.0,69679.0,
    -3.0,8.0,-79.0,132.0,200.0,-57.0,-1283.0,1817.0,794.0,1095.0,-8998.0,9966.0,-4533.0,16155.0,-58333.0,68692.0,
    -4.0,7.0,-85.0,125.0,189.0,-83.0,-1356.0,1759.0,605.0,814.0,-9219.0,9959.0,-5818.0,14548.0,-59838.0,67629.0,
    -4.0,7.0,-91.0,117.0,177.0,-106.0,-1428.0,1698.0,402.0,545.0,-9416.0,9916.0,-7154.0,12980.0,-61289.0,66494.0,
    -5.0,6.0,-97.0,111.0,163.0,-127.0,-1498.0,1634.0,185.0,288.0,-9585.0,9838.0,-8540.0,11455.0,-62684.0,65290.0,
];

/// Four lanes from `at` (`DRMP3_VLD`).
#[inline]
fn lanes(buf: &[f32], at: usize) -> [f32; 4] {
    let s = &buf[at..at + 4];
    [s[0], s[1], s[2], s[3]]
}

/// `drmp3d_synth`: 64 output frames from columns `xl` and `xl + 1` (and
/// the same columns 576 later for the right channel), written to `pcm`
/// from `dstl` with a stride of `nch`; `lins[lin..]` holds the filter
/// history.
fn synth(
    grbuf: &[f32],
    xl: usize,
    pcm: &mut [f32],
    dstl: usize,
    nch: usize,
    lins: &mut [f32],
    lin: usize,
) {
    let xr = xl + 576 * (nch - 1);
    let dstr = dstl + (nch - 1);
    let zlin = lin + 15 * 64;

    lins[zlin + 4 * 15] = grbuf[xl + 18 * 16];
    lins[zlin + 4 * 15 + 1] = grbuf[xr + 18 * 16];
    lins[zlin + 4 * 15 + 2] = grbuf[xl];
    lins[zlin + 4 * 15 + 3] = grbuf[xr];

    lins[zlin + 4 * 31] = grbuf[xl + 1 + 18 * 16];
    lins[zlin + 4 * 31 + 1] = grbuf[xr + 1 + 18 * 16];
    lins[zlin + 4 * 31 + 2] = grbuf[xl + 1];
    lins[zlin + 4 * 31 + 3] = grbuf[xr + 1];

    synth_pair(pcm, dstr, nch, lins, lin + 4 * 15 + 1);
    synth_pair(pcm, dstr + 32 * nch, nch, lins, lin + 4 * 15 + 64 + 1);
    synth_pair(pcm, dstl, nch, lins, lin + 4 * 15);
    synth_pair(pcm, dstl + 32 * nch, nch, lins, lin + 4 * 15 + 64);

    let mut w = 0;
    for i in (0..15).rev() {
        let row = 18 * (31 - i);
        lins[zlin + 4 * i] = grbuf[xl + row];
        lins[zlin + 4 * i + 1] = grbuf[xr + row];
        lins[zlin + 4 * i + 2] = grbuf[xl + 1 + row];
        lins[zlin + 4 * i + 3] = grbuf[xr + 1 + row];
        lins[zlin + 4 * i + 64] = grbuf[xl + 1 + 18 * (1 + i)];
        lins[zlin + 4 * i + 64 + 1] = grbuf[xr + 1 + 18 * (1 + i)];
        lins[zlin + 4 * i - 64 + 2] = grbuf[xl + 18 * (1 + i)];
        lins[zlin + 4 * i - 64 + 3] = grbuf[xr + 18 * (1 + i)];

        // DRMP3_V0(0) DRMP3_V2(1) DRMP3_V1(2) DRMP3_V2(3) ... DRMP3_V2(7)
        // on four lanes.
        let mut a = [0.0_f32; 4];
        let mut b = [0.0_f32; 4];
        for k in 0..8 {
            let (w0, w1) = (WIN[w], WIN[w + 1]);
            w += 2;
            let z = lanes(lins, zlin + 4 * i - 64 * k);
            let y = lanes(lins, zlin + 4 * i - 64 * (15 - k));
            if k == 0 {
                for j in 0..4 {
                    b[j] = z[j] * w1 + y[j] * w0;
                    a[j] = z[j] * w0 - y[j] * w1;
                }
            } else if k % 2 == 1 {
                for j in 0..4 {
                    b[j] += z[j] * w1 + y[j] * w0;
                    a[j] += y[j] * w1 - z[j] * w0;
                }
            } else {
                for j in 0..4 {
                    b[j] += z[j] * w1 + y[j] * w0;
                    a[j] += z[j] * w0 - y[j] * w1;
                }
            }
        }

        // The SSE stores, in their order (for mono, left and right are
        // the same frames and the left lanes land last).
        pcm[dstr + (15 - i) * nch] = scale_pcm(a[1]);
        pcm[dstr + (17 + i) * nch] = scale_pcm(b[1]);
        pcm[dstl + (15 - i) * nch] = scale_pcm(a[0]);
        pcm[dstl + (17 + i) * nch] = scale_pcm(b[0]);
        pcm[dstr + (47 - i) * nch] = scale_pcm(a[3]);
        pcm[dstr + (49 + i) * nch] = scale_pcm(b[3]);
        pcm[dstl + (47 - i) * nch] = scale_pcm(a[2]);
        pcm[dstl + (49 + i) * nch] = scale_pcm(b[2]);
    }
}

/// `drmp3d_synth_granule`: `nbands` columns of each of `nch` channels of
/// `grbuf` to 32 frames each in `pcm` from `pcm_at`, through `lins` (the
/// decoder's `syn` scratch) and the filter history `qmf_state`.
pub(super) fn synth_granule(
    qmf_state: &mut [f32; 15 * 64],
    grbuf: &mut [f32],
    nbands: usize,
    nch: usize,
    pcm: &mut [f32],
    pcm_at: usize,
    lins: &mut [f32; 33 * 64],
) {
    for i in 0..nch {
        dct_ii(grbuf, 576 * i, nbands);
    }
    lins[..15 * 64].copy_from_slice(qmf_state);
    for i in (0..nbands).step_by(2) {
        synth(grbuf, i, pcm, pcm_at + 32 * nch * i, nch, lins, i * 64);
    }
    let history = &lins[nbands * 64..nbands * 64 + 15 * 64];
    if nch == 1 {
        // Not DR_MP3_NONSTANDARD_BUT_LOGICAL: only the even entries.
        for i in (0..15 * 64).step_by(2) {
            qmf_state[i] = history[i];
        }
    } else {
        qmf_state.copy_from_slice(history);
    }
}
