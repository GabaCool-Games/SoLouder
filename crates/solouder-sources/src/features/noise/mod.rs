// Ported from SoLoud src/audiosource/noise/soloud_noise.cpp and
// include/soloud_noise.h (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa);
// altered for SoLouder.

//! Noise of a chosen colour (`Noise`): white noise plus nine octaves of
//! lower-rate noise, each with its own weight.

use std::sync::atomic::{AtomicU32, Ordering};

use solouder_core::{Generator, GeneratorContext, GeneratorInstance};

use crate::prg::Prg;

/// Octaves of noise mixed, each at half the rate of the one before.
pub const OCTAVES: usize = 10;

/// Preset octave weights (`Noise::NOISETYPES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseType {
    /// Only the first octave: `1, 0, ...`.
    White,
    /// Every octave alike: `1, 1, ...`.
    Pink,
    /// Low octaves louder: `1, 2, ..., 10`.
    Brownish,
    /// High octaves louder: `10, 9, ..., 1`.
    Blueish,
}

impl NoiseType {
    /// The preset for SoLoud's `int` type: 0 to 3, anything else white.
    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Pink,
            2 => Self::Brownish,
            3 => Self::Blueish,
            _ => Self::White,
        }
    }

    /// The octave weights of the preset.
    pub fn octave_scale(self) -> [f32; OCTAVES] {
        match self {
            Self::White => [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            Self::Pink => [1.0; OCTAVES],
            Self::Brownish => std::array::from_fn(|i| (i + 1) as f32),
            Self::Blueish => std::array::from_fn(|i| (OCTAVES - i) as f32),
        }
    }
}

/// A noise generator (SoLoud's `Noise`), mono at 44100 Hz, white by
/// default. Share it in an `Arc` and wrap it in a
/// [`Synth`](solouder_core::Synth) to play it; the weights may change at
/// any time and apply to voices started afterwards, as in SoLoud.
///
/// Every voice starts from the same seed, so voices sound alike. The
/// output is the sum of white noise and each octave's weighted value,
/// divided by the sum of the weights; weights summing to 0 give silence
/// (reference patch 0069; SoLoud computed infinity, then NaN).
#[derive(Debug)]
pub struct Noise {
    octave_scale: [AtomicU32; OCTAVES],
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

impl Noise {
    /// White noise.
    pub fn new() -> Self {
        let noise = Self {
            octave_scale: std::array::from_fn(|_| AtomicU32::new(0)),
        };
        noise.set_type(NoiseType::White);
        noise
    }

    /// The octave weights.
    pub fn octave_scale(&self) -> [f32; OCTAVES] {
        std::array::from_fn(|i| f32::from_bits(self.octave_scale[i].load(Ordering::Relaxed)))
    }

    /// Sets the weight of each octave for voices started after this call
    /// (`Noise::setOctaveScale`). The weights are stored one at a time, as
    /// SoLoud's array is: a voice prepared on another thread during the
    /// call can start with some old and some new.
    pub fn set_octave_scale(&self, scale: [f32; OCTAVES]) {
        for (slot, value) in self.octave_scale.iter().zip(scale) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    /// Sets a preset's weights (`Noise::setType`).
    pub fn set_type(&self, kind: NoiseType) {
        self.set_octave_scale(kind.octave_scale());
    }
}

impl Generator for Noise {
    fn channels(&self) -> u32 {
        1
    }

    fn sample_rate(&self) -> f32 {
        44100.0
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        Box::new(NoiseInstance {
            octave_scale: self.octave_scale(),
            prg: Prg::new(0xfade),
        })
    }
}

/// One voice of a [`Noise`] (`NoiseInstance`).
#[derive(Debug)]
struct NoiseInstance {
    octave_scale: [f32; OCTAVES],
    prg: Prg,
}

impl GeneratorInstance for NoiseInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        _: usize,
        _: &GeneratorContext,
    ) -> usize {
        // The octave state starts over at every call, as in SoLoud: with
        // 512-frame reads the last octave never takes a value.
        let mut step: [i32; OCTAVES] = std::array::from_fn(|j| 1 << j);
        let mut value = [0.0_f32; OCTAVES];
        let mut total = 0.0_f32;
        for scale in self.octave_scale {
            total += scale;
        }
        let gain = if total == 0.0 { 0.0 } else { 1.0 / total };
        for sample in &mut out[..frames] {
            let mut s = self.prg.rand_float() - 0.5;
            for j in 0..OCTAVES {
                step[j] += 1;
                if step[j] > 1 << (j + 1) {
                    step[j] = 0;
                    value[j] = self.prg.rand_float() - 0.5;
                }
                s += value[j] * self.octave_scale[j];
            }
            *sample = s * gain;
        }
        frames
    }

    fn has_ended(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
