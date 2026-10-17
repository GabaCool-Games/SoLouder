//! The per-frame snapshot of the listener and emitters, taken from
//! `GlobalTransform` after transform propagation.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::components::{
    AudioListener, AudioTeleport, AudioVelocity, ListenerState, SpatialAudioSettings,
    SpatialEmitter,
};

/// The listener's pose: SoLoud's default until a listener is seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ListenerPose {
    pub position: Vec3,
    pub at: Vec3,
    pub up: Vec3,
    pub velocity: Vec3,
}

impl Default for ListenerPose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            at: Vec3::NEG_Z,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EmitterPose {
    pub position: Vec3,
    pub velocity: Vec3,
    pub settings: SpatialEmitter,
}

/// One coherent view of the scene for the frame's sync.
#[derive(Resource, Default, Debug)]
pub(crate) struct SpatialSnapshot {
    pub listener: ListenerPose,
    pub emitters: HashMap<Entity, EmitterPose>,
    pub sound_speed: f32,
    /// Last frame's positions, to derive velocities from.
    tracks: HashMap<Entity, Vec3>,
    /// Reused for the next frame's positions.
    spare: HashMap<Entity, Vec3>,
}

/// How a velocity is found for one entity this frame.
struct Motion {
    dt: f32,
    max_speed: f32,
}

impl Motion {
    /// The explicit velocity if any; otherwise the distance moved since
    /// last frame over the time step, or 0 for a first sighting, a zero or
    /// paused time step, a teleport, or a speed past the plausible maximum.
    fn velocity(
        &self,
        previous: Option<Vec3>,
        position: Vec3,
        explicit: Option<&AudioVelocity>,
        teleport: bool,
    ) -> Vec3 {
        if let Some(AudioVelocity(velocity)) = explicit {
            return if velocity.is_finite() {
                *velocity
            } else {
                Vec3::ZERO
            };
        }
        let Some(previous) = previous.filter(|_| !teleport && self.dt > 0.0) else {
            return Vec3::ZERO;
        };
        let velocity = (position - previous) / self.dt;
        if velocity.is_finite() && velocity.length() <= self.max_speed {
            velocity
        } else {
            Vec3::ZERO
        }
    }
}

type Listener<'a> = (
    Entity,
    &'a GlobalTransform,
    Option<&'a AudioVelocity>,
    Has<AudioTeleport>,
);
type Emitter<'a> = (
    Entity,
    &'a GlobalTransform,
    &'a SpatialEmitter,
    Option<&'a AudioVelocity>,
    Has<AudioTeleport>,
);

/// Takes the frame's snapshot, chooses the listener, and clears teleport
/// markers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn take_snapshot(
    time: Option<Res<Time>>,
    settings: Res<SpatialAudioSettings>,
    listeners: Query<Listener, With<AudioListener>>,
    emitters: Query<Emitter>,
    teleported: Query<Entity, With<AudioTeleport>>,
    mut snapshot: ResMut<SpatialSnapshot>,
    mut state: ResMut<ListenerState>,
    mut commands: Commands,
) {
    let snapshot = &mut *snapshot;
    let motion = Motion {
        dt: time.map_or(0.0, |time| time.delta_secs()),
        max_speed: settings.max_derived_speed,
    };
    let mut next = std::mem::take(&mut snapshot.spare);
    next.clear();
    snapshot.sound_speed = settings.sound_speed;

    let count = listeners.iter().count();
    // By index: `Entity`'s own order is not its index's.
    let chosen = listeners
        .iter()
        .min_by_key(|(entity, ..)| entity.index_u32());
    let new_state = match chosen {
        None => ListenerState::None,
        Some((entity, ..)) if count == 1 => ListenerState::One(entity),
        Some((used, ..)) => ListenerState::Several { used, count },
    };
    state.set_if_neq(new_state);
    match chosen {
        Some((entity, transform, explicit, teleport)) => {
            let position = transform.translation();
            let previous = snapshot.tracks.get(&entity).copied();
            next.insert(entity, position);
            let (at, up) = (Vec3::from(transform.forward()), Vec3::from(transform.up()));
            let listener = &mut snapshot.listener;
            // A degenerate transform (zero scale) keeps the last pose.
            if position.is_finite() && at.is_finite() && up.is_finite() {
                listener.position = position;
                listener.at = at;
                listener.up = up;
            }
            listener.velocity = motion.velocity(previous, position, explicit, teleport);
        }
        None => snapshot.listener.velocity = Vec3::ZERO,
    }

    snapshot.emitters.clear();
    for (entity, transform, emitter, explicit, teleport) in &emitters {
        let position = transform.translation();
        if !position.is_finite() {
            continue;
        }
        let previous = snapshot.tracks.get(&entity).copied();
        next.insert(entity, position);
        let pose = EmitterPose {
            position,
            velocity: motion.velocity(previous, position, explicit, teleport),
            settings: *emitter,
        };
        snapshot.emitters.insert(entity, pose);
    }
    snapshot.spare = std::mem::replace(&mut snapshot.tracks, next);

    for entity in &teleported {
        commands.entity(entity).try_remove::<AudioTeleport>();
    }
}
