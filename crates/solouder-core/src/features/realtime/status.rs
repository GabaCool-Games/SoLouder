//! What the audio thread reports back: per-voice events and a snapshot of
//! engine counters.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::VoiceId;

/// A change in a voice's life, reported once, in the order it happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The play request got a voice; it is now being mixed (or held paused).
    Started(VoiceId),
    /// The play request found no voice: every voice is protected.
    Failed(VoiceId),
    /// The voice is gone. Its ID no longer addresses anything.
    Finished(VoiceId, FinishReason),
}

impl Event {
    /// The voice this event is about.
    pub fn id(self) -> VoiceId {
        match self {
            Self::Started(id) | Self::Failed(id) | Self::Finished(id, _) => id,
        }
    }

    /// Whether this is the voice's last event.
    pub fn is_final(self) -> bool {
        !matches!(self, Self::Started(_))
    }
}

/// Why a voice finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishReason {
    /// It ended during a mix: it played to the end, or a scheduled stop came
    /// due.
    Ended,
    /// A command stopped it: a stop, stopping its bus or every voice of its
    /// sound, or a play that replaced it (a single-instance sound, or a bus
    /// or queue played again).
    Stopped,
    /// Every voice slot was busy and a new play took this voice's slot (the
    /// oldest unprotected voice is the one stolen). Reported just before the
    /// new voice's [`Event::Started`].
    Stolen,
}

/// Engine counters published after every render.
///
/// Each field is published atomically on its own. A snapshot taken while a
/// render is publishing can combine fields from two consecutive renders, so
/// treat them as gauges, not as one consistent record.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    /// Seconds of audio rendered (`getStreamTime`).
    pub stream_time: f64,
    /// Voices in the table, playing or paused (`getVoiceCount`).
    pub voices: u32,
    /// Voices selected for mixing as of the end of the last render
    /// (`getActiveVoiceCount`): voices that ended in that render no longer
    /// count.
    pub active_voices: u32,
    /// Completed renders.
    pub renders: u64,
    /// Commands still queued after the last render: more than the per-render
    /// limit arrived, or they were queued while it ran.
    pub deferred_commands: u64,
    /// Retired items the audio thread had to free itself. The controller
    /// reserves room for every item a request can retire, so this stays 0
    /// for voices started through it.
    pub retired_overflow: u64,
    /// Sounds [`Controller::queue_play`](super::Controller::queue_play)
    /// sent that could not be queued: the queue was not playing or was full.
    pub rejected_queue_sounds: u64,
    /// Frames of silence streams played because the
    /// [`StreamWorker`](super::StreamWorker) had not decoded their audio in
    /// time, after it first arrived (waiting for a stream to start, or to
    /// continue after a seek, is not counted).
    pub stream_underflow_frames: u64,
}

/// The snapshot, shared through atomics so neither side waits.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    stream_time: AtomicU64,
    voices: AtomicU32,
    active_voices: AtomicU32,
    renders: AtomicU64,
    deferred_commands: AtomicU64,
    retired_overflow: AtomicU64,
    rejected_queue_sounds: AtomicU64,
}

impl Shared {
    pub fn publish(&self, status: &Status) {
        let relaxed = Ordering::Relaxed;
        self.stream_time
            .store(status.stream_time.to_bits(), relaxed);
        self.voices.store(status.voices, relaxed);
        self.active_voices.store(status.active_voices, relaxed);
        self.deferred_commands
            .store(status.deferred_commands, relaxed);
        self.retired_overflow
            .store(status.retired_overflow, relaxed);
        self.rejected_queue_sounds
            .store(status.rejected_queue_sounds, relaxed);
        // Readers that see this count see the fields stored before it.
        self.renders.store(status.renders, Ordering::Release);
    }

    pub fn read(&self) -> Status {
        let relaxed = Ordering::Relaxed;
        let renders = self.renders.load(Ordering::Acquire);
        Status {
            stream_time: f64::from_bits(self.stream_time.load(relaxed)),
            voices: self.voices.load(relaxed),
            active_voices: self.active_voices.load(relaxed),
            renders,
            deferred_commands: self.deferred_commands.load(relaxed),
            retired_overflow: self.retired_overflow.load(relaxed),
            rejected_queue_sounds: self.rejected_queue_sounds.load(relaxed),
            stream_underflow_frames: 0,
        }
    }
}
