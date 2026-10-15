//! Bevy plugin for SoLouder.
//!
//! Add [`SolouderPlugin`] (after Bevy's asset plugin), load `.wav`, `.flac`,
//! `.ogg` (Vorbis) and `.mp3` files as [`AudioClip`]s (decoded at load, or streamed with
//! [`WavSettings::stream`]),
//! and spawn entities with a [`SoundPlayer`]. A sound plays
//! while its player exists: despawning the entity, removing the component or
//! inserting a new one stops it. [`PlaybackSettings`] holds the requested
//! volume, pan, speed, pause, looping and protection; [`SoundStatus`] shows
//! what the mixer has applied, and [`SoundFinished`] / [`SoundFailed`]
//! messages report endings.
//!
//! The mixer runs on the audio device's thread. Systems never wait for it:
//! one system in [`SolouderSystems::Sync`] (in `PostUpdate`, after transform
//! propagation) sends each frame's changes as one batch of commands and
//! reads back the mixer's events.
//!
//! Disable Bevy's own audio plugin when using this one, or both will open the
//! device.

mod features;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use solouder_decoders::DecodeLimits;

pub use features::decoded_audio::{AudioClip, AudioLoadError, WavLoader, WavSettings};
pub use features::output_lifecycle::{
    AudioOutput, AudioOutputState, HeadlessRenderer, HeadlessStreamWorker, OutputMode,
    SolouderController,
};
pub use features::playback::{
    FinishBehavior, PlaybackError, PlaybackSettings, PlaybackState, SoundFailed, SoundFinished,
    SoundPlayer, SoundStatus,
};
pub use solouder_core::{FinishReason, MixerConfig, VoiceId};
pub use solouder_cpal::{OutputConfig, OutputInfo, StreamPreferences};

/// Adds SoLouder audio: the clip asset and loader, the output, and playback.
#[derive(Clone, Debug, Default)]
pub struct SolouderPlugin {
    /// Device or headless output.
    pub output: OutputMode,
    /// Limits for decoding audio files.
    pub decode_limits: DecodeLimits,
}

/// The plugin's system sets.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SolouderSystems {
    /// Sends playback changes and reads mixer events, in `PostUpdate`.
    Sync,
}

impl Plugin for SolouderPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            SolouderSystems::Sync.after(TransformSystems::Propagate),
        );
        features::decoded_audio::build(app, self.decode_limits);
        features::output_lifecycle::build(app, &self.output);
        features::playback::build(app);
    }
}
