// Ported from SoLoud include/soloud_filter.h and src/core/soloud_filter.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Filters: DSP applied to a voice's source blocks or to the whole mix.
//!
//! A [`Filter`] holds a filter's settings and creates a [`FilterInstance`]
//! for each place it runs: one per voice of a sound that uses it, or one per
//! global filter slot of a mixer. Instances keep their own DSP state and live
//! [`FilterParams`], which can be set, faded and oscillated while they play.

use std::fmt;

mod bassboost;
mod biquad;
mod dc_removal;
mod echo;
mod fft_filter;
mod flanger;
mod freeverb;
mod lofi;
mod params;
mod robotize;
mod waveshaper;

pub use bassboost::BassboostFilter;
pub use biquad::{BiquadResonantFilter, BiquadType};
pub use dc_removal::DcRemovalFilter;
pub use echo::EchoFilter;
pub use fft_filter::FftFilter;
pub use flanger::FlangerFilter;
pub use freeverb::FreeverbFilter;
pub use lofi::LofiFilter;
pub use params::{FilterParams, MAX_FILTER_PARAMS};
pub use robotize::RobotizeFilter;
pub use waveshaper::WaveShaperFilter;

/// Filter slots per sound and per mixer (`FILTERS_PER_STREAM`).
pub const FILTERS_PER_STREAM: usize = 8;

/// Settings that can create filter instances.
///
/// The parameter description methods correspond to `Filter::getParamCount`,
/// `getParamName`, `getParamType`, `getParamMax` and `getParamMin`; their
/// defaults describe a filter whose only parameter is the wet/dry mix.
pub trait Filter: Send + Sync + fmt::Debug {
    /// A new instance with this filter's current settings
    /// (`Filter::createInstance`).
    fn create_instance(&self) -> Box<dyn FilterInstance>;

    /// Number of live parameters of an instance.
    fn param_count(&self) -> u32 {
        1
    }

    /// The parameter's name, or `None` past the last parameter for filters
    /// whose SoLoud version returns a null name.
    fn param_name(&self, _index: u32) -> Option<&'static str> {
        Some("Wet")
    }

    /// How the parameter's value is read.
    fn param_type(&self, _index: u32) -> ParamType {
        ParamType::Float
    }

    /// The largest useful value; parameters are not clamped to it.
    fn param_max(&self, _index: u32) -> f32 {
        1.0
    }

    /// The smallest useful value; parameters are not clamped to it.
    fn param_min(&self, _index: u32) -> f32 {
        0.0
    }
}

/// How a parameter's value is meant to be read (`Filter::PARAMTYPE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamType {
    /// Any value.
    Float,
    /// Truncated to an integer, such as a filter type.
    Int,
    /// 0 or 1.
    Bool,
}

/// A running filter: DSP state plus live parameters.
pub trait FilterInstance: Send + fmt::Debug {
    /// The live parameters.
    fn params(&self) -> &FilterParams;

    /// The live parameters, to set, fade or oscillate.
    fn params_mut(&mut self) -> &mut FilterParams;

    /// Filters `frames` frames of planar audio in place; channel `c` starts
    /// at `c * stride`. `time` is the mixer's stream time, which drives
    /// parameter fades. By default each channel goes to
    /// [`FilterInstance::filter_channel`].
    fn filter(
        &mut self,
        buffer: &mut [f32],
        frames: usize,
        stride: usize,
        channels: usize,
        sample_rate: f32,
        time: f64,
    ) {
        for channel in 0..channels {
            let samples = &mut buffer[channel * stride..][..frames];
            self.filter_channel(samples, sample_rate, time, channel, channels);
        }
    }

    /// Allocates what the first [`FilterInstance::filter`] call with this
    /// channel count and rate would, so that call does not. The real-time
    /// controller calls it on its own thread; a first call with other values
    /// allocates as usual. Does nothing by default.
    fn prepare(&mut self, _channels: usize, _sample_rate: f32) {}

    /// Filters one channel in place.
    fn filter_channel(
        &mut self,
        _samples: &mut [f32],
        _sample_rate: f32,
        _time: f64,
        _channel: usize,
        _channels: usize,
    ) {
    }
}

/// A filter setting outside its valid range (`INVALID_PARAMETER`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidFilterParams;

impl fmt::Display for InvalidFilterParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "filter setting out of range")
    }
}

impl std::error::Error for InvalidFilterParams {}

/// Frames in `seconds` of audio at `rate`, rounded up, as SoLoud sizes its
/// delay lines: `(int)ceil(seconds * rate)`, at least 1 (reference patches
/// 0033 and 0036). Values beyond the `int` range convert to `INT_MIN` on x86
/// and so also give 1.
pub(crate) fn delay_frames(seconds: f32, rate: f32) -> usize {
    let frames = (seconds * rate).ceil();
    if (1.0..2_147_483_648.0).contains(&frames) {
        frames as usize
    } else {
        1
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_spectral;
