// Ported from SoLoud src/core/soloud_filter.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use crate::dsp::fader::{Fader, FaderState};

/// Most parameters an instance can have: each has a bit in
/// [`FilterParams::changed`].
pub const MAX_FILTER_PARAMS: usize = 32;

/// A filter instance's live parameters and their fades
/// (`FilterInstance::mParam`, `mParamFader` and `mParamChanged`). Parameter 0
/// is the wet/dry mix: 1 is fully filtered, 0 is the input unchanged.
///
/// Values are not clamped to the filter's
/// [`param_min`](super::Filter::param_min) and
/// [`param_max`](super::Filter::param_max).
#[derive(Clone, Debug)]
pub struct FilterParams {
    values: Vec<f32>,
    faders: Vec<Fader>,
    changed: u32,
}

impl FilterParams {
    /// `count` parameters, all 0 except the wet mix, which is 1
    /// (`FilterInstance::initParams`).
    ///
    /// # Panics
    ///
    /// If `count` is 0 or above [`MAX_FILTER_PARAMS`].
    pub fn new(count: usize) -> Self {
        assert!(
            (1..=MAX_FILTER_PARAMS).contains(&count),
            "a filter has 1 to {MAX_FILTER_PARAMS} parameters, not {count}"
        );
        let mut values = vec![0.0; count];
        values[0] = 1.0;
        Self {
            values,
            faders: vec![Fader::default(); count],
            changed: 0,
        }
    }

    /// Current values, as of the last [`FilterParams::update`].
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// Current values for an instance to initialize; changes made here are
    /// not marked in [`FilterParams::changed`].
    pub fn values_mut(&mut self) -> &mut [f32] {
        &mut self.values
    }

    /// The parameter's value, or `None` past the last parameter
    /// (`getFilterParameter`, which returns 0 there).
    pub fn get(&self, index: u32) -> Option<f32> {
        self.values.get(index as usize).copied()
    }

    /// Sets a parameter, cancelling its fade (`setFilterParameter`). Indices
    /// past the last parameter are ignored.
    pub fn set(&mut self, index: u32, value: f32) {
        let i = index as usize;
        if i < self.values.len() {
            self.faders[i].state = FaderState::Off;
            self.values[i] = value;
            self.changed |= 1 << i;
        }
    }

    /// Fades a parameter from its current value to `to` over `time` seconds
    /// starting at stream time `start` (`fadeFilterParameter`). Does nothing
    /// for a time not above 0 or a parameter already at `to`.
    pub fn fade(&mut self, index: u32, to: f32, time: f64, start: f64) {
        let i = index as usize;
        if i >= self.values.len() || time <= 0.0 || to == self.values[i] {
            return;
        }
        self.faders[i].set(self.values[i], to, time, start);
    }

    /// Oscillates a parameter between `from` and `to` with a period of `time`
    /// seconds from stream time `start` (`oscillateFilterParameter`). Does
    /// nothing for a time not above 0 or equal bounds.
    pub fn oscillate(&mut self, index: u32, from: f32, to: f32, time: f64, start: f64) {
        let i = index as usize;
        if i >= self.values.len() || time <= 0.0 || from == to {
            return;
        }
        self.faders[i].set_lfo(from, to, time, start);
    }

    /// Moves faded and oscillating parameters to their value at stream time
    /// `time`, marking them changed (`updateParams`). Instances call this at
    /// the start of each block they filter.
    pub fn update(&mut self, time: f64) {
        for (i, (value, fader)) in self.values.iter_mut().zip(&mut self.faders).enumerate() {
            if fader.is_running() {
                self.changed |= 1 << i;
                *value = fader.get(time);
            }
        }
    }

    /// One bit per parameter set or faded since [`FilterParams::clear_changed`].
    pub fn changed(&self) -> u32 {
        self.changed
    }

    /// Clears [`FilterParams::changed`], once an instance has applied the
    /// changes.
    pub fn clear_changed(&mut self) {
        self.changed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_start_dry_except_the_wet_mix() {
        let params = FilterParams::new(3);
        assert_eq!(params.values(), &[1.0, 0.0, 0.0]);
        assert_eq!(params.get(3), None);
        assert_eq!(params.changed(), 0);
    }

    #[test]
    fn setting_cancels_a_fade_and_marks_the_change() {
        let mut params = FilterParams::new(2);
        params.fade(1, 1.0, 1.0, 0.0);
        params.set(1, 0.25);
        params.update(0.5);
        assert_eq!(params.get(1), Some(0.25));
        assert_eq!(params.changed(), 0b10);
        params.set(7, 1.0);
        assert_eq!(params.changed(), 0b10);
    }

    #[test]
    fn fades_follow_stream_time_and_skip_no_op_requests() {
        let mut params = FilterParams::new(2);
        params.fade(1, 0.0, 1.0, 0.0);
        params.fade(1, 1.0, 0.0, 0.0);
        params.update(0.5);
        assert_eq!(params.changed(), 0);
        params.fade(1, 1.0, 2.0, 1.0);
        params.update(2.0);
        assert_eq!(params.get(1), Some(0.5));
        assert_eq!(params.changed(), 0b10);
        params.clear_changed();
        params.oscillate(0, 0.5, 0.5, 1.0, 0.0);
        params.update(2.5);
        assert_eq!(params.changed(), 0b10);
    }
}
