// Ported from SoLoud src/core/soloud.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use crate::contracts::SAMPLE_GRANULARITY;

/// Fractional bits of the fixed-point source position.
pub(crate) const FRAC_BITS: u32 = 20;
/// `1 << FRAC_BITS`: one source frame in fixed point.
pub(crate) const FRAC_MUL: u32 = 1 << FRAC_BITS;
const FRAC_MASK: u32 = FRAC_MUL - 1;
const BLOCK: usize = SAMPLE_GRANULARITY as usize;

/// How source frames are interpolated to the output rate
/// (`Soloud::setMainResampler`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Resampler {
    /// Nearest earlier frame (`RESAMPLER_POINT`).
    Point,
    /// Linear interpolation (`RESAMPLER_LINEAR`), SoLoud's default.
    #[default]
    Linear,
    /// Catmull-Rom spline (`RESAMPLER_CATMULLROM`).
    CatmullRom,
}

impl Resampler {
    /// Renders `dst.len()` frames from one channel of the current block `src`,
    /// starting at fixed-point position `offset` and advancing by `step` per
    /// frame. `prev` is the same channel of the previous block, which supplies
    /// the frames before position 0.
    ///
    /// The caller keeps every position below `SAMPLE_GRANULARITY << FRAC_BITS`.
    pub(crate) fn run(self, src: &[f32], prev: &[f32], dst: &mut [f32], offset: u32, step: u32) {
        let mut pos = offset;
        for out in dst.iter_mut() {
            let p = (pos >> FRAC_BITS) as usize;
            let f = pos & FRAC_MASK;
            *out = match self {
                Self::Point => src[p],
                Self::Linear => {
                    let s1 = if p != 0 { src[p - 1] } else { prev[BLOCK - 1] };
                    let s2 = src[p];
                    s1 + (s2 - s1) * f as f32 * (1.0 / FRAC_MUL as f32)
                }
                Self::CatmullRom => {
                    let before = |back: usize| {
                        if p < back {
                            prev[BLOCK + p - back]
                        } else {
                            src[p - back]
                        }
                    };
                    catmull_rom(
                        f as f32 / FRAC_MUL as f32,
                        before(3),
                        before(2),
                        before(1),
                        src[p],
                    )
                }
            };
            pos += step;
        }
    }
}

fn catmull_rom(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(start: f32) -> Vec<f32> {
        (0..BLOCK).map(|i| start + i as f32).collect()
    }

    #[test]
    fn unit_step_linear_lags_one_frame() {
        let (src, prev) = (ramp(512.0), ramp(0.0));
        let mut dst = [0.0; 4];
        Resampler::Linear.run(&src, &prev, &mut dst, 0, FRAC_MUL);
        assert_eq!(dst, [511.0, 512.0, 513.0, 514.0]);
    }

    #[test]
    fn half_step_linear_interpolates() {
        let (src, prev) = (ramp(512.0), ramp(0.0));
        let mut dst = [0.0; 3];
        Resampler::Linear.run(&src, &prev, &mut dst, FRAC_MUL, FRAC_MUL / 2);
        assert_eq!(dst, [512.0, 512.5, 513.0]);
    }

    #[test]
    fn point_and_catmull_rom_on_a_line() {
        let (src, prev) = (ramp(512.0), ramp(0.0));
        let mut dst = [0.0; 3];
        Resampler::Point.run(&src, &prev, &mut dst, FRAC_MUL / 2, FRAC_MUL);
        assert_eq!(dst, [512.0, 513.0, 514.0]);
        // Catmull-Rom reproduces a straight line exactly, lagging two frames.
        Resampler::CatmullRom.run(&src, &prev, &mut dst, 0, FRAC_MUL / 2);
        assert_eq!(dst, [510.0, 510.5, 511.0]);
    }
}
