// Ported from SoLoud src/core/soloud_core_basicops.cpp,
// src/core/soloud_core_setters.cpp and src/core/soloud_core_getters.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::fmt;

use super::handle::{Handle, next_play_index};
use super::mixer::Mixer;
use super::sound::Sound;
use super::voice::{Voice, flags};
use crate::dsp::Resampler;

/// How [`Mixer::play_with`] starts a voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    /// Voice volume; `None` uses [`Sound::volume`].
    pub volume: Option<f32>,
    /// Stereo pan from -1 (left) to 1 (right).
    pub pan: f32,
    /// Start paused; the voice keeps its slot but is not mixed.
    pub paused: bool,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            volume: None,
            pan: 0.0,
            paused: false,
        }
    }
}

/// A control call that SoLoud would answer with `INVALID_PARAMETER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControlError {
    /// Play speeds must be greater than 0.
    InvalidSpeed,
    /// The active voice limit must be 1 to `VOICE_COUNT - 1`.
    InvalidVoiceCount,
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpeed => write!(f, "play speed must be greater than 0"),
            Self::InvalidVoiceCount => write!(f, "active voice limit must be 1 to 1023"),
        }
    }
}

impl std::error::Error for ControlError {}

/// Playing and controlling voices. Calls with a handle whose voice has
/// stopped do nothing; getters then return `None`.
impl Mixer {
    /// Starts `sound` with default options (`Soloud::play`).
    pub fn play(&mut self, sound: &Sound) -> Handle {
        self.play_with(sound, PlayOptions::default())
    }

    /// Starts `sound`. When all `VOICE_COUNT` slots are taken, the oldest
    /// unprotected voice is stopped to make room; if every voice is protected,
    /// nothing plays and [`Handle::INVALID`] is returned.
    pub fn play_with(&mut self, sound: &Sound, options: PlayOptions) -> Handle {
        let Some(slot) = self.voices.find_free() else {
            return Handle::INVALID;
        };
        let play_index = self.voices.play_index;
        self.voices.play_index = next_play_index(play_index);
        let mut voice = Voice::new(sound, play_index);
        voice.set_flag(flags::PAUSED, options.paused);
        voice.set_pan(options.pan);
        voice.set_volume(options.volume.unwrap_or(sound.volume()));
        // Start at full volume rather than ramping up from silence.
        voice.snap_gains();
        voice.set_relative_play_speed(1.0);
        self.voices.slots[slot] = Some(voice);
        self.voices.dirty = true;
        Handle::new(slot, play_index)
    }

    /// Stops a voice immediately.
    pub fn stop(&mut self, handle: Handle) {
        if let Some(slot) = self.voices.slot_of(handle) {
            self.voices.stop(slot);
        }
    }

    /// Stops every voice.
    pub fn stop_all(&mut self) {
        for slot in 0..self.voices.highest {
            self.voices.stop(slot);
        }
    }

    /// Sets a voice's volume; the change ramps in over the next mix.
    pub fn set_volume(&mut self, handle: Handle, volume: f32) {
        if let Some(voice) = self.voices.get_mut(handle) {
            voice.set_volume(volume);
            self.voices.dirty = true;
        }
    }

    /// Sets a voice's pan; the change ramps in over the next mix.
    pub fn set_pan(&mut self, handle: Handle, pan: f32) {
        if let Some(voice) = self.voices.get_mut(handle) {
            voice.set_pan(pan);
        }
    }

    /// Pauses or resumes a voice. A paused voice keeps its position.
    pub fn set_pause(&mut self, handle: Handle, paused: bool) {
        if let Some(voice) = self.voices.get_mut(handle) {
            voice.set_flag(flags::PAUSED, paused);
            self.voices.dirty = true;
        }
    }

    /// Sets whether a voice loops back to the start when it reaches the end.
    pub fn set_looping(&mut self, handle: Handle, looping: bool) {
        self.with_voice(handle, |voice| voice.set_flag(flags::LOOPING, looping));
    }

    /// Protected voices are never stopped to make room for new ones and are
    /// always mixed, even past the active voice limit.
    pub fn set_protect_voice(&mut self, handle: Handle, protect: bool) {
        self.with_voice(handle, |voice| voice.set_flag(flags::PROTECTED, protect));
    }

    /// Delays the start of a voice by `frames` output frames.
    pub fn set_delay_samples(&mut self, handle: Handle, frames: u32) {
        self.with_voice(handle, |voice| voice.delay_samples = frames);
    }

    /// Scales a voice's playback rate, changing pitch and speed together.
    pub fn set_relative_play_speed(
        &mut self,
        handle: Handle,
        speed: f32,
    ) -> Result<(), ControlError> {
        match self.voices.get_mut(handle) {
            Some(voice) => voice
                .set_relative_play_speed(speed)
                .then_some(())
                .ok_or(ControlError::InvalidSpeed),
            None => Ok(()),
        }
    }

    /// Volume applied to the whole mix before clipping.
    pub fn set_global_volume(&mut self, volume: f32) {
        self.global_volume = volume;
    }

    /// Scale applied after clipping; 0.95 by default.
    pub fn set_post_clip_scaler(&mut self, scale: f32) {
        self.post_clip_scaler = scale;
    }

    /// Interpolation used for every voice.
    pub fn set_resampler(&mut self, resampler: Resampler) {
        self.resampler = resampler;
    }

    /// How many voices are mixed; the rest play silently paused in place
    /// until they are among the loudest again. 16 by default.
    pub fn set_max_active_voice_count(&mut self, count: u32) -> Result<(), ControlError> {
        if self.voices.set_max_active(count as usize) {
            Ok(())
        } else {
            Err(ControlError::InvalidVoiceCount)
        }
    }

    fn with_voice(&mut self, handle: Handle, change: impl FnOnce(&mut Voice)) {
        if let Some(voice) = self.voices.get_mut(handle) {
            change(voice);
        }
    }
}

/// Queries.
impl Mixer {
    /// Whether `handle` refers to a voice that has not stopped.
    pub fn is_valid_voice_handle(&self, handle: Handle) -> bool {
        self.voices.get(handle).is_some()
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

    /// The sample rate of the voice's sound (`Soloud::getSamplerate`).
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

    /// Voices playing or paused.
    pub fn voice_count(&self) -> usize {
        self.voices.count()
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
}
