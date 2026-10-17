// Ported from SoLoud src/core/soloud_core_3d.cpp and src/core/soloud.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::custom::SpatialVoice;
use super::doppler::doppler;
use super::settings::VoiceSpatial;
use super::vector::{Mat3, Vec3};
use crate::contracts::{MAX_CHANNELS, VoiceId};
use crate::features::playback::Handle;

const MAX: usize = MAX_CHANNELS as usize;

/// SoLoud's speed of sound: units per second, 343.3 for meters in air.
pub(crate) const SOUND_SPEED: f32 = 343.3;

/// The listener and the speakers: the mixer's 3D state outside its voices.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Scene {
    pub position: Vec3,
    /// The direction the listener faces; need not be normalized.
    pub at: Vec3,
    pub up: Vec3,
    pub velocity: Vec3,
    /// Units per second, for Doppler and distance delay.
    pub sound_speed: f32,
    /// `LEFT_HANDED_3D`: the listener basis from `mat3::lookatLH`.
    pub left_handed: bool,
    /// Speaker directions for the output channels (`m3dSpeakerPosition`);
    /// a zero vector hears every direction at full volume.
    pub speakers: [Vec3; MAX],
    channels: usize,
}

impl Scene {
    /// SoLoud's listener at the origin facing -z with +y up, still, and its
    /// speaker layout for `channels` outputs (`Soloud::postinit_internal`).
    pub fn new(channels: usize, left_handed: bool) -> Self {
        let v = |x: f32, z: f32| Vec3 { x, y: 0.0, z };
        let (front_l, front_r, center) = (v(2.0, 1.0), v(-2.0, 1.0), v(0.0, 1.0));
        let (back_l, back_r, zero) = (v(2.0, -1.0), v(-2.0, -1.0), Vec3::default());
        let mut speakers = [zero; MAX];
        let layout: &[Vec3] = match channels {
            1 => &[center],
            2 => &[front_l, front_r],
            4 => &[front_l, front_r, back_l, back_r],
            // The subwoofer is a zero vector, a mix of every direction.
            6 => &[front_l, front_r, center, zero, back_l, back_r],
            8 => &[
                front_l,
                front_r,
                center,
                zero,
                v(2.0, 0.0),
                v(-2.0, 0.0),
                back_l,
                back_r,
            ],
            _ => &[],
        };
        speakers[..layout.len()].copy_from_slice(layout);
        Self {
            position: Vec3::default(),
            at: v(0.0, -1.0),
            up: Vec3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
            velocity: Vec3::default(),
            sound_speed: SOUND_SPEED,
            left_handed,
            speakers,
            channels,
        }
    }

    /// Sets the speed of sound, if it is finite and greater than 0 (SoLoud
    /// took NaN and infinity until reference patch 0064).
    pub fn set_sound_speed(&mut self, speed: f32) -> bool {
        let valid = speed > 0.0 && speed <= f32::MAX;
        if valid {
            self.sound_speed = speed;
        }
        valid
    }

    /// The speaker of output channel `channel`, if the output has it.
    pub fn speaker(&self, channel: u32) -> Option<[f32; 3]> {
        let channel = channel as usize;
        (channel < self.channels).then(|| self.speakers[channel].into())
    }

    /// Moves the speaker of output channel `channel`, if the output has it.
    pub fn set_speaker(&mut self, channel: u32, position: [f32; 3]) -> bool {
        let channel = channel as usize;
        let exists = channel < self.channels;
        if exists {
            self.speakers[channel] = position.into();
        }
        exists
    }

    /// What a 3D update needs for every voice, computed once per update.
    pub fn frame(&self) -> Frame {
        let mut speakers = [Vec3::default(); MAX];
        for (out, speaker) in speakers.iter_mut().zip(&self.speakers[..self.channels]) {
            *out = speaker.normalize();
        }
        let basis = if self.left_handed {
            Mat3::look_at_lh(self.at, self.up)
        } else {
            Mat3::look_at_rh(self.at, self.up)
        };
        Frame {
            speakers,
            channels: self.channels,
            listener: self.position,
            listener_velocity: self.velocity,
            sound_speed: self.sound_speed,
            basis,
        }
    }
}

/// The normalized speakers and the listener's basis for one update.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    speakers: [Vec3; MAX],
    channels: usize,
    listener: Vec3,
    listener_velocity: Vec3,
    sound_speed: f32,
    basis: Mat3,
}

/// A voice's attenuation, per-speaker gains and Doppler factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Gains {
    pub volume: f32,
    pub speakers: [f32; MAX],
    pub doppler: f32,
}

impl Frame {
    /// The gains of the voice `handle` or `voice`, whichever the caller has
    /// (the loop body of `update3dVoices_internal`). The volume is the collider's factor times
    /// the attenuator's or the attenuation model's. A speaker hears
    /// `(cos θ + 1) / 2` of the voice, θ being the angle between it and the
    /// voice in the listener's frame, times the volume; speakers past the
    /// output's channels hear nothing.
    pub fn gains(
        &self,
        spatial: &VoiceSpatial,
        listener_relative: bool,
        handle: Handle,
        voice: Option<VoiceId>,
    ) -> Gains {
        let mut volume = 1.0_f32;
        if let Some((collider, user_data)) = &spatial.collider {
            let voice = SpatialVoice {
                handle,
                voice,
                position: spatial.position.into(),
                velocity: spatial.velocity.into(),
                listener_relative,
                min_distance: spatial.min_distance,
                max_distance: spatial.max_distance,
                rolloff: spatial.rolloff,
                doppler_factor: spatial.doppler_factor,
            };
            volume *= collider.collide(&voice, *user_data);
        }
        let mut pos = spatial.position;
        if !listener_relative {
            pos = pos.sub(self.listener);
        }
        let distance = pos.mag();
        let (min, max, rolloff) = (spatial.min_distance, spatial.max_distance, spatial.rolloff);
        volume *= match &spatial.attenuator {
            Some(attenuator) => attenuator.attenuate(distance, min, max, rolloff),
            None => spatial.attenuation.attenuate(distance, min, max, rolloff),
        };
        let doppler = doppler(
            pos,
            spatial.velocity,
            self.listener_velocity,
            spatial.doppler_factor,
            self.sound_speed,
        );
        let pos = self.basis.mul(pos).normalize();
        let mut speakers = [0.0; MAX];
        for (gain, speaker) in speakers.iter_mut().zip(&self.speakers[..self.channels]) {
            let heard = if speaker.is_null() {
                1.0
            } else {
                (speaker.dot(pos) + 1.0) / 2.0
            };
            *gain = volume * heard;
        }
        Gains {
            volume,
            speakers,
            doppler,
        }
    }
}
