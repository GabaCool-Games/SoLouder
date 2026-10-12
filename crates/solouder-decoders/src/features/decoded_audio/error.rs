//! Why a file could not be decoded.

use std::fmt;

use solouder_core::{MAX_CHANNELS, PcmError};

/// Limits applied before decoded audio is allocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Largest number of decoded samples (frames times channels) accepted.
    pub max_samples: u64,
}

impl Default for DecodeLimits {
    /// 256 Mi samples (1 GiB of `f32`): about 46 minutes of 48 kHz stereo.
    fn default() -> Self {
        Self {
            max_samples: 1 << 28,
        }
    }
}

/// Why a file could not be decoded.
#[derive(Debug)]
#[non_exhaustive]
pub enum DecodeError {
    /// The data is not in the expected container format.
    NotRecognized,
    /// The container or codec variant is valid but not supported.
    Unsupported(String),
    /// The data is malformed or truncated.
    Malformed(String),
    /// Channel counts must be 1 to [`MAX_CHANNELS`].
    UnsupportedChannelCount(u32),
    /// The sample rate is missing or 0.
    InvalidSampleRate,
    /// The file holds no audio frames.
    Empty,
    /// The decoded audio would exceed [`DecodeLimits::max_samples`].
    TooLarge {
        /// Samples (frames times channels) the file needs, as far as known.
        samples: u64,
        /// The configured limit.
        limit: u64,
    },
    /// Allocating the decoded audio failed.
    OutOfMemory,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRecognized => write!(f, "not a recognized audio file"),
            Self::Unsupported(what) => write!(f, "unsupported audio: {what}"),
            Self::Malformed(what) => write!(f, "malformed audio file: {what}"),
            Self::UnsupportedChannelCount(n) => {
                write!(
                    f,
                    "unsupported channel count {n} (expected 1 to {MAX_CHANNELS})"
                )
            }
            Self::InvalidSampleRate => write!(f, "missing or zero sample rate"),
            Self::Empty => write!(f, "the file holds no audio frames"),
            Self::TooLarge { samples, limit } => {
                write!(f, "{samples} samples exceed the limit of {limit}")
            }
            Self::OutOfMemory => write!(f, "out of memory while decoding"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<PcmError> for DecodeError {
    fn from(error: PcmError) -> Self {
        match error {
            PcmError::UnsupportedChannelCount(n) => Self::UnsupportedChannelCount(n),
            PcmError::InvalidSampleRate => Self::InvalidSampleRate,
            PcmError::Empty => Self::Empty,
            PcmError::TooLong => Self::TooLarge {
                samples: u64::MAX,
                limit: u64::from(u32::MAX),
            },
            other => Self::Malformed(other.to_string()),
        }
    }
}
