//! Visualization section of the allocation test: the renderer publishes the
//! mix's data and its watched buses' after every render.

use solouder_core::{Bus, Controller, PlayOptions, Renderer};

use super::{drain, measured_render, sound};

/// Records the mix and two buses, one enabled before it plays and one while
/// it plays, across long, short and empty renders, then stops a bus.
pub(super) fn visualization(controller: &mut Controller, renderer: &mut Renderer, out: &mut [f32]) {
    controller.set_visualization(true).unwrap();
    let mut first = Bus::new();
    first.set_visualization(true);
    let first_voice = controller.play(&first).unwrap();
    let mut second = Bus::new();
    second.set_channels(4).unwrap();
    controller.play(&second).unwrap();
    let looping = PlayOptions::default();
    for bus in [&first, &second] {
        let mut source = sound(2, 3000, 44100.0);
        source.set_looping(true);
        controller.bus_play(bus, &source, looping).unwrap();
    }
    measured_render(renderer, &mut out[..2 * 512], "visualized buses start");
    controller.set_bus_visualization(&mut second, true).unwrap();
    for frames in [1, 100, 256, 1024, 0, 4096] {
        measured_render(renderer, &mut out[..2 * frames], "visualized renders");
    }
    assert!(controller.wave().iter().any(|&s| s != 0.0));
    assert!(controller.approximate_volume(1) > 0.0);
    assert!(controller.bus_wave(&first).is_some());
    assert!(controller.bus_approximate_volume(&second, 3) > 0.0);
    controller.stop(first_voice).unwrap();
    measured_render(renderer, &mut out[..2 * 64], "a visualized bus stops");
    assert_eq!(controller.bus_fft(&first), None);
    controller.set_visualization(false).unwrap();
    controller.stop_all().unwrap();
    measured_render(renderer, &mut out[..2 * 64], "visualization off");
    drain(controller);
}
