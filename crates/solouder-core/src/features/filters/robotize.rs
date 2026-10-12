// Ported from SoLoud src/filter/soloud_robotizefilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::{Filter, FilterInstance, FilterParams, ParamType};
use crate::dsp::{c_int, waveform};

/// Amplitude modulation by a waveform, for a robotic voice
/// (`RobotizeFilter`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RobotizeFilter {
    freq: f32,
    wave: i32,
}

impl RobotizeFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// Modulation frequency in Hz.
    pub const FREQ: u32 = 1;
    /// The waveform, one of the [`waveform`] constants; truncated to an
    /// integer.
    pub const WAVE: u32 = 2;

    /// Changes the settings that new instances start with
    /// (`RobotizeFilter::setParams`). Nothing is rejected.
    pub fn set_params(&mut self, freq: f32, waveform: i32) {
        *self = Self {
            freq,
            wave: waveform,
        };
    }
}

impl Default for RobotizeFilter {
    /// 30 Hz, square wave.
    fn default() -> Self {
        Self {
            freq: 30.0,
            wave: waveform::SQUARE,
        }
    }
}

impl Filter for RobotizeFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(3);
        let values = params.values_mut();
        values[Self::FREQ as usize] = self.freq;
        values[Self::WAVE as usize] = self.wave as f32;
        Box::new(RobotizeInstance { params })
    }

    fn param_count(&self) -> u32 {
        3
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        ["Wet", "Frequency", "Waveform"]
            .get(index as usize)
            .copied()
    }

    fn param_type(&self, index: u32) -> ParamType {
        if index == Self::WAVE {
            ParamType::Int
        } else {
            ParamType::Float
        }
    }

    fn param_max(&self, index: u32) -> f32 {
        match index {
            Self::WAVE => 6.0,
            Self::FREQ => 100.0,
            _ => 1.0,
        }
    }

    fn param_min(&self, index: u32) -> f32 {
        if index == Self::FREQ { 0.1 } else { 0.0 }
    }
}

#[derive(Debug)]
struct RobotizeInstance {
    params: FilterParams,
}

impl FilterInstance for RobotizeInstance {
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
        _channel: usize,
        _channels: usize,
    ) {
        self.params.update(time);
        let values = self.params.values();
        let (wet, freq) = (
            values[RobotizeFilter::WET as usize],
            values[RobotizeFilter::FREQ as usize],
        );
        let wave = c_int(f64::from(values[RobotizeFilter::WAVE as usize]));
        // A period that is not at least 1 (a frequency above the rate, not
        // above 0, or out of the int range) is 1 (reference patch 0042).
        let period = c_int(f64::from(sample_rate / freq)).max(1);
        let start = c_int((time * f64::from(sample_rate)) % f64::from(period));
        let period = period as u32;
        for (i, sample) in samples.iter_mut().enumerate() {
            let position = (start as u32).wrapping_add(i as u32) % period;
            let p = position as f32 / period as f32;
            let s = *sample * (waveform::generate(wave, p) + 0.5);
            *sample += (s - *sample) * wet;
        }
    }
}
