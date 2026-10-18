// Ported from SoLoud src/audiosource/tedsid/soloud_tedsid.cpp (zlib/libpng,
// Copyright (c) 2015-2020 Jari Komppa); altered for SoLouder. The TED and
// SID emulation is based on tedplay, (c) 2012 Attila Grosz, used under the
// Unlicense (http://unlicense.org/).

//! Register dumps of the Commodore 64's SID and the Plus/4's TED (SoLoud's
//! `TedSid`).

mod envelope;
mod sid;
mod ted;

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use solouder_core::{Generator, GeneratorContext, GeneratorInstance};

use crate::chipdump::ChipDump;
use crate::chipdump::ChipDumpError;
use sid::{Sid, TED_SOUND_CLOCK};
use ted::Ted;

/// Chip types TedSid plays: SID, TED, and both.
const CHIPS: [u8; 4] = [0, 4, 5, 6];

/// The SID revision a dump asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SidModel {
    /// The original 6581 (revision 4).
    #[default]
    Mos6581,
    /// The 8580.
    Mos8580,
    /// The 8580 with the digi boost.
    Mos8580DigiBoost,
    /// The first 6581 revision.
    Mos6581R1,
}

impl SidModel {
    /// The model in bits 4 and 5 of a dump's flags (reference patch 0076;
    /// SoLoud read the source's own 3D flags).
    fn from_flags(flags: u8) -> Self {
        match flags & 0x30 {
            0x00 => Self::Mos6581,
            0x10 => Self::Mos8580,
            0x20 => Self::Mos8580DigiBoost,
            _ => Self::Mos6581R1,
        }
    }

    fn index(self) -> u32 {
        self as u32
    }
}

/// A TED and SID register dump (SoLoud's `TedSid`), mono at the TED's
/// sound clock of 221680 Hz, made from C64 and Plus/4 tunes by SoLoud's
/// `tedsid2dump` tool. Dumps loop forever from their loop position and
/// never end; voices cannot seek back.
///
/// [`solouder_core::Mixer::info`] on a voice reports register `key & 127`
/// as last written: the SID's are 0 to 31 and the TED's 64 to 68.
#[derive(Clone, Debug, Default)]
pub struct TedSid {
    dump: Option<Arc<ChipDump>>,
}

impl TedSid {
    /// No dump: voices play silence.
    pub fn new() -> Self {
        Self::default()
    }

    /// A dump from its bytes (`TedSid::loadMem`).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChipDumpError> {
        Self::with_dump(ChipDump::parse(bytes, &CHIPS)?)
    }

    fn with_dump(dump: ChipDump) -> Result<Self, ChipDumpError> {
        // The loop must be inside the data (reference patch 0078).
        if dump.loop_pos >= dump.length {
            return Err(ChipDumpError::BadHeader);
        }
        if !dump.loop_waits() {
            return Err(ChipDumpError::NoWait);
        }
        Ok(Self {
            dump: Some(Arc::new(dump)),
        })
    }

    /// A dump read from `reader` to its end, at most 80 MiB
    /// (`TedSid::loadFile`).
    pub fn from_reader(reader: impl Read) -> Result<Self, ChipDumpError> {
        Self::with_dump(ChipDump::read(reader, &CHIPS)?)
    }

    /// A dump from a file (`TedSid::load`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ChipDumpError> {
        let file = std::fs::File::open(path).map_err(ChipDumpError::Io)?;
        Self::from_reader(file)
    }

    /// The SID model of the dump, if there is one.
    pub fn model(&self) -> Option<SidModel> {
        self.dump
            .as_ref()
            .map(|dump| SidModel::from_flags(dump.flags))
    }
}

impl Generator for TedSid {
    fn channels(&self) -> u32 {
        1
    }

    fn sample_rate(&self) -> f32 {
        TED_SOUND_CLOCK as f32
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        let model = self.model().unwrap_or_default();
        Box::new(TedSidInstance {
            dump: self.dump.clone(),
            sid: Sid::new(model.index()),
            ted: Ted::new(),
            pos: 0,
            wait: 0,
            regs: [0; 128],
        })
    }
}

/// One voice of a [`TedSid`] (`TedSidInstance`).
#[derive(Debug)]
struct TedSidInstance {
    dump: Option<Arc<ChipDump>>,
    sid: Sid,
    ted: Ted,
    /// The byte position of the next operation.
    pos: usize,
    /// Samples until the next operations.
    wait: u32,
    regs: [u8; 128],
}

impl TedSidInstance {
    /// Runs operations up to the next wait (`TedSidInstance::tick`). The
    /// loop holds a wait (reference patch 0079), so this returns.
    fn tick(&mut self) {
        let Some(dump) = &self.dump else {
            return;
        };
        while self.wait == 0 {
            let op = dump.op(self.pos);
            self.pos += 2;
            if self.pos >= dump.length {
                self.pos = dump.loop_pos;
            }
            if op & 0x8000 != 0 {
                self.wait = u32::from(op & 0x7fff);
                continue;
            }
            let reg = u32::from(op >> 8);
            let value = op as u8;
            self.regs[reg as usize] = value;
            if reg < 64 {
                self.sid.write(reg, value);
            } else if reg < 64 + 5 {
                self.ted.write(reg - 64, value);
            }
        }
    }
}

impl GeneratorInstance for TedSidInstance {
    fn get_audio(
        &mut self,
        out: &mut [f32],
        frames: usize,
        _: usize,
        _: &GeneratorContext,
    ) -> usize {
        for sample in &mut out[..frames] {
            self.tick();
            let sid = i32::from(self.sid.sample());
            let ted = i32::from(self.ted.sample());
            *sample = (sid + ted) as f32 / 8192.0;
            // Without a dump this wraps, as SoLoud's unsigned count does.
            self.wait = self.wait.wrapping_sub(1);
        }
        frames
    }

    fn has_ended(&self) -> bool {
        false
    }

    fn info(&self, key: u32) -> f32 {
        f32::from(self.regs[(key & 127) as usize])
    }
}

#[cfg(test)]
mod tests;
