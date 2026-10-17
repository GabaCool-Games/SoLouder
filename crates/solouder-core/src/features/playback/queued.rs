// Ported from SoLoud src/core/soloud_queue.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Sounds and streams waiting in a queue (`Queue::mSource`).

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use super::feed;
use super::feed_worker::WorkerFeed;
use super::generator::{self, GeneratorContext, Synth};
use super::sound::{Sound, SoundId};
use super::source::Playable;
use super::stream::{self, Stream};
use super::voice::VoiceSource;

/// What [`Mixer::queue_play`](super::Mixer::queue_play) can queue: a
/// [`Sound`], a [`Stream`] or a [`Synth`].
pub trait Queueable: Playable + sealed::Queueable {}

impl Queueable for Sound {}
impl Queueable for Stream {}
impl Queueable for Synth {}

pub(crate) mod sealed {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    use super::{Queued, WorkerFeed};

    pub trait Queueable {
        /// An instance to queue, read on the thread that mixes.
        fn queued(&self) -> Queued;

        /// An instance to queue on a real-time renderer: a stream reads
        /// through a feed, whose worker end comes back too.
        fn queued_fed(
            &self,
            chunks: usize,
            underflow: Arc<AtomicU64>,
        ) -> (Queued, Option<WorkerFeed>);
    }
}

impl sealed::Queueable for Sound {
    fn queued(&self) -> Queued {
        Queued {
            id: self.id(),
            audio: VoiceSource::Pcm {
                pcm: self.pcm().clone(),
                offset: 0,
            },
            looping: self.looping(),
            sample_rate: self.pcm().sample_rate(),
        }
    }

    fn queued_fed(&self, _: usize, _: Arc<AtomicU64>) -> (Queued, Option<WorkerFeed>) {
        (self.queued(), None)
    }
}

impl sealed::Queueable for Synth {
    fn queued(&self) -> Queued {
        Queued {
            id: self.id(),
            audio: self.source(),
            looping: self.looping(),
            sample_rate: self.sample_rate(),
        }
    }

    fn queued_fed(&self, _: usize, _: Arc<AtomicU64>) -> (Queued, Option<WorkerFeed>) {
        (self.queued(), None)
    }
}

impl sealed::Queueable for Stream {
    fn queued(&self) -> Queued {
        Queued {
            id: self.id(),
            audio: self.direct_source(),
            looping: self.looping(),
            sample_rate: self.format().sample_rate,
        }
    }

    fn queued_fed(&self, chunks: usize, underflow: Arc<AtomicU64>) -> (Queued, Option<WorkerFeed>) {
        let (audio, feed) = self.fed_source(chunks, underflow);
        let queued = Queued {
            id: self.id(),
            audio,
            looping: self.looping(),
            sample_rate: self.format().sample_rate,
        };
        (queued, Some(feed))
    }
}

/// A queued sound or stream: an instance that is read but not mixed on its
/// own. `audio` is a sound's, stream's or generator's. Public
/// only to the crate: the module is private.
#[derive(Debug)]
pub struct Queued {
    pub id: SoundId,
    audio: VoiceSource,
    looping: bool,
    /// `mSamplerate` of the instance: its source's rate (generators read
    /// it).
    sample_rate: f32,
}

/// Where [`Queued::read`] puts the frames it reads.
pub enum Output<'a> {
    /// Into this buffer.
    Write(&'a mut [f32]),
    /// Nowhere: the frames are skipped. A generator still computes them,
    /// into this buffer, which holds every channel at the given stride.
    Skip(&'a mut [f32]),
}

impl Output<'_> {
    pub fn reborrow(&mut self) -> Output<'_> {
        match self {
            Self::Write(out) => Output::Write(out),
            Self::Skip(out) => Output::Skip(out),
        }
    }
}

impl Queued {
    /// Reads up to `frames` frames into `out` (channel `c` at
    /// `c * stride + start`), or skips them. The queue's `channels` the
    /// instance does not have are silent (reference patch 0050). Returns
    /// the frames read.
    pub fn read(
        &mut self,
        mut out: Output<'_>,
        start: usize,
        stride: usize,
        frames: u32,
        channels: usize,
    ) -> u32 {
        let (count, own) = match &mut self.audio {
            VoiceSource::Pcm { pcm, offset } => {
                let count = (pcm.frames() - *offset).min(frames);
                if let Output::Write(out) = &mut out {
                    let from = *offset as usize;
                    for c in 0..pcm.channels() as usize {
                        let source = &pcm.channel(c as u32)[from..][..count as usize];
                        out[c * stride + start..][..count as usize].copy_from_slice(source);
                    }
                }
                *offset += count;
                (count, pcm.channels() as usize)
            }
            VoiceSource::Stream { channels, .. } => {
                let own = *channels as usize;
                let out_ref = match &mut out {
                    Output::Write(out) => Some(&mut **out),
                    Output::Skip(_) => None,
                };
                (
                    stream::read(&mut self.audio, out_ref, start, stride, frames, None),
                    own,
                )
            }
            VoiceSource::Generator { channels, .. } => {
                let own = *channels as usize;
                // Skipped audio is computed into the discard buffer, as the
                // reference computes it into its seek scratch.
                let buffer = match &mut out {
                    Output::Write(out) | Output::Skip(out) => &mut out[start..],
                };
                let context = GeneratorContext::new(self.looping, self.sample_rate);
                let count = generator::read(&mut self.audio, buffer, stride, frames, &context);
                (count, own)
            }
            VoiceSource::Bus(_) | VoiceSource::Queue { .. } => (0, 0),
        };
        if let Output::Write(out) = out {
            for c in own..channels {
                out[c * stride + start..][..count as usize].fill(0.0);
            }
        }
        count
    }

    /// Moves a stream read on the mixing thread onto a feed; returns the
    /// worker's end.
    pub fn feed_direct(&mut self, chunks: usize, underflow: &Arc<AtomicU64>) -> Option<WorkerFeed> {
        feed::feed_direct(&mut self.audio, chunks, underflow, None)
    }

    pub fn has_ended(&self) -> bool {
        match &self.audio {
            VoiceSource::Pcm { pcm, offset } => !self.looping && *offset >= pcm.frames(),
            audio @ VoiceSource::Generator { .. } => generator::has_ended(audio),
            audio => stream::has_ended(audio, self.looping),
        }
    }
}
