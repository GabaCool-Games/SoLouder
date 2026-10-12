//! Audio released by voices that stop, kept for disposal elsewhere.
//!
//! Dropping the last reference to a sound's samples frees memory, which a
//! real-time callback must not do. A mixer that keeps retired audio moves it
//! into a preallocated list instead; its owner drains the list and hands the
//! buffers to a non-real-time thread. Offline mixers drop them immediately.

use super::mixer::Mixer;
use crate::contracts::Pcm;

#[derive(Debug, Default)]
pub(crate) struct Retired {
    kept: Vec<Pcm>,
    keep: bool,
    /// Buffers dropped in place because `kept` was full.
    overflowed: u64,
}

impl Retired {
    /// Keeps `pcm` if retention is on and there is room; otherwise drops it.
    pub fn put(&mut self, pcm: Pcm) {
        if !self.keep {
            return;
        }
        if self.kept.len() < self.kept.capacity() {
            self.kept.push(pcm);
        } else {
            self.overflowed += 1;
        }
    }
}

impl Mixer {
    /// Keeps the audio of voices that stop, up to `capacity` buffers between
    /// drains, instead of dropping it inside the mixer. Allocates the list, so
    /// call it before handing the mixer to a real-time thread. Beyond
    /// `capacity`, buffers are dropped in place and counted by
    /// [`Mixer::retired_overflow`].
    pub fn keep_retired_audio(&mut self, capacity: usize) {
        let retired = &mut self.voices.retired;
        retired
            .kept
            .reserve_exact(capacity.saturating_sub(retired.kept.len()));
        retired.keep = true;
    }

    /// Hands up to `max` kept buffers to `dispose`, oldest first. Does not
    /// allocate.
    pub fn drain_retired_audio(&mut self, max: usize, mut dispose: impl FnMut(Pcm)) {
        let kept = &mut self.voices.retired.kept;
        for pcm in kept.drain(..max.min(kept.len())) {
            dispose(pcm);
        }
    }

    /// Adds `pcm` to the kept list, for audio the owner holds outside any
    /// voice (for example the sound of a play request that found no voice).
    pub fn retire_audio(&mut self, pcm: Pcm) {
        self.voices.retired.put(pcm);
    }

    /// Buffers dropped in place because the kept list was full.
    pub fn retired_overflow(&self) -> u64 {
        self.voices.retired.overflowed
    }
}
