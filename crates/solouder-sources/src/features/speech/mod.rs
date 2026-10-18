// Ported from SoLoud src/audiosource/speech/soloud_speech.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder. The
// synthesiser and text rules are SoLoud's copy of rsynth by Nick
// Ing-Simmons et al. (public domain; SoLoud's changes public domain or CC0).

//! Text to speech (SoLoud's `Speech`): English text through rsynth's
//! letter-to-sound rules into a Klatt formant synthesiser, mono at 11025 Hz.

mod cdata;
mod elements;
mod klatt;
mod resonator;
mod rules;
mod synth;
mod tts;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use solouder_core::dsp::c_int;
use solouder_core::{Generator, GeneratorContext, GeneratorInstance};

use klatt::{Klatt, SAMPLE_RATE};

/// The voicing source's waveform (`KLATT_WAVEFORM`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KlattWaveform {
    /// A sawtooth; also SoLoud's choice for indices past the last.
    #[default]
    Saw,
    /// A triangle.
    Triangle,
    /// A sine.
    Sin,
    /// A square wave, `Speech`'s default.
    Square,
    /// A pulse of a quarter period.
    Pulse,
    /// Follows the noise source's lowest bit.
    Noise,
    /// Follows the noise source's lowest 3 bits.
    Warble,
}

impl KlattWaveform {
    const ALL: [Self; 7] = [
        Self::Saw,
        Self::Triangle,
        Self::Sin,
        Self::Square,
        Self::Pulse,
        Self::Noise,
        Self::Warble,
    ];

    /// SoLoud's index (`KW_SAW` 0 to `KW_WARBLE` 6).
    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|&w| w == self).unwrap_or(0) as u32
    }

    /// The waveform of a SoLoud index, if there is one.
    pub fn from_index(index: u32) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }
}

/// A [`Speech`]'s voice (`Speech::setParams`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeechParams {
    /// The base pitch in tenths of Hz (1330 is 133 Hz); SoLoud stores it in
    /// an `int`, so values past `i32::MAX` wrap to negative.
    pub frequency: u32,
    /// Milliseconds per frame: higher is slower. Frames must hold 1 to 11025
    /// samples, so 0.0907 to 1000 (reference patch 0093).
    pub speed: f32,
    /// How fast the pitch falls, in tenths of Hz per frame.
    pub declination: f32,
    /// The voicing source's waveform.
    pub waveform: KlattWaveform,
}

impl Default for SpeechParams {
    /// `Speech::Speech`'s voice: 133 Hz, 10 ms frames, square wave. (C's
    /// `setParams` defaults its waveform to a triangle instead.)
    fn default() -> Self {
        Self {
            frequency: 1330,
            speed: 10.0,
            declination: 0.5,
            waveform: KlattWaveform::Square,
        }
    }
}

/// A speed whose frames would hold no samples or more than 11025.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvalidSpeechSpeed(pub f32);

impl fmt::Display for InvalidSpeechSpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "speech speed {} is out of range (frames of 1 to 11025 samples)",
            self.0
        )
    }
}

impl std::error::Error for InvalidSpeechSpeed {}

/// Samples per frame at `speed`, if 1 to 11025.
fn frame_samples(speed: f32) -> Option<usize> {
    if !(speed > 0.0 && speed <= 1000.0) {
        return None;
    }
    let samples = c_int(f64::from(SAMPLE_RATE as f32 * speed)) / 1000;
    (samples >= 1).then_some(samples as usize)
}

#[derive(Debug)]
struct Params {
    frequency: AtomicU32,
    speed: AtomicU32,
    declination: AtomicU32,
    waveform: AtomicU8,
}

impl Params {
    fn load(&self) -> SpeechParams {
        SpeechParams {
            frequency: self.frequency.load(Ordering::Relaxed),
            speed: f32::from_bits(self.speed.load(Ordering::Relaxed)),
            declination: f32::from_bits(self.declination.load(Ordering::Relaxed)),
            waveform: KlattWaveform::from_index(u32::from(self.waveform.load(Ordering::Relaxed)))
                .unwrap_or_default(),
        }
    }

    fn store(&self, params: SpeechParams) {
        self.frequency.store(params.frequency, Ordering::Relaxed);
        self.speed.store(params.speed.to_bits(), Ordering::Relaxed);
        self.declination
            .store(params.declination.to_bits(), Ordering::Relaxed);
        self.waveform
            .store(params.waveform.index() as u8, Ordering::Relaxed);
    }
}

/// Text to speech (SoLoud's `Speech`), mono at 11025 Hz.
///
/// The text is read as bytes up to its first 0. Words go through English
/// letter-to-sound rules; numbers are read out, words without vowels or
/// in capitals are spelled, and `[...]` passes phonemes through as they
/// are. Bytes from 0x80 have no class: they are spelled as their low 7
/// bits, as SoLoud's C runtimes do.
///
/// The frequency, declination and waveform reach playing voices at their
/// next block, as in SoLoud; the speed reaches new voices only (reference
/// patch 0094). Clones share the parameters.
#[derive(Clone, Debug)]
pub struct Speech {
    elements: Arc<[u8]>,
    frames: i32,
    params: Arc<Params>,
}

impl Default for Speech {
    /// No text: voices end at once, as SoLoud's before `setText`.
    fn default() -> Self {
        Self::new("")
    }
}

impl Speech {
    /// Speech of `text` (`Speech::setText`), with [`SpeechParams::default`].
    /// SoLoud's `setText` stopped the source's voices and changed it; build
    /// a new `Speech` instead.
    pub fn new(text: impl AsRef<[u8]>) -> Self {
        let text = text.as_ref();
        let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
        let mut phonemes = Vec::new();
        tts::xlate_string(text, &mut phonemes);
        let mut elements = Vec::new();
        let frames = elements::phone_to_elm(&phonemes, &mut elements);
        let params = Params {
            frequency: AtomicU32::new(0),
            speed: AtomicU32::new(0),
            declination: AtomicU32::new(0),
            waveform: AtomicU8::new(0),
        };
        params.store(SpeechParams::default());
        Self {
            elements: elements.into(),
            frames,
            params: Arc::new(params),
        }
    }

    /// The text's length in frames (`mFrames`); each frame is
    /// `speed` milliseconds.
    pub fn frames(&self) -> u32 {
        self.frames.max(0) as u32
    }

    /// The voice (`Speech`'s `mBase*` fields).
    pub fn params(&self) -> SpeechParams {
        self.params.load()
    }

    /// Sets the voice (`Speech::setParams`). A speed whose frames would
    /// hold no samples or more than 11025 is an error and changes nothing
    /// (SoLoud took any; reference patch 0093).
    pub fn set_params(&self, params: SpeechParams) -> Result<(), InvalidSpeechSpeed> {
        frame_samples(params.speed).ok_or(InvalidSpeechSpeed(params.speed))?;
        self.params.store(params);
        Ok(())
    }
}

impl Generator for Speech {
    fn channels(&self) -> u32 {
        1
    }

    fn sample_rate(&self) -> f32 {
        SAMPLE_RATE as f32
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        let params = self.params.load();
        let frames = frame_samples(params.speed).unwrap_or(1);
        let mut instance = SpeechInstance {
            params: Arc::clone(&self.params),
            elements: Arc::clone(&self.elements),
            speed: params.speed,
            klatt: Klatt::new(),
            samples: vec![0; frames * elements::longest()].into_boxed_slice(),
            sample_count: 10,
            offset: 10,
        };
        instance.start();
        Box::new(instance)
    }
}

/// One voice of a [`Speech`] (`SpeechInstance`).
#[derive(Debug)]
struct SpeechInstance {
    params: Arc<Params>,
    elements: Arc<[u8]>,
    /// The speed `samples` is sized for (reference patch 0094).
    speed: f32,
    klatt: Klatt,
    /// One element's samples, the longest element at `speed`.
    samples: Box<[i16]>,
    sample_count: i32,
    offset: i32,
}

impl SpeechInstance {
    /// `klatt::init` with the voice's parameters and speed.
    fn init(&mut self) {
        let p = self.params.load();
        self.klatt
            .init(p.frequency as i32, self.speed, p.declination, p.waveform);
    }

    /// From the first element (the constructor and `rewind`).
    fn start(&mut self) {
        self.init();
        self.klatt.init_synth(self.elements.len());
        self.offset = 10;
        self.sample_count = 10;
    }
}

/// 16-bit samples to floats (`writesamples`).
fn write_samples(src: &[i16], dst: &mut [f32]) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = f32::from(s) * (1.0 / 32768.0);
    }
}

impl GeneratorInstance for SpeechInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        _: usize,
        _: &GeneratorContext,
    ) -> usize {
        // As in SoLoud, every read starts the glottal period over.
        self.init();
        let mut written = 0;
        if self.sample_count > self.offset {
            let copy = ((self.sample_count - self.offset) as usize).min(frames);
            let from = self.offset as usize;
            write_samples(&self.samples[from..from + copy], &mut out[..copy]);
            self.offset += copy as i32;
            written += copy;
        }
        while self.sample_count >= 0 && written < frames {
            self.offset = 0;
            self.sample_count = self.klatt.synth(&self.elements, &mut self.samples);
            if self.sample_count > 0 {
                let copy = (self.sample_count as usize).min(frames - written);
                write_samples(&self.samples[..copy], &mut out[written..written + copy]);
                self.offset += copy as i32;
                written += copy;
            }
        }
        written
    }

    fn has_ended(&self) -> bool {
        self.sample_count < 0
    }

    fn rewind(&mut self) -> bool {
        self.start();
        true
    }
}

#[cfg(test)]
mod tests;
