// Ported from SoLoud src/filter/soloud_biquadresonantfilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::f64::consts::PI;

use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams, ParamType};
use crate::contracts::MAX_CHANNELS;

/// The response of a [`BiquadResonantFilter`] (`FILTERTYPE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BiquadType {
    /// Passes frequencies below the cutoff.
    #[default]
    Lowpass = 0,
    /// Passes frequencies above the cutoff.
    Highpass = 1,
    /// Passes frequencies around the center frequency.
    Bandpass = 2,
}

/// A two-pole resonant lowpass, highpass or bandpass filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiquadResonantFilter {
    filter_type: BiquadType,
    frequency: f32,
    resonance: f32,
}

impl BiquadResonantFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// The [`BiquadType`] as a number; other values act as lowpass.
    pub const TYPE: u32 = 1;
    /// Cutoff or center frequency in Hz. It is limited to 0.49 times the rate
    /// of the audio filtered, which for a voice falls with its play speed
    /// (reference patch 0031).
    pub const FREQUENCY: u32 = 2;
    /// Q: the resonance peak grows with it.
    pub const RESONANCE: u32 = 3;

    /// Changes the settings that new instances start with
    /// (`BiquadResonantFilter::setParams`). Frequency and resonance must be
    /// above 0.
    pub fn set_params(
        &mut self,
        filter_type: BiquadType,
        frequency: f32,
        resonance: f32,
    ) -> Result<(), InvalidFilterParams> {
        if !(frequency > 0.0 && resonance > 0.0) {
            return Err(InvalidFilterParams);
        }
        *self = Self {
            filter_type,
            frequency,
            resonance,
        };
        Ok(())
    }
}

impl Default for BiquadResonantFilter {
    /// Lowpass at 1000 Hz with a resonance of 2.
    fn default() -> Self {
        Self {
            filter_type: BiquadType::Lowpass,
            frequency: 1000.0,
            resonance: 2.0,
        }
    }
}

impl Filter for BiquadResonantFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(4);
        let values = params.values_mut();
        values[Self::RESONANCE as usize] = self.resonance;
        values[Self::FREQUENCY as usize] = self.frequency;
        values[Self::TYPE as usize] = self.filter_type as i32 as f32;
        let mut instance = BiquadInstance {
            params,
            state: [State::default(); MAX_CHANNELS as usize],
            coefficients: Coefficients::default(),
            sample_rate: 44100.0,
        };
        instance.calc();
        Box::new(instance)
    }

    fn param_count(&self) -> u32 {
        4
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        ["Wet", "Type", "Frequency", "Resonance"]
            .get(index as usize)
            .copied()
    }

    fn param_type(&self, index: u32) -> ParamType {
        if index == Self::TYPE {
            ParamType::Int
        } else {
            ParamType::Float
        }
    }

    fn param_max(&self, index: u32) -> f32 {
        match index {
            Self::TYPE => 2.0,
            Self::FREQUENCY => 8000.0,
            Self::RESONANCE => 20.0,
            _ => 1.0,
        }
    }

    fn param_min(&self, index: u32) -> f32 {
        match index {
            Self::FREQUENCY => 10.0,
            Self::RESONANCE => 0.1,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct State {
    y1: f32,
    y2: f32,
    x1: f32,
    x2: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Coefficients {
    a0: f32,
    a1: f32,
    a2: f32,
    b1: f32,
    b2: f32,
}

#[derive(Debug)]
struct BiquadInstance {
    params: FilterParams,
    state: [State; MAX_CHANNELS as usize],
    coefficients: Coefficients,
    /// The rate the coefficients were computed for.
    sample_rate: f32,
}

impl BiquadInstance {
    /// `calcBQRParams`.
    fn calc(&mut self) {
        let p = self.params.values();
        let limit = self.sample_rate * 0.49;
        let requested = p[BiquadResonantFilter::FREQUENCY as usize];
        let frequency = if requested < limit { requested } else { limit };
        let omega = (2.0 * PI * f64::from(frequency) / f64::from(self.sample_rate)) as f32;
        let (sin, cos) = (omega.sin(), omega.cos());
        let alpha = sin / (2.0 * p[BiquadResonantFilter::RESONANCE as usize]);
        let scalar = 1.0 / (1.0 + alpha);
        let b1 = -2.0 * cos * scalar;
        let b2 = (1.0 - alpha) * scalar;
        self.coefficients = match p[BiquadResonantFilter::TYPE as usize] as i32 {
            1 => {
                let a0 = 0.5 * (1.0 + cos) * scalar;
                let a1 = -(1.0 + cos) * scalar;
                Coefficients {
                    a0,
                    a1,
                    a2: a0,
                    b1,
                    b2,
                }
            }
            2 => {
                let a0 = alpha * scalar;
                Coefficients {
                    a0,
                    a1: 0.0,
                    a2: -a0,
                    b1,
                    b2,
                }
            }
            _ => {
                let a0 = 0.5 * (1.0 - cos) * scalar;
                let a1 = (1.0 - cos) * scalar;
                Coefficients {
                    a0,
                    a1,
                    a2: a0,
                    b1,
                    b2,
                }
            }
        };
    }
}

impl FilterInstance for BiquadInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn params_mut(&mut self) -> &mut FilterParams {
        &mut self.params
    }

    fn filter_channel(
        &mut self,
        samples: &mut [f32],
        sample_rate: f32,
        time: f64,
        channel: usize,
        _channels: usize,
    ) {
        if channel == 0 {
            self.params.update(time);
            let shape = (1 << BiquadResonantFilter::FREQUENCY)
                | (1 << BiquadResonantFilter::RESONANCE)
                | (1 << BiquadResonantFilter::TYPE);
            if self.params.changed() & shape != 0 || sample_rate != self.sample_rate {
                self.sample_rate = sample_rate;
                self.calc();
            }
            self.params.clear_changed();
        }
        let wet = self.params.values()[BiquadResonantFilter::WET as usize];
        let Coefficients { a0, a1, a2, b1, b2 } = self.coefficients;
        let s = &mut self.state[channel];
        // Samples are filtered in pairs, with the state update permuted
        // between the two.
        let (pairs, odd) = samples.as_chunks_mut::<2>();
        for pair in pairs {
            let x = pair[0];
            s.y2 = (a0 * x) + (a1 * s.x1) + (a2 * s.x2) - (b1 * s.y1) - (b2 * s.y2);
            pair[0] += (s.y2 - pair[0]) * wet;
            s.x2 = pair[1];
            s.y1 = (a0 * s.x2) + (a1 * x) + (a2 * s.x1) - (b1 * s.y2) - (b2 * s.y1);
            pair[1] += (s.y1 - pair[1]) * wet;
            s.x1 = s.x2;
            s.x2 = x;
        }
        // An odd last sample copies the one before it; a single sample is
        // left dry (reference patch 0038).
        if !odd.is_empty() && samples.len() > 1 {
            let last = samples.len() - 1;
            samples[last] = samples[last - 1];
        }
    }
}
