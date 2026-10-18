//! Spatial audio through the plugin, headless: Bevy's axes and handedness
//! against SoLoud's, emitters in a hierarchy, despawned emitters and
//! listeners, the listener policy, and the Doppler direction of derived,
//! explicit, teleporting and paused motion.

mod common;

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_solouder::{
    AudioListener, AudioTeleport, AudioVelocity, FinishReason, ListenerState, SpatialAudioSettings,
};
use common::finished;
use common::spatial::{
    app, centred, crossings, emitter, frame, left, moving, nothing, place, right,
};

/// Bevy's listener looks at -Z with +Y up, as SoLoud's does by default; a
/// sound on its right (+X) is on the right speaker. Turned to face +X, its
/// right is +Z: the mapping is right-handed.
#[test]
fn axes_and_handedness_follow_bevy() {
    let mut app = app();
    let e = emitter(&mut app, Vec3::new(3.0, 0.0, 0.0));
    // No listener yet: SoLoud's default, which is Bevy's.
    assert!(right(&mut app));
    let listener = app.world_mut().spawn(AudioListener).id();
    assert!(right(&mut app));
    place(&mut app, e, Transform::from_xyz(-3.0, 0.0, 0.0));
    assert!(left(&mut app));
    place(&mut app, e, Transform::from_xyz(0.0, 0.0, -3.0));
    assert!(centred(&mut app));
    let facing_x = Transform::default().looking_to(Vec3::X, Vec3::Y);
    place(&mut app, listener, facing_x);
    assert!(left(&mut app));
    place(&mut app, e, Transform::from_xyz(0.0, 0.0, 3.0));
    assert!(right(&mut app));
    place(&mut app, e, Transform::from_xyz(3.0, 0.0, 0.0));
    assert!(centred(&mut app));
    // The listener's own position counts: moved past the sound, it hears
    // it behind (centred) rather than ahead.
    place(
        &mut app,
        listener,
        facing_x.with_translation(Vec3::new(6.0, 0.0, 0.0)),
    );
    assert!(centred(&mut app));
    place(
        &mut app,
        listener,
        facing_x.with_translation(Vec3::new(3.0, 0.0, -4.0)),
    );
    assert!(right(&mut app));
}

/// An emitter is placed by its global transform: it moves with its parent,
/// and with a new parent once reparented.
#[test]
fn emitters_follow_their_hierarchy() {
    let mut app = app();
    app.world_mut().spawn(AudioListener);
    let world = app.world_mut();
    let parent = world.spawn(Transform::from_xyz(5.0, 0.0, 0.0)).id();
    let other = world.spawn(Transform::from_xyz(-5.0, 0.0, 0.0)).id();
    let child = emitter(&mut app, Vec3::ZERO);
    app.world_mut().entity_mut(child).insert(ChildOf(parent));
    assert!(right(&mut app));
    place(&mut app, parent, Transform::from_xyz(-5.0, 0.0, 0.0));
    assert!(left(&mut app));
    place(&mut app, other, Transform::from_xyz(5.0, 0.0, 0.0));
    app.world_mut().entity_mut(child).insert(ChildOf(other));
    assert!(right(&mut app));
    // A child offset by its parent's rotation: a parent turned to face +X
    // puts its local -Z (ahead) at world +X.
    let turned = Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::X, Vec3::Y);
    place(&mut app, other, turned);
    place(&mut app, child, Transform::from_xyz(0.0, 0.0, -3.0));
    assert!(right(&mut app));
}

/// Despawning an emitter stops its sound; despawning the listener keeps
/// its last place, so sounds do not jump.
#[test]
fn despawning_emitters_and_listeners() {
    let mut app = app();
    let facing_x = Transform::default().looking_to(Vec3::X, Vec3::Y);
    let listener = app.world_mut().spawn((AudioListener, facing_x)).id();
    let e = emitter(&mut app, Vec3::new(0.0, 0.0, 3.0));
    assert!(right(&mut app));
    assert_eq!(
        *app.world().resource::<ListenerState>(),
        ListenerState::One(listener)
    );
    app.world_mut().despawn(listener);
    // SoLoud's default listener would hear this sound behind it, centred.
    assert!(right(&mut app));
    assert_eq!(
        *app.world().resource::<ListenerState>(),
        ListenerState::None
    );
    finished(&mut app);
    app.world_mut().despawn(e);
    frame(&mut app);
    frame(&mut app);
    let ended = finished(&mut app);
    assert_eq!(ended.len(), 1);
    assert_eq!(
        (ended[0].entity, ended[0].reason),
        (e, FinishReason::Stopped)
    );
    assert!(frame(&mut app).iter().all(|&s| s == 0.0));
}

/// With several listeners the lowest entity index is heard, and the state
/// says how many there are.
#[test]
fn several_listeners_use_the_lowest_entity_index() {
    let mut app = app();
    let facing_x = Transform::default().looking_to(Vec3::X, Vec3::Y);
    let first = app.world_mut().spawn((AudioListener, facing_x)).id();
    let second = app.world_mut().spawn(AudioListener).id();
    emitter(&mut app, Vec3::new(0.0, 0.0, 3.0));
    assert!(right(&mut app));
    assert_eq!(
        *app.world().resource::<ListenerState>(),
        ListenerState::Several {
            used: first,
            count: 2
        }
    );
    app.world_mut().despawn(first);
    // The other listener faces -Z: the sound is behind it, centred.
    assert!(centred(&mut app));
    assert_eq!(
        *app.world().resource::<ListenerState>(),
        ListenerState::One(second)
    );
}

/// Velocities derived from motion: approaching at 60 m/s raises the pitch,
/// receding lowers it; so does an explicit velocity, and a moving listener.
#[test]
fn doppler_follows_the_direction_of_motion() {
    let still = crossings(nothing, nothing);
    let approaching = crossings(nothing, moving(1.0));
    let receding = crossings(nothing, moving(-1.0));
    assert!(approaching > still + still / 10, "{approaching} vs {still}");
    assert!(receding + still / 10 < still, "{receding} vs {still}");
    let explicit = |app: &mut App, e: Entity| {
        app.world_mut()
            .entity_mut(e)
            .insert(AudioVelocity(Vec3::new(0.0, 0.0, 60.0)));
    };
    assert!(crossings(explicit, nothing) > still + still / 10);
    // The listener moving towards the emitter.
    let listener_moves = |app: &mut App, _: Entity| {
        let mut listeners = app
            .world_mut()
            .query_filtered::<&mut Transform, With<AudioListener>>();
        for mut transform in listeners.iter_mut(app.world_mut()) {
            transform.translation.z -= 1.0;
        }
    };
    assert!(crossings(nothing, listener_moves) > still + still / 10);
}

/// Jumps are not motion: a jump faster than the plausible maximum, or one
/// marked with `AudioTeleport`, has no Doppler; nor does motion while time
/// is paused or does not advance. Unmarked and allowed, the same jumps
/// pass the speed of sound and hold the voice.
#[test]
fn teleports_and_paused_time_have_no_doppler() {
    let still = crossings(nothing, nothing);
    let jump = moving(10.0);
    assert_eq!(crossings(nothing, &jump), still);
    let allow = |app: &mut App, _: Entity| {
        app.world_mut()
            .resource_mut::<SpatialAudioSettings>()
            .max_derived_speed = 1.0e6;
    };
    let marked = |app: &mut App, e: Entity| {
        jump(app, e);
        app.world_mut().entity_mut(e).insert(AudioTeleport);
    };
    assert_eq!(crossings(allow, marked), still);
    assert_eq!(crossings(allow, &jump), 0, "faster than sound is held");
    let pause = |app: &mut App, _: Entity| {
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
    };
    assert_eq!(crossings(pause, moving(1.0)), still);
    let stop_time = |app: &mut App, _: Entity| {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    };
    assert_eq!(crossings(stop_time, moving(1.0)), still);
}
