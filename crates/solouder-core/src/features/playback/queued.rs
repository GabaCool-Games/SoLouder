// Ported from SoLoud src/core/soloud_queue.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! Sounds and streams waiting in a queue (`Queue::mSource`).

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use super::feed::WorkerFeed;
use super::sound::{Sound, SoundId};
use super::source::Playable;
use super::stream::{self, Stream};
use super::voice::VoiceSource;

/// What [`Mixer::queue_play`](super::Mixer::queue_play) can queue: a
/// [`Sound`] or a [`Stream`].
pub trait Queueable: Playable + sealed::Queueable {}

impl Queueable for Sound {}
impl Queueable for Stream {}

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
        }
    }

    fn queued_fed(&self, chunks: usize, underflow: Arc<AtomicU64>) -> (Queued, Option<WorkerFeed>) {
        let (audio, feed) = self.fed_source(chunks, underflow);
        let queued = Queued {
            id: self.id(),
            audio,
            looping: self.looping(),
        };
        (queued, Some(feed))
    }
}

/// A queued sound or stream: an instance that is read but not mixed on its
/// own. `audio` is [`VoiceSource::Pcm`] or [`VoiceSource::Stream`]. Public
/// only to the crate: the module is private.
#[derive(Debug)]
pub struct Queued {
    pub id: SoundId,
    audio: VoiceSource,
    looping: bool,
}

impl Queued {
    /// Reads up to `frames` frames into `out` (channel `c` at
    /// `c * stride + start`), or skips them without `out`. The queue's
    /// `channels` the instance does not have are silent (reference patch
    /// 0050). Returns the frames read.
    pub fn read(
        &mut self,
        mut out: Option<&mut [f32]>,
        start: usize,
        stride: usize,
        frames: u32,
        channels: usize,
    ) -> u32 {
        let (count, own) = match &mut self.audio {
            VoiceSource::Pcm { pcm, offset } => {
                let count = (pcm.frames() - *offset).min(frames);
                if let Some(out) = out.as_deref_mut() {
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
                let out_ref = out.as_deref_mut();
                (
                    stream::read(&mut self.audio, out_ref, start, stride, frames, None),
                    own,
                )
            }
            VoiceSource::Bus(_) | VoiceSource::Queue { .. } => (0, 0),
        };
        if let Some(out) = out {
            for c in own..channels {
                out[c * stride + start..][..count as usize].fill(0.0);
            }
        }
        count
    }

    pub fn has_ended(&self) -> bool {
        match &self.audio {
            VoiceSource::Pcm { pcm, offset } => !self.looping && *offset >= pcm.frames(),
            audio => stream::has_ended(audio, self.looping),
        }
    }
}
