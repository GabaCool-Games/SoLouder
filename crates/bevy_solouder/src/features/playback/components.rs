//! Playback components, messages and the hook that stops a voice when its
//! player goes away.

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use solouder_core::{FinishReason, VoiceId};

use crate::features::decoded_audio::AudioClip;

/// Plays a clip for as long as this component stays on the entity.
///
/// Removing the component, replacing it, or despawning the entity stops the
/// sound, including one still waiting for its asset to load.
#[derive(Component, Clone, Debug)]
#[component(on_discard = stop_voice)]
#[require(PlaybackSettings, SoundStatus)]
pub struct SoundPlayer(pub Handle<AudioClip>);

/// What to do with the entity once its sound has played to the end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FinishBehavior {
    /// Keep the entity; its status says the sound finished.
    #[default]
    Keep,
    /// Despawn the entity.
    Despawn,
}

/// Requested playback settings. Edits are sent to the mixer in the next
/// sync; the mixer applies them with its usual volume and pan ramps.
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
    /// No output is open (see [`AudioOutput`](crate::AudioOutput)).
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
    /// The voice ended or was stopped.
    Finished(FinishReason),
    /// The sound could not play.
    Failed(PlaybackError),
}

/// Applied playback state, maintained by the plugin.
#[derive(Component, Clone, Copy, Debug)]
pub struct SoundStatus {
    pub(crate) state: PlaybackState,
    pub(crate) voice: Option<VoiceId>,
    /// Settings last sent to the mixer; `None` until the play is sent.
    pub(crate) sent: Option<PlaybackSettings>,
}

impl Default for SoundStatus {
    fn default() -> Self {
        Self {
            state: PlaybackState::Loading,
            voice: None,
            sent: None,
        }
    }
}

impl SoundStatus {
    /// The current state.
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// The mixer voice, while one is reserved.
    pub fn voice(&self) -> Option<VoiceId> {
        self.voice
    }
}

/// A sound ended. Sent once per play, also when the entity is gone.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundFinished {
    /// The player entity (it may since have been despawned).
    pub entity: Entity,
    /// Played to the end, or stopped.
    pub reason: FinishReason,
}

/// A sound could not play.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundFailed {
    /// The player entity.
    pub entity: Entity,
    /// Why.
    pub error: PlaybackError,
}

/// Voices to stop at the next sync, queued by the player hook, and the
/// entity each live voice belongs to.
#[derive(Resource, Default, Debug)]
pub(crate) struct Voices {
    pub stops: Vec<VoiceId>,
    pub owners: HashMap<VoiceId, Entity>,
}

fn stop_voice(mut world: DeferredWorld, context: HookContext) {
    let Some(mut status) = world.get_mut::<SoundStatus>(context.entity) else {
        return;
    };
    let voice = status.voice.take();
    *status = SoundStatus::default();
    if let Some(voice) = voice
        && let Some(mut voices) = world.get_resource_mut::<Voices>()
    {
        voices.stops.push(voice);
    }
}
