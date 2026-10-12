// Ported from SoLoud src/core/soloud_misc.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! SoLoud's basic waveforms (`Soloud::WAVEFORM`, `Misc::generateWaveform`).

use std::f64::consts::PI;

/// Square wave; also used for indices without a waveform.
pub const SQUARE: i32 = 0;
/// Rising saw.
pub const SAW: i32 = 1;
/// Sine.
pub const SIN: i32 = 2;
/// Triangle.
pub const TRIANGLE: i32 = 3;
/// Rectified sine.
pub const BOUNCE: i32 = 4;
/// The first quarter of a sine.
pub const JAWS: i32 = 5;
/// The first half of a sine.
pub const HUMPS: i32 = 6;
/// Square from the first 11 odd harmonics.
pub const FSQUARE: i32 = 7;
/// Saw from the first 14 harmonics.
pub const FSAW: i32 = 8;

fn sine(p: f32) -> f32 {
    (f64::from(p) * PI * 2.0).sin() as f32 * 0.5
}

/// The value of `waveform` at phase `p`, from 0 to 1, roughly in -0.5 to
/// 0.5 (`Misc::generateWaveform`).
pub fn generate(waveform: i32, p: f32) -> f32 {
    match waveform {
        SAW => p - 0.5,
        SIN => sine(p),
        TRIANGLE => {
            (if p > 0.5 {
                1.0 - (p - 0.5) * 2.0
            } else {
                p * 2.0
            }) - 0.5
        }
        BOUNCE => (if p < 0.5 { sine(p) } else { -sine(p) }) - 0.5,
        JAWS => (if p < 0.25 { sine(p) } else { 0.0 }) - 0.5,
        HUMPS => (if p < 0.5 { sine(p) } else { 0.0 }) - 0.5,
        FSQUARE => {
            let mut f = 0.0_f32;
            for i in (1..22).step_by(2) {
                let i = f64::from(i);
                f += (4.0 / (PI * i)) as f32 * (2.0 * PI * i * f64::from(p)).sin() as f32;
            }
            f * 0.5
        }
        FSAW => {
            let mut f = 0.0_f32;
            for i in 1..15 {
                let harmonic = f64::from(i);
                let term = (1.0 / (PI * harmonic)) as f32
                    * (f64::from(p * 2.0) * PI * harmonic).sin() as f32;
                if i & 1 != 0 {
                    f += term;
                } else {
                    f -= term;
                }
            }
            f
        }
        _ => {
            if p > 0.5 {
                0.5
            } else {
                -0.5
            }
        }
    }
}
