// Ported from SoLoud src/filter/soloud_lofifilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams};
use crate::contracts::MAX_CHANNELS;

/// Lowers the sample rate (by holding samples) and the bit depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LofiFilter {
    sample_rate: f32,
    bit_depth: f32,
}

impl LofiFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// The rate samples are held at, in Hz.
    pub const SAMPLE_RATE: u32 = 1;
    /// Bits per sample; fractions are allowed.
    pub const BIT_DEPTH: u32 = 2;

    /// Changes the settings that new instances start with
    /// (`LofiFilter::setParams`). Both must be above 0.
    pub fn set_params(
        &mut self,
        sample_rate: f32,
        bit_depth: f32,
    ) -> Result<(), InvalidFilterParams> {
        if !(sample_rate > 0.0 && bit_depth > 0.0) {
            return Err(InvalidFilterParams);
        }
        *self = Self {
            sample_rate,
            bit_depth,
        };
        Ok(())
    }
}

impl Default for LofiFilter {
    /// 4000 Hz at 3 bits.
    fn default() -> Self {
        Self {
            sample_rate: 4000.0,
            bit_depth: 3.0,
        }
    }
}

impl Filter for LofiFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(3);
        let values = params.values_mut();
        values[Self::SAMPLE_RATE as usize] = self.sample_rate;
        values[Self::BIT_DEPTH as usize] = self.bit_depth;
        Box::new(LofiInstance {
            params,
            channels: [Channel::default(); MAX_CHANNELS as usize],
        })
    }

    fn param_count(&self) -> u32 {
        3
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        ["Wet", "Samplerate", "Bitdepth"]
            .get(index as usize)
            .copied()
    }

    fn param_max(&self, index: u32) -> f32 {
        match index {
            Self::SAMPLE_RATE => 22000.0,
            Self::BIT_DEPTH => 16.0,
            _ => 1.0,
        }
    }

    fn param_min(&self, index: u32) -> f32 {
        match index {
            Self::SAMPLE_RATE => 100.0,
            Self::BIT_DEPTH => 0.5,
            _ => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    sample: f32,
    samples_to_skip: f32,
}

/// Holds state for every channel (reference patch 0032).
#[derive(Debug)]
struct LofiInstance {
    params: FilterParams,
    channels: [Channel; MAX_CHANNELS as usize],
}

impl FilterInstance for LofiInstance {
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
        self.params.update(time);
        let p = self.params.values();
        let (wet, target, bits) = (
            p[LofiFilter::WET as usize],
            p[LofiFilter::SAMPLE_RATE as usize],
            p[LofiFilter::BIT_DEPTH as usize],
        );
        let state = &mut self.channels[channel];
        for x in samples {
            if state.samples_to_skip <= 0.0 {
                state.samples_to_skip += (sample_rate / target) - 1.0;
                let q = 2f64.powf(f64::from(bits)) as f32;
                state.sample = (q * *x).floor() / q;
            } else {
                state.samples_to_skip -= 1.0;
            }
            *x += (state.sample - *x) * wet;
        }
    }
}
