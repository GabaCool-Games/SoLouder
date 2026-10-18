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

/// The fastest chip clock (reference patch 0113): SoLoud's dump tools
/// write 1774400 (ZX Spectrum) and 2000000 (Atari ST), and the fastest of
/// these chips takes a 4 MHz input clock (the YM2149, with its divider).
const MAX_CHIP_SPEED: u32 = 4_000_000;

/// The most operations a dump may hold before waits of two frames' ticks
/// (reference patch 0114).
const MAX_OPS: usize = 131_072;

/// The chips a dump is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AyChipType {
    /// One AY-3-8910 or YM2149F (chip type 1).
    Single,
    /// TurboSound, two chips (type 2).
    TurboSound,
    /// TurboSound Next, three chips (type 3), played as two: writes to the
    /// third chip's registers go to the second.
    TurboSoundNext,
}

impl AyChipType {
    fn from_byte(chip: u8) -> Self {
        match chip {
            2 => Self::TurboSound,
            3 => Self::TurboSoundNext,
            _ => Self::Single,
        }
    }

    /// The chip type byte of a dump's header.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Single => 1,
            Self::TurboSound => 2,
            Self::TurboSoundNext => 3,
        }
    }
}

/// An AY or YM register dump (SoLoud's `Ay`), stereo at 44100 Hz, made by
/// SoLoud's dump tools. Dumps play once and end; looping voices start
/// again from the beginning (the dump's loop position is read but, as in
/// SoLoud, not used).
///
/// Dumps are untrusted input: besides their structure and speeds, a dump
/// is refused if a chip clock above 4 MHz or more than 131072 operations
/// before waits of two frames' ticks (about 0.19 s; counting restarts of a
/// looping voice) would make a voice's work for a block unbounded
/// ([`ChipDumpError::UnsupportedSpeed`],
/// [`ChipDumpError::TooManyOperations`]). A Z80 at 3.5 MHz writes at most
/// about 28000 registers in that time.
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
    /// The longest dump read (80 MiB), as for `TedSid`.
    pub const MAX_FILE_BYTES: u64 = crate::chipdump::MAX_FILE;

    /// No dump: voices end at once.
    pub fn new() -> Self {
        Self::default()
    }

    /// A dump from its bytes (`Ay::loadMem`).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChipDumpError> {
        Self::with_dump(ChipDump::parse(bytes, &CHIPS)?)
    }

    fn with_dump(dump: ChipDump) -> Result<Self, ChipDumpError> {
        let frame = ticks_per_frame(dump.cpu_speed);
        if !(MIN_CHIP_SPEED..=MAX_CHIP_SPEED).contains(&dump.chip_speed) || frame == 0 {
            return Err(ChipDumpError::UnsupportedSpeed);
        }
        // A frame plays its operations at once, taking them until its
        // waits reach the frame's ticks (up to twice that after a long
        // wait), and a looping voice restarts within it, unless the dump
        // never waits and a restart plays nothing.
        let waits = (0..dump.length)
            .step_by(2)
            .any(|pos| dump.op(pos) & 0x8000 != 0 && dump.op(pos) & 0x7fff != 0);
        let repeat = waits.then_some(0);
        if dump.most_ops_before_waits(repeat, 2 * u64::from(frame), MAX_OPS) > MAX_OPS {
            return Err(ChipDumpError::TooManyOperations);
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

    /// The dump's chip type, if there is a dump. All play alike: register
    /// writes with bit 4 set go to the second chip.
    pub fn chip_type(&self) -> Option<AyChipType> {
        self.dump
            .as_ref()
            .map(|dump| AyChipType::from_byte(dump.chip))
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
