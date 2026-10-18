// Ported from SoLoud src/audiosource/tedsid/sid.cpp and sid.h, SoLoud's
// copy of tedplay, (c) 2012 Attila Grosz, used under the Unlicense
// (http://unlicense.org/); altered for SoLouder.

//! The Commodore 64's SID sound chip (tedplay's `SIDsound`): three voices
//! with an envelope each, ring modulation and sync, and a state-variable
//! filter. The envelope follows Kevin Horton's notes, the filter a
//! simplified version of Antti Lankila's.

use std::f64::consts::PI;

use super::envelope::Envelope;

/// The TED's sound clock, at which SoLoud runs both chips.
pub(crate) const TED_SOUND_CLOCK: u32 = 221_680;

/// The PAL C64's clock.
const SID_CLOCK: u32 = 985_248;

/// One SID voice (`SIDVoice`); the envelope is in [`Envelope`].
#[derive(Clone, Copy, Debug)]
struct Voice {
    wave: u32,
    /// The waveform accumulator, 8.16 fixed point in 28 bits.
    accu: u32,
    add: u32,
    shift_reg: u32,
    freq: u16,
    pw: u16,
    envelope: Envelope,
    ring: u32,
    test: u32,
    filter: u32,
    muted: u32,
    /// Set on the voice whose wrap resets the next one.
    sync: u32,
}

impl Voice {
    /// `SIDsound::reset`'s voice.
    fn new() -> Self {
        Self {
            wave: 0,
            accu: 0,
            add: 0,
            shift_reg: 0x7F_FFFC,
            freq: 0,
            pw: 0,
            envelope: Envelope::new(),
            ring: 0,
            test: 0,
            filter: 0,
            muted: 0,
            sync: 0,
        }
    }

    fn update_shift_reg(&mut self) {
        let feedback = ((self.shift_reg >> 22) ^ (self.shift_reg >> 17)) & 1;
        self.shift_reg = (self.shift_reg << 1) | feedback;
    }
}

fn bit(value: u32, n: u32) -> u32 {
    (value >> n) & 1
}

/// A SID (`SIDsound`) at the TED's sound clock, after the setup
/// `TedSidInstance` gives it.
#[derive(Clone, Debug)]
pub(crate) struct Sid {
    voices: [Voice; 3],
    volume: i32,
    cycles_per_sample: u32,
    clock_remainder: u32,
    dc_mixer: i32,
    dc_voice: i32,
    dc_wave: i32,
    filter_type: u8,
    filter_cutoff: u32,
    cutoff_freq: Box<[f64; 2048]>,
    resonance: i32,
    w0: i32,
    vhp: i32,
    vbp: i32,
    vlp: i32,
}

impl Sid {
    /// A SID of `model` (0 6581, 1 8580, 2 8580 with the digi boost, 3
    /// 6581 R1). As in SoLoud, the filter's cutoff stays 0 until a cutoff
    /// register is written: it was set while the sample rate was still 0.
    pub(crate) fn new(model: u32) -> Self {
        let mut cutoff_freq = Box::new([0.0; 2048]);
        let (dc_wave, dc_mixer, dc_voice) = match model {
            1 | 2 => {
                for (i, freq) in cutoff_freq.iter_mut().enumerate() {
                    let x = i as f64 / 8.0;
                    let cf = -0.0156 * x * x + 48.473 * x - 45.074; // 8580R5
                    *freq = if cf <= 0.0 { 0.0 } else { cf };
                }
                (0x800, 0, 0)
            }
            0 => {
                // The 6581 R4.
                for (i, freq) in cutoff_freq.iter_mut().enumerate() {
                    let i = i as f64;
                    *freq = if i < 1024.0 {
                        (((i / 1.5 - 1024.0) / 1024.0 * PI).tanh() + PI.tanh()) * (6000.0 - 220.0)
                            + 220.0
                    } else if i < 1056.0 {
                        let x = (i - 1024.0) / (1056.0 - 1003.);
                        x * (1315.0 - 1003.0) + 1003.0
                    } else {
                        let x = (i - 1056.0) / (2048.0 - 1056.0);
                        (20163.0 - 1315.0) * x + 1315.0
                    };
                }
                (0x380, -454, 0x800 * 0xFF)
            }
            _ => {
                for (i, freq) in cutoff_freq.iter_mut().enumerate() {
                    let i = i as f64;
                    *freq = if i < 1024.0 {
                        (((i - 1024.0) / 1024.0 * PI).tanh() + PI.tanh()) * (6000.0 - 220.0) + 220.0
                    } else {
                        (((i - 2048.0) / 1024.0 * PI).tanh() + PI.tanh()) * (18000.0 - 4600.0)
                            + 4600.0
                    };
                }
                (0x380, -454, 0x800 * 0xFF)
            }
        };
        let delta = f64::from(SID_CLOCK) * 256.0 / f64::from(TED_SOUND_CLOCK);
        Self {
            voices: [Voice::new(); 3],
            volume: 0,
            cycles_per_sample: (delta + 0.5) as u32,
            clock_remainder: 0,
            dc_mixer,
            dc_voice,
            dc_wave,
            filter_type: 0,
            filter_cutoff: 0,
            cutoff_freq,
            // 1024 / (0.707 + 1.9 * 0 / 15) + 0.5.
            resonance: (1024.0 / 0.707 + 0.5) as i32,
            w0: 0,
            vhp: 0,
            vbp: 0,
            vlp: 0,
        }
    }

    fn set_filter_cutoff(&mut self) {
        let coeff = 2.0 * PI * 1.048576;
        let w0 = (self.cutoff_freq[self.filter_cutoff as usize] * coeff) as i32;
        // The Nyquist frequency of the TED clock is above 16 kHz.
        let max = (16000.0 * coeff) as i32;
        self.w0 = w0.min(max);
    }

    fn set_resonance(&mut self, resonance: u8) {
        self.resonance = (1024.0 / (0.707 + 1.9 * f64::from(resonance) / 15.0) + 0.5) as i32;
    }

    fn step(freq: u16) -> u32 {
        (f64::from(freq) * f64::from(SID_CLOCK) * 16.0 / f64::from(TED_SOUND_CLOCK) + 0.5) as u32
    }

    /// Writes register `reg`, 0 to 63 (`SIDsound::write`).
    pub(crate) fn write(&mut self, reg: u32, value: u8) {
        let index = (reg / 7) as usize;
        match reg {
            0 | 7 | 14 => {
                let v = &mut self.voices[index];
                v.freq = (v.freq & 0xff00) | u16::from(value);
                v.add = Self::step(v.freq);
            }
            1 | 8 | 15 => {
                let v = &mut self.voices[index];
                v.freq = (v.freq & 0xff) | (u16::from(value) << 8);
                v.add = Self::step(v.freq);
            }
            2 | 9 | 16 => {
                let v = &mut self.voices[index];
                v.pw = (v.pw & 0x0f00) | u16::from(value);
            }
            3 | 10 | 17 => {
                let v = &mut self.voices[index];
                v.pw = (v.pw & 0xff) | (u16::from(value & 0xf) << 8);
            }
            4 | 11 | 18 => self.control(index, value),
            5 | 12 | 19 => self.voices[index].envelope.set_attack_decay(value),
            6 | 13 | 20 => self.voices[index].envelope.set_sustain_release(value),
            21 => {
                if u32::from(value & 7) != (self.filter_cutoff & 7) {
                    self.filter_cutoff = u32::from(value & 7) | (self.filter_cutoff & 0x7F8);
                    self.set_filter_cutoff();
                }
            }
            22 => {
                self.filter_cutoff = (u32::from(value) << 3) | (self.filter_cutoff & 7);
                self.set_filter_cutoff();
            }
            23 => {
                for (i, voice) in self.voices.iter_mut().enumerate() {
                    voice.filter = u32::from(value) & (1 << i);
                }
                self.set_resonance(value >> 4);
            }
            24 => {
                self.volume = i32::from(value & 0x0F);
                self.voices[2].muted = u32::from(value & 0x80);
                self.filter_type = (value >> 4) & 7;
            }
            // The DigiBlaster registers (30, 31) are off, as in SoLoud.
            _ => {}
        }
    }

    /// A voice's control register: gate, sync, ring, test and waveform.
    fn control(&mut self, index: usize, value: u8) {
        let value = u32::from(value);
        self.voices[(index + 2) % 3].sync = value & 2;
        let v = &mut self.voices[index];
        v.envelope.set_gate(value & 1);
        v.ring = value & 4;
        if value & 8 != 0 && v.test == 0 {
            v.accu = 0;
            let bit19 = (v.shift_reg >> 19) & 1;
            v.shift_reg = (v.shift_reg & 0x7f_fffd) | ((bit19 ^ 1) << 1);
            v.test = 0xFFF;
        } else if v.test != 0 && value & 8 == 0 {
            let bit0 = ((v.shift_reg >> 22) ^ (v.shift_reg >> 17)) & 1;
            v.shift_reg = ((v.shift_reg << 1) & 0x7f_ffff) | bit0;
            v.test = 0;
        }
        v.wave = (value >> 4) & 0x0F;
        if v.wave > 8 {
            v.shift_reg &= 0x7f_ffff
                ^ (1 << 22)
                ^ (1 << 20)
                ^ (1 << 16)
                ^ (1 << 13)
                ^ (1 << 11)
                ^ (1 << 7)
                ^ (1 << 4)
                ^ (1 << 2);
        }
    }

    fn triangle(&self, index: usize) -> u32 {
        let v = &self.voices[index];
        let accu = if v.ring != 0 {
            v.accu ^ self.voices[(index + 2) % 3].accu
        } else {
            v.accu
        };
        let value = if accu & 0x800_0000 != 0 {
            !v.accu
        } else {
            v.accu
        };
        (value >> 15) & 0xFFE
    }

    fn wave_sample(&self, index: usize) -> u32 {
        let v = &self.voices[index];
        let saw = || (v.accu >> 16) & 0xFFF;
        let pulse = || {
            v.test
                | if (v.accu >> 16) >= u32::from(v.pw) {
                    0xFFF
                } else {
                    0
                }
        };
        let pair = |sm: u32| (sm >> 1) & (sm << 1);
        match v.wave {
            1 => self.triangle(index),
            2 => saw(),
            3 => pair(self.triangle(index) & saw()),
            4 => pulse(),
            5 => pair(self.triangle(index) & pulse()),
            6 => saw() & pulse(),
            7 => pair(self.triangle(index) & saw() & pulse()),
            8 => {
                let s = v.shift_reg;
                (bit(s, 22) << 11)
                    | (bit(s, 20) << 10)
                    | (bit(s, 16) << 9)
                    | (bit(s, 13) << 8)
                    | (bit(s, 11) << 7)
                    | (bit(s, 7) << 6)
                    | (bit(s, 4) << 5)
                    | (bit(s, 2) << 4)
            }
            _ => 0x800,
        }
    }

    /// SID cycles this sample, carrying the fraction.
    fn clock(&mut self) -> u32 {
        let mut count = self.cycles_per_sample >> 8;
        let sum = self.clock_remainder + (self.cycles_per_sample & 0xFF);
        if sum >= 0x100 {
            count += 1;
        }
        self.clock_remainder = sum & 0xFF;
        count
    }

    fn filter_output(&mut self, cycles: u32, vi: i32) -> i32 {
        let dt = self.w0 >> 6;
        let vi = vi >> 7;
        for _ in 0..cycles.max(1) {
            self.vlp = self.vlp.wrapping_sub(dt.wrapping_mul(self.vbp) >> 14);
            self.vbp = self.vbp.wrapping_sub(dt.wrapping_mul(self.vhp) >> 14);
            self.vhp = (self.vbp.wrapping_mul(self.resonance) >> 10)
                .wrapping_sub(self.vlp)
                .wrapping_sub(vi);
        }
        let (lp, bp, hp) = (self.vlp, self.vbp, self.vhp);
        let vf = match self.filter_type {
            1 => lp,
            2 => bp,
            3 => lp.wrapping_add(bp),
            4 => hp,
            5 => lp.wrapping_add(hp),
            6 => bp.wrapping_add(hp),
            7 => lp.wrapping_add(bp).wrapping_add(hp),
            _ => 0,
        };
        vf << 7
    }

    /// One sample (`SIDsound::calcSamples` of 1).
    pub(crate) fn sample(&mut self) -> i16 {
        let mut filtered = 0_i32;
        let mut direct = 0_i32;
        let cycles = self.clock();
        for index in [2, 1, 0] {
            let envelope = self.voices[index].envelope.run(cycles) as i32;
            let v = &mut self.voices[index];
            if v.test == 0 {
                let mut prev = v.accu;
                v.accu = v.accu.wrapping_add(v.add);
                let synced = v.sync != 0 && prev & 0x800_0000 == 0 && v.accu & 0x800_0000 != 0;
                if v.freq != 0 {
                    let mut next = prev;
                    let step = u32::from(v.freq) << 4;
                    loop {
                        next = next.wrapping_add(step);
                        // The noise register moves whatever the waveform.
                        if prev & 0x80_0000 == 0 && next & 0x80_0000 != 0 {
                            v.update_shift_reg();
                        }
                        prev = next;
                        if next >= v.accu {
                            break;
                        }
                    }
                }
                v.accu &= 0xFFF_FFFF;
                if synced {
                    self.voices[(index + 1) % 3].accu = 0;
                }
            }
            let output = self.wave_sample(index) as i32;
            let v = &self.voices[index];
            let level = if v.filter == 0 && v.muted != 0 {
                0x0800
            } else {
                output
            };
            let value = (level - self.dc_wave) * envelope + self.dc_voice;
            if v.filter != 0 {
                filtered += value;
            } else {
                direct += value;
            }
        }
        let mixed = direct
            .wrapping_add(self.filter_output(cycles, filtered))
            .wrapping_add(self.dc_mixer);
        (mixed.wrapping_mul(self.volume) >> 12) as i16
    }
}
