//! The real-time controller's voice IDs, shared with the slices whose
//! callbacks see them (spatial colliders).

use std::fmt;

/// A play request and, once started, its voice.
///
/// The [`Controller`](crate::Controller) reserves the ID when the request is
/// queued, so commands can address the voice before the audio thread has
/// created it. IDs are generation-checked: once the voice's final event has
/// been read, commands with the old ID do nothing, even after the index is
/// reused.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoiceId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl VoiceId {
    pub(crate) fn index(self) -> usize {
        self.index as usize
    }
}

impl fmt::Debug for VoiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VoiceId({}v{})", self.index, self.generation)
    }
}
