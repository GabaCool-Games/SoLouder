//! The audio-thread side of a real-time mixer.

use std::sync::Arc;

use rtrb::{Consumer, Producer};

use super::command::Command;
use super::ids::LiveTable;
use super::status::{Event, FinishReason, Shared, Status};
use crate::contracts::Pcm;
use crate::features::playback::{Handle, Mixer, MixerConfig};

/// Owns the mixer on the audio thread. Each [`Renderer::render`] applies
/// queued commands, mixes, and reports finished voices. Rendering does not
/// allocate, free memory, lock or wait: audio that voices release is handed
/// back to the controller, which reserved room for it when it accepted the
/// play.
///
/// Drop the renderer on a thread that may free memory, after the audio
/// callback that uses it has stopped.
#[derive(Debug)]
pub struct Renderer {
    pub(super) mixer: Mixer,
    pub(super) commands: Consumer<Command>,
    pub(super) events: Producer<Event>,
    pub(super) retired: Producer<Pcm>,
    pub(super) live: LiveTable,
    pub(super) shared: Arc<Shared>,
    pub(super) commands_per_render: usize,
    pub(super) status: Status,
}

impl Renderer {
    /// The mixer's configuration: output channels and sample rate.
    pub fn config(&self) -> MixerConfig {
        self.mixer.config()
    }

    /// Renders `output.len() / channels` interleaved frames, as
    /// [`Mixer::mix_interleaved`] does, after applying up to the configured
    /// number of queued commands.
    pub fn render(&mut self, output: &mut [f32]) {
        self.apply_commands();
        self.mixer.mix_interleaved(output);
        let Self {
            mixer,
            live,
            events,
            ..
        } = self;
        live.retain(
            |handle| !mixer.is_valid_voice_handle(handle),
            |id| report(events, Event::Finished(id, FinishReason::Ended)),
        );
        self.release_retired();
        self.publish();
    }

    fn apply_commands(&mut self) {
        for _ in 0..self.commands_per_render {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            self.apply(command);
        }
        self.status.deferred_commands = self.commands.slots() as u64;
    }

    fn apply(&mut self, command: Command) {
        let mixer = &mut self.mixer;
        let live = &self.live;
        let handle = |id| live.handle(id).unwrap_or(Handle::INVALID);
        match command {
            Command::Play { id, sound, options } => {
                let mixer = &mut self.mixer;
                let handle = mixer.play_with(&sound, options);
                let stole = mixer.take_stolen();
                if handle == Handle::INVALID {
                    report(&mut self.events, Event::Failed(id));
                } else {
                    // The slot's previous voice is gone: stolen just now, or
                    // (defensively) ended without being noticed yet.
                    let slot = handle.slot();
                    if let Some(old) = slot.and_then(|slot| self.live.remove_slot(slot)) {
                        let reason = if stole {
                            FinishReason::Stolen
                        } else {
                            FinishReason::Ended
                        };
                        report(&mut self.events, Event::Finished(old, reason));
                    }
                    self.live.insert(id, handle);
                    report(&mut self.events, Event::Started(id));
                }
                mixer.retire_audio(sound.into_pcm());
            }
            Command::Stop(id) => {
                if let Some(handle) = self.live.remove(id) {
                    self.mixer.stop(handle);
                    report(&mut self.events, Event::Finished(id, FinishReason::Stopped));
                }
            }
            Command::StopAll => {
                self.mixer.stop_all();
                let events = &mut self.events;
                self.live
                    .clear(|id| report(events, Event::Finished(id, FinishReason::Stopped)));
            }
            Command::Volume(id, volume) => mixer.set_volume(handle(id), volume),
            Command::Pan(id, pan) => mixer.set_pan(handle(id), pan),
            Command::Pause(id, paused) => mixer.set_pause(handle(id), paused),
            Command::Looping(id, looping) => mixer.set_looping(handle(id), looping),
            Command::Protect(id, protect) => mixer.set_protect_voice(handle(id), protect),
            Command::Speed(id, speed) => {
                let _ = mixer.set_relative_play_speed(handle(id), speed);
            }
            Command::GlobalVolume(volume) => mixer.set_global_volume(volume),
            Command::PostClipScaler(scale) => mixer.set_post_clip_scaler(scale),
            Command::Resampler(resampler) => mixer.set_resampler(resampler),
        }
    }

    /// Moves retired audio to the controller, as much as the queue holds.
    fn release_retired(&mut self) {
        let room = self.retired.slots();
        let retired = &mut self.retired;
        self.mixer.drain_retired_audio(room, |pcm| {
            // Cannot fail: at most `room` buffers are drained.
            let _ = retired.push(pcm);
        });
    }

    fn publish(&mut self) {
        self.status.stream_time = self.mixer.stream_time();
        self.status.voices = self.mixer.voice_count() as u32;
        self.status.active_voices = self.mixer.active_voice_count() as u32;
        self.status.renders += 1;
        self.status.retired_overflow = self.mixer.retired_overflow();
        self.shared.publish(&self.status);
    }
}

/// The event queue holds two events per voice ID and an ID is reused only
/// after its final event is read, so pushing cannot fail.
fn report(events: &mut Producer<Event>, event: Event) {
    let pushed = events.push(event);
    debug_assert!(pushed.is_ok(), "event queue sized for every voice ID");
}
