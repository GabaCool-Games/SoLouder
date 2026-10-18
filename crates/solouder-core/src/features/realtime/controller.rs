//! The game-thread side of a real-time mixer.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::{Consumer, Producer};

use super::VoiceId;
use super::bus_layouts::BusLayouts;
use super::command::{Command, PlayRequest};
use super::controller_visual::watch_commands;
use super::ids::IdPool;
use super::spatial_mirror::{FilterLayout, SpatialMirror};
use super::status::{Event, Shared, Status};
use super::stream_worker::StreamWorker;
use super::visual::VisualReader;
use crate::features::playback::{
    Bus, Handle, MixerConfig, PlayOptions, Playable, QUEUE_MAX, RetiredItem, SoundId, Start,
    WorkerFeed, flags,
};
use crate::features::spatial::{Placement, distance_delay};

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
    /// More streams were played than the [`StreamWorker`] has taken
    /// ([`RealtimeConfig::pending_streams`](super::RealtimeConfig::pending_streams)):
    /// it is not running, or has fallen behind.
    StreamsFull,
    /// The [`StreamWorker`] was dropped: streams cannot play.
    NoStreamWorker,
    /// The speed of sound must be finite and greater than 0.
    InvalidSoundSpeed,
    /// The output has no such channel, so no speaker for it.
    NoSuchSpeaker,
    /// More buses record visualization data at once than
    /// [`RealtimeConfig::visualized_buses`](super::RealtimeConfig::visualized_buses) allows.
    TooManyVisualizedBuses,
}

impl fmt::Display for ControllerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => write!(f, "the audio command queue is full"),
            Self::NoFreeVoiceId => write!(f, "every voice ID is in use"),
            Self::InvalidSpeed => write!(f, "play speed must be greater than 0"),
            Self::TooManyQueues => write!(f, "too many queues"),
            Self::Disconnected => write!(f, "the audio renderer is gone"),
            Self::StreamsFull => write!(f, "the stream worker has not taken the streams played"),
            Self::NoStreamWorker => write!(f, "the stream worker is gone"),
            Self::InvalidSoundSpeed => {
                write!(f, "the speed of sound must be finite and greater than 0")
            }
            Self::NoSuchSpeaker => write!(f, "the output has no such speaker"),
            Self::TooManyVisualizedBuses => write!(f, "too many buses record visualization data"),
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
    /// Streams played, for the worker to take.
    pub(super) feeds: Producer<WorkerFeed>,
    pub(super) stream_chunks: usize,
    /// Frames of silence streams played because the worker fell behind.
    pub(super) underflow: Arc<AtomicU64>,
    pub(super) worker: Option<StreamWorker>,
    /// The listener, speakers and 3D voices, whose gains are computed here.
    pub(super) spatial: SpatialMirror,
    /// The latest visualization data the renderer published.
    pub(super) visual: VisualReader,
}

/// Where a 3D play starts.
#[derive(Clone, Copy, Debug)]
pub(super) struct Start3d {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// The sound time of a clocked play.
    pub clock: Option<f64>,
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
        self.start(source, options, Start::Now, None, None)
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
        self.start(source, options, Start::Clocked(sound_time), None, None)
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
        self.start(source, options, Start::Background, None, None)
    }

    /// Plays `source` through `bus`'s voice (`Bus::play`). The play fails
    /// with [`Event::Failed`] if the bus is not playing when it is applied.
    pub fn bus_play(
        &mut self,
        bus: &Bus,
        source: &impl Playable,
        options: PlayOptions,
    ) -> Result<VoiceId, ControllerError> {
        self.start(source, options, Start::Now, Some(bus.id()), None)
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
        self.start(source, options, start, Some(bus.id()), None)
    }

    /// Queues a play. A 3D play starts paused at pan 0 and carries its
    /// gains and delay, computed here as `Mixer::play_3d` computes them.
    pub(super) fn start(
        &mut self,
        source: &impl Playable,
        options: PlayOptions,
        start: Start,
        bus: Option<SoundId>,
        spatial: Option<Start3d>,
    ) -> Result<VoiceId, ControllerError> {
        if options.speed.is_nan() || options.speed <= 0.0 {
            return Err(ControllerError::InvalidSpeed);
        }
        if self.commands.is_abandoned() {
            return Err(ControllerError::Disconnected);
        }
        if source.stream().is_some() {
            self.stream_room()?;
        }
        let queue = source.queue_id().filter(|id| !self.queues.contains(id));
        let watch = self.bus_watch(source.bus_id().filter(|_| source.bus_visualization()))?;
        let needed = 1 + usize::from(queue.is_some()) + watch_commands(&watch);
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
        self.send_watch(watch);
        let (mut prepared, feed) = match source.stream() {
            Some(stream) => {
                let underflow = Arc::clone(&self.underflow);
                let (prepared, feed) = stream.prepare_fed(self.stream_chunks, underflow);
                (prepared, Some(feed))
            }
            None => (source.prepare(), None),
        };
        let layout = (prepared.voice.channels(), prepared.voice.base_sample_rate);
        // The speed the voice's filters first run at: a 3D voice's starting
        // Doppler factor multiplies it.
        let mut speed = options.speed;
        let mut sent = PlayOptions {
            bus: Handle::INVALID,
            ..options
        };
        let mut placement = None;
        let mut tracked = None;
        if let Some(at) = spatial {
            let voice = &mut prepared.voice;
            voice.set_flag(flags::PROCESS_3D, true);
            voice.spatial.position = at.position.into();
            voice.spatial.velocity = at.velocity.into();
            let relative = voice.has_flag(flags::LISTENER_RELATIVE);
            let scene = &self.spatial.scene;
            let gains = scene
                .frame()
                .gains(&voice.spatial, relative, Handle::INVALID, Some(id));
            let rate = self.config.sample_rate;
            placement = Some(Placement {
                gains,
                distance_delay: distance_delay(&voice.spatial, relative, scene, rate),
                clock: at.clock,
                paused: options.paused,
            });
            tracked = Some((voice.spatial.clone(), relative, gains.doppler));
            sent.pan = 0.0;
            sent.paused = true;
            speed *= gains.doppler.max(1.0);
        }
        prepared.prepare_filters(speed);
        let request = PlayRequest {
            id,
            voice: prepared.into(),
            options: sent,
            start,
            bus,
            placement,
        };
        if let Err(error) = self.send(Command::Play(request)) {
            self.ids.release(id);
            return Err(error);
        }
        self.retire_credits -= 1;
        if let Some((spatial, relative, doppler)) = tracked {
            let filters = FilterLayout {
                filters: source.filter_slots(),
                channels: layout.0,
                base_rate: layout.1,
                speed: options.speed,
                reserved: layout.1 * speed,
                bus: source.bus_id(),
            };
            self.spatial.insert(id, spatial, relative, filters, doppler);
        }
        if let Some(feed) = feed {
            // Cannot fail: checked above, and only this thread pushes.
            let _ = self.feeds.push(feed);
        }
        if let Some(bus) = source.bus_id() {
            self.buses.started(bus, id, layout.0, layout.1, speed);
        }
        Ok(id)
    }

    /// Whether one more stream can be handed to the worker.
    pub(super) fn stream_room(&self) -> Result<(), ControllerError> {
        if self.feeds.is_abandoned() {
            return Err(ControllerError::NoStreamWorker);
        }
        if self.feeds.is_full() {
            return Err(ControllerError::StreamsFull);
        }
        Ok(())
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
            self.spatial.forget(event.id());
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
        Status {
            stream_underflow_frames: self.underflow.load(Ordering::Relaxed),
            ..self.shared.read()
        }
    }

    /// The worker that decodes the streams this controller plays; `None`
    /// after the first call. Run it (see [`StreamWorker`]) before playing
    /// streams, or they stay silent.
    pub fn take_stream_worker(&mut self) -> Option<StreamWorker> {
        self.worker.take()
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
