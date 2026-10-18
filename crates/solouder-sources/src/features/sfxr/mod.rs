// Ported from SoLoud src/audiosource/sfxr/soloud_sfxr.cpp (zlib/libpng,
// Copyright (c) 2014 Jari Komppa), based on sfxr by Tomas Pettersson,
// relicensed under zlib/libpng by permission; altered for SoLouder.

//! Sound effects from sfxr's parameters (SoLoud's `Sfxr`).

mod instance;
mod params;
mod presets;

use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use solouder_core::{Generator, GeneratorInstance};

use crate::Prg;
pub use params::{SfxrError, SfxrParams};
pub use presets::SfxrPreset;

#[derive(Debug)]
struct State {
    params: SfxrParams,
    /// Seeded by every preset load (`Sfxr::mRand`).
    rand: Prg,
}

/// An sfxr sound effect (SoLoud's `Sfxr`), mono at 44100 Hz: a waveform
/// with frequency slides, vibrato, an arpeggio, an envelope, low- and
/// high-pass filters, a phaser and repeats, from a seeded preset or an
/// sfxr parameter file. Share it in an `Arc` and wrap it in a
/// [`Synth`](solouder_core::Synth) to play it.
///
/// Settings changed after a play apply to later voices: each voice copies
/// them when it starts, as in SoLoud. Every voice seeds its noise alike, so
/// voices of the same settings sound alike. A voice ends when its envelope
/// does, or when its frequency slides past `freq_limit`; a looping voice
/// starts over instead, inside the source as in SoLoud, so its loop point
/// is not used.
#[derive(Debug)]
pub struct Sfxr {
    state: Mutex<State>,
}

impl Default for Sfxr {
    fn default() -> Self {
        Self::new()
    }
}

impl Sfxr {
    /// The default settings (`Sfxr::resetParams`).
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                params: SfxrParams::default(),
                rand: Prg::new(0),
            }),
        }
    }

    /// A sound with `preset`'s settings for `seed`
    /// ([`load_preset`](Self::load_preset)).
    pub fn from_preset(preset: SfxrPreset, seed: i32) -> Self {
        let sfxr = Self::new();
        sfxr.load_preset(preset, seed);
        sfxr
    }

    /// A sound with the settings of the parameter file held in `bytes`
    /// ([`load_params`](Self::load_params)).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SfxrError> {
        let sfxr = Self::new();
        sfxr.load_params(bytes)?;
        Ok(sfxr)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // The lock guards plain data, which a panic cannot leave invalid.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The current settings.
    pub fn params(&self) -> SfxrParams {
        self.state().params
    }

    /// Replaces the settings, for later plays.
    pub fn set_params(&self, params: SfxrParams) {
        self.state().params = params;
    }

    /// Restores the default settings (`Sfxr::resetParams`).
    pub fn reset_params(&self) {
        self.set_params(SfxrParams::default());
    }

    /// Sets `preset`'s settings, randomized from `seed`
    /// (`Sfxr::loadPreset`): the defaults, then the preset's changes drawn
    /// from a WELL512 generator seeded with `seed`, so a preset and seed
    /// always give the same sound.
    pub fn load_preset(&self, preset: SfxrPreset, seed: i32) {
        let mut state = self.state();
        state.rand.srand(seed);
        state.params = presets::preset(preset, &mut state.rand);
    }

    /// Sets the settings of an sfxr parameter file held in `bytes`
    /// (`Sfxr::loadParamsMem`); see [`SfxrParams::with_file`] for the
    /// format. On error nothing changes.
    pub fn load_params(&self, bytes: &[u8]) -> Result<(), SfxrError> {
        let mut state = self.state();
        state.params = state.params.with_file(bytes)?;
        Ok(())
    }

    /// The most bytes of a parameter file that are read: version 102's
    /// fields take 105.
    pub const MAX_FILE_BYTES: u64 = params::MAX_FILE;

    /// [`load_params`](Self::load_params) of what `reader` holds
    /// (`Sfxr::loadParamsFile`); at most the first 128 bytes are read.
    pub fn load_params_reader(&self, reader: impl Read) -> Result<(), SfxrError> {
        let mut bytes = Vec::new();
        reader
            .take(params::MAX_FILE)
            .read_to_end(&mut bytes)
            .map_err(SfxrError::Io)?;
        self.load_params(&bytes)
    }

    /// [`load_params`](Self::load_params) of the file at `path`
    /// (`Sfxr::loadParams`).
    pub fn load_params_file(&self, path: impl AsRef<Path>) -> Result<(), SfxrError> {
        let file = std::fs::File::open(path).map_err(SfxrError::Io)?;
        self.load_params_reader(file)
    }
}

impl Generator for Sfxr {
    fn channels(&self) -> u32 {
        1
    }

    fn sample_rate(&self) -> f32 {
        44100.0
    }

    fn instance(&self) -> Box<dyn GeneratorInstance> {
        Box::new(instance::SfxrInstance::new(self.params()))
    }
}

#[cfg(test)]
mod tests;
