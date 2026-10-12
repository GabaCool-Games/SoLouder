// Ported from SoLoud src/backend/null/soloud_null.cpp and src/core/soloud.cpp
// (zlib/libpng, Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use std::fmt;

use crate::contracts::SAMPLE_GRANULARITY;
use crate::dsp::Clipper;

/// Output format of a [`Mixer`](super::Mixer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixerConfig {
    /// Output sample rate in Hz.
    pub sample_rate: u32,
    /// Frames per device buffer. Sets the size of internal scratch space.
    pub buffer_size: u32,
    /// Interleaved output channels: 1, 2, 4, 6 or 8.
    pub channels: u32,
    /// How the final mix is limited to `[-1, 1]`.
    pub clipper: Clipper,
}

impl Default for MixerConfig {
    /// SoLoud's defaults for `Soloud::init` with the null driver.
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            buffer_size: 2048,
            channels: 2,
            clipper: Clipper::Roundoff,
        }
    }
}

impl MixerConfig {
    /// Checks the configuration the way SoLoud's null driver does, and also
    /// rejects a zero sample rate (SoLoud treats 0 as "use the default").
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.sample_rate == 0 {
            return Err(ConfigError::ZeroSampleRate);
        }
        if !matches!(self.channels, 1 | 2 | 4 | 6 | 8) {
            return Err(ConfigError::UnsupportedChannelCount(self.channels));
        }
        if self.buffer_size < SAMPLE_GRANULARITY {
            return Err(ConfigError::BufferTooSmall(self.buffer_size));
        }
        Ok(())
    }
}

/// Why a [`MixerConfig`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// The sample rate was 0.
    ZeroSampleRate,
    /// Channel counts other than 1, 2, 4, 6 and 8 are not supported.
    UnsupportedChannelCount(u32),
    /// The buffer is smaller than [`SAMPLE_GRANULARITY`] frames.
    BufferTooSmall(u32),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroSampleRate => write!(f, "sample rate must be greater than 0"),
            Self::UnsupportedChannelCount(n) => {
                write!(
                    f,
                    "unsupported channel count {n} (expected 1, 2, 4, 6 or 8)"
                )
            }
            Self::BufferTooSmall(n) => {
                write!(
                    f,
                    "buffer of {n} frames is smaller than {SAMPLE_GRANULARITY}"
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(channels: u32, buffer_size: u32) -> MixerConfig {
        MixerConfig {
            channels,
            buffer_size,
            ..MixerConfig::default()
        }
    }

    #[test]
    fn default_is_valid() {
        assert_eq!(MixerConfig::default().validate(), Ok(()));
    }

    #[test]
    fn accepts_supported_channel_counts() {
        for channels in [1, 2, 4, 6, 8] {
            assert_eq!(
                config(channels, 512).validate(),
                Ok(()),
                "{channels} channels"
            );
        }
    }

    #[test]
    fn rejects_unsupported_channel_counts() {
        for channels in [0, 3, 5, 7, 9, 16] {
            assert_eq!(
                config(channels, 512).validate(),
                Err(ConfigError::UnsupportedChannelCount(channels))
            );
        }
    }

    #[test]
    fn rejects_buffers_below_granularity() {
        assert_eq!(
            config(2, 511).validate(),
            Err(ConfigError::BufferTooSmall(511))
        );
    }

    #[test]
    fn rejects_zero_sample_rate() {
        let zero_rate = MixerConfig {
            sample_rate: 0,
            ..MixerConfig::default()
        };
        assert_eq!(zero_rate.validate(), Err(ConfigError::ZeroSampleRate));
    }
}
