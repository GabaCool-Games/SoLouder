// Ported from SoLoud src/core/soloud.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

/// How the final mix is limited to `[-1, 1]` (SoLoud's `CLIP_ROUNDOFF` flag).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clipper {
    /// Soft clip with a cubic curve that reaches about ±0.986 at ±1.65
    /// (`CLIP_ROUNDOFF`, SoLoud's default).
    #[default]
    Roundoff,
    /// Hard clip at ±1 (flags without `CLIP_ROUNDOFF`).
    Hard,
}

/// Applies a linear volume ramp from `volume0` to `volume1`, clips, and scales
/// by `post_scale`, copying `channels` planar channels of `stride` frames from
/// `src` to `dst`.
///
/// The ramp spans the whole stride, not just the frames that will be output,
/// and restarts for every channel, as in SoLoud's `Soloud::clip_internal`.
/// Volumes for each group of four frames are formed the way the SSE code forms
/// them, so rounding matches. `stride` is a multiple of 4.
pub(crate) fn clip(
    clipper: Clipper,
    src: &[f32],
    dst: &mut [f32],
    channels: usize,
    stride: usize,
    (volume0, volume1): (f32, f32),
    post_scale: f32,
) {
    if stride == 0 {
        return;
    }
    let mut delta = (volume1 - volume0) / stride as f32;
    let start = [
        volume0,
        volume0 + delta,
        volume0 + delta + delta,
        volume0 + delta + delta + delta,
    ];
    delta *= 4.0;
    let len = channels * stride;
    for (src, dst) in src[..len]
        .chunks_exact(stride)
        .zip(dst[..len].chunks_exact_mut(stride))
    {
        let mut volume = start;
        let (src_quads, _) = src.as_chunks::<4>();
        let (dst_quads, _) = dst.as_chunks_mut::<4>();
        for (input, output) in src_quads.iter().zip(dst_quads.iter_mut()) {
            for lane in 0..4 {
                let f = input[lane] * volume[lane];
                output[lane] = limit(clipper, f) * post_scale;
                volume[lane] += delta;
            }
        }
    }
}

fn limit(clipper: Clipper, f: f32) -> f32 {
    match clipper {
        Clipper::Roundoff => {
            // NaN fails both comparisons and ends at the positive wall.
            let curve = f * f * f * -0.1 + f * 0.87;
            let low = if f > -1.65 { curve } else { -0.986_287_5 };
            if f < 1.65 { low } else { 0.986_287_5 }
        }
        Clipper::Hard => {
            // maxps/minps return the second operand for NaN.
            let low = if f > -1.0 { f } else { -1.0 };
            if low < 1.0 { low } else { 1.0 }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundoff_curve_and_walls() {
        assert_eq!(limit(Clipper::Roundoff, 0.0), 0.0);
        assert_eq!(limit(Clipper::Roundoff, 2.0), 0.986_287_5);
        assert_eq!(limit(Clipper::Roundoff, -1.65), -0.986_287_5);
        assert_eq!(limit(Clipper::Roundoff, f32::NAN), 0.986_287_5);
        let half = limit(Clipper::Roundoff, 0.5);
        assert!((half - (0.435 - 0.0125)).abs() < 1e-7);
    }

    #[test]
    fn hard_clip_limits_and_maps_nan_to_minus_one() {
        assert_eq!(limit(Clipper::Hard, 3.0), 1.0);
        assert_eq!(limit(Clipper::Hard, -3.0), -1.0);
        assert_eq!(limit(Clipper::Hard, 0.25), 0.25);
        assert_eq!(limit(Clipper::Hard, f32::NAN), -1.0);
    }

    #[test]
    fn volume_ramp_restarts_per_channel() {
        let src = [1.0_f32; 8];
        let mut dst = [0.0; 8];
        clip(Clipper::Hard, &src, &mut dst, 2, 4, (0.0, 1.0), 1.0);
        assert_eq!(dst, [0.0, 0.25, 0.5, 0.75, 0.0, 0.25, 0.5, 0.75]);
    }
}
