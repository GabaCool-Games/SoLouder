// Ported from SoLoud src/audiosource/monotone/soloud_monotone.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Songs from the Monotone PC-speaker tracker (SoLoud's `Monotone`).

mod instance;
mod song;

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use solouder_core::dsp::waveform;
use solouder_core::{Generator, GeneratorInstance};

use instance::MonotoneInstance;
pub use song::{MONOTONE_CHANNELS, MonotoneError, MonotoneSong};

/// Settings voices read at the start of every block.
#[derive(Debug)]
struct Live {
    hardware_channels: AtomicU32,
    waveform: AtomicI32,
}

/// A Monotone song (SoLoud's `Monotone`), mono at 44100 Hz. Each tick is a
/// 60th of a second of the voice's rate, so play speed changes the tempo
/// as well as the pitch. Songs never end: they start again after the last
/// position of their order list. Seeking back is not possible, as in
/// SoLoud (voices cannot rewind).
///
/// Tracks share the hardware channels, 1 at first: with more active tracks
/// than channels they take turns a tick each, as on a PC speaker. The
/// channels and the waveform may change while voices play; voices read
/// them at the start of every block, as in SoLoud. Clones share them.
#[derive(Clone, Debug)]
pub struct Monotone {
    song: Arc<MonotoneSong>,
    live: Arc<Live>,
}

impl Default for Monotone {
    fn default() -> Self {
        Self::new()
    }
}

impl Monotone {
    /// An empty song, which plays silence, with one hardware channel and
    /// square waves.
    pub fn new() -> Self {
        Self::with_song(MonotoneSong::default())
    }

    fn with_song(song: MonotoneSong) -> Self {
        Self {
            song: Arc::new(song),
            live: Arc::new(Live {
                hardware_channels: AtomicU32::new(1),
                waveform: AtomicI32::new(waveform::SQUARE),
            }),
        }
    }

    /// A song from a file's bytes (`Monotone::loadMem`).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MonotoneError> {
        Self::from_reader(bytes)
    }

    /// A song read from `reader` (`Monotone::loadFile`), which is read up
    /// to the song's last cell.
    pub fn from_reader(reader: impl Read) -> Result<Self, MonotoneError> {
        MonotoneSong::read(reader).map(Self::with_song)
    }

    /// A song from a file (`Monotone::load`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MonotoneError> {
        let file = std::fs::File::open(path).map_err(MonotoneError::Io)?;
        Self::from_reader(std::io::BufReader::new(file))
    }

    /// The song.
    pub fn song(&self) -> &MonotoneSong {
        &self.song
    }

    /// The hardware channels, 1 to [`MONOTONE_CHANNELS`].
    pub fn hardware_channels(&self) -> u32 {
        self.live.hardware_channels.load(Ordering::Relaxed)
    }

    /// The waveform, one of `solouder_core::dsp::waveform`'s (other values
    /// play square waves).
    pub fn waveform(&self) -> i32 {
        self.live.waveform.load(Ordering::Relaxed)
    }

    /// Sets the hardware channels, 1 to [`MONOTONE_CHANNELS`], and the waveform,
    /// 0 or more (`Monotone::setParams`). Out of range, nothing changes
    /// (above 12 channels is reference patch 0078; SoLoud overran its
    /// channels).
    pub fn set_params(&self, hardware_channels: u32, waveform: i32) -> Result<(), MonotoneError> {
        if !(1..=MONOTONE_CHANNELS).contains(&hardware_channels) || waveform < 0 {
            return Err(MonotoneError::InvalidParams {
                hardware_channels,
                waveform,
            });
        }
        self.live
            .hardware_channels
            .store(hardware_channels, Ordering::Relaxed);
        self.live.waveform.store(waveform, Ordering::Relaxed);
        Ok(())
    }
}

impl Generator for Monotone {
    fn channels(&self) -> u32 {
        1
    }

    fn sample_rate(&self) -> f32 {
        44100.0
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        Box::new(MonotoneInstance::new(
            Arc::clone(&self.song),
            Arc::clone(&self.live),
        ))
    }
}

#[cfg(test)]
mod tests;
