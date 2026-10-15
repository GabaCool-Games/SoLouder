// Ported from dr_mp3 0.6.31 by David Reid (public domain or MIT No
// Attribution), based on minimp3 by Lion (lieff) (CC0); altered for
// SoLouder.

//! minimp3's `drmp3d_DCT_II` as its SIMD path runs it on x64 and arm64
//! (`DR_MP3_ONLY_SIMD`). The SIMD path works on four columns at a time,
//! but its lanes never mix, so each column is computed here on its own with
//! the operations of one SSE lane. The sums at the end are in the SIMD
//! path's order, which differs from the scalar code's.

// Float constants keep C's digits, so they round to the same `f32`.
#![allow(clippy::excessive_precision)]

const SEC: [f32; 24] = [
    10.19000816,
    0.50060302,
    0.50241929,
    3.40760851,
    0.50547093,
    0.52249861,
    2.05778098,
    0.51544732,
    0.56694406,
    1.48416460,
    0.53104258,
    0.64682180,
    1.16943991,
    0.55310392,
    0.78815460,
    0.97256821,
    0.58293498,
    1.06067765,
    0.83934963,
    0.62250412,
    1.72244716,
    0.74453628,
    0.67480832,
    5.10114861,
];

/// The butterflies of one row of eight (the second loop of `DCT_II`).
#[inline]
fn butterflies(x: &mut [f32; 8]) {
    let [
        mut x0,
        mut x1,
        mut x2,
        mut x3,
        mut x4,
        mut x5,
        mut x6,
        mut x7,
    ] = *x;
    let mut xt = x0 - x7;
    x0 += x7;
    x7 = x1 - x6;
    x1 += x6;
    x6 = x2 - x5;
    x2 += x5;
    x5 = x3 - x4;
    x3 += x4;
    x4 = x0 - x3;
    x0 += x3;
    x3 = x1 - x2;
    x1 += x2;
    x[0] = x0 + x1;
    x[4] = (x0 - x1) * 0.70710677;
    x5 += x6;
    x6 = (x6 + x7) * 0.70710677;
    x7 += xt;
    x3 = (x3 + x4) * 0.70710677;
    x5 -= x7 * 0.198912367; // rotate by PI/8
    x7 += x5 * 0.382683432;
    x5 -= x7 * 0.198912367;
    x0 = xt - x6;
    xt += x6;
    x[1] = (xt + x7) * 0.50979561;
    x[2] = (x4 + x3) * 0.54119611;
    x[3] = (x0 - x5) * 0.60134488;
    x[5] = (x0 + x5) * 0.89997619;
    x[6] = (x4 - x3) * 1.30656302;
    x[7] = (xt - x7) * 2.56291556;
}

/// `drmp3d_DCT_II`: the first `n` columns (lines 18 apart) of the 32 bands
/// from `at`.
pub(super) fn dct_ii(grbuf: &mut [f32], at: usize, n: usize) {
    for k in 0..n {
        let y = at + k;
        let mut t = [[0.0_f32; 8]; 4];
        for i in 0..8 {
            let x0 = grbuf[y + i * 18];
            let x1 = grbuf[y + (15 - i) * 18];
            let x2 = grbuf[y + (16 + i) * 18];
            let x3 = grbuf[y + (31 - i) * 18];
            let t0 = x0 + x3;
            let t1 = x1 + x2;
            let t2 = (x1 - x2) * SEC[3 * i];
            let t3 = (x0 - x3) * SEC[3 * i + 1];
            t[0][i] = t0 + t1;
            t[1][i] = (t0 - t1) * SEC[3 * i + 2];
            t[2][i] = t3 + t2;
            t[3][i] = (t3 - t2) * SEC[3 * i + 2];
        }
        for x in &mut t {
            butterflies(x);
        }
        let mut y = y;
        for i in 0..7 {
            // SSE: s = t[3][i] + t[3][i + 1] first, then t[2] + s. The
            // scalar code sums t[2][i] + t[3][i] + t[3][i + 1] left to
            // right, which rounds differently.
            let s = t[3][i] + t[3][i + 1];
            grbuf[y] = t[0][i];
            grbuf[y + 18] = t[2][i] + s;
            grbuf[y + 2 * 18] = t[1][i] + t[1][i + 1];
            grbuf[y + 3 * 18] = t[2][i + 1] + s;
            y += 4 * 18;
        }
        grbuf[y] = t[0][7];
        grbuf[y + 18] = t[2][7] + t[3][7];
        grbuf[y + 2 * 18] = t[1][7];
        grbuf[y + 3 * 18] = t[3][7];
    }
}
