//! Real-time playback: a [`Controller`] for the game thread and a
//! [`Renderer`] for the audio callback, joined by bounded wait-free queues.
//!
//! ```text
//! Controller --commands--> Renderer (owns the Mixer) --> device callback
//!     ^                        |
//!     +--events, retired audio-+   (status counters through atomics)
//! ```
//!
//! Each queue has one producer and one consumer. Offline code can keep using
//! [`Mixer`] directly.

mod command;
mod controller;
mod ids;
mod renderer;
mod status;

use std::sync::Arc;

pub use controller::{Controller, ControllerError};
pub use ids::VoiceId;
pub use renderer::Renderer;
pub use status::{Event, FinishReason, Status};

use crate::features::playback::Mixer;
use ids::{IdPool, LiveTable};
use status::Shared;

/// Queue sizes and limits for [`split`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealtimeConfig {
    /// Voice IDs: play requests that may be queued, playing, or finished
    /// with their final event not yet read.
    pub voice_ids: usize,
    /// Commands the queue holds before [`ControllerError::QueueFull`].
    pub command_capacity: usize,
    /// Commands applied per render; the rest wait for the next one, so a
    /// burst of requests cannot make one render arbitrarily slow.
    pub commands_per_render: usize,
    /// Retired audio buffers in flight to the controller. Buffers beyond this
    /// wait on the audio thread, in storage reserved for them, until the
    /// queue has room.
    pub retired_capacity: usize,
}

impl Default for RealtimeConfig {
    fn default() -> Self {
        Self {
            voice_ids: 4096,
            command_capacity: 4096,
            commands_per_render: 512,
            retired_capacity: 4096,
        }
    }
}

/// Splits `mixer` into a controller and a renderer. Allocates every queue
/// and table here, so rendering never does. Configure the mixer (for
/// example its active voice limit) before splitting.
pub fn split(mut mixer: Mixer, config: RealtimeConfig) -> (Controller, Renderer) {
    let voice_ids = config.voice_ids.max(1);
    let commands_per_render = config.commands_per_render.max(1);
    // A play retires at most two buffers: the request's sound, and the
    // voice's audio when it finishes. The controller holds two credits per
    // voice ID and spends two per play, so the mixer's kept list holds
    // everything not yet collected and never has to free audio itself.
    let retire_credits = 2 * voice_ids;
    mixer.keep_retired_audio(retire_credits);
    let (command_tx, command_rx) = rtrb::RingBuffer::new(config.command_capacity.max(1));
    let (event_tx, event_rx) = rtrb::RingBuffer::new(voice_ids * 2);
    let (retired_tx, retired_rx) = rtrb::RingBuffer::new(config.retired_capacity.max(1));
    let shared = Arc::new(Shared::default());
    let controller = Controller {
        commands: command_tx,
        events: event_rx,
        retired: retired_rx,
        ids: IdPool::new(voice_ids),
        retire_credits,
        shared: Arc::clone(&shared),
    };
    let renderer = Renderer {
        mixer,
        commands: command_rx,
        events: event_tx,
        retired: retired_tx,
        live: LiveTable::new(voice_ids),
        shared,
        commands_per_render,
        status: Default::default(),
    };
    (controller, renderer)
}

#[cfg(test)]
#[path = "realtime_tests.rs"]
mod tests;
