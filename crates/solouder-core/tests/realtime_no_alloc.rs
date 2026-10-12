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
use std::sync::Arc;

use solouder_core::filters::{
    BassboostFilter, BiquadResonantFilter, DcRemovalFilter, EchoFilter, FftFilter, Filter,
    FlangerFilter, FreeverbFilter,
};
use solouder_core::realtime::{FilterTarget, RealtimeConfig, split};
use solouder_core::{
    Bus, Controller, ControllerError, Mixer, MixerConfig, Pcm, PlayOptions, Queue, Renderer,
    Resampler, Sound,
};
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

/// Filters, buses and queues. Every filter whose instance allocates when it
/// first runs is used, and the game side drops its filters and sources
/// before rendering, so the renderer would free them if it dropped what it
/// replaces.
fn routing(controller: &mut Controller, renderer: &mut Renderer, out: &mut [f32]) {
    let filters: Vec<Arc<dyn Filter>> = vec![
        Arc::new(EchoFilter::default()),
        Arc::new(DcRemovalFilter::default()),
        Arc::new(FftFilter),
        Arc::new(BassboostFilter::default()),
        Arc::new(FlangerFilter::default()),
        Arc::new(FreeverbFilter::default()),
        Arc::new(BiquadResonantFilter::default()),
    ];
    let mut bus = Bus::new();
    bus.set_filter(0, Some(Arc::clone(&filters[0])));
    bus.set_filter(1, Some(Arc::clone(&filters[4])));
    let bus_id = controller.play(&bus).unwrap();
    let mut children = Vec::new();
    for (i, filter) in filters.iter().enumerate() {
        let mut filtered = sound(2, 3000, 22050.0);
        filtered.set_filter(i as u32 % 8, Some(Arc::clone(filter)));
        let options = PlayOptions {
            speed: 1.25,
            ..PlayOptions::default()
        };
        children.push(controller.bus_play(&bus, &filtered, options).unwrap());
    }
    controller.set_global_filter(0, Some(&*filters[1])).unwrap();
    controller.set_global_filter(1, Some(&*filters[2])).unwrap();
    let mut queue = Queue::new();
    queue.set_params(44100.0, 2).unwrap();
    let queue_id = controller.play(&queue).unwrap();
    for _ in 0..34 {
        controller
            .queue_play(&queue, &sound(2, 300, 44100.0))
            .unwrap();
    }
    controller
        .play_clocked(0.5, &sound(1, 2000, 44100.0), PlayOptions::default())
        .unwrap();
    controller
        .play_background(&sound(2, 2000, 44100.0), Some(0.5), false)
        .unwrap();
    measured_render(
        renderer,
        &mut out[..2 * 512],
        "starting filtered voices, a bus and a queue",
    );

    let child = children[0];
    controller.fade_volume(child, 0.2, 0.01).unwrap();
    controller.oscillate_pan(child, -1.0, 1.0, 0.01).unwrap();
    controller
        .set_filter_parameter(FilterTarget::Voice(child), 0, 0, 0.5)
        .unwrap();
    controller
        .fade_filter_parameter(FilterTarget::Global, 0, 0, 0.3, 0.01)
        .unwrap();
    controller.seek(children[1], 0.02).unwrap();
    controller.seek(queue_id, 0.001).unwrap();
    controller
        .set_bus_filter(&mut bus, 0, Some(Arc::clone(&filters[5])))
        .unwrap();
    controller.set_bus_filter(&mut bus, 1, None).unwrap();
    controller
        .set_bus_resampler(&mut bus, Resampler::Point)
        .unwrap();
    controller.set_bus_visualization(&mut bus, true).unwrap();
    controller.set_global_filter(0, None).unwrap();
    controller.set_global_filter(1, Some(&*filters[3])).unwrap();
    drop(filters);
    measured_render(
        renderer,
        &mut out[..2 * 700],
        "controls, filter swaps and seeks",
    );
    for frames in [1, 333, 4096, 4096] {
        measured_render(
            renderer,
            &mut out[..2 * frames],
            "queued sounds playing out",
        );
    }

    controller.stop(bus_id).unwrap();
    controller.stop_sound(&queue).unwrap();
    drop(bus);
    drop(queue);
    measured_render(renderer, &mut out[..2 * 64], "stopping a bus and a queue");
    controller.stop_all().unwrap();
    for _ in 0..4 {
        measured_render(renderer, &mut out[..2 * 64], "retiring the rest");
    }
    drain(controller);
    assert_eq!(controller.status().retired_overflow, 0);
    assert_eq!(controller.status().rejected_queue_sounds, 2);
}

/// Filter instances first running at another layout than the one they were
/// created for: a bus whose channel setting changed after it started, a bus
/// at double speed, and delays changed before a filter's first block.
fn first_use(controller: &mut Controller, renderer: &mut Renderer, out: &mut [f32]) {
    let mut bus = Bus::new();
    let fast = PlayOptions {
        speed: 2.0,
        ..PlayOptions::default()
    };
    controller.play_with(&bus, fast).unwrap();
    controller
        .bus_play(&bus, &sound(2, 4000, 44100.0), PlayOptions::default())
        .unwrap();
    measured_render(renderer, &mut out[..2 * 256], "starting a fast bus");
    bus.set_channels(1).unwrap();
    let bus_filters: [Arc<dyn Filter>; 4] = [
        Arc::new(EchoFilter::default()),
        Arc::new(DcRemovalFilter::default()),
        Arc::new(FlangerFilter::default()),
        Arc::new(FftFilter),
    ];
    for (slot, filter) in bus_filters.into_iter().enumerate() {
        controller
            .set_bus_filter(&mut bus, slot as u32, Some(filter))
            .unwrap();
    }
    controller
        .set_global_filter(0, Some(&EchoFilter::default()))
        .unwrap();
    controller
        .set_filter_parameter(FilterTarget::Global, 0, EchoFilter::DELAY, 0.1)
        .unwrap();
    controller
        .set_global_filter(1, Some(&FlangerFilter::default()))
        .unwrap();
    controller
        .set_filter_parameter(FilterTarget::Global, 1, FlangerFilter::DELAY, 0.001)
        .unwrap();
    let mut echoed = sound(2, 3000, 44100.0);
    echoed.set_filter(0, Some(Arc::new(EchoFilter::default())));
    let voice = controller.play(&echoed).unwrap();
    drop(echoed);
    controller
        .set_filter_parameter(FilterTarget::Voice(voice), 0, EchoFilter::DELAY, 0.05)
        .unwrap();
    measured_render(
        renderer,
        &mut out[..2 * 512],
        "filters first running at another layout or delay",
    );
    controller
        .set_filter_parameter(FilterTarget::Global, 1, FlangerFilter::DELAY, 0.1)
        .unwrap();
    measured_render(
        renderer,
        &mut out[..2 * 512],
        "a flanger delay growing to its maximum",
    );

    controller.stop_all().unwrap();
    controller.set_global_filter(0, None).unwrap();
    controller.set_global_filter(1, None).unwrap();
    drop(bus);
    for _ in 0..3 {
        measured_render(renderer, &mut out[..2 * 64], "retiring first-use filters");
    }
    drain(controller);
    assert_eq!(controller.status().retired_overflow, 0);
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

    routing(&mut controller, &mut renderer, out);
    first_use(&mut controller, &mut renderer, out);

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
