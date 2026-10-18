//! The once-per-frame sync between playback entities and the mixer.
//!
//! Each frame touches only what changed: queued stops, mixer events, new
//! players, players whose [`PlaybackSettings`] changed, and work a full
//! command queue deferred from an earlier frame.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use solouder_core::{Controller, ControllerError, Event, PlayOptions, Sound, VoiceId};

use super::components::{
    FinishBehavior, PlaybackError, PlaybackSettings, PlaybackState, SoundFailed, SoundFinished,
    SoundPlayer, SoundStatus,
};
use super::records::{Playback, Record};
use crate::features::decoded_audio::AudioClip;
use crate::features::output_lifecycle::{AudioOutput, AudioOutputState, SolouderController};

/// Loaded clips and their load states.
#[derive(SystemParam)]
pub(crate) struct Clips<'w> {
    assets: Res<'w, Assets<AudioClip>>,
    server: Res<'w, AssetServer>,
}

/// Writes outcomes to statuses, messages and despawns.
#[derive(SystemParam)]
pub(crate) struct Reports<'w, 's> {
    statuses: Query<'w, 's, &'static mut SoundStatus>,
    settings: Query<'w, 's, &'static PlaybackSettings>,
    finished: MessageWriter<'w, SoundFinished>,
    failed: MessageWriter<'w, SoundFailed>,
    commands: Commands<'w, 's>,
}

impl Reports<'_, '_> {
    fn show(&mut self, entity: Entity, record: &Record) {
        if let Ok(mut status) = self.statuses.get_mut(entity)
            && !status.shows(record)
        {
            status.show(record);
        }
    }

    /// Records the sound's final state and applies its finish behavior.
    fn end(&mut self, entity: Entity, record: &mut Record, state: PlaybackState) {
        record.state = state;
        record.voice = None;
        record.sent = None;
        self.show(entity, record);
        if self
            .settings
            .get(entity)
            .is_ok_and(|s| s.on_finish == FinishBehavior::Despawn)
        {
            self.commands.entity(entity).try_despawn();
        }
    }

    fn fail(&mut self, entity: Entity, record: &mut Record, error: PlaybackError) {
        self.failed.write(SoundFailed { entity, error });
        self.end(entity, record, PlaybackState::Failed(error));
    }
}

/// Stops discarded voices, reads mixer events into records and messages,
/// starts loaded sounds, and sends changed settings. Everything goes out as
/// one batch of commands; a full queue defers the rest to the next frame.
pub(crate) fn sync_playback(
    controller: Option<ResMut<SolouderController>>,
    output: Res<AudioOutput>,
    mut playback: ResMut<Playback>,
    clips: Clips,
    players: Query<(&SoundPlayer, Option<&PlaybackSettings>)>,
    changed: Query<Entity, (With<SoundPlayer>, Changed<PlaybackSettings>)>,
    mut reports: Reports,
) {
    let playback = &mut *playback;
    let mut controller = controller.filter(|_| {
        matches!(
            output.state,
            AudioOutputState::Playing | AudioOutputState::Headless
        )
    });
    let Some(controller) = controller
        .as_mut()
        .map(|c| c.get())
        .filter(|c| c.is_connected())
    else {
        shut_down(playback, &mut reports);
        return;
    };
    playback.dead = false;

    while let Some(&voice) = playback.stops.last() {
        if controller.stop(voice).is_err() {
            break;
        }
        playback.stops.pop();
    }
    read_events(controller, playback, &mut reports);
    for entity in &changed {
        playback.queue(entity);
    }

    let Playback {
        records,
        owners,
        work,
        ..
    } = playback;
    let mut again = Vec::new();
    for entity in std::mem::take(work) {
        let Some(record) = records.get_mut(&entity).filter(|r| r.queued) else {
            continue;
        };
        record.queued = false;
        let Ok((player, settings)) = players.get(entity) else {
            continue;
        };
        let settings = settings.copied().unwrap_or_default();
        let done = match record.state {
            PlaybackState::Loading => match start(controller, player, settings, &clips) {
                Ok(Some((voice, sent))) => {
                    record.state = PlaybackState::Starting;
                    record.voice = Some(voice);
                    record.sent = Some(sent);
                    owners.insert(voice, entity);
                    reports.show(entity, record);
                    true
                }
                Ok(None) => false,
                Err(error) => {
                    reports.fail(entity, record, error);
                    true
                }
            },
            PlaybackState::Starting | PlaybackState::Playing => match (record.voice, record.sent) {
                (Some(voice), Some(sent)) => {
                    let (now, complete) = apply(controller, voice, sent, settings);
                    record.sent = Some(now);
                    complete
                }
                _ => true,
            },
            PlaybackState::Finished(_) | PlaybackState::Failed(_) => true,
        };
        if !done {
            record.queued = true;
            again.push(entity);
        }
    }
    *work = again;
}

fn read_events(controller: &mut Controller, playback: &mut Playback, reports: &mut Reports) {
    while let Some(event) = controller.poll_event() {
        let id = event.id();
        let entity = if event.is_final() {
            playback.owners.remove(&id)
        } else {
            playback.owners.get(&id).copied()
        };
        let Some(entity) = entity else { continue };
        let current = playback
            .records
            .get_mut(&entity)
            .filter(|record| record.voice == Some(id));
        match event {
            Event::Started(_) => {
                if let Some(record) = current {
                    record.state = PlaybackState::Playing;
                    reports.show(entity, record);
                }
            }
            Event::Finished(_, reason) => {
                reports.finished.write(SoundFinished { entity, reason });
                if let Some(record) = current {
                    reports.end(entity, record, PlaybackState::Finished(reason));
                }
            }
            Event::Failed(_) => match current {
                Some(record) => reports.fail(entity, record, PlaybackError::Rejected),
                None => {
                    reports.failed.write(SoundFailed {
                        entity,
                        error: PlaybackError::Rejected,
                    });
                }
            },
        }
    }
}

/// Fails every live sound once the output is gone, and every new one after.
fn shut_down(playback: &mut Playback, reports: &mut Reports) {
    if !playback.dead {
        playback.dead = true;
        playback.owners.clear();
        playback.stops.clear();
        let live: Vec<Entity> = playback
            .records
            .iter()
            .filter(|(_, record)| record.is_live())
            .map(|(&entity, _)| entity)
            .collect();
        for entity in live {
            playback.queue(entity);
        }
    }
    for entity in std::mem::take(&mut playback.work) {
        if let Some(record) = playback.records.get_mut(&entity) {
            record.queued = false;
            if record.is_live() {
                reports.fail(entity, record, PlaybackError::OutputUnavailable);
            }
        }
    }
}

fn valid_speed(speed: f32) -> bool {
    speed > 0.0 && speed.is_finite()
}

/// Sends the play for a loaded clip, with every setting applied from its
/// first frame. `None` means "not yet": still loading, or the command queue
/// is full this frame.
fn start(
    controller: &mut Controller,
    player: &SoundPlayer,
    settings: PlaybackSettings,
    clips: &Clips,
) -> Result<Option<(VoiceId, PlaybackSettings)>, PlaybackError> {
    let Some(clip) = clips.assets.get(&player.0) else {
        if clips.server.load_state(&player.0).is_failed() {
            return Err(PlaybackError::AssetFailed);
        }
        return Ok(None);
    };
    let sent = PlaybackSettings {
        volume: Some(settings.volume)
            .filter(|v| v.is_finite())
            .unwrap_or(1.0),
        pan: Some(settings.pan).filter(|p| p.is_finite()).unwrap_or(0.0),
        speed: Some(settings.speed)
            .filter(|&s| valid_speed(s))
            .unwrap_or(1.0),
        ..settings
    };
    let options = PlayOptions {
        volume: Some(sent.volume),
        pan: sent.pan,
        paused: sent.paused,
        speed: sent.speed,
        protect: sent.protect,
        ..PlayOptions::default()
    };
    let played = if let Some(pcm) = clip.pcm() {
        let mut sound = Sound::new(pcm.clone());
        sound.set_looping(sent.looping);
        controller.play_with(&sound, options)
    } else if let Some(stream) = clip.stream() {
        let mut stream = stream.clone();
        stream.set_looping(sent.looping);
        controller.play_with(&stream, options)
    } else if let Some(synth) = clip.synth() {
        let mut synth = synth.clone();
        synth.set_looping(sent.looping);
        controller.play_with(&synth, options)
    } else {
        return Err(PlaybackError::AssetFailed);
    };
    match played {
        Ok(voice) => Ok(Some((voice, sent))),
        Err(ControllerError::QueueFull | ControllerError::StreamsFull) => Ok(None),
        Err(ControllerError::NoFreeVoiceId) => Err(PlaybackError::NoFreeVoice),
        Err(ControllerError::Disconnected | ControllerError::NoStreamWorker) => {
            Err(PlaybackError::OutputUnavailable)
        }
        Err(_) => Err(PlaybackError::Rejected),
    }
}

/// Moves `now` to `wanted` if the value is usable and the command was
/// accepted. Returns false if it still has to be sent.
fn update<T: Copy + PartialEq>(
    now: &mut T,
    wanted: T,
    usable: bool,
    send: impl FnOnce(T) -> Result<(), ControllerError>,
) -> bool {
    if !usable || *now == wanted {
        return true;
    }
    if send(wanted).is_err() {
        return false;
    }
    *now = wanted;
    true
}

/// Sends the settings that differ from `sent`. Returns what the mixer will
/// have once the accepted commands run, and whether that is everything.
fn apply(
    controller: &mut Controller,
    voice: VoiceId,
    sent: PlaybackSettings,
    wanted: PlaybackSettings,
) -> (PlaybackSettings, bool) {
    let mut now = PlaybackSettings {
        on_finish: wanted.on_finish,
        ..sent
    };
    let mut complete = true;
    complete &= update(
        &mut now.volume,
        wanted.volume,
        wanted.volume.is_finite(),
        |v| controller.set_volume(voice, v),
    );
    complete &= update(&mut now.pan, wanted.pan, wanted.pan.is_finite(), |p| {
        controller.set_pan(voice, p)
    });
    complete &= update(
        &mut now.speed,
        wanted.speed,
        valid_speed(wanted.speed),
        |s| controller.set_relative_play_speed(voice, s),
    );
    complete &= update(&mut now.paused, wanted.paused, true, |p| {
        controller.set_pause(voice, p)
    });
    complete &= update(&mut now.looping, wanted.looping, true, |l| {
        controller.set_looping(voice, l)
    });
    complete &= update(&mut now.protect, wanted.protect, true, |p| {
        controller.set_protect(voice, p)
    });
    (now, complete)
}
