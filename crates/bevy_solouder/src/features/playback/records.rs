//! The plugin's own record of every player, kept apart from the components
//! so that removing or replacing [`SoundStatus`] can never orphan a voice.
//!
//! Hooks keep the records in step with [`SoundPlayer`]: inserting a player
//! creates a record and queues it for the next sync; discarding it (removal,
//! replacement or despawn) drops the record and queues a stop for its voice.

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use solouder_core::VoiceId;

use super::components::{PlaybackSettings, PlaybackState, SoundStatus};
use crate::features::spatial::{EmitterPose, ListenerPose};

/// One player's playback, as far as the plugin knows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Record {
    pub state: PlaybackState,
    /// The mixer voice, from the play request until its final event.
    pub voice: Option<VoiceId>,
    /// What the mixer will have once the accepted commands run.
    pub sent: Option<PlaybackSettings>,
    /// Whether the entity is in [`Playback::work`].
    pub queued: bool,
    /// Whether the sound was started in 3D.
    pub spatial: bool,
    /// The emitter pose the controller last has for the 3D voice.
    pub placed: Option<EmitterPose>,
}

impl Record {
    fn new() -> Self {
        Self {
            state: PlaybackState::Loading,
            voice: None,
            sent: None,
            queued: false,
            spatial: false,
            placed: None,
        }
    }

    /// Whether the sound may still start or is playing.
    pub fn is_live(&self) -> bool {
        matches!(
            self.state,
            PlaybackState::Loading | PlaybackState::Starting | PlaybackState::Playing
        )
    }
}

#[derive(Resource, Default, Debug)]
pub(crate) struct Playback {
    /// Every entity that has a [`SoundPlayer`](super::SoundPlayer).
    pub records: HashMap<Entity, Record>,
    /// The entity each reserved voice was started for, kept until the
    /// voice's final event so its message names the entity even after the
    /// record is gone.
    pub owners: HashMap<VoiceId, Entity>,
    /// Voices of discarded players, to stop at the next sync.
    pub stops: Vec<VoiceId>,
    /// Entities to look at in the next sync: waiting to start, or with
    /// settings the mixer has not yet been sent.
    pub work: Vec<Entity>,
    /// The output was found dead and every live record was failed.
    pub dead: bool,
    /// The listener and speed of sound of the last 3D update the
    /// controller accepted.
    pub scene: Option<(ListenerPose, f32)>,
    /// A 3D update the controller refused, to send again.
    pub update_pending: bool,
}

impl Playback {
    /// Adds the entity to the next sync's work, once.
    pub fn queue(&mut self, entity: Entity) {
        if let Some(record) = self.records.get_mut(&entity)
            && !record.queued
        {
            record.queued = true;
            self.work.push(entity);
        }
    }
}

pub(crate) fn player_inserted(mut world: DeferredWorld, context: HookContext) {
    let entity = context.entity;
    let Some(mut playback) = world.get_resource_mut::<Playback>() else {
        return;
    };
    if let Some(old) = playback.records.insert(entity, Record::new())
        && let Some(voice) = old.voice
    {
        playback.stops.push(voice);
    }
    playback.queue(entity);
    mirror(&mut world, entity);
}

pub(crate) fn player_discarded(mut world: DeferredWorld, context: HookContext) {
    if let Some(mut playback) = world.get_resource_mut::<Playback>()
        && let Some(record) = playback.records.remove(&context.entity)
        && let Some(voice) = record.voice
    {
        playback.stops.push(voice);
    }
}

pub(crate) fn status_inserted(mut world: DeferredWorld, context: HookContext) {
    mirror(&mut world, context.entity);
}

/// Copies the record into the entity's [`SoundStatus`], if both exist.
fn mirror(world: &mut DeferredWorld, entity: Entity) {
    let Some(record) = world
        .get_resource::<Playback>()
        .and_then(|playback| playback.records.get(&entity).copied())
    else {
        return;
    };
    if let Some(status) = world.get::<SoundStatus>(entity)
        && !status.shows(&record)
        && let Some(mut status) = world.get_mut::<SoundStatus>(entity)
    {
        status.show(&record);
    }
}
