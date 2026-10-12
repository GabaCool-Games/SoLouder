//! The game-thread side of a real-time mixer.

use std::fmt;
use std::sync::Arc;

use rtrb::{Consumer, Producer};

use super::VoiceId;
use super::command::Command;
use super::ids::IdPool;
use super::status::{Event, Shared, Status};
use crate::contracts::Pcm;
use crate::dsp::Resampler;
use crate::features::playback::{PlayOptions, Sound};

/// Why the controller refused a request. Nothing was queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControllerError {
    /// The command queue is full; the audio thread has not caught up.
    QueueFull,
    /// Every voice ID is in use. IDs come back when their final event is
    /// read with [`Controller::poll_event`].
    NoFreeVoiceId,
    /// Play speeds must be greater than 0.
    InvalidSpeed,
}

impl fmt::Display for ControllerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => write!(f, "the audio command queue is full"),
            Self::NoFreeVoiceId => write!(f, "every voice ID is in use"),
            Self::InvalidSpeed => write!(f, "play speed must be greater than 0"),
        }
    }
}

impl std::error::Error for ControllerError {}

/// Sends commands to a [`Renderer`](super::Renderer) and reads what it
/// reports. No method waits for the audio thread.
///
/// Requests take effect at the start of a later render, in the order they
/// were made. Call [`Controller::poll_event`] regularly: it frees the IDs of
/// finished voices, and the audio of finished voices is released through it
/// on this thread rather than on the audio thread.
#[derive(Debug)]
pub struct Controller {
    pub(super) commands: Producer<Command>,
    pub(super) events: Consumer<Event>,
    pub(super) retired: Consumer<Pcm>,
    pub(super) ids: IdPool,
    pub(super) shared: Arc<Shared>,
}

impl Controller {
    fn send(&mut self, command: Command) -> Result<(), ControllerError> {
        self.commands
            .push(command)
            .map_err(|_| ControllerError::QueueFull)
    }

    /// Plays `sound` with default options.
    pub fn play(&mut self, sound: &Sound) -> Result<VoiceId, ControllerError> {
        self.play_with(sound, PlayOptions::default())
    }

    /// Queues a play request and returns the ID of its voice-to-be. The
    /// voice reports [`Event::Started`] or [`Event::Failed`], and later
    /// [`Event::Finished`].
    pub fn play_with(
        &mut self,
        sound: &Sound,
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        if self.commands.slots() == 0 {
            return Err(ControllerError::QueueFull);
        }
        let id = self.ids.allocate().ok_or(ControllerError::NoFreeVoiceId)?;
        let sound = sound.clone();
        self.send(Command::Play { id, sound, options })?;
        Ok(id)
    }

    /// Stops the voice; it reports [`Event::Finished`] with
    /// [`FinishReason::Stopped`](super::FinishReason::Stopped).
    pub fn stop(&mut self, id: VoiceId) -> Result<(), ControllerError> {
        self.send(Command::Stop(id))
    }

    /// Stops every voice.
    pub fn stop_all(&mut self) -> Result<(), ControllerError> {
        self.send(Command::StopAll)
    }

    /// Sets the voice's volume (ramped over the next render).
    pub fn set_volume(&mut self, id: VoiceId, volume: f32) -> Result<(), ControllerError> {
        self.send(Command::Volume(id, volume))
    }

    /// Sets the voice's stereo pan, -1 (left) to 1 (right).
    pub fn set_pan(&mut self, id: VoiceId, pan: f32) -> Result<(), ControllerError> {
        self.send(Command::Pan(id, pan))
    }

    /// Pauses or resumes the voice.
    pub fn set_pause(&mut self, id: VoiceId, paused: bool) -> Result<(), ControllerError> {
        self.send(Command::Pause(id, paused))
    }

    /// Turns looping on or off for the voice.
    pub fn set_looping(&mut self, id: VoiceId, looping: bool) -> Result<(), ControllerError> {
        self.send(Command::Looping(id, looping))
    }

    /// Protects the voice from being stolen or made inactive.
    pub fn set_protect(&mut self, id: VoiceId, protect: bool) -> Result<(), ControllerError> {
        self.send(Command::Protect(id, protect))
    }

    /// Sets the voice's relative play speed, which must be greater than 0.
    pub fn set_relative_play_speed(
        &mut self,
        id: VoiceId,
        speed: f32,
    ) -> Result<(), ControllerError> {
        if speed.is_nan() || speed <= 0.0 {
            return Err(ControllerError::InvalidSpeed);
        }
        self.send(Command::Speed(id, speed))
    }

    /// Sets the volume applied to the whole mix.
    pub fn set_global_volume(&mut self, volume: f32) -> Result<(), ControllerError> {
        self.send(Command::GlobalVolume(volume))
    }

    /// Sets the scale applied after clipping.
    pub fn set_post_clip_scaler(&mut self, scale: f32) -> Result<(), ControllerError> {
        self.send(Command::PostClipScaler(scale))
    }

    /// Selects the resampler for every voice.
    pub fn set_resampler(&mut self, resampler: Resampler) -> Result<(), ControllerError> {
        self.send(Command::Resampler(resampler))
    }

    /// The next voice event, if any. A final event frees its voice ID.
    /// Also releases, on this thread, audio the renderer no longer uses.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.release_retired_audio();
        let event = self.events.pop().ok()?;
        if event.is_final() {
            self.ids.release(event.id());
        }
        Some(event)
    }

    /// Drops audio buffers the renderer has finished with. Returns how many.
    pub fn release_retired_audio(&mut self) -> usize {
        let mut released = 0;
        while self.retired.pop().is_ok() {
            released += 1;
        }
        released
    }

    /// The counters from the most recent render.
    pub fn status(&self) -> Status {
        self.shared.read()
    }

    /// Voice IDs reserved and not yet freed by a final event.
    pub fn voice_ids_in_use(&self) -> usize {
        self.ids.in_use()
    }

    /// Room left in the command queue.
    pub fn queue_space(&self) -> usize {
        self.commands.slots()
    }
}
