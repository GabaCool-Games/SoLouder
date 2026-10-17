//! Spatial audio: an [`AudioListener`] and [`SpatialEmitter`]s placed by
//! their `GlobalTransform`s.
//!
//! Once per frame, in [`SolouderSystems::Sync`](crate::SolouderSystems)
//! after transform propagation, one snapshot of the listener and every
//! emitter is taken, with their velocities. The playback sync then starts
//! new emitters' sounds in 3D at their snapshot position, sends the
//! listener and the playing emitters' positions to the controller, and
//! asks it for one 3D update, which reaches the mixer as one batched
//! command. The 3D math (attenuation, speaker gains, Doppler) runs in that
//! update on the main thread.
//!
//! Axes: Bevy is right-handed with +Y up and -Z forward, as SoLoud's
//! listener defaults are (looking at -Z, up +Y, `left_handed_3d` off), so
//! positions and directions pass unchanged. A sound to the listener's right
//! (its +X) is heard on the right speaker. Units are world units taken as
//! metres; velocities are metres per second.

mod components;
mod snapshot;

use bevy::prelude::*;

pub use components::{
    AudioListener, AudioTeleport, AudioVelocity, ListenerState, SpatialAudioSettings,
    SpatialEmitter,
};
pub(crate) use snapshot::{EmitterPose, SpatialSnapshot, take_snapshot};

use crate::SolouderSystems;

pub(crate) fn build(app: &mut App) {
    app.init_resource::<SpatialAudioSettings>()
        .init_resource::<ListenerState>()
        .init_resource::<SpatialSnapshot>()
        .add_systems(PostUpdate, take_snapshot.in_set(SolouderSystems::Sync));
}
