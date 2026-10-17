//! 3D sections of the allocation test: 3D plays, batched updates, kills,
//! buses and clocked plays, and Doppler raising the rate filters run at.

use std::sync::Arc;

use solouder_core::filters::{DcRemovalFilter, EchoFilter, Filter, FlangerFilter};
use solouder_core::spatial::{Attenuation, Collider, SpatialVoice};
use solouder_core::{Bus, Controller, PlayOptions, Renderer};

use super::{drain, measured_render, sound};

/// Allocates on every call, so a call on the audio thread would show up.
#[derive(Debug)]
struct Allocating;

impl Collider for Allocating {
    fn collide(&self, voice: &SpatialVoice, _user_data: i32) -> f32 {
        let scratch: Vec<f32> = voice.position.to_vec();
        if scratch[1] > 50.0 { 0.5 } else { 1.0 }
    }
}

const STILL: [f32; 3] = [0.0; 3];

/// Filtered 3D voices at another rate than the output's, at 1.25 times
/// speed and approaching (a Doppler factor of 1.78 at the start), so their
/// filters first run well above the source's rate; later updates keep the
/// Doppler factor at or below its start. A receding voice starts below 1.
pub(super) fn spatial(controller: &mut Controller, renderer: &mut Renderer, out: &mut [f32]) {
    let filters: [Arc<dyn Filter>; 3] = [
        Arc::new(EchoFilter::default()),
        Arc::new(DcRemovalFilter::default()),
        Arc::new(FlangerFilter::default()),
    ];
    let collider: Arc<dyn Collider> = Arc::new(Allocating);
    let fast = PlayOptions {
        speed: 1.25,
        ..PlayOptions::default()
    };
    let mut approaching = Vec::new();
    for (i, filter) in filters.iter().enumerate() {
        let mut filtered = sound(2, 6000, 22050.0);
        filtered.set_filter(i as u32, Some(Arc::clone(filter)));
        filtered.set_3d_collider(Some(Arc::clone(&collider)), 0);
        filtered.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
        let at = [i as f32, 0.0, -20.0];
        let id = controller.play_3d(&filtered, at, [0.0, 0.0, 150.0], fast);
        approaching.push(id.unwrap());
    }
    let mut receding = sound(1, 6000, 48000.0);
    receding.set_filter(0, Some(Arc::clone(&filters[2])));
    let away = [0.0, 0.0, -150.0];
    let receding_id = controller.play_3d(&receding, [3.0, 0.0, -5.0], away, fast);
    let receding_id = receding_id.unwrap();

    // A filtered 3D bus with a 3D voice in it, and clocked plays.
    let mut bus = Bus::new();
    bus.set_filter(0, Some(Arc::clone(&filters[2])));
    let bus_id = controller
        .play_3d(&bus, [0.0, 0.0, -3.0], [0.0, 0.0, 100.0], fast)
        .unwrap();
    controller
        .bus_play_3d(
            &bus,
            &sound(1, 3000, 44100.0),
            [1.0, 0.0, -2.0],
            STILL,
            fast,
        )
        .unwrap();
    let mut delayed = sound(2, 3000, 32000.0);
    delayed.set_3d_distance_delay(true);
    controller
        .play_3d_clocked(0.25, &delayed, [0.0, 0.0, -30.0], STILL, fast)
        .unwrap();
    controller
        .bus_play_3d_clocked(&bus, 0.3, &delayed, [2.0, 0.0, -10.0], STILL, fast)
        .unwrap();
    // Too quiet to start: killed as it is placed.
    let mut quiet = sound(1, 3000, 44100.0);
    quiet.set_3d_attenuation(Attenuation::InverseDistance, 1.0);
    quiet.set_inaudible_behavior(false, true);
    controller
        .play_3d(&quiet, [0.0, 0.0, -5000.0], STILL, PlayOptions::default())
        .unwrap();
    drop((filters, collider, receding, bus, delayed));
    measured_render(renderer, &mut out[..2 * 512], "starting 3D voices");

    // Moving the listener and voices; Doppler at or below its start.
    let velocities = [100.0, 150.0, 0.0, 120.0];
    for (step, &speed) in velocities.iter().enumerate() {
        let offset = step as f32;
        controller.set_3d_listener_parameters(
            [offset, 0.0, 0.0],
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, -0.0],
        );
        for (i, &id) in approaching.iter().enumerate() {
            let at = [i as f32, 0.0, -20.0 + offset];
            controller.set_3d_source_parameters(id, at, [0.0, 0.0, speed]);
        }
        controller.set_3d_source_velocity(receding_id, [0.0, 0.0, -speed]);
        controller.set_3d_source_position(bus_id, [0.0, offset * 30.0, -3.0]);
        controller.update_3d_audio().unwrap();
        measured_render(renderer, &mut out[..2 * 700], "3D updates");
        drain(controller);
    }

    // An update that kills: an inaudible voice with a killing behavior.
    let id = approaching[0];
    controller.set_inaudible_behavior(id, false, true).unwrap();
    controller.set_3d_source_position(id, [0.0, 0.0, -1.0e7]);
    controller.update_3d_audio().unwrap();
    measured_render(renderer, &mut out[..2 * 256], "an update that kills");
    for frames in [4096, 4096, 4096] {
        measured_render(renderer, &mut out[..2 * frames], "3D voices playing out");
    }
    controller.stop_all().unwrap();
    for _ in 0..4 {
        measured_render(renderer, &mut out[..2 * 64], "retiring 3D voices");
    }
    drain(controller);
    assert_eq!(controller.status().retired_overflow, 0);
}
