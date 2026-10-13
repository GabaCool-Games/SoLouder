// Ported from dr_flac 0.12.31 by David Reid (public domain or MIT No
// Attribution), as patched for SoLouder's C++ reference; altered for SoLouder.

//! FLAC subframes: constant, verbatim, fixed and LPC prediction, and Rice
//! coded residuals (`drflac__decode_subframe` and what it calls), in the
//! integer arithmetic dr_flac uses, wrapping where it wraps.

use std::io::{Read, Seek};

use super::bits::Bits;
use super::frame::{Header, LEFT_SIDE, MID_SIDE, RIGHT_SIDE};

const FIXED: [[i32; 4]; 5] = [
    [0, 0, 0, 0],
    [1, 0, 0, 0],
    [2, -1, 0, 0],
    [3, -3, 1, 0],
    [4, -6, 4, -1],
];

/// Decodes subframe `channel` of a frame into `out`, which holds its block,
/// and returns its wasted bits. `None` if the subframe is invalid or the
/// data ends; dr_flac ignores some of these failures and plays what it has,
/// but the frame is dropped here.
pub(super) fn decode<R: Read + Seek>(
    bits: &mut Bits<R>,
    header: &Header,
    channel: usize,
    out: &mut [i32],
) -> Option<u32> {
    let kind = bits.read(8)?;
    if kind & 0x80 != 0 {
        return None;
    }
    let wasted = if kind & 1 == 1 {
        // dr_flac stores the count in 8 bits.
        u32::from((bits.unary()? as u8).wrapping_add(1))
    } else {
        0
    };
    let side = matches!(
        (header.assignment, channel),
        (LEFT_SIDE | MID_SIDE, 1) | (RIGHT_SIDE, 0)
    );
    let sample_bits = header.sample_bits + u32::from(side);
    if wasted >= sample_bits {
        return None;
    }
    let sample_bits = sample_bits - wasted;
    let kind = (kind & 0x7E) >> 1;
    match kind {
        0 => {
            let sample = bits.read_signed(sample_bits)?;
            out.fill(sample);
        }
        1 => {
            for sample in out.iter_mut() {
                *sample = bits.read_signed(sample_bits)?;
            }
        }
        // dr_flac reads the reserved types 24 to 28 as fixed prediction too.
        _ if kind & 0x20 == 0 && kind & 0x08 != 0 && kind & 7 <= 4 => {
            let order = (kind & 7) as usize;
            warm_up(bits, sample_bits, out.get_mut(..order)?)?;
            residual(bits, sample_bits, &FIXED[order][..order], 0, 4, out)?;
        }
        _ if kind & 0x20 != 0 => {
            let order = (kind & 0x1F) as usize + 1;
            warm_up(bits, sample_bits, out.get_mut(..order)?)?;
            let precision = bits.read(4)?;
            if precision == 15 {
                return None;
            }
            let precision = precision + 1;
            let shift = bits.read_signed(5)?;
            // Negative shifts are valid FLAC; dr_flac does not decode them.
            let shift = u32::try_from(shift).ok()?;
            let mut coefficients = [0; 32];
            for coefficient in &mut coefficients[..order] {
                *coefficient = bits.read_signed(precision)?;
            }
            residual(
                bits,
                sample_bits,
                &coefficients[..order],
                shift,
                precision,
                out,
            )?;
        }
        _ => return None,
    }
    Some(wasted)
}

fn warm_up<R: Read + Seek>(bits: &mut Bits<R>, sample_bits: u32, out: &mut [i32]) -> Option<()> {
    for sample in out {
        *sample = bits.read_signed(sample_bits)?;
    }
    Some(())
}

/// Whether a prediction is summed in 64 bits (patch 0057, from dr_flac
/// 0.12.35): when the sample size, coefficient precision and the bits of the
/// order add up to more than 32.
pub(super) fn wide_prediction(sample_bits: u32, order: usize, precision: u32) -> bool {
    let order_bits = usize::BITS - order.leading_zeros();
    sample_bits + precision + order_bits > 32
}

/// The prediction for the sample after `history`, whose last
/// `coefficients.len()` samples it weighs, the nearest first.
#[inline]
pub(super) fn predict(coefficients: &[i32], history: &[i32], shift: u32, wide: bool) -> i32 {
    let past = &history[history.len() - coefficients.len()..];
    if wide {
        let mut sum = 0_i64;
        for (&c, &s) in coefficients.iter().zip(past.iter().rev()) {
            sum += i64::from(c) * i64::from(s);
        }
        (sum >> shift) as i32
    } else {
        let mut sum = 0_i32;
        for (&c, &s) in coefficients.iter().zip(past.iter().rev()) {
            sum = sum.wrapping_add(c.wrapping_mul(s));
        }
        sum >> shift
    }
}

/// Adds to each residual in `out` from `start` on the prediction from the
/// samples before it, as [`predict`] does, unrolled for each order.
fn add_predictions(coefficients: &[i32], out: &mut [i32], start: usize, shift: u32, wide: bool) {
    macro_rules! orders {
        ($($n:literal)*) => {
            match coefficients.len() {
                $($n => predict_run::<$n>(coefficients, out, start, shift, wide),)*
                _ => {}
            }
        };
    }
    orders!(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32);
}

fn predict_run<const N: usize>(
    coefficients: &[i32],
    out: &mut [i32],
    start: usize,
    shift: u32,
    wide: bool,
) {
    let mut reversed = [0; N];
    for (r, &c) in reversed.iter_mut().zip(coefficients.iter().rev()) {
        *r = c;
    }
    for i in start.max(N)..out.len() {
        let Ok(window) = <&[i32; N]>::try_from(&out[i - N..i]) else {
            return;
        };
        let prediction = if wide {
            let mut sum = 0_i64;
            for k in 0..N {
                sum += i64::from(reversed[k]) * i64::from(window[k]);
            }
            (sum >> shift) as i32
        } else {
            let mut sum = 0_i32;
            for k in 0..N {
                sum = sum.wrapping_add(reversed[k].wrapping_mul(window[k]));
            }
            sum >> shift
        };
        out[i] = out[i].wrapping_add(prediction);
    }
}

/// Decodes the residual after the warm-up samples of `out` and adds the
/// prediction from `coefficients` to each sample.
fn residual<R: Read + Seek>(
    bits: &mut Bits<R>,
    sample_bits: u32,
    coefficients: &[i32],
    shift: u32,
    precision: u32,
    out: &mut [i32],
) -> Option<()> {
    let method = bits.read(2)?;
    if method > 1 {
        return None;
    }
    let order = coefficients.len();
    let partition_order = bits.read(4)?;
    if partition_order > 8 {
        return None;
    }
    let partitions = 1_usize << partition_order;
    let per_partition = out.len() >> partition_order;
    // dr_flac leaves the samples past the last whole partition as they were.
    if per_partition < order || !out.len().is_multiple_of(partitions) {
        return None;
    }
    let wide = wide_prediction(sample_bits, order, precision);
    let (param_bits, escape) = if method == 0 { (4, 15) } else { (5, 31) };
    let mut at = order;
    for partition in 0..partitions {
        let end = (partition + 1) * per_partition;
        let param = bits.read(param_bits)?;
        if param == escape {
            let raw_bits = bits.read(5)?;
            for i in at..end {
                let raw = bits.read_signed(raw_bits)?;
                out[i] = raw.wrapping_add(predict(coefficients, &out[..i], shift, wide));
            }
        } else {
            bits.rice_run(param, &mut out[at..end])?;
            if order > 0 {
                add_predictions(coefficients, &mut out[..end], at, shift, wide);
            }
        }
        at = end;
    }
    Some(())
}
