//! Memory the mixer stops using, kept for disposal elsewhere.
//!
//! Freeing memory is something a real-time callback must not do: dropping
//! the last reference to a sound's samples, a voice's filter instances or a
//! bus voice's buffers would. A mixer that keeps retired memory moves it
//! into a preallocated list instead; its owner drains the list and hands the
//! items to a non-real-time thread. Offline mixers drop them immediately.

use std::collections::VecDeque;
use std::fmt;

use super::mixer::Mixer;
use super::queue::Queued;
use super::voice::Voice;
use crate::contracts::Pcm;
use crate::features::filters::FilterInstance;

/// Memory a mixer no longer uses: a sound's samples, an ended voice, or a
/// replaced filter instance. Dropping it frees the memory.
pub struct RetiredItem(Garbage);

impl fmt::Debug for RetiredItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.0 {
            Garbage::Pcm(_) => "audio",
            Garbage::Voice(_) => "voice",
            Garbage::Filter(_) => "filter instance",
            Garbage::QueueStorage(_) => "queue storage",
        };
        f.debug_tuple("RetiredItem").field(&kind).finish()
    }
}

/// Held only to be dropped later.
#[allow(dead_code)]
enum Garbage {
    Pcm(Pcm),
    /// A voice in the box it arrived in, or that box empty.
    Voice(Box<Option<Voice>>),
    Filter(Option<Box<dyn FilterInstance>>),
    QueueStorage(VecDeque<Queued>),
}

#[derive(Default)]
pub(crate) struct Retired {
    kept: Vec<RetiredItem>,
    /// Empty voice boxes: an ended voice moves into one, so retiring it
    /// does not allocate. The boxes leave this list whole, inside
    /// `Garbage::Voice`, so they cannot be stored unboxed.
    #[allow(clippy::vec_box)]
    boxes: Vec<Box<Option<Voice>>>,
    keep: bool,
    /// Items dropped in place because `kept` was full or no box was left.
    overflowed: u64,
}

impl fmt::Debug for Retired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Retired")
            .field("kept", &self.kept.len())
            .field("boxes", &self.boxes.len())
            .field("keep", &self.keep)
            .field("overflowed", &self.overflowed)
            .finish()
    }
}

impl Retired {
    pub fn kept_len(&self) -> usize {
        self.kept.len()
    }

    pub fn put(&mut self, pcm: Pcm) {
        self.keep_item(Garbage::Pcm(pcm));
    }

    pub fn put_filter(&mut self, instance: Option<Box<dyn FilterInstance>>) {
        self.keep_item(Garbage::Filter(instance));
    }

    pub fn put_queue_storage(&mut self, storage: VecDeque<Queued>) {
        self.keep_item(Garbage::QueueStorage(storage));
    }

    /// Keeps a voice that ended, in a box adopted earlier.
    pub fn put_voice(&mut self, voice: Voice) {
        if !self.keep {
            return;
        }
        if self.kept.len() < self.kept.capacity()
            && let Some(mut boxed) = self.boxes.pop()
        {
            *boxed = Some(voice);
            self.kept.push(RetiredItem(Garbage::Voice(boxed)));
        } else {
            self.overflowed += 1;
        }
    }

    /// Makes a box for a voice started on this thread, if retired voices
    /// are kept.
    pub fn add_box(&mut self) {
        if self.keep {
            self.boxes.push(Box::new(None));
        }
    }

    /// Takes an empty voice box for a later [`Retired::put_voice`].
    pub fn adopt_box(&mut self, boxed: Box<Option<Voice>>) {
        if self.boxes.len() < self.boxes.capacity() {
            self.boxes.push(boxed);
        } else {
            self.keep_item(Garbage::Voice(boxed));
        }
    }

    fn keep_item(&mut self, item: Garbage) {
        if !self.keep {
            return;
        }
        if self.kept.len() < self.kept.capacity() {
            self.kept.push(RetiredItem(item));
        } else {
            self.overflowed += 1;
        }
    }
}

impl Mixer {
    /// Keeps what voices and filters release, up to `capacity` items between
    /// drains, instead of freeing it inside the mixer: each ended voice,
    /// replaced filter instance and finished queued sound is one item.
    /// Allocates the list, and a box for each voice already playing, so call
    /// it before handing the mixer to a real-time thread. Plays after this
    /// call allocate a box for their voice. Beyond `capacity`,
    /// items are dropped in place and counted by [`Mixer::retired_overflow`].
    pub fn keep_retired(&mut self, capacity: usize) {
        let voices = self.voices.count();
        let retired = &mut self.voices.retired;
        // Boxes beyond one per voice, left by voices dropped in place, would
        // make the renderer retire the box a later play brings.
        retired.boxes.truncate(voices);
        retired
            .kept
            .reserve_exact(capacity.saturating_sub(retired.kept.len()));
        retired
            .boxes
            .reserve_exact(capacity.saturating_sub(retired.boxes.len()));
        while retired.boxes.len() < voices.min(capacity) {
            retired.boxes.push(Box::new(None));
        }
        retired.keep = true;
    }

    /// Hands up to `max` kept items to `dispose`, oldest first. Does not
    /// allocate or free.
    pub fn drain_retired(&mut self, max: usize, mut dispose: impl FnMut(RetiredItem)) {
        let kept = &mut self.voices.retired.kept;
        for item in kept.drain(..max.min(kept.len())) {
            dispose(item);
        }
    }

    /// Items dropped in place because the kept list was full.
    pub fn retired_overflow(&self) -> u64 {
        self.voices.retired.overflowed
    }

    /// Whether a play stole a voice since the last call.
    pub(crate) fn take_stolen(&mut self) -> bool {
        std::mem::take(&mut self.voices.stole)
    }
}
