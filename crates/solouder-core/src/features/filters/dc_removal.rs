// Ported from SoLoud src/filter/soloud_dcremovalfilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams, delay_frames};

/// Removes a constant offset by subtracting the running average of the last
/// `length` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DcRemovalFilter {
    length: f32,
}

impl DcRemovalFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;

    /// Changes the averaging window new instances start with
    /// (`DCRemovalFilter::setParams`); it must be above 0 seconds.
    pub fn set_params(&mut self, length: f32) -> Result<(), InvalidFilterParams> {
        if length.is_nan() || length <= 0.0 {
            return Err(InvalidFilterParams);
        }
        self.length = length;
        Ok(())
    }
}

impl Default for DcRemovalFilter {
    /// A 0.1 second window.
    fn default() -> Self {
        Self { length: 0.1 }
    }
}

impl Filter for DcRemovalFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        Box::new(DcRemovalInstance {
            params: FilterParams::new(1),
            length: self.length,
            buffer: Vec::new(),
            totals: Vec::new(),
            frames: 0,
            offset: 0,
            prepared: false,
        })
    }
}

#[derive(Debug)]
struct DcRemovalInstance {
    params: FilterParams,
    /// Copied at creation (reference patch 0035).
    length: f32,
    /// Planar window of the last `frames` inputs per channel; allocated by the
    /// first `filter` call, when the channel count and rate are known.
    buffer: Vec<f32>,
    /// Running sum of each channel's window.
    totals: Vec<f32>,
    frames: usize,
    offset: usize,
    /// The window was allocated by `prepare`, for a first call that may not
    /// match it.
    prepared: bool,
}

impl DcRemovalInstance {
    fn size_window(&mut self, channels: usize, sample_rate: f32) {
        let frames = delay_frames(self.length, sample_rate);
        if self.frames != frames || self.totals.len() != channels {
            self.frames = frames;
            self.buffer = vec![0.0; frames * channels];
            self.totals = vec![0.0; channels];
        }
    }
}

impl FilterInstance for DcRemovalInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn prepare(&mut self, channels: usize, sample_rate: f32) {
        if self.buffer.is_empty() {
            self.size_window(channels, sample_rate);
            self.prepared = true;
        }
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
        let wet = self.params.values()[DcRemovalFilter::WET as usize];
        if self.buffer.is_empty() || self.prepared {
            self.prepared = false;
            self.size_window(channels, sample_rate);
        }
        let len = self.frames;
        for i in 0..frames {
            for (j, total) in self.totals.iter_mut().enumerate().take(channels) {
                let at = self.offset + j * len;
                let sample = &mut buffer[i + j * stride];
                let mut n = *sample;
                *total -= self.buffer[at];
                *total += n;
                self.buffer[at] = n;
                n -= *total / len as f32;
                *sample += (n - *sample) * wet;
            }
            self.offset = (self.offset + 1) % len;
        }
    }
}
