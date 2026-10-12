// Ported from SoLoud src/core/soloud.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::config::{ConfigError, MixerConfig};
use super::fades::advance_voices;
use super::mix_bus::{Layout, mix_bus};
use super::voices::Voices;
use crate::contracts::MAX_CHANNELS;
use crate::dsp::Resampler;
use crate::dsp::clip::clip;
use crate::dsp::fader::{Fader, FaderState};

/// The SoLouder mixer: plays [`Sound`](super::Sound)s and renders interleaved
/// `f32` output.
///
/// Corresponds to an initialized SoLoud engine driven by explicit `mix` calls
/// (the null driver). The mixer has a single owner: device output adapters
/// call [`Mixer::mix_interleaved`] from their callback, and offline code calls
/// it directly. Mixing does not allocate.
#[derive(Debug)]
pub struct Mixer {
    pub(super) config: MixerConfig,
    /// Frames the scratch buffers hold per channel; the most one internal mix
    /// pass renders (`mScratchSize`).
    scratch_frames: usize,
    /// Each voice's resampled audio, then the clipped mix (`mScratch`).
    scratch: Vec<f32>,
    /// The accumulated mix before clipping (`mOutputScratch`).
    output_scratch: Vec<f32>,
    pub(super) voices: Voices,
    pub(super) global_volume: f32,
    pub(super) post_clip_scaler: f32,
    pub(super) resampler: Resampler,
    pub(super) stream_time: f64,
    pub(super) global_fader: Fader,
    /// Time of the first clocked play since the last mix (`mLastClockedTime`).
    pub(super) last_clocked_time: f64,
}

impl Mixer {
    /// Creates a mixer, rejecting configurations SoLoud would refuse.
    pub fn new(config: MixerConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        // At least 4096 frames, rounded up to a multiple of 16.
        let scratch_frames = (config.buffer_size.next_multiple_of(16) as usize).max(4096);
        let scratch_len = scratch_frames * MAX_CHANNELS as usize;
        Ok(Self {
            config,
            scratch_frames,
            scratch: vec![0.0; scratch_len],
            output_scratch: vec![0.0; scratch_len],
            voices: Voices::new(),
            global_volume: 1.0,
            post_clip_scaler: 0.95,
            resampler: Resampler::default(),
            stream_time: 0.0,
            global_fader: Fader::default(),
            last_clocked_time: 0.0,
        })
    }

    /// The configuration the mixer was created with.
    pub fn config(&self) -> MixerConfig {
        self.config
    }

    /// Renders `output.len() / channels` frames of interleaved audio.
    ///
    /// Any trailing partial frame is set to silence. Requests longer than the
    /// internal scratch buffer (the buffer size rounded up to a multiple of
    /// 16, and at least 4096 frames) are rendered in several passes, so
    /// volume ramps restart at each pass; SoLoud's `Soloud::mix` instead
    /// writes past its scratch buffer.
    pub fn mix_interleaved(&mut self, output: &mut [f32]) {
        let channels = self.config.channels as usize;
        let whole = output.len() / channels * channels;
        let (frames, partial) = output.split_at_mut(whole);
        partial.fill(0.0);
        if frames.is_empty() {
            // Like `Soloud::mix(buffer, 0)`: no audio, but voices still finish
            // their gain ramps and ended voices stop.
            return self.mix_pass(frames);
        }
        for pass in frames.chunks_mut(self.scratch_frames * channels) {
            self.mix_pass(pass);
        }
    }

    /// One `Soloud::mix` call of at most `scratch_frames` frames.
    fn mix_pass(&mut self, output: &mut [f32]) {
        let channels = self.config.channels as usize;
        let frames = output.len() / channels;
        let stride = frames.next_multiple_of(16);
        let sample_rate = self.config.sample_rate as f32;

        let buffer_time = frames as f32 / sample_rate;
        self.stream_time += f64::from(buffer_time);
        self.last_clocked_time = 0.0;
        // The global volume ramps across the pass from its value before the
        // fader step to its value after.
        let volume0 = self.global_volume;
        if self.global_fader.state != FaderState::Off {
            self.global_volume = self.global_fader.get(self.stream_time);
        }
        let volumes = (volume0, self.global_volume);

        let voices = &mut self.voices;
        advance_voices(voices, buffer_time);
        if voices.dirty {
            voices.calc_active();
        }

        let layout = Layout {
            frames: frames as u32,
            stride,
            channels,
            sample_rate,
            resampler: self.resampler,
        };
        mix_bus(voices, &mut self.output_scratch, &mut self.scratch, layout);
        clip(
            self.config.clipper,
            &self.output_scratch,
            &mut self.scratch,
            channels,
            stride,
            volumes,
            self.post_clip_scaler,
        );
        // Planar to interleaved.
        if stride == 0 {
            return;
        }
        for (c, planar) in self.scratch.chunks_exact(stride).take(channels).enumerate() {
            for (frame, &sample) in output.chunks_exact_mut(channels).zip(planar) {
                frame[c] = sample;
            }
        }
    }
}

#[cfg(test)]
#[path = "mixer_tests.rs"]
mod tests;
