//! Deterministic test signals for the `sound` operation. Both runners compute
//! them with the same single-precision operations, so the samples are
//! bit-identical.

use crate::scenario::{ScenarioError, Step};

/// Largest `frames * channels` a `sound` step may request.
pub const MAX_SOUND_SAMPLES: u32 = 1 << 22;

/// A signal shape and its parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pattern {
    /// Every sample is `amp`.
    Dc,
    /// Rising ramp from -1 to just under 1 every `period` frames; channel `c`
    /// is shifted by `7 * c` frames.
    Saw {
        /// Frames per ramp; at least 1.
        period: u32,
    },
    /// xorshift32 noise in `[-1, 1)`, generated channel by channel.
    Noise {
        /// Initial xorshift32 state; at least 1.
        seed: u32,
    },
}

impl Pattern {
    /// Reads `pattern=`, `period=` and `seed=` from `step`.
    pub fn from_step(step: &Step) -> Result<Self, ScenarioError> {
        let pattern = match step.get("pattern").unwrap_or("dc") {
            "dc" => Self::Dc,
            "saw" => Self::Saw {
                period: step.value_or("period", 100)?,
            },
            "noise" => Self::Noise {
                seed: step.value_or("seed", 1)?,
            },
            other => return Err(step.error(format!("unknown pattern `{other}`"))),
        };
        match pattern {
            Self::Saw { period: 0 } => Err(step.error("`period` must be at least 1")),
            Self::Noise { seed: 0 } => Err(step.error("`seed` must be at least 1")),
            _ => Ok(pattern),
        }
    }

    /// Planar samples: `frames` per channel, channel after channel.
    pub fn render(self, channels: u32, frames: u32, amp: f32) -> Vec<f32> {
        let mut samples = Vec::with_capacity((channels * frames) as usize);
        let mut state = match self {
            Self::Noise { seed } => seed,
            _ => 0,
        };
        for c in 0..channels {
            for f in 0..frames {
                let value = match self {
                    Self::Dc => 1.0,
                    Self::Saw { period } => {
                        ((f + c * 7) % period) as f32 / period as f32 * 2.0 - 1.0
                    }
                    Self::Noise { .. } => {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        (state >> 8) as f32 / 8_388_608.0 - 1.0
                    }
                };
                samples.push(value * amp);
            }
        }
        samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_planar_and_bounded() {
        let saw = Pattern::Saw { period: 4 }.render(2, 4, 1.0);
        assert_eq!(saw, [-1.0, -0.5, 0.0, 0.5, 0.5, -1.0, -0.5, 0.0]);
        let noise = Pattern::Noise { seed: 1 }.render(1, 1000, 0.5);
        assert!(noise.iter().all(|&s| (-0.5..0.5).contains(&s)));
        assert_eq!(Pattern::Dc.render(1, 2, 0.25), [0.25, 0.25]);
    }
}
