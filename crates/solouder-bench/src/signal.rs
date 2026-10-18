//! The audio the cases play: tones generated here, and fixture files.
//! `reference_bench.cpp` generates the same tones with the same formulas.

use std::path::PathBuf;

use solouder_core::{Pcm, PlayOptions, Sound};

/// Output rate of every case.
pub const OUTPUT_RATE: u32 = 48_000;

/// Rate of the generated tones, so every voice is resampled.
pub const TONE_RATE: f32 = 44_100.0;

/// Distinct tones; voice `i` plays tone `i % TONES`.
pub const TONES: usize = 8;

/// Frames per tone: two seconds.
const TONE_FRAMES: usize = 88_200;

/// The streamed fixture of each codec, relative to `tests/fixtures`: all
/// stereo at 44.1 kHz.
pub const STREAMS: [(&str, &str); 4] = [
    ("wav", "wav/s16_stereo_44100.wav"),
    ("flac", "flac/s16_stereo_44100.flac"),
    ("ogg", "ogg/vorbis_stereo_44100.ogg"),
    ("mp3", "mp3/mpeg1_stereo_44100_cbr128.mp3"),
];

/// Samples of tone `k`: a sine at `110 * (k + 1) * 1.25` Hz, amplitude 0.5.
pub fn tone_samples(k: usize) -> Vec<f32> {
    let frequency = 110.0 * (k as f64 + 1.0) * 1.25;
    let step = std::f64::consts::TAU * frequency / f64::from(TONE_RATE);
    (0..TONE_FRAMES)
        .map(|i| (0.5 * (step * i as f64).sin()) as f32)
        .collect()
}

/// The [`TONES`] looping mono tones.
pub fn tones() -> Vec<Sound> {
    (0..TONES)
        .map(|k| {
            let pcm = Pcm::from_planar(tone_samples(k), 1, TONE_RATE).expect("valid tone");
            let mut sound = Sound::new(pcm);
            sound.set_looping(true);
            sound
        })
        .collect()
}

/// Volume and pan of voice `i`, as the C++ bench plays it.
pub fn voice_options(i: usize) -> PlayOptions {
    PlayOptions {
        volume: Some(0.05),
        pan: ((i * 37) % 201) as f32 / 100.0 - 1.0,
        ..PlayOptions::default()
    }
}

/// The path of a file under `tests/fixtures`.
pub fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(relative)
}
