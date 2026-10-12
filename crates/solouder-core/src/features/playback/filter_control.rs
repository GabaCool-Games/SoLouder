// Ported from SoLoud src/core/soloud_core_filterops.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::handle::Handle;
use super::mixer::Mixer;
use crate::features::filters::{FILTERS_PER_STREAM, Filter, FilterParams};

/// Which filters a filter parameter call changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterTarget {
    /// The mixer's global filters (voice handle 0 in SoLoud).
    Global,
    /// A voice's filters, or those of every voice in a voice group.
    Voice(Handle),
}

/// Global filters and live filter parameters. Slots are 0 to 7; calls for
/// other slots, empty slots or stopped voices do nothing. Fades and
/// oscillations run on the mixer's stream time.
impl Mixer {
    /// Sets or clears global filter `slot`, which filters the whole mix
    /// before clipping (`Soloud::setGlobalFilter`). The new instance has the
    /// filter's current settings and replaces the slot's previous instance.
    pub fn set_global_filter(&mut self, slot: u32, filter: Option<&dyn Filter>) {
        if let Some(entry) = self.global_filters.get_mut(slot as usize) {
            *entry = filter.map(|filter| filter.create_instance());
        }
    }

    /// The live value of a filter parameter (`Soloud::getFilterParameter`);
    /// for a voice group, that of its first voice. `None` when the slot is
    /// empty or past the last parameter, where SoLoud returns
    /// `INVALID_PARAMETER` (1.0) and 0.
    pub fn filter_parameter(&self, target: FilterTarget, slot: u32, param: u32) -> Option<f32> {
        let slot = slot as usize;
        let instance = match target {
            FilterTarget::Global => self.global_filters.get(slot)?,
            FilterTarget::Voice(handle) => self.voices.get(handle)?.filters.get(slot)?,
        };
        instance.as_ref()?.params().get(param)
    }

    /// Sets a filter parameter, cancelling its fade
    /// (`Soloud::setFilterParameter`).
    pub fn set_filter_parameter(
        &mut self,
        target: FilterTarget,
        slot: u32,
        param: u32,
        value: f32,
    ) {
        self.with_filter_params(target, slot, |params, _| params.set(param, value));
    }

    /// Fades a filter parameter to `to` over `seconds`
    /// (`Soloud::fadeFilterParameter`).
    pub fn fade_filter_parameter(
        &mut self,
        target: FilterTarget,
        slot: u32,
        param: u32,
        to: f32,
        seconds: f64,
    ) {
        self.with_filter_params(target, slot, |params, now| {
            params.fade(param, to, seconds, now);
        });
    }

    /// Oscillates a filter parameter between `from` and `to` with a period of
    /// `seconds` (`Soloud::oscillateFilterParameter`).
    pub fn oscillate_filter_parameter(
        &mut self,
        target: FilterTarget,
        slot: u32,
        param: u32,
        from: f32,
        to: f32,
        seconds: f64,
    ) {
        self.with_filter_params(target, slot, |params, now| {
            params.oscillate(param, from, to, seconds, now);
        });
    }

    fn with_filter_params(
        &mut self,
        target: FilterTarget,
        slot: u32,
        mut change: impl FnMut(&mut FilterParams, f64),
    ) {
        let slot = slot as usize;
        if slot >= FILTERS_PER_STREAM {
            return;
        }
        let now = self.stream_time;
        match target {
            FilterTarget::Global => {
                if let Some(instance) = &mut self.global_filters[slot] {
                    change(instance.params_mut(), now);
                }
            }
            FilterTarget::Voice(handle) => self.voices.for_each_voice(handle, |voice| {
                if let Some(instance) = &mut voice.filters[slot] {
                    change(instance.params_mut(), now);
                }
            }),
        }
    }
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
