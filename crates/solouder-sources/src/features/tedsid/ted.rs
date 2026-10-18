// Ported from SoLoud src/audiosource/tedsid/ted.cpp and ted.h, SoLoud's
// copy of tedplay, (c) 2012 Attila Grosz, used under the Unlicense
// (http://unlicense.org/); altered for SoLouder.

//! The Commodore Plus/4's TED sound (tedplay's `TED`): two square-wave
//! oscillators, the second of which can play noise instead, and a digital
//! output mode. tedplay's other waveforms are never selected in SoLoud,
//! so only the square is ported.

/// An oscillator wraps at this count.
const RELOAD: i32 = 0x3FF;

/// The TED after `oscillatorInit` (`TED`).
#[derive(Clone, Debug)]
pub(crate) struct Ted {
    volume: i32,
    snd1: bool,
    snd2: bool,
    noise_on: bool,
    digital: bool,
    freq: [u16; 2],
    noise_counter: usize,
    flip_flop: [bool; 2],
    dc_output: [bool; 2],
    count: [i32; 2],
    reload: [i32; 2],
    noise: [bool; 256],
}

impl Ted {
    pub(crate) fn new() -> Self {
        // The noise: an 8-bit LFSR from 0xa8; only its low bits are read.
        let mut noise = [false; 256];
        let mut im: u32 = 0xa8;
        for value in &mut noise {
            *value = im & 1 != 0;
            let feedback =
                1 ^ ((im >> 7) & 1) ^ ((im >> 5) & 1) ^ ((im >> 4) & 1) ^ ((im >> 1) & 1);
            im = (im << 1).wrapping_add(feedback);
        }
        Self {
            volume: 8,
            snd1: false,
            snd2: false,
            noise_on: false,
            digital: false,
            freq: [0; 2],
            noise_counter: 0,
            flip_flop: [false; 2],
            dc_output: [false; 2],
            count: [0; 2],
            reload: [0; 2],
            noise,
        }
    }

    fn set_freq(&mut self, channel: usize, freq: u16) {
        self.dc_output[channel] = freq == 0x3FE;
        self.reload[channel] = i32::from(freq.wrapping_add(1) & 0x3FF);
    }

    /// Writes sound register `reg`, 0 to 4 (`TED::writeSoundReg`).
    pub(crate) fn write(&mut self, reg: u32, value: u8) {
        let value16 = u16::from(value);
        match reg {
            0 => {
                self.freq[0] = (self.freq[0] & 0x300) | value16;
                self.set_freq(0, self.freq[0]);
            }
            1 => {
                self.freq[1] = (self.freq[1] & 0x300) | value16;
                self.set_freq(1, self.freq[1]);
            }
            2 => {
                self.freq[1] = (self.freq[1] & 0xFF) | (value16 << 8);
                self.set_freq(1, self.freq[1]);
            }
            3 => {
                self.digital = value & 0x80 != 0;
                if self.digital {
                    self.flip_flop = [true; 2];
                    self.count = self.reload;
                    self.noise_counter = 0xFF;
                }
                // The master volume is 8: (volume << 8) * 8 / 10.
                let volume = i32::from(value & 0x0F).min(8);
                self.volume = (volume << 8) * 8 / 10;
                self.snd1 = value & 0x10 != 0;
                self.snd2 = value & 0x20 != 0;
                self.noise_on = value & 0x40 != 0;
            }
            4 => {
                self.freq[0] = (self.freq[0] & 0xFF) | (value16 << 8);
                self.set_freq(0, self.freq[0]);
            }
            _ => {}
        }
    }

    /// Steps an oscillator; true when it wrapped.
    fn step(&mut self, channel: usize) -> bool {
        if self.dc_output[channel] {
            self.flip_flop[channel] = true;
            return false;
        }
        self.count[channel] += 1;
        if self.count[channel] < RELOAD {
            return false;
        }
        self.flip_flop[channel] = !self.flip_flop[channel];
        self.count[channel] = self.reload[channel] + (self.count[channel] - RELOAD);
        true
    }

    /// One sample (`TED::renderSound` of 1).
    pub(crate) fn sample(&mut self) -> i16 {
        if self.digital {
            let mut sample = 0_i16;
            if self.snd1 {
                sample = self.volume as i16;
            }
            if self.snd2 {
                sample = sample.wrapping_add(self.volume as i16);
            }
            return sample;
        }
        self.step(0);
        if self.step(1) {
            self.noise_counter = (self.noise_counter + 1) & 0xFF;
        }
        let mut result = if self.snd1 && self.flip_flop[0] {
            self.volume
        } else {
            0
        };
        // The second voice plays its square, or else noise.
        if (self.snd2 && self.flip_flop[1]) || (self.noise_on && self.noise[self.noise_counter]) {
            result += self.volume;
        }
        result as i16
    }
}
