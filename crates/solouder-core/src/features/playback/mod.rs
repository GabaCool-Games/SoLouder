//! Playback: sounds, buses, voices and their controls, and rendering of the
//! mix.

mod bus;
mod bus_control;
mod config;
mod control;
mod fades;
mod feed;
mod feed_worker;
mod filter_control;
mod groups;
mod handle;
mod mix_bus;
mod mixer;
mod queries;
mod queue;
mod queue_control;
mod queued;
mod retire;
mod sound;
mod source;
mod start;
mod stream;
mod voice;
mod voices;

pub(crate) use bus::BUS_RATE;
pub use bus::Bus;
pub use config::{ConfigError, MixerConfig};
pub use control::{ControlError, PlayOptions};
pub(crate) use feed_worker::WorkerFeed;
pub use filter_control::FilterTarget;
pub use handle::{Handle, VOICE_COUNT};
pub use mixer::Mixer;
pub use queue::{QUEUE_MAX, Queue};
pub use queue_control::QueueError;
pub use queued::Queueable;
pub(crate) use queued::Queued;
pub use retire::RetiredItem;
pub use sound::{Sound, SoundId};
pub(crate) use source::FilterSlots;
pub use source::Playable;
pub(crate) use start::{BoxedVoice, Start};
pub use stream::{Stream, StreamError, StreamFormat, StreamReader, StreamSource};
pub(crate) use voice::{Voice, flags};
