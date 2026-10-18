//! Generated clips: generator files loaded through the asset server, and
//! generators wrapped in code, played headless.

mod common;

use bevy::prelude::*;
use bevy_solouder::{
    AudioClip, MonotoneSettings, PlaybackError, PlaybackSettings, PlaybackState, SoundPlayer,
};
use common::{add_clip, headless_app, render, state, update_until, voice_ids_in_use};
use solouder_core::{FinishReason, Synth};
use solouder_sources::{Vic, Vizsn};

fn loaded(app: &mut App, players: &[Entity]) {
    update_until(app, |app| {
        players
            .iter()
            .all(|&player| state(app, player) != PlaybackState::Loading)
    });
}

fn audible(samples: &[f32]) -> bool {
    samples.iter().any(|&s| s != 0.0) && samples.iter().all(|s| s.is_finite())
}

#[test]
fn generator_files_load_and_play() {
    let mut app = headless_app();
    let server = app.world().resource::<AssetServer>().clone();
    let clips: Vec<Handle<AudioClip>> = [
        "sfxr/v102_laser.sfx",
        "monotone/demo.mon",
        "chipdump/ted.dump",
        "chipdump/sid_8580.zx7.dump",
        "chipdump/ay_ay.dump",
    ]
    .into_iter()
    .map(|path| server.load(path))
    .collect();
    for clip in &clips {
        let player = app.world_mut().spawn(SoundPlayer(clip.clone())).id();
        update_until(&mut app, |app| state(app, player) == PlaybackState::Playing);
        let assets = app.world().resource::<Assets<AudioClip>>();
        let asset = assets.get(clip).unwrap();
        assert!(asset.synth().is_some() && asset.pcm().is_none() && asset.stream().is_none());
        assert_eq!(asset.duration_seconds(), None);
        let mut heard = false;
        for _ in 0..20 {
            heard |= audible(&render(&mut app, 2048));
        }
        assert!(heard, "{clip:?} is heard");
        app.world_mut().entity_mut(player).despawn();
        app.update();
        render(&mut app, 256);
        app.update();
    }
}

#[test]
fn bad_generator_files_fail_their_players() {
    let mut app = headless_app();
    let server = app.world().resource::<AssetServer>().clone();
    let players: Vec<Entity> = [
        "sfxr/bad_cut_102.sfx",
        "monotone/bad_13_tracks.mon",
        "chipdump/bad_magic.dump",
        "chipdump/bad_ay_cpu_0.dump",
        "chipdump/bad_no_wait.dump",
    ]
    .into_iter()
    .map(|path| app.world_mut().spawn(SoundPlayer(server.load(path))).id())
    .collect();
    loaded(&mut app, &players);
    for player in players {
        assert_eq!(
            state(&app, player),
            PlaybackState::Failed(PlaybackError::AssetFailed)
        );
    }
}

#[test]
fn monotone_settings_reach_the_song() {
    let mut app = headless_app();
    let server = app.world().resource::<AssetServer>().clone();
    // The asset server loads a path once, so each setting takes a file.
    let load = |path: &'static str, hardware_channels| -> Handle<AudioClip> {
        server
            .load_builder()
            .with_settings(move |settings: &mut MonotoneSettings| {
                settings.hardware_channels = hardware_channels;
            })
            .load(path)
    };
    let players =
        [("monotone/twelve.mon", 3), ("monotone/demo.mon", 13)].map(|(path, channels)| {
            app.world_mut()
                .spawn(SoundPlayer(load(path, channels)))
                .id()
        });
    loaded(&mut app, &players);
    update_until(&mut app, |app| {
        state(app, players[0]) == PlaybackState::Playing
    });
    assert_eq!(
        state(&app, players[1]),
        PlaybackState::Failed(PlaybackError::AssetFailed),
        "13 hardware channels are refused"
    );
}

#[test]
fn code_built_generators_play_and_stay_live() {
    let mut app = headless_app();
    let vic = Vic::new();
    vic.set_register(Vic::BASS, 200).unwrap();
    let clip = add_clip(&mut app, AudioClip::from_generator(vic.clone()).unwrap());
    let player = app.world_mut().spawn(SoundPlayer(clip)).id();
    update_until(&mut app, |app| state(app, player) == PlaybackState::Playing);
    assert!(audible(&render(&mut app, 4096)));
    // The clip's generator is the caller's: a register change reaches the
    // playing voice.
    vic.set_register(Vic::BASS, 0).unwrap();
    render(&mut app, 4096);
    assert!(render(&mut app, 4096).iter().all(|&s| s.abs() < 1e-3));
    app.world_mut().entity_mut(player).despawn();
    app.update();
    render(&mut app, 256);
    app.update();
    assert_eq!(voice_ids_in_use(&mut app), 0);

    // A synth's own settings come with it; the player's looping wins.
    let mut synth = Synth::new(std::sync::Arc::new(Vizsn::new("moi"))).unwrap();
    synth.set_volume(0.5);
    let clip = add_clip(&mut app, AudioClip::generated(synth));
    let once = app.world_mut().spawn(SoundPlayer(clip.clone())).id();
    let looping = app
        .world_mut()
        .spawn((SoundPlayer(clip), PlaybackSettings::looping()))
        .id();
    update_until(&mut app, |app| {
        state(app, once) == PlaybackState::Playing && state(app, looping) == PlaybackState::Playing
    });
    for _ in 0..8 {
        render(&mut app, 4096);
    }
    app.update();
    assert_eq!(
        state(&app, once),
        PlaybackState::Finished(FinishReason::Ended)
    );
    assert_eq!(state(&app, looping), PlaybackState::Playing);
}
