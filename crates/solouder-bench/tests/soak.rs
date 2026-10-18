//! Seeded soak test: a game thread drives the `Controller` with random
//! requests (plays of sounds, filtered sounds, streams of every codec,
//! generated sources with the `sources` feature, bus bursts and 3D voices; stops, fades, pauses, seeks, speed changes,
//! scheduled stops, 3D updates), an audio thread renders random buffer sizes
//! through the `Renderer`, and the `StreamWorker` decodes on its own thread.
//!
//! Checks: no thread panics; the audio thread never allocates or frees while
//! rendering; every sample is finite and within [-1, 1]; every play reports
//! `Started` or `Failed` and a started voice exactly one `Finished`; at the
//! end every voice ID is released and nothing overflowed the retirement
//! queue. Every half second a probe checks the active-voice behavior behind
//! upstream #351, #163 and #274 (see `game.rs`).
//!
//! Runs for 3 seconds by default. `SOLOUDER_SOAK_SECONDS` sets the length
//! and `SOLOUDER_SOAK_SEED` the seed (both printed). The audio thread renders
//! at four times real time, so the stream worker can fall behind: underflow
//! is reported, not an error.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use solouder_bench::signal::OUTPUT_RATE;
use solouder_bench::timing::budget;
use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{Mixer, MixerConfig, Renderer};

#[path = "soak/game.rs"]
mod game;
#[path = "soak/sources.rs"]
mod sources;
#[path = "soak/tracker.rs"]
mod tracker;

#[global_allocator]
static GLOBAL: solouder_count_alloc::Counting = solouder_count_alloc::Counting;

/// What the audio thread publishes.
#[derive(Debug, Default)]
pub struct Audio {
    stop: AtomicBool,
    /// Largest absolute sample since the game last cleared it (`f32` bits;
    /// non-negative floats order as their bits do).
    peak: AtomicU32,
    /// Samples that were not finite or outside [-1, 1].
    bad: AtomicU64,
}

const BUFFERS: [usize; 5] = [128, 256, 480, 512, 1024];

/// Renders until told to stop, at four times real time.
fn audio_thread(mut renderer: Renderer, audio: Arc<Audio>, seed: u64) -> Renderer {
    let mut rng = sources::Rng::new(seed ^ 0xA0D1_0000);
    let mut output = vec![0.0_f32; 1024 * 2];
    while !audio.stop.load(Ordering::Acquire) {
        let frames = BUFFERS[rng.below(BUFFERS.len() as u64) as usize];
        let output = &mut output[..frames * 2];
        solouder_count_alloc::rendering(|| renderer.render(output));
        let mut peak = 0.0_f32;
        let mut bad = 0;
        for &sample in output.iter() {
            if !sample.is_finite() || sample.abs() > 1.0 {
                bad += 1;
            } else {
                peak = peak.max(sample.abs());
            }
        }
        audio.bad.fetch_add(bad, Ordering::Relaxed);
        audio.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
        std::thread::sleep(budget(frames as u32, OUTPUT_RATE) / 4);
    }
    renderer
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
fn controller_renderer_and_stream_worker_soak() {
    let seconds: f64 = env_or("SOLOUDER_SOAK_SECONDS", 3.0);
    let seed: u64 = env_or("SOLOUDER_SOAK_SEED", 0x50AC);
    eprintln!("soak: {seconds} s, seed {seed}");

    let mut mixer = Mixer::new(MixerConfig {
        sample_rate: OUTPUT_RATE,
        ..MixerConfig::default()
    })
    .unwrap();
    mixer
        .set_max_active_voice_count(game::ACTIVE_LIMIT)
        .unwrap();
    // More IDs than voice slots, so floods steal voices; a small command
    // queue and retirement queue, so requests are refused under load.
    let config = RealtimeConfig {
        voice_ids: 1500,
        command_capacity: 1024,
        retired_capacity: 256,
        ..RealtimeConfig::default()
    };
    let (mut controller, renderer) = split(mixer, config);
    let worker = controller.take_stream_worker().unwrap().spawn().unwrap();
    let audio = Arc::new(Audio::default());
    let audio_handle = {
        let audio = Arc::clone(&audio);
        std::thread::Builder::new()
            .name("soak-audio".to_owned())
            .spawn(move || audio_thread(renderer, audio, seed))
            .unwrap()
    };

    let mut game = game::Game::new(controller, seed, Arc::clone(&audio));
    let started = Instant::now();
    game.run(started + Duration::from_secs_f64(seconds));
    let status = game.controller.status();
    let in_use = game.controller.voice_ids_in_use();

    audio.stop.store(true, Ordering::Release);
    let renderer = audio_handle.join().expect("the audio thread panicked");
    let audio_allocations = solouder_count_alloc::audio_allocations();
    drop(renderer);
    let game::Game {
        controller,
        tracker,
        report,
        ..
    } = game;
    drop(controller);
    worker.join().expect("the stream worker panicked");

    eprintln!(
        "soak: {} renders, {:.1} s of audio in {:.1} s; {} frames, {} probes, {} bus bursts, \
         {} refused requests; plays {}, started {}, failed {}, finished (ended, stopped, stolen) \
         {:?}; probe bursts seen unstarted for at most {} renders; stream underflow {} frames",
        status.renders,
        status.stream_time,
        started.elapsed().as_secs_f64(),
        report.frames,
        report.probes,
        report.bursts,
        report.refused,
        tracker.plays,
        tracker.started,
        tracker.failed,
        tracker.finished,
        report.max_start_renders,
        status.stream_underflow_frames,
    );
    assert_eq!(audio_allocations, 0, "the audio thread allocated or freed");
    assert_eq!(
        audio.bad.load(Ordering::Relaxed),
        0,
        "samples not finite or outside [-1, 1]"
    );
    assert_eq!(tracker.live(), 0);
    assert_eq!(tracker.finals(), tracker.plays, "one final event per play");
    assert_eq!(in_use, 0, "every voice ID released");
    assert_eq!(status.retired_overflow, 0, "retirement overflow");
    assert!(
        report.probes > 0 && tracker.plays > 100,
        "the soak did too little"
    );
}
