// Ported from SoLoud src/audiosource/wav/soloud_wavstream.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Streamed sounds, decoded a block at a time while they play
//! (`WavStream`). The core knows only the [`StreamSource`] and
//! [`StreamReader`] traits; `solouder-decoders` implements them for files.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use super::feed::{self, FeedReader, WorkerFeed};
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
/// The offline [`Mixer`](super::Mixer) reads streams as it mixes. Through
/// the real-time [`Controller`](crate::Controller), a
/// [`StreamWorker`](crate::StreamWorker) decodes them ahead of the audio
/// thread, which never reads a file.
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
        self.voice_of(self.direct_source())
    }

    /// Audio read on the thread that mixes, from a reader of its own.
    pub(crate) fn direct_source(&self) -> VoiceSource {
        let input = match self.source.open() {
            Some(reader) => StreamInput::Direct(reader),
            None => StreamInput::Missing,
        };
        self.source_reading(input)
    }

    /// Audio read through a feed, and the worker's end of the feed.
    pub(crate) fn fed_source(
        &self,
        chunks: usize,
        underflow: Arc<AtomicU64>,
    ) -> (VoiceSource, WorkerFeed) {
        let source = Arc::clone(&self.source);
        let (reader, worker) = feed::feed(source, self.format.channels, chunks, underflow);
        (
            self.source_reading(StreamInput::Feed(Box::new(reader))),
            worker,
        )
    }

    /// [`Sealed::prepare`] for a voice that reads through a feed.
    pub(crate) fn prepare_fed(
        &self,
        chunks: usize,
        underflow: Arc<AtomicU64>,
    ) -> (Prepared, WorkerFeed) {
        let (source, feed) = self.fed_source(chunks, underflow);
        let prepared = Prepared {
            voice: self.voice_of(source),
            stop_others: self.settings.single_instance,
            default_volume: self.settings.volume,
        };
        (prepared, feed)
    }

    fn source_reading(&self, input: StreamInput) -> VoiceSource {
        VoiceSource::Stream {
            input,
            channels: self.format.channels,
            frames: self.format.frames,
            offset: 0,
        }
    }

    fn voice_of(&self, source: VoiceSource) -> Voice {
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

    fn stream(&self) -> Option<&Stream> {
        Some(self)
    }
}

/// Where a stream voice's audio comes from.
#[derive(Debug)]
pub(crate) enum StreamInput {
    /// The source could not be opened.
    Missing,
    /// Read on the thread that mixes (the offline [`Mixer`](super::Mixer)).
    Direct(Box<dyn StreamReader>),
    /// Decoded ahead by a worker (the real-time renderer).
    Feed(Box<FeedReader>),
}

/// The frame a loop wrap continues from: [`seek`] to `loop_point` from the
/// end of the stream.
pub(crate) fn loop_frame(loop_point: f64, base_rate: f32, total: u32) -> u32 {
    let frames = (f64::from(base_rate) * loop_point).floor();
    if (1.0..2_147_483_648.0).contains(&frames) {
        (frames as u32).min(total)
    } else {
        0
    }
}

/// The frame a loop wrap of `voice` continues from, if it loops.
pub(crate) fn loop_target(voice: &Voice) -> Option<u32> {
    let VoiceSource::Stream { frames, .. } = voice.source else {
        return None;
    };
    let looping = voice.has_flag(super::voice::flags::LOOPING);
    looping.then(|| loop_frame(voice.loop_point, voice.base_sample_rate, frames))
}

/// `WavStreamInstance::getAudio` for a stream voice or queued stream:
/// reads up to `frames` frames into `out` (channel `c` at
/// `c * stride + start`), or skips them without `out`. A short read is the
/// end of the stream (reference patch 0054). `looping` is the frame a loop
/// wrap continues from, if the voice loops.
pub(crate) fn read(
    source: &mut VoiceSource,
    out: Option<&mut [f32]>,
    start: usize,
    stride: usize,
    frames: u32,
    looping: Option<u32>,
) -> u32 {
    let VoiceSource::Stream {
        input,
        channels,
        frames: total,
        offset,
    } = source
    else {
        return 0;
    };
    let (count, read) = match input {
        StreamInput::Missing => return 0,
        StreamInput::Direct(reader) => {
            let count = match out {
                Some(out) => reader.read(&mut out[start..], stride, frames as usize),
                None => reader.skip(u64::from(frames)) as usize,
            };
            let count = count.min(frames as usize) as u32;
            (count, count)
        }
        StreamInput::Feed(feed) => {
            feed.set_looping(looping);
            let out = out.map(|out| (out, start, stride));
            let (read, ended) = feed.read(out, frames, *channels);
            // Silence while the feed catches up keeps the voice going.
            (if ended { read } else { frames }, read)
        }
    };
    *offset = offset.saturating_add(read);
    if count < frames && *offset < *total {
        *offset = *total;
    }
    count
}

/// `hasEnded`: a voice without a reader has ended even when looping
/// (reference patch 0053).
pub(crate) fn has_ended(source: &VoiceSource, looping: bool) -> bool {
    match source {
        VoiceSource::Stream {
            input: StreamInput::Missing,
            ..
        } => true,
        VoiceSource::Stream {
            input: StreamInput::Feed(feed),
            ..
        } if feed.failed() => true,
        VoiceSource::Stream { frames, offset, .. } => !looping && offset >= frames,
        _ => false,
    }
}

/// SoLoud's generic seek on a stream voice: rewinds unless `seconds` is
/// past the stream position (a stream that cannot rewind stays where it
/// is), then skips frames at `base_rate`. Returns false if it did nothing.
pub(crate) fn seek(source: &mut VoiceSource, position: f64, seconds: f64, base_rate: f32) -> bool {
    let VoiceSource::Stream {
        input,
        frames: total,
        offset: read,
        ..
    } = source
    else {
        return false;
    };
    let mut offset = seconds - position;
    if offset <= 0.0 {
        if let StreamInput::Direct(reader) = input
            && !reader.rewind()
        {
            return false;
        }
        *read = 0;
        offset = seconds;
    }
    let frames = (f64::from(base_rate) * offset).floor();
    // `(int)` of a value outside its range is INT_MIN on x86: skip nothing.
    let skip = if (1.0..2_147_483_648.0).contains(&frames) {
        frames as u64
    } else {
        0
    };
    match input {
        StreamInput::Missing => {}
        StreamInput::Direct(reader) => {
            if skip > 0 {
                let skipped = reader.skip(skip);
                *read = read.saturating_add(u32::try_from(skipped).unwrap_or(u32::MAX));
                if skipped < skip && *read < *total {
                    *read = *total;
                }
            }
        }
        StreamInput::Feed(feed) => {
            *read = (u64::from(*read) + skip).min(u64::from(*total)) as u32;
            feed.seek_to(*read);
        }
    }
    true
}
