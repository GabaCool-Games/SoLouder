//! Voice IDs handed out by the controller before the mixer has a voice, and
//! the renderer's table from those IDs to mixer handles.

use std::fmt;

use crate::features::playback::{Handle, VOICE_COUNT};

/// A play request and, once started, its voice.
///
/// The controller reserves the ID when the request is queued, so commands can
/// address the voice before the audio thread has created it. IDs are
/// generation-checked: once the voice's final event has been read, commands
/// with the old ID do nothing, even after the index is reused.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VoiceId {
    index: u32,
    generation: u32,
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

/// Controller side: which IDs are reserved.
#[derive(Debug)]
pub(crate) struct IdPool {
    generations: Vec<u32>,
    free: Vec<u32>,
}

impl IdPool {
    pub fn new(capacity: usize) -> Self {
        Self {
            generations: vec![0; capacity],
            // Pop from the end, so low indices are used first.
            free: (0..capacity as u32).rev().collect(),
        }
    }

    pub fn allocate(&mut self) -> Option<VoiceId> {
        let index = self.free.pop()?;
        Some(VoiceId {
            index,
            generation: self.generations[index as usize],
        })
    }

    /// Frees `id` for reuse with the next generation. Ignores stale IDs.
    pub fn release(&mut self, id: VoiceId) {
        let generation = &mut self.generations[id.index()];
        if *generation == id.generation {
            *generation = generation.wrapping_add(1);
            self.free.push(id.index);
        }
    }

    pub fn in_use(&self) -> usize {
        self.generations.len() - self.free.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    id: VoiceId,
    handle: Handle,
    /// Position in `LiveTable::live`.
    position: u32,
}

/// Renderer side: the mixer handle of every started voice. Preallocated; no
/// operation allocates.
#[derive(Debug)]
pub(crate) struct LiveTable {
    entries: Vec<Option<Entry>>,
    live: Vec<u32>,
    /// For each mixer voice slot, the index of the entry using it, plus one
    /// (0: none).
    by_slot: Vec<u32>,
}

impl LiveTable {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: vec![None; capacity],
            live: Vec::with_capacity(capacity),
            by_slot: vec![0; VOICE_COUNT],
        }
    }

    pub fn insert(&mut self, id: VoiceId, handle: Handle) {
        if let Some(old) = self.entries[id.index()] {
            self.remove(old.id);
        }
        self.entries[id.index()] = Some(Entry {
            id,
            handle,
            position: self.live.len() as u32,
        });
        self.live.push(id.index);
        if let Some(slot) = handle.slot() {
            self.by_slot[slot] = id.index + 1;
        }
    }

    /// Removes the voice that used mixer slot `slot`, if the table still has
    /// one there.
    pub fn remove_slot(&mut self, slot: usize) -> Option<VoiceId> {
        let index = self.by_slot.get(slot)?.checked_sub(1)?;
        let entry = self.entries[index as usize]?;
        if entry.handle.slot() != Some(slot) {
            return None;
        }
        self.remove(entry.id);
        Some(entry.id)
    }

    pub fn handle(&self, id: VoiceId) -> Option<Handle> {
        self.entries
            .get(id.index())?
            .filter(|entry| entry.id == id)
            .map(|entry| entry.handle)
    }

    pub fn remove(&mut self, id: VoiceId) -> Option<Handle> {
        let entry = self
            .entries
            .get(id.index())?
            .filter(|entry| entry.id == id)?;
        self.entries[id.index()] = None;
        if let Some(slot) = entry.handle.slot()
            && self.by_slot[slot] == id.index + 1
        {
            self.by_slot[slot] = 0;
        }
        let position = entry.position as usize;
        self.live.swap_remove(position);
        if let Some(&moved) = self.live.get(position)
            && let Some(moved) = self.entries[moved as usize].as_mut()
        {
            moved.position = position as u32;
        }
        Some(entry.handle)
    }

    /// Removes every voice for which `ended` is true, calling `on_removed`
    /// with its ID.
    pub fn retain(&mut self, ended: impl Fn(Handle) -> bool, mut on_removed: impl FnMut(VoiceId)) {
        let mut i = 0;
        while i < self.live.len() {
            let entry = self.entries[self.live[i] as usize].expect("live entries exist");
            if ended(entry.handle) {
                self.remove(entry.id);
                on_removed(entry.id);
            } else {
                i += 1;
            }
        }
    }

    /// Removes every voice, calling `on_removed` with its ID.
    pub fn clear(&mut self, mut on_removed: impl FnMut(VoiceId)) {
        while let Some(&index) = self.live.last() {
            let id = self.entries[index as usize].expect("live entries exist").id;
            self.remove(id);
            on_removed(id);
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.live.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_ids_come_back_with_a_new_generation() {
        let mut pool = IdPool::new(2);
        let a = pool.allocate().unwrap();
        let b = pool.allocate().unwrap();
        assert!(pool.allocate().is_none());
        pool.release(a);
        pool.release(a);
        let c = pool.allocate().unwrap();
        assert_eq!(c.index(), a.index());
        assert_ne!(c, a);
        assert_eq!(pool.in_use(), 2);
        pool.release(b);
        assert_eq!(pool.in_use(), 1);
    }

    #[test]
    fn table_tracks_live_voices_through_swap_removal() {
        let mut pool = IdPool::new(4);
        let ids: Vec<_> = (0..4).map(|_| pool.allocate().unwrap()).collect();
        let mut table = LiveTable::new(4);
        for (i, &id) in ids.iter().enumerate() {
            table.insert(id, Handle::from_raw(i as u32 + 1));
        }
        assert_eq!(table.remove(ids[0]), Some(Handle::from_raw(1)));
        assert_eq!(table.remove(ids[0]), None);
        assert_eq!(table.handle(ids[3]), Some(Handle::from_raw(4)));
        let mut removed = Vec::new();
        table.retain(|handle| handle.raw() == 3, |id| removed.push(id));
        assert_eq!(removed, vec![ids[2]]);
        assert_eq!(table.len(), 2);
        table.clear(|id| removed.push(id));
        assert_eq!(table.len(), 0);
        assert_eq!(removed.len(), 3);
        pool.release(ids[1]);
        let reused = pool.allocate().unwrap();
        assert_eq!(table.handle(reused), None);
    }
}
