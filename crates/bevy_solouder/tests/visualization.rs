//! The visualization resource, headless: off by default, switched by its
//! flag or by the headless configuration, refreshed while recording and
//! kept once recording stops.

mod common;

use bevy::prelude::*;
use bevy_solouder::{AudioVisualization, MixerConfig, OutputMode, SoundPlayer};
use common::{add_clip, app_with, headless_app, render, tone};

fn visual(app: &App) -> AudioVisualization {
    app.world().resource::<AudioVisualization>().clone()
}

/// One app frame, then audio, then the frame that reads it.
fn frame(app: &mut App) {
    app.update();
    render(app, 800);
    app.update();
}

#[test]
fn the_resource_follows_the_mix_while_enabled() {
    let mut app = headless_app();
    let clip = add_clip(&mut app, tone(2.0));
    app.world_mut().spawn(SoundPlayer(clip));
    frame(&mut app);
    assert!(!visual(&app).enabled);
    assert_eq!(visual(&app).wave, [0.0; 256]);
    app.world_mut().resource_mut::<AudioVisualization>().enabled = true;
    frame(&mut app);
    frame(&mut app);
    let first = visual(&app);
    assert!(first.wave.iter().any(|&s| s != 0.0));
    assert!(first.volumes[0] > 0.0 && first.volumes[1] > 0.0);
    assert_eq!(first.volumes[2..], [0.0; 6]);
    assert!(first.spectrum.iter().any(|&m| m > 0.0));
    frame(&mut app);
    assert_ne!(visual(&app).wave, first.wave, "the tone moves on");
    // Off keeps the last data.
    app.world_mut().resource_mut::<AudioVisualization>().enabled = false;
    frame(&mut app);
    let kept = visual(&app);
    frame(&mut app);
    assert_eq!(visual(&app), kept);
}

#[test]
fn the_headless_configuration_starts_recording() {
    let OutputMode::Headless(config) = OutputMode::headless() else {
        unreachable!()
    };
    let mut app = app_with(OutputMode::Headless(MixerConfig {
        visualization: true,
        ..config
    }));
    assert!(visual(&app).enabled);
    let clip = add_clip(&mut app, tone(2.0));
    app.world_mut().spawn(SoundPlayer(clip));
    frame(&mut app);
    frame(&mut app);
    assert!(visual(&app).volumes[0] > 0.0);
}
