// Ported from SoLoud src/core/soloud_audiosource.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::sync::Arc;

use super::attenuation::Attenuation;
use super::custom::{Attenuator, Collider};
use super::vector::Vec3;
use crate::contracts::MAX_CHANNELS;

/// A source's 3D defaults, copied into each voice it starts (`AudioSource`'s
/// `m3d*` members).
#[derive(Clone, Debug)]
pub(crate) struct SpatialSettings {
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: f32,
    pub attenuation: Attenuation,
    pub doppler_factor: f32,
    /// Positions are relative to the listener (`LISTENER_RELATIVE`).
    pub listener_relative: bool,
    /// 3D plays start late by the time sound takes to reach the listener
    /// (`DISTANCE_DELAY`).
    pub distance_delay: bool,
    /// With the user data passed to it.
    pub collider: Option<(Arc<dyn Collider>, i32)>,
    pub attenuator: Option<Arc<dyn Attenuator>>,
}

impl Default for SpatialSettings {
    /// `AudioSource::AudioSource`: distances 1 to 1,000,000, rolloff 1, no
    /// attenuation, Doppler factor 1, world coordinates, no distance delay,
    /// no collider or attenuator.
    fn default() -> Self {
        Self {
            min_distance: 1.0,
            max_distance: 1_000_000.0,
            rolloff: 1.0,
            attenuation: Attenuation::None,
            doppler_factor: 1.0,
            listener_relative: false,
            distance_delay: false,
            collider: None,
            attenuator: None,
        }
    }
}

/// A voice's 3D state (`AudioSourceInstance3dData`). Every voice has one; only
/// voices started with [`Mixer::play_3d`](crate::Mixer::play_3d) use it,
/// except `volume`, which every voice's overall volume includes.
#[derive(Clone, Debug)]
pub(crate) struct VoiceSpatial {
    pub position: Vec3,
    pub velocity: Vec3,
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: f32,
    pub attenuation: Attenuation,
    pub doppler_factor: f32,
    /// Copied from the source, which `play_3d` asks.
    pub distance_delay: bool,
    pub collider: Option<(Arc<dyn Collider>, i32)>,
    pub attenuator: Option<Arc<dyn Attenuator>>,
    /// The attenuation computed by the last 3D update (`m3dVolume`); 1 until
    /// then.
    pub volume: f32,
    /// The play speed factor from the last 3D update (`mDopplerValue`); 1
    /// until then.
    pub doppler: f32,
    /// The speaker gains from the last 3D update (`mChannelVolume`).
    pub channel_volume: [f32; MAX_CHANNELS as usize],
}

impl VoiceSpatial {
    /// `AudioSourceInstance3dData::init`. The listener-relative flag is a
    /// voice flag, set from the source when the voice is made, so a slot
    /// never keeps a previous voice's (reference patch 0013).
    pub fn new(settings: &SpatialSettings) -> Self {
        Self {
            position: Vec3::default(),
            velocity: Vec3::default(),
            min_distance: settings.min_distance,
            max_distance: settings.max_distance,
            rolloff: settings.rolloff,
            attenuation: settings.attenuation,
            doppler_factor: settings.doppler_factor,
            distance_delay: settings.distance_delay,
            collider: settings.collider.clone(),
            attenuator: settings.attenuator.clone(),
            volume: 1.0,
            doppler: 1.0,
            channel_volume: [0.0; MAX_CHANNELS as usize],
        }
    }
}

/// The `AudioSource` 3D setters and getters, on a type with a
/// `settings: SourceSettings` field. Changes apply to voices started
/// afterwards.
macro_rules! spatial_settings_api {
    () => {
        /// The attenuation model and rolloff factor for 3D voices; no
        /// attenuation and 1 by default.
        pub fn attenuation_3d(&self) -> ($crate::features::spatial::Attenuation, f32) {
            let spatial = &self.settings.spatial;
            (spatial.attenuation, spatial.rolloff)
        }

        /// Sets how 3D voices started after this call fall off with
        /// distance (`AudioSource::set3dAttenuation`).
        pub fn set_3d_attenuation(
            &mut self,
            attenuation: $crate::features::spatial::Attenuation,
            rolloff: f32,
        ) {
            self.settings.spatial.attenuation = attenuation;
            self.settings.spatial.rolloff = rolloff;
        }

        /// The distances between which 3D attenuation applies; 1 and
        /// 1,000,000 by default.
        pub fn min_max_distance_3d(&self) -> (f32, f32) {
            let spatial = &self.settings.spatial;
            (spatial.min_distance, spatial.max_distance)
        }

        /// Sets the minimum distance (full volume at or below it) and the
        /// maximum distance (no further attenuation past it) for 3D voices
        /// started after this call (`AudioSource::set3dMinMaxDistance`).
        pub fn set_3d_min_max_distance(&mut self, min: f32, max: f32) {
            self.settings.spatial.min_distance = min;
            self.settings.spatial.max_distance = max;
        }

        /// Whether 3D voices take their positions relative to the listener.
        pub fn is_3d_listener_relative(&self) -> bool {
            self.settings.spatial.listener_relative
        }

        /// Sets whether 3D voices started after this call take their
        /// positions relative to the listener, so they move with it
        /// (`AudioSource::set3dListenerRelative`).
        pub fn set_3d_listener_relative(&mut self, relative: bool) {
            self.settings.spatial.listener_relative = relative;
        }

        /// How strongly 3D voices' pitch follows their speed towards or away
        /// from the listener; 1 by default, 0 for none.
        pub fn doppler_factor_3d(&self) -> f32 {
            self.settings.spatial.doppler_factor
        }

        /// Sets the Doppler factor of 3D voices started after this call
        /// (`AudioSource::set3dDopplerFactor`).
        pub fn set_3d_doppler_factor(&mut self, factor: f32) {
            self.settings.spatial.doppler_factor = factor;
        }

        /// Whether 3D plays start late by their distance from the listener.
        pub fn is_3d_distance_delay(&self) -> bool {
            self.settings.spatial.distance_delay
        }

        /// Sets whether 3D plays after this call start late by the time
        /// sound takes to reach the listener at the mixer's speed of sound
        /// (`AudioSource::set3dDistanceDelay`); off by default. Only the
        /// start is delayed: moving the voice later changes nothing.
        pub fn set_3d_distance_delay(&mut self, delay: bool) {
            self.settings.spatial.distance_delay = delay;
        }

        /// The collider of 3D voices and its user data, if any.
        pub fn collider_3d(
            &self,
        ) -> Option<(
            &std::sync::Arc<dyn $crate::features::spatial::Collider>,
            i32,
        )> {
            let (collider, user_data) = self.settings.spatial.collider.as_ref()?;
            Some((collider, *user_data))
        }

        /// Sets the collider of 3D voices started after this call, with the
        /// `user_data` it is passed (`AudioSource::set3dCollider`); `None`
        /// removes it.
        pub fn set_3d_collider(
            &mut self,
            collider: Option<std::sync::Arc<dyn $crate::features::spatial::Collider>>,
            user_data: i32,
        ) {
            self.settings.spatial.collider = collider.map(|collider| (collider, user_data));
        }

        /// The attenuator of 3D voices, if any.
        pub fn attenuator_3d(
            &self,
        ) -> Option<&std::sync::Arc<dyn $crate::features::spatial::Attenuator>> {
            self.settings.spatial.attenuator.as_ref()
        }

        /// Sets the attenuator of 3D voices started after this call, which
        /// replaces their attenuation model (`AudioSource::set3dAttenuator`);
        /// `None` removes it.
        pub fn set_3d_attenuator(
            &mut self,
            attenuator: Option<std::sync::Arc<dyn $crate::features::spatial::Attenuator>>,
        ) {
            self.settings.spatial.attenuator = attenuator;
        }
    };
}

pub(crate) use spatial_settings_api;
