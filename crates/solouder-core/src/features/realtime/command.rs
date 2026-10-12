//! Requests from the controller to the renderer.

use super::VoiceId;
use crate::dsp::Resampler;
use crate::features::playback::{PlayOptions, Sound};

/// One queued request. Voice commands for an ID without a voice do nothing.
#[derive(Debug)]
pub(crate) enum Command {
    Play {
        id: VoiceId,
        sound: Sound,
        options: PlayOptions,
    },
    Stop(VoiceId),
    StopAll,
    Volume(VoiceId, f32),
    Pan(VoiceId, f32),
    Pause(VoiceId, bool),
    Looping(VoiceId, bool),
    Protect(VoiceId, bool),
    /// Validated as greater than 0 by the controller.
    Speed(VoiceId, f32),
    GlobalVolume(f32),
    PostClipScaler(f32),
    Resampler(Resampler),
}
