//! Cases that need more than the mixer: streams decoded by the stream
//! worker, and 3D voices updated between renders.

use std::sync::Arc;

use solouder_core::{PlayOptions, Stream};
use solouder_decoders::WavStream;

use crate::cases::{Case, Reference, case, setup};
use crate::session::{Session, Setup};
use crate::signal::{STREAMS, TONES, fixture, tones, voice_options};

/// Streams per `streams_*` case.
pub const STREAM_VOICES: usize = 16;

/// 3D voices in the `spatial` case.
pub const SPATIAL_VOICES: usize = 256;

/// `streams_{codec}`: 16 looping voices of one stereo 44.1 kHz file held in
/// memory, decoded on the stream worker's thread. Rendered at the output's
/// pace, as behind a device.
pub fn streams() -> Vec<Case> {
    STREAMS
        .iter()
        .map(|&(codec, path)| {
            case(
                format!("streams_{codec}"),
                Reference::DecodesInMix,
                move |buffer| {
                    let mut session = Session::new(Setup {
                        streams: true,
                        ..setup(buffer, STREAM_VOICES as u32)
                    });
                    let file = WavStream::load_to_memory(fixture(path)).expect("stream fixture");
                    let mut stream = Stream::new(Arc::new(file)).expect("stream");
                    stream.set_looping(true);
                    for i in 0..STREAM_VOICES {
                        session
                            .controller
                            .play_with(&stream, voice_options(i))
                            .expect("play");
                    }
                    session
                },
            )
        })
        .collect()
}

/// Where 3D voice `i` is at step `t`: on a circle of radius 2 to 17 m
/// around the origin, moving along it at 5 m/s.
fn orbit(i: usize, t: u32) -> ([f32; 3], [f32; 3]) {
    let radius = 2.0 + (i % 16) as f32;
    let angle = i as f32 * 0.1 + t as f32 * 5.0 / radius * 0.01;
    let (sin, cos) = angle.sin_cos();
    (
        [radius * cos, 0.0, radius * sin],
        [-5.0 * sin, 0.0, 5.0 * cos],
    )
}

/// The listener at step `t`: at the origin, turning, moving along x.
fn listener(t: u32) -> [[f32; 3]; 4] {
    let (sin, cos) = (t as f32 * 0.02).sin_cos();
    [[0.0; 3], [sin, 0.0, -cos], [0.0, 1.0, 0.0], [3.0, 0.0, 0.0]]
}

/// `spatial`: 256 looping tones in 3D, all moving, with the listener
/// turning. Before each render (untimed) every voice and the listener move
/// and `update_3d_audio` runs, so each render applies a full batch of
/// gains and Doppler speeds.
pub fn spatial() -> Case {
    case("spatial", Reference::Same, |buffer| {
        let mut session = Session::new(setup(buffer, SPATIAL_VOICES as u32));
        let tones = tones();
        let options = PlayOptions {
            volume: Some(0.05),
            ..PlayOptions::default()
        };
        let mut ids = Vec::with_capacity(SPATIAL_VOICES);
        for i in 0..SPATIAL_VOICES {
            let (position, velocity) = orbit(i, 0);
            let tone = &tones[i % TONES];
            let id = session
                .controller
                .play_3d(tone, position, velocity, options);
            ids.push(id.expect("play"));
        }
        let mut step = 0;
        session.between = Some(Box::new(move |controller| {
            step += 1;
            for (i, &id) in ids.iter().enumerate() {
                let (position, velocity) = orbit(i, step);
                controller.set_3d_source_parameters(id, position, velocity);
            }
            let [position, at, up, velocity] = listener(step);
            controller.set_3d_listener_parameters(position, at, up, velocity);
            // Refused only while two batches are in flight; the next
            // render frees one.
            let _ = controller.update_3d_audio();
        }));
        session
    })
}
