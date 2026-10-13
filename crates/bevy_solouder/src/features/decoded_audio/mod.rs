//! Audio assets: decoded PCM, or audio decoded while it plays, loaded
//! through the asset server.

use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{Asset, AssetApp, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use solouder_core::{Pcm, Stream};
use solouder_decoders::{DecodeError, DecodeLimits, WavStream, decode};

/// Immutable audio, shared by every entity that plays it: decoded samples,
/// or a stream that each voice decodes as it plays.
///
/// Playing a decoded clip shares its samples with the voice, and a streamed
/// clip's encoded bytes with its reader, so removing or reloading the asset
/// never affects audio already playing.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct AudioClip {
    audio: ClipAudio,
}

#[derive(Clone, Debug)]
enum ClipAudio {
    Decoded(Pcm),
    Streamed(Stream),
}

impl AudioClip {
    /// Wraps decoded or generated audio.
    pub fn new(pcm: Pcm) -> Self {
        Self {
            audio: ClipAudio::Decoded(pcm),
        }
    }

    /// Wraps a stream, decoded while it plays by the output's stream worker.
    pub fn streamed(stream: Stream) -> Self {
        Self {
            audio: ClipAudio::Streamed(stream),
        }
    }

    /// The samples of a decoded clip.
    pub fn pcm(&self) -> Option<&Pcm> {
        match &self.audio {
            ClipAudio::Decoded(pcm) => Some(pcm),
            ClipAudio::Streamed(_) => None,
        }
    }

    /// The stream of a streamed clip.
    pub fn stream(&self) -> Option<&Stream> {
        match &self.audio {
            ClipAudio::Decoded(_) => None,
            ClipAudio::Streamed(stream) => Some(stream),
        }
    }

    /// Length in seconds at the clip's own sample rate.
    pub fn duration_seconds(&self) -> f64 {
        let (frames, rate) = match &self.audio {
            ClipAudio::Decoded(pcm) => (pcm.frames(), pcm.sample_rate()),
            ClipAudio::Streamed(stream) => (stream.format().frames, stream.format().sample_rate),
        };
        f64::from(frames) / f64::from(rate)
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

/// How [`WavLoader`] loads a file. The asset server loads each path once,
/// so a file cannot be both decoded and streamed through one path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WavSettings {
    /// Keep the file's bytes and decode them while the clip plays, instead
    /// of decoding the whole file when it loads.
    pub stream: bool,
}

/// Loads `.wav` and `.flac` files into [`AudioClip`]s, decoding on the asset
/// task pool, or with [`WavSettings::stream`] checking the header there and
/// decoding while the clip plays. As in SoLoud's `Wav`, the format is chosen
/// by the file's first bytes, not its extension.
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

fn streamed(bytes: Vec<u8>) -> Result<AudioClip, DecodeError> {
    let wav = WavStream::from_bytes(bytes)?;
    let stream =
        Stream::new(Arc::new(wav)).map_err(|error| DecodeError::Unsupported(error.to_string()))?;
    Ok(AudioClip::streamed(stream))
}

impl AssetLoader for WavLoader {
    type Asset = AudioClip;
    type Settings = WavSettings;
    type Error = AudioLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &WavSettings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<AudioClip, AudioLoadError> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(AudioLoadError::Io)?;
        let clip = if settings.stream {
            streamed(bytes)
        } else {
            decode(&bytes, &self.limits).map(AudioClip::new)
        };
        clip.map_err(AudioLoadError::Decode)
    }

    fn extensions(&self) -> &[&str] {
        &["wav", "flac"]
    }
}

pub(crate) fn build(app: &mut App, limits: DecodeLimits) {
    app.init_asset::<AudioClip>()
        .register_asset_loader(WavLoader::new(limits));
}
