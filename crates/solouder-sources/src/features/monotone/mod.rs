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

/// The waveform a Monotone song plays (SoLoud's `Soloud::WAVEFORM`, the
/// values of `solouder_core::dsp::waveform`). [`Monotone::set_params_raw`]
/// takes SoLoud's `int`; `i32::from` gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MonotoneWaveform {
    /// Square wave.
    #[default]
    Square,
    /// Rising saw.
    Saw,
    /// Sine.
    Sin,
    /// Triangle.
    Triangle,
    /// Rectified sine.
    Bounce,
    /// The first quarter of a sine.
    Jaws,
    /// The first half of a sine.
    Humps,
    /// Square from the first 11 odd harmonics.
    FSquare,
    /// Saw from the first 14 harmonics.
    FSaw,
}

impl MonotoneWaveform {
    const ALL: [Self; 9] = [
        Self::Square,
        Self::Saw,
        Self::Sin,
        Self::Triangle,
        Self::Bounce,
        Self::Jaws,
        Self::Humps,
        Self::FSquare,
        Self::FSaw,
    ];

    /// The waveform of SoLoud's value, 0 to 8.
    pub fn from_i32(value: i32) -> Option<Self> {
        usize::try_from(value)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
    }
}

impl From<MonotoneWaveform> for i32 {
    fn from(wave: MonotoneWaveform) -> Self {
        wave as i32
    }
}

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
/// each at the start of every block, as in SoLoud. Clones share them.
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

    /// The most bytes a song is read up to (392031); the rest of a file is
    /// not read.
    pub const MAX_FILE_BYTES: u64 = song::MAX_FILE;

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

    /// The waveform voices play: a value past 8, set with
    /// [`Monotone::set_params_raw`], plays square waves.
    pub fn waveform(&self) -> MonotoneWaveform {
        MonotoneWaveform::from_i32(self.waveform_raw()).unwrap_or_default()
    }

    /// The waveform as SoLoud's `int`, as set.
    pub fn waveform_raw(&self) -> i32 {
        self.live.waveform.load(Ordering::Relaxed)
    }

    /// Sets the hardware channels, 1 to [`MONOTONE_CHANNELS`], and the
    /// waveform (`Monotone::setParams`). Out of range, nothing changes
    /// (above 12 channels is reference patch 0078; SoLoud overran its
    /// channels).
    ///
    /// The two settings are stored one after the other, as SoLoud's two
    /// fields are: a block that starts while this runs on another thread
    /// can see the new channels with the old waveform.
    pub fn set_params(
        &self,
        hardware_channels: u32,
        waveform: MonotoneWaveform,
    ) -> Result<(), MonotoneError> {
        self.set_params_raw(hardware_channels, waveform.into())
    }

    /// [`Monotone::set_params`] with SoLoud's `int` waveform, 0 or more;
    /// values past 8 play square waves.
    pub fn set_params_raw(
        &self,
        hardware_channels: u32,
        waveform: i32,
    ) -> Result<(), MonotoneError> {
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
