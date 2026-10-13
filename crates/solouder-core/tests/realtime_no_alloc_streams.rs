//! `Renderer::render` must not allocate or free memory for streaming voices
//! either: waiting for the worker, playing, wrapping loops, seeking, running
//! dry, ending, and failing to open. The worker runs between renders,
//! outside the measured regions.
//!
//! The counting allocator is global, so this file holds a single test.

use std::alloc::System;
use std::sync::Arc;

use solouder_core::realtime::{RealtimeConfig, split};
use solouder_core::{
    Controller, Mixer, MixerConfig, Renderer, Stream, StreamFormat, StreamReader, StreamSource,
    StreamWorker,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[derive(Debug)]
struct Memory {
    channels: u32,
    frames: usize,
    rate: f32,
    missing: bool,
}

#[derive(Debug)]
struct MemoryReader {
    channels: usize,
    frames: usize,
    at: usize,
}

impl StreamSource for Memory {
    fn format(&self) -> StreamFormat {
        StreamFormat {
            channels: self.channels,
            sample_rate: self.rate,
            frames: self.frames as u32,
        }
    }

    fn open(&self) -> Option<Box<dyn StreamReader>> {
        let reader = MemoryReader {
            channels: self.channels as usize,
            frames: self.frames,
            at: 0,
        };
        (!self.missing).then(|| Box::new(reader) as Box<dyn StreamReader>)
    }
}

impl StreamReader for MemoryReader {
    fn read(&mut self, out: &mut [f32], stride: usize, frames: usize) -> usize {
        let count = frames.min(self.frames - self.at);
        for c in 0..self.channels {
            for (i, sample) in out[c * stride..][..count].iter_mut().enumerate() {
                *sample = ((self.at + i + c) % 89) as f32 / 89.0 - 0.5;
            }
        }
        self.at += count;
        count
    }

    fn rewind(&mut self) -> bool {
        self.at = 0;
        true
    }
}

fn stream(channels: u32, frames: usize, rate: f32, missing: bool) -> Stream {
    let source = Memory {
        channels,
        frames,
        rate,
        missing,
    };
    Stream::new(Arc::new(source)).unwrap()
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

fn between(controller: &mut Controller, worker: &mut StreamWorker) {
    while controller.poll_event().is_some() {}
    controller.release_retired();
    while worker.service() {}
}

#[test]
fn rendering_streams_never_allocates_or_frees() {
    let config = RealtimeConfig {
        retired_capacity: 2,
        stream_chunks: 4,
        ..RealtimeConfig::default()
    };
    let (mut controller, mut renderer) = split(Mixer::new(MixerConfig::default()).unwrap(), config);
    let mut worker = controller.take_stream_worker().unwrap();
    let mut output = vec![0.0; 2 * 1500];

    let mut looped = stream(2, 3000, 48000.0, false);
    looped.set_looping(true);
    looped.set_loop_point(0.01);
    let looping = controller.play(&looped).unwrap();
    measured_render(&mut renderer, &mut output, "waiting for the worker");
    for _ in 0..3 {
        controller.play(&stream(6, 1200, 22050.0, false)).unwrap();
        controller.play(&stream(1, 100, 44100.0, true)).unwrap();
    }
    drop(looped);
    for round in 0..12 {
        between(&mut controller, &mut worker);
        if round == 4 {
            controller.seek(looping, 0.03).unwrap();
        }
        let frames = [1500, 700, 333][round % 3];
        measured_render(&mut renderer, &mut output[..2 * frames], "streaming");
    }
    // Running dry: no service between renders.
    for _ in 0..4 {
        measured_render(&mut renderer, &mut output, "underflow");
    }
    assert!(controller.status().stream_underflow_frames > 0);
    controller.stop(looping).unwrap();
    measured_render(&mut renderer, &mut output, "stopping");
    between(&mut controller, &mut worker);
    assert_eq!(worker.streams(), 0);
}
