// Ported from SoLoud src/audiosource/wav/soloud_wav.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::fmt;
use std::sync::Arc;

use super::MAX_CHANNELS;

/// Immutable, planar `f32` audio, cheap to clone and share between sounds and
/// the voices playing them.
///
/// Planar means each channel's frames are stored contiguously: channel `c`
/// occupies `samples[c * frames..(c + 1) * frames]`. This is the layout of
/// SoLoud's `Wav` after loading (`Wav::loadRawWave`).
#[derive(Clone, Debug)]
pub struct Pcm {
    samples: Arc<[f32]>,
    channels: u32,
    frames: u32,
    sample_rate: f32,
}

impl Pcm {
    /// Wraps planar samples, validating the layout like `Wav::loadRawWave`.
    ///
    /// The frame count is `samples.len() / channels`; samples past the last
    /// whole frame are dropped. Unlike SoLoud, an infinite sample rate is
    /// rejected.
    pub fn from_planar(
        mut samples: Vec<f32>,
        channels: u32,
        sample_rate: f32,
    ) -> Result<Self, PcmError> {
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(PcmError::UnsupportedChannelCount(channels));
        }
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(PcmError::InvalidSampleRate);
        }
        let frames = samples.len() / channels as usize;
        if frames == 0 {
            return Err(PcmError::Empty);
        }
        let frames = u32::try_from(frames).map_err(|_| PcmError::TooLong)?;
        samples.truncate(frames as usize * channels as usize);
        Ok(Self {
            samples: samples.into(),
            channels,
            frames,
            sample_rate,
        })
    }

    /// Number of channels (1 to [`MAX_CHANNELS`]).
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// Frames per channel; always at least 1.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Sample rate in Hz.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The frames of one channel.
    ///
    /// # Panics
    ///
    /// If `channel` is not below [`Pcm::channels`].
    pub fn channel(&self, channel: u32) -> &[f32] {
        assert!(channel < self.channels, "channel {channel} out of range");
        let frames = self.frames as usize;
        let start = channel as usize * frames;
        &self.samples[start..start + frames]
    }
}

/// Why samples could not become [`Pcm`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PcmError {
    /// Channel counts must be 1 to [`MAX_CHANNELS`].
    UnsupportedChannelCount(u32),
    /// The sample rate must be finite and greater than 0.
    InvalidSampleRate,
    /// Fewer samples than one frame.
    Empty,
    /// More than `u32::MAX` frames.
    TooLong,
}

impl fmt::Display for PcmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedChannelCount(n) => {
                write!(
                    f,
                    "unsupported channel count {n} (expected 1 to {MAX_CHANNELS})"
                )
            }
            Self::InvalidSampleRate => write!(f, "sample rate must be finite and greater than 0"),
            Self::Empty => write!(f, "fewer samples than one frame"),
            Self::TooLong => write!(f, "more than u32::MAX frames"),
        }
    }
}

impl std::error::Error for PcmError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_planar_channels_and_drops_partial_frames() {
        let pcm = Pcm::from_planar(vec![1.0, 2.0, 3.0, 4.0, 5.0], 2, 8000.0).unwrap();
        assert_eq!(pcm.frames(), 2);
        assert_eq!(pcm.channel(0), [1.0, 2.0]);
        assert_eq!(pcm.channel(1), [3.0, 4.0]);
    }

    #[test]
    fn rejects_layouts_the_mixer_cannot_play() {
        let samples = || vec![0.0; 18];
        assert_eq!(
            Pcm::from_planar(samples(), 9, 44100.0).unwrap_err(),
            PcmError::UnsupportedChannelCount(9)
        );
        assert_eq!(
            Pcm::from_planar(samples(), 0, 44100.0).unwrap_err(),
            PcmError::UnsupportedChannelCount(0)
        );
        assert_eq!(
            Pcm::from_planar(vec![0.0], 2, 44100.0).unwrap_err(),
            PcmError::Empty
        );
        for rate in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                Pcm::from_planar(samples(), 1, rate).unwrap_err(),
                PcmError::InvalidSampleRate
            );
        }
    }
}
