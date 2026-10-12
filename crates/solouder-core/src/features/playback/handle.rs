// Ported from SoLoud src/core/soloud_core_getters.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

/// Voice slots in a mixer; at most this many voices play at once
/// (`VOICE_COUNT`).
pub const VOICE_COUNT: usize = 1024;

/// Play indices wrap before reaching this value; SoLoud reserves all-ones for
/// voice-group handles.
const PLAY_INDEX_LIMIT: u32 = 0xfffff;

/// High bits of every voice group handle; the low 12 bits are its index.
pub(crate) const GROUP_MARK: u32 = 0xffff_f000;

/// Identifies one playing voice, with SoLoud's bit layout: the low 12 bits
/// hold the voice slot plus one and the high 20 bits the play index.
///
/// A handle stays valid until its voice stops; a later voice in the same slot
/// gets a different play index, so stale handles are ignored rather than
/// affecting the new voice (until the 20-bit index wraps).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Handle(u32);

impl Handle {
    /// Never refers to a voice. `play` returns it when no voice slot is free.
    pub const INVALID: Self = Self(0);

    /// Rebuilds a handle from [`Handle::raw`].
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The handle's bits, identical to the SoLoud C API's handle value.
    pub fn raw(self) -> u32 {
        self.0
    }

    pub(crate) fn new(slot: usize, play_index: u32) -> Self {
        debug_assert!(slot < VOICE_COUNT && play_index < PLAY_INDEX_LIMIT);
        Self((slot as u32 + 1) | (play_index << 12))
    }

    /// The slot, if the slot field names one (reference patch 0020).
    pub(crate) fn slot(self) -> Option<usize> {
        let field = (self.0 & 0xfff) as usize;
        (1..=VOICE_COUNT).contains(&field).then(|| field - 1)
    }

    pub(crate) fn play_index(self) -> u32 {
        self.0 >> 12
    }

    /// Whether the bits are those of a voice group handle, existing or not.
    pub(crate) fn has_group_mark(self) -> bool {
        self.0 & GROUP_MARK == GROUP_MARK
    }
}

/// The play index after `index` (`mPlayIndex++`, wrapping before 0xfffff).
pub(crate) fn next_play_index(index: u32) -> u32 {
    let next = index + 1;
    if next == PLAY_INDEX_LIMIT { 0 } else { next }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_soloud() {
        let handle = Handle::new(5, 3);
        assert_eq!(handle.raw(), 6 | (3 << 12));
        assert_eq!(handle.slot(), Some(5));
        assert_eq!(handle.play_index(), 3);
    }

    #[test]
    fn slot_field_outside_the_voice_table_is_no_slot() {
        assert_eq!(Handle::INVALID.slot(), None);
        assert_eq!(Handle::from_raw(1 << 12).slot(), None);
        assert_eq!(Handle::from_raw(0xfff).slot(), None);
        assert_eq!(
            Handle::from_raw(VOICE_COUNT as u32).slot(),
            Some(VOICE_COUNT - 1)
        );
    }

    #[test]
    fn play_index_wraps_before_the_group_marker() {
        assert_eq!(next_play_index(0xffffd), 0xffffe);
        assert_eq!(next_play_index(0xffffe), 0);
    }
}
