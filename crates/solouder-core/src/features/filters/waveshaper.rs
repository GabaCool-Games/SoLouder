// Ported from SoLoud src/filter/soloud_waveshaperfilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams};

/// Soft clipping (positive amounts) or expansion (negative amounts) through
/// the curve `(1 + k) x / (1 + k |x|)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaveShaperFilter {
    amount: f32,
}

impl WaveShaperFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// -1 to 1; 0 leaves the audio unchanged.
    pub const AMOUNT: u32 = 1;

    /// Changes the amount new instances start with
    /// (`WaveShaperFilter::setParams`); it must be from -1 to 1.
    pub fn set_params(&mut self, amount: f32) -> Result<(), InvalidFilterParams> {
        if !(-1.0..=1.0).contains(&amount) {
            return Err(InvalidFilterParams);
        }
        self.amount = amount;
        Ok(())
    }
}

impl Filter for WaveShaperFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(2);
        params.values_mut()[Self::AMOUNT as usize] = self.amount;
        Box::new(WaveShaperInstance { params })
    }

    fn param_count(&self) -> u32 {
        2
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        Some(if index == Self::AMOUNT {
            "Amount"
        } else {
            "Wet"
        })
    }

    fn param_min(&self, index: u32) -> f32 {
        if index == Self::AMOUNT { -1.0 } else { 0.0 }
    }
}

#[derive(Debug)]
struct WaveShaperInstance {
    params: FilterParams,
}

impl FilterInstance for WaveShaperInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn params_mut(&mut self) -> &mut FilterParams {
        &mut self.params
    }

    fn filter_channel(
        &mut self,
        samples: &mut [f32],
        _sample_rate: f32,
        time: f64,
        _channel: usize,
        _channels: usize,
    ) {
        self.params.update(time);
        let p = self.params.values();
        let (wet, amount) = (
            p[WaveShaperFilter::WET as usize],
            p[WaveShaperFilter::AMOUNT as usize],
        );
        let k = if amount == 1.0 {
            2.0 * amount / 0.01
        } else {
            2.0 * amount / (1.0 - amount)
        };
        for x in samples {
            let dry = *x;
            // Negative amounts put a pole at |x| = -1 / k; past it the output
            // is 0 (reference patch 0034).
            let denominator = 1.0 + k * x.abs();
            let shaped = if denominator > 0.0 {
                (1.0 + k) * *x / denominator
            } else {
                0.0
            };
            *x += (shaped - dry) * wet;
        }
    }
}
