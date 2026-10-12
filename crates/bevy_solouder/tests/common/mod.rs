//! A headless app and helpers shared by the plugin tests.

#![allow(dead_code)]

use std::time::{Duration, Instant};

use bevy::app::TaskPoolPlugin;
use bevy::asset::AssetPlugin;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use bevy_solouder::{
    AudioClip, HeadlessRenderer, OutputMode, PlaybackState, SolouderController, SolouderPlugin,
    SoundFailed, SoundFinished, SoundStatus,
};
use solouder_core::Pcm;

pub fn app_with(output: OutputMode) -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: "../../tests/fixtures".into(),
            ..AssetPlugin::default()
        },
        SolouderPlugin {
            output,
            ..SolouderPlugin::default()
        },
    ));
    app.finish();
    app.cleanup();
    app
}

pub fn headless_app() -> App {
    app_with(OutputMode::headless())
}

/// A mono clip at 48 kHz.
pub fn tone(seconds: f32) -> AudioClip {
    let samples = (0..(48_000.0 * seconds) as usize)
        .map(|i| (i as f32 * 0.05).sin() * 0.5)
        .collect();
    AudioClip::new(Pcm::from_planar(samples, 1, 48_000.0).unwrap())
}

pub fn add_clip(app: &mut App, clip: AudioClip) -> Handle<AudioClip> {
    app.world_mut()
        .resource_mut::<Assets<AudioClip>>()
        .add(clip)
}

/// Renders `frames` stereo frames and returns them.
pub fn render(app: &mut App, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    app.world_mut()
        .resource_mut::<HeadlessRenderer>()
        .render(&mut out);
    out
}

pub fn state(app: &App, entity: Entity) -> PlaybackState {
    app.world().get::<SoundStatus>(entity).unwrap().state()
}

pub fn finished(app: &mut App) -> Vec<SoundFinished> {
    app.world_mut()
        .resource_mut::<Messages<SoundFinished>>()
        .drain()
        .collect()
}

pub fn failed(app: &mut App) -> Vec<SoundFailed> {
    app.world_mut()
        .resource_mut::<Messages<SoundFailed>>()
        .drain()
        .collect()
}

pub fn voice_ids_in_use(app: &mut App) -> usize {
    app.world_mut()
        .resource_mut::<SolouderController>()
        .get()
        .voice_ids_in_use()
}

/// Updates (rendering a little audio each time) until `done` holds.
pub fn update_until(app: &mut App, mut done: impl FnMut(&mut App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        if done(app) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out");
        render(app, 256);
        std::thread::sleep(Duration::from_millis(1));
    }
}
