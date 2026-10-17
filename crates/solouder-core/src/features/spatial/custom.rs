// Ported from SoLoud include/soloud_audiosource.h (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::fmt;

use crate::features::playback::Handle;

/// Scales a 3D voice's volume by what stands between it and the listener
/// (`AudioCollider`), for example walls or rooms. Set one on a source with
/// `set_3d_collider`; it runs for each of the source's 3D voices whenever
/// their gains are computed ([`Mixer::play_3d`](crate::Mixer::play_3d) and
/// [`Mixer::update_3d_audio`](crate::Mixer::update_3d_audio)), on the thread
/// that calls them, never on the audio thread.
pub trait Collider: Send + Sync + fmt::Debug {
    /// The volume factor for `voice`, normally 0 to 1
    /// (`AudioCollider::collide`); `user_data` is the value given with the
    /// collider. It multiplies the distance attenuation.
    fn collide(&self, voice: &SpatialVoice, user_data: i32) -> f32;
}

/// Replaces the [`Attenuation`](super::Attenuation) model of the sources it
/// is set on (`AudioAttenuator`). Runs where a [`Collider`] does.
pub trait Attenuator: Send + Sync + fmt::Debug {
    /// The volume factor at `distance` from the listener, given the voice's
    /// minimum and maximum distance and rolloff factor
    /// (`AudioAttenuator::attenuate`). Nothing clamps `distance` first.
    fn attenuate(&self, distance: f32, min_distance: f32, max_distance: f32, rolloff: f32) -> f32;
}

/// What a [`Collider`] sees of a 3D voice (`AudioSourceInstance3dData`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SpatialVoice {
    /// The voice's handle.
    pub handle: Handle,
    /// Its position: in world coordinates, or relative to the listener when
    /// `listener_relative`.
    pub position: [f32; 3],
    /// Its velocity in units per second.
    pub velocity: [f32; 3],
    /// Whether `position` is relative to the listener.
    pub listener_relative: bool,
    /// The distance up to which it is not attenuated.
    pub min_distance: f32,
    /// The distance past which it is attenuated no further.
    pub max_distance: f32,
    /// Its attenuation rolloff factor.
    pub rolloff: f32,
    /// Its Doppler factor.
    pub doppler_factor: f32,
}
