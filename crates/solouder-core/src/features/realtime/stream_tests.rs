//! Streams through the controller: decoded on the worker, rendered as the
//! offline mixer renders them.

use std::sync::Arc;

use super::*;
use crate::{Mixer, MixerConfig, Stream, StreamFormat, StreamReader, StreamSource};

/// Planar samples in memory, read like a file.
#[derive(Debug)]
pub(super) struct Memory {
    pub planes: Arc<Vec<Vec<f32>>>,
    pub rate: f32,
    pub missing: bool,
}

#[derive(Debug)]
struct MemoryReader {
    planes: Arc<Vec<Vec<f32>>>,
    at: usize,
}

impl StreamSource for Memory {
    fn format(&self) -> StreamFormat {
        StreamFormat {
            channels: self.planes.len() as u32,
            sample_rate: self.rate,
            frames: self.planes[0].len() as u32,
        }
    }

    fn open(&self) -> Option<Box<dyn StreamReader>> {
        let reader = MemoryReader {
            planes: Arc::clone(&self.planes),
            at: 0,
        };
        (!self.missing).then(|| Box::new(reader) as Box<dyn StreamReader>)
    }
}

impl StreamReader for MemoryReader {
    fn read(&mut self, out: &mut [f32], stride: usize, frames: usize) -> usize {
        let count = frames.min(self.planes[0].len() - self.at);
        for (c, plane) in self.planes.iter().enumerate() {
            out[c * stride..][..count].copy_from_slice(&plane[self.at..self.at + count]);
        }
        self.at += count;
        count
    }

    fn rewind(&mut self) -> bool {
        self.at = 0;
        true
    }
}

pub(super) fn stream(frames: usize, rate: f32) -> Stream {
    let ramp = |c: usize| {
        (0..frames)
            .map(|i| ((i * (c + 3)) % 97) as f32 / 97.0 - 0.5)
            .collect()
    };
    let source = Memory {
        planes: Arc::new(vec![ramp(0), ramp(1)]),
        rate,
        missing: false,
    };
    Stream::new(Arc::new(source)).unwrap()
}

pub(super) fn runtime(config: RealtimeConfig) -> (Controller, Renderer, StreamWorker) {
    let (mut controller, renderer) = split(Mixer::new(MixerConfig::default()).unwrap(), config);
    let worker = controller.take_stream_worker().unwrap();
    (controller, renderer, worker)
}

pub(super) fn render(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    renderer.render(&mut out);
    out
}

pub(super) fn serve(worker: &mut StreamWorker) {
    while worker.service() {}
}

pub(super) const CALLS: [usize; 6] = [700, 1023, 1500, 256, 4096, 2000];

#[test]
fn prebuffered_streams_render_like_the_mixer() {
    let mut looped = stream(3000, 48000.0);
    looped.set_looping(true);
    looped.set_loop_point(0.01);
    looped.set_volume(0.5);
    let once = stream(5000, 22050.0);

    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&looped);
    mixer.play(&once);
    let mut expected = Vec::new();
    for frames in CALLS {
        let mut out = vec![0.0; frames * 2];
        mixer.mix_interleaved(&mut out);
        expected.extend(out);
    }

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    controller.play(&looped).unwrap();
    controller.play(&once).unwrap();
    let mut rendered = Vec::new();
    for frames in CALLS {
        serve(&mut worker);
        rendered.extend(render(&mut renderer, frames));
    }
    let first = rendered.iter().zip(&expected).position(|(a, b)| a != b);
    assert_eq!(
        first,
        None,
        "{}",
        controller.status().stream_underflow_frames
    );
    assert_eq!(controller.status().stream_underflow_frames, 0);
}

#[test]
fn a_stream_waits_silently_for_the_worker() {
    let tone = stream(4000, 44100.0);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&tone);
    let mut expected = vec![0.0; 1024 * 2];
    mixer.mix_interleaved(&mut expected);

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    controller.play(&tone).unwrap();
    assert!(render(&mut renderer, 512).iter().all(|&s| s == 0.0));
    serve(&mut worker);
    assert_eq!(render(&mut renderer, 1024), expected);
    assert_eq!(controller.status().stream_underflow_frames, 0);
}

#[test]
fn a_dry_feed_counts_underflow_and_keeps_its_place() {
    let tone = stream(8000, 44100.0);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&tone);
    let mut expected = vec![0.0; 2048 * 2];
    mixer.mix_interleaved(&mut expected);

    let config = RealtimeConfig {
        stream_chunks: 2,
        ..RealtimeConfig::default()
    };
    let (mut controller, mut renderer, mut worker) = runtime(config);
    controller.play(&tone).unwrap();
    serve(&mut worker);
    // The linear resampler plays each frame one frame late.
    let first = render(&mut renderer, 2048);
    assert_eq!(first[..2050], expected[..2050]);
    assert!(first[2050..].iter().all(|&s| s == 0.0));
    assert_eq!(controller.status().stream_underflow_frames, 1024);
    serve(&mut worker);
    let second = render(&mut renderer, 1024);
    assert_eq!(second[2..], expected[2050..]);
}

#[test]
fn seeks_continue_from_the_requested_frame() {
    let tone = stream(40000, 44100.0);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let handle = mixer.play(&tone);
    mixer.seek(handle, 0.5).unwrap();
    let mut from_start = vec![0.0; 1536 * 2];
    mixer.mix_interleaved(&mut from_start);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let handle = mixer.play(&tone);
    let mut skipped = vec![0.0; 1024 * 2];
    mixer.mix_interleaved(&mut skipped);
    mixer.seek(handle, 0.25).unwrap();
    let mut expected = vec![0.0; 3072 * 2];
    mixer.mix_interleaved(&mut expected);

    // Seeking before the first frame: the voice waits for the new place.
    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    let id = controller.play(&tone).unwrap();
    controller.seek(id, 0.5).unwrap();
    serve(&mut worker);
    assert!(render(&mut renderer, 512).iter().all(|&s| s == 0.0));
    serve(&mut worker);
    assert_eq!(render(&mut renderer, 1536), from_start);
    controller.stop(id).unwrap();

    // Seeking while playing: the queued audio is dropped, and the voice
    // continues one block of silence later than the mixer's.
    let id = controller.play(&tone).unwrap();
    serve(&mut worker);
    assert_eq!(render(&mut renderer, 1024), skipped);
    controller.seek(id, 0.25).unwrap();
    let mut rendered = render(&mut renderer, 512);
    serve(&mut worker);
    rendered.extend(render(&mut renderer, 2560));
    assert_eq!(rendered[1536 * 2..], expected[1024 * 2..2560 * 2]);
    assert_eq!(controller.status().stream_underflow_frames, 0);
}

#[test]
fn ended_and_missing_streams_finish_and_close_their_feeds() {
    let missing = Stream::new(Arc::new(Memory {
        planes: Arc::new(vec![vec![0.0; 100]]),
        rate: 44100.0,
        missing: true,
    }))
    .unwrap();
    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    let short = controller.play(&stream(700, 44100.0)).unwrap();
    let gone = controller.play(&missing).unwrap();
    for _ in 0..4 {
        serve(&mut worker);
        render(&mut renderer, 512);
    }
    let finished: Vec<Event> = std::iter::from_fn(|| controller.poll_event())
        .filter(|event| matches!(event, Event::Finished(..)))
        .collect();
    assert!(finished.contains(&Event::Finished(short, FinishReason::Ended)));
    assert!(finished.contains(&Event::Finished(gone, FinishReason::Ended)));
    controller.release_retired();
    serve(&mut worker);
    assert_eq!(worker.streams(), 0);
}

#[test]
fn the_worker_finishes_with_the_controller_and_renderer() {
    let (mut controller, renderer, mut worker) = runtime(RealtimeConfig::default());
    let mut looped = stream(1000, 44100.0);
    looped.set_looping(true);
    controller.play(&looped).unwrap();
    assert!(controller.take_stream_worker().is_none());
    serve(&mut worker);
    assert!(!worker.is_finished());
    drop(controller);
    drop(renderer);
    serve(&mut worker);
    assert!(worker.is_finished());
}

#[test]
fn a_worker_that_does_not_run_fills_the_pending_streams() {
    let config = RealtimeConfig {
        pending_streams: 1,
        ..RealtimeConfig::default()
    };
    let (mut controller, _renderer, mut worker) = runtime(config);
    let tone = stream(1000, 44100.0);
    controller.play(&tone).unwrap();
    assert_eq!(controller.play(&tone), Err(ControllerError::StreamsFull));
    serve(&mut worker);
    controller.play(&tone).unwrap();
}

#[test]
fn queued_streams_render_like_the_mixer() {
    let first = stream(1300, 44100.0);
    let mut looping = stream(700, 44100.0);
    looping.set_looping(true);
    let tone = crate::Sound::new(crate::Pcm::from_planar(vec![0.25; 900], 1, 44100.0).unwrap());
    let mut queue = crate::Queue::new();
    queue.set_params(44100.0, 2).unwrap();

    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&queue);
    mixer.queue_play(&queue, &first).unwrap();
    mixer.queue_play(&queue, &tone).unwrap();
    mixer.queue_play(&queue, &looping).unwrap();
    mixer.queue_play(&queue, &first).unwrap();
    let mut expected = Vec::new();
    for frames in CALLS {
        let mut out = vec![0.0; frames * 2];
        mixer.mix_interleaved(&mut out);
        expected.extend(out);
    }

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    controller.play(&queue).unwrap();
    controller.queue_play(&queue, &first).unwrap();
    controller.queue_play(&queue, &tone).unwrap();
    controller.queue_play(&queue, &looping).unwrap();
    controller.queue_play(&queue, &first).unwrap();
    let mut rendered = Vec::new();
    for frames in CALLS {
        serve(&mut worker);
        rendered.extend(render(&mut renderer, frames));
    }
    assert!(expected.iter().any(|&s| s != 0.0));
    assert_eq!(rendered, expected);
    assert_eq!(controller.status().stream_underflow_frames, 0);
    controller.release_retired();
    while controller.poll_event().is_some() {}
    controller.release_retired();
    serve(&mut worker);
    assert_eq!(worker.streams(), 0);
}
