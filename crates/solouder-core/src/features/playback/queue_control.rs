// Ported from SoLoud src/core/soloud_queue.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::fmt;

use super::mixer::Mixer;
use super::queue::{QUEUE_MAX, Queue, Queued};
use super::sound::Sound;
use super::source::Playable;
use super::voice::VoiceSource;

/// Why [`Mixer::queue_play`] did not queue a sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueueError {
    /// The queue is not playing on this mixer (`INVALID_PARAMETER`).
    NotPlaying,
    /// [`QUEUE_MAX`] sounds are queued already (`OUT_OF_MEMORY`).
    Full,
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPlaying => write!(f, "the queue is not playing"),
            Self::Full => write!(f, "the queue is full"),
        }
    }
}

impl std::error::Error for QueueError {}

/// Queues on this mixer.
impl Mixer {
    /// Queues `sound` to play after the sounds already queued
    /// (`Queue::play`). The queue must be playing on this mixer. The sound
    /// plays once even if it loops, at the queue's rate; its volume, filters
    /// and other settings do not apply.
    pub fn queue_play(&mut self, queue: &Queue, sound: &Sound) -> Result<(), QueueError> {
        if !self.queue_is_playing(queue) {
            return Err(QueueError::NotPlaying);
        }
        if self.voices.queues.count(queue.id()) >= QUEUE_MAX {
            return Err(QueueError::Full);
        }
        self.voices.queues.push(queue.id(), Queued::new(sound));
        Ok(())
    }

    /// Sounds queued and not finished (`Queue::getQueueCount`), also while
    /// the queue is not playing.
    pub fn queue_count(&self, queue: &Queue) -> usize {
        self.voices.queues.count(queue.id())
    }

    /// Whether `source` is the queued sound playing now
    /// (`Queue::isCurrentlyPlaying`).
    pub fn queue_is_current(&self, queue: &Queue, source: &impl Playable) -> bool {
        self.queue_is_playing(queue)
            && self
                .voices
                .queues
                .get(queue.id())
                .and_then(|items| items.front())
                .is_some_and(|item| item.id == source.source_id())
    }

    fn queue_is_playing(&self, queue: &Queue) -> bool {
        self.voices.slots[..self.voices.highest]
            .iter()
            .flatten()
            .any(|v| v.sound_id == queue.id() && matches!(v.source, VoiceSource::Queue { .. }))
    }
}
