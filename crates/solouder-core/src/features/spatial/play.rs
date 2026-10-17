// Ported from SoLoud src/core/soloud_core_3d.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use crate::features::playback::{Handle, Mixer, PlayOptions, Playable, Voice, flags};

/// `update_3d_audio` marks voices whose overall volume is below this
/// inaudible (#240: SoLoud hard-codes it).
pub(crate) const UPDATE_INAUDIBLE: f32 = 0.001;
/// `play_3d` uses a higher threshold than `update_3d_audio` (#240).
pub(crate) const PLAY_INAUDIBLE: f32 = 0.01;

/// Starting 3D voices and updating their gains.
impl Mixer {
    /// Starts `source` at `position` (`Soloud::play3d`). The voice's
    /// attenuation and speaker gains are computed before its first frame, so
    /// it never starts at full volume (#95). A voice below 0.01 overall
    /// volume starts inaudible, and is stopped at once if its inaudible
    /// behavior kills (the returned handle is then already invalid).
    ///
    /// `options.pan` is ignored: the speaker gains come from the position.
    /// `options.volume` of `None` uses the source's volume, where SoLoud's
    /// `play3d` defaults to 1 (#381).
    pub fn play_3d(
        &mut self,
        source: &impl Playable,
        position: [f32; 3],
        options: PlayOptions,
    ) -> Handle {
        let start = PlayOptions {
            pan: 0.0,
            paused: true,
            ..options
        };
        let handle = self.play_with(source, start);
        let Some(slot) = self.voices.slot_of(handle) else {
            return handle;
        };
        let frame = self.scene.frame();
        let voices = &mut self.voices;
        let Some(voice) = voices.slots[slot].as_mut() else {
            return handle;
        };
        voice.set_flag(flags::PROCESS_3D, true);
        voice.spatial.position = position.into();
        let spatial = voice.spatial;
        let gains = frame.gains(&spatial, voice.has_flag(flags::LISTENER_RELATIVE));
        voice.spatial.volume = gains.volume;
        voice.channel_volume = gains.speakers;
        voice.update_volume();
        // Start at the computed gains rather than ramping up to them.
        voice.snap_gains();
        let kill = mark_inaudible(voice, PLAY_INAUDIBLE);
        voices.dirty = true;
        if kill {
            voices.stop(slot);
        }
        self.set_pause(handle, options.paused);
        handle
    }

    /// Recomputes the attenuation and speaker gains of every 3D voice from
    /// the current listener, speakers and voice positions
    /// (`Soloud::update3dAudio`). Call it after moving the listener or
    /// voices; nothing else does. The new gains ramp in over the next mix.
    /// Voices below 0.001 overall volume become inaudible (and stop, if
    /// their inaudible behavior kills); the others become audible again.
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
            let spatial = voice.spatial;
            let gains = frame.gains(&spatial, voice.has_flag(flags::LISTENER_RELATIVE));
            voice.spatial.volume = gains.volume;
            // As in SoLoud, a paused voice's gains jump to the new volume
            // before the speaker gains change, so it ramps from there when
            // it resumes.
            voice.update_volume();
            voice.channel_volume = gains.speakers;
            if mark_inaudible(voice, UPDATE_INAUDIBLE) {
                voices.stop(slot);
            }
        }
        voices.dirty = true;
    }
}

/// Sets or clears the voice's inaudible flag by its overall volume; true if
/// the voice should now be stopped.
fn mark_inaudible(voice: &mut Voice, threshold: f32) -> bool {
    let inaudible = voice.overall_volume < threshold;
    voice.set_flag(flags::INAUDIBLE, inaudible);
    inaudible && voice.has_flag(flags::INAUDIBLE_KILL)
}
