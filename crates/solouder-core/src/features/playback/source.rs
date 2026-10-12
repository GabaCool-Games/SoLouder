// Ported from SoLoud src/core/soloud_audiosource.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::sync::Arc;

use super::control::PlayOptions;
use super::handle::Handle;
use super::mixer::Mixer;
use super::sound::SoundId;
use super::voice::flags;
use crate::features::filters::{FILTERS_PER_STREAM, Filter, FilterInstance};

type FilterSlots = [Option<Arc<dyn Filter>>; FILTERS_PER_STREAM];

/// Something [`Mixer::play_with`] can start: a [`Sound`](super::Sound) or a
/// [`Bus`](super::Bus).
pub trait Playable: private::Sealed {
    /// Identifies the source and its clones.
    fn source_id(&self) -> SoundId;

    #[doc(hidden)]
    fn play_on(&self, mixer: &mut Mixer, options: PlayOptions) -> Handle;
}

mod private {
    pub trait Sealed {}
    impl Sealed for super::super::Sound {}
    impl Sealed for super::super::Bus {}
}

/// Playback defaults every source has (`AudioSource`).
#[derive(Clone, Debug)]
pub(crate) struct SourceSettings {
    pub volume: f32,
    pub looping: bool,
    pub loop_point: f64,
    pub auto_stop: bool,
    pub single_instance: bool,
    pub inaudible_tick: bool,
    pub inaudible_kill: bool,
    /// Shared with clones until one of them changes a filter.
    pub filters: Option<Arc<FilterSlots>>,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            looping: false,
            loop_point: 0.0,
            auto_stop: true,
            single_instance: false,
            inaudible_tick: false,
            inaudible_kill: false,
            filters: None,
        }
    }
}

impl SourceSettings {
    /// The flags a new voice starts with (`AudioSourceInstance::init`).
    pub fn voice_flags(&self) -> u32 {
        let mut voice_flags = 0;
        for (on, flag) in [
            (self.looping, flags::LOOPING),
            (self.inaudible_kill, flags::INAUDIBLE_KILL),
            (self.inaudible_tick, flags::INAUDIBLE_TICK),
            (!self.auto_stop, flags::DISABLE_AUTOSTOP),
        ] {
            if on {
                voice_flags |= flag;
            }
        }
        voice_flags
    }

    pub fn filter(&self, slot: u32) -> Option<&Arc<dyn Filter>> {
        self.filters.as_ref()?.get(slot as usize)?.as_ref()
    }

    pub fn set_filter(&mut self, slot: u32, filter: Option<Arc<dyn Filter>>) {
        let slot = slot as usize;
        if slot < FILTERS_PER_STREAM {
            Arc::make_mut(self.filters.get_or_insert_default())[slot] = filter;
        }
    }

    /// A new instance of each filter, for a new voice.
    pub fn filter_instances(&self) -> [Option<Box<dyn FilterInstance>>; FILTERS_PER_STREAM] {
        std::array::from_fn(|slot| {
            self.filter(slot as u32)
                .map(|filter| filter.create_instance())
        })
    }
}

/// The `AudioSource` setters and getters, on a type with a
/// `settings: SourceSettings` field.
macro_rules! source_settings_api {
    () => {
        /// Volume used by `play` when no volume is given (`AudioSource::setVolume`).
        pub fn volume(&self) -> f32 {
            self.settings.volume
        }

        /// Sets the default volume for voices started after this call.
        pub fn set_volume(&mut self, volume: f32) {
            self.settings.volume = volume;
        }

        /// Whether new voices loop (`AudioSource::setLooping`).
        pub fn looping(&self) -> bool {
            self.settings.looping
        }

        /// Sets whether voices started after this call loop.
        pub fn set_looping(&mut self, looping: bool) {
            self.settings.looping = looping;
        }

        /// Seconds into the source where looping voices restart; 0 by default.
        pub fn loop_point(&self) -> f64 {
            self.settings.loop_point
        }

        /// Sets the loop point for voices started after this call
        /// (`AudioSource::setLoopPoint`).
        pub fn set_loop_point(&mut self, seconds: f64) {
            self.settings.loop_point = seconds;
        }

        /// Whether voices stop once the source has played out; true by default.
        pub fn auto_stop(&self) -> bool {
            self.settings.auto_stop
        }

        /// Sets whether voices started after this call stop at the end of the
        /// source or stay, silent, until stopped (`AudioSource::setAutoStop`).
        pub fn set_auto_stop(&mut self, auto_stop: bool) {
            self.settings.auto_stop = auto_stop;
        }

        /// Whether playing the source first stops its other voices on that mixer.
        pub fn single_instance(&self) -> bool {
            self.settings.single_instance
        }

        /// Sets whether playing the source first stops its other voices
        /// (`AudioSource::setSingleInstance`).
        pub fn set_single_instance(&mut self, single_instance: bool) {
            self.settings.single_instance = single_instance;
        }

        /// Whether voices keep playing when inaudible (`tick`) and whether
        /// they are stopped then (`kill`).
        pub fn inaudible_behavior(&self) -> (bool, bool) {
            (self.settings.inaudible_tick, self.settings.inaudible_kill)
        }

        /// Sets the inaudible behavior of voices started after this call
        /// (`AudioSource::setInaudibleBehavior`). Ticking voices are always among
        /// the active voices.
        pub fn set_inaudible_behavior(&mut self, tick: bool, kill: bool) {
            self.settings.inaudible_tick = tick;
            self.settings.inaudible_kill = kill;
        }

        /// The filter in `slot`, if any.
        pub fn filter(
            &self,
            slot: u32,
        ) -> Option<&std::sync::Arc<dyn $crate::features::filters::Filter>> {
            self.settings.filter(slot)
        }

        /// Sets or clears the filter in `slot`, 0 to 7, for voices started after
        /// this call (`AudioSource::setFilter`); other slots are ignored. Each
        /// voice gets its own instance with the filter's settings, and runs its
        /// filters in slot order on the source's audio at the voice's rate.
        pub fn set_filter(
            &mut self,
            slot: u32,
            filter: Option<std::sync::Arc<dyn $crate::features::filters::Filter>>,
        ) {
            self.settings.set_filter(slot, filter);
        }
    };
}

pub(crate) use source_settings_api;
