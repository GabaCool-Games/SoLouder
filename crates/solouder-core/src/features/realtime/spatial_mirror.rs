//! The controller's copy of the 3D state, and the batches that carry each
//! 3D update's results to the renderer.
//!
//! The renderer never computes 3D gains: the listener, the speakers and the
//! 3D settings of every voice played in 3D through the controller live
//! here, on the game thread, where colliders and attenuators run. An update
//! fills one preallocated batch with every such voice's gains and sends it
//! as one command; the renderer applies it and hands the batch back through
//! its own queue, so neither side allocates or frees for it.
//!
//! A rising Doppler factor raises the rate a voice's filters run at, and
//! some filters (echo, DC removal, flanger) need memory in proportion to
//! it. The mirror knows the rate each voice's filters were prepared for;
//! when an update would take a voice past it, the batch also carries
//! memory prepared here, which the renderer swaps into the voice's filter
//! instances before applying the gains. The memory they no longer use rides
//! back in the batch and is freed here.

use std::sync::Arc;

use rtrb::Consumer;

use super::VoiceId;
use crate::features::filters::{FILTERS_PER_STREAM, Filter};
use crate::features::playback::{FilterSlots, Handle, SoundId};
use crate::features::spatial::{Gains, Scene, VoiceSpatial};

/// How far past the rate it needs a voice's filters are prepared once a
/// Doppler factor outgrows them, so a steadily rising factor sends memory
/// every few updates rather than every update.
const HEADROOM: f32 = 1.25;

/// Memory for a voice's filters to grow into, by slot.
pub(crate) type FilterSpares = [Option<Vec<f32>>; FILTERS_PER_STREAM];

/// Memory for one voice's filters, sent with an update.
#[derive(Debug)]
pub(crate) struct Growth {
    pub id: VoiceId,
    /// Filled here; after the renderer applied it, the memory the filters
    /// gave up.
    pub spares: FilterSpares,
    /// The rate the filters were prepared for before.
    previous: f32,
}

/// One 3D update's results: the gains of each voice, by ID, and memory for
/// the filters of the voices whose Doppler factor outgrew them.
#[derive(Debug)]
pub(crate) struct SpatialBatch {
    pub entries: Vec<(VoiceId, Gains)>,
    pub growth: Vec<Growth>,
}

/// What a voice's filter instances were prepared for, and what may raise
/// the rate they run at.
#[derive(Clone, Debug)]
pub(super) struct FilterLayout {
    /// The filters the voice's instances were created from.
    pub filters: Option<Arc<FilterSlots>>,
    pub channels: usize,
    /// The voice's sample rate before speed.
    pub base_rate: f32,
    /// The highest play speed set, faded or oscillated to.
    pub speed: f32,
    /// The rate the filter instances have memory for.
    pub reserved: f32,
    /// The bus whose voice this is, for a bus.
    pub bus: Option<SoundId>,
}

/// A voice played in 3D through the controller.
#[derive(Debug)]
struct MirrorVoice {
    id: VoiceId,
    spatial: VoiceSpatial,
    listener_relative: bool,
    layout: FilterLayout,
    /// The Doppler factor of the last update sent, at least 1.
    doppler: f32,
}

#[derive(Debug)]
pub(crate) struct SpatialMirror {
    /// The listener and speakers, as the renderer's mixer had them at
    /// `split` and as the controller has changed them since.
    pub scene: Scene,
    /// By voice ID index; preallocated for every ID.
    voices: Vec<Option<MirrorVoice>>,
    /// Batches not in flight. Kept boxed: they travel through the queues
    /// as boxes, and unboxing one would mean allocating to send it again.
    #[allow(clippy::vec_box)]
    free: Vec<Box<SpatialBatch>>,
    /// Batches the renderer has applied.
    returned: Consumer<Box<SpatialBatch>>,
}

impl SpatialMirror {
    /// `batches` batches, each with room for `voice_ids` voices.
    pub fn new(
        scene: Scene,
        voice_ids: usize,
        batches: usize,
        returned: Consumer<Box<SpatialBatch>>,
    ) -> Self {
        let free = (0..batches)
            .map(|_| {
                Box::new(SpatialBatch {
                    entries: Vec::with_capacity(voice_ids),
                    growth: Vec::new(),
                })
            })
            .collect();
        Self {
            scene,
            voices: (0..voice_ids).map(|_| None).collect(),
            free,
            returned,
        }
    }

    /// Starts tracking a voice just sent to play in 3D with Doppler factor
    /// `doppler`.
    pub fn insert(
        &mut self,
        id: VoiceId,
        spatial: VoiceSpatial,
        listener_relative: bool,
        layout: FilterLayout,
        doppler: f32,
    ) {
        self.voices[id.index()] = Some(MirrorVoice {
            id,
            spatial,
            listener_relative,
            layout,
            doppler: doppler.max(1.0),
        });
    }

    /// Stops tracking a voice whose final event was read.
    pub fn forget(&mut self, id: VoiceId) {
        let entry = &mut self.voices[id.index()];
        if entry.as_ref().is_some_and(|voice| voice.id == id) {
            *entry = None;
        }
    }

    fn voice(&mut self, id: VoiceId) -> Option<&mut MirrorVoice> {
        self.voices
            .get_mut(id.index())?
            .as_mut()
            .filter(|voice| voice.id == id)
    }

    /// The 3D settings of voice `id`, if it was played in 3D and its final
    /// event has not been read.
    pub fn voice_mut(&mut self, id: VoiceId) -> Option<&mut VoiceSpatial> {
        self.voice(id).map(|voice| &mut voice.spatial)
    }

    /// The Doppler factor last sent for voice `id`, at least 1; 1 for a
    /// voice not tracked here.
    pub fn doppler(&mut self, id: VoiceId) -> f32 {
        self.voice(id).map_or(1.0, |voice| voice.doppler)
    }

    /// The voice's play speed was set to `speed`, or may reach it if
    /// `raise`.
    pub fn set_speed(&mut self, id: VoiceId, speed: f32, raise: bool) {
        if let Some(voice) = self.voice(id)
            && speed.is_finite()
            && speed > 0.0
        {
            let layout = &mut voice.layout;
            layout.speed = if raise {
                layout.speed.max(speed)
            } else {
                speed
            };
        }
    }

    /// The voice's sample rate before speed is now `rate`.
    pub fn set_sample_rate(&mut self, id: VoiceId, rate: f32) {
        if let Some(voice) = self.voice(id)
            && rate.is_finite()
            && rate > 0.0
        {
            voice.layout.base_rate = rate;
        }
    }

    /// Bus `bus`'s filter `slot` was replaced, on its playing voice too.
    pub fn set_bus_filter(&mut self, bus: SoundId, slot: usize, filter: Option<Arc<dyn Filter>>) {
        for voice in self.voices.iter_mut().flatten() {
            if voice.layout.bus == Some(bus) && slot < FILTERS_PER_STREAM {
                let filters = voice.layout.filters.get_or_insert_default();
                Arc::make_mut(filters)[slot] = filter.clone();
            }
        }
    }

    /// The bus a tracked voice plays, and the speed its filters may run at
    /// with Doppler factor `doppler`.
    pub fn bus_speed(&mut self, id: VoiceId, doppler: f32) -> Option<f32> {
        let voice = self.voice(id)?;
        voice.layout.bus?;
        Some(voice.layout.speed * doppler)
    }

    /// A batch filled with the gains of every tracked voice, computed from
    /// the current listener and speakers, and with memory for the filters
    /// those gains would run faster than they were prepared for; `None`
    /// while every batch is still in flight.
    pub fn take_batch(&mut self) -> Option<Box<SpatialBatch>> {
        while let Ok(mut batch) = self.returned.pop() {
            // Frees the memory the filters gave up.
            batch.growth.clear();
            self.free.push(batch);
        }
        let mut batch = self.free.pop()?;
        batch.entries.clear();
        let frame = self.scene.frame();
        for voice in self.voices.iter_mut().flatten() {
            let gains = frame.gains(
                &voice.spatial,
                voice.listener_relative,
                Handle::INVALID,
                Some(voice.id),
            );
            batch.entries.push((voice.id, gains));
            voice.doppler = gains.doppler.max(1.0);
            let layout = &mut voice.layout;
            let needed = layout.base_rate * layout.speed * gains.doppler;
            if needed.is_finite() && needed > layout.reserved && has_filters(layout) {
                let reserved = needed * HEADROOM;
                batch.growth.push(Growth {
                    id: voice.id,
                    spares: spares(layout, reserved),
                    previous: layout.reserved,
                });
                layout.reserved = reserved;
            }
        }
        Some(batch)
    }

    /// A batch that could not be sent: the voices it would have grown keep
    /// their old reservations.
    pub fn give_back(&mut self, mut batch: Box<SpatialBatch>) {
        for growth in batch.growth.drain(..) {
            if let Some(voice) = self.voice(growth.id) {
                voice.layout.reserved = growth.previous;
            }
        }
        self.free.push(batch);
    }
}

fn has_filters(layout: &FilterLayout) -> bool {
    layout
        .filters
        .as_ref()
        .is_some_and(|filters| filters.iter().any(Option::is_some))
}

/// Memory for `layout`'s filters at `rate`, from instances prepared for it.
/// Filters that need none at any rate have no spare.
fn spares(layout: &FilterLayout, rate: f32) -> FilterSpares {
    std::array::from_fn(|slot| {
        let filter = layout.filters.as_ref()?[slot].as_ref()?;
        let mut instance = filter.create_instance();
        instance.prepare(layout.channels, rate);
        instance.take_reserved()
    })
}
