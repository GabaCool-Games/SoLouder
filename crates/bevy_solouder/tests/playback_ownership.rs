//! Who owns a voice, and what happens when the mixer or the output takes it
//! away.

mod common;

use bevy::prelude::*;
use bevy_solouder::{
    AudioOutput, AudioOutputState, FinishReason, HeadlessRenderer, OutputMode, PlaybackError,
    PlaybackSettings, PlaybackState, SolouderController, SoundFailed, SoundFinished, SoundPlayer,
    SoundStatus,
};
use common::*;
use solouder_core::{Mixer, PlayOptions, Sound};

fn queue_space(app: &mut App) -> usize {
    app.world_mut()
        .resource_mut::<SolouderController>()
        .get()
        .queue_space()
}

fn spawn_playing(app: &mut App, settings: PlaybackSettings) -> Entity {
    let clip = add_clip(app, tone(0.1));
    let entity = app.world_mut().spawn((SoundPlayer(clip), settings)).id();
    app.update();
    render(app, 256);
    app.update();
    assert_eq!(state(app, entity), PlaybackState::Playing);
    entity
}

#[test]
fn removing_the_status_does_not_orphan_the_voice() {
    let mut app = headless_app();
    let entity = spawn_playing(&mut app, PlaybackSettings::looping());
    app.world_mut().entity_mut(entity).remove::<SoundStatus>();
    app.update();
    app.world_mut().despawn(entity);
    app.update();
    render(&mut app, 256);
    app.update();
    assert_eq!(
        finished(&mut app),
        [SoundFinished {
            entity,
            reason: FinishReason::Stopped
        }]
    );
    assert_eq!(voice_ids_in_use(&mut app), 0);
}

#[test]
fn a_replaced_status_shows_the_current_state() {
    let mut app = headless_app();
    let playing = spawn_playing(&mut app, PlaybackSettings::looping());
    let waiting = app.world_mut().spawn(SoundPlayer(Handle::default())).id();
    let stale = *app.world().get::<SoundStatus>(waiting).unwrap();
    app.world_mut().entity_mut(playing).insert(stale);
    assert_eq!(state(&app, playing), PlaybackState::Playing);
    assert!(
        app.world()
            .get::<SoundStatus>(playing)
            .unwrap()
            .voice()
            .is_some()
    );
}

#[test]
fn a_stolen_sound_says_so_and_finishes_its_entity() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.01));
    let paused = PlaybackSettings {
        paused: true,
        ..PlaybackSettings::despawn_on_finish()
    };
    let first: Vec<Entity> = (0..solouder_core::VOICE_COUNT)
        .map(|_| {
            app.world_mut()
                .spawn((SoundPlayer(clip.clone()), paused))
                .id()
        })
        .collect();
    update_until(&mut app, |app| {
        first
            .iter()
            .all(|&e| state(app, e) == PlaybackState::Playing)
    });
    let extra = app.world_mut().spawn(SoundPlayer(clip)).id();
    let mut messages = Vec::new();
    update_until(&mut app, |app| {
        messages.extend(finished(app));
        matches!(state(app, extra), PlaybackState::Finished(_))
    });

    let stolen: Vec<_> = messages
        .iter()
        .filter(|m| m.reason == FinishReason::Stolen)
        .collect();
    assert_eq!(stolen.len(), 1);
    assert!(first.contains(&stolen[0].entity));
    assert!(app.world().get_entity(stolen[0].entity).is_err());
    let alive = first
        .iter()
        .filter(|&&e| app.world().get_entity(e).is_ok())
        .count();
    assert_eq!(alive, first.len() - 1);
}

#[test]
fn starting_settings_ride_with_the_play_request() {
    let tone = tone(0.1);
    let options = PlayOptions {
        speed: 1.7,
        protect: true,
        ..PlayOptions::default()
    };
    let mut mixer = Mixer::new(match OutputMode::headless() {
        OutputMode::Headless(config) => config,
        OutputMode::Device(_) => unreachable!(),
    })
    .unwrap();
    mixer.play_with(&Sound::new(tone.pcm().clone()), options);
    let mut expected = vec![0.0; 2 * 512];
    mixer.mix_interleaved(&mut expected);

    let mut app = headless_app();
    let capacity = queue_space(&mut app);
    // Leave room for exactly one command: the play itself.
    while queue_space(&mut app) > 1 {
        app.world_mut()
            .resource_mut::<SolouderController>()
            .get()
            .set_global_volume(1.0)
            .unwrap();
    }
    let clip = add_clip(&mut app, tone);
    let settings = PlaybackSettings {
        speed: 1.7,
        protect: true,
        ..PlaybackSettings::default()
    };
    let entity = app.world_mut().spawn((SoundPlayer(clip), settings)).id();
    app.update();
    assert_eq!(state(&app, entity), PlaybackState::Starting);
    while queue_space(&mut app) < capacity {
        render(&mut app, 0);
    }
    assert_eq!(render(&mut app, 512), expected);
}

#[test]
fn unusable_settings_are_ignored_and_never_resent() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(0.1));
    let broken = PlaybackSettings {
        volume: f32::NAN,
        pan: f32::INFINITY,
        speed: f32::NAN,
        looping: true,
        ..PlaybackSettings::default()
    };
    let entity = app.world_mut().spawn((SoundPlayer(clip), broken)).id();
    app.update();
    let out = render(&mut app, 256);
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(out.iter().any(|&s| s != 0.0));
    app.update();
    assert_eq!(state(&app, entity), PlaybackState::Playing);

    let space = queue_space(&mut app);
    app.world_mut()
        .get_mut::<PlaybackSettings>(entity)
        .unwrap()
        .speed = -1.0;
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(queue_space(&mut app), space);
    assert!(render(&mut app, 256).iter().all(|s| s.is_finite()));
}

#[test]
fn quiet_frames_send_nothing() {
    let mut app = headless_app();
    spawn_playing(&mut app, PlaybackSettings::looping());
    render(&mut app, 0);
    let space = queue_space(&mut app);
    for _ in 0..5 {
        app.update();
    }
    assert_eq!(queue_space(&mut app), space);
}

fn assert_all_fail_with_output_unavailable(app: &mut App, entities: &[Entity]) {
    for &entity in entities {
        assert_eq!(
            state(app, entity),
            PlaybackState::Failed(PlaybackError::OutputUnavailable)
        );
    }
    let mut failures = failed(app);
    failures.sort_by_key(|m| m.entity);
    let mut expected: Vec<_> = entities
        .iter()
        .map(|&entity| SoundFailed {
            entity,
            error: PlaybackError::OutputUnavailable,
        })
        .collect();
    expected.sort_by_key(|m| m.entity);
    assert_eq!(failures, expected);
}

#[test]
fn a_failed_output_fails_playing_and_waiting_sounds() {
    let mut app = headless_app();
    let playing = spawn_playing(&mut app, PlaybackSettings::looping());
    let waiting = app.world_mut().spawn(SoundPlayer(Handle::default())).id();
    app.update();
    assert_eq!(state(&app, waiting), PlaybackState::Loading);

    app.world_mut().resource_mut::<AudioOutput>().state = AudioOutputState::Failed;
    app.update();
    assert_all_fail_with_output_unavailable(&mut app, &[playing, waiting]);

    let late = add_clip(&mut app, tone(0.1));
    let late = app.world_mut().spawn(SoundPlayer(late)).id();
    app.update();
    assert_all_fail_with_output_unavailable(&mut app, &[late]);
}

#[test]
fn a_dropped_renderer_fails_playing_sounds() {
    let mut app = headless_app();
    let playing = spawn_playing(&mut app, PlaybackSettings::looping());
    drop(app.world_mut().remove_resource::<HeadlessRenderer>());
    app.update();
    assert_all_fail_with_output_unavailable(&mut app, &[playing]);
    app.world_mut().despawn(playing);
    app.update();
}
