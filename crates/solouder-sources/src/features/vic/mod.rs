// Ported from SoLoud src/audiosource/vic/soloud_vic.cpp (zlib/libpng,
// Copyright (c) 2015 Jari Komppa); altered for SoLouder. Its notice:
//
//   VIC 6560/6561 sound chip emulator
//   Copyright (c) 2015 Petri Hakkinen
//
//   This software is provided 'as-is', without any express or implied
//   warranty. In no event will the authors be held liable for any damages
//   arising from the use of this software.
//
//   Permission is granted to anyone to use this software for any purpose,
//   including commercial applications, and to alter it and redistribute it
//   freely, subject to the following restrictions:
//
//   1. The origin of this software must not be misrepresented; you must not
//   claim that you wrote the original software. If you use this software
//   in a product, an acknowledgment in the product documentation would be
//   appreciated but is not required.
//
//   2. Altered source versions must be plainly marked as such, and must not be
//   misrepresented as being the original software.
//
//   3. This notice may not be removed or altered from any source
//   distribution.

//! The Commodore VIC-20's sound chip (SoLoud's `Vic`).

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use solouder_core::{Generator, GeneratorContext, GeneratorInstance};

/// Registers of a [`Vic`]: three square-wave oscillators and noise.
pub const VIC_REGISTERS: usize = 4;

/// The VIC model, which sets the oscillators' base clocks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VicModel {
    /// The 6561 (European).
    #[default]
    Pal,
    /// The 6560 (North American).
    Ntsc,
}

impl VicModel {
    /// SoLoud's model index: 0 PAL, 1 NTSC.
    pub fn index(self) -> u32 {
        match self {
            Self::Pal => 0,
            Self::Ntsc => 1,
        }
    }

    /// The model of a SoLoud index, if there is one.
    pub fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Pal),
            1 => Some(Self::Ntsc),
            _ => None,
        }
    }

    /// Base clock of each oscillator, in Hz.
    fn clocks(self) -> [f32; VIC_REGISTERS] {
        match self {
            Self::Pal => [4329.0, 8659.0, 17320.0, 34640.0],
            Self::Ntsc => [3995.0, 7990.0, 15980.0, 31960.0],
        }
    }
}

/// A register index past the last ([`VIC_REGISTERS`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisterOutOfRange(pub usize);

impl fmt::Display for RegisterOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "register {} is out of range (expected below {VIC_REGISTERS})",
            self.0
        )
    }
}

impl std::error::Error for RegisterOutOfRange {}

/// Bytes of the noise table.
const NOISE_LEN: usize = 8192;

/// The noise waveform: a 16-bit Galois LFSR seeded `0xACE1`, each byte the
/// XOR of its two halves.
static NOISE: [u8; NOISE_LEN] = noise_table();

const fn noise_table() -> [u8; NOISE_LEN] {
    let mut table = [0; NOISE_LEN];
    let mut lfsr: u16 = 0xACE1;
    let mut i = 0;
    while i < NOISE_LEN {
        let lsb = lfsr & 1;
        lfsr >>= 1;
        lfsr ^= 0u16.wrapping_sub(lsb) & 0xB400;
        table[i] = ((lfsr & 0xff) ^ (lfsr >> 8)) as u8;
        i += 1;
    }
    table
}

#[derive(Debug)]
struct Registers {
    regs: [AtomicU8; VIC_REGISTERS],
    model: AtomicU8,
}

/// A VIC-20 sound chip (SoLoud's `Vic`), mono at 44100 Hz: bass, alto and
/// soprano square waves with 7-bit frequencies, and noise. A register's
/// top bit turns its oscillator on. The chip has no per-channel volume;
/// use the source's. SoLoud suggests a biquad lowpass at 1500 Hz with
/// resonance 2 for the authentic sound.
///
/// Registers and the model may change at any time: playing voices read
/// them at the start of every block, as in SoLoud. Phase steps assume
/// 44100 Hz, whatever rate the voice plays at.
#[derive(Clone, Debug)]
pub struct Vic {
    registers: Arc<Registers>,
}

impl Default for Vic {
    fn default() -> Self {
        Self::new()
    }
}

impl Vic {
    /// The bass oscillator's register.
    pub const BASS: usize = 0;
    /// The alto oscillator's register.
    pub const ALTO: usize = 1;
    /// The soprano oscillator's register.
    pub const SOPRANO: usize = 2;
    /// The noise register.
    pub const NOISE: usize = 3;

    /// A PAL chip with every register 0 (silent).
    pub fn new() -> Self {
        Self {
            registers: Arc::new(Registers {
                regs: std::array::from_fn(|_| AtomicU8::new(0)),
                model: AtomicU8::new(VicModel::Pal.index() as u8),
            }),
        }
    }

    /// The model (`Vic::getModel`).
    pub fn model(&self) -> VicModel {
        model_of(&self.registers)
    }

    /// Sets the model (`Vic::setModel`).
    pub fn set_model(&self, model: VicModel) {
        self.registers
            .model
            .store(model.index() as u8, Ordering::Relaxed);
    }

    /// A register's value (`Vic::getRegister`), or `None` past the last
    /// (SoLoud read past its registers; reference patch 0064 reads 0).
    pub fn register(&self, index: usize) -> Option<u8> {
        let reg = self.registers.regs.get(index)?;
        Some(reg.load(Ordering::Relaxed))
    }

    /// Sets a register (`Vic::setRegister`); an index past the last is an
    /// error (SoLoud wrote past its registers; reference patch 0064 ignores
    /// it).
    pub fn set_register(&self, index: usize, value: u8) -> Result<(), RegisterOutOfRange> {
        let reg = self
            .registers
            .regs
            .get(index)
            .ok_or(RegisterOutOfRange(index))?;
        reg.store(value, Ordering::Relaxed);
        Ok(())
    }
}

fn model_of(registers: &Registers) -> VicModel {
    let index = registers.model.load(Ordering::Relaxed);
    VicModel::from_index(u32::from(index)).unwrap_or_default()
}

impl Generator for Vic {
    fn channels(&self) -> u32 {
        1
    }

    fn sample_rate(&self) -> f32 {
        44100.0
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        Box::new(VicInstance {
            registers: Arc::clone(&self.registers),
            phase: [0; VIC_REGISTERS],
            noise_pos: 0,
        })
    }
}

/// One voice of a [`Vic`] (`VicInstance`).
#[derive(Debug)]
struct VicInstance {
    registers: Arc<Registers>,
    phase: [u32; VIC_REGISTERS],
    noise_pos: usize,
}

impl VicInstance {
    /// Each oscillator's phase step for this block; 0 when it is off.
    fn phase_steps(&self) -> [u32; VIC_REGISTERS] {
        let clocks = model_of(&self.registers).clocks();
        std::array::from_fn(|i| {
            let reg = self.registers.regs[i].load(Ordering::Relaxed);
            if reg < 128 {
                return 0;
            }
            let divisor = if reg < 255 { 255 - reg } else { 1 };
            let freq = clocks[i] / f32::from(divisor);
            // At most 34640 * 65536 / 44100 + 0.5, well inside `u32`.
            (freq * 65536.0 / 44100.0 + 0.5) as u32
        })
    }
}

impl GeneratorInstance for VicInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        _: usize,
        _: &GeneratorContext,
    ) -> usize {
        let steps = self.phase_steps();
        for sample in &mut out[..frames] {
            let mut s = 0.0_f32;
            for (phase, &step) in self.phase.iter_mut().zip(&steps).take(3) {
                if step != 0 {
                    s += if *phase < 32768 { 0.5 } else { -0.5 };
                    *phase = (*phase + step) & 65535;
                }
            }
            if steps[3] != 0 {
                s += f32::from(NOISE[self.noise_pos]) / 255.0 - 0.5;
                self.phase[3] += steps[3];
                if self.phase[3] >= 32768 {
                    self.noise_pos = (self.noise_pos + 1) & (NOISE_LEN - 1);
                    self.phase[3] &= 32767;
                }
            }
            *sample = s / 4.0;
        }
        frames
    }

    fn has_ended(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
