//! Playback entities driven by a headless mixer.

mod common;

use bevy::prelude::*;
use bevy_solouder::{
    AudioClip, FinishReason, HeadlessStreamWorker, PlaybackError, PlaybackSettings, PlaybackState,
    SoundFailed, SoundFinished, SoundPlayer, WavSettings,
};
use common::*;

#[test]
fn a_clip_plays_to_the_end() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.01));
    let entity = app.world_mut().spawn(SoundPlayer(clip)).id();
    app.update();
    assert_eq!(state(&app, entity), PlaybackState::Starting);

    let out = render(&mut app, 1024);
    assert!(out.iter().any(|&s| s != 0.0), "the clip was not mixed");
    app.update();
    assert_eq!(
        state(&app, entity),
        PlaybackState::Finished(FinishReason::Ended)
    );
    assert_eq!(
        finished(&mut app),
        [SoundFinished {
            entity,
            reason: FinishReason::Ended
        }]
    );
    assert_eq!(voice_ids_in_use(&mut app), 0);
}

#[test]
fn despawning_stops_the_voice() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.1));
    let entity = app
        .world_mut()
        .spawn((SoundPlayer(clip), PlaybackSettings::looping()))
        .id();
    app.update();
    render(&mut app, 512);
    app.update();
    assert_eq!(state(&app, entity), PlaybackState::Playing);

    app.world_mut().despawn(entity);
    app.update();
    render(&mut app, 512);
    app.update();
    assert_eq!(
        finished(&mut app),
        [SoundFinished {
            entity,
            reason: FinishReason::Stopped
        }]
    );
    assert_eq!(voice_ids_in_use(&mut app), 0);
    // The voice ramps out over the first render; after that, silence.
    assert!(render(&mut app, 512).iter().all(|&s| s == 0.0));
}

#[test]
fn removing_the_player_stops_the_voice_and_keeps_the_entity() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.1));
    let entity = app
        .world_mut()
        .spawn((SoundPlayer(clip), PlaybackSettings::looping()))
        .id();
    app.update();
    render(&mut app, 256);
    app.world_mut().entity_mut(entity).remove::<SoundPlayer>();
    app.update();
    render(&mut app, 256);
    app.update();
    assert_eq!(finished(&mut app).len(), 1);
    assert!(app.world().get_entity(entity).is_ok());
    assert_eq!(voice_ids_in_use(&mut app), 0);
}

#[test]
fn replacing_the_player_restarts_with_the_new_clip() {
    let mut app = headless_app();
    let first = add_clip(&mut app, tone(0.1));
    let second = add_clip(&mut app, tone(0.01));
    let entity = app
        .world_mut()
        .spawn((SoundPlayer(first), PlaybackSettings::looping()))
        .id();
    app.update();
    render(&mut app, 256);
    app.update();
    let old = app
        .world()
        .get::<bevy_solouder::SoundStatus>(entity)
        .unwrap()
        .voice();

    app.world_mut()
        .entity_mut(entity)
        .insert((SoundPlayer(second), PlaybackSettings::default()));
    app.update();
    let new = app
        .world()
        .get::<bevy_solouder::SoundStatus>(entity)
        .unwrap()
        .voice();
    assert!(new.is_some() && new != old);
    render(&mut app, 2048);
    app.update();
    let mut reasons: Vec<_> = finished(&mut app).iter().map(|m| m.reason).collect();
    reasons.sort_by_key(|r| *r == FinishReason::Ended);
    assert_eq!(reasons, [FinishReason::Stopped, FinishReason::Ended]);
    assert_eq!(
        state(&app, entity),
        PlaybackState::Finished(FinishReason::Ended)
    );
}

#[test]
fn pausing_silences_and_resuming_continues() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.1));
    let entity = app
        .world_mut()
        .spawn((SoundPlayer(clip), PlaybackSettings::looping()))
        .id();
    app.update();
    render(&mut app, 256);
    app.world_mut()
        .get_mut::<PlaybackSettings>(entity)
        .unwrap()
        .paused = true;
    app.update();
    render(&mut app, 256);
    assert!(render(&mut app, 256).iter().all(|&s| s == 0.0));

    app.world_mut()
        .get_mut::<PlaybackSettings>(entity)
        .unwrap()
        .paused = false;
    app.update();
    assert!(render(&mut app, 256).iter().any(|&s| s != 0.0));
    assert_eq!(state(&app, entity), PlaybackState::Playing);
}

#[test]
fn despawn_on_finish_removes_the_entity() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.01));
    let entity = app
        .world_mut()
        .spawn((SoundPlayer(clip), PlaybackSettings::despawn_on_finish()))
        .id();
    app.update();
    render(&mut app, 1024);
    app.update();
    assert!(app.world().get_entity(entity).is_err());
    assert_eq!(finished(&mut app).len(), 1);
}

#[test]
fn wav_files_load_through_the_asset_server() {
    let mut app = headless_app();
    let clip = app
        .world()
        .resource::<AssetServer>()
        .load("wav/s16_stereo_44100.wav");
    let entity = app.world_mut().spawn(SoundPlayer(clip)).id();
    update_until(&mut app, |app| state(app, entity) != PlaybackState::Loading);
    render(&mut app, 4096);
    app.update();
    assert_eq!(
        state(&app, entity),
        PlaybackState::Finished(FinishReason::Ended)
    );
}

#[test]
fn streamed_wav_files_play_through_the_worker() {
    let mut app = headless_app();
    let streamed: Handle<AudioClip> = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|settings: &mut WavSettings| settings.stream = true)
        .load("wav/s16_stereo_44100.wav");
    let entity = app.world_mut().spawn(SoundPlayer(streamed.clone())).id();
    update_until(&mut app, |app| state(app, entity) != PlaybackState::Loading);
    let clips = app.world().resource::<Assets<AudioClip>>();
    let clip = clips.get(&streamed).unwrap();
    assert!(clip.stream().is_some() && clip.pcm().is_none());
    assert_eq!(clip.duration_seconds(), 1500.0 / 44100.0);

    // Silent until the worker has decoded the start.
    assert!(render(&mut app, 512).iter().all(|&s| s == 0.0));
    app.world_mut()
        .resource_mut::<HeadlessStreamWorker>()
        .service();
    assert!(render(&mut app, 512).iter().any(|&s| s != 0.0));
    for _ in 0..4 {
        app.world_mut()
            .resource_mut::<HeadlessStreamWorker>()
            .service();
        render(&mut app, 1024);
    }
    app.update();
    assert_eq!(
        state(&app, entity),
        PlaybackState::Finished(FinishReason::Ended)
    );
}

#[test]
fn flac_files_load_and_stream_through_the_asset_server() {
    let mut app = headless_app();
    let server = app.world().resource::<AssetServer>().clone();
    let decoded = server.load("flac/s16_stereo_44100.flac");
    let streamed: Handle<AudioClip> = server
        .load_builder()
        .with_settings(|settings: &mut WavSettings| settings.stream = true)
        .load("flac/s32_mono_44100.flac");
    let first = app.world_mut().spawn(SoundPlayer(decoded)).id();
    let second = app.world_mut().spawn(SoundPlayer(streamed.clone())).id();
    update_until(&mut app, |app| {
        state(app, first) != PlaybackState::Loading && state(app, second) != PlaybackState::Loading
    });
    let clips = app.world().resource::<Assets<AudioClip>>();
    assert_eq!(
        clips.get(&streamed).unwrap().duration_seconds(),
        1500.0 / 44100.0
    );
    for _ in 0..8 {
        app.world_mut()
            .resource_mut::<HeadlessStreamWorker>()
            .service();
        render(&mut app, 1024);
    }
    app.update();
    for entity in [first, second] {
        assert_eq!(
            state(&app, entity),
            PlaybackState::Finished(FinishReason::Ended)
        );
    }
}

#[test]
fn a_bad_file_fails_the_player() {
    let mut app = headless_app();
    let clip = app
        .world()
        .resource::<AssetServer>()
        .load("wav/bad_zero_rate.wav");
    let entity = app.world_mut().spawn(SoundPlayer(clip)).id();
    update_until(&mut app, |app| state(app, entity) != PlaybackState::Loading);
    assert_eq!(
        state(&app, entity),
        PlaybackState::Failed(PlaybackError::AssetFailed)
    );
    assert_eq!(
        failed(&mut app),
        [SoundFailed {
            entity,
            error: PlaybackError::AssetFailed
        }]
    );
}
