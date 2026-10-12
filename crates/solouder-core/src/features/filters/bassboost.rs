// Ported from SoLoud src/filter/soloud_bassboostfilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::fft_filter::{Stft, comp_to_mag_phase, mag_phase_to_comp};
use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams};

/// Boosts the lowest two FFT bins (`BassboostFilter`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BassboostFilter {
    boost: f32,
}

impl BassboostFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// Gain of the lowest bins.
    pub const BOOST: u32 = 1;

    /// Changes the boost that new instances start with
    /// (`BassboostFilter::setParams`). It must be at least 0.
    pub fn set_params(&mut self, boost: f32) -> Result<(), InvalidFilterParams> {
        if boost.is_nan() || boost < 0.0 {
            return Err(InvalidFilterParams);
        }
        self.boost = boost;
        Ok(())
    }
}

impl Default for BassboostFilter {
    /// A boost of 2.
    fn default() -> Self {
        Self { boost: 2.0 }
    }
}

impl Filter for BassboostFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(2);
        params.values_mut()[Self::BOOST as usize] = self.boost;
        Box::new(BassboostInstance {
            params,
            stft: Stft::default(),
        })
    }

    fn param_count(&self) -> u32 {
        2
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        Some(if index == Self::BOOST { "Boost" } else { "Wet" })
    }

    fn param_max(&self, index: u32) -> f32 {
        if index == Self::BOOST { 10.0 } else { 1.0 }
    }
}

#[derive(Debug)]
struct BassboostInstance {
    params: FilterParams,
    stft: Stft,
}

impl FilterInstance for BassboostInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn prepare(&mut self, channels: usize, _sample_rate: f32) {
        self.stft.reserve(channels);
    }

    fn params_mut(&mut self) -> &mut FilterParams {
        &mut self.params
    }

    fn filter_channel(
        &mut self,
        samples: &mut [f32],
        _sample_rate: f32,
        time: f64,
        channel: usize,
        channels: usize,
    ) {
        if channel == 0 {
            self.params.update(time);
        }
        let values = self.params.values();
        let (wet, boost) = (
            values[BassboostFilter::WET as usize],
            values[BassboostFilter::BOOST as usize],
        );
        self.stft
            .filter_channel(samples, wet, channel, channels, |spectrum, _| {
                comp_to_mag_phase(spectrum, 2);
                spectrum[0] *= boost;
                spectrum[2] *= boost;
                mag_phase_to_comp(spectrum, 2);
            });
    }
}
