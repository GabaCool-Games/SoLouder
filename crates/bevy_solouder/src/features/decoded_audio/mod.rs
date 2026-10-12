//! Audio assets: decoded PCM loaded through the asset server.

use bevy::asset::io::Reader;
use bevy::asset::{Asset, AssetApp, AssetLoader, LoadContext};
use bevy::prelude::*;
use solouder_core::Pcm;
use solouder_decoders::{DecodeError, DecodeLimits, decode_wav};

/// Immutable decoded audio, shared by every entity that plays it.
///
/// Playing a clip shares its samples with the voice, so removing or
/// reloading the asset never affects audio already playing.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct AudioClip {
    pcm: Pcm,
}

impl AudioClip {
    /// Wraps decoded or generated audio.
    pub fn new(pcm: Pcm) -> Self {
        Self { pcm }
    }

    /// The samples.
    pub fn pcm(&self) -> &Pcm {
        &self.pcm
    }

    /// Length in seconds at the clip's own sample rate.
    pub fn duration_seconds(&self) -> f64 {
        f64::from(self.pcm.frames()) / f64::from(self.pcm.sample_rate())
    }
}

/// Why a clip failed to load.
#[derive(Debug)]
#[non_exhaustive]
pub enum AudioLoadError {
    /// The asset reader failed.
    Io(std::io::Error),
    /// The bytes are not decodable audio.
    Decode(DecodeError),
}

impl std::fmt::Display for AudioLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not read audio: {error}"),
            Self::Decode(error) => write!(f, "could not decode audio: {error}"),
        }
    }
}

impl std::error::Error for AudioLoadError {}

/// Loads `.wav` files into [`AudioClip`]s, decoding on the asset task pool.
#[derive(TypePath, Clone, Copy, Debug, Default)]
pub struct WavLoader {
    limits: DecodeLimits,
}

impl WavLoader {
    /// A loader that refuses files decoding to more samples than `limits`
    /// allows.
    pub fn new(limits: DecodeLimits) -> Self {
        Self { limits }
    }
}

impl AssetLoader for WavLoader {
    type Asset = AudioClip;
    type Settings = ();
    type Error = AudioLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<AudioClip, AudioLoadError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(AudioLoadError::Io)?;
        let pcm = decode_wav(&bytes, &self.limits).map_err(AudioLoadError::Decode)?;
        Ok(AudioClip::new(pcm))
    }

    fn extensions(&self) -> &[&str] {
        &["wav"]
    }
}

pub(crate) fn build(app: &mut App, limits: DecodeLimits) {
    app.init_asset::<AudioClip>()
        .register_asset_loader(WavLoader::new(limits));
}
