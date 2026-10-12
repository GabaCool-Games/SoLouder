//! SoLoud's in-place complex FFT (`SoLoud::FFT`), a port of Takuya Ooura's
//! split-radix kernels with the same operation order, so results match the
//! C++ reference bit for bit.
//!
//! A buffer of `n` floats holds `n / 2` interleaved complex values.

// Ported from SoLoud src/core/soloud_fft.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa), which is based on Takuya Ooura's FFT
// package (Copyright Takuya OOURA, 1996-2001); altered for SoLouder.
//
// Ooura's licence: "Copyright Takuya OOURA, 1996-2001. You may use, copy,
// modify and distribute this code for any purpose (include commercial use)
// and without fee. Please refer to this package when you modify this code."

mod first;
mod leaf16a;
mod leaf16b;
mod leaf8;
mod middle1;
mod middle2;
#[cfg(test)]
mod tests;

use first::cftb1st;
use leaf8::{cftb040, cftf040, cftf081, cftf082, cftx020};
use leaf16a::cftf161;
use leaf16b::cftf162;
use middle1::cftmdl1;
use middle2::cftmdl2;

const M_PI_2: f32 = std::f32::consts::FRAC_PI_2;
const WR5000: f32 = std::f32::consts::FRAC_1_SQRT_2;
#[allow(clippy::excessive_precision)]
const WR2500: f32 = 0.923_879_532_511_286_756_128_183_189_396_788_286_822_416_625_863;
#[allow(clippy::excessive_precision)]
const WI2500: f32 = 0.382_683_432_365_089_771_728_459_984_030_398_866_761_344_562_485;
#[allow(clippy::excessive_precision)]
const WR1250: f32 = 0.980_785_280_403_230_449_126_182_236_134_239_036_973_933_730_893;
#[allow(clippy::excessive_precision)]
const WI1250: f32 = 0.195_090_322_016_128_267_848_284_868_477_022_240_927_691_617_751;
#[allow(clippy::excessive_precision)]
const WR3750: f32 = 0.831_469_612_302_545_237_078_788_377_617_905_756_738_560_811_987;
#[allow(clippy::excessive_precision)]
const WI3750: f32 = 0.555_570_233_019_602_224_742_830_813_948_532_874_374_937_190_754;
const CDFT_LOOP_DIV: isize = 32;

/// Forward transform of `buffer` in place (`FFT::fft`).
///
/// # Panics
///
/// If the length is not a power of two of at least 4.
pub fn fft(buffer: &mut [f32]) {
    check_len(buffer.len());
    cftbsub(buffer);
}

/// Inverse transform of `buffer` in place, scaled by `2 / len`
/// (`FFT::ifft`).
///
/// # Panics
///
/// If the length is not a power of two of at least 4.
pub fn ifft(buffer: &mut [f32]) {
    check_len(buffer.len());
    cftfsub(buffer);
    let len = buffer.len();
    for value in buffer.iter_mut() {
        *value *= 1.0 / (len / 2) as f32;
    }
}

fn check_len(len: usize) {
    assert!(
        len >= 4 && len.is_power_of_two(),
        "FFT length {len} is not a power of two of at least 4"
    );
}

fn as_isize(n: usize) -> isize {
    isize::try_from(n).expect("FFT length fits isize")
}

fn cftfsub(a: &mut [f32]) {
    let n = a.len();
    if n > 32 {
        cftmdl1(as_isize(n), a);
        cft_core(a);
        bit_reverse(a, false);
    } else if n == 32 {
        cftf161(a);
        bit_reverse(a, false);
    } else if n == 16 {
        cftf081(a);
        bit_reverse(a, false);
    } else if n == 8 {
        cftf040(a);
    } else {
        cftx020(a);
    }
}

fn cftbsub(a: &mut [f32]) {
    let n = a.len();
    if n > 32 {
        cftb1st(as_isize(n), a);
        cft_core(a);
        bit_reverse(a, true);
    } else if n == 32 {
        cftf161(a);
        bit_reverse_negated(a);
    } else if n == 16 {
        cftf081(a);
        bit_reverse_negated(a);
    } else if n == 8 {
        cftb040(a);
    } else {
        cftx020(a);
    }
}

fn cft_core(a: &mut [f32]) {
    let n = a.len();
    if n > 512 {
        cftrec4(a);
    } else if n > 128 {
        cftleaf(n, true, a);
    } else {
        cftfx41(a);
    }
}

fn cftfx41(a: &mut [f32]) {
    if a.len() == 128 {
        cftf161(a);
        cftf162(&mut a[32..]);
        cftf161(&mut a[64..]);
        cftf161(&mut a[96..]);
    } else {
        cftf081(a);
        cftf082(&mut a[16..]);
        cftf081(&mut a[32..]);
        cftf081(&mut a[48..]);
    }
}

fn cfttree(n: usize, j: usize, k: usize, a: &mut [f32]) -> bool {
    if k & 3 != 0 {
        let odd = k & 1 != 0;
        if odd {
            cftmdl1(as_isize(n), &mut a[j - n..]);
        } else {
            cftmdl2(as_isize(n), &mut a[j - n..]);
        }
        return odd;
    }
    let mut m = n;
    let mut i = k;
    while i & 3 == 0 {
        m <<= 2;
        i >>= 2;
    }
    let odd = i & 1 != 0;
    while m > 128 {
        if odd {
            cftmdl1(as_isize(m), &mut a[j - m..]);
        } else {
            cftmdl2(as_isize(m), &mut a[j - m..]);
        }
        m >>= 2;
    }
    odd
}

fn cftleaf(n: usize, split: bool, a: &mut [f32]) {
    type Leaf = fn(&mut [f32]);
    let (size, step, one, two): (isize, usize, Leaf, Leaf) = if n == 512 {
        (128, 32, cftf161, cftf162)
    } else {
        (64, 16, cftf081, cftf082)
    };
    let quarter = 4 * step;
    cftmdl1(size, a);
    one(a);
    two(&mut a[step..]);
    one(&mut a[2 * step..]);
    one(&mut a[3 * step..]);
    cftmdl2(size, &mut a[quarter..]);
    one(&mut a[quarter..]);
    two(&mut a[quarter + step..]);
    one(&mut a[quarter + 2 * step..]);
    two(&mut a[quarter + 3 * step..]);
    cftmdl1(size, &mut a[2 * quarter..]);
    one(&mut a[2 * quarter..]);
    two(&mut a[2 * quarter + step..]);
    one(&mut a[2 * quarter + 2 * step..]);
    one(&mut a[2 * quarter + 3 * step..]);
    if split {
        cftmdl1(size, &mut a[3 * quarter..]);
        one(&mut a[3 * quarter + 3 * step..]);
    } else {
        cftmdl2(size, &mut a[3 * quarter..]);
        two(&mut a[3 * quarter + 3 * step..]);
    }
    one(&mut a[3 * quarter..]);
    two(&mut a[3 * quarter + step..]);
    one(&mut a[3 * quarter + 2 * step..]);
}

fn cftrec4(a: &mut [f32]) {
    let n = a.len();
    let mut m = n;
    while m > 512 {
        m >>= 2;
        cftmdl1(as_isize(m), &mut a[n - m..]);
    }
    cftleaf(m, true, &mut a[n - m..]);
    let mut k = 0;
    let mut j = n - m;
    while j > 0 {
        k += 1;
        let split = cfttree(m, j, k, a);
        cftleaf(m, split, &mut a[j - m..]);
        j -= m;
    }
}

fn reversed(index: usize, bits: u32) -> usize {
    index.reverse_bits() >> (usize::BITS - bits)
}

/// Complex values to bit-reversed order (`bitrv2`, `bitrv216`, `bitrv208`),
/// negating every imaginary part when `conjugate` (`bitrv2conj`).
fn bit_reverse(a: &mut [f32], conjugate: bool) {
    let count = a.len() / 2;
    let bits = count.trailing_zeros();
    for i in 0..count {
        let j = reversed(i, bits);
        if i < j {
            a.swap(2 * i, 2 * j);
            a.swap(2 * i + 1, 2 * j + 1);
        }
    }
    if conjugate {
        for pair in a.as_chunks_mut::<2>().0 {
            pair[1] = -pair[1];
        }
    }
}

/// `bitrv216neg` and `bitrv208neg` (at most 16 complex values): value `k`
/// takes the one at the bit reversal of `count - k`, for `k` from 1.
fn bit_reverse_negated(a: &mut [f32]) {
    let count = a.len() / 2;
    let bits = count.trailing_zeros();
    let mut old = [0.0; 32];
    old[..a.len()].copy_from_slice(a);
    for k in 1..count {
        let from = reversed(count - k, bits);
        a[2 * k] = old[2 * from];
        a[2 * k + 1] = old[2 * from + 1];
    }
}
