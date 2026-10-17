//! Playback: entities with a [`SoundPlayer`] play while they exist.

mod components;
mod records;
mod spatial_sync;
mod sync;

use bevy::prelude::*;

pub use components::{
    FinishBehavior, PlaybackError, PlaybackSettings, PlaybackState, SoundFailed, SoundFinished,
    SoundPlayer, SoundStatus,
};

use crate::SolouderSystems;

pub(crate) fn build(app: &mut App) {
    app.init_resource::<records::Playback>()
        .add_message::<SoundFinished>()
        .add_message::<SoundFailed>()
        .add_systems(
            PostUpdate,
            sync::sync_playback
                .in_set(SolouderSystems::Sync)
                .after(crate::features::spatial::take_snapshot),
        );
}
