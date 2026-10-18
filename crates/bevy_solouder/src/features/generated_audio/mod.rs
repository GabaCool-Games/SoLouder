//! Loaders for the files SoLoud's generators read: sfxr parameter files,
//! Monotone songs and chip register dumps, each into a generated
//! [`AudioClip`].

use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{AssetApp, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use solouder_core::{Generator, Synth, SynthError};
use solouder_sources::{
    Ay, ChipDumpError, Monotone, MonotoneError, MonotoneWaveform, Sfxr, SfxrError, TedSid,
};

use crate::features::decoded_audio::{AudioClip, read_up_to};

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

/// At most the first `limit` bytes of `reader`: as much as the source's own
/// reader takes.
async fn read_prefix(reader: &mut dyn Reader, limit: u64) -> Result<Vec<u8>, GeneratorLoadError> {
    read_up_to(reader, limit)
        .await
        .map_err(GeneratorLoadError::Io)
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
        let bytes = read_prefix(reader, Sfxr::MAX_FILE_BYTES).await?;
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
    /// The waveform as SoLoud's `int` (`i32::from` a [`MonotoneWaveform`]),
    /// 0 or more; values past 8 play square waves.
    pub waveform: i32,
}

impl Default for MonotoneSettings {
    /// One channel of square waves, as a new `Monotone`.
    fn default() -> Self {
        Self {
            hardware_channels: 1,
            waveform: MonotoneWaveform::Square.into(),
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
        let bytes = read_prefix(reader, Monotone::MAX_FILE_BYTES).await?;
        let song = Monotone::from_bytes(&bytes).map_err(GeneratorLoadError::Monotone)?;
        song.set_params_raw(settings.hardware_channels, settings.waveform)
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
///
/// Dumps are untrusted input. Files past 80 MiB are refused unread, and
/// `TedSid` and `Ay` check a dump's structure and refuse those whose
/// operations or chip clock would make a voice's work for a block
/// unbounded (`ChipDumpError::TooManyOperations`, `UnsupportedSpeed`).
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
        let limit = TedSid::MAX_FILE_BYTES.max(Ay::MAX_FILE_BYTES);
        let bytes = read_prefix(reader, limit + 1).await?;
        if bytes.len() as u64 > limit {
            return Err(GeneratorLoadError::ChipDump(ChipDumpError::TooLarge));
        }
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
