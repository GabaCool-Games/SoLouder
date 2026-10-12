//! The once-per-frame sync between playback entities and the mixer.

use bevy::prelude::*;
use solouder_core::{
    Controller, ControllerError, Event, FinishReason, PlayOptions, Sound, VoiceId,
};

use super::components::{
    FinishBehavior, PlaybackError, PlaybackSettings, PlaybackState, SoundFailed, SoundFinished,
    SoundPlayer, SoundStatus, Voices,
};
use crate::features::decoded_audio::AudioClip;
use crate::features::output_lifecycle::SolouderController;

type Players<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static SoundPlayer,
        &'static PlaybackSettings,
        &'static mut SoundStatus,
    ),
>;

/// Stops discarded voices, reads mixer events into statuses and messages,
/// starts loaded sounds, and sends changed settings. Everything goes out as
/// one batch of commands; a full queue defers the rest to the next frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_playback(
    controller: Option<ResMut<SolouderController>>,
    mut voices: ResMut<Voices>,
    clips: Res<Assets<AudioClip>>,
    server: Res<AssetServer>,
    mut players: Players,
    mut finished: MessageWriter<SoundFinished>,
    mut failed: MessageWriter<SoundFailed>,
    mut commands: Commands,
) {
    let Some(mut controller) = controller else {
        for (entity, _, _, mut status) in &mut players {
            if status.state == PlaybackState::Loading {
                status.state = PlaybackState::Failed(PlaybackError::OutputUnavailable);
                failed.write(SoundFailed {
                    entity,
                    error: PlaybackError::OutputUnavailable,
                });
            }
        }
        return;
    };
    let controller = controller.get();

    while let Some(&voice) = voices.stops.last() {
        if controller.stop(voice).is_err() {
            break;
        }
        voices.stops.pop();
    }

    while let Some(event) = controller.poll_event() {
        let id = event.id();
        let entity = if event.is_final() {
            voices.owners.remove(&id)
        } else {
            voices.owners.get(&id).copied()
        };
        let Some(entity) = entity else { continue };
        let current = players
            .get_mut(entity)
            .ok()
            .filter(|(.., status)| status.voice == Some(id));
        match event {
            Event::Started(_) => {
                if let Some((.., mut status)) = current {
                    status.state = PlaybackState::Playing;
                }
            }
            Event::Finished(_, reason) => {
                finished.write(SoundFinished { entity, reason });
                if let Some((_, _, settings, mut status)) = current {
                    status.state = PlaybackState::Finished(reason);
                    status.voice = None;
                    if settings.on_finish == FinishBehavior::Despawn
                        && reason == FinishReason::Ended
                    {
                        commands.entity(entity).try_despawn();
                    }
                }
            }
            Event::Failed(_) => {
                failed.write(SoundFailed {
                    entity,
                    error: PlaybackError::Rejected,
                });
                if let Some((.., mut status)) = current {
                    status.state = PlaybackState::Failed(PlaybackError::Rejected);
                    status.voice = None;
                }
            }
        }
    }

    for (entity, player, settings, mut status) in &mut players {
        match status.state {
            PlaybackState::Loading => {
                if let Err(error) = start(
                    controller,
                    &mut voices,
                    entity,
                    player,
                    settings,
                    &clips,
                    &server,
                    &mut status,
                ) {
                    status.state = PlaybackState::Failed(error);
                    failed.write(SoundFailed { entity, error });
                }
            }
            PlaybackState::Starting | PlaybackState::Playing => {
                if let (Some(voice), Some(sent)) = (status.voice, status.sent)
                    && sent != *settings
                {
                    status.sent = Some(apply(controller, voice, sent, *settings));
                }
            }
            PlaybackState::Finished(_) | PlaybackState::Failed(_) => {}
        }
    }
}

/// Sends the play for a loaded clip. `Ok` also covers "not yet": still
/// loading, or the command queue is full this frame.
#[allow(clippy::too_many_arguments)]
fn start(
    controller: &mut Controller,
    voices: &mut Voices,
    entity: Entity,
    player: &SoundPlayer,
    settings: &PlaybackSettings,
    clips: &Assets<AudioClip>,
    server: &AssetServer,
    status: &mut SoundStatus,
) -> Result<(), PlaybackError> {
    let Some(clip) = clips.get(&player.0) else {
        if server.load_state(&player.0).is_failed() {
            return Err(PlaybackError::AssetFailed);
        }
        return Ok(());
    };
    let mut sound = Sound::new(clip.pcm().clone());
    sound.set_looping(settings.looping);
    let options = PlayOptions {
        volume: Some(settings.volume),
        pan: settings.pan,
        paused: settings.paused,
    };
    let voice = match controller.play_with(&sound, options) {
        Ok(voice) => voice,
        Err(ControllerError::QueueFull) => return Ok(()),
        Err(_) => return Err(PlaybackError::NoFreeVoice),
    };
    voices.owners.insert(voice, entity);
    status.voice = Some(voice);
    status.state = PlaybackState::Starting;
    let played = PlaybackSettings {
        speed: 1.0,
        protect: false,
        ..*settings
    };
    status.sent = Some(apply(controller, voice, played, *settings));
    Ok(())
}

/// Sends the settings that differ from `sent` and returns what the mixer
/// will have once the accepted commands run.
fn apply(
    controller: &mut Controller,
    voice: VoiceId,
    sent: PlaybackSettings,
    wanted: PlaybackSettings,
) -> PlaybackSettings {
    let mut now = PlaybackSettings {
        on_finish: wanted.on_finish,
        ..sent
    };
    if now.volume != wanted.volume && controller.set_volume(voice, wanted.volume).is_ok() {
        now.volume = wanted.volume;
    }
    if now.pan != wanted.pan && controller.set_pan(voice, wanted.pan).is_ok() {
        now.pan = wanted.pan;
    }
    if now.speed != wanted.speed {
        match controller.set_relative_play_speed(voice, wanted.speed) {
            // An invalid speed is never sent; remember it so it is not retried.
            Ok(()) | Err(ControllerError::InvalidSpeed) => now.speed = wanted.speed,
            Err(_) => {}
        }
    }
    if now.paused != wanted.paused && controller.set_pause(voice, wanted.paused).is_ok() {
        now.paused = wanted.paused;
    }
    if now.looping != wanted.looping && controller.set_looping(voice, wanted.looping).is_ok() {
        now.looping = wanted.looping;
    }
    if now.protect != wanted.protect && controller.set_protect(voice, wanted.protect).is_ok() {
        now.protect = wanted.protect;
    }
    now
}
