//! The game-thread side of a real-time mixer.

use std::fmt;
use std::sync::Arc;

use rtrb::{Consumer, Producer};

use super::VoiceId;
use super::bus_layouts::BusLayouts;
use super::command::{Command, PlayRequest};
use super::ids::IdPool;
use super::status::{Event, Shared, Status};
use crate::features::playback::{
    Bus, Handle, MixerConfig, PlayOptions, Playable, QUEUE_MAX, RetiredItem, SoundId, Start,
};

/// Why the controller refused a request. Nothing was queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControllerError {
    /// The command queue is full; the audio thread has not caught up. Also
    /// returned while the audio thread holds as much released memory as it
    /// has room for, because [`Controller::poll_event`] has not collected it.
    QueueFull,
    /// Every voice ID is in use. IDs come back when their final event is
    /// read with [`Controller::poll_event`].
    NoFreeVoiceId,
    /// Play speeds must be greater than 0.
    InvalidSpeed,
    /// More distinct queues were played than
    /// [`RealtimeConfig::queues`](super::RealtimeConfig::queues) allows.
    TooManyQueues,
    /// The renderer has been dropped: nothing will apply this request, and
    /// voices already queued or playing will not report again.
    Disconnected,
}

impl fmt::Display for ControllerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => write!(f, "the audio command queue is full"),
            Self::NoFreeVoiceId => write!(f, "every voice ID is in use"),
            Self::InvalidSpeed => write!(f, "play speed must be greater than 0"),
            Self::TooManyQueues => write!(f, "too many queues"),
            Self::Disconnected => write!(f, "the audio renderer is gone"),
        }
    }
}

impl std::error::Error for ControllerError {}

/// Sends commands to a [`Renderer`](super::Renderer) and reads what it
/// reports. No method waits for the audio thread.
///
/// Requests take effect at the start of a later render, in the order they
/// were made. Call [`Controller::poll_event`] regularly: it frees the IDs of
/// finished voices, and the memory the renderer no longer uses (ended
/// voices, replaced filters, played queued sounds) is freed through it on
/// this thread rather than on the audio thread.
///
/// Voices, filter instances and queue storage are built here, so a request
/// may allocate; the renderer never does.
#[derive(Debug)]
pub struct Controller {
    pub(super) commands: Producer<Command>,
    pub(super) events: Consumer<Event>,
    pub(super) retired: Consumer<RetiredItem>,
    pub(super) ids: IdPool,
    /// Retired items the renderer may still send back without running out
    /// of reserved storage. A play, a filter swap and a queued sound each
    /// spend one.
    pub(super) retire_credits: usize,
    /// Queues the renderer has storage for.
    pub(super) queues: Vec<SoundId>,
    pub(super) max_queues: usize,
    /// The mixer's output layout, which global filters run at.
    pub(super) config: MixerConfig,
    /// The layout of each bus voice started here, which bus filters run at.
    pub(super) buses: BusLayouts,
    pub(super) shared: Arc<Shared>,
}

impl Controller {
    pub(super) fn send(&mut self, command: Command) -> Result<(), ControllerError> {
        if self.commands.is_abandoned() {
            return Err(ControllerError::Disconnected);
        }
        self.commands
            .push(command)
            .map_err(|_| ControllerError::QueueFull)
    }

    /// Sends a command that makes the renderer retire one item.
    pub(super) fn send_retiring(&mut self, command: Command) -> Result<(), ControllerError> {
        if self.retire_credits == 0 {
            return Err(ControllerError::QueueFull);
        }
        self.send(command)?;
        self.retire_credits -= 1;
        Ok(())
    }

    /// Plays `source` with default options.
    pub fn play(&mut self, source: &impl Playable) -> Result<VoiceId, ControllerError> {
        self.play_with(source, PlayOptions::default())
    }

    /// Queues a play request and returns the ID of its voice-to-be. The
    /// voice reports [`Event::Started`] or [`Event::Failed`], and later
    /// [`Event::Finished`]. Speed and protection in `options` apply from the
    /// first mixed frame. `options.bus` is ignored: play through a bus with
    /// [`Controller::bus_play`].
    ///
    /// Fails with [`ControllerError::QueueFull`] when the command queue is
    /// full, and also when the audio thread holds as much released memory as
    /// it has room for because [`Controller::poll_event`] has not collected
    /// it.
    pub fn play_with(
        &mut self,
        source: &impl Playable,
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        self.start(source, options, Start::Now, None)
    }

    /// [`Mixer::play_clocked`](crate::Mixer::play_clocked): starts the voice
    /// delayed by the time `sound_time` lies after the previous clocked
    /// play's, so sounds triggered from a game loop keep their spacing.
    pub fn play_clocked(
        &mut self,
        sound_time: f64,
        source: &impl Playable,
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        self.start(source, options, Start::Clocked(sound_time), None)
    }

    /// [`Mixer::play_background`](crate::Mixer::play_background): full gain
    /// on the left and right speakers, for music and ambience.
    pub fn play_background(
        &mut self,
        source: &impl Playable,
        volume: Option<f32>,
        paused: bool,
    ) -> Result<VoiceId, ControllerError> {
        let options = PlayOptions {
            volume,
            paused,
            ..PlayOptions::default()
        };
        self.start(source, options, Start::Background, None)
    }

    /// Plays `source` through `bus`'s voice (`Bus::play`). The play fails
    /// with [`Event::Failed`] if the bus is not playing when it is applied.
    pub fn bus_play(
        &mut self,
        bus: &Bus,
        source: &impl Playable,
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        self.start(source, options, Start::Now, Some(bus.id()))
    }

    /// [`Controller::play_clocked`] through `bus` (`Bus::playClocked`).
    pub fn bus_play_clocked(
        &mut self,
        bus: &Bus,
        sound_time: f64,
        source: &impl Playable,
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        let start = Start::Clocked(sound_time);
        self.start(source, options, start, Some(bus.id()))
    }

    fn start(
        &mut self,
        source: &impl Playable,
        options: PlayOptions,
        start: Start,
        bus: Option<SoundId>,
    ) -> Result<VoiceId, ControllerError> {
        if options.speed.is_nan() || options.speed <= 0.0 {
            return Err(ControllerError::InvalidSpeed);
        }
        if self.commands.is_abandoned() {
            return Err(ControllerError::Disconnected);
        }
        let queue = source.queue_id().filter(|id| !self.queues.contains(id));
        let needed = 1 + usize::from(queue.is_some());
        if self.commands.slots() < needed || self.retire_credits == 0 {
            return Err(ControllerError::QueueFull);
        }
        if queue.is_some() && self.queues.len() >= self.max_queues {
            return Err(ControllerError::TooManyQueues);
        }
        let id = self.ids.allocate().ok_or(ControllerError::NoFreeVoiceId)?;
        if let Some(queue) = queue {
            let storage = std::collections::VecDeque::with_capacity(QUEUE_MAX);
            // Cannot fail: there is room for both commands.
            let _ = self.send(Command::QueueStorage(queue, storage));
            self.queues.push(queue);
        }
        let mut prepared = source.prepare();
        prepared.prepare_filters(options.speed);
        let layout = (prepared.voice.channels(), prepared.voice.base_sample_rate);
        let request = PlayRequest {
            id,
            voice: prepared.into(),
            options: PlayOptions {
                bus: Handle::INVALID,
                ..options
            },
            start,
            bus,
        };
        if let Err(error) = self.send(Command::Play(request)) {
            self.ids.release(id);
            return Err(error);
        }
        self.retire_credits -= 1;
        if let Some(bus) = source.bus_id() {
            self.buses
                .started(bus, id, layout.0, layout.1, options.speed);
        }
        Ok(id)
    }

    /// Whether the renderer still exists. Once it is dropped every request
    /// fails with [`ControllerError::Disconnected`].
    pub fn is_connected(&self) -> bool {
        !self.commands.is_abandoned()
    }

    /// The next voice event, if any. A final event frees its voice ID.
    /// Also frees, on this thread, memory the renderer no longer uses.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.release_retired();
        let event = self.events.pop().ok()?;
        if event.is_final() {
            self.ids.release(event.id());
            self.buses.finished(event.id());
        }
        Some(event)
    }

    /// Frees the memory the renderer has finished with: ended voices,
    /// replaced filter instances and played queued sounds. Returns how many
    /// items were freed.
    pub fn release_retired(&mut self) -> usize {
        let mut released = 0;
        while self.retired.pop().is_ok() {
            released += 1;
        }
        self.retire_credits += released;
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
