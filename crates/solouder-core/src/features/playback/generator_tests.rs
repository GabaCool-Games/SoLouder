use std::sync::{Arc, Mutex};

use super::*;
use crate::dsp::Clipper;
use crate::features::playback::{ControlError, Mixer, MixerConfig, Queue};

/// Every `(frames, stride)` a generator was asked for.
type Calls = Arc<Mutex<Vec<(usize, usize)>>>;

/// Counts frames (source frame `n` is `n / 65536`, which the linear
/// resampler plays one output frame later), records every call's sizes, and
/// ends after `length` frames unless it loops.
#[derive(Debug)]
struct Counter {
    channels: u32,
    length: u64,
    can_rewind: bool,
    calls: Calls,
}

#[derive(Debug)]
struct CounterInstance {
    channels: usize,
    length: u64,
    at: u64,
    can_rewind: bool,
    calls: Calls,
}

impl Generator for Counter {
    fn channels(&self) -> u32 {
        self.channels
    }

    fn sample_rate(&self) -> f32 {
        44100.0
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        Box::new(CounterInstance {
            channels: self.channels as usize,
            length: self.length,
            at: 0,
            can_rewind: self.can_rewind,
            calls: Arc::clone(&self.calls),
        })
    }
}

impl GeneratorInstance for CounterInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        stride: usize,
        _: &GeneratorContext,
    ) -> usize {
        self.calls.lock().unwrap().push((frames, stride));
        let count = (self.length - self.at).min(frames as u64) as usize;
        for f in 0..count {
            for c in 0..self.channels {
                out[c * stride + f] = (self.at + f as u64) as f32 / 65536.0;
            }
        }
        self.at += count as u64;
        count
    }

    fn has_ended(&self) -> bool {
        self.at >= self.length
    }

    fn rewind(&mut self) -> bool {
        if self.can_rewind {
            self.at = 0;
        }
        self.can_rewind
    }

    fn info(&self, key: u32) -> f32 {
        key as f32 + self.at as f32
    }
}

fn counter(channels: u32, length: u64, can_rewind: bool) -> (Synth, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let generator = Counter {
        channels,
        length,
        can_rewind,
        calls: Arc::clone(&calls),
    };
    (Synth::new(Arc::new(generator)).unwrap(), calls)
}

fn mixer() -> Mixer {
    let config = MixerConfig {
        clipper: Clipper::Hard,
        channels: 1,
        ..MixerConfig::default()
    };
    let mut mixer = Mixer::new(config).unwrap();
    mixer.set_post_clip_scaler(1.0);
    mixer
}

fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames];
    mixer.mix_interleaved(&mut out);
    out
}

#[test]
fn synths_validate_their_generator() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let bad = Counter {
        channels: 9,
        length: 1,
        can_rewind: false,
        calls,
    };
    assert_eq!(
        Synth::new(Arc::new(bad)).unwrap_err(),
        SynthError::UnsupportedChannelCount(9)
    );
}

#[test]
fn voices_read_whole_blocks_and_play_the_frames_computed() {
    let (synth, calls) = counter(1, 700, false);
    let mut mixer = mixer();
    let handle = mixer.play(&synth);
    let out = render(&mut mixer, 1024);
    assert!(out[700] > out[699] && out[699] > 0.0);
    assert!(out[701..].iter().all(|&s| s == 0.0));
    assert_eq!(*calls.lock().unwrap(), [(512, 512), (512, 512)]);
    assert!(!mixer.is_valid_voice_handle(handle));
}

#[test]
fn loop_wraps_rewind_and_refill_the_block() {
    let (mut synth, calls) = counter(1, 300, true);
    synth.set_looping(true);
    let mut mixer = mixer();
    let handle = mixer.play(&synth);
    let out = render(&mut mixer, 512);
    assert_eq!(out[301], 0.0);
    assert!(out[302] > 0.0 && out[300] > out[302]);
    assert_eq!(mixer.loop_count(handle), Some(1));
    assert_eq!(*calls.lock().unwrap(), [(512, 512), (212, 512)]);
}

#[test]
fn a_looping_voice_that_cannot_rewind_ends() {
    let (mut synth, _) = counter(1, 300, false);
    synth.set_looping(true);
    let mut mixer = mixer();
    let handle = mixer.play(&synth);
    render(&mut mixer, 1024);
    assert!(!mixer.is_valid_voice_handle(handle));
}

#[test]
fn seeks_discard_in_passes_of_the_scratch_size() {
    let (synth, calls) = counter(2, 1 << 30, true);
    let mut mixer = mixer();
    let handle = mixer.play(&synth);
    // 10000 frames, in passes of 4096 / 2 frames at their own stride.
    assert_eq!(mixer.seek(handle, 10000.0 / 44100.0), Ok(()));
    let passes: Vec<_> = calls.lock().unwrap().clone();
    assert_eq!(
        passes,
        [
            (2048, 2048),
            (2048, 2048),
            (2048, 2048),
            (2048, 2048),
            (1808, 1808)
        ]
    );
    assert_eq!(mixer.info(handle, 3), Some(10003.0));
    calls.lock().unwrap().clear();
    // Back: rewind, then discard from the start.
    assert_eq!(mixer.seek(handle, 100.0 / 44100.0), Ok(()));
    assert_eq!(*calls.lock().unwrap(), [(100, 100)]);
}

#[test]
fn seeking_back_without_rewind_fails() {
    let (synth, _) = counter(1, 1 << 20, false);
    let mut mixer = mixer();
    let handle = mixer.play(&synth);
    assert_eq!(mixer.seek(handle, 0.5), Ok(()));
    assert_eq!(mixer.seek(handle, 0.1), Err(ControlError::CannotSeekBack));
    assert_eq!(mixer.stream_position(handle), Some(0.5));
}

#[test]
fn queued_synths_play_back_to_back() {
    let (first, first_calls) = counter(1, 600, false);
    let (second, _) = counter(1, 600, false);
    let mut mixer = mixer();
    let queue = Queue::new();
    mixer.play(&queue);
    mixer.queue_play(&queue, &first).unwrap();
    mixer.queue_play(&queue, &second).unwrap();
    let out = render(&mut mixer, 1024);
    assert!(out[600] > out[599] && out[599] > 0.0);
    assert_eq!(out[601], 0.0);
    assert!(out[602] > 0.0 && out[600] > out[602]);
    assert_eq!(*first_calls.lock().unwrap(), [(512, 512), (512, 512)]);
}
