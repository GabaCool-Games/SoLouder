// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Streamed sounds, decoded a block at a time while they play
//! (`WavStream`). The core knows only the [`StreamSource`] and
//! [`StreamReader`] traits; `solouder-decoders` implements them for files.

use std::fmt;
use std::sync::Arc;

use super::sound::SoundId;
use super::source::private::{Prepared, Sealed};
use super::source::{Playable, SourceSettings, source_settings_api};
use super::voice::{Voice, VoiceSource};
use crate::contracts::{MAX_CHANNELS, SAMPLE_GRANULARITY};

const BLOCK: usize = SAMPLE_GRANULARITY as usize;

/// The layout of a stream's audio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamFormat {
    /// 1 to [`MAX_CHANNELS`].
    pub channels: u32,
    /// Frames per second, finite and greater than 0.
    pub sample_rate: f32,
    /// Frames in the stream (`mSampleCount`). A reader that runs out sooner
    /// ends the voice there.
    pub frames: u32,
}

/// Encoded audio that each voice reads on its own: a file name, bytes in
/// memory, or anything else that can be opened more than once.
/// `solouder_decoders::WavStream` implements it.
pub trait StreamSource: Send + Sync + fmt::Debug {
    /// The layout every reader produces.
    fn format(&self) -> StreamFormat;

    /// A reader at the first frame, for one voice, or `None` when the audio
    /// can no longer be opened (a file deleted since it was loaded); that
    /// voice ends at once.
    fn open(&self) -> Option<Box<dyn StreamReader>>;
}

/// One voice's place in a stream. Reads may wait for I/O.
pub trait StreamReader: Send + fmt::Debug {
    /// Decodes up to `frames` frames, channel `c` into
    /// `out[c * stride..][..frames]`, and returns how many it decoded:
    /// fewer only at the end of the audio or when reading fails.
    fn read(&mut self, out: &mut [f32], stride: usize, frames: usize) -> usize;

    /// Goes back to the first frame; false if the stream cannot.
    fn rewind(&mut self) -> bool;

    /// Moves `frames` frames forward and returns how many it moved, fewer
    /// only at the end. By default the frames are decoded and discarded, as
    /// SoLoud's generic seek does; readers that can jump should.
    fn skip(&mut self, frames: u64) -> u64 {
        let mut scratch = [0.0_f32; BLOCK * MAX_CHANNELS as usize];
        let mut skipped = 0;
        while skipped < frames {
            let want = (frames - skipped).min(BLOCK as u64) as usize;
            let got = self.read(&mut scratch, BLOCK, want).min(want);
            skipped += got as u64;
            if got < want {
                break;
            }
        }
        skipped
    }
}

/// Why a [`Stream`] cannot be made from a source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// Streams have 1 to [`MAX_CHANNELS`] channels.
    UnsupportedChannelCount(u32),
    /// The sample rate is not finite and greater than 0.
    InvalidSampleRate,
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedChannelCount(n) => {
                write!(
                    f,
                    "unsupported channel count {n} (expected 1 to {MAX_CHANNELS})"
                )
            }
            Self::InvalidSampleRate => write!(f, "the sample rate must be finite and above 0"),
        }
    }
}

impl std::error::Error for StreamError {}

/// A sound decoded while it plays, with its playback defaults
/// (SoLoud's `WavStream`). Each voice opens its own reader, so plays are
/// independent. Cloning shares the source and keeps the [`SoundId`], as for
/// [`Sound`](super::Sound).
///
/// Only the offline [`Mixer`](super::Mixer) plays streams for now: the
/// real-time controller would read the file on the audio thread, so it
/// refuses them until streaming moves to a worker.
#[derive(Clone, Debug)]
pub struct Stream {
    source: Arc<dyn StreamSource>,
    format: StreamFormat,
    id: SoundId,
    settings: SourceSettings,
}

impl Stream {
    /// A stream at volume 1 that does not loop, with a new [`SoundId`].
    pub fn new(source: Arc<dyn StreamSource>) -> Result<Self, StreamError> {
        let format = source.format();
        if format.channels == 0 || format.channels > MAX_CHANNELS {
            return Err(StreamError::UnsupportedChannelCount(format.channels));
        }
        if !(format.sample_rate.is_finite() && format.sample_rate > 0.0) {
            return Err(StreamError::InvalidSampleRate);
        }
        Ok(Self {
            source,
            format,
            id: SoundId::next(),
            settings: SourceSettings::default(),
        })
    }

    /// The layout of the audio.
    pub fn format(&self) -> StreamFormat {
        self.format
    }

    /// Shared by this stream and its clones.
    pub fn id(&self) -> SoundId {
        self.id
    }

    /// Seconds of audio (`WavStream::getLength`).
    pub fn length(&self) -> f64 {
        // SoLoud divides in single precision.
        f64::from(self.format.frames as f32 / self.format.sample_rate)
    }

    source_settings_api!();

    /// A voice at the start of the stream, with a reader of its own.
    pub(crate) fn voice(&self) -> Voice {
        let source = VoiceSource::Stream {
            reader: self.source.open(),
            channels: self.format.channels,
            frames: self.format.frames,
            offset: 0,
        };
        Voice::new(&self.settings, self.id, source, self.format.sample_rate, 0)
    }
}

impl Playable for Stream {
    fn source_id(&self) -> SoundId {
        self.id
    }
}

impl Sealed for Stream {
    fn prepare(&self) -> Prepared {
        Prepared {
            voice: self.voice(),
            stop_others: self.settings.single_instance,
            default_volume: self.settings.volume,
        }
    }

    fn is_stream(&self) -> bool {
        true
    }
}

/// `WavStreamInstance::getAudio` for a stream voice: like
/// [`Voice::read`](super::voice::Voice::read), into `block` at `start`.
/// A short read is the end of the stream (reference patch 0054).
pub(crate) fn read(source: &mut VoiceSource, block: &mut [f32], start: usize, frames: u32) -> u32 {
    let VoiceSource::Stream {
        reader: Some(reader),
        frames: total,
        offset,
        ..
    } = source
    else {
        return 0;
    };
    let count = reader
        .read(&mut block[start..], BLOCK, frames as usize)
        .min(frames as usize) as u32;
    *offset = offset.saturating_add(count);
    if count < frames && *offset < *total {
        *offset = *total;
    }
    count
}

/// `hasEnded`: a voice without a reader has ended even when looping
/// (reference patch 0053).
pub(crate) fn has_ended(source: &VoiceSource, looping: bool) -> bool {
    match source {
        VoiceSource::Stream { reader: None, .. } => true,
        VoiceSource::Stream { frames, offset, .. } => !looping && offset >= frames,
        _ => false,
    }
}

/// SoLoud's generic seek on a stream voice: rewinds unless `seconds` is
/// past the stream position (a stream that cannot rewind stays where it
/// is), then skips frames at `base_rate`. Returns false if it did nothing.
pub(crate) fn seek(source: &mut VoiceSource, position: f64, seconds: f64, base_rate: f32) -> bool {
    let VoiceSource::Stream {
        reader,
        frames: total,
        offset: read,
        ..
    } = source
    else {
        return false;
    };
    let mut offset = seconds - position;
    if offset <= 0.0 {
        if let Some(reader) = reader
            && !reader.rewind()
        {
            return false;
        }
        *read = 0;
        offset = seconds;
    }
    let frames = (f64::from(base_rate) * offset).floor();
    // `(int)` of a value outside its range is INT_MIN on x86: skip nothing.
    if let Some(reader) = reader
        && (1.0..2_147_483_648.0).contains(&frames)
    {
        let skipped = reader.skip(frames as u64);
        *read = read.saturating_add(u32::try_from(skipped).unwrap_or(u32::MAX));
        if skipped < frames as u64 && *read < *total {
            *read = *total;
        }
    }
    true
}
