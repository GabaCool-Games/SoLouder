//! Loaders for the files SoLoud's generators read: sfxr parameter files,
//! Monotone songs and chip register dumps, each into a generated
//! [`AudioClip`].

use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetApp, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use solouder_core::dsp::waveform;
use solouder_core::{Generator, Synth, SynthError};
use solouder_sources::{Ay, ChipDumpError, Monotone, MonotoneError, Sfxr, SfxrError, TedSid};

use crate::features::decoded_audio::AudioClip;

/// Why a generator's file failed to load.
#[derive(Debug)]
#[non_exhaustive]
pub enum GeneratorLoadError {
    /// The asset reader failed.
    Io(std::io::Error),
    /// Not an sfxr parameter file.
    Sfxr(SfxrError),
    /// Not a Monotone song, or settings it cannot use.
    Monotone(MonotoneError),
    /// Not a chip dump that `TedSid` or `Ay` plays.
    ChipDump(ChipDumpError),
    /// The generator cannot be played.
    Synth(SynthError),
}

impl std::fmt::Display for GeneratorLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not read the file: {error}"),
            Self::Sfxr(error) => write!(f, "could not load the sfxr file: {error}"),
            Self::Monotone(error) => write!(f, "could not load the Monotone song: {error}"),
            Self::ChipDump(error) => write!(f, "could not load the chip dump: {error}"),
            Self::Synth(error) => write!(f, "could not play the generator: {error}"),
        }
    }
}

impl std::error::Error for GeneratorLoadError {}

async fn read_all(reader: &mut dyn Reader) -> Result<Vec<u8>, GeneratorLoadError> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(GeneratorLoadError::Io)?;
    Ok(bytes)
}

fn clip(generator: Arc<dyn Generator>) -> Result<AudioClip, GeneratorLoadError> {
    Synth::new(generator)
        .map(AudioClip::generated)
        .map_err(GeneratorLoadError::Synth)
}

/// Loads sfxr parameter files (`.sfx`, `.sfs`; `Sfxr::loadParams`).
#[derive(TypePath, Clone, Copy, Debug, Default)]
pub struct SfxrLoader;

impl AssetLoader for SfxrLoader {
    type Asset = AudioClip;
    type Settings = ();
    type Error = GeneratorLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<AudioClip, GeneratorLoadError> {
        let bytes = read_all(reader).await?;
        let sfxr = Sfxr::from_bytes(&bytes).map_err(GeneratorLoadError::Sfxr)?;
        clip(Arc::new(sfxr))
    }

    fn extensions(&self) -> &[&str] {
        &["sfx", "sfs"]
    }
}

/// How [`MonotoneLoader`] sets up a song (`Monotone::setParams`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonotoneSettings {
    /// The hardware channels the tracks share, 1 to 12.
    pub hardware_channels: u32,
    /// The waveform, one of `solouder_core::dsp::waveform`'s.
    pub waveform: i32,
}

impl Default for MonotoneSettings {
    /// One channel of square waves, as a new `Monotone`.
    fn default() -> Self {
        Self {
            hardware_channels: 1,
            waveform: waveform::SQUARE,
        }
    }
}

/// Loads Monotone songs (`.mon`; `Monotone::loadFile`).
#[derive(TypePath, Clone, Copy, Debug, Default)]
pub struct MonotoneLoader;

impl AssetLoader for MonotoneLoader {
    type Asset = AudioClip;
    type Settings = MonotoneSettings;
    type Error = GeneratorLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &MonotoneSettings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<AudioClip, GeneratorLoadError> {
        let bytes = read_all(reader).await?;
        let song = Monotone::from_bytes(&bytes).map_err(GeneratorLoadError::Monotone)?;
        song.set_params(settings.hardware_channels, settings.waveform)
            .map_err(GeneratorLoadError::Monotone)?;
        clip(Arc::new(song))
    }

    fn extensions(&self) -> &[&str] {
        &["mon"]
    }
}

/// Loads chip register dumps (`.dump`): SID and TED dumps play through
/// `TedSid`, AY-3-8910 and YM2149F dumps through `Ay`, chosen by the dump's
/// chip type.
#[derive(TypePath, Clone, Copy, Debug, Default)]
pub struct ChipDumpLoader;

/// The generator for a dump's bytes.
fn chip_dump(bytes: &[u8]) -> Result<Arc<dyn Generator>, ChipDumpError> {
    match TedSid::from_bytes(bytes) {
        Ok(tedsid) => Ok(Arc::new(tedsid)),
        Err(ChipDumpError::UnsupportedChip(_)) => Ok(Arc::new(Ay::from_bytes(bytes)?)),
        Err(error) => Err(error),
    }
}

impl AssetLoader for ChipDumpLoader {
    type Asset = AudioClip;
    type Settings = ();
    type Error = GeneratorLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<AudioClip, GeneratorLoadError> {
        let bytes = read_all(reader).await?;
        clip(chip_dump(&bytes).map_err(GeneratorLoadError::ChipDump)?)
    }

    fn extensions(&self) -> &[&str] {
        &["dump"]
    }
}

pub(crate) fn build(app: &mut App) {
    app.register_asset_loader(SfxrLoader)
        .register_asset_loader(MonotoneLoader)
        .register_asset_loader(ChipDumpLoader);
}
