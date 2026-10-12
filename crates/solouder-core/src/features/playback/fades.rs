// Ported from SoLoud src/core/soloud_core_faderops.cpp,
// src/core/soloud_core_basicops.cpp and src/core/soloud.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::control::PlayOptions;
use super::handle::Handle;
use super::mixer::Mixer;
use super::sound::Sound;
use super::voice::flags;
use super::voices::Voices;
use crate::dsp::fader::{Fader, FaderState};

/// Most frames [`Mixer::play_clocked`] delays a voice by.
const MAX_CLOCKED_DELAY: f64 = 2048.0;

/// Fades, oscillations and timers. They run on each voice's stream time,
/// which advances only while the voice is not paused; the global volume runs
/// on the mixer's stream time. A time of 0 or less applies the change at once.
impl Mixer {
    /// Fades the voice's volume to `to` over `seconds`. Setting the volume
    /// cancels the fade; so does fading to the current volume.
    pub fn fade_volume(&mut self, handle: Handle, to: f32, seconds: f64) {
        let from = self.volume(handle).unwrap_or(0.0);
        if seconds <= 0.0 || to == from {
            return self.set_volume(handle, to);
        }
        self.with_voice(handle, |voice| {
            voice
                .faders
                .volume
                .set(from, to, seconds, voice.stream_time)
        });
    }

    /// Fades the voice's pan to `to` over `seconds`. As in SoLoud,
    /// [`Mixer::set_pan`] does not cancel the fade.
    pub fn fade_pan(&mut self, handle: Handle, to: f32, seconds: f64) {
        let from = self.pan(handle).unwrap_or(0.0);
        if seconds <= 0.0 || to == from {
            return self.set_pan(handle, to);
        }
        self.with_voice(handle, |voice| {
            voice.faders.pan.set(from, to, seconds, voice.stream_time)
        });
    }

    /// Fades the voice's relative play speed to `to` over `seconds`. Values
    /// the fade passes through that are not greater than 0 are skipped.
    pub fn fade_relative_play_speed(&mut self, handle: Handle, to: f32, seconds: f64) {
        let from = self.relative_play_speed(handle).unwrap_or(1.0);
        if seconds <= 0.0 || to == from {
            let _ = self.set_relative_play_speed(handle, to);
            return;
        }
        self.with_voice(handle, |voice| {
            voice.faders.speed.set(from, to, seconds, voice.stream_time)
        });
    }

    /// Fades the global volume to `to` over `seconds` of mixed output.
    pub fn fade_global_volume(&mut self, to: f32, seconds: f64) {
        let from = self.global_volume;
        if seconds <= 0.0 || to == from {
            return self.set_global_volume(to);
        }
        self.global_fader.set(from, to, seconds, self.stream_time);
    }

    /// Swings the voice's volume between `from` and `to`, one cycle every
    /// `seconds`, starting halfway and rising.
    pub fn oscillate_volume(&mut self, handle: Handle, from: f32, to: f32, seconds: f64) {
        if seconds <= 0.0 || to == from {
            return self.set_volume(handle, to);
        }
        self.with_voice(handle, |voice| {
            voice
                .faders
                .volume
                .set_lfo(from, to, seconds, voice.stream_time)
        });
    }

    /// Swings the voice's pan between `from` and `to`, one cycle every
    /// `seconds`.
    pub fn oscillate_pan(&mut self, handle: Handle, from: f32, to: f32, seconds: f64) {
        if seconds <= 0.0 || to == from {
            return self.set_pan(handle, to);
        }
        self.with_voice(handle, |voice| {
            voice
                .faders
                .pan
                .set_lfo(from, to, seconds, voice.stream_time)
        });
    }

    /// Swings the voice's relative play speed between `from` and `to`, one
    /// cycle every `seconds`.
    pub fn oscillate_relative_play_speed(
        &mut self,
        handle: Handle,
        from: f32,
        to: f32,
        seconds: f64,
    ) {
        if seconds <= 0.0 || to == from {
            let _ = self.set_relative_play_speed(handle, to);
            return;
        }
        self.with_voice(handle, |voice| {
            voice
                .faders
                .speed
                .set_lfo(from, to, seconds, voice.stream_time)
        });
    }

    /// Swings the global volume between `from` and `to`, one cycle every
    /// `seconds`.
    pub fn oscillate_global_volume(&mut self, from: f32, to: f32, seconds: f64) {
        if seconds <= 0.0 || to == from {
            return self.set_global_volume(to);
        }
        self.global_fader
            .set_lfo(from, to, seconds, self.stream_time);
    }

    /// Pauses the voice after `seconds`. Pausing or resuming it cancels this.
    pub fn schedule_pause(&mut self, handle: Handle, seconds: f64) {
        if seconds <= 0.0 {
            return self.set_pause(handle, true);
        }
        self.with_voice(handle, |voice| {
            voice.faders.pause.set(1.0, 0.0, seconds, voice.stream_time)
        });
    }

    /// Stops the voice after `seconds`.
    pub fn schedule_stop(&mut self, handle: Handle, seconds: f64) {
        if seconds <= 0.0 {
            return self.stop(handle);
        }
        self.with_voice(handle, |voice| {
            voice.faders.stop.set(1.0, 0.0, seconds, voice.stream_time)
        });
    }

    /// Plays `sound` delayed by `sound_time` minus the time of the first
    /// clocked play since the last mix, so sounds started from one game
    /// update keep their spacing (`Soloud::playClocked`). Delays outside 0 to
    /// 2048 frames are dropped.
    pub fn play_clocked(&mut self, sound_time: f64, sound: &Sound, options: PlayOptions) -> Handle {
        let handle = self.play_with(
            sound,
            PlayOptions {
                paused: true,
                ..options
            },
        );
        if self.last_clocked_time == 0.0 {
            self.last_clocked_time = sound_time;
        }
        let frames =
            ((sound_time - self.last_clocked_time) * f64::from(self.config.sample_rate)).floor();
        let frames = if (0.0..=MAX_CLOCKED_DELAY).contains(&frames) {
            frames as u32
        } else {
            0
        };
        self.set_delay_samples(handle, frames);
        self.set_pause(handle, options.paused);
        handle
    }
}

/// Advances every unpaused voice's clocks by `buffer_time` seconds and
/// applies its fades and timers, in SoLoud's order: speed, volume, pan,
/// scheduled pause, scheduled stop.
pub(super) fn advance_voices(voices: &mut Voices, buffer_time: f32) {
    for slot in 0..voices.highest {
        let Some(voice) = voices.slots[slot].as_mut() else {
            continue;
        };
        if voice.has_flag(flags::PAUSED) {
            continue;
        }
        voice.stream_time += f64::from(buffer_time);
        voice.stream_position +=
            f64::from(buffer_time) * f64::from(voice.overall_relative_play_speed);
        let now = voice.stream_time;
        if voice.faders.speed.is_running() {
            let speed = voice.faders.speed.get(now);
            voice.set_relative_play_speed(speed);
        }
        let mut dirty = false;
        if voice.faders.volume.is_running() {
            let volume = voice.faders.volume.get(now);
            voice.set_volume(volume);
            dirty = true;
        }
        if voice.faders.pan.is_running() {
            let pan = voice.faders.pan.get(now);
            voice.set_pan(pan);
        }
        if timer_fired(&mut voice.faders.pause, now) {
            voice.set_flag(flags::PAUSED, true);
            dirty = true;
        }
        let stop = timer_fired(&mut voice.faders.stop, now);
        voices.dirty |= dirty;
        if stop {
            voices.stop(slot);
        }
    }
}

/// Runs a scheduled pause or stop; true (and the timer cleared) once it ends.
fn timer_fired(timer: &mut Fader, now: f64) -> bool {
    if timer.state == FaderState::Off {
        return false;
    }
    timer.get(now);
    let fired = timer.state == FaderState::Finished;
    if fired {
        timer.state = FaderState::Off;
    }
    fired
}
