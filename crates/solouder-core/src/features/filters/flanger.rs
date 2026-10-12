// Ported from SoLoud src/filter/soloud_flangerfilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::f64::consts::PI;

use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams};
use crate::dsp::c_int;

/// A short delay swept by a cosine LFO, mixed with the dry signal
/// (`FlangerFilter`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlangerFilter {
    delay: f32,
    freq: f32,
}

impl FlangerFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// Largest delay in seconds.
    pub const DELAY: u32 = 1;
    /// LFO frequency in Hz.
    pub const FREQ: u32 = 2;

    /// Changes the settings that new instances start with
    /// (`FlangerFilter::setParams`). Both must be above 0.
    pub fn set_params(&mut self, delay: f32, freq: f32) -> Result<(), InvalidFilterParams> {
        if !(delay > 0.0 && freq > 0.0) {
            return Err(InvalidFilterParams);
        }
        *self = Self { delay, freq };
        Ok(())
    }
}

impl Default for FlangerFilter {
    /// 5 ms at 10 Hz.
    fn default() -> Self {
        Self {
            delay: 0.005,
            freq: 10.0,
        }
    }
}

impl Filter for FlangerFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(3);
        let values = params.values_mut();
        values[Self::FREQ as usize] = self.freq;
        values[Self::DELAY as usize] = self.delay;
        Box::new(FlangerInstance {
            params,
            buffer: Vec::new(),
            buffer_len: 0,
            offset: 0,
            index: 0.0,
        })
    }

    fn param_count(&self) -> u32 {
        3
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        ["Wet", "Delay", "Freq"].get(index as usize).copied()
    }

    fn param_max(&self, index: u32) -> f32 {
        match index {
            Self::DELAY => 0.1,
            Self::FREQ => 100.0,
            _ => 1.0,
        }
    }

    fn param_min(&self, index: u32) -> f32 {
        if index == Self::WET { 0.0 } else { 0.001 }
    }
}

#[derive(Debug)]
struct FlangerInstance {
    params: FilterParams,
    /// Planar delay line, `buffer_len` frames per channel.
    buffer: Vec<f32>,
    buffer_len: u32,
    offset: u32,
    /// LFO phase in radians.
    index: f64,
}

impl FlangerInstance {
    /// Grows the delay line to `len` frames per channel, cleared. When the
    /// allocation fails the call leaves the audio unfiltered, as SoLoud does
    /// when `new` returns null.
    fn reserve(&mut self, len: u32, channels: usize) -> bool {
        if self.buffer_len >= len {
            return true;
        }
        self.buffer_len = len;
        let total = len as usize * channels;
        self.buffer = Vec::new();
        if self.buffer.try_reserve_exact(total).is_err() {
            self.buffer_len = 0;
            return false;
        }
        self.buffer.resize(total, 0.0);
        true
    }
}

impl FilterInstance for FlangerInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn params_mut(&mut self) -> &mut FilterParams {
        &mut self.params
    }

    fn filter(
        &mut self,
        buffer: &mut [f32],
        frames: usize,
        stride: usize,
        channels: usize,
        sample_rate: f32,
        time: f64,
    ) {
        self.params.update(time);
        let values = self.params.values();
        let (wet, delay, freq) = (
            values[FlangerFilter::WET as usize],
            values[FlangerFilter::DELAY as usize],
            values[FlangerFilter::FREQ as usize],
        );
        // A delay out of the int range converts to INT_MIN, and even a delay
        // of 0 needs one frame of line (reference patch 0044).
        let max_frames = c_int(f64::from((delay * sample_rate).ceil())).max(0);
        if !self.reserve(max_frames.max(1) as u32, channels) {
            return;
        }
        let len = self.buffer_len;
        let inc = f64::from(freq) * PI * 2.0 / f64::from(sample_rate);
        let frames_u32 = frames as u32;
        // Every channel follows the same LFO, which advances once per frame
        // (reference patch 0043).
        let start = self.index;
        for channel in 0..channels {
            self.index = start;
            let line = &mut self.buffer[channel * len as usize..][..len as usize];
            let samples = &mut buffer[channel * stride..][..frames];
            for sample in samples {
                let swing = c_int((f64::from(max_frames) * (1.0 + self.index.cos())).floor()) / 2;
                self.index += inc;
                line[(self.offset % len) as usize] = *sample;
                let from = len.wrapping_sub(swing as u32).wrapping_add(self.offset) % len;
                let n = 0.5 * (*sample + line[from as usize]);
                self.offset = self.offset.wrapping_add(1);
                *sample += (n - *sample) * wet;
            }
            self.offset = self.offset.wrapping_sub(frames_u32);
        }
        self.offset = self.offset.wrapping_add(frames_u32) % len;
    }
}
