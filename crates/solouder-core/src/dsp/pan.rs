// Ported from SoLoud src/core/soloud.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

const MAX: usize = crate::contracts::MAX_CHANNELS as usize;

type Frame = [f32; MAX];

/// Speaker gains of one voice for one mix call: where the ramp starts, and
/// the per-speaker volume (`mChannelVolume`) and overall volume it ends at.
pub(crate) struct Gains<'a> {
    /// `mCurrentChannelVolume`: updated to the end of the ramp.
    pub current: &'a mut Frame,
    pub channel_volume: &'a Frame,
    pub overall_volume: f32,
}

/// Mixes one voice's resampled planar `scratch` (`source_channels` channels)
/// into the planar accumulator `out` (`channels` channels), both with
/// `stride` frames per channel, ramping each speaker's gain linearly over
/// `frames` (SoLoud's `panAndExpand`).
///
/// Channel layouts follow SoLoud: 1 mono, 2 stereo, 4 quad, 6 for 5.1 (FL, FR,
/// C, LFE, RL, RR) and 8 for 7.1 (adds side L/R before rear L/R). A source with
/// 3, 5 or 7 channels is only heard on a mono output, as in SoLoud.
pub(crate) fn pan_and_expand(
    gains: Gains<'_>,
    source_channels: usize,
    out: &mut [f32],
    frames: usize,
    stride: usize,
    scratch: &[f32],
    channels: usize,
) {
    let mut pan = [0.0; MAX];
    let mut target = [0.0; MAX];
    let mut step = [0.0; MAX];
    for k in 0..channels {
        pan[k] = gains.current[k];
        target[k] = gains.channel_volume[k] * gains.overall_volume;
        step[k] = (target[k] - pan[k]) / frames as f32;
    }

    if channels == 1 {
        // Sum every source channel, each with its own ramp.
        for c in 0..source_channels {
            let mut gain = gains.current[0];
            let src = &scratch[c * stride..c * stride + frames];
            for (out, &s) in out[..frames].iter_mut().zip(src) {
                gain += step[0];
                *out += s * gain;
            }
        }
    } else if channels == 2 && source_channels <= 2 {
        stereo_quads(
            &mut pan,
            &step,
            source_channels,
            out,
            frames,
            stride,
            scratch,
        );
    } else if let Some(weights) = weights(source_channels, channels) {
        let mut frame = [0.0; MAX];
        for j in 0..frames {
            for k in 0..channels {
                pan[k] += step[k];
            }
            for (c, sample) in frame.iter_mut().enumerate().take(source_channels) {
                *sample = scratch[c * stride + j];
            }
            let w = weights(&frame);
            for k in 0..channels {
                out[k * stride + j] += w[k] * pan[k];
            }
        }
    }

    gains.current[..channels].copy_from_slice(&target[..channels]);
}

/// 1->2 and 2->2, which SoLoud runs four frames at a time with SSE: lane `l`
/// of the first group starts at `pan + step * (l + 1)` and every group adds
/// `step * 4`. Frames after the last whole group continue the ramp one frame
/// at a time (reference patch 0018).
fn stereo_quads(
    pan: &mut Frame,
    step: &Frame,
    source_channels: usize,
    out: &mut [f32],
    frames: usize,
    stride: usize,
    scratch: &[f32],
) {
    let quads = frames / 4;
    let mut lanes = [[0.0_f32; 4]; 2];
    for (ch, lane) in lanes.iter_mut().enumerate() {
        for (l, gain) in lane.iter_mut().enumerate() {
            *gain = pan[ch] + step[ch] * (l + 1) as f32;
        }
    }
    let quad_step = [step[0] * 4.0, step[1] * 4.0];
    for q in 0..quads {
        for (ch, lane) in lanes.iter_mut().enumerate() {
            let src = if source_channels == 2 { ch } else { 0 };
            for (l, gain) in lane.iter_mut().enumerate() {
                let j = q * 4 + l;
                out[ch * stride + j] += scratch[src * stride + j] * *gain;
                *gain += quad_step[ch];
            }
        }
    }
    let done = quads * 4;
    pan[0] += step[0] * done as f32;
    pan[1] += step[1] * done as f32;
    for j in done..frames {
        pan[0] += step[0];
        pan[1] += step[1];
        let s1 = scratch[j];
        let s2 = if source_channels == 2 {
            scratch[stride + j]
        } else {
            s1
        };
        out[j] += s1 * pan[0];
        out[stride + j] += s2 * pan[1];
    }
}

/// Per-speaker signal for each supported (source, output) channel pair other
/// than mono output and 1/2 -> 2. Each entry is the expression SoLoud
/// multiplies by the speaker gain, with its operation order preserved.
fn weights(source_channels: usize, channels: usize) -> Option<fn(&Frame) -> Frame> {
    let f: fn(&Frame) -> Frame = match (source_channels, channels) {
        (8, 2) => |s| {
            let left = 0.2 * (s[0] + s[2] + s[3] + s[4] + s[6]);
            let right = 0.2 * (s[1] + s[2] + s[3] + s[5] + s[7]);
            expand(&[left, right])
        },
        (6, 2) => |s| {
            let left = 0.3 * (s[0] + s[2] + s[3] + s[4]);
            let right = 0.3 * (s[1] + s[2] + s[3] + s[5]);
            expand(&[left, right])
        },
        (4, 2) => |s| expand(&[0.5 * (s[0] + s[2]), 0.5 * (s[1] + s[3])]),
        (8, 4) => |s| {
            let c = (s[2] + s[3]) * 0.7;
            expand(&[s[0] + c, s[1] + c, 0.5 * (s[4] + s[6]), 0.5 * (s[5] + s[7])])
        },
        (6, 4) => |s| {
            let c = (s[2] + s[3]) * 0.7;
            expand(&[s[0] + c, s[1] + c, s[4], s[5]])
        },
        (2, 4) => |s| expand(&[s[0], s[1], s[0], s[1]]),
        (8, 6) => |s| {
            expand(&[
                s[0],
                s[1],
                s[2],
                s[3],
                0.5 * (s[4] + s[6]),
                0.5 * (s[5] + s[7]),
            ])
        },
        (4, 6) => |s| {
            let center = 0.5 * (s[0] + s[1]);
            let lfe = 0.25 * (s[0] + s[1] + s[2] + s[3]);
            expand(&[s[0], s[1], center, lfe, s[2], s[3]])
        },
        (2, 6) => |s| {
            let mid = 0.5 * (s[0] + s[1]);
            expand(&[s[0], s[1], mid, mid, s[0], s[1]])
        },
        (6, 8) => |s| {
            let side_l = 0.5 * (s[4] + s[0]);
            let side_r = 0.5 * (s[5] + s[1]);
            expand(&[s[0], s[1], s[2], s[3], side_l, side_r, s[4], s[5]])
        },
        (4, 8) => |s| {
            let center = 0.5 * (s[0] + s[1]);
            let lfe = 0.25 * (s[0] + s[1] + s[2] + s[3]);
            let (side_l, side_r) = (0.5 * (s[0] + s[2]), 0.5 * (s[1] + s[3]));
            expand(&[s[0], s[1], center, lfe, side_l, side_r, s[2], s[3]])
        },
        (2, 8) => |s| {
            let mid = 0.5 * (s[0] + s[1]);
            expand(&[s[0], s[1], mid, mid, s[0], s[1], s[0], s[1]])
        },
        (1, 4 | 6 | 8) => |s| [s[0]; MAX],
        (a, b) if a == b && matches!(a, 4 | 6 | 8) => |s| *s,
        _ => return None,
    };
    Some(f)
}

fn expand(values: &[f32]) -> Frame {
    let mut frame = [0.0; MAX];
    frame[..values.len()].copy_from_slice(values);
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        current: &mut Frame,
        volume: f32,
        source_channels: usize,
        channels: usize,
        frames: usize,
        stride: usize,
    ) -> Vec<f32> {
        let scratch = vec![1.0; source_channels * stride];
        let mut out = vec![0.0; channels * stride];
        let gains = Gains {
            current,
            channel_volume: &[1.0; MAX],
            overall_volume: volume,
        };
        pan_and_expand(
            gains,
            source_channels,
            &mut out,
            frames,
            stride,
            &scratch,
            channels,
        );
        out
    }

    #[test]
    fn stereo_ramp_continues_through_leftover_frames() {
        for source_channels in [1, 2] {
            let mut current = [0.0; MAX];
            let out = run(&mut current, 1.0, source_channels, 2, 6, 16);
            for j in 0..6 {
                let expected = (j + 1) as f32 / 6.0;
                assert!(
                    (out[j] - expected).abs() < 1e-6,
                    "{source_channels}->2 frame {j}"
                );
                assert!((out[16 + j] - expected).abs() < 1e-6);
            }
            assert_eq!(current[..2], [1.0, 1.0]);
        }
    }

    #[test]
    fn silent_voice_adds_nothing_for_every_layout() {
        for source_channels in [1, 2, 4, 6, 8] {
            for channels in [1, 2, 4, 6, 8] {
                let out = run(&mut [0.0; MAX], 0.0, source_channels, channels, 7, 16);
                assert!(
                    out.iter().all(|&s| s == 0.0),
                    "{source_channels}->{channels}"
                );
            }
        }
    }

    #[test]
    fn every_supported_layout_reaches_each_speaker() {
        for source_channels in [1, 2, 4, 6, 8] {
            for channels in [2, 4, 6, 8] {
                let out = run(&mut [1.0; MAX], 1.0, source_channels, channels, 4, 16);
                for k in 0..channels {
                    assert!(
                        out[k * 16 + 3] > 0.0,
                        "{source_channels}->{channels} speaker {k}"
                    );
                }
            }
        }
    }

    #[test]
    fn mono_output_sums_source_channels() {
        let out = run(&mut [1.0; MAX], 1.0, 4, 1, 3, 16);
        assert_eq!(out[..3], [4.0, 4.0, 4.0]);
    }
}
