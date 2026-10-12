//! The audio-thread side of a real-time mixer.

use std::sync::Arc;

use rtrb::{Consumer, Producer};

use super::command::{Command, PlayRequest};
use super::ids::LiveTable;
use super::status::{Event, FinishReason, Shared, Status};
use crate::features::playback::{Handle, Mixer, MixerConfig, RetiredItem};

/// Owns the mixer on the audio thread. Each [`Renderer::render`] applies
/// queued commands, mixes, and reports finished voices. Rendering does not
/// allocate, free memory, lock or wait: what voices and filters release is
/// handed back to the controller, which reserved room for it when it
/// accepted the request.
///
/// Drop the renderer on a thread that may free memory, after the audio
/// callback that uses it has stopped.
#[derive(Debug)]
pub struct Renderer {
    pub(super) mixer: Mixer,
    pub(super) commands: Consumer<Command>,
    pub(super) events: Producer<Event>,
    pub(super) retired: Producer<RetiredItem>,
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
        self.report_gone(FinishReason::Ended);
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
        match command {
            Command::Play(request) => self.start(request),
            Command::Stop(id) => {
                if let Some(handle) = self.live.remove(id) {
                    self.mixer.stop(handle);
                    report(&mut self.events, Event::Finished(id, FinishReason::Stopped));
                    self.report_gone(FinishReason::Stopped);
                }
            }
            Command::StopAll => {
                self.mixer.stop_all();
                self.report_gone(FinishReason::Stopped);
            }
            Command::StopSound(sound) => {
                self.mixer.stop_sound_id(sound);
                self.report_gone(FinishReason::Stopped);
            }
            Command::QueueAdd { queue, item } => {
                if self.mixer.queue_push(queue, item).is_err() {
                    self.status.rejected_queue_sounds += 1;
                }
            }
            command => self.apply_control(command),
        }
    }

    fn start(&mut self, request: PlayRequest) {
        let PlayRequest {
            id,
            voice,
            options,
            start,
            bus,
        } = request;
        let handle = self.mixer.start_boxed(voice, options, start, bus);
        let stole = self.mixer.take_stolen();
        // The slot's previous voice was stolen, or stopped by this play.
        if let Some(old) = handle.slot().and_then(|slot| self.live.remove_slot(slot)) {
            let reason = if stole {
                FinishReason::Stolen
            } else {
                FinishReason::Stopped
            };
            report(&mut self.events, Event::Finished(old, reason));
        }
        // Other voices the play stopped: instances of a single-instance
        // sound, bus or queue, and the voices of such a bus.
        self.report_gone(FinishReason::Stopped);
        if handle == Handle::INVALID {
            report(&mut self.events, Event::Failed(id));
            return;
        }
        self.live.insert(id, handle);
        report(&mut self.events, Event::Started(id));
    }

    /// Reports every voice in the table that is no longer playing.
    fn report_gone(&mut self, reason: FinishReason) {
        let Self {
            mixer,
            live,
            events,
            ..
        } = self;
        live.retain(
            |handle| !mixer.is_valid_voice_handle(handle),
            |id| report(events, Event::Finished(id, reason)),
        );
    }

    /// Moves retired items to the controller, as many as the queue holds.
    fn release_retired(&mut self) {
        let room = self.retired.slots();
        let retired = &mut self.retired;
        self.mixer.drain_retired(room, |item| {
            // Cannot fail: at most `room` items are drained.
            let _ = retired.push(item);
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
pub(super) fn report(events: &mut Producer<Event>, event: Event) {
    let pushed = events.push(event);
    debug_assert!(pushed.is_ok(), "event queue sized for every voice ID");
}
