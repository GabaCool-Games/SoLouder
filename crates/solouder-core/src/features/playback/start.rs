//! Starting voices prepared on another thread, for the real-time renderer.
//! Nothing here allocates or frees: the voice arrives in a box that the
//! mixer keeps, and retires the voice in, when the voice ends.

use std::collections::VecDeque;

use super::control::PlayOptions;
use super::handle::Handle;
use super::mixer::Mixer;
use super::queue::Queued;
use super::queue_control::QueueError;
use super::sound::SoundId;
use super::source::private::Prepared;
use super::voice::Voice;
use crate::features::filters::{FILTERS_PER_STREAM, FilterInstance};

/// A [`Prepared`] voice, boxed so it moves between threads without being
/// copied; the box comes back with the voice when it ends.
#[derive(Debug)]
pub(crate) struct BoxedVoice {
    pub voice: Box<Option<Voice>>,
    pub stop_others: bool,
    pub default_volume: f32,
}

impl From<Prepared> for BoxedVoice {
    fn from(prepared: Prepared) -> Self {
        Self {
            voice: Box::new(Some(prepared.voice)),
            stop_others: prepared.stop_others,
            default_volume: prepared.default_volume,
        }
    }
}

/// How a prepared voice starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Start {
    Now,
    /// [`Mixer::play_clocked`] at this sound time.
    Clocked(f64),
    /// [`Mixer::play_background`]: full gain on the left and right speakers.
    Background,
}

impl Mixer {
    /// Starts a prepared voice, through the bus `bus` if given; a bus that
    /// is not playing fails the play. Returns [`Handle::INVALID`] if the
    /// voice did not start; it is then retired.
    pub(crate) fn start_boxed(
        &mut self,
        boxed: BoxedVoice,
        mut options: PlayOptions,
        start: Start,
        bus: Option<SoundId>,
    ) -> Handle {
        let BoxedVoice {
            voice: mut slot,
            stop_others,
            default_volume,
        } = boxed;
        let voice = slot.take();
        self.voices.retired.adopt_box(slot);
        let Some(voice) = voice else {
            return Handle::INVALID;
        };
        if let Some(bus) = bus {
            match self.bus_handle_of(bus) {
                Some(handle) => options.bus = handle,
                None => {
                    self.voices.retired.put_voice(voice);
                    return Handle::INVALID;
                }
            }
        }
        match start {
            Start::Now => self.start_voice(voice, stop_others, default_volume, options),
            Start::Background => {
                let handle = self.start_voice(voice, stop_others, default_volume, options);
                self.set_pan_absolute(handle, 1.0, 1.0);
                handle
            }
            Start::Clocked(sound_time) => {
                let paused = options.paused;
                options.paused = true;
                let handle = self.start_voice(voice, stop_others, default_volume, options);
                self.clock(handle, sound_time, paused);
                handle
            }
        }
    }

    /// Replaces global filter `slot`'s instance, retiring the old one (or
    /// `instance`, for a slot that does not exist).
    pub(crate) fn swap_global_filter(
        &mut self,
        slot: u32,
        instance: Option<Box<dyn FilterInstance>>,
    ) {
        let old = match self.global_filters.get_mut(slot as usize) {
            Some(entry) => std::mem::replace(entry, instance),
            None => instance,
        };
        self.voices.retired.put_filter(old);
    }

    /// Replaces filter `slot` of the bus voice playing `bus`, retiring the
    /// old instance (or `instance`, if there is no such voice or slot).
    pub(crate) fn swap_bus_filter(
        &mut self,
        bus: SoundId,
        slot: u32,
        instance: Option<Box<dyn FilterInstance>>,
    ) {
        let entry = (slot as usize) < FILTERS_PER_STREAM;
        let old = match self.bus_voice_mut_of(bus) {
            Some(voice) if entry => std::mem::replace(&mut voice.filters[slot as usize], instance),
            _ => instance,
        };
        self.voices.retired.put_filter(old);
    }

    /// Items the mixer will retire without a new request: one for each
    /// voice playing and each sound waiting in a queue, plus those already
    /// kept.
    pub(crate) fn pending_retirements(&self) -> usize {
        let voices = &self.voices;
        voices.count() + voices.queues.queued() + voices.retired.kept_len()
    }

    /// The queues the mixer already has storage for.
    pub(crate) fn queue_ids(&self) -> Vec<SoundId> {
        self.voices.queues.ids().collect()
    }

    /// Room for `queues` more queues' storage.
    pub(crate) fn reserve_queues(&mut self, queues: usize) {
        self.voices.queues.reserve(queues);
    }

    /// Makes room for the sounds of queue `id` without allocating later.
    /// Storage for a queue that has some already is retired.
    pub(crate) fn add_queue_storage(&mut self, id: SoundId, storage: VecDeque<Queued>) {
        if let Some(storage) = self.voices.queues.insert(id, storage) {
            self.voices.retired.put_queue_storage(storage);
        }
    }

    /// [`Mixer::queue_play`] for a sound prepared elsewhere, into storage
    /// added with [`Mixer::add_queue_storage`]. A sound that cannot be
    /// queued is retired.
    pub(crate) fn queue_push(&mut self, queue: SoundId, item: Queued) -> Result<(), QueueError> {
        let result = if !self.queue_id_is_playing(queue) {
            Err(QueueError::NotPlaying)
        } else if self.voices.queues.count(queue) >= super::QUEUE_MAX {
            Err(QueueError::Full)
        } else {
            match self.voices.queues.push_existing(queue, item) {
                Ok(()) => return Ok(()),
                Err(item) => {
                    self.voices.retired.put(item.into_pcm());
                    return Err(QueueError::NotPlaying);
                }
            }
        };
        self.voices.retired.put(item.into_pcm());
        result
    }
}
