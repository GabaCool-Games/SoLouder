//! Audio assets: decoded PCM, audio decoded while it plays, or a
//! generator's audio; files are loaded through the asset server.

use std::sync::Arc;

use bevy::asset::io::Reader;
use bevy::asset::{Asset, AssetApp, AssetLoader, AsyncReadExt, LoadContext};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use solouder_core::{Generator, Pcm, Stream, Synth, SynthError};
use solouder_decoders::{DecodeError, DecodeLimits, WavStream, decode};

/// Immutable audio, shared by every entity that plays it: decoded samples,
/// a stream that each voice decodes as it plays, or a generator (a
/// [`Synth`]) whose voices compute their audio as they play.
///
/// Playing a decoded clip shares its samples with the voice, a streamed
/// clip's encoded bytes with its reader, and a generated clip's generator
/// with the voice's instance, so removing or reloading the asset never
/// affects audio already playing.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct AudioClip {
    audio: ClipAudio,
}

#[derive(Clone, Debug)]
enum ClipAudio {
    Decoded(Pcm),
    Streamed(Stream),
    Generated(Synth),
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

    /// Wraps a synth: a generator with its playback defaults (volume,
    /// filters, 3D settings). Its voices share the generator, so a
    /// generator whose parameters are live (a `Vic`'s registers, a
    /// `Speech`'s voice) can still be changed through a clone kept by the
    /// caller.
    pub fn generated(synth: Synth) -> Self {
        Self {
            audio: ClipAudio::Generated(synth),
        }
    }

    /// Wraps a generator with the default playback settings.
    pub fn from_generator(generator: impl Generator + 'static) -> Result<Self, SynthError> {
        Synth::new(Arc::new(generator)).map(Self::generated)
    }

    /// The samples of a decoded clip.
    pub fn pcm(&self) -> Option<&Pcm> {
        match &self.audio {
            ClipAudio::Decoded(pcm) => Some(pcm),
            _ => None,
        }
    }

    /// The stream of a streamed clip.
    pub fn stream(&self) -> Option<&Stream> {
        match &self.audio {
            ClipAudio::Streamed(stream) => Some(stream),
            _ => None,
        }
    }

    /// The synth of a generated clip.
    pub fn synth(&self) -> Option<&Synth> {
        match &self.audio {
            ClipAudio::Generated(synth) => Some(synth),
            _ => None,
        }
    }

    /// Length in seconds at the clip's own sample rate; `None` for a
    /// generated clip, whose generator decides when it ends.
    pub fn duration_seconds(&self) -> Option<f64> {
        let (frames, rate) = match &self.audio {
            ClipAudio::Decoded(pcm) => (pcm.frames(), pcm.sample_rate()),
            ClipAudio::Streamed(stream) => (stream.format().frames, stream.format().sample_rate),
            ClipAudio::Generated(_) => return None,
        };
        Some(f64::from(frames) / f64::from(rate))
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

/// Loads `.wav`, `.flac`, `.ogg` and `.mp3` files into [`AudioClip`]s, decoding on the asset
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

/// At most the first `limit` bytes of `reader`.
pub(crate) async fn read_up_to(reader: &mut dyn Reader, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut *reader).take(limit).read_to_end(&mut bytes).await?;
    Ok(bytes)
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
        // As `decode_reader`: no more than the limits allow is read.
        let limit = self.limits.max_file_bytes();
        let bytes = read_up_to(reader, limit.saturating_add(1))
            .await
            .map_err(AudioLoadError::Io)?;
        if bytes.len() as u64 > limit {
            return Err(AudioLoadError::Decode(DecodeError::FileTooLarge { limit }));
        }
        let clip = if settings.stream {
            streamed(bytes)
        } else {
            decode(&bytes, &self.limits).map(AudioClip::new)
        };
        clip.map_err(AudioLoadError::Decode)
    }

    fn extensions(&self) -> &[&str] {
        &["wav", "flac", "ogg", "mp3"]
    }
}

pub(crate) fn build(app: &mut App, limits: DecodeLimits) {
    app.init_asset::<AudioClip>()
        .register_asset_loader(WavLoader::new(limits));
}
