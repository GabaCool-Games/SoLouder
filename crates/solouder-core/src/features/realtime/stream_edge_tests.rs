//! Streams through the controller: splitting a mixer that is playing them,
//! audio that has not arrived yet, short loops, and losing the source or
//! the worker.

use std::sync::Arc;

use super::stream_tests::{CALLS, Memory, render, runtime, serve, stream};
use super::*;
use crate::{Mixer, MixerConfig, Queue, Stream};

fn mix(mixer: &mut Mixer, calls: &[usize]) -> Vec<f32> {
    let mut mixed = Vec::new();
    for &frames in calls {
        let mut out = vec![0.0; frames * 2];
        mixer.mix_interleaved(&mut out);
        mixed.extend(out);
    }
    mixed
}

fn finished(controller: &mut Controller) -> Vec<Event> {
    std::iter::from_fn(|| controller.poll_event())
        .filter(|event| matches!(event, Event::Finished(..)))
        .collect()
}

#[test]
fn streams_playing_when_split_continue_on_the_worker() {
    let mut looped = stream(3000, 48000.0);
    looped.set_looping(true);
    looped.set_loop_point(0.01);
    let mut queue = Queue::new();
    queue.set_params(44100.0, 2).unwrap();
    let start = |mixer: &mut Mixer| {
        mixer.play(&looped);
        mixer.play(&queue);
        mixer.queue_play(&queue, &stream(5000, 44100.0)).unwrap();
        mixer.queue_play(&queue, &stream(2000, 44100.0)).unwrap();
    };
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    start(&mut mixer);
    let expected = mix(&mut mixer, &CALLS);

    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    start(&mut mixer);
    let mut rendered = mix(&mut mixer, &CALLS[..2]);
    let (mut controller, mut renderer) = split(mixer, RealtimeConfig::default());
    let mut worker = controller.take_stream_worker().unwrap();
    // The voice and both queued streams no longer read on the audio thread.
    assert_eq!(worker.streams(), 3);
    for frames in &CALLS[2..] {
        serve(&mut worker);
        rendered.extend(render(&mut renderer, *frames));
    }
    assert_eq!(rendered, expected);
    assert_eq!(controller.status().stream_underflow_frames, 0);
}

#[test]
fn a_looping_stream_split_at_its_end_wraps_on_the_worker() {
    let mut looped = stream(512, 44100.0);
    looped.set_looping(true);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&looped);
    let expected = mix(&mut mixer, &[512, 512]);

    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&looped);
    let mut rendered = mix(&mut mixer, &[512]);
    let (mut controller, mut renderer) = split(mixer, RealtimeConfig::default());
    let mut worker = controller.take_stream_worker().unwrap();
    serve(&mut worker);
    rendered.extend(render(&mut renderer, 512));
    assert_eq!(rendered, expected);
}

#[test]
fn a_short_loop_is_decoded_past_its_end_from_the_start() {
    let mut looped = stream(300, 44100.0);
    looped.set_looping(true);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    mixer.play(&looped);
    let expected = mix(&mut mixer, &[4096]);

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    controller.play(&looped).unwrap();
    serve(&mut worker);
    assert_eq!(render(&mut renderer, 4096), expected);
    assert_eq!(controller.status().stream_underflow_frames, 0);
}

#[test]
fn a_queue_skipping_audio_not_decoded_yet_asks_for_the_frame_after_it() {
    let tone = stream(20000, 44100.0);
    let mut queue = Queue::new();
    queue.set_params(44100.0, 2).unwrap();
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let handle = mixer.play(&queue);
    mixer.queue_play(&queue, &tone).unwrap();
    mixer.seek(handle, 0.05).unwrap();
    let expected = mix(&mut mixer, &[1536]);

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    let id = controller.play(&queue).unwrap();
    controller.queue_play(&queue, &tone).unwrap();
    controller.seek(id, 0.05).unwrap();
    assert!(render(&mut renderer, 512).iter().all(|&s| s == 0.0));
    serve(&mut worker);
    assert_eq!(render(&mut renderer, 1536), expected);
}

#[test]
fn a_seek_after_waiting_for_audio_counts_from_the_audio() {
    let tone = stream(20000, 44100.0);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let handle = mixer.play(&tone);
    mixer.seek(handle, 0.1).unwrap();
    let expected = mix(&mut mixer, &[1536]);

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    let id = controller.play(&tone).unwrap();
    // The voice's position moves on while it waits; its audio does not.
    assert!(render(&mut renderer, 1024).iter().all(|&s| s == 0.0));
    controller.seek(id, 0.1).unwrap();
    serve(&mut worker);
    assert!(render(&mut renderer, 512).iter().all(|&s| s == 0.0));
    serve(&mut worker);
    assert_eq!(render(&mut renderer, 1536), expected);
}

#[test]
fn a_seek_after_waiting_mid_block_lands_where_the_mixer_lands() {
    // The generic seek counts from the block read ahead, not from the frame
    // playing; a voice that waited lands where it would have with its audio.
    let tone = stream(20000, 44100.0);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let handle = mixer.play(&tone);
    mix(&mut mixer, &[700]);
    mixer.seek(handle, 0.1).unwrap();
    let expected = mix(&mut mixer, &[324, 1536]);

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    let id = controller.play(&tone).unwrap();
    render(&mut renderer, 700);
    controller.seek(id, 0.1).unwrap();
    render(&mut renderer, 324);
    serve(&mut worker);
    let rendered = render(&mut renderer, 1536);
    // The linear resampler blends in one frame of the block before.
    assert_eq!(rendered[2..], expected[324 * 2 + 2..]);
}

#[test]
fn a_looping_stream_without_its_file_ends() {
    let mut missing = Stream::new(Arc::new(Memory {
        planes: Arc::new(vec![vec![0.0; 100]]),
        rate: 44100.0,
        missing: true,
    }))
    .unwrap();
    missing.set_looping(true);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let handle = mixer.play(&missing);
    // A step of a whole block or more: the voice never fills a block.
    let stuck = mixer.play(&missing);
    mixer.set_relative_play_speed(stuck, 600.0).unwrap();
    mix(&mut mixer, &[2048]);
    assert!(!mixer.is_valid_voice_handle(handle));
    assert!(!mixer.is_valid_voice_handle(stuck));

    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    let id = controller.play(&missing).unwrap();
    let options = crate::PlayOptions {
        speed: 600.0,
        ..Default::default()
    };
    let stuck = controller.play_with(&missing, options).unwrap();
    for _ in 0..4 {
        serve(&mut worker);
        render(&mut renderer, 512);
    }
    let finished = finished(&mut controller);
    assert!(finished.contains(&Event::Finished(id, FinishReason::Ended)));
    assert!(finished.contains(&Event::Finished(stuck, FinishReason::Ended)));
}

#[test]
fn streams_end_once_the_worker_is_gone() {
    let mut looped = stream(1000, 44100.0);
    looped.set_looping(true);
    let (mut controller, mut renderer, mut worker) = runtime(RealtimeConfig::default());
    let id = controller.play(&looped).unwrap();
    serve(&mut worker);
    drop(worker);
    assert_eq!(
        controller.play(&looped),
        Err(ControllerError::NoStreamWorker)
    );
    // What was decoded still plays.
    assert!(render(&mut renderer, 2048).iter().any(|&s| s != 0.0));
    for _ in 0..40 {
        render(&mut renderer, 512);
    }
    assert!(finished(&mut controller).contains(&Event::Finished(id, FinishReason::Ended)));
}

/// A source of `frames` frames of 0.5 whose readers cannot rewind.
#[derive(Debug)]
struct NoRewind {
    frames: u32,
}

#[derive(Debug)]
struct NoRewindReader {
    left: u32,
}

impl crate::StreamSource for NoRewind {
    fn format(&self) -> crate::StreamFormat {
        crate::StreamFormat {
            channels: 1,
            sample_rate: 44100.0,
            frames: self.frames,
        }
    }

    fn open(&self) -> Option<Box<dyn crate::StreamReader>> {
        Some(Box::new(NoRewindReader { left: self.frames }))
    }
}

impl crate::StreamReader for NoRewindReader {
    fn read(&mut self, out: &mut [f32], _stride: usize, frames: usize) -> usize {
        let count = frames.min(self.left as usize);
        out[..count].fill(0.5);
        self.left -= count as u32;
        count
    }

    fn rewind(&mut self) -> bool {
        false
    }
}

#[test]
fn a_looping_stream_that_cannot_rewind_ends_offline() {
    let mut looped = Stream::new(Arc::new(NoRewind { frames: 700 })).unwrap();
    looped.set_looping(true);
    let mut mixer = Mixer::new(MixerConfig::default()).unwrap();
    let h = mixer.play(&looped);
    let mixed = mix(&mut mixer, &[512, 512, 512]);
    // The linear resampler blends in one silent frame before the start.
    assert!(mixed[2..2 * 600].iter().all(|&s| s > 0.1));
    assert!(!mixer.is_valid_voice_handle(h));
}
