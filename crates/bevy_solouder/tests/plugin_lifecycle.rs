//! Plugin startup, failure and teardown.
//!
//! The device tests skip without an audio device; set
//! `SOLOUDER_REQUIRE_DEVICE=1` to make a missing device a failure.

mod common;

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy_solouder::{
    AudioOutput, AudioOutputState, FinishReason, MixerConfig, OutputMode, PlaybackError,
    PlaybackSettings, PlaybackState, SolouderController, SoundPlayer, SoundStatus,
};
use common::*;
use solouder_core::Clipper;

#[test]
fn without_an_output_players_fail_and_the_app_keeps_running() {
    let mut app = app_with(OutputMode::Headless(MixerConfig {
        channels: 3,
        sample_rate: 48_000,
        buffer_size: 2048,
        clipper: Clipper::Roundoff,
        left_handed_3d: false,
    }));
    assert!(matches!(
        app.world().resource::<AudioOutput>().state,
        AudioOutputState::Unavailable(_)
    ));
    assert!(app.world().get_resource::<SolouderController>().is_none());
    let clip = add_clip(&mut app, tone(0.01));
    let entity = app.world_mut().spawn(SoundPlayer(clip)).id();
    app.update();
    assert_eq!(
        state(&app, entity),
        PlaybackState::Failed(PlaybackError::OutputUnavailable)
    );
    assert_eq!(failed(&mut app).len(), 1);
    app.world_mut().despawn(entity);
    app.update();
}

#[test]
fn despawning_before_the_clip_loads_never_starts_a_voice() {
    let mut app = headless_app();
    let clip: Handle<_> = app
        .world()
        .resource::<AssetServer>()
        .load("wav/s16_stereo_44100.wav");
    let entity = app.world_mut().spawn(SoundPlayer(clip.clone())).id();
    app.world_mut().despawn(entity);
    update_until(&mut app, |app| {
        app.world().resource::<AssetServer>().is_loaded(clip.id())
    });
    app.update();
    assert_eq!(voice_ids_in_use(&mut app), 0);
    assert!(finished(&mut app).is_empty());
}

#[test]
fn a_burst_larger_than_the_queues_is_deferred_not_lost() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.005));
    let entities: Vec<Entity> = (0..5000)
        .map(|_| app.world_mut().spawn(SoundPlayer(clip.clone())).id())
        .collect();
    app.update();
    let waiting = entities
        .iter()
        .filter(|&&e| state(&app, e) == PlaybackState::Loading)
        .count();
    assert!(waiting > 0, "the command queue should have filled");

    // Only 16 voices are mixed at once, so voice IDs are released more slowly
    // than commands drain; once all 4096 are reserved, new plays fail rather
    // than start late. Every play still gets exactly one outcome.
    let mut outcomes = finished(&mut app).len() + failed(&mut app).len();
    update_until(&mut app, |app| {
        outcomes += finished(app).len() + failed(app).len();
        render(app, 2048);
        outcomes == entities.len()
    });
    let mut failures = 0;
    for &entity in &entities {
        match state(&app, entity) {
            PlaybackState::Finished(_) => {}
            PlaybackState::Failed(PlaybackError::NoFreeVoice) => failures += 1,
            other => panic!("{other:?}"),
        }
    }
    assert!(failures < entities.len() - 4096);
    assert_eq!(voice_ids_in_use(&mut app), 0);
}

fn device_app() -> Option<App> {
    let app = app_with(OutputMode::default());
    if let AudioOutputState::Unavailable(why) = &app.world().resource::<AudioOutput>().state {
        assert!(
            std::env::var_os("SOLOUDER_REQUIRE_DEVICE").is_none(),
            "no usable output device: {why}"
        );
        eprintln!("skipped: {why}");
        return None;
    }
    Some(app)
}

fn update_for(app: &mut App, mut done: impl FnMut(&mut App) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        app.update();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn device_apps_start_play_and_tear_down_repeatedly() {
    for _ in 0..3 {
        let Some(mut app) = device_app() else { return };
        assert_eq!(
            app.world().resource::<AudioOutput>().state,
            AudioOutputState::Playing
        );
        assert!(app.world().resource::<AudioOutput>().info.is_some());
        let clip = add_clip(&mut app, tone(0.02));
        let entity = app.world_mut().spawn(SoundPlayer(clip)).id();
        assert!(update_for(&mut app, |app| {
            state(app, entity) == PlaybackState::Finished(FinishReason::Ended)
        }));
        drop(app);
    }
}

#[test]
fn dropping_a_device_app_mid_playback_returns_promptly() {
    let Some(mut app) = device_app() else { return };
    let clip = add_clip(&mut app, tone(0.2));
    for _ in 0..8 {
        app.world_mut()
            .spawn((SoundPlayer(clip.clone()), PlaybackSettings::looping()));
    }
    assert!(update_for(&mut app, |app| {
        let mut statuses = app.world_mut().query::<&SoundStatus>();
        statuses
            .iter(app.world())
            .all(|s| s.state() == PlaybackState::Playing)
    }));
    let started = Instant::now();
    drop(app);
    assert!(started.elapsed() < Duration::from_secs(2));
}
