//! A controller, its renderer and (for streams) its stream worker thread,
//! and the timed render loop.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{Controller, Mixer, MixerConfig, Renderer, Resampler, Status};

use crate::signal::OUTPUT_RATE;
use crate::timing::{Timing, budget, summarize, wait_until};

/// The smallest mixer buffer size (scratch space), as in `solouder-cpal`;
/// the C++ bench initializes the null driver with the same.
pub const MIX_FRAMES: u32 = 2048;

/// Work done on the game side before each render, outside the timing.
pub type Between = Box<dyn FnMut(&mut Controller)>;

/// One case, built and ready to render.
pub struct Session {
    /// Requests go here; its events are read between renders.
    pub controller: Controller,
    /// What is timed.
    pub renderer: Renderer,
    /// Run before each render, outside the timing.
    pub between: Option<Between>,
    /// Render at the output's pace instead of back to back, so the stream
    /// worker decodes ahead as it would behind a device.
    pub paced: bool,
    worker: Option<JoinHandle<()>>,
}

/// The settings a case builds its mixer with.
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    /// Frames per render.
    pub buffer: u32,
    /// `Mixer::set_max_active_voice_count`.
    pub max_active: u32,
    /// The mixer's resampler.
    pub resampler: Resampler,
    /// Start the stream worker on its own thread.
    pub streams: bool,
}

impl Session {
    /// A stereo 48 kHz mixer split for real time, with nothing playing. Its
    /// buffer size is at least [`MIX_FRAMES`], as `solouder-cpal` sets it;
    /// renders are `setup.buffer` frames long.
    pub fn new(setup: Setup) -> Self {
        let config = MixerConfig {
            sample_rate: OUTPUT_RATE,
            buffer_size: setup.buffer.max(MIX_FRAMES),
            channels: 2,
            ..MixerConfig::default()
        };
        let mut mixer = Mixer::new(config).expect("valid mixer config");
        mixer
            .set_max_active_voice_count(setup.max_active)
            .expect("valid active voice count");
        mixer.set_resampler(setup.resampler);
        let (mut controller, renderer) = split(mixer, RealtimeConfig::default());
        let worker = setup.streams.then(|| {
            let worker = controller.take_stream_worker().expect("a stream worker");
            worker.spawn().expect("stream worker thread")
        });
        Self {
            controller,
            renderer,
            between: None,
            paced: setup.streams,
            worker,
        }
    }

    /// Reads every pending event, freeing IDs and retired memory.
    pub fn collect(&mut self) -> usize {
        std::iter::from_fn(|| self.controller.poll_event()).count()
    }

    /// Renders `warmup` untimed renders, then `renders` timed ones of
    /// `frames` frames each.
    pub fn run(&mut self, frames: u32, warmup: usize, renders: usize) -> Timing {
        let mut output = vec![0.0_f32; frames as usize * 2];
        let period = budget(frames, OUTPUT_RATE);
        let mut times = Vec::with_capacity(renders);
        let start = Instant::now();
        for index in 0..warmup + renders {
            if let Some(between) = self.between.as_mut() {
                between(&mut self.controller);
            }
            self.collect();
            if self.paced {
                wait_until(start + period * index as u32);
            }
            let began = Instant::now();
            self.renderer.render(&mut output);
            let took = began.elapsed();
            std::hint::black_box(&output);
            if index >= warmup {
                times.push(took);
            }
        }
        summarize(times)
    }

    /// The renderer's counters after the last render.
    pub fn status(&self) -> Status {
        self.controller.status()
    }

    /// Stops the session and waits for its stream worker.
    pub fn finish(self) {
        let Self {
            controller,
            renderer,
            worker,
            ..
        } = self;
        drop(controller);
        drop(renderer);
        if let Some(worker) = worker {
            worker.join().expect("stream worker panicked");
        }
    }
}

/// How many renders of `frames` frames cover `seconds` of output.
pub fn renders_for(seconds: f64, frames: u32) -> usize {
    let per_second = f64::from(OUTPUT_RATE) / f64::from(frames);
    (seconds * per_second).ceil().max(1.0) as usize
}

/// Microseconds, for reports.
pub fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e6
}
