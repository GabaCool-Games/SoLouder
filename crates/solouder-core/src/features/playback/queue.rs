// Ported from SoLoud src/core/soloud_queue.cpp and
// src/core/soloud_audiosource.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::collections::VecDeque;

use super::control::ControlError;
use super::queued::Queued;
use super::retire::Retired;
use super::sound::{Sound, SoundId};
use super::source::private::{Prepared, Sealed};
use super::source::{Playable, SourceSettings, source_settings_api};
use super::voice::{Voice, VoiceSource, flags};
use crate::contracts::{MAX_CHANNELS, SAMPLE_GRANULARITY};
use crate::dsp::c_int;

const BLOCK: usize = SAMPLE_GRANULARITY as usize;
/// Most sounds a queue holds (`SOLOUD_QUEUE_MAX`).
pub const QUEUE_MAX: usize = 32;

/// A queue (`Queue`): sounds queued on it play one after another, gapless,
/// as one voice. The sounds are not resampled or remixed: they play at the
/// queue's sample rate on its first channels, so they should match its
/// parameters ([`Queue::set_params`]).
///
/// Start a queue with [`Mixer::play_with`], then add sounds with
/// [`Mixer::queue_play`]. The queued sounds belong to the queue on that
/// mixer: they stay queued when its voice stops and continue when it plays
/// again. The voice stops once at least one sound has finished and none is
/// left. A queue has one voice per mixer: playing it again stops the earlier
/// one. Queue voices are protected.
#[derive(Clone, Debug)]
pub struct Queue {
    id: SoundId,
    settings: SourceSettings,
    channels: u32,
    sample_rate: f32,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

impl Queue {
    /// A mono queue at 44100 Hz.
    pub fn new() -> Self {
        Self {
            id: SoundId::next(),
            settings: SourceSettings::default(),
            channels: 1,
            sample_rate: 44100.0,
        }
    }

    /// Shared by this queue and its clones.
    pub fn id(&self) -> SoundId {
        self.id
    }

    source_settings_api!();

    /// Channels of queue voices.
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// The rate queued sounds play at, before speed.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Sets the rate and channels (1 to 8) of queue voices started after
    /// this call (`Queue::setParams`). Any rate is accepted; one that is not
    /// greater than 0 renders silence.
    pub fn set_params(&mut self, sample_rate: f32, channels: u32) -> Result<(), ControlError> {
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err(ControlError::InvalidChannels);
        }
        self.channels = channels;
        self.sample_rate = sample_rate;
        Ok(())
    }

    /// Takes the rate and channels of `sound` (`Queue::setParamsFromAudioSource`).
    pub fn set_params_from(&mut self, sound: &Sound) {
        self.channels = sound.pcm().channels();
        self.sample_rate = sound.pcm().sample_rate();
    }
}

impl Playable for Queue {
    fn source_id(&self) -> SoundId {
        self.id
    }
}

impl Sealed for Queue {
    fn filter_slots(&self) -> Option<std::sync::Arc<super::source::FilterSlots>> {
        self.settings.filters.clone()
    }

    fn prepare(&self) -> Prepared {
        let source = VoiceSource::Queue {
            channels: self.channels,
        };
        Prepared {
            voice: Voice::new(
                &self.settings,
                self.id,
                source,
                self.sample_rate,
                flags::PROTECTED,
            ),
            stop_others: true,
            default_volume: self.settings.volume,
        }
    }

    fn queue_id(&self) -> Option<SoundId> {
        Some(self.id)
    }
}

/// The sounds queued on each queue played on a mixer (`Queue::mSource`).
#[derive(Debug, Default)]
pub(crate) struct Queues(Vec<(SoundId, VecDeque<Queued>)>);

impl Queues {
    pub fn get(&self, id: SoundId) -> Option<&VecDeque<Queued>> {
        self.0
            .iter()
            .find(|(q, _)| *q == id)
            .map(|(_, items)| items)
    }

    pub fn count(&self, id: SoundId) -> usize {
        self.get(id).map_or(0, VecDeque::len)
    }

    /// The queues that have storage.
    pub fn ids(&self) -> impl Iterator<Item = SoundId> + '_ {
        self.0.iter().map(|(id, _)| *id)
    }

    /// Every sound waiting in a queue.
    pub fn items_mut(&mut self) -> impl Iterator<Item = &mut Queued> {
        self.0.iter_mut().flat_map(|(_, items)| items.iter_mut())
    }

    /// Sounds waiting in every queue.
    pub fn queued(&self) -> usize {
        self.0.iter().map(|(_, items)| items.len()).sum()
    }

    /// Adds storage for queue `id`, unless it has some, in which case
    /// `storage` is handed back. Does not allocate if
    /// [`Queues::reserve`] made room.
    pub fn insert(&mut self, id: SoundId, storage: VecDeque<Queued>) -> Option<VecDeque<Queued>> {
        if self.get(id).is_some() || self.0.len() == self.0.capacity() {
            return Some(storage);
        }
        self.0.push((id, storage));
        None
    }

    /// Room for `queues` more queues.
    pub fn reserve(&mut self, queues: usize) {
        self.0.reserve_exact(queues);
    }

    /// Queues `item` in existing storage without growing it.
    pub fn push_existing(&mut self, id: SoundId, item: Queued) -> Result<(), Queued> {
        match self.0.iter_mut().find(|(q, _)| *q == id) {
            Some((_, items)) if items.len() < items.capacity() => {
                items.push_back(item);
                Ok(())
            }
            _ => Err(item),
        }
    }

    pub fn push(&mut self, id: SoundId, item: Queued) {
        match self.0.iter_mut().find(|(q, _)| *q == id) {
            Some((_, items)) => items.push_back(item),
            None => {
                let mut items = VecDeque::with_capacity(QUEUE_MAX);
                items.push_back(item);
                self.0.push((id, items));
            }
        }
    }

    /// `QueueInstance::getAudio`: reads up to `frames` frames from the
    /// queued sounds in turn into `out` (channel `c` at `c * stride`, from
    /// `start`), or skips them without `out`. A sound that ends, or gives no
    /// audio (reference patch 0049), is dropped and counts as a loop. The
    /// queue's channels a sound does not have are silent (patch 0050).
    #[allow(clippy::too_many_arguments)]
    pub fn read(
        &mut self,
        id: SoundId,
        mut out: Option<&mut [f32]>,
        start: usize,
        frames: u32,
        stride: usize,
        channels: usize,
        loop_count: &mut u32,
        retired: &mut Retired,
    ) -> u32 {
        let Some((_, items)) = self.0.iter_mut().find(|(q, _)| *q == id) else {
            return 0;
        };
        let mut copied = 0;
        while copied < frames {
            let Some(item) = items.front_mut() else {
                break;
            };
            let at = start + copied as usize;
            let count = item.read(out.as_deref_mut(), at, stride, frames - copied, channels);
            copied += count;
            if count == 0 || item.has_ended() {
                if let Some(item) = items.pop_front() {
                    retired.put_queued(item);
                }
                *loop_count = loop_count.wrapping_add(1);
            }
        }
        copied
    }
}

/// `QueueInstance::hasEnded`: a sound has finished and none is left.
pub(crate) fn has_ended(voice: &Voice, queues: &Queues) -> bool {
    voice.loop_count != 0 && queues.count(voice.sound_id) == 0
}

/// SoLoud's generic seek on a queue voice (`AudioSourceInstance::seek`),
/// with a scratch buffer of `scratch` floats. A queue cannot rewind, so
/// seeking back does nothing and returns false. Seeking forward skips the
/// frames in passes of `scratch` divided by the queue's channels.
pub(crate) fn seek(
    voice: &mut Voice,
    queues: &mut Queues,
    retired: &mut Retired,
    seconds: f64,
    scratch: usize,
) -> bool {
    let offset = seconds - voice.stream_position;
    if offset <= 0.0 {
        return false;
    }
    let channels = voice.channels();
    let pass = (scratch / channels) as i32;
    let mut remaining = c_int((f64::from(voice.base_sample_rate) * offset).floor());
    while remaining > 0 {
        let frames = pass.min(remaining);
        let loop_count = &mut voice.loop_count;
        queues.read(
            voice.sound_id,
            None,
            0,
            frames as u32,
            0,
            channels,
            loop_count,
            retired,
        );
        remaining -= frames;
    }
    voice.stream_position = seconds;
    true
}

/// The block size a queue's loop seek uses (`mSeekScratch`).
pub(crate) const LOOP_SEEK_SCRATCH: usize = BLOCK * MAX_CHANNELS as usize;

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
