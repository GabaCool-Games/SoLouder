// Ported from SoLoud src/core/soloud_core_3d.cpp and src/core/soloud_bus.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::scene::{Gains, Scene};
use super::settings::VoiceSpatial;
use crate::dsp::c_int;
use crate::features::playback::{Bus, Handle, Mixer, PlayOptions, Playable, Voice, flags};

/// `update_3d_audio` marks voices whose overall volume is below this
/// inaudible (#240: SoLoud hard-codes it).
pub(crate) const UPDATE_INAUDIBLE: f32 = 0.001;
/// `play_3d` uses a higher threshold than `update_3d_audio` (#240).
pub(crate) const PLAY_INAUDIBLE: f32 = 0.01;

/// What a 3D play needs once its voice has started paused: computed on the
/// mixer's thread by [`Mixer::play_3d`], or on the game thread by the
/// real-time controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    pub gains: Gains,
    /// The distance delay in frames, wrapped as x86 converts to `int`.
    pub distance_delay: i32,
    /// The sound time of a clocked play.
    pub clock: Option<f64>,
    /// Whether the voice stays paused once placed.
    pub paused: bool,
}

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
        if let Some(slot) = self.voices.slot_of(handle) {
            let placement = self.placement(slot, None, options.paused);
            self.place_3d(handle, slot, placement);
        }
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
        if let Some(slot) = self.voices.slot_of(handle) {
            let placement = self.placement(slot, Some(sound_time), options.paused);
            self.place_3d(handle, slot, placement);
        }
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
    ///
    /// As in SoLoud, every voice's gains are computed, running its collider
    /// and attenuator, before any is applied: a 3D bus killed for being
    /// inaudible stops its voices only after their colliders ran.
    pub fn update_3d_audio(&mut self) {
        let frame = self.scene.frame();
        let voices = &mut self.voices;
        let updates = &mut self.spatial_updates;
        updates.clear();
        for slot in 0..voices.highest {
            let Some(voice) = voices.slots[slot].as_mut() else {
                continue;
            };
            if !voice.has_flag(flags::PROCESS_3D) {
                continue;
            }
            let handle = Handle::new(slot, voice.play_index & 0xfffff);
            let relative = voice.has_flag(flags::LISTENER_RELATIVE);
            let gains = frame.gains(&voice.spatial, relative, handle, None);
            updates.push((slot, gains));
        }
        for &(slot, gains) in updates.iter() {
            // A voice stopped by an earlier kill (a bus's) is skipped.
            let Some(voice) = voices.slots[slot].as_mut() else {
                continue;
            };
            if apply_update(voice, &gains) {
                voices.stop(slot);
            }
        }
        voices.dirty = true;
    }

    /// One voice of a 3D update whose gains the real-time controller
    /// computed: the voice `handle` names, if it still plays in 3D.
    pub(crate) fn apply_3d_update(&mut self, handle: Handle, gains: &Gains) {
        let Some(slot) = self.voices.slot_of(handle) else {
            return;
        };
        let Some(voice) = self.voices.slots[slot].as_mut() else {
            return;
        };
        if voice.has_flag(flags::PROCESS_3D) && apply_update(voice, gains) {
            self.voices.stop(slot);
        }
    }

    /// Ends a 3D update applied with [`Mixer::apply_3d_update`]: the active
    /// voices are chosen again at the next mix, as after
    /// [`Mixer::update_3d_audio`].
    pub(crate) fn finish_3d_update(&mut self) {
        self.voices.dirty = true;
    }

    /// Places a voice the real-time controller started paused for a 3D
    /// play, with the gains it computed; does nothing if the play failed.
    pub(crate) fn place_started_3d(&mut self, handle: Handle, placement: Placement) {
        if let Some(slot) = self.voices.slot_of(handle) {
            self.place_3d(handle, slot, placement);
        }
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

    /// The gains and distance delay of the voice just started in `slot`.
    fn placement(&self, slot: usize, clock: Option<f64>, paused: bool) -> Placement {
        let voice = self.voices.slots[slot]
            .as_ref()
            .expect("a voice just started");
        let handle = Handle::new(slot, voice.play_index & 0xfffff);
        let relative = voice.has_flag(flags::LISTENER_RELATIVE);
        let rate = self.config.sample_rate;
        Placement {
            gains: self
                .scene
                .frame()
                .gains(&voice.spatial, relative, handle, None),
            distance_delay: distance_delay(&voice.spatial, relative, &self.scene, rate),
            clock,
            paused,
        }
    }

    /// The rest of `play3d` for a voice just started paused in `slot`: its
    /// gains and speed before its first frame, the 0.01 inaudibility
    /// threshold, its delay, then its pause state. A clocked play reads the
    /// clock here, so a play that failed leaves it alone.
    fn place_3d(&mut self, handle: Handle, slot: usize, placement: Placement) {
        let mut delay = placement.distance_delay;
        if let Some(sound_time) = placement.clock {
            delay = (self.clocked_delay(sound_time) as i32).wrapping_add(delay);
        }
        let voices = &mut self.voices;
        let Some(voice) = voices.slots[slot].as_mut() else {
            return;
        };
        set_gains(voice, &placement.gains);
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
        self.set_delay_samples(handle, delay as u32);
        self.set_pause(handle, placement.paused);
    }
}

/// The frames a 3D voice starts late by when its source has distance delay:
/// its distance from the listener (or its position's length, if
/// listener-relative) over the speed of sound, at `rate` frames per second.
/// As in SoLoud this converts to `int` as x86 does, so delays of `i32::MAX`
/// frames or more (13.5 hours at 44.1 kHz) become `i32::MIN`, which the
/// voice's unsigned delay takes as 2^31 frames: the voice starts about 13.5
/// hours late, not at once.
pub(crate) fn distance_delay(
    spatial: &VoiceSpatial,
    listener_relative: bool,
    scene: &Scene,
    rate: u32,
) -> i32 {
    if !spatial.distance_delay {
        return 0;
    }
    let mut offset = spatial.position;
    if !listener_relative {
        offset = offset.sub(scene.position);
    }
    let frames = offset.mag() / scene.sound_speed * rate as f32;
    c_int(f64::from(frames).floor())
}

/// Stores a voice's 3D volume, speaker gains and Doppler factor
/// (`update3dVoices_internal`'s results).
fn set_gains(voice: &mut Voice, gains: &Gains) {
    voice.spatial.volume = gains.volume;
    voice.spatial.doppler = gains.doppler;
    voice.spatial.channel_volume = gains.speakers;
}

/// One voice of `update3dAudio`'s last step; true if the voice should now be
/// stopped.
fn apply_update(voice: &mut Voice, gains: &Gains) -> bool {
    set_gains(voice, gains);
    voice.update_speed();
    // As in SoLoud, a paused voice's gains jump to the new volume before the
    // speaker gains change, so it ramps from there when it resumes.
    voice.update_volume();
    voice.channel_volume = voice.spatial.channel_volume;
    mark_inaudible(voice, UPDATE_INAUDIBLE)
}

/// Sets or clears the voice's inaudible flag by its overall volume; true if
/// the voice should now be stopped.
fn mark_inaudible(voice: &mut Voice, threshold: f32) -> bool {
    let inaudible = voice.overall_volume < threshold;
    voice.set_flag(flags::INAUDIBLE, inaudible);
    inaudible && voice.has_flag(flags::INAUDIBLE_KILL)
}
