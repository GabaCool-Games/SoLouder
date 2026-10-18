//! Spatial audio edge cases through the plugin, headless: degenerate
//! transforms, emitters without a usable position, motion on a fixed step,
//! the first frame and distance delay of a placed sound, switching
//! listeners, many emitters, and frames with nothing to update.

mod common;

use bevy::prelude::*;
use bevy_solouder::{
    AudioListener, ListenerState, PlaybackState, SolouderController, SoundStatus, SpatialEmitter,
};
use common::spatial::{FRAME, app, centred, emitter, frame, left, place, right};
use common::{render, state};

fn facing_x() -> Transform {
    Transform::default().looking_to(Vec3::X, Vec3::Y)
}

/// A listener scaled to zero, or given a NaN transform, keeps its last pose
/// instead of panicking; restored, it is heard from again.
#[test]
fn a_degenerate_listener_keeps_its_last_pose() {
    let mut app = app();
    let listener = app.world_mut().spawn((AudioListener, facing_x())).id();
    emitter(&mut app, Vec3::new(0.0, 0.0, 3.0));
    assert!(right(&mut app));
    place(&mut app, listener, facing_x().with_scale(Vec3::ZERO));
    assert!(right(&mut app));
    let nan = Transform::from_translation(Vec3::NAN).with_scale(Vec3::NAN);
    place(&mut app, listener, nan);
    assert!(right(&mut app));
    place(&mut app, listener, Transform::default());
    assert!(centred(&mut app), "facing -Z again, the sound is behind");
}

/// An emitter whose transform is not finite waits to start; once placed it
/// starts in 3D, rather than in 2D at full volume for good.
#[test]
fn an_emitter_without_a_usable_position_waits_to_start() {
    let mut app = app();
    app.world_mut().spawn(AudioListener);
    let e = emitter(&mut app, Vec3::NAN);
    for _ in 0..3 {
        assert!(frame(&mut app).iter().all(|&s| s == 0.0));
        assert_eq!(state(&app, e), PlaybackState::Loading);
    }
    place(&mut app, e, Transform::from_xyz(3.0, 0.0, 0.0));
    assert!(right(&mut app));
}

/// Zero crossings of the left channel in each of 12 frames of an emitter
/// 200 m ahead, moved 3 m closer on every third frame (60 m/s): with the
/// velocity derived over the time since the last move, the pitch holds
/// steady between moves instead of alternating between rest and 180 m/s.
#[test]
fn motion_on_a_fixed_step_keeps_a_steady_pitch() {
    let mut app = app();
    app.world_mut().spawn(AudioListener);
    let e = emitter(&mut app, Vec3::new(0.0, 0.0, -200.0));
    let count = |out: Vec<f32>| {
        let left: Vec<f32> = out.into_iter().step_by(2).collect();
        let pairs = left.windows(2);
        pairs
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count()
    };
    frame(&mut app);
    let still = count(frame(&mut app));
    let mut per_frame = Vec::new();
    for i in 0..15 {
        if i % 3 == 0 {
            let mut transform = app.world_mut().get_mut::<Transform>(e).unwrap();
            transform.translation.z += 3.0;
        }
        let heard = count(frame(&mut app));
        if i >= 3 {
            per_frame.push(heard);
        }
    }
    // 343.3 / (343.3 - 60) is 1.21 times the pitch.
    let (low, high) = (still + still / 8, still + still / 3);
    for &heard in &per_frame {
        assert!(
            (low..=high).contains(&heard),
            "{per_frame:?}, still {still}"
        );
    }
}

/// A sound placed to the listener's right is on the right from its first
/// rendered frame.
#[test]
fn the_first_frame_of_a_placed_sound_is_already_placed() {
    let mut app = app();
    app.world_mut().spawn(AudioListener);
    emitter(&mut app, Vec3::new(3.0, 0.0, 0.0));
    app.update();
    let first = render(&mut app, 64);
    let (left, right) = first.chunks(2).fold((0.0, 0.0), |(l, r), frame| {
        (l + frame[0] * frame[0], r + frame[1] * frame[1])
    });
    assert!(
        right > 0.0 && right > 4.0 * left,
        "left {left}, right {right}"
    );
}

/// The first audible frame of an emitter `distance` metres ahead.
fn onset(distance: f32, distance_delay: bool) -> usize {
    let mut app = app();
    app.world_mut().spawn(AudioListener);
    let e = emitter(&mut app, Vec3::new(0.0, 0.0, -distance));
    app.world_mut()
        .get_mut::<SpatialEmitter>(e)
        .unwrap()
        .distance_delay = distance_delay;
    let mut heard = Vec::new();
    for _ in 0..12 {
        heard.extend(frame(&mut app));
    }
    heard.iter().position(|&s| s != 0.0).expect("audible") / 2
}

/// With distance delay, a sound 34.33 m away starts a tenth of a second
/// (4,800 frames at 48 kHz) later than without.
#[test]
fn distance_delay_starts_the_sound_late() {
    let near = onset(34.33, false);
    let delayed = onset(34.33, true);
    assert!(near < FRAME, "{near}");
    let late = delayed - near;
    assert!((4700..=4900).contains(&late), "{late} frames late");
}

/// Moving `AudioListener` from one entity to another switches what is
/// heard to the new entity's pose; with both, the lower index wins.
#[test]
fn switching_listeners() {
    let mut app = app();
    let a = app.world_mut().spawn((AudioListener, facing_x())).id();
    let facing_back = Transform::default().looking_to(Vec3::NEG_X, Vec3::Y);
    let b = app.world_mut().spawn(facing_back).id();
    emitter(&mut app, Vec3::new(0.0, 0.0, 3.0));
    assert!(right(&mut app));
    app.world_mut().entity_mut(a).remove::<AudioListener>();
    app.world_mut().entity_mut(b).insert(AudioListener);
    assert!(left(&mut app));
    assert_eq!(
        *app.world().resource::<ListenerState>(),
        ListenerState::One(b)
    );
    app.world_mut().entity_mut(a).insert(AudioListener);
    assert!(right(&mut app));
}

/// Three hundred emitters spawned in one frame all start in 3D, and all
/// follow their entities, through one 3D update per frame.
#[test]
fn many_emitters() {
    let mut app = app();
    app.world_mut().spawn(AudioListener);
    let emitters: Vec<Entity> = (0..300)
        .map(|i| emitter(&mut app, Vec3::new(3.0 + i as f32 * 0.01, 0.0, 0.0)))
        .collect();
    for _ in 0..20 {
        frame(&mut app);
    }
    for &e in &emitters {
        let status = app.world().get::<SoundStatus>(e).unwrap();
        assert_eq!(status.state(), PlaybackState::Playing);
    }
    assert!(right(&mut app));
    for &e in &emitters {
        place(&mut app, e, Transform::from_xyz(-3.0, 0.0, 0.0));
    }
    assert!(left(&mut app));
}

/// Frames in which nothing moved send no 3D update; a moved emitter, or a
/// moved listener, sends one.
#[test]
fn still_frames_send_no_update() {
    let mut app = app();
    let listener = app.world_mut().spawn(AudioListener).id();
    let e = emitter(&mut app, Vec3::new(3.0, 0.0, 0.0));
    for _ in 0..3 {
        frame(&mut app);
    }
    let space = |app: &mut App| {
        app.world_mut()
            .resource_mut::<SolouderController>()
            .get()
            .queue_space()
    };
    let before = space(&mut app);
    app.update();
    app.update();
    assert_eq!(space(&mut app), before, "still frames queued commands");
    place(&mut app, e, Transform::from_xyz(4.0, 0.0, 0.0));
    app.update();
    assert_eq!(space(&mut app), before - 1, "one update for the move");
    place(&mut app, listener, Transform::from_xyz(0.0, 1.0, 0.0));
    app.update();
    assert_eq!(space(&mut app), before - 2, "one update for the listener");
    app.update();
    assert_eq!(space(&mut app), before - 2);
}
