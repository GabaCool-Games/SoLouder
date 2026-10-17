//! The controller's copy of the 3D state, and the batches that carry each
//! 3D update's results to the renderer.
//!
//! The renderer never computes 3D gains: the listener, the speakers and the
//! 3D settings of every voice played in 3D through the controller live
//! here, on the game thread, where colliders and attenuators run. An update
//! fills one preallocated batch with every such voice's gains and sends it
//! as one command; the renderer applies it and hands the batch back through
//! its own queue, so neither side allocates or frees for it.

use rtrb::Consumer;

use super::VoiceId;
use crate::features::playback::Handle;
use crate::features::spatial::{Gains, Scene, VoiceSpatial};

/// One 3D update's results: the gains of each voice, by ID.
#[derive(Debug)]
pub(crate) struct SpatialBatch {
    pub entries: Vec<(VoiceId, Gains)>,
}

/// A voice played in 3D through the controller.
#[derive(Debug)]
struct MirrorVoice {
    id: VoiceId,
    spatial: VoiceSpatial,
    listener_relative: bool,
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

    /// Starts tracking a voice just sent to play in 3D.
    pub fn insert(&mut self, id: VoiceId, spatial: VoiceSpatial, listener_relative: bool) {
        self.voices[id.index()] = Some(MirrorVoice {
            id,
            spatial,
            listener_relative,
        });
    }

    /// Stops tracking a voice whose final event was read.
    pub fn forget(&mut self, id: VoiceId) {
        let entry = &mut self.voices[id.index()];
        if entry.as_ref().is_some_and(|voice| voice.id == id) {
            *entry = None;
        }
    }

    /// The 3D settings of voice `id`, if it was played in 3D and its final
    /// event has not been read.
    pub fn voice_mut(&mut self, id: VoiceId) -> Option<&mut VoiceSpatial> {
        self.voices
            .get_mut(id.index())?
            .as_mut()
            .filter(|voice| voice.id == id)
            .map(|voice| &mut voice.spatial)
    }

    /// A batch filled with the gains of every tracked voice, computed from
    /// the current listener and speakers; `None` while every batch is still
    /// in flight.
    pub fn take_batch(&mut self) -> Option<Box<SpatialBatch>> {
        while let Ok(batch) = self.returned.pop() {
            self.free.push(batch);
        }
        let mut batch = self.free.pop()?;
        batch.entries.clear();
        let frame = self.scene.frame();
        for voice in self.voices.iter().flatten() {
            let gains = frame.gains(
                &voice.spatial,
                voice.listener_relative,
                Handle::INVALID,
                Some(voice.id),
            );
            batch.entries.push((voice.id, gains));
        }
        Some(batch)
    }

    /// A batch that could not be sent.
    pub fn give_back(&mut self, batch: Box<SpatialBatch>) {
        self.free.push(batch);
    }
}
