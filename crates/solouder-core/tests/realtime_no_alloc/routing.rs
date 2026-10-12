//! Routing sections of the allocation test: filters, buses, queues, filters
//! first running at another layout, and mixers split after offline use.

use std::sync::Arc;

use solouder_core::filters::{
    BassboostFilter, BiquadResonantFilter, DcRemovalFilter, EchoFilter, FftFilter, Filter,
    FlangerFilter, FreeverbFilter,
};
use solouder_core::realtime::{FilterTarget, RealtimeConfig, split};
use solouder_core::{Bus, Controller, Mixer, MixerConfig, PlayOptions, Queue, Renderer, Resampler};

use super::{drain, measured_render, sound};

/// Filters, buses and queues. Every filter whose instance allocates when it
/// first runs is used, and the game side drops its filters and sources
/// before rendering, so the renderer would free them if it dropped what it
/// replaces.
pub(super) fn routing(controller: &mut Controller, renderer: &mut Renderer, out: &mut [f32]) {
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
pub(super) fn first_use(controller: &mut Controller, renderer: &mut Renderer, out: &mut [f32]) {
    let mut bus = Bus::new();
    let fast = PlayOptions {
        speed: 2.0,
        ..PlayOptions::default()
    };
    let bus_id = controller.play_with(&bus, fast).unwrap();
    controller
        .bus_play(&bus, &sound(2, 4000, 44100.0), PlayOptions::default())
        .unwrap();
    measured_render(renderer, &mut out[..2 * 256], "starting a fast bus");
    bus.set_channels(1).unwrap();
    // A replay through a bus that is not playing fails, leaving the fast
    // stereo voice playing; a speed oscillation from 3 to 1 stays below 3.
    controller
        .bus_play(&Bus::new(), &bus, PlayOptions::default())
        .unwrap();
    controller
        .oscillate_relative_play_speed(bus_id, 3.0, 1.0, 1.0)
        .unwrap();
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
    // The failed replay has reported; the playing voice's layout still counts.
    drain(controller);
    controller
        .set_bus_filter(&mut bus, 4, Some(Arc::new(EchoFilter::default())))
        .unwrap();
    measured_render(
        renderer,
        &mut out[..2 * 512],
        "a bus filter after a failed replay",
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

/// A mixer split with filters it was given offline, on voices that have not
/// played yet (one paused, one oscillating its speed) and globally.
pub(super) fn split_after_offline_use(out: &mut [f32]) {
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.set_global_filter(0, Some(&EchoFilter::default()));
    mixer.set_global_filter(1, Some(&DcRemovalFilter::default()));
    let mut filtered = sound(2, 6000, 44100.0);
    filtered.set_filter(0, Some(Arc::new(EchoFilter::default())));
    filtered.set_filter(1, Some(Arc::new(FlangerFilter::default())));
    filtered.set_filter(2, Some(Arc::new(DcRemovalFilter::default())));
    let paused = PlayOptions {
        paused: true,
        ..PlayOptions::default()
    };
    mixer.play_with(&filtered, paused);
    let swinging = mixer.play(&filtered);
    mixer.oscillate_relative_play_speed(swinging, 3.0, 1.0, 0.5);
    drop(filtered);
    let (mut controller, mut renderer) = split(mixer, RealtimeConfig::default());
    for _ in 0..4 {
        measured_render(
            &mut renderer,
            &mut out[..2 * 512],
            "filters the mixer had before the split",
        );
    }
    controller.stop_all().unwrap();
    measured_render(&mut renderer, &mut out[..2 * 64], "stopping them");
    assert_eq!(controller.status().retired_overflow, 0);
}
