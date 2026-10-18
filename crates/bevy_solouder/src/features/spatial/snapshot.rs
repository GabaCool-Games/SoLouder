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
    /// Emitters with a usable position this frame.
    pub emitters: HashMap<Entity, EmitterPose>,
    pub sound_speed: f32,
    /// Each entity's motion so far, to derive velocities from.
    tracks: HashMap<Entity, Track>,
    /// Reused for the next frame's tracks.
    spare: HashMap<Entity, Track>,
}

/// Where an entity was when its position last changed, the time since, and
/// the velocity derived then.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Track {
    position: Vec3,
    since: f32,
    velocity: Vec3,
}

impl Track {
    fn at_rest(position: Vec3) -> Self {
        Self {
            position,
            since: 0.0,
            velocity: Vec3::ZERO,
        }
    }
}

/// How a velocity is found for one entity this frame.
#[derive(Clone, Copy, Debug)]
struct Motion {
    dt: f32,
    max_speed: f32,
    hold: f32,
}

impl Motion {
    /// The entity's track after this frame, whose `velocity` is the one to
    /// use: the explicit velocity if any; otherwise, when the entity moved,
    /// the distance over the time since its position last changed (one
    /// frame after a rest longer than `hold`), so motion stepped less often
    /// than frames (a fixed physics step) keeps a steady speed; when it did
    /// not move, the last derived velocity, until it has been still for
    /// `hold`. 0 for a first sighting, a zero or paused time step, a
    /// teleport, or a speed past the plausible maximum.
    fn step(
        &self,
        track: Option<Track>,
        position: Vec3,
        explicit: Option<&AudioVelocity>,
        teleport: bool,
    ) -> Track {
        if let Some(AudioVelocity(velocity)) = explicit {
            let velocity = if velocity.is_finite() {
                *velocity
            } else {
                Vec3::ZERO
            };
            return Track {
                velocity,
                ..Track::at_rest(position)
            };
        }
        let Some(track) = track.filter(|_| !teleport && self.dt > 0.0) else {
            return Track::at_rest(position);
        };
        let since = track.since + self.dt;
        if position == track.position {
            let velocity = if since < self.hold {
                track.velocity
            } else {
                Vec3::ZERO
            };
            return Track {
                since,
                velocity,
                ..track
            };
        }
        let span = if since <= self.hold { since } else { self.dt };
        let velocity = (position - track.position) / span;
        let velocity = if velocity.is_finite() && velocity.length() <= self.max_speed {
            velocity
        } else {
            Vec3::ZERO
        };
        Track {
            velocity,
            ..Track::at_rest(position)
        }
    }
}

/// The listener's facing (-Z) and up (+Y) directions, from the transform's
/// axes; `None` for a degenerate transform (a zero or non-finite scale or
/// rotation), whose directions `GlobalTransform::forward` would panic on.
fn orientation(transform: &GlobalTransform) -> Option<(Vec3, Vec3)> {
    let axes = transform.affine().matrix3;
    let at = -Vec3::from(axes.z_axis).normalize_or_zero();
    let up = Vec3::from(axes.y_axis).normalize_or_zero();
    (at != Vec3::ZERO && up != Vec3::ZERO).then_some((at, up))
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
        hold: settings.velocity_hold,
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
    let listener = &mut snapshot.listener;
    listener.velocity = Vec3::ZERO;
    if let Some((entity, transform, explicit, teleport)) = chosen {
        let position = transform.translation();
        // A degenerate transform keeps the last pose, at rest.
        if position.is_finite()
            && let Some((at, up)) = orientation(transform)
        {
            let previous = snapshot.tracks.get(&entity).copied();
            let track = motion.step(previous, position, explicit, teleport);
            next.insert(entity, track);
            listener.position = position;
            listener.at = at;
            listener.up = up;
            listener.velocity = track.velocity;
        }
    }

    snapshot.emitters.clear();
    for (entity, transform, emitter, explicit, teleport) in &emitters {
        let position = transform.translation();
        if !position.is_finite() {
            continue;
        }
        let previous = snapshot.tracks.get(&entity).copied();
        let track = motion.step(previous, position, explicit, teleport);
        next.insert(entity, track);
        let pose = EmitterPose {
            position,
            velocity: track.velocity,
            settings: *emitter,
        };
        snapshot.emitters.insert(entity, pose);
    }
    snapshot.spare = std::mem::replace(&mut snapshot.tracks, next);

    for entity in &teleported {
        commands.entity(entity).try_remove::<AudioTeleport>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOTION: Motion = Motion {
        dt: 1.0 / 144.0,
        max_speed: 200.0,
        hold: 0.1,
    };

    /// The velocities derived for positions seen once per frame.
    fn derive(motion: Motion, positions: impl IntoIterator<Item = Vec3>) -> Vec<Vec3> {
        let mut track = None;
        positions
            .into_iter()
            .map(|position| {
                let next = motion.step(track, position, None, false);
                track = Some(next);
                next.velocity
            })
            .collect()
    }

    /// Moved every third frame at 10 m/s: the derived velocity stays 10 m/s
    /// between the moves, rather than alternating 0 and 30.
    #[test]
    fn motion_stepped_less_often_than_frames_keeps_a_steady_speed() {
        let step = 10.0 * 3.0 * MOTION.dt;
        let positions = (0..30).map(|frame| Vec3::new(0.0, 0.0, (frame / 3) as f32 * step));
        let velocities = derive(MOTION, positions);
        for velocity in &velocities[3..] {
            assert!((velocity.z - 10.0).abs() < 1e-3, "{velocities:?}");
        }
    }

    /// A fixed 64 Hz step seen at 144 frames per second moves on two or
    /// three frames out of two or three: the speed stays within the ratio
    /// of those gaps to the step, never 0 or 2.25 times.
    #[test]
    fn a_fixed_physics_step_does_not_flutter() {
        let positions = (0..300).map(|frame| {
            let steps = (frame as f32 * MOTION.dt * 64.0).floor();
            Vec3::new(steps * 10.0 / 64.0, 0.0, 0.0)
        });
        for velocity in &derive(MOTION, positions)[10..] {
            assert!((6.5..=12.5).contains(&velocity.x), "{velocity}");
        }
    }

    /// A stopped entity reads 0 once it has been still for `hold`, and its
    /// next move is measured over one frame, not over the rest.
    #[test]
    fn a_stop_reads_zero_after_the_hold() {
        let dt = MOTION.dt;
        let mut positions: Vec<Vec3> = (0..5).map(|i| Vec3::X * i as f32 * dt).collect();
        positions.extend([Vec3::X * 4.0 * dt; 30]);
        positions.push(Vec3::X * 5.0 * dt);
        let velocities = derive(MOTION, positions);
        assert!((velocities[4].x - 1.0).abs() < 1e-3);
        let still = (MOTION.hold / dt) as usize;
        assert!((velocities[4 + still - 1].x - 1.0).abs() < 1e-3);
        assert_eq!(velocities[4 + still + 1], Vec3::ZERO);
        assert!((velocities.last().unwrap().x - 1.0).abs() < 1e-3);
    }

    /// Zero, non-finite and NaN transforms have no orientation.
    #[test]
    fn degenerate_transforms_have_no_orientation() {
        let zero = GlobalTransform::from(Transform::from_scale(Vec3::ZERO));
        assert_eq!(orientation(&zero), None);
        let nan = GlobalTransform::from(Transform::from_scale(Vec3::NAN));
        assert_eq!(orientation(&nan), None);
        let flat = GlobalTransform::from(Transform::from_scale(Vec3::new(1.0, 0.0, 1.0)));
        assert_eq!(orientation(&flat), None);
        let scaled = GlobalTransform::from(Transform::from_scale(Vec3::splat(3.0)));
        assert_eq!(orientation(&scaled), Some((Vec3::NEG_Z, Vec3::Y)));
    }
}
