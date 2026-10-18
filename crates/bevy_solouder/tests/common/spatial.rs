//! Spatial helpers: a headless app whose time advances 1/60 s per update,
//! emitters, and what the listener hears.

use std::time::Duration;

use bevy::app::TaskPoolPlugin;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use bevy::transform::TransformPlugin;
use bevy_solouder::{
    Attenuation, AudioListener, OutputMode, PlaybackSettings, SolouderPlugin, SoundPlayer,
    SpatialEmitter,
};

use super::{add_clip, render, tone};

/// Audio frames per app frame: 48 kHz at 60 frames per second.
pub const FRAME: usize = 800;

/// A headless app with time advancing 1/60 s per update and transform
/// propagation.
pub fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        TransformPlugin,
        AssetPlugin {
            file_path: "../../tests/fixtures".into(),
            ..AssetPlugin::default()
        },
        SolouderPlugin {
            output: OutputMode::headless(),
            ..SolouderPlugin::default()
        },
    ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app.finish();
    app.cleanup();
    app
}

/// A looping tone played by an emitter with no attenuation at `at`.
pub fn emitter(app: &mut App, at: Vec3) -> Entity {
    let clip = add_clip(app, tone(2.0));
    let looping = PlaybackSettings {
        looping: true,
        ..PlaybackSettings::default()
    };
    let flat = SpatialEmitter {
        attenuation: Attenuation::None,
        ..SpatialEmitter::default()
    };
    let place = Transform::from_translation(at);
    app.world_mut()
        .spawn((SoundPlayer(clip), looping, flat, place))
        .id()
}

/// One app frame, then its audio.
pub fn frame(app: &mut App) -> Vec<f32> {
    app.update();
    render(app, FRAME)
}

/// The left and right energy of a few frames' audio, once the sound plays.
pub fn sides(app: &mut App) -> (f32, f32) {
    frame(app);
    let mut out = Vec::new();
    for _ in 0..3 {
        out.extend(frame(app));
    }
    let energy = |channel: usize| out.iter().skip(channel).step_by(2).map(|s| s * s).sum();
    (energy(0), energy(1))
}

pub fn right(app: &mut App) -> bool {
    let (left, right) = sides(app);
    right > 4.0 * left && right > 1.0
}

pub fn left(app: &mut App) -> bool {
    let (left, right) = sides(app);
    left > 4.0 * right && left > 1.0
}

pub fn centred(app: &mut App) -> bool {
    let (left, right) = sides(app);
    (left - right).abs() < 0.01 * left && left > 1.0
}

pub fn place(app: &mut App, entity: Entity, transform: Transform) {
    *app.world_mut().get_mut::<Transform>(entity).unwrap() = transform;
}

/// Zero crossings of the left channel over eight frames of an emitter
/// starting 200 m ahead of a listener at the origin, after `motion` runs
/// before each frame.
pub fn crossings(setup: impl FnOnce(&mut App, Entity), motion: impl Fn(&mut App, Entity)) -> usize {
    let mut app = app();
    app.world_mut().spawn(AudioListener);
    let e = emitter(&mut app, Vec3::new(0.0, 0.0, -200.0));
    setup(&mut app, e);
    let mut heard = Vec::new();
    for i in 0..10 {
        motion(&mut app, e);
        let out = frame(&mut app);
        if i >= 2 {
            heard.extend(out.into_iter().step_by(2));
        }
    }
    heard
        .windows(2)
        .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
        .count()
}

pub fn nothing(_: &mut App, _: Entity) {}

/// Moves the entity along z by `step` metres.
pub fn moving(step: f32) -> impl Fn(&mut App, Entity) {
    move |app, entity| {
        app.world_mut()
            .get_mut::<Transform>(entity)
            .unwrap()
            .translation
            .z += step;
    }
}
