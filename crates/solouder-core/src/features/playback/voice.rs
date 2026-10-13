// Ported from SoLoud src/core/soloud_audiosource.cpp,
// src/core/soloud_core_voiceops.cpp and src/audiosource/wav/soloud_wav.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::f64::consts::PI;

use super::bus::BusVoice;
use super::handle::Handle;
use super::queue::{self, Queues};
use super::sound::SoundId;
use super::source::SourceSettings;
use super::stream::{self, StreamReader};
use crate::contracts::{MAX_CHANNELS, Pcm, SAMPLE_GRANULARITY};
use crate::dsp::fader::Fader;
use crate::features::filters::{FILTERS_PER_STREAM, FilterInstance};

const MAX: usize = MAX_CHANNELS as usize;
const BLOCK: usize = SAMPLE_GRANULARITY as usize;

/// Voice flag bits, as in `AudioSourceInstance::FLAGS`.
pub(crate) mod flags {
    pub const LOOPING: u32 = 1;
    pub const PROTECTED: u32 = 2;
    pub const PAUSED: u32 = 4;
    /// Set by 3D attenuation (milestone E); inaudible voices are not mixed.
    pub const INAUDIBLE: u32 = 32;
    pub const INAUDIBLE_KILL: u32 = 64;
    pub const INAUDIBLE_TICK: u32 = 128;
    pub const DISABLE_AUTOSTOP: u32 = 256;
}

/// The two resample blocks a voice owns while it is active: indices into the
/// mixer's resample pool. `current` holds the latest source block and
/// `previous` the one before it, which interpolation reads across.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResampleBlocks {
    pub current: usize,
    pub previous: usize,
}

/// What a voice plays.
#[derive(Debug)]
pub(crate) enum VoiceSource {
    /// A sound's samples (`WavInstance`) and the next frame to read.
    Pcm { pcm: Pcm, offset: u32 },
    /// The mix of the voices playing through this voice (`BusInstance`).
    Bus(Box<BusVoice>),
    /// The sounds queued on the queue `sound_id` (`QueueInstance`), kept in
    /// `Voices::queues`.
    Queue { channels: u32 },
    /// A stream's reader (`WavStreamInstance`): `None` if it could not be
    /// opened. `offset` counts the frames read, up to `frames`.
    Stream {
        reader: Option<Box<dyn StreamReader>>,
        channels: u32,
        frames: u32,
        offset: u32,
    },
}

/// One playing instance of a sound or bus (`AudioSourceInstance`). Fields
/// keep SoLoud's meaning and units.
#[derive(Debug)]
pub(crate) struct Voice {
    pub source: VoiceSource,
    /// The bus voice this voice plays through; [`Handle::INVALID`] is the
    /// main output (`mBusHandle`).
    pub bus: Handle,
    pub sound_id: SoundId,
    pub play_index: u32,
    pub flags: u32,
    pub pan: f32,
    /// Per-speaker gain from the pan law (1 for speakers it does not set).
    pub channel_volume: [f32; MAX],
    /// Per-speaker gain reached at the end of the last mix; ramps start here.
    pub current_channel_volume: [f32; MAX],
    pub set_volume: f32,
    /// `set_volume` times the 3D volume (always 1 until 3D audio is ported).
    pub overall_volume: f32,
    pub base_sample_rate: f32,
    /// `base_sample_rate` times the relative play speed.
    pub sample_rate: f32,
    pub set_relative_play_speed: f32,
    pub overall_relative_play_speed: f32,
    /// Seconds of output rendered while not paused.
    pub stream_time: f64,
    /// Seconds of source played, scaled by play speed.
    pub stream_position: f64,
    pub loop_count: u32,
    /// Seconds into the sound where a looping voice restarts.
    pub loop_point: f64,
    /// Fixed-point read position in the current resample block.
    pub src_offset: u32,
    /// Output frames still to render from the current block.
    pub leftover_samples: u32,
    /// Source frames read into the current block; the rest is silence.
    pub block_samples: u32,
    /// Output frames of silence before the voice starts.
    pub delay_samples: u32,
    pub resample: Option<ResampleBlocks>,
    pub faders: VoiceFaders,
    pub filters: [Option<Box<dyn FilterInstance>>; FILTERS_PER_STREAM],
}

/// A voice's fades and timers, run on its stream time.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VoiceFaders {
    pub volume: Fader,
    pub pan: Fader,
    pub speed: Fader,
    /// Pauses the voice when it finishes (`mPauseScheduler`).
    pub pause: Fader,
    /// Stops the voice when it finishes (`mStopScheduler`).
    pub stop: Fader,
}

impl Voice {
    /// A voice at the start of `source`, before `play` applies volume and
    /// pan, with `extra_flags` on top of the source's.
    pub fn new(
        settings: &SourceSettings,
        sound_id: SoundId,
        source: VoiceSource,
        rate: f32,
        extra_flags: u32,
    ) -> Self {
        Self {
            source,
            bus: Handle::INVALID,
            sound_id,
            play_index: 0,
            flags: settings.voice_flags() | extra_flags,
            pan: 0.0,
            channel_volume: [1.0; MAX],
            current_channel_volume: [0.0; MAX],
            set_volume: 1.0,
            overall_volume: 0.0,
            base_sample_rate: rate,
            sample_rate: rate,
            set_relative_play_speed: 1.0,
            overall_relative_play_speed: 1.0,
            stream_time: 0.0,
            stream_position: 0.0,
            loop_count: 0,
            loop_point: settings.loop_point,
            src_offset: 0,
            leftover_samples: 0,
            block_samples: 0,
            delay_samples: 0,
            resample: None,
            faders: VoiceFaders::default(),
            filters: settings.filter_instances(),
        }
    }

    pub fn channels(&self) -> usize {
        channels_of(&self.source)
    }

    pub fn has_flag(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    pub fn set_flag(&mut self, flag: u32, on: bool) {
        if on {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }

    /// Copies up to `frames` frames into `block` (one resample block: channel
    /// `c` at `c * SAMPLE_GRANULARITY`), starting `start` frames into each
    /// channel, and returns how many were copied (`WavInstance::getAudio`).
    pub fn read(&mut self, block: &mut [f32], start: usize, frames: u32) -> u32 {
        let VoiceSource::Pcm { pcm, offset } = &mut self.source else {
            return stream::read(&mut self.source, block, start, frames);
        };
        let count = (pcm.frames() - *offset).min(frames);
        let from = *offset as usize;
        for c in 0..pcm.channels() {
            let src = &pcm.channel(c)[from..from + count as usize];
            let at = c as usize * BLOCK + start;
            block[at..at + count as usize].copy_from_slice(src);
        }
        *offset += count;
        count
    }

    /// The bus state, for a bus voice.
    pub fn as_bus(&self) -> Option<&BusVoice> {
        match &self.source {
            VoiceSource::Bus(bus) => Some(bus),
            VoiceSource::Pcm { .. } | VoiceSource::Queue { .. } | VoiceSource::Stream { .. } => {
                None
            }
        }
    }

    /// `hasEnded`: a looping voice never ends, nor does a bus.
    pub fn has_ended(&self, queues: &Queues) -> bool {
        match &self.source {
            VoiceSource::Pcm { pcm, offset } => {
                !self.has_flag(flags::LOOPING) && *offset >= pcm.frames()
            }
            VoiceSource::Bus(_) => false,
            VoiceSource::Queue { .. } => queue::has_ended(self, queues),
            VoiceSource::Stream { .. } => {
                stream::has_ended(&self.source, self.has_flag(flags::LOOPING))
            }
        }
    }

    /// SoLoud's generic seek (`AudioSourceInstance::seek`): rewinds unless
    /// `seconds` is past the stream position, then skips frames at the base
    /// sample rate (reference patch 0028) up to the end. The resample blocks
    /// already read still play first. Buses seek in `bus::seek`.
    pub fn seek(&mut self, seconds: f64) {
        let VoiceSource::Pcm { pcm, offset: read } = &mut self.source else {
            let (position, rate) = (self.stream_position, self.base_sample_rate);
            if stream::seek(&mut self.source, position, seconds, rate) {
                self.stream_position = seconds;
            }
            return;
        };
        let mut offset = seconds - self.stream_position;
        if offset <= 0.0 {
            *read = 0;
            self.stream_position = 0.0;
            offset = seconds;
        }
        let frames = (f64::from(self.base_sample_rate) * offset).floor();
        // `(int)` of a value outside its range is INT_MIN on x86: skip nothing.
        if (1.0..2_147_483_648.0).contains(&frames) {
            let end = u64::from(*read) + frames as u64;
            *read = end.min(u64::from(pcm.frames())) as u32;
        }
        self.stream_position = seconds;
    }

    /// The constant-power pan law (`setVoicePan_internal`). Surround speakers
    /// the law does not cover keep their gain.
    pub fn set_pan(&mut self, pan: f32) {
        self.pan = pan;
        let angle = f64::from(pan + 1.0) * PI / 4.0;
        let (l, r) = (angle.cos() as f32, angle.sin() as f32);
        let v = &mut self.channel_volume;
        v[0] = l;
        v[1] = r;
        match channels_of(&self.source) {
            4 => (v[2], v[3]) = (l, r),
            6 => (v[2], v[3], v[4], v[5]) = (1.0 / 2.0_f32.sqrt(), 1.0, l, r),
            8 => {
                (v[2], v[3], v[4], v[5]) = (1.0 / 2.0_f32.sqrt(), 1.0, l, r);
                (v[6], v[7]) = (l, r);
            }
            _ => {}
        }
    }

    /// Sets the left and right gains directly (`Soloud::setPanAbsolute`);
    /// the pan value is unchanged. Quad copies them to the rear pair; 5.1 and
    /// 7.1 give the center and subwoofer their mean.
    pub fn set_pan_absolute(&mut self, left: f32, right: f32) {
        let v = &mut self.channel_volume;
        v[0] = left;
        v[1] = right;
        let mean = (left + right) * 0.5;
        match channels_of(&self.source) {
            4 => (v[2], v[3]) = (left, right),
            6 => (v[2], v[3], v[4], v[5]) = (mean, mean, left, right),
            8 => {
                (v[2], v[3], v[4], v[5]) = (mean, mean, left, right);
                (v[6], v[7]) = (left, right);
            }
            _ => {}
        }
    }

    /// `setVoiceVolume_internal` and `updateVoiceVolume_internal`. A paused
    /// voice jumps to the new gains so it does not ramp when resumed.
    pub fn set_volume(&mut self, volume: f32) {
        self.set_volume = volume;
        self.overall_volume = volume;
        if self.has_flag(flags::PAUSED) {
            self.snap_gains();
        }
    }

    /// Ends any gain ramp: the current gains become the target gains.
    pub fn snap_gains(&mut self) {
        for (current, &volume) in self
            .current_channel_volume
            .iter_mut()
            .zip(&self.channel_volume)
        {
            *current = volume * self.overall_volume;
        }
    }

    /// `setVoiceRelativePlaySpeed_internal`; false (and no change) unless
    /// `speed` is greater than 0. SoLoud also accepts NaN.
    pub fn set_relative_play_speed(&mut self, speed: f32) -> bool {
        if speed.is_nan() || speed <= 0.0 {
            return false;
        }
        self.set_relative_play_speed = speed;
        self.overall_relative_play_speed = speed;
        self.sample_rate = self.base_sample_rate * speed;
        true
    }

    /// Replaces the rate the sound plays at before speed is applied
    /// (`Soloud::setSamplerate`). Any value is accepted; one that is not
    /// greater than 0 renders silence.
    pub fn set_base_sample_rate(&mut self, rate: f32) {
        self.base_sample_rate = rate;
        self.sample_rate = rate * self.overall_relative_play_speed;
    }
}

fn channels_of(source: &VoiceSource) -> usize {
    match source {
        VoiceSource::Pcm { pcm, .. } => pcm.channels() as usize,
        VoiceSource::Bus(bus) => bus.channels(),
        VoiceSource::Queue { channels } | VoiceSource::Stream { channels, .. } => {
            *channels as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::playback::Sound;

    fn voice(channels: u32, frames: usize) -> Voice {
        let samples = (0..channels as usize * frames).map(|i| i as f32).collect();
        Sound::new(Pcm::from_planar(samples, channels, 44100.0).unwrap()).voice()
    }

    fn offset(v: &Voice) -> u32 {
        match v.source {
            VoiceSource::Pcm { offset, .. } => offset,
            _ => unreachable!(),
        }
    }

    #[test]
    fn read_copies_planar_frames_until_the_end() {
        let mut v = voice(2, 3);
        let mut block = vec![0.0; BLOCK * MAX];
        assert_eq!(v.read(&mut block, 0, 2), 2);
        assert_eq!(v.read(&mut block, 2, 5), 1);
        assert_eq!(block[..3], [0.0, 1.0, 2.0]);
        assert_eq!(block[BLOCK..BLOCK + 3], [3.0, 4.0, 5.0]);
        assert!(v.has_ended(&Queues::default()));
        v.set_flag(flags::LOOPING, true);
        assert!(!v.has_ended(&Queues::default()));
    }

    #[test]
    fn seek_rewinds_or_skips_at_the_base_rate() {
        let mut v = voice(1, 44100);
        v.set_relative_play_speed(2.0);
        v.seek(0.25);
        assert_eq!((offset(&v), v.stream_position), (11025, 0.25));
        v.seek(0.5);
        assert_eq!(offset(&v), 22050);
        v.seek(0.1);
        assert_eq!(offset(&v), 4410);
        v.seek(5.0);
        assert!(v.has_ended(&Queues::default()));
        assert_eq!(v.stream_position, 5.0);
        v.seek(f64::NAN);
        assert_eq!(offset(&v), 44100);
    }

    #[test]
    fn pan_law_is_constant_power() {
        let mut v = voice(1, 1);
        v.set_pan(-1.0);
        assert_eq!(v.channel_volume[..2], [1.0, 0.0]);
        v.set_pan(0.3);
        let [l, r] = [v.channel_volume[0], v.channel_volume[1]];
        assert!((l * l + r * r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_non_positive_speeds() {
        let mut v = voice(1, 1);
        assert!(!v.set_relative_play_speed(0.0));
        assert!(!v.set_relative_play_speed(f32::NAN));
        assert!(v.set_relative_play_speed(2.0));
        assert_eq!(v.sample_rate, 88200.0);
    }
}
