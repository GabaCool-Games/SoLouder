//! Requests from the controller to the renderer. Everything that allocates
//! (voices, filter instances, queue storage) is built by the controller and
//! moved in; what the renderer replaces goes back as retired items.

use std::collections::VecDeque;

use super::VoiceId;
use super::spatial_mirror::SpatialBatch;
use crate::dsp::Resampler;
use crate::features::filters::FilterInstance;
use crate::features::playback::{BoxedVoice, PlayOptions, Queued, SoundId, Start};
use crate::features::spatial::Placement;

/// One queued request. Voice commands for an ID without a voice do nothing.
#[derive(Debug)]
pub(crate) enum Command {
    Play(PlayRequest),
    Stop(VoiceId),
    StopAll,
    StopSound(SoundId),
    Voice(VoiceId, VoiceCommand),
    GlobalVolume(f32),
    FadeGlobalVolume {
        to: f32,
        seconds: f64,
    },
    OscillateGlobalVolume {
        from: f32,
        to: f32,
        seconds: f64,
    },
    PostClipScaler(f32),
    Resampler(Resampler),
    PauseAll(bool),
    GlobalFilter {
        slot: u32,
        instance: Option<Box<dyn FilterInstance>>,
    },
    /// `voice` `None` changes the global filters.
    FilterParameter {
        voice: Option<VoiceId>,
        slot: u32,
        param: u32,
        change: ParamChange,
    },
    BusFilter {
        bus: SoundId,
        slot: u32,
        instance: Option<Box<dyn FilterInstance>>,
    },
    BusResampler(SoundId, Resampler),
    BusVisualization(SoundId, bool),
    /// `setVisualizationEnable`.
    Visualization(bool),
    /// Publish `bus`'s visualization data in record `slot`.
    WatchBus {
        slot: usize,
        bus: SoundId,
    },
    Annex {
        bus: SoundId,
        voice: VoiceId,
    },
    QueueStorage(SoundId, VecDeque<Queued>),
    QueueAdd {
        queue: SoundId,
        item: Queued,
    },
    /// A 3D update's results, applied to the voices still playing; the
    /// batch goes back to the controller.
    Spatial(Box<SpatialBatch>),
}

/// A voice to start.
#[derive(Debug)]
pub(crate) struct PlayRequest {
    pub id: VoiceId,
    pub voice: BoxedVoice,
    pub options: PlayOptions,
    pub start: Start,
    /// Play through this bus's voice.
    pub bus: Option<SoundId>,
    /// For a 3D play, what the controller computed: the voice starts paused
    /// and is placed with it.
    pub placement: Option<Placement>,
}

/// A change to one voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum VoiceCommand {
    Volume(f32),
    Pan(f32),
    PanAbsolute(f32, f32),
    ChannelVolume(u32, f32),
    Pause(bool),
    Looping(bool),
    LoopPoint(f64),
    AutoStop(bool),
    Inaudible {
        tick: bool,
        kill: bool,
    },
    Protect(bool),
    Delay(u32),
    Seek(f64),
    /// Validated as greater than 0 by the controller.
    Speed(f32),
    SampleRate(f32),
    Fade(Faded, f32, f64),
    Oscillate(Faded, f32, f32, f64),
    SchedulePause(f64),
    ScheduleStop(f64),
}

/// What a voice fade or oscillation changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Faded {
    /// The voice's volume.
    Volume,
    /// The voice's pan.
    Pan,
    /// The voice's relative play speed.
    Speed,
}

/// A change to a filter parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ParamChange {
    Set(f32),
    Fade { to: f32, seconds: f64 },
    Oscillate { from: f32, to: f32, seconds: f64 },
}
