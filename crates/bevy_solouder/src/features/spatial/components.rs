//! Spatial audio components and resources.

use bevy::prelude::*;
use solouder_core::Attenuation;

/// Hears 3D sounds from the entity's [`GlobalTransform`]: its translation is
/// the listener's position, its forward (-Z) and up (+Y) axes the
/// listener's orientation. Scale is ignored.
///
/// Exactly one listener is used. With several, the one with the lowest
/// entity index ([`Entity::index_u32`]) is used, so the choice is the same
/// every frame, and [`ListenerState::Several`] says so; mixing several
/// listeners (split screen) is not supported. With none, the last listener's
/// position and orientation are kept, at rest; before any listener, it is
/// at the origin facing -Z with +Y up, SoLoud's default, which matches
/// Bevy's axes.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(Transform)]
pub struct AudioListener;

/// Plays the entity's [`SoundPlayer`](crate::SoundPlayer) in 3D at its
/// [`GlobalTransform`]'s translation, moving with it, hierarchy included.
///
/// Read when the sound starts: an emitter added afterwards does not make a
/// playing sound 3D, and a sound whose emitter's transform is not finite
/// waits to start until it is. Its fields other than `distance_delay` may
/// change while the sound plays. Removing it leaves the sound where it was.
/// [`PlaybackSettings::pan`](crate::PlaybackSettings::pan) is ignored for
/// 3D sounds: the speakers' gains come from the position.
///
/// Distances are in world units, taken as metres, so speeds are metres per
/// second and the speed of sound is
/// [`SpatialAudioSettings::sound_speed`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[require(Transform)]
pub struct SpatialEmitter {
    /// How the volume falls with distance. Inverse distance by default
    /// (SoLoud's own default is no attenuation).
    pub attenuation: Attenuation,
    /// How fast the attenuation falls; 1 by default.
    pub rolloff: f32,
    /// The distance up to which the sound is not attenuated; 1 by default.
    pub min_distance: f32,
    /// The distance past which it is attenuated no further; 1,000,000 by
    /// default.
    pub max_distance: f32,
    /// Scales the Doppler effect; 1 by default, 0 turns it off.
    pub doppler_factor: f32,
    /// Start late by the distance from the listener over the speed of
    /// sound; off by default. Read only when the sound starts.
    pub distance_delay: bool,
}

impl Default for SpatialEmitter {
    fn default() -> Self {
        Self {
            attenuation: Attenuation::InverseDistance,
            rolloff: 1.0,
            min_distance: 1.0,
            max_distance: 1_000_000.0,
            doppler_factor: 1.0,
            distance_delay: false,
        }
    }
}

/// The velocity of a listener or emitter in metres per second, for the
/// Doppler effect. Without it the velocity is derived from the entity's
/// `GlobalTransform`: how far it moved when its position last changed, over
/// the virtual time since the change before (see
/// [`SpatialAudioSettings::velocity_hold`],
/// [`SpatialAudioSettings::max_derived_speed`] and [`AudioTeleport`]).
///
/// Derived velocities follow what each frame sees. An entity moved by a
/// fixed physics step that does not divide the frame rate (64 Hz at 144
/// frames per second moves on every second or third frame) reads a speed
/// that varies with those gaps, between about 0.75 and 1.13 times
/// its own; a game with a physics engine should insert its body's velocity
/// here every frame instead.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioVelocity(pub Vec3);

/// Marks a listener or emitter that jumped this frame rather than moved:
/// its derived velocity is 0 for the frame. The plugin removes the marker
/// once it has been read.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct AudioTeleport;

/// Engine-wide spatial settings.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SpatialAudioSettings {
    /// The speed of sound in metres per second: 343.3 by default, SoLoud's
    /// default (dry air at about 20 °C). Must be finite and greater than 0;
    /// other values are ignored.
    pub sound_speed: f32,
    /// The fastest a listener or emitter without [`AudioVelocity`] is taken
    /// to move, in metres per second: a faster derived speed is treated as
    /// a teleport, and the velocity is 0 for that frame. 200 by default.
    pub max_derived_speed: f32,
    /// How long, in seconds, a listener or emitter without
    /// [`AudioVelocity`] keeps its last derived velocity while its position
    /// does not change, so motion stepped less often than frames reads a
    /// steady speed; still for longer, it reads 0. 0.1 by default, which
    /// covers motion stepped at 10 Hz or more.
    pub velocity_hold: f32,
}

impl Default for SpatialAudioSettings {
    fn default() -> Self {
        Self {
            sound_speed: 343.3,
            max_derived_speed: 200.0,
            velocity_hold: 0.1,
        }
    }
}

/// Which [`AudioListener`] the last frame used.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListenerState {
    /// No listener: the last one's position and orientation are kept.
    #[default]
    None,
    /// Exactly one listener.
    One(Entity),
    /// Several listeners; only `used`, the lowest entity index, is heard.
    Several {
        /// The listener used.
        used: Entity,
        /// How many there are.
        count: usize,
    },
}
