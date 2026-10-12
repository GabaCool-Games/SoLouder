//! Playback components and messages.

use bevy::prelude::*;
use solouder_core::{FinishReason, VoiceId};

use super::records::{Record, player_discarded, player_inserted, status_inserted};
use crate::features::decoded_audio::AudioClip;

/// Plays a clip for as long as this component stays on the entity.
///
/// Removing the component, replacing it, or despawning the entity stops the
/// sound, including one still waiting for its asset to load. The component
/// is immutable: insert a new one to play another clip.
#[derive(Component, Clone, Debug)]
#[component(immutable, on_insert = player_inserted, on_discard = player_discarded)]
#[require(PlaybackSettings, SoundStatus = SoundStatus::LOADING)]
pub struct SoundPlayer(pub Handle<AudioClip>);

/// What to do with the entity once its sound is over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FinishBehavior {
    /// Keep the entity; its status says how the sound ended.
    #[default]
    Keep,
    /// Despawn the entity when the sound ends for any reason: played to the
    /// end, stopped, stolen, or failed. The [`SoundFinished`] or
    /// [`SoundFailed`] message still says why.
    Despawn,
}

/// Requested playback settings. Edits are sent to the mixer in the next
/// sync; the mixer applies them with its usual volume and pan ramps.
///
/// Values the mixer cannot use are ignored and the voice keeps its current
/// setting: a volume or pan that is not finite, or a speed that is not
/// greater than 0. A sound that starts with one plays at volume 1, centre
/// pan or speed 1.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PlaybackSettings {
    /// Voice volume; 1 plays the clip as recorded.
    pub volume: f32,
    /// Stereo pan from -1 (left) to 1 (right).
    pub pan: f32,
    /// Playback speed relative to the clip's sample rate; must be positive.
    pub speed: f32,
    /// Keep the voice but stop mixing it.
    pub paused: bool,
    /// Restart at the end instead of finishing.
    pub looping: bool,
    /// Never steal this voice for a new one.
    pub protect: bool,
    /// What happens to the entity at the end.
    pub on_finish: FinishBehavior,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            speed: 1.0,
            paused: false,
            looping: false,
            protect: false,
            on_finish: FinishBehavior::Keep,
        }
    }
}

impl PlaybackSettings {
    /// Default settings with looping on.
    pub fn looping() -> Self {
        Self {
            looping: true,
            ..Self::default()
        }
    }

    /// Default settings that despawn the entity at the end.
    pub fn despawn_on_finish() -> Self {
        Self {
            on_finish: FinishBehavior::Despawn,
            ..Self::default()
        }
    }
}

/// Why a sound could not play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackError {
    /// The clip failed to load.
    AssetFailed,
    /// No output is open, or it stopped working (see
    /// [`AudioOutput`](crate::AudioOutput)).
    OutputUnavailable,
    /// Every voice ID is in use.
    NoFreeVoice,
    /// The mixer refused the voice: every voice slot is protected.
    Rejected,
}

/// Where a sound is in its life, as last reported by the mixer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    /// Waiting for the clip to load.
    Loading,
    /// Sent to the mixer; not yet confirmed.
    Starting,
    /// The mixer is playing the voice (it may be paused).
    Playing,
    /// The voice ended, was stopped, or was stolen.
    Finished(FinishReason),
    /// The sound could not play, or the output failed while it played.
    Failed(PlaybackError),
}

/// Applied playback state, a read-only copy of the plugin's own record.
///
/// Removing or replacing this component does not affect playback; a new one
/// shows the current state.
#[derive(Component, Clone, Copy, Debug)]
#[component(on_insert = status_inserted)]
pub struct SoundStatus {
    state: PlaybackState,
    voice: Option<VoiceId>,
}

impl SoundStatus {
    pub(crate) const LOADING: Self = Self {
        state: PlaybackState::Loading,
        voice: None,
    };

    /// The current state.
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// The mixer voice, while one is reserved.
    pub fn voice(&self) -> Option<VoiceId> {
        self.voice
    }

    pub(crate) fn shows(&self, record: &Record) -> bool {
        self.state == record.state && self.voice == record.voice
    }

    pub(crate) fn show(&mut self, record: &Record) {
        self.state = record.state;
        self.voice = record.voice;
    }
}

/// A sound ended. Sent once per play, also when the entity is gone.
///
/// Like all Bevy messages it is kept for two updates, so a reader that does
/// not run every frame can miss it; [`SoundStatus`] keeps the final state.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundFinished {
    /// The player entity (it may since have been despawned).
    pub entity: Entity,
    /// Played to the end, stopped, or stolen for a new sound.
    pub reason: FinishReason,
}

/// A sound could not play, or stopped because the output failed.
///
/// Kept for two updates, like [`SoundFinished`].
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundFailed {
    /// The player entity (it may since have been despawned).
    pub entity: Entity,
    /// Why.
    pub error: PlaybackError,
}
