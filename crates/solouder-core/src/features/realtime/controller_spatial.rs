//! Controller requests for 3D audio. Each mirrors the [`Mixer`](crate::Mixer)
//! call of the same name.
//!
//! The 3D math runs here, on the game thread, as SoLoud runs it outside its
//! mixer: the controller keeps the listener, the speakers and the settings
//! of every voice it played in 3D. Setters change only that copy, as they
//! change only the mixer's state offline: nothing is heard until
//! [`Controller::update_3d_audio`] computes every 3D voice's gains (calling
//! colliders and attenuators here) and sends them to the audio thread in
//! one command.

use super::VoiceId;
use super::command::Command;
use super::controller::{Controller, ControllerError, Start3d};
use crate::features::playback::{Bus, PlayOptions, Playable, Start};
use crate::features::spatial::{Attenuation, VoiceSpatial};

impl Controller {
    /// [`Mixer::play_3d`](crate::Mixer::play_3d): plays `source` at
    /// `position`, moving at `velocity`. Its gains, Doppler factor and
    /// distance delay are computed now, from the listener and speakers as
    /// this controller has them, and applied before its first frame.
    /// `options.pan` and `options.bus` are ignored.
    pub fn play_3d(
        &mut self,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        let at = Start3d {
            position,
            velocity,
            clock: None,
        };
        self.start(source, options, Start::Now, None, Some(at))
    }

    /// [`Mixer::play_3d_clocked`](crate::Mixer::play_3d_clocked): as
    /// [`Controller::play_3d`], delayed by the time `sound_time` lies after
    /// the previous clocked play's, plus any distance delay.
    pub fn play_3d_clocked(
        &mut self,
        sound_time: f64,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        let at = Start3d {
            position,
            velocity,
            clock: Some(sound_time),
        };
        self.start(source, options, Start::Now, None, Some(at))
    }

    /// [`Controller::play_3d`] through `bus` (`Bus::play3d`). The play fails
    /// with [`Event::Failed`](super::Event::Failed) if the bus is not
    /// playing when it is applied.
    pub fn bus_play_3d(
        &mut self,
        bus: &Bus,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        let at = Start3d {
            position,
            velocity,
            clock: None,
        };
        self.start(source, options, Start::Now, Some(bus.id()), Some(at))
    }

    /// [`Controller::play_3d_clocked`] through `bus` (`Bus::play3dClocked`),
    /// as [`Controller::bus_play_3d`].
    pub fn bus_play_3d_clocked(
        &mut self,
        bus: &Bus,
        sound_time: f64,
        source: &impl Playable,
        position: [f32; 3],
        velocity: [f32; 3],
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        let at = Start3d {
            position,
            velocity,
            clock: Some(sound_time),
        };
        self.start(source, options, Start::Now, Some(bus.id()), Some(at))
    }

    /// [`Mixer::update_3d_audio`](crate::Mixer::update_3d_audio): computes
    /// the gains and Doppler factor of every voice played in 3D through this
    /// controller whose final event has not been read, and queues them as
    /// one command; at the start of the next render they apply as they
    /// would offline, and voices too quiet to hear become inaudible (those
    /// whose inaudible behavior kills finish with
    /// [`FinishReason::Stopped`](super::FinishReason::Stopped)).
    ///
    /// Fails with [`ControllerError::QueueFull`] when the command queue is
    /// full or the renderer has not yet applied the previous
    /// [`RealtimeConfig::spatial_batches`](super::RealtimeConfig::spatial_batches)
    /// updates; nothing is lost by calling it again later, since it always
    /// sends the current state. Voices played in 3D on the `Mixer` before
    /// [`split`](super::split) are not updated.
    pub fn update_3d_audio(&mut self) -> Result<(), ControllerError> {
        if self.commands.is_abandoned() {
            return Err(ControllerError::Disconnected);
        }
        if self.commands.slots() == 0 {
            return Err(ControllerError::QueueFull);
        }
        let batch = self
            .spatial
            .take_batch()
            .ok_or(ControllerError::QueueFull)?;
        match self.commands.push(Command::Spatial(batch)) {
            Ok(()) => Ok(()),
            Err(rtrb::PushError::Full(command)) => {
                if let Command::Spatial(batch) = command {
                    self.spatial.give_back(batch);
                }
                Err(ControllerError::QueueFull)
            }
        }
    }

    /// [`Mixer::set_3d_listener_parameters`](crate::Mixer::set_3d_listener_parameters).
    pub fn set_3d_listener_parameters(
        &mut self,
        position: [f32; 3],
        at: [f32; 3],
        up: [f32; 3],
        velocity: [f32; 3],
    ) {
        let scene = &mut self.spatial.scene;
        scene.position = position.into();
        scene.at = at.into();
        scene.up = up.into();
        scene.velocity = velocity.into();
    }

    /// Moves the listener (`Soloud::set3dListenerPosition`).
    pub fn set_3d_listener_position(&mut self, position: [f32; 3]) {
        self.spatial.scene.position = position.into();
    }

    /// Turns the listener to face `at` (`Soloud::set3dListenerAt`).
    pub fn set_3d_listener_at(&mut self, at: [f32; 3]) {
        self.spatial.scene.at = at.into();
    }

    /// Sets the listener's up direction (`Soloud::set3dListenerUp`).
    pub fn set_3d_listener_up(&mut self, up: [f32; 3]) {
        self.spatial.scene.up = up.into();
    }

    /// Sets the listener's velocity, for Doppler
    /// (`Soloud::set3dListenerVelocity`).
    pub fn set_3d_listener_velocity(&mut self, velocity: [f32; 3]) {
        self.spatial.scene.velocity = velocity.into();
    }

    /// The speed of sound in units per second; 343.3 by default.
    pub fn sound_speed_3d(&self) -> f32 {
        self.spatial.scene.sound_speed
    }

    /// [`Mixer::set_3d_sound_speed`](crate::Mixer::set_3d_sound_speed);
    /// [`ControllerError::InvalidSoundSpeed`] unless the speed is finite and
    /// greater than 0.
    pub fn set_3d_sound_speed(&mut self, speed: f32) -> Result<(), ControllerError> {
        if self.spatial.scene.set_sound_speed(speed) {
            Ok(())
        } else {
            Err(ControllerError::InvalidSoundSpeed)
        }
    }

    /// [`Mixer::speaker_position`](crate::Mixer::speaker_position).
    pub fn speaker_position(&self, channel: u32) -> Option<[f32; 3]> {
        self.spatial.scene.speaker(channel)
    }

    /// [`Mixer::set_speaker_position`](crate::Mixer::set_speaker_position);
    /// [`ControllerError::NoSuchSpeaker`] past the output's channels.
    pub fn set_speaker_position(
        &mut self,
        channel: u32,
        position: [f32; 3],
    ) -> Result<(), ControllerError> {
        if self.spatial.scene.set_speaker(channel, position) {
            Ok(())
        } else {
            Err(ControllerError::NoSuchSpeaker)
        }
    }

    /// Moves a 3D voice and sets its velocity
    /// (`Soloud::set3dSourceParameters`). Like every 3D voice setter, it
    /// does nothing for a voice not played in 3D through this controller,
    /// or whose final event has been read.
    pub fn set_3d_source_parameters(
        &mut self,
        id: VoiceId,
        position: [f32; 3],
        velocity: [f32; 3],
    ) {
        self.with_3d_voice(id, |spatial| {
            spatial.position = position.into();
            spatial.velocity = velocity.into();
        });
    }

    /// Moves a 3D voice (`Soloud::set3dSourcePosition`).
    pub fn set_3d_source_position(&mut self, id: VoiceId, position: [f32; 3]) {
        self.with_3d_voice(id, |spatial| spatial.position = position.into());
    }

    /// Sets a 3D voice's velocity (`Soloud::set3dSourceVelocity`).
    pub fn set_3d_source_velocity(&mut self, id: VoiceId, velocity: [f32; 3]) {
        self.with_3d_voice(id, |spatial| spatial.velocity = velocity.into());
    }

    /// Sets a 3D voice's Doppler factor
    /// (`Soloud::set3dSourceDopplerFactor`).
    pub fn set_3d_source_doppler_factor(&mut self, id: VoiceId, factor: f32) {
        self.with_3d_voice(id, |spatial| spatial.doppler_factor = factor);
    }

    /// Sets a 3D voice's minimum and maximum distance
    /// (`Soloud::set3dSourceMinMaxDistance`).
    pub fn set_3d_source_min_max_distance(&mut self, id: VoiceId, min: f32, max: f32) {
        self.with_3d_voice(id, |spatial| {
            spatial.min_distance = min;
            spatial.max_distance = max;
        });
    }

    /// Sets a 3D voice's attenuation model and rolloff factor
    /// (`Soloud::set3dSourceAttenuation`).
    pub fn set_3d_source_attenuation(
        &mut self,
        id: VoiceId,
        attenuation: Attenuation,
        rolloff: f32,
    ) {
        self.with_3d_voice(id, |spatial| {
            spatial.attenuation = attenuation;
            spatial.rolloff = rolloff;
        });
    }

    fn with_3d_voice(&mut self, id: VoiceId, change: impl FnOnce(&mut VoiceSpatial)) {
        if let Some(spatial) = self.spatial.voice_mut(id) {
            change(spatial);
        }
    }
}
