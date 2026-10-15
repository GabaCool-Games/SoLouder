// Ported from stb_vorbis 1.22 by Sean Barrett (public domain or MIT);
// altered for SoLouder.

//! `inverse_mdct`: stb's optimised inverse MDCT, its steps kept as written
//! (and in the same float operation order). stb walks pointers, mostly
//! backwards; here they are chunks of the buffers: a chunk of 8 ending at
//! the C's `ee0` holds `ee0[-7]` to `ee0[0]` at indices 0 to 7.

use super::setup::BlockTables;
use super::tables::ilog;

/// One butterfly of step 3 at `ee0[-j]`, `ee0[-j-1]` (index `i = 7 - j`).
#[inline(always)]
fn rotate(x0: &mut [f32; 8], x2: &mut [f32; 8], i: usize, a0: f32, a1: f32) {
    let k00_20 = x0[i] - x2[i];
    let k01_21 = x0[i - 1] - x2[i - 1];
    x0[i] += x2[i];
    x0[i - 1] += x2[i - 1];
    x2[i] = k00_20 * a0 - k01_21 * a1;
    x2[i - 1] = k01_21 * a0 + k00_20 * a1;
}

/// The 8 values ending at `ee0` and the 8 ending at `ee0 - k_off`, which
/// do not overlap (every caller's `k_off` is at least 8 when it loops).
#[inline(always)]
fn pair(e: &mut [f32], ee0: usize, k_off: usize) -> Option<(&mut [f32; 8], &mut [f32; 8])> {
    let (lo, hi) = e.split_at_mut(ee0.checked_sub(7)?);
    let x0 = hi.first_chunk_mut::<8>()?;
    let x2 = lo
        .get_mut(ee0.checked_sub(k_off + 7)?..)?
        .first_chunk_mut::<8>()?;
    Some((x0, x2))
}

/// `imdct_step3_iter0_loop`; `k_off` is the C's `-k_off`.
fn step3_iter0_loop(n: usize, e: &mut [f32], i_off: usize, k_off: usize, a: &[f32]) {
    let mut ee0 = i_off;
    for a in a.chunks(32).take(n >> 2) {
        let Some((x0, x2)) = pair(e, ee0, k_off) else {
            return;
        };
        for (i, a) in [7, 5, 3, 1].into_iter().zip(a.chunks(8)) {
            rotate(x0, x2, i, a[0], a[1]);
        }
        ee0 = ee0.wrapping_sub(8);
    }
}

/// `imdct_step3_inner_r_loop`; `k_off` is the C's `-k_off`.
fn step3_inner_r_loop(lim: usize, e: &mut [f32], d0: usize, k_off: usize, a: &[f32], k1: usize) {
    let mut e0 = d0;
    let mut aa = 0;
    for _ in 0..lim >> 2 {
        let Some((x0, x2)) = pair(e, e0, k_off) else {
            return;
        };
        for i in [7, 5, 3, 1] {
            rotate(x0, x2, i, a[aa], a[aa + 1]);
            aa += k1;
        }
        e0 = e0.wrapping_sub(8);
    }
}

/// `imdct_step3_inner_s_loop`; `k_off` is the C's `-k_off`.
fn step3_inner_s_loop(
    n: usize,
    e: &mut [f32],
    i_off: usize,
    k_off: usize,
    a: &[f32],
    a_off: usize,
    k0: usize,
) {
    let aa = [
        (a[0], a[1]),
        (a[a_off], a[a_off + 1]),
        (a[a_off * 2], a[a_off * 2 + 1]),
        (a[a_off * 3], a[a_off * 3 + 1]),
    ];
    let mut ee0 = i_off;
    for _ in 0..n {
        let Some((x0, x2)) = pair(e, ee0, k_off) else {
            return;
        };
        for (i, (a0, a1)) in [7, 5, 3, 1].into_iter().zip(aa) {
            rotate(x0, x2, i, a0, a1);
        }
        ee0 = ee0.wrapping_sub(k0);
    }
}

/// `iter_54` on the eight values `z[-7]` to `z[0]` (each is read before it
/// is written, so they can be loaded first).
#[inline(always)]
fn iter_54(s: &mut [f32]) {
    let [z7, z6, z5, z4, z3, z2, z1, z0] = [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]];
    let k00 = z0 - z4;
    let y0 = z0 + z4;
    let y2 = z2 + z6;
    let k22 = z2 - z6;
    s[7] = y0 + y2; // z0 + z4 + z2 + z6
    s[5] = y0 - y2; // z0 + z4 - z2 - z6
    let k33 = z3 - z7;
    s[3] = k00 + k33; // z0 - z4 + z3 - z7
    s[1] = k00 - k33; // z0 - z4 - z3 + z7
    let k11 = z1 - z5;
    let y1 = z1 + z5;
    let y3 = z3 + z7;
    s[6] = y1 + y3; // z1 + z5 + z3 + z7
    s[4] = y1 - y3; // z1 + z5 - z3 - z7
    s[2] = k11 - k22; // z1 - z5 + z2 - z6
    s[0] = k11 + k22; // z1 - z5 - z2 + z6
}

/// `imdct_step3_inner_s_loop_ld654`: `n` blocks of 16 values, the first
/// ending at `i_off` (index 15 of a block is the C's `z[0]`).
fn step3_inner_s_loop_ld654(n: usize, e: &mut [f32], i_off: usize, a: &[f32], base_n: usize) {
    let a2 = a[base_n >> 3];
    let Some(blocks) = (i_off + 1)
        .checked_sub(16 * n)
        .and_then(|from| e.get_mut(from..=i_off))
    else {
        return;
    };
    for z in blocks.as_rchunks_mut::<16>().1.iter_mut().rev() {
        let k00 = z[15] - z[7];
        let k11 = z[14] - z[6];
        let l00 = z[13] - z[5];
        let l11 = z[12] - z[4];
        z[15] += z[7];
        z[14] += z[6];
        z[13] += z[5];
        z[12] += z[4];
        z[7] = k00;
        z[6] = k11;
        z[5] = (l00 + l11) * a2;
        z[4] = (l11 - l00) * a2;

        let k00 = z[11] - z[3];
        let k11 = z[10] - z[2];
        let l00 = z[9] - z[1];
        let l11 = z[8] - z[0];
        z[11] += z[3];
        z[10] += z[2];
        z[9] += z[1];
        z[8] += z[0];
        z[3] = k11;
        z[2] = -k00;
        z[1] = (l11 - l00) * a2;
        z[0] = (l00 + l11) * -a2;

        let (low, high) = z.split_at_mut(8);
        iter_54(high);
        iter_54(low);
    }
}

/// `inverse_mdct` of the first `n / 2` values of `buffer` into its first
/// `n`, with the tables of its blocksize and `buf2` (`n / 2` values) as
/// scratch.
pub(super) fn inverse_mdct(buffer: &mut [f32], n: usize, t: &BlockTables, buf2: &mut [f32]) {
    let (n2, n4, n8) = (n >> 1, n >> 2, n >> 3);
    let a = &t.a[..n2];
    let (u, v) = (&mut buffer[..n], &mut buf2[..n2]);

    // copy and reflect spectral data, and step 0: `d` runs down `v` in
    // pairs, `e` up `u` in fours, then down again
    {
        let (a_lo, a_hi) = a.split_at(n4);
        let (v_lo, v_hi) = v.split_at_mut(n4);
        let pairs = a_lo
            .as_chunks::<2>()
            .0
            .iter()
            .zip(v_hi.as_rchunks_mut::<2>().1.iter_mut().rev());
        for (e, (aa, d)) in u[..n2].as_chunks::<4>().0.iter().zip(pairs) {
            d[1] = e[0] * aa[0] - e[2] * aa[1];
            d[0] = e[0] * aa[1] + e[2] * aa[0];
        }
        let pairs = a_hi
            .as_chunks::<2>()
            .0
            .iter()
            .zip(v_lo.as_rchunks_mut::<2>().1.iter_mut().rev());
        for (e, (aa, d)) in u[..n2].as_rchunks::<4>().1.iter().rev().zip(pairs) {
            // e[1] is the C's e[0], e[3] its e[2]
            d[1] = -e[3] * aa[0] - -e[1] * aa[1];
            d[0] = -e[3] * aa[1] + -e[1] * aa[0];
        }
    }

    // step 2 (paper output is w, now u): `AA` runs down `A` in eights
    {
        let (v_lo, v_hi) = v.split_at(n4);
        let (u_lo, u_hi) = u.split_at_mut(n4);
        let quads = v_hi
            .as_chunks::<4>()
            .0
            .iter()
            .zip(v_lo.as_chunks::<4>().0.iter());
        let outs = u_hi[..n4]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(u_lo.as_chunks_mut::<4>().0.iter_mut());
        for ((aa, (e0, e1)), (d0, d1)) in a.as_rchunks::<8>().1.iter().rev().zip(quads).zip(outs) {
            let v41_21 = e0[1] - e1[1];
            let v40_20 = e0[0] - e1[0];
            d0[1] = e0[1] + e1[1];
            d0[0] = e0[0] + e1[0];
            d1[1] = v41_21 * aa[4] - v40_20 * aa[5];
            d1[0] = v40_20 * aa[4] + v41_21 * aa[5];

            let v41_21 = e0[3] - e1[3];
            let v40_20 = e0[2] - e1[2];
            d0[3] = e0[3] + e1[3];
            d0[2] = e0[2] + e1[2];
            d1[3] = v41_21 * aa[0] - v40_20 * aa[1];
            d1[2] = v40_20 * aa[0] + v41_21 * aa[1];
        }
    }

    // step 3
    let ld = (ilog(n as i32) - 1) as usize;
    // iteration 0
    step3_iter0_loop(n >> 4, u, n2 - 1, n >> 3, a);
    step3_iter0_loop(n >> 4, u, n2 - 1 - n4, n >> 3, a);
    // iteration 1
    for i in 0..4 {
        step3_inner_r_loop(n >> 5, u, n2 - 1 - n8 * i, n >> 4, a, 16);
    }
    let mut l = 2;
    while l < (ld - 3) >> 1 {
        let k0 = n >> (l + 2);
        let k0_2 = k0 >> 1;
        let lim = 1 << (l + 1);
        for i in 0..lim {
            step3_inner_r_loop(n >> (l + 4), u, n2 - 1 - k0 * i, k0_2, a, 1 << (l + 3));
        }
        l += 1;
    }
    while l < ld - 6 {
        let (k0, k1) = (n >> (l + 2), 1 << (l + 3));
        let k0_2 = k0 >> 1;
        let rlim = n >> (l + 6);
        let lim = 1 << (l + 1);
        let mut a0 = 0;
        let mut i_off = n2 - 1;
        for _ in 0..rlim {
            step3_inner_s_loop(lim, u, i_off, k0_2, &a[a0..], k1, k0);
            a0 += k1 * 4;
            i_off = i_off.wrapping_sub(8);
        }
        l += 1;
    }
    // iterations ld-6, -5 and -4 together
    step3_inner_s_loop_ld654(n >> 5, u, n2 - 1, a, n);

    // steps 4, 5 and 6 (paper output is u, now v): `d0` and `d1` run down
    // the halves of `v` in fours
    {
        let bitrev = &t.bit_reverse[..n8];
        let (v_lo, v_hi) = v.split_at_mut(n4);
        let outs = v_lo
            .as_rchunks_mut::<4>()
            .1
            .iter_mut()
            .rev()
            .zip(v_hi.as_rchunks_mut::<4>().1.iter_mut().rev());
        for (br, (d0, d1)) in bitrev.as_chunks::<2>().0.iter().zip(outs) {
            let k4 = usize::from(br[0]);
            let s = &u[k4..k4 + 4];
            d1[3] = s[0];
            d1[2] = s[1];
            d0[3] = s[2];
            d0[2] = s[3];
            let k4 = usize::from(br[1]);
            let s = &u[k4..k4 + 4];
            d1[1] = s[0];
            d1[0] = s[1];
            d0[1] = s[2];
            d0[0] = s[3];
        }
    }

    // step 7 (paper output is v, now v), in place: `d` up the first half,
    // `e` down the second
    {
        let (v_lo, v_hi) = v.split_at_mut(n4);
        let quads = v_lo
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(v_hi.as_rchunks_mut::<4>().1.iter_mut().rev());
        for (c, (d, e)) in t.c[..n4].as_chunks::<4>().0.iter().zip(quads) {
            let a02 = d[0] - e[2];
            let a11 = d[1] + e[3];
            let b0 = c[1] * a02 + c[0] * a11;
            let b1 = c[1] * a11 - c[0] * a02;
            let b2 = d[0] + e[2];
            let b3 = d[1] - e[3];
            d[0] = b2 + b0;
            d[1] = b3 + b1;
            e[2] = b2 - b0;
            e[3] = b1 - b3;

            let a02 = d[2] - e[0];
            let a11 = d[3] + e[1];
            let b0 = c[3] * a02 + c[2] * a11;
            let b1 = c[3] * a11 - c[2] * a02;
            let b2 = d[2] + e[0];
            let b3 = d[3] - e[1];
            d[2] = b2 + b0;
            d[3] = b3 + b1;
            e[0] = b2 - b0;
            e[1] = b1 - b3;
        }
    }

    // step 8 and decode (paper output is X, now buffer): `e` and `B` run
    // down in eights; `d0` up the first quarter of `buffer`, `d1` down the
    // second, `d2` up the third and `d3` down the fourth
    {
        let (half0, half1) = u.split_at_mut(n2);
        let (q0, q1) = half0.split_at_mut(n4);
        let (q2, q3) = half1.split_at_mut(n4);
        let ins = v
            .as_rchunks::<8>()
            .1
            .iter()
            .rev()
            .zip(t.b[..n2].as_rchunks::<8>().1.iter().rev());
        let low = q0
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(q1.as_rchunks_mut::<4>().1.iter_mut().rev());
        let high = q2
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(q3.as_rchunks_mut::<4>().1.iter_mut().rev());
        for ((e, b), ((d0, d1), (d2, d3))) in ins.zip(low.zip(high)) {
            let p3 = e[6] * b[7] - e[7] * b[6];
            let p2 = -e[6] * b[6] - e[7] * b[7];
            d0[0] = p3;
            d1[3] = -p3;
            d2[0] = p2;
            d3[3] = p2;

            let p1 = e[4] * b[5] - e[5] * b[4];
            let p0 = -e[4] * b[4] - e[5] * b[5];
            d0[1] = p1;
            d1[2] = -p1;
            d2[1] = p0;
            d3[2] = p0;

            let p3 = e[2] * b[3] - e[3] * b[2];
            let p2 = -e[2] * b[2] - e[3] * b[3];
            d0[2] = p3;
            d1[1] = -p3;
            d2[2] = p2;
            d3[1] = p2;

            let p1 = e[0] * b[1] - e[1] * b[0];
            let p0 = -e[0] * b[0] - e[1] * b[1];
            d0[3] = p1;
            d1[0] = -p1;
            d2[3] = p0;
            d3[0] = p0;
        }
    }
}
