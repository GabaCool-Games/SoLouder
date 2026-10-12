// Ported from SoLoud src/core/soloud_core_getters.cpp and
// src/core/soloud_core_basicops.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::handle::Handle;
use super::mixer::Mixer;
use super::source::Playable;
use super::voice::flags;
use crate::dsp::Resampler;

/// Queries. A handle whose voice has stopped gives `None`; a voice group
/// handle reads the group's first voice.
impl Mixer {
    /// Whether `handle` refers to a voice that has not stopped; false for a
    /// voice group.
    pub fn is_valid_voice_handle(&self, handle: Handle) -> bool {
        !handle.has_group_mark() && self.voices.get(handle).is_some()
    }

    /// The volume last set for the voice.
    pub fn volume(&self, handle: Handle) -> Option<f32> {
        self.voices.get(handle).map(|v| v.set_volume)
    }

    /// The voice's effective volume (set volume times 3D attenuation).
    pub fn overall_volume(&self, handle: Handle) -> Option<f32> {
        self.voices.get(handle).map(|v| v.overall_volume)
    }

    /// The pan last set for the voice.
    pub fn pan(&self, handle: Handle) -> Option<f32> {
        self.voices.get(handle).map(|v| v.pan)
    }

    /// Whether the voice is paused.
    pub fn is_paused(&self, handle: Handle) -> Option<bool> {
        self.voices.get(handle).map(|v| v.has_flag(flags::PAUSED))
    }

    /// Whether the voice loops.
    pub fn is_looping(&self, handle: Handle) -> Option<bool> {
        self.voices.get(handle).map(|v| v.has_flag(flags::LOOPING))
    }

    /// Where the voice restarts when it loops, in seconds into its sound.
    pub fn loop_point(&self, handle: Handle) -> Option<f64> {
        self.voices.get(handle).map(|v| v.loop_point)
    }

    /// Whether the voice stops once its sound has played out.
    pub fn auto_stop(&self, handle: Handle) -> Option<bool> {
        self.voices
            .get(handle)
            .map(|v| !v.has_flag(flags::DISABLE_AUTOSTOP))
    }

    /// Whether the voice is protected.
    pub fn is_protected(&self, handle: Handle) -> Option<bool> {
        self.voices
            .get(handle)
            .map(|v| v.has_flag(flags::PROTECTED))
    }

    /// The speed last set for the voice.
    pub fn relative_play_speed(&self, handle: Handle) -> Option<f32> {
        self.voices.get(handle).map(|v| v.set_relative_play_speed)
    }

    /// The sample rate the voice's sound plays at before speed
    /// (`Soloud::getSamplerate`).
    pub fn sample_rate(&self, handle: Handle) -> Option<f32> {
        self.voices.get(handle).map(|v| v.base_sample_rate)
    }

    /// Seconds of output the voice has been mixed for while not paused.
    pub fn voice_stream_time(&self, handle: Handle) -> Option<f64> {
        self.voices.get(handle).map(|v| v.stream_time)
    }

    /// Seconds into the sound, accounting for play speed; resets on loop.
    pub fn stream_position(&self, handle: Handle) -> Option<f64> {
        self.voices.get(handle).map(|v| v.stream_position)
    }

    /// How many times the voice has looped.
    pub fn loop_count(&self, handle: Handle) -> Option<u32> {
        self.voices.get(handle).map(|v| v.loop_count)
    }

    /// A value the voice's source reports under `key` (`Soloud::getInfo`).
    /// Sounds report nothing, so this is 0 for every voice today.
    pub fn info(&self, handle: Handle, _key: u32) -> Option<f32> {
        self.voices.get(handle).map(|_| 0.0)
    }

    /// Voices playing or paused.
    pub fn voice_count(&self) -> usize {
        self.voices.count()
    }

    /// Voices of `source` and its clones, playing or paused
    /// (`Soloud::countAudioSource`).
    pub fn count_sound(&self, source: &impl Playable) -> usize {
        let id = source.source_id();
        self.voices.slots[..self.voices.highest]
            .iter()
            .flatten()
            .filter(|v| v.sound_id == id)
            .count()
    }

    /// Voices that the next mix renders. Updates the active set if voices
    /// changed since the last mix, as `Soloud::getActiveVoiceCount` does.
    pub fn active_voice_count(&mut self) -> usize {
        if self.voices.dirty {
            self.voices.calc_active();
        }
        self.voices.active_count
    }

    /// Seconds of output mixed since the mixer was created.
    pub fn stream_time(&self) -> f64 {
        self.stream_time
    }

    /// The global volume, including the current point of a fade.
    pub fn global_volume(&self) -> f32 {
        self.global_volume
    }

    /// The scale applied after clipping.
    pub fn post_clip_scaler(&self) -> f32 {
        self.post_clip_scaler
    }

    /// The interpolation used for every voice.
    pub fn main_resampler(&self) -> Resampler {
        self.resampler
    }

    /// The active voice limit.
    pub fn max_active_voice_count(&self) -> u32 {
        self.voices.max_active() as u32
    }
}
