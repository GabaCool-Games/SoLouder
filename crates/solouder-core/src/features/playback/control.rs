// Ported from SoLoud src/core/soloud_core_basicops.cpp and
// src/core/soloud_core_setters.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::fmt;

use super::bus;
use super::handle::{Handle, next_play_index};
use super::mixer::Mixer;
use super::queue;
use super::sound::SoundId;
use super::source::Playable;
use super::source::private::Prepared;
use super::voice::{Voice, VoiceSource, flags};
use crate::dsp::Resampler;
use crate::dsp::fader::FaderState;

/// How [`Mixer::play_with`] starts a voice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    /// Voice volume; `None` uses the source's volume.
    pub volume: Option<f32>,
    /// Stereo pan from -1 (left) to 1 (right).
    pub pan: f32,
    /// Start paused; the voice keeps its slot but is not mixed.
    pub paused: bool,
    /// Relative play speed from the first frame; values not greater than 0
    /// (and NaN) play at 1.
    pub speed: f32,
    /// Start protected: never stolen, always mixed. Bus voices always are.
    pub protect: bool,
    /// The bus voice to play through (`aBus`); [`Handle::INVALID`] plays on
    /// the main output. A bus mixes nothing until a bus call such as
    /// [`Mixer::bus_play`] has looked up its handle.
    pub bus: Handle,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            volume: None,
            pan: 0.0,
            paused: false,
            speed: 1.0,
            protect: false,
            bus: Handle::INVALID,
        }
    }
}

/// A control call that SoLoud would answer with `INVALID_PARAMETER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControlError {
    /// Play speeds must be greater than 0.
    InvalidSpeed,
    /// The active voice limit must be 1 to `VOICE_COUNT - 1`.
    InvalidVoiceCount,
    /// The handle does not name an existing voice group.
    InvalidVoiceGroup,
    /// Buses have 1, 2, 4, 6 or 8 channels, queues 1 to 8.
    InvalidChannels,
    /// A bus or queue cannot seek backwards (`NOT_IMPLEMENTED`).
    CannotSeekBack,
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpeed => write!(f, "play speed must be greater than 0"),
            Self::InvalidVoiceCount => write!(f, "active voice limit must be 1 to 1023"),
            Self::InvalidVoiceGroup => write!(f, "not a voice group"),
            Self::InvalidChannels => write!(f, "unsupported channel count"),
            Self::CannotSeekBack => write!(f, "a bus or queue cannot seek backwards"),
        }
    }
}

impl std::error::Error for ControlError {}

/// Playing and controlling voices. Calls with a handle whose voice has
/// stopped do nothing; a voice group handle changes every voice in the group.
impl Mixer {
    /// Starts `source` with default options (`Soloud::play`).
    pub fn play(&mut self, source: &impl Playable) -> Handle {
        self.play_with(source, PlayOptions::default())
    }

    /// Starts `source`. When all `VOICE_COUNT` slots are taken, the oldest
    /// unprotected voice is stopped to make room; if every voice is protected,
    /// nothing plays and [`Handle::INVALID`] is returned. A single-instance
    /// sound, and any bus, first stops its other voices.
    pub fn play_with(&mut self, source: &impl Playable, options: PlayOptions) -> Handle {
        let Prepared {
            voice,
            stop_others,
            default_volume,
        } = source.prepare();
        self.start_voice(voice, stop_others, default_volume, options)
    }

    /// The common part of `play_with`: `default_volume` is the source's.
    pub(crate) fn start_voice(
        &mut self,
        mut voice: Voice,
        stop_others: bool,
        default_volume: f32,
        options: PlayOptions,
    ) -> Handle {
        if stop_others {
            self.voices.stop_sound(voice.sound_id);
        }
        let Some(slot) = self.voices.find_free() else {
            self.voices.retired.put_voice(voice);
            return Handle::INVALID;
        };
        let play_index = self.voices.play_index;
        self.voices.play_index = next_play_index(play_index);
        voice.play_index = play_index;
        voice.bus = options.bus;
        voice.set_flag(flags::PAUSED, options.paused);
        if options.protect {
            voice.set_flag(flags::PROTECTED, true);
        }
        voice.set_pan(options.pan);
        voice.set_volume(options.volume.unwrap_or(default_volume));
        // Start at full volume rather than ramping up from silence.
        voice.snap_gains();
        if !voice.set_relative_play_speed(options.speed) {
            voice.set_relative_play_speed(1.0);
        }
        self.voices.slots[slot] = Some(voice);
        self.voices.dirty = true;
        Handle::new(slot, play_index)
    }

    /// Starts `source` at full gain on the left and right speakers, for music
    /// and ambience (`Soloud::playBackground`).
    pub fn play_background(
        &mut self,
        source: &impl Playable,
        volume: Option<f32>,
        paused: bool,
    ) -> Handle {
        let options = PlayOptions {
            volume,
            paused,
            ..PlayOptions::default()
        };
        let handle = self.play_with(source, options);
        self.set_pan_absolute(handle, 1.0, 1.0);
        handle
    }

    /// Stops a voice immediately.
    pub fn stop(&mut self, handle: Handle) {
        self.voices
            .for_each_slot(handle, |voices, slot| voices.stop(slot));
    }

    /// Stops every voice.
    pub fn stop_all(&mut self) {
        for slot in 0..self.voices.highest {
            self.voices.stop(slot);
        }
    }

    /// Stops every voice of `source` and its clones (`Soloud::stopAudioSource`;
    /// `AudioSource::stop` is this call on each mixer).
    pub fn stop_sound(&mut self, source: &impl Playable) {
        self.stop_sound_id(source.source_id());
    }

    pub(crate) fn stop_sound_id(&mut self, id: SoundId) {
        self.voices.stop_sound(id);
    }

    /// Sets a voice's volume, cancelling a volume fade; the change ramps in
    /// over the next mix.
    pub fn set_volume(&mut self, handle: Handle, volume: f32) {
        self.voices.for_each_slot(handle, |voices, slot| {
            if let Some(voice) = voices.slots[slot].as_mut() {
                voice.faders.volume.state = FaderState::Off;
                voice.set_volume(volume);
                voices.dirty = true;
            }
        });
    }

    /// Sets a voice's pan; the change ramps in over the next mix.
    pub fn set_pan(&mut self, handle: Handle, pan: f32) {
        self.with_voice(handle, |voice| voice.set_pan(pan));
    }

    /// Sets the left and right gains directly, cancelling a pan fade but
    /// keeping the pan value (`Soloud::setPanAbsolute`).
    pub fn set_pan_absolute(&mut self, handle: Handle, left: f32, right: f32) {
        self.with_voice(handle, |voice| {
            voice.faders.pan.state = FaderState::Off;
            voice.set_pan_absolute(left, right);
        });
    }

    /// Sets the gain of one output speaker for the voice, if the voice's
    /// sound has more than `channel` channels.
    pub fn set_channel_volume(&mut self, handle: Handle, channel: u32, volume: f32) {
        self.with_voice(handle, |voice| {
            let channels = voice.channels();
            if let Some(gain) = voice.channel_volume[..channels].get_mut(channel as usize) {
                *gain = volume;
            }
        });
    }

    /// Pauses or resumes a voice, cancelling a scheduled pause. A paused
    /// voice keeps its position.
    pub fn set_pause(&mut self, handle: Handle, paused: bool) {
        self.voices.for_each_slot(handle, |voices, slot| {
            if let Some(voice) = voices.slots[slot].as_mut() {
                voice.faders.pause.state = FaderState::Off;
                voice.set_flag(flags::PAUSED, paused);
                voices.dirty = true;
            }
        });
    }

    /// Pauses or resumes every voice, cancelling scheduled pauses.
    pub fn set_pause_all(&mut self, paused: bool) {
        self.voices.dirty = true;
        for voice in self.voices.slots[..self.voices.highest]
            .iter_mut()
            .flatten()
        {
            voice.faders.pause.state = FaderState::Off;
            voice.set_flag(flags::PAUSED, paused);
        }
    }

    /// Sets whether a voice loops when it reaches the end.
    pub fn set_looping(&mut self, handle: Handle, looping: bool) {
        self.with_voice(handle, |voice| voice.set_flag(flags::LOOPING, looping));
    }

    /// Sets where a looping voice restarts, in seconds into its sound.
    pub fn set_loop_point(&mut self, handle: Handle, seconds: f64) {
        self.with_voice(handle, |voice| voice.loop_point = seconds);
    }

    /// Sets whether a voice stops once its sound has played out.
    pub fn set_auto_stop(&mut self, handle: Handle, auto_stop: bool) {
        self.with_voice(handle, |voice| {
            voice.set_flag(flags::DISABLE_AUTOSTOP, !auto_stop)
        });
    }

    /// Sets whether a voice keeps playing when inaudible (`tick`; such voices
    /// are always among the active voices) and whether it is stopped then
    /// (`kill`). Reference patch 0026: the active set is recomputed at the
    /// next mix.
    pub fn set_inaudible_behavior(&mut self, handle: Handle, tick: bool, kill: bool) {
        self.voices.for_each_slot(handle, |voices, slot| {
            if let Some(voice) = voices.slots[slot].as_mut() {
                voice.set_flag(flags::INAUDIBLE_TICK, tick);
                voice.set_flag(flags::INAUDIBLE_KILL, kill);
                voices.dirty = true;
            }
        });
    }

    /// Protected voices are never stopped to make room for new ones and are
    /// always mixed, even past the active voice limit.
    /// (Reference patch 0023: the active set is recomputed at the next mix.)
    pub fn set_protect_voice(&mut self, handle: Handle, protect: bool) {
        self.voices.for_each_slot(handle, |voices, slot| {
            if let Some(voice) = voices.slots[slot].as_mut() {
                voice.set_flag(flags::PROTECTED, protect);
                voices.dirty = true;
            }
        });
    }

    /// Delays the start of a voice by `frames` output frames.
    pub fn set_delay_samples(&mut self, handle: Handle, frames: u32) {
        self.with_voice(handle, |voice| voice.delay_samples = frames);
    }

    /// Moves a voice to `seconds` into its sound (`Soloud::seek`). Seeking
    /// back restarts the sound and skips forward from there. Audio already
    /// read ahead (up to one 512-frame block) plays before the new position.
    /// A bus or queue cannot seek back; seeking a bus forward mixes its
    /// voices over the time skipped, and a queue skips its queued sounds. For a group, the error is that of any voice that failed.
    pub fn seek(&mut self, handle: Handle, seconds: f64) -> Result<(), ControlError> {
        let (scratch_frames, time) = (self.scratch_frames, self.stream_time);
        let mut result = Ok(());
        self.voices.for_each_slot(handle, |voices, slot| {
            let Some(mut voice) = voices.slots[slot].take() else {
                return;
            };
            let sought = match voice.source {
                VoiceSource::Pcm { .. } => {
                    voice.seek(seconds);
                    true
                }
                VoiceSource::Bus(_) => bus::seek(&mut voice, voices, seconds, scratch_frames, time),
                VoiceSource::Queue { .. } => {
                    let (queues, retired) = (&mut voices.queues, &mut voices.retired);
                    queue::seek(&mut voice, queues, retired, seconds, scratch_frames)
                }
            };
            if !sought {
                result = Err(ControlError::CannotSeekBack);
            }
            voices.slots[slot] = Some(voice);
        });
        result
    }

    /// Scales a voice's playback rate, changing pitch and speed together, and
    /// cancels a speed fade (even when the speed is rejected). For a group,
    /// the result is that of its last voice.
    pub fn set_relative_play_speed(
        &mut self,
        handle: Handle,
        speed: f32,
    ) -> Result<(), ControlError> {
        let mut result = Ok(());
        self.with_voice(handle, |voice| {
            voice.faders.speed.state = FaderState::Off;
            result = voice
                .set_relative_play_speed(speed)
                .then_some(())
                .ok_or(ControlError::InvalidSpeed);
        });
        result
    }

    /// Replaces the sample rate a voice's sound plays at, before speed
    /// (`Soloud::setSamplerate`). A rate that is not greater than 0 renders
    /// silence (reference patch 0027).
    pub fn set_sample_rate(&mut self, handle: Handle, rate: f32) {
        self.with_voice(handle, |voice| voice.set_base_sample_rate(rate));
    }

    /// Volume applied to the whole mix before clipping; cancels a global
    /// volume fade.
    pub fn set_global_volume(&mut self, volume: f32) {
        self.global_fader.state = FaderState::Off;
        self.global_volume = volume;
    }

    /// Scale applied after clipping; 0.95 by default.
    pub fn set_post_clip_scaler(&mut self, scale: f32) {
        self.post_clip_scaler = scale;
    }

    /// Interpolation used for every voice.
    pub fn set_resampler(&mut self, resampler: Resampler) {
        self.resampler = resampler;
    }

    /// How many voices are mixed; the rest play silently paused in place
    /// until they are among the loudest again. 16 by default.
    pub fn set_max_active_voice_count(&mut self, count: u32) -> Result<(), ControlError> {
        if self.voices.set_max_active(count as usize) {
            Ok(())
        } else {
            Err(ControlError::InvalidVoiceCount)
        }
    }

    /// Applies `change` to the voice `handle` names, or to each voice of a
    /// group.
    pub(super) fn with_voice(&mut self, handle: Handle, change: impl FnMut(&mut Voice)) {
        self.voices.for_each_voice(handle, change);
    }
}
