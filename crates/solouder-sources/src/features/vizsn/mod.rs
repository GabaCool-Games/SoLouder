// Ported from SoLoud src/audiosource/vizsn/soloud_vizsn.cpp: vizsn by
// Ville-Matias Heikkilä, integrated into SoLoud by Jari Komppa (2013-2018),
// both released under the WTFPL (http://www.wtfpl.net/txt/copying/); altered
// for SoLouder.

//! Vizsn (SoLoud's `Vizsn`): a small formant synthesiser that reads Finnish
//! text letter by letter, mono at 8000 Hz.

mod voice;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use solouder_core::{Generator, GeneratorContext, GeneratorInstance};

use voice::{Bank, KEYZ, P_CLR, P_END, Voice, plosive};

/// Vizsn's sample rate.
const SAMPLE_RATE: f32 = 8000.0;

/// The most samples one phone renders: 300, then 50 slides of 10.
const PHONE_SAMPLES: usize = 800;

/// The base pitch (`mPitch`).
const PITCH: f32 = 800.0;

/// Ticks of the slide from one phone to the next.
const SLIDE_TICKS: i32 = 50;

/// Finnish text to speech (SoLoud's `Vizsn`), mono at 8000 Hz.
///
/// The text is read as bytes up to its first 0, one phone a letter: `a` to
/// `z`, `{` or 0x84 for ä and `|` or 0x94 for ö (CP437). Any other byte,
/// and the start of the text, is a pause. As in SoLoud the output lies
/// between 0 and 1, centred on 128/255; once the text has been read voices
/// end (looping ones start over).
#[derive(Clone, Debug)]
pub struct Vizsn {
    /// The phones, from a pause to [`P_END`].
    phones: Arc<[i8]>,
}

impl Default for Vizsn {
    /// No text: voices end at once (reference patch 0099), as SoLoud's
    /// before `setText`.
    fn default() -> Self {
        Self {
            phones: Arc::new([P_END]),
        }
    }
}

impl Vizsn {
    /// Vizsn reading `text` (`Vizsn::setText`). SoLoud's `setText` stopped
    /// the source's voices and changed it; build a new `Vizsn` instead.
    pub fn new(text: impl AsRef<[u8]>) -> Self {
        let text = text.as_ref();
        let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
        let mut phones = Vec::with_capacity(text.len() + 2);
        phones.push(P_CLR);
        phones.extend(text.iter().map(|&b| phone(b)));
        phones.push(P_END);
        Self {
            phones: phones.into(),
        }
    }

    /// How many samples the text plays before its voices end.
    pub fn samples(&self) -> usize {
        self.phones
            .windows(2)
            .map(|p| phone_samples(p[0], p[1]))
            .sum()
    }
}

/// The phone of a text byte.
fn phone(byte: u8) -> i8 {
    let byte = match byte {
        0x84 => b'{',
        0x94 => b'|',
        b => b,
    };
    match byte {
        b'a'..=b'|' => KEYZ[usize::from(byte - b'a')],
        _ => P_CLR,
    }
}

/// The samples of `phone` followed by `next`.
fn phone_samples(phone: i8, next: i8) -> usize {
    let held = if plosive(phone) { 100 } else { 300 };
    let slide = if plosive(next) { 3 } else { 10 };
    held + SLIDE_TICKS as usize * slide
}

impl Generator for Vizsn {
    fn channels(&self) -> u32 {
        1
    }

    fn sample_rate(&self) -> f32 {
        SAMPLE_RATE
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        Box::new(VizsnInstance {
            phones: Arc::clone(&self.phones),
            at: 0,
            voice: Voice::new(),
            next: Bank::default(),
            buffer: [0.0; PHONE_SAMPLES],
            read: 0,
            written: 0,
        })
    }
}

/// One voice of a [`Vizsn`] (`VizsnInstance`).
#[derive(Debug)]
struct VizsnInstance {
    phones: Arc<[i8]>,
    /// The phone being read (`mS`).
    at: usize,
    voice: Voice,
    /// The next phone's bank (`mBank1`).
    next: Bank,
    /// One phone's samples (reference patch 0101).
    buffer: [f32; PHONE_SAMPLES],
    read: usize,
    written: usize,
}

impl VizsnInstance {
    /// Renders the current phone into the buffer; false at the text's end.
    fn render_phone(&mut self) -> bool {
        let phone = self.phones[self.at];
        self.voice.bank.set_phone(phone, PITCH);
        if phone == P_END {
            return false;
        }
        let next = self.phones[self.at + 1];
        self.next.set_phone(next, PITCH);
        let step = self.voice.bank.slide(&self.next, SLIDE_TICKS as f32);
        self.voice.nper = 0;
        let mut n = 0;
        let held = if plosive(phone) { 100 } else { 300 };
        for _ in 0..held {
            self.buffer[n] = self.voice.sample();
            n += 1;
        }
        for _ in 0..SLIDE_TICKS {
            let samples = if plosive(next) { 3 } else { 10 };
            for _ in 0..samples {
                self.buffer[n] = self.voice.sample();
                n += 1;
            }
            // Only a slide into a vowel or a continuant moves.
            if !plosive(next) {
                self.voice.bank.tick(&step);
            }
        }
        self.written = n;
        self.at += 1;
        // SoLoud copies the whole next bank, so each phone starts from its
        // resonator histories, which only ever hold zeros.
        self.voice.bank = self.next;
        true
    }
}

impl GeneratorInstance for VizsnInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        _: usize,
        _: &GeneratorContext,
    ) -> usize {
        let out = &mut out[..frames];
        let mut idx = 0;
        while idx < frames {
            if self.read < self.written {
                let copy = (self.written - self.read).min(frames - idx);
                out[idx..idx + copy].copy_from_slice(&self.buffer[self.read..self.read + copy]);
                self.read += copy;
                idx += copy;
                continue;
            }
            self.read = 0;
            self.written = 0;
            if !self.render_phone() {
                // Past the text the block is silent, and the read ends there
                // so that the mixer sees the end (reference patch 0103).
                out[idx..].fill(0.0);
                return idx;
            }
        }
        frames
    }

    fn has_ended(&self) -> bool {
        self.phones[self.at] == P_END && self.read >= self.written
    }

    fn rewind(&mut self) -> bool {
        // A new voice's state (reference patch 0102).
        self.at = 0;
        self.voice = Voice::new();
        self.next = Bank::default();
        self.buffer = [0.0; PHONE_SAMPLES];
        self.read = 0;
        self.written = 0;
        true
    }
}
