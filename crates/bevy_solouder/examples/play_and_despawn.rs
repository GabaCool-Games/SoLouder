//! Plays a looping drone and a one-shot, then despawns the drone to stop it.
//!
//! ```sh
//! cargo run -p bevy_solouder --example play_and_despawn
//! ```

use std::time::Duration;

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;
use bevy_solouder::{
    AudioClip, AudioOutput, PlaybackSettings, SolouderPlugin, SoundFailed, SoundFinished,
    SoundPlayer,
};
use solouder_core::Pcm;

#[derive(Component)]
struct Drone;

fn chord(seconds: f32, freqs: &[f32]) -> AudioClip {
    let rate = 48_000.0;
    let samples = (0..(rate * seconds) as usize)
        .map(|i| {
            let t = i as f32 / rate;
            freqs
                .iter()
                .map(|f| (std::f32::consts::TAU * f * t).sin())
                .sum::<f32>()
                * 0.15
        })
        .collect();
    AudioClip::new(Pcm::from_planar(samples, 1, rate).expect("valid audio"))
}

fn setup(mut commands: Commands, mut clips: ResMut<Assets<AudioClip>>, output: Res<AudioOutput>) {
    println!("output: {:?} {:?}", output.state, output.info);
    let drone = clips.add(chord(0.5, &[110.0, 165.0]));
    commands.spawn((Drone, SoundPlayer(drone), PlaybackSettings::looping()));

    let ping = clips.add(chord(0.4, &[880.0]));
    commands.spawn((
        SoundPlayer(ping),
        PlaybackSettings {
            volume: 0.6,
            pan: 0.5,
            ..PlaybackSettings::despawn_on_finish()
        },
    ));
}

fn stop_drone_after_two_seconds(
    time: Res<Time>,
    drones: Query<Entity, With<Drone>>,
    mut commands: Commands,
) {
    if time.elapsed_secs() > 2.0 {
        for drone in &drones {
            println!("despawning the drone");
            commands.entity(drone).despawn();
        }
    }
}

fn report(
    mut finished: MessageReader<SoundFinished>,
    mut failed: MessageReader<SoundFailed>,
    mut ended: Local<usize>,
    mut exit: MessageWriter<AppExit>,
) {
    for message in failed.read() {
        println!("failed: {message:?}");
        exit.write(AppExit::error());
    }
    for message in finished.read() {
        println!("finished: {message:?}");
        *ended += 1;
    }
    if *ended == 2 {
        exit.write(AppExit::Success);
    }
}

fn main() -> AppExit {
    App::new()
        .add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                1.0 / 60.0,
            ))),
            AssetPlugin::default(),
            SolouderPlugin::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (stop_drone_after_two_seconds, report))
        .run()
}
