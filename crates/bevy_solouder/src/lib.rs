//! Bevy plugin for SoLouder.
//!
//! Add [`SolouderPlugin`] (after Bevy's asset plugin), load `.wav`, `.flac`,
//! `.ogg` (Vorbis) and `.mp3` files as [`AudioClip`]s (decoded at load, or streamed with
//! [`WavSettings::stream`]),
//! sfxr parameter files (`.sfx`, `.sfs`), Monotone songs (`.mon`) and chip
//! register dumps (`.dump`) as generated clips (with the default `sources`
//! feature), or wrap any generator with [`AudioClip::from_generator`],
//! and spawn entities with a [`SoundPlayer`]. A sound plays
//! while its player exists: despawning the entity, removing the component or
//! inserting a new one stops it. [`PlaybackSettings`] holds the requested
//! volume, pan, speed, pause, looping and protection; [`SoundStatus`] shows
//! what the mixer has applied, and [`SoundFinished`] / [`SoundFailed`]
//! messages report endings.
//!
//! A sound whose entity also has a [`SpatialEmitter`] plays in 3D at the
//! entity's `GlobalTransform`, heard from the entity with the
//! [`AudioListener`]; see [`SpatialAudioSettings`], [`AudioVelocity`],
//! [`AudioTeleport`] and [`ListenerState`] for the speed of sound,
//! velocities, teleports and the listener policy. Bevy's axes (right-handed,
//! +Y up, -Z forward) are SoLoud's defaults; units are metres.
//!
//! Set [`AudioVisualization::enabled`] to have the resource carry the mix's
//! wave, spectrum and channel volumes each frame.
//!
//! The mixer runs on the audio device's thread. Systems never wait for it:
//! two systems in [`SolouderSystems::Sync`] (in `PostUpdate`, after transform
//! propagation) run each frame, one after the other: the first takes a
//! snapshot of the listener and emitters, the second sends the frame's
//! changes as one batch of commands and reads back the mixer's events. A
//! third copies visualization data, while it is recorded.
//!
//! Disable Bevy's own audio plugin when using this one, or both will open the
//! device.

mod features;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use solouder_decoders::DecodeLimits;

pub use features::decoded_audio::{AudioClip, AudioLoadError, WavLoader, WavSettings};
#[cfg(feature = "sources")]
pub use features::generated_audio::{
    ChipDumpLoader, GeneratorLoadError, MonotoneLoader, MonotoneSettings, SfxrLoader,
};
pub use features::output_lifecycle::{
    AudioOutput, AudioOutputState, HeadlessRenderer, HeadlessStreamWorker, OutputMode,
    SolouderController,
};
pub use features::playback::{
    FinishBehavior, PlaybackError, PlaybackSettings, PlaybackState, SoundFailed, SoundFinished,
    SoundPlayer, SoundStatus,
};
pub use features::spatial::{
    AudioListener, AudioTeleport, AudioVelocity, ListenerState, SpatialAudioSettings,
    SpatialEmitter,
};
pub use features::visualization::AudioVisualization;
pub use solouder_core::{Attenuation, FinishReason, MixerConfig, VoiceId};
pub use solouder_cpal::{OutputConfig, OutputInfo, StreamPreferences};

/// Adds SoLouder audio: the clip asset and loaders, the output, and playback.
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
        #[cfg(feature = "sources")]
        features::generated_audio::build(app);
        features::output_lifecycle::build(app, &self.output);
        features::playback::build(app);
        features::spatial::build(app);
        features::visualization::build(app, &self.output);
    }
}
