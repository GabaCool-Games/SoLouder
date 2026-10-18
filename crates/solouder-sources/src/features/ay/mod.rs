// Ported from SoLoud src/audiosource/ay/soloud_ay.cpp (zlib/libpng,
// Copyright (c) 2020 Jari Komppa); altered for SoLouder. The AY emulation
// is based on the public domain zxssk library, "created under public
// domain license by SMT, jan.2006".

//! Register dumps of the AY-3-8910 and YM2149F sound chips, alone or as
//! two (TurboSound) (SoLoud's `Ay`).

mod chip;
mod player;
mod render;

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use solouder_core::{Generator, GeneratorContext, GeneratorInstance};

use crate::chipdump::{ChipDump, ChipDumpError};
use player::{Player, ticks_per_frame};

/// Chip types Ay plays: an AY or YM, TurboSound and TurboSound Next.
const CHIPS: [u8; 3] = [1, 2, 3];

/// SoLoud's defaults, used without a dump.
const DEFAULT_CPU_SPEED: u32 = 50;
const DEFAULT_CHIP_SPEED: u32 = 1_774_400;

/// The slowest chip clock: the chip ticks at its clock / 8, at least as
/// often as samples are made (reference patch 0095).
const MIN_CHIP_SPEED: u32 = 8 * 44100;

/// An AY or YM register dump (SoLoud's `Ay`), stereo at 44100 Hz, made by
/// SoLoud's dump tools. Dumps play once and end; looping voices start
/// again from the beginning (the dump's loop position is read but, as in
/// SoLoud, not used).
///
/// [`solouder_core::Mixer::info`] on a voice reports the first chip's
/// register `key` for keys 0 to 13, and the second chip's register
/// `key & 15` for keys with bit 4 set whose low bits are 0 to 13; other
/// keys report 0.
#[derive(Clone, Debug, Default)]
pub struct Ay {
    dump: Option<Arc<ChipDump>>,
}

impl Ay {
    /// No dump: voices end at once.
    pub fn new() -> Self {
        Self::default()
    }

    /// A dump from its bytes (`Ay::loadMem`).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChipDumpError> {
        Self::with_dump(ChipDump::parse(bytes, &CHIPS)?)
    }

    fn with_dump(dump: ChipDump) -> Result<Self, ChipDumpError> {
        if dump.chip_speed < MIN_CHIP_SPEED || ticks_per_frame(dump.cpu_speed) == 0 {
            return Err(ChipDumpError::UnsupportedSpeed);
        }
        Ok(Self {
            dump: Some(Arc::new(dump)),
        })
    }

    /// A dump read from `reader` to its end, at most 80 MiB
    /// (`Ay::loadFile`).
    pub fn from_reader(reader: impl Read) -> Result<Self, ChipDumpError> {
        Self::with_dump(ChipDump::read(reader, &CHIPS)?)
    }

    /// A dump from a file (`Ay::load`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ChipDumpError> {
        let file = std::fs::File::open(path).map_err(ChipDumpError::Io)?;
        Self::from_reader(file)
    }

    /// The dump's chip type, if there is a dump: 1 for one chip, 2 for
    /// TurboSound and 3 for TurboSound Next (played alike: register writes
    /// with bit 4 set go to the second chip).
    pub fn chip_type(&self) -> Option<u8> {
        self.dump.as_ref().map(|dump| dump.chip)
    }

    /// Whether the dump is for YM2149F chips (flag 64) rather than
    /// AY-3-8910s, if there is a dump.
    pub fn is_ym(&self) -> Option<bool> {
        self.dump.as_ref().map(|dump| dump.flags & 64 != 0)
    }

    /// The dump's tick rate and chip clock in Hz, if there is a dump.
    pub fn speeds(&self) -> Option<(u32, u32)> {
        self.dump
            .as_ref()
            .map(|dump| (dump.cpu_speed, dump.chip_speed))
    }
}

impl Generator for Ay {
    fn channels(&self) -> u32 {
        2
    }

    fn sample_rate(&self) -> f32 {
        44100.0
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        let (cpu, chip) = self
            .speeds()
            .unwrap_or((DEFAULT_CPU_SPEED, DEFAULT_CHIP_SPEED));
        Box::new(AyInstance {
            dump: self.dump.clone(),
            player: Player::new(cpu, chip, self.is_ym().unwrap_or(false)),
            pos: 0,
        })
    }
}

/// One voice of an [`Ay`] (`AyInstance`).
#[derive(Debug)]
struct AyInstance {
    dump: Option<Arc<ChipDump>>,
    player: Player,
    /// The byte position of the next operation.
    pos: usize,
}

impl AyInstance {
    fn length(&self) -> usize {
        self.dump.as_ref().map_or(0, |dump| dump.length)
    }
}

impl GeneratorInstance for AyInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        stride: usize,
        _: &GeneratorContext,
    ) -> usize {
        let dump = self.dump.as_deref();
        self.player.play(out, frames, stride, dump, &mut self.pos)
    }

    /// Once the operations are read, any wait has played and the ring is
    /// empty (reference patch 0098).
    fn has_ended(&self) -> bool {
        self.pos >= self.length() && self.player.pending_ticks == 0 && self.player.buffered() == 0
    }

    /// Starts the chips again, as a new voice does (reference patch 0099).
    fn rewind(&mut self) -> bool {
        self.player.reset();
        self.pos = 0;
        true
    }

    fn info(&self, key: u32) -> f32 {
        if key & 0xF > 13 {
            return 0.0;
        }
        if key < 0x10 {
            f32::from(self.player.chip.reg[key as usize])
        } else if key & 0x10 != 0 {
            f32::from(self.player.chip2.reg[(key & 0xF) as usize])
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests;
