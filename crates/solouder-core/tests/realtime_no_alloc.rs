//! `Renderer::render` must not allocate or free memory: it runs in the audio
//! callback. Every render below is measured on its own; commands are queued
//! and events read between renders, outside the measured regions.
//!
//! Every play uses its own buffer, dropped by the test before rendering, so
//! the renderer holds the last reference to the audio it retires: freeing it
//! there would show up as a deallocation. The retirement queue is tiny and
//! collected late, so retired audio piles up on the audio thread.
//!
//! The counting allocator is global, so this file holds a single test.

use std::alloc::System;

use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{
    Controller, ControllerError, Mixer, MixerConfig, Pcm, PlayOptions, Renderer, Resampler, Sound,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[path = "realtime_no_alloc/routing.rs"]
mod routing;
#[path = "realtime_no_alloc/spatial.rs"]
mod spatial;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn sound(channels: u32, frames: usize, rate: f32) -> Sound {
    let samples = (0..frames * channels as usize)
        .map(|i| ((i * 7) % 101) as f32 / 101.0 - 0.5)
        .collect();
    Sound::new(Pcm::from_planar(samples, channels, rate).unwrap())
}

fn measured_render(renderer: &mut Renderer, output: &mut [f32], step: &str) {
    let region = Region::new(GLOBAL);
    renderer.render(output);
    let stats = region.change();
    assert_eq!(
        (stats.allocations, stats.deallocations, stats.reallocations),
        (0, 0, 0),
        "render allocated or freed memory during: {step}"
    );
}

fn drain(controller: &mut Controller) -> usize {
    std::iter::from_fn(|| controller.poll_event()).count()
}

/// Plays a fresh buffer, rendering (measured) whenever the controller asks
/// to wait. The caller's copy of the sound is dropped on return.
fn play_fresh(
    controller: &mut Controller,
    renderer: &mut Renderer,
    output: &mut [f32],
    shape: (u32, usize, f32),
    options: PlayOptions,
) -> solouder_core::VoiceId {
    let source = sound(shape.0, shape.1, shape.2);
    loop {
        match controller.play_with(&source, options) {
            Ok(id) => return id,
            Err(ControllerError::QueueFull | ControllerError::NoFreeVoiceId) => {
                measured_render(renderer, &mut output[..2 * 64], "waiting for room");
                drain(controller);
            }
            Err(other) => panic!("{other:?}"),
        }
    }
}

#[test]
fn rendering_never_allocates_or_frees() {
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.set_max_active_voice_count(32).unwrap();
    let config = RealtimeConfig {
        voice_ids: 1200,
        retired_capacity: 2,
        ..RealtimeConfig::default()
    };
    let (mut controller, mut renderer) = split(mixer, config);
    let shapes = [(1, 300, 22050.0), (2, 5000, 48000.0), (6, 2000, 44100.0)];
    let mut output = vec![0.0f32; 2 * 4096];
    let out = &mut output;

    let mut ids = Vec::new();
    for i in 0..100 {
        let options = PlayOptions {
            volume: Some(0.01 * i as f32),
            pan: (i as f32 / 50.0) - 1.0,
            paused: i % 10 == 0,
            speed: 0.5 + (i % 4) as f32 * 0.25,
            protect: i % 7 == 0,
            ..PlayOptions::default()
        };
        let shape = shapes[i % 3];
        ids.push(play_fresh(
            &mut controller,
            &mut renderer,
            out,
            shape,
            options,
        ));
    }
    measured_render(&mut renderer, &mut out[..2 * 512], "starting 100 voices");

    for (i, &id) in ids.iter().enumerate() {
        match i % 7 {
            0 => controller.set_pause(id, false).unwrap(),
            1 => controller.set_relative_play_speed(id, 1.7).unwrap(),
            2 => controller.set_looping(id, true).unwrap(),
            3 => controller.set_volume(id, 0.9).unwrap(),
            4 => controller.set_pan(id, -0.5).unwrap(),
            5 => controller.set_protect(id, true).unwrap(),
            _ => controller.stop(id).unwrap(),
        }
    }
    controller.set_post_clip_scaler(0.8).unwrap();
    controller.set_resampler(Resampler::CatmullRom).unwrap();
    measured_render(
        &mut renderer,
        &mut out[..2 * 333],
        "voice and global controls",
    );
    for frames in [1, 7, 512, 1000, 4096] {
        measured_render(&mut renderer, &mut out[..2 * frames], "voices ending");
    }
    // Audio is collected late, and only through events.
    drain(&mut controller);

    // More plays than voice slots: the oldest unprotected voices are stolen.
    for _ in 0..1100 {
        let looping = PlayOptions::default();
        play_fresh(&mut controller, &mut renderer, out, shapes[1], looping);
    }
    for _ in 0..4 {
        measured_render(&mut renderer, &mut out[..2 * 64], "stealing voices");
    }
    drain(&mut controller);

    // Every slot protected: the next plays fail.
    controller.stop_all().unwrap();
    measured_render(&mut renderer, &mut out[..2 * 64], "stop all");
    drain(&mut controller);
    let protected = PlayOptions {
        protect: true,
        paused: true,
        ..PlayOptions::default()
    };
    for _ in 0..solouder_core::VOICE_COUNT {
        play_fresh(&mut controller, &mut renderer, out, shapes[0], protected);
    }
    for _ in 0..3 {
        play_fresh(
            &mut controller,
            &mut renderer,
            out,
            shapes[0],
            PlayOptions::default(),
        );
    }
    for _ in 0..4 {
        measured_render(
            &mut renderer,
            &mut out[..2 * 64],
            "plays that find no voice",
        );
    }
    measured_render(&mut renderer, &mut [], "an empty render");
    assert!(drain(&mut controller) > 0);
    controller.stop_all().unwrap();
    measured_render(&mut renderer, &mut out[..2 * 64], "stop all protected");
    assert_eq!(controller.status().retired_overflow, 0);
    drain(&mut controller);

    routing::routing(&mut controller, &mut renderer, out);
    routing::first_use(&mut controller, &mut renderer, out);
    routing::split_after_offline_use(out);
    spatial::spatial(&mut controller, &mut renderer, out);

    // The game side goes away first; the audio thread keeps rendering.
    drop(controller);
    for _ in 0..4 {
        measured_render(
            &mut renderer,
            &mut out[..2 * 256],
            "after the controller is dropped",
        );
    }
}
