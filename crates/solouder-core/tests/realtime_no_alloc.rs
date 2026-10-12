//! `Renderer::render` must not allocate or free memory: it runs in the audio
//! callback. Every render below is measured on its own; commands are queued
//! and events read between renders, outside the measured regions.
//!
//! The counting allocator is global, so this file holds a single test.

use std::alloc::System;

use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{Controller, Mixer, MixerConfig, Pcm, PlayOptions, Renderer, Sound};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

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

#[test]
fn rendering_never_allocates_or_frees() {
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.set_max_active_voice_count(32).unwrap();
    let (mut controller, mut renderer) = split(mixer, RealtimeConfig::default());
    let short = sound(1, 300, 22050.0);
    let stereo = sound(2, 5000, 48000.0);
    let surround = sound(6, 2000, 44100.0);
    let mut output = vec![0.0f32; 2 * 4096];

    let mut ids = Vec::new();
    for i in 0..100 {
        let source = [&short, &stereo, &surround][i % 3];
        let options = PlayOptions {
            volume: Some(0.01 * i as f32),
            pan: (i as f32 / 50.0) - 1.0,
            paused: i % 10 == 0,
        };
        ids.push(controller.play_with(source, options).unwrap());
    }
    measured_render(&mut renderer, &mut output[..2 * 512], "starting 100 voices");
    assert!(drain(&mut controller) >= 100);

    for (i, &id) in ids.iter().enumerate() {
        match i % 5 {
            0 => controller.set_pause(id, false).unwrap(),
            1 => controller.set_relative_play_speed(id, 1.7).unwrap(),
            2 => controller.set_looping(id, true).unwrap(),
            3 => controller.set_volume(id, 0.9).unwrap(),
            _ => controller.stop(id).unwrap(),
        }
    }
    measured_render(&mut renderer, &mut output[..2 * 333], "voice controls");
    for frames in [1, 7, 512, 1000, 4096] {
        measured_render(&mut renderer, &mut output[..2 * frames], "voices ending");
    }
    drain(&mut controller);
    controller.release_retired_audio();

    // More plays than voices: the oldest voices are stolen.
    for _ in 0..1100 {
        if controller.queue_space() == 0 {
            measured_render(&mut renderer, &mut output[..2 * 64], "stealing voices");
            drain(&mut controller);
            controller.release_retired_audio();
        }
        controller.play(&stereo).unwrap();
    }
    for _ in 0..4 {
        measured_render(&mut renderer, &mut output[..2 * 64], "stealing voices");
        drain(&mut controller);
        controller.release_retired_audio();
    }

    controller.set_global_volume(0.5).unwrap();
    controller.stop_all().unwrap();
    measured_render(&mut renderer, &mut output[..2 * 256], "stop all");
    measured_render(&mut renderer, &mut [], "an empty render");
    assert!(drain(&mut controller) > 0);
    assert_eq!(controller.status().retired_overflow, 0);
    assert_eq!(controller.voice_ids_in_use(), 0);
}
