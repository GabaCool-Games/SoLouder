// Ported from SoLoud src/core/soloud_bus.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::sync::Arc;

use super::bus::{Bus, BusVoice};
use super::control::PlayOptions;
use super::handle::Handle;
use super::mixer::Mixer;
use super::sound::SoundId;
use super::source::Playable;
use super::visualization::{WAVE_LEN, spectrum};
use super::voice::{Voice, VoiceSource};
use crate::dsp::Resampler;
use crate::features::filters::{FILTERS_PER_STREAM, Filter};

/// Buses on this mixer. Calls for a bus that is not playing here do nothing
/// and return [`Handle::INVALID`], 0 or `None`.
impl Mixer {
    /// Plays `source` through `bus` (`Bus::play`). Unlike SoLoud, where the
    /// volume defaults to 1, a `volume` of `None` uses the source's volume
    /// as [`Mixer::play_with`] does.
    pub fn bus_play(&mut self, bus: &Bus, source: &impl Playable, options: PlayOptions) -> Handle {
        match self.bus_handle_of(bus.id()) {
            Some(handle) => self.play_with(
                source,
                PlayOptions {
                    bus: handle,
                    ..options
                },
            ),
            None => Handle::INVALID,
        }
    }

    /// [`Mixer::play_clocked`] through `bus` (`Bus::playClocked`).
    pub fn bus_play_clocked(
        &mut self,
        bus: &Bus,
        sound_time: f64,
        source: &impl Playable,
        options: PlayOptions,
    ) -> Handle {
        match self.bus_handle_of(bus.id()) {
            Some(handle) => self.play_clocked(
                sound_time,
                source,
                PlayOptions {
                    bus: handle,
                    ..options
                },
            ),
            None => Handle::INVALID,
        }
    }

    /// Moves a voice, or every voice of a group, to play through `bus`
    /// (`Bus::annexSound`).
    pub fn annex_sound(&mut self, bus: &Bus, handle: Handle) {
        self.annex_sound_of(bus.id(), handle);
    }

    pub(crate) fn annex_sound_of(&mut self, bus: SoundId, handle: Handle) {
        if let Some(bus) = self.bus_handle_of(bus) {
            self.voices.for_each_voice(handle, |voice| voice.bus = bus);
        }
    }

    /// Voices playing through `bus`, paused or not (`Bus::getActiveVoiceCount`).
    pub fn bus_voice_count(&mut self, bus: &Bus) -> usize {
        let Some(handle) = self.bus_handle_of(bus.id()) else {
            return 0;
        };
        let voices = &self.voices.slots[..self.voices.highest];
        voices.iter().flatten().filter(|v| v.bus == handle).count()
    }

    /// The peak of `channel` over the first 256 frames of the bus's latest
    /// mix (`Bus::getApproximateVolume`). Needs visualization data; 0 for a
    /// channel the bus does not have (reference patch 0047).
    pub fn bus_approximate_volume(&self, bus: &Bus, channel: u32) -> f32 {
        if channel >= bus.channels() {
            return 0.0;
        }
        self.bus_voice(bus)
            .map_or(0.0, |voice| voice.channel_volume[channel as usize])
    }

    /// The first 256 frames of the bus's latest mix, summed over channels
    /// (`Bus::getWave`). Needs visualization data. SoLoud returns its last
    /// copy once the bus has stopped; this returns `None`.
    pub fn bus_wave(&self, bus: &Bus) -> Option<[f32; WAVE_LEN]> {
        self.bus_voice(bus).map(|voice| voice.wave)
    }

    /// Magnitudes of the first 256 bins of a 512-point FFT of
    /// [`Mixer::bus_wave`], zero-padded (`Bus::calcFFT`).
    pub fn bus_fft(&self, bus: &Bus) -> Option<[f32; WAVE_LEN]> {
        self.bus_wave(bus).map(|wave| spectrum(&wave))
    }

    /// Sets or clears filter `slot` of `bus` and, if the bus is playing
    /// here, replaces the bus voice's filter in that slot (`Bus::setFilter`).
    pub fn set_bus_filter(&mut self, bus: &mut Bus, slot: u32, filter: Option<Arc<dyn Filter>>) {
        bus.set_filter(slot, filter.clone());
        if (slot as usize) < FILTERS_PER_STREAM
            && let Some(voice) = self.bus_voice_mut_of(bus.id())
        {
            voice.filters[slot as usize] = filter.map(|filter| filter.create_instance());
        }
    }

    /// Sets how `bus` resamples its voices, including a playing bus
    /// (`Bus::setResampler`).
    pub fn set_bus_resampler(&mut self, bus: &mut Bus, resampler: Resampler) {
        bus.set_resampler(resampler);
        self.set_bus_resampler_of(bus.id(), resampler);
    }

    pub(crate) fn set_bus_resampler_of(&mut self, bus: SoundId, resampler: Resampler) {
        if let Some(VoiceSource::Bus(state)) = self.bus_voice_mut_of(bus).map(|v| &mut v.source) {
            state.resampler = resampler;
        }
    }

    /// Sets whether `bus` records visualization data, including a playing
    /// bus (`Bus::setVisualizationEnable`).
    pub fn set_bus_visualization(&mut self, bus: &mut Bus, enable: bool) {
        bus.set_visualization(enable);
        self.set_bus_visualization_of(bus.id(), enable);
    }

    pub(crate) fn set_bus_visualization_of(&mut self, bus: SoundId, enable: bool) {
        if let Some(VoiceSource::Bus(state)) = self.bus_voice_mut_of(bus).map(|v| &mut v.source) {
            state.visualization = enable;
        }
    }

    fn bus_slot(&self, bus: SoundId) -> Option<usize> {
        let voices = &self.voices.slots[..self.voices.highest];
        voices.iter().position(|voice| {
            voice
                .as_ref()
                .is_some_and(|v| v.sound_id == bus && v.as_bus().is_some())
        })
    }

    fn bus_voice(&self, bus: &Bus) -> Option<&BusVoice> {
        self.bus_voice_of(bus.id())
    }

    pub(crate) fn bus_voice_of(&self, bus: SoundId) -> Option<&BusVoice> {
        self.voices.slots[self.bus_slot(bus)?].as_ref()?.as_bus()
    }

    pub(crate) fn bus_voice_mut_of(&mut self, bus: SoundId) -> Option<&mut Voice> {
        let slot = self.bus_slot(bus)?;
        self.voices.slots[slot].as_mut()
    }

    /// The bus voice's handle, which from now on lets it mix and stop the
    /// voices playing through it (`Bus::findBusHandle`).
    pub(crate) fn bus_handle_of(&mut self, bus: SoundId) -> Option<Handle> {
        let slot = self.bus_slot(bus)?;
        let voice = self.voices.slots[slot].as_mut()?;
        let handle = Handle::new(slot, voice.play_index);
        if let VoiceSource::Bus(state) = &mut voice.source {
            state.handle = Some(handle);
        }
        Some(handle)
    }
}
