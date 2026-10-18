//! What the soak plays, and its random numbers.

use std::sync::Arc;

use solouder_bench::signal::{STREAMS, TONE_RATE, TONES, fixture, tone_samples, tones};
use solouder_core::filters::Filter;
use solouder_core::{Bus, Pcm, Sound, Stream, Synth};
use solouder_decoders::WavStream;

/// xorshift64*.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    /// Uniform in `[low, high)`.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Looping tones, short clips, a clip per filter kind, a stream per codec,
/// a bus, and with the `sources` feature the generated sources' looping
/// generators (empty without it).
pub struct Sources {
    pub tones: Vec<Sound>,
    pub shots: Vec<Sound>,
    pub filtered: Vec<Sound>,
    pub streams: Vec<Stream>,
    pub bus: Bus,
    pub synths: Vec<Synth>,
}

/// Every generator of the `sources_*` benchmark cases.
#[cfg(feature = "sources")]
fn synths() -> Vec<Synth> {
    use solouder_bench::cases_sources::{KINDS, synths};
    KINDS.iter().flat_map(|kind| synths(kind)).collect()
}

#[cfg(not(feature = "sources"))]
fn synths() -> Vec<Synth> {
    Vec::new()
}

pub fn sources() -> Sources {
    let clip = |k: usize, frames: usize| {
        let samples = tone_samples(k)[..frames].to_vec();
        Sound::new(Pcm::from_planar(samples, 1, TONE_RATE).expect("valid clip"))
    };
    let shots = (0..TONES).map(|k| clip(k, 2_000 + 3_000 * k)).collect();
    let kinds: Vec<Arc<dyn Filter>> = solouder_bench::cases::filter_kinds();
    let filtered = kinds
        .into_iter()
        .enumerate()
        .map(|(k, filter)| {
            let mut sound = clip(k % TONES, 44_100);
            sound.set_filter(0, Some(filter));
            sound
        })
        .collect();
    let streams = STREAMS
        .iter()
        .map(|(_, path)| {
            let file = WavStream::load_to_memory(fixture(path)).expect("stream fixture");
            Stream::new(Arc::new(file)).expect("stream")
        })
        .collect();
    Sources {
        tones: tones(),
        shots,
        filtered,
        streams,
        bus: Bus::new(),
        synths: synths(),
    }
}
