// Ported from SoLoud src/core/soloud_core_3d.cpp,
// src/core/soloud_core_getters.cpp and src/core/soloud_core_setters.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::attenuation::Attenuation;
use super::settings::VoiceSpatial;
use crate::features::playback::{ControlError, Handle, Mixer, flags};

/// The listener, the speakers, and the 3D settings of playing voices. None
/// of these change what is heard until the next
/// [`Mixer::update_3d_audio`] (or `play_3d`).
impl Mixer {
    /// Places the listener at `position`, facing `at`, with `up` as its up
    /// direction, moving at `velocity` units per second
    /// (`Soloud::set3dListenerParameters`). `at` and `up` need not be
    /// normalized. By default the listener is still at the origin facing
    /// -z with +y up, in right-handed coordinates (see
    /// [`MixerConfig::left_handed_3d`](crate::MixerConfig::left_handed_3d)).
    pub fn set_3d_listener_parameters(
        &mut self,
        position: [f32; 3],
        at: [f32; 3],
        up: [f32; 3],
        velocity: [f32; 3],
    ) {
        self.set_3d_listener_position(position);
        self.set_3d_listener_at(at);
        self.set_3d_listener_up(up);
        self.set_3d_listener_velocity(velocity);
    }

    /// Moves the listener (`Soloud::set3dListenerPosition`).
    pub fn set_3d_listener_position(&mut self, position: [f32; 3]) {
        self.scene.position = position.into();
    }

    /// Turns the listener to face `at` (`Soloud::set3dListenerAt`).
    pub fn set_3d_listener_at(&mut self, at: [f32; 3]) {
        self.scene.at = at.into();
    }

    /// Sets the listener's up direction (`Soloud::set3dListenerUp`).
    pub fn set_3d_listener_up(&mut self, up: [f32; 3]) {
        self.scene.up = up.into();
    }

    /// Sets the listener's velocity in units per second, for Doppler
    /// (`Soloud::set3dListenerVelocity`). Nothing moves the listener by it.
    pub fn set_3d_listener_velocity(&mut self, velocity: [f32; 3]) {
        self.scene.velocity = velocity.into();
    }

    /// The speed of sound in units per second; 343.3 (meters in air) by
    /// default (`Soloud::get3dSoundSpeed`).
    pub fn sound_speed_3d(&self) -> f32 {
        self.scene.sound_speed
    }

    /// Sets the speed of sound in units per second, for Doppler and
    /// distance delay (`Soloud::set3dSoundSpeed`). It must be finite and
    /// greater than 0 (SoLoud took NaN and infinity until reference patch
    /// 0064); otherwise nothing changes and
    /// [`ControlError::InvalidSoundSpeed`] is returned.
    pub fn set_3d_sound_speed(&mut self, speed: f32) -> Result<(), ControlError> {
        if self.scene.set_sound_speed(speed) {
            Ok(())
        } else {
            Err(ControlError::InvalidSoundSpeed)
        }
    }

    /// Moves a 3D voice and sets its velocity
    /// (`Soloud::set3dSourceParameters`). Like every 3D voice setter, it
    /// does nothing for voices not started with [`Mixer::play_3d`]; a voice
    /// group handle changes each 3D voice in the group.
    pub fn set_3d_source_parameters(
        &mut self,
        handle: Handle,
        position: [f32; 3],
        velocity: [f32; 3],
    ) {
        self.with_3d_voice(handle, |spatial| {
            spatial.position = position.into();
            spatial.velocity = velocity.into();
        });
    }

    /// Moves a 3D voice (`Soloud::set3dSourcePosition`).
    pub fn set_3d_source_position(&mut self, handle: Handle, position: [f32; 3]) {
        self.with_3d_voice(handle, |spatial| spatial.position = position.into());
    }

    /// Sets a 3D voice's velocity in units per second, for Doppler
    /// (`Soloud::set3dSourceVelocity`). Nothing moves the voice by it.
    pub fn set_3d_source_velocity(&mut self, handle: Handle, velocity: [f32; 3]) {
        self.with_3d_voice(handle, |spatial| spatial.velocity = velocity.into());
    }

    /// Sets how strongly a 3D voice's pitch follows Doppler, 0 for not at
    /// all (`Soloud::set3dSourceDopplerFactor`).
    pub fn set_3d_source_doppler_factor(&mut self, handle: Handle, factor: f32) {
        self.with_3d_voice(handle, |spatial| spatial.doppler_factor = factor);
    }

    /// Sets the distances between which a 3D voice's attenuation applies
    /// (`Soloud::set3dSourceMinMaxDistance`).
    pub fn set_3d_source_min_max_distance(&mut self, handle: Handle, min: f32, max: f32) {
        self.with_3d_voice(handle, |spatial| {
            spatial.min_distance = min;
            spatial.max_distance = max;
        });
    }

    /// Sets a 3D voice's attenuation model and rolloff factor
    /// (`Soloud::set3dSourceAttenuation`).
    pub fn set_3d_source_attenuation(
        &mut self,
        handle: Handle,
        attenuation: Attenuation,
        rolloff: f32,
    ) {
        self.with_3d_voice(handle, |spatial| {
            spatial.attenuation = attenuation;
            spatial.rolloff = rolloff;
        });
    }

    /// The direction 3D voices are heard from on output channel `channel`
    /// (`Soloud::getSpeakerPosition`), or `None` past the output's channels.
    /// The defaults put stereo's left speaker at (2, 0, 1) and its right at
    /// (-2, 0, 1) (the listener faces +z in its own frame), quad's rear pair
    /// at z = -1, and 5.1's and 7.1's subwoofer at the zero vector, which
    /// hears every direction at full volume.
    pub fn speaker_position(&self, channel: u32) -> Option<[f32; 3]> {
        self.scene.speaker(channel)
    }

    /// Moves the speaker of output channel `channel`
    /// (`Soloud::setSpeakerPosition`); only the direction matters.
    pub fn set_speaker_position(
        &mut self,
        channel: u32,
        position: [f32; 3],
    ) -> Result<(), ControlError> {
        if self.scene.set_speaker(channel, position) {
            Ok(())
        } else {
            Err(ControlError::NoSuchSpeaker)
        }
    }

    fn with_3d_voice(&mut self, handle: Handle, mut change: impl FnMut(&mut VoiceSpatial)) {
        self.voices.for_each_voice(handle, |voice| {
            if voice.has_flag(flags::PROCESS_3D) {
                change(&mut voice.spatial);
            }
        });
    }
}
