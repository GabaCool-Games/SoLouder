// Ported from SoLoud src/core/soloud_audiosource.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::attenuation::Attenuation;
use super::vector::Vec3;

/// A source's 3D defaults, copied into each voice it starts (`AudioSource`'s
/// `m3d*` members).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpatialSettings {
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: f32,
    pub attenuation: Attenuation,
    /// Positions are relative to the listener (`LISTENER_RELATIVE`).
    pub listener_relative: bool,
}

impl Default for SpatialSettings {
    /// `AudioSource::AudioSource`: distances 1 to 1,000,000, rolloff 1, no
    /// attenuation, world coordinates.
    fn default() -> Self {
        Self {
            min_distance: 1.0,
            max_distance: 1_000_000.0,
            rolloff: 1.0,
            attenuation: Attenuation::None,
            listener_relative: false,
        }
    }
}

/// A voice's 3D state (`AudioSourceInstance3dData`). Every voice has one; only
/// voices started with [`Mixer::play_3d`](crate::Mixer::play_3d) use it,
/// except `volume`, which every voice's overall volume includes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VoiceSpatial {
    pub position: Vec3,
    pub min_distance: f32,
    pub max_distance: f32,
    pub rolloff: f32,
    pub attenuation: Attenuation,
    /// The attenuation computed by the last 3D update (`m3dVolume`); 1 until
    /// then.
    pub volume: f32,
}

impl VoiceSpatial {
    /// `AudioSourceInstance3dData::init`. The listener-relative flag is a
    /// voice flag, set from the source when the voice is made, so a slot
    /// never keeps a previous voice's (reference patch 0013).
    pub fn new(settings: &SpatialSettings) -> Self {
        Self {
            position: Vec3::default(),
            min_distance: settings.min_distance,
            max_distance: settings.max_distance,
            rolloff: settings.rolloff,
            attenuation: settings.attenuation,
            volume: 1.0,
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
    };
}

pub(crate) use spatial_settings_api;
