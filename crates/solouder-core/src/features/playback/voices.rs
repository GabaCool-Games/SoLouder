// Ported from SoLoud src/core/soloud.cpp, src/core/soloud_core_getters.cpp
// and src/core/soloud_core_voiceops.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::groups::VoiceGroups;
use super::handle::{Handle, VOICE_COUNT};
use super::queue::Queues;
use super::retire::Retired;
use super::sound::SoundId;
use super::voice::{ResampleBlocks, Voice, flags};
use crate::contracts::{MAX_CHANNELS, SAMPLE_GRANULARITY};

/// Floats in one resample block: `SAMPLE_GRANULARITY` frames of every channel.
pub(crate) const BLOCK_LEN: usize = (SAMPLE_GRANULARITY * MAX_CHANNELS) as usize;

/// Active voices when the mixer is created (`mMaxActiveVoices`).
pub(crate) const DEFAULT_MAX_ACTIVE: usize = 16;

/// The voice table: every slot, the active set chosen for mixing, and the
/// resample blocks lent to active voices.
#[derive(Debug)]
pub(crate) struct Voices {
    pub slots: Vec<Option<Voice>>,
    /// One past the highest slot that may hold a voice (`mHighestVoice`).
    pub highest: usize,
    pub play_index: u32,
    /// Slots to mix, loudest first after sorting. Entries past `active_count`
    /// are left over from earlier passes and still keep resample blocks
    /// assigned, as in SoLoud.
    pub active: Vec<u32>,
    pub active_count: usize,
    /// The active set must be recomputed before the next mix.
    pub dirty: bool,
    pub pool: ResamplePool,
    /// Audio of stopped voices, when the owner disposes of it elsewhere.
    pub retired: Retired,
    /// `find_free` stopped a voice to make room since the flag was last
    /// taken.
    pub stole: bool,
    pub groups: VoiceGroups,
    /// Sounds queued on queues played on this mixer.
    pub queues: Queues,
}

/// Two resample blocks per active voice (`mResampleData`), each owned by at
/// most one voice slot (`mResampleDataOwner`).
#[derive(Debug)]
pub(crate) struct ResamplePool {
    pub data: Vec<f32>,
    pub owners: Vec<Option<usize>>,
    live: Vec<u8>,
}

impl ResamplePool {
    fn new(max_active: usize) -> Self {
        Self {
            data: vec![0.0; max_active * 2 * BLOCK_LEN],
            owners: vec![None; max_active],
            live: vec![0; max_active],
        }
    }
}

impl Voices {
    pub fn new() -> Self {
        Self {
            slots: (0..VOICE_COUNT).map(|_| None).collect(),
            highest: 0,
            play_index: 0,
            active: vec![0; VOICE_COUNT],
            active_count: 0,
            dirty: true,
            pool: ResamplePool::new(DEFAULT_MAX_ACTIVE),
            retired: Retired::default(),
            stole: false,
            groups: VoiceGroups::default(),
            queues: Queues::default(),
        }
    }

    pub fn max_active(&self) -> usize {
        self.pool.owners.len()
    }

    /// `setMaxActiveVoiceCount`: 1 to `VOICE_COUNT - 1`. Every voice loses its
    /// resample blocks and gets fresh, silent ones when next mixed.
    pub fn set_max_active(&mut self, count: usize) -> bool {
        if count == 0 || count >= VOICE_COUNT {
            return false;
        }
        self.pool = ResamplePool::new(count);
        for voice in self.slots.iter_mut().flatten() {
            voice.resample = None;
        }
        self.dirty = true;
        true
    }

    /// The slot `handle` refers to, if its voice is still playing
    /// (`getVoiceFromHandle_internal`). A voice group stands for its first
    /// member, even one that has stopped.
    pub fn slot_of(&self, handle: Handle) -> Option<usize> {
        let handle = match self.groups.index_of(handle) {
            Some(group) => self.groups.member(group, 0)?,
            None => handle,
        };
        let slot = handle.slot()?;
        let voice = self.slots[slot].as_ref()?;
        (voice.play_index & 0xfffff == handle.play_index()).then_some(slot)
    }

    pub fn get(&self, handle: Handle) -> Option<&Voice> {
        self.slot_of(handle)
            .and_then(|slot| self.slots[slot].as_ref())
    }

    #[cfg(test)]
    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut Voice> {
        let slot = self.slot_of(handle)?;
        self.slots[slot].as_mut()
    }

    /// Calls `change` with the slot of `handle`'s voice or, for a voice
    /// group, of each member still playing (`FOR_ALL_VOICES_PRE`).
    pub fn for_each_slot(&mut self, handle: Handle, mut change: impl FnMut(&mut Self, usize)) {
        let group = self.groups.index_of(handle);
        for i in 0.. {
            let member = match group {
                Some(group) => match self.groups.member(group, i) {
                    Some(member) => member,
                    None => break,
                },
                None if i == 0 => handle,
                None => break,
            };
            if let Some(slot) = self.slot_of(member) {
                change(self, slot);
            }
        }
    }

    /// [`Voices::for_each_slot`] for changes to the voice alone.
    pub fn for_each_voice(&mut self, handle: Handle, mut change: impl FnMut(&mut Voice)) {
        self.for_each_slot(handle, |voices, slot| {
            if let Some(voice) = voices.slots[slot].as_mut() {
                change(voice);
            }
        });
    }

    /// Stops every voice of the sound `id` (`Soloud::stopAudioSource`).
    pub fn stop_sound(&mut self, id: SoundId) {
        for slot in 0..self.highest {
            if self.slots[slot].as_ref().is_some_and(|v| v.sound_id == id) {
                self.stop(slot);
            }
        }
    }

    /// A free slot, or the oldest unprotected voice's slot after stopping it
    /// (`findFreeVoice_internal`). `None` when every voice is protected
    /// (reference patch 0020).
    pub fn find_free(&mut self) -> Option<usize> {
        if self.highest > 0 && self.slots[self.highest - 1].is_none() {
            self.highest -= 1;
        }
        let mut oldest: Option<(u32, usize)> = None;
        for (i, slot) in self.slots.iter().enumerate() {
            let Some(voice) = slot else {
                self.highest = self.highest.max(i + 1);
                return Some(i);
            };
            if !voice.has_flag(flags::PROTECTED)
                && oldest.is_none_or(|(index, _)| voice.play_index < index)
            {
                oldest = Some((voice.play_index, i));
            }
        }
        let (_, slot) = oldest?;
        self.stop(slot);
        self.stole = true;
        Some(slot)
    }

    /// `stopVoice_internal`: frees the slot and its resample blocks. A bus
    /// voice whose handle is known also stops the voices playing through it
    /// (`~BusInstance`).
    pub fn stop(&mut self, slot: usize) {
        self.dirty = true;
        if let Some(voice) = self.slots[slot].take() {
            for owner in &mut self.pool.owners {
                if *owner == Some(slot) {
                    *owner = None;
                }
            }
            let bus = voice.as_bus().and_then(|bus| bus.handle);
            self.retired.put_voice(voice);
            if let Some(bus) = bus {
                for child in 0..self.highest {
                    if self.slots[child].as_ref().is_some_and(|v| v.bus == bus) {
                        self.stop(child);
                    }
                }
            }
        }
    }

    /// Voices in the table, playing or paused (`getVoiceCount`).
    pub fn count(&self) -> usize {
        self.slots[..self.highest].iter().flatten().count()
    }

    /// Chooses which voices to mix (`calcActiveVoices_internal`): every
    /// audible, unpaused voice and every ticking voice if they fit, otherwise
    /// ticking and protected voices first and then the loudest by overall
    /// volume, using SoLoud's partial quicksort so ties resolve identically.
    pub fn calc_active(&mut self) {
        self.dirty = false;
        let max = self.max_active();
        let mut candidates = 0;
        let mut must_live = 0;
        for (i, slot) in self.slots[..self.highest].iter().enumerate() {
            let Some(voice) = slot else { continue };
            if voice.has_flag(flags::INAUDIBLE | flags::PAUSED)
                && !voice.has_flag(flags::INAUDIBLE_TICK)
            {
                continue;
            }
            self.active[candidates] = i as u32;
            candidates += 1;
            if voice.has_flag(flags::INAUDIBLE_TICK | flags::PROTECTED) {
                self.active[candidates - 1] = self.active[must_live];
                self.active[must_live] = i as u32;
                must_live += 1;
            }
        }

        if candidates <= max {
            self.active_count = candidates;
        } else {
            self.active_count = max;
            if must_live < max {
                let volume = |slot: u32| {
                    self.slots[slot as usize]
                        .as_ref()
                        .map_or(0.0, |voice| voice.overall_volume)
                };
                partial_sort(&mut self.active[must_live..candidates], max as i32, volume);
            }
        }
        self.map_resample_blocks();
    }

    /// Lends resample blocks to active voices that have none and takes them
    /// back from voices no longer listed (`mapResampleBuffers_internal`).
    /// Newly lent blocks are cleared.
    fn map_resample_blocks(&mut self) {
        let max = self.max_active();
        let pool = &mut self.pool;
        pool.live.fill(0);
        for i in 0..max {
            let Some(owner) = pool.owners[i] else {
                continue;
            };
            for j in 0..max {
                if owner == self.active[j] as usize {
                    pool.live[i] |= 1;
                    pool.live[j] |= 2;
                }
            }
        }
        for i in 0..max {
            if pool.live[i] & 1 == 0
                && let Some(owner) = pool.owners[i].take()
                && let Some(voice) = self.slots[owner].as_mut()
            {
                voice.resample = None;
            }
        }
        let mut first_free = 0;
        for i in 0..self.active_count {
            let slot = self.active[i] as usize;
            if pool.live[i] & 2 != 0 || self.slots[slot].is_none() {
                continue;
            }
            let Some(found) = (first_free..max).find(|&j| pool.owners[j].is_none()) else {
                continue;
            };
            pool.owners[found] = Some(slot);
            pool.data[found * 2 * BLOCK_LEN..(found + 1) * 2 * BLOCK_LEN].fill(0.0);
            if let Some(voice) = self.slots[slot].as_mut() {
                voice.resample = Some(ResampleBlocks {
                    current: found * 2,
                    previous: found * 2 + 1,
                });
            }
            first_free = found + 1;
        }
    }
}

/// SoLoud's iterative partial quicksort (descending by `volume`), which
/// orders at least the first `k` entries. Ported statement by statement so
/// equal volumes end up in the same order as in C++.
fn partial_sort(data: &mut [u32], k: i32, volume: impl Fn(u32) -> f32) {
    let mut left: i32 = 0;
    let mut len = data.len() as i32;
    let mut stack = [0_i32; 24];
    let mut pos = 0;
    loop {
        while left + 1 < len {
            if pos == 24 {
                pos = 0;
                len = stack[0];
            }
            let pivot = volume(data[left as usize]);
            stack[pos] = len;
            pos += 1;
            let mut right = left - 1;
            loop {
                right += 1;
                while volume(data[right as usize]) > pivot {
                    right += 1;
                }
                len -= 1;
                while pivot > volume(data[len as usize]) {
                    len -= 1;
                }
                if right >= len {
                    break;
                }
                data.swap(right as usize, len as usize);
            }
            len += 1;
        }
        if pos == 0 || left >= k {
            break;
        }
        left = len;
        pos -= 1;
        len = stack[pos];
    }
}

#[cfg(test)]
#[path = "voices_tests.rs"]
mod tests;
