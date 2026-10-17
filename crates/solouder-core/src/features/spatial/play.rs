// Ported from SoLoud src/core/soloud_core_3d.cpp and src/core/soloud_bus.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::scene::Frame;
use crate::dsp::c_int;
use crate::features::playback::{Bus, Handle, Mixer, PlayOptions, Playable, Voice, flags};

/// `update_3d_audio` marks voices whose overall volume is below this
/// inaudible (#240: SoLoud hard-codes it).
pub(crate) const UPDATE_INAUDIBLE: f32 = 0.001;
/// `play_3d` uses a higher threshold than `update_3d_audio` (#240).
pub(crate) const PLAY_INAUDIBLE: f32 = 0.01;

/// Starting 3D voices and updating their gains.
impl Mixer {
    /// Starts `source` at `position`, moving at `velocity` units per second
    /// (`Soloud::play3d`). The voice's attenuation, speaker gains and
    /// Doppler factor are computed before its first frame, so it never
    /// starts at full volume (#95). A voice below 0.01 overall volume starts
    /// inaudible, and is stopped at once if its inaudible behavior kills
    /// (the returned handle is then already invalid). A source with
    /// distance delay starts late by its distance from the listener over
    /// the speed of sound.
    ///
    /// `options.pan` is ignored: the speaker gains come from the position.
    /// `options.volume` of `None` uses the source's volume, where SoLoud's
    /// `play3d` defaults to 1 (#381). `options.bus` plays through a bus
    /// voice, as [`Mixer::bus_play_3d`] does.
    pub fn play_3d(
        &mut self,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Handle {
        let handle = self.start_3d(source, position, velocity, options);
        let Some(slot) = self.voices.slot_of(handle) else {
            return handle;
        };
        let delay = self.distance_delay(slot);
        self.place_3d(slot);
        self.set_delay_samples(handle, delay as u32);
        self.set_pause(handle, options.paused);
        handle
    }

    /// [`Mixer::play_3d`] delayed by `sound_time`, as
    /// [`Mixer::play_clocked`] delays a play (`Soloud::play3dClocked`); a
    /// distance delay adds to it. The voice starts paused if
    /// `options.paused` (SoLoud's call has no such option).
    pub fn play_3d_clocked(
        &mut self,
        sound_time: f64,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Handle {
        let handle = self.start_3d(source, position, velocity, options);
        // As in SoLoud, a play that failed leaves the clock alone.
        let Some(slot) = self.voices.slot_of(handle) else {
            return handle;
        };
        let clocked = self.clocked_delay(sound_time) as i32;
        let delay = clocked.wrapping_add(self.distance_delay(slot));
        self.place_3d(slot);
        self.set_delay_samples(handle, delay as u32);
        self.set_pause(handle, options.paused);
        handle
    }

    /// [`Mixer::play_3d`] through `bus` (`Bus::play3d`); nothing plays, and
    /// [`Handle::INVALID`] is returned, unless the bus is playing on this
    /// mixer. `options.bus` is replaced.
    pub fn bus_play_3d(
        &mut self,
        bus: &Bus,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Handle {
        match self.bus_handle_of(bus.id()) {
            Some(handle) => self.play_3d(
                source,
                position,
                velocity,
                PlayOptions {
                    bus: handle,
                    ..options
                },
            ),
            None => Handle::INVALID,
        }
    }

    /// [`Mixer::play_3d_clocked`] through `bus` (`Bus::play3dClocked`), as
    /// [`Mixer::bus_play_3d`].
    pub fn bus_play_3d_clocked(
        &mut self,
        bus: &Bus,
        sound_time: f64,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Handle {
        match self.bus_handle_of(bus.id()) {
            Some(handle) => self.play_3d_clocked(
                sound_time,
                source,
                position,
                velocity,
                PlayOptions {
                    bus: handle,
                    ..options
                },
            ),
            None => Handle::INVALID,
        }
    }

    /// Recomputes the attenuation, speaker gains and Doppler factor of every
    /// 3D voice from the current listener, speakers and voice positions and
    /// velocities (`Soloud::update3dAudio`). Call it after moving the
    /// listener or voices; nothing else does. The new gains ramp in over
    /// the next mix; the new speed applies at once. Voices below 0.001
    /// overall volume become inaudible (and stop, if their inaudible
    /// behavior kills); the others become audible again.
    pub fn update_3d_audio(&mut self) {
        let frame = self.scene.frame();
        let voices = &mut self.voices;
        for slot in 0..voices.highest {
            let Some(voice) = voices.slots[slot].as_mut() else {
                continue;
            };
            if !voice.has_flag(flags::PROCESS_3D) {
                continue;
            }
            apply_gains(&frame, voice, slot);
            voice.update_speed();
            // As in SoLoud, a paused voice's gains jump to the new volume
            // before the speaker gains change, so it ramps from there when
            // it resumes.
            voice.update_volume();
            voice.channel_volume = voice.spatial.channel_volume;
            if mark_inaudible(voice, UPDATE_INAUDIBLE) {
                voices.stop(slot);
            }
        }
        voices.dirty = true;
    }

    /// Plays `source` paused, with pan 0, as a 3D voice at `position`.
    fn start_3d(
        &mut self,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Handle {
        let start = PlayOptions {
            pan: 0.0,
            paused: true,
            ..options
        };
        let handle = self.play_with(source, start);
        if let Some(slot) = self.voices.slot_of(handle)
            && let Some(voice) = self.voices.slots[slot].as_mut()
        {
            voice.set_flag(flags::PROCESS_3D, true);
            voice.spatial.position = position.into();
            voice.spatial.velocity = velocity.into();
        }
        handle
    }

    /// The frames the voice in `slot` starts late by, when its source has
    /// distance delay: its distance from the listener (or its position's
    /// length, if listener-relative) over the speed of sound. As in SoLoud,
    /// this converts to `int` as x86 does, so delays of `i32::MAX` frames
    /// or more (13.5 hours at 44.1 kHz) wrap.
    fn distance_delay(&self, slot: usize) -> i32 {
        let Some(voice) = self.voices.slots[slot].as_ref() else {
            return 0;
        };
        if !voice.spatial.distance_delay {
            return 0;
        }
        let mut offset = voice.spatial.position;
        if !voice.has_flag(flags::LISTENER_RELATIVE) {
            offset = offset.sub(self.scene.position);
        }
        let rate = self.config.sample_rate as f32;
        let frames = offset.mag() / self.scene.sound_speed * rate;
        c_int(f64::from(frames).floor())
    }

    /// The rest of `play3d` for a voice just started: its gains and speed
    /// before its first frame, and the 0.01 inaudibility threshold.
    fn place_3d(&mut self, slot: usize) {
        let frame = self.scene.frame();
        let voices = &mut self.voices;
        let Some(voice) = voices.slots[slot].as_mut() else {
            return;
        };
        apply_gains(&frame, voice, slot);
        voice.update_speed();
        voice.channel_volume = voice.spatial.channel_volume;
        voice.update_volume();
        // Start at the computed gains rather than ramping up to them.
        voice.snap_gains();
        let kill = mark_inaudible(voice, PLAY_INAUDIBLE);
        voices.dirty = true;
        if kill {
            voices.stop(slot);
        }
    }
}

/// Computes the voice's 3D volume, speaker gains and Doppler factor into
/// its 3D state (`update3dVoices_internal`).
fn apply_gains(frame: &Frame, voice: &mut Voice, slot: usize) {
    let handle = Handle::new(slot, voice.play_index & 0xfffff);
    let relative = voice.has_flag(flags::LISTENER_RELATIVE);
    let gains = frame.gains(&voice.spatial, relative, handle);
    voice.spatial.volume = gains.volume;
    voice.spatial.doppler = gains.doppler;
    voice.spatial.channel_volume = gains.speakers;
}

/// Sets or clears the voice's inaudible flag by its overall volume; true if
/// the voice should now be stopped.
fn mark_inaudible(voice: &mut Voice, threshold: f32) -> bool {
    let inaudible = voice.overall_volume < threshold;
    voice.set_flag(flags::INAUDIBLE, inaudible);
    inaudible && voice.has_flag(flags::INAUDIBLE_KILL)
}
