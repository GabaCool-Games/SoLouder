// Ported from SoLoud src/core/soloud_core_3d.cpp and src/core/soloud.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::settings::VoiceSpatial;
use super::vector::{Mat3, Vec3};
use crate::contracts::MAX_CHANNELS;

const MAX: usize = MAX_CHANNELS as usize;

/// The listener and the speakers: the mixer's 3D state outside its voices.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Scene {
    pub position: Vec3,
    /// The direction the listener faces; need not be normalized.
    pub at: Vec3,
    pub up: Vec3,
    /// Speaker directions for the output channels (`m3dSpeakerPosition`);
    /// a zero vector hears every direction at full volume.
    pub speakers: [Vec3; MAX],
    channels: usize,
}

impl Scene {
    /// SoLoud's listener at the origin facing -z with +y up, and its speaker
    /// layout for `channels` outputs (`Soloud::postinit_internal`).
    pub fn new(channels: usize) -> Self {
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
            speakers,
            channels,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// What a 3D update needs for every voice, computed once per update.
    pub fn frame(&self) -> Frame {
        let mut speakers = [Vec3::default(); MAX];
        for (out, speaker) in speakers.iter_mut().zip(&self.speakers[..self.channels]) {
            *out = speaker.normalize();
        }
        Frame {
            speakers,
            channels: self.channels,
            listener: self.position,
            basis: Mat3::look_at_rh(self.at, self.up),
        }
    }
}

/// The normalized speakers and the listener's basis for one update.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    speakers: [Vec3; MAX],
    channels: usize,
    listener: Vec3,
    basis: Mat3,
}

/// A voice's attenuation and per-speaker gains.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Gains {
    pub volume: f32,
    pub speakers: [f32; MAX],
}

impl Frame {
    /// The gains of one voice (the loop body of `update3dVoices_internal`).
    /// A speaker hears `(cos θ + 1) / 2` of the voice, θ being the angle
    /// between it and the voice in the listener's frame, times the
    /// attenuation; speakers past the output's channels hear nothing.
    pub fn gains(&self, spatial: &VoiceSpatial, listener_relative: bool) -> Gains {
        let mut volume = 1.0_f32;
        let mut pos = spatial.position;
        if !listener_relative {
            pos = pos.sub(self.listener);
        }
        let distance = pos.mag();
        volume *= spatial.attenuation.attenuate(
            distance,
            spatial.min_distance,
            spatial.max_distance,
            spatial.rolloff,
        );
        // TODO(port): Doppler (milestone E unit 7) is computed here from
        // `pos` and the velocities.
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
        Gains { volume, speakers }
    }
}
