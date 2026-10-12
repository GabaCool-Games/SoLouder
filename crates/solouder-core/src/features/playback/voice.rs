// Ported from SoLoud src/core/soloud_audiosource.cpp,
// src/core/soloud_core_voiceops.cpp and src/audiosource/wav/soloud_wav.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::f64::consts::PI;

use super::sound::Sound;
use crate::contracts::{MAX_CHANNELS, Pcm, SAMPLE_GRANULARITY};
use crate::dsp::fader::Fader;

const MAX: usize = MAX_CHANNELS as usize;
const BLOCK: usize = SAMPLE_GRANULARITY as usize;

/// Voice flag bits, as in `AudioSourceInstance::FLAGS`.
pub(crate) mod flags {
    pub const LOOPING: u32 = 1;
    pub const PROTECTED: u32 = 2;
    pub const PAUSED: u32 = 4;
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

/// One playing instance of a [`Sound`] (`AudioSourceInstance` plus
/// `WavInstance`). Fields keep SoLoud's meaning and units.
#[derive(Debug)]
pub(crate) struct Voice {
    pcm: Pcm,
    /// Next frame to read from `pcm`.
    offset: u32,
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
    /// A voice at the start of `sound`, before `play` applies volume and pan.
    pub fn new(sound: &Sound, play_index: u32) -> Self {
        let pcm = sound.pcm().clone();
        let rate = pcm.sample_rate();
        Self {
            pcm,
            offset: 0,
            play_index,
            flags: if sound.looping() { flags::LOOPING } else { 0 },
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
            src_offset: 0,
            leftover_samples: 0,
            block_samples: 0,
            delay_samples: 0,
            resample: None,
            faders: VoiceFaders::default(),
        }
    }

    pub fn channels(&self) -> usize {
        self.pcm.channels() as usize
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
        let count = (self.pcm.frames() - self.offset).min(frames);
        let from = self.offset as usize;
        for c in 0..self.pcm.channels() {
            let src = &self.pcm.channel(c)[from..from + count as usize];
            let at = c as usize * BLOCK + start;
            block[at..at + count as usize].copy_from_slice(src);
        }
        self.offset += count;
        count
    }

    /// The voice's samples, released when it stops.
    pub fn into_pcm(self) -> Pcm {
        self.pcm
    }

    /// `WavInstance::hasEnded`: a looping voice never ends.
    pub fn has_ended(&self) -> bool {
        !self.has_flag(flags::LOOPING) && self.offset >= self.pcm.frames()
    }

    /// Seeks to the loop point, which is always the start for now
    /// (`AudioSourceInstance::seek` with `mLoopPoint == 0` rewinds).
    pub fn seek_to_loop_point(&mut self) {
        self.offset = 0;
        self.stream_position = 0.0;
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
        match self.pcm.channels() {
            4 => (v[2], v[3]) = (l, r),
            6 => (v[2], v[3], v[4], v[5]) = (1.0 / 2.0_f32.sqrt(), 1.0, l, r),
            8 => {
                (v[2], v[3], v[4], v[5]) = (1.0 / 2.0_f32.sqrt(), 1.0, l, r);
                (v[6], v[7]) = (l, r);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(channels: u32, frames: usize) -> Voice {
        let samples = (0..channels as usize * frames).map(|i| i as f32).collect();
        let sound = Sound::new(Pcm::from_planar(samples, channels, 44100.0).unwrap());
        Voice::new(&sound, 0)
    }

    #[test]
    fn read_copies_planar_frames_until_the_end() {
        let mut v = voice(2, 3);
        let mut block = vec![0.0; BLOCK * MAX];
        assert_eq!(v.read(&mut block, 0, 2), 2);
        assert_eq!(v.read(&mut block, 2, 5), 1);
        assert_eq!(block[..3], [0.0, 1.0, 2.0]);
        assert_eq!(block[BLOCK..BLOCK + 3], [3.0, 4.0, 5.0]);
        assert!(v.has_ended());
        v.set_flag(flags::LOOPING, true);
        assert!(!v.has_ended());
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
