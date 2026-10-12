// Ported from SoLoud src/filter/soloud_echofilter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::{Filter, FilterInstance, FilterParams, InvalidFilterParams, delay_frames};

/// A feedback delay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EchoFilter {
    delay: f32,
    decay: f32,
    filter: f32,
}

impl EchoFilter {
    /// Parameter index of the wet/dry mix.
    pub const WET: u32 = 0;
    /// Seconds between echoes. The delay line is sized for the delay an
    /// instance starts with, so a longer delay set later is limited to it.
    pub const DELAY: u32 = 1;
    /// Gain of each repeat.
    pub const DECAY: u32 = 2;
    /// How much each repeat is low-passed, 0 to just below 1.
    pub const FILTER: u32 = 3;

    /// Changes the settings that new instances start with
    /// (`EchoFilter::setParams`). Delay and decay must be above 0, and the
    /// filter at least 0 and below 1.
    pub fn set_params(
        &mut self,
        delay: f32,
        decay: f32,
        filter: f32,
    ) -> Result<(), InvalidFilterParams> {
        if !(delay > 0.0 && decay > 0.0 && (0.0..1.0).contains(&filter)) {
            return Err(InvalidFilterParams);
        }
        *self = Self {
            delay,
            decay,
            filter,
        };
        Ok(())
    }
}

impl Default for EchoFilter {
    /// A 0.3 second delay, decay 0.7, unfiltered.
    fn default() -> Self {
        Self {
            delay: 0.3,
            decay: 0.7,
            filter: 0.0,
        }
    }
}

impl Filter for EchoFilter {
    fn create_instance(&self) -> Box<dyn FilterInstance> {
        let mut params = FilterParams::new(4);
        let values = params.values_mut();
        values[Self::DELAY as usize] = self.delay;
        values[Self::DECAY as usize] = self.decay;
        values[Self::FILTER as usize] = self.filter;
        Box::new(EchoInstance {
            params,
            buffer: Vec::new(),
            max_frames: 0,
            offset: 0,
            prepared: false,
        })
    }

    fn param_count(&self) -> u32 {
        4
    }

    fn param_name(&self, index: u32) -> Option<&'static str> {
        ["Wet", "Delay", "Decay", "Filter"]
            .get(index as usize)
            .copied()
    }

    fn param_max(&self, index: u32) -> f32 {
        if index == Self::DELAY {
            self.delay
        } else {
            1.0
        }
    }
}

#[derive(Debug)]
struct EchoInstance {
    params: FilterParams,
    /// Planar delay line, `max_frames` per channel; allocated by the first
    /// `filter` call, when the channel count and rate are known.
    buffer: Vec<f32>,
    max_frames: usize,
    offset: usize,
    /// `buffer` was allocated by `prepare`, for a first call that may not
    /// match it.
    prepared: bool,
}

impl EchoInstance {
    fn size_line(&mut self, delay: f32, channels: usize, sample_rate: f32) {
        let max_frames = delay_frames(delay, sample_rate);
        if self.max_frames != max_frames || self.buffer.len() != max_frames * channels {
            self.max_frames = max_frames;
            self.buffer = vec![0.0; max_frames * channels];
        }
    }
}

impl FilterInstance for EchoInstance {
    fn params(&self) -> &FilterParams {
        &self.params
    }

    fn prepare(&mut self, channels: usize, sample_rate: f32) {
        if self.buffer.is_empty() {
            let delay = self.params.values()[EchoFilter::DELAY as usize];
            self.size_line(delay, channels, sample_rate);
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
        let p = self.params.values();
        let (wet, delay, decay, filter) = (
            p[EchoFilter::WET as usize],
            p[EchoFilter::DELAY as usize],
            p[EchoFilter::DECAY as usize],
            p[EchoFilter::FILTER as usize],
        );
        if self.buffer.is_empty() || self.prepared {
            self.prepared = false;
            self.size_line(delay, channels, sample_rate);
        }
        let len = delay_frames(delay, sample_rate).min(self.max_frames);
        let line = &mut self.buffer;
        // After the delay shrinks, `offset` can be past `len` for one frame;
        // it still indexes inside the line.
        let mut previous = (self.offset + len - 1) % len;
        for i in 0..frames {
            for j in 0..channels {
                let (at, from) = (self.offset + j * len, previous + j * len);
                let sample = &mut buffer[i + j * stride];
                line[at] = filter * line[from] + (1.0 - filter) * line[at];
                let n = *sample + line[at] * decay;
                line[at] = n;
                *sample += (n - *sample) * wet;
            }
            previous = self.offset;
            self.offset = (self.offset + 1) % len;
        }
    }
}
