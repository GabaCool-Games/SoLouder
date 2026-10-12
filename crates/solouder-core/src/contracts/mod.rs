//! Small contracts shared by feature slices: buffer limits and prepared PCM.

mod pcm;

pub use pcm::{Pcm, PcmError};

/// Frames a source renders per block, and the smallest accepted buffer size
/// (`SAMPLE_GRANULARITY` in SoLoud).
pub const SAMPLE_GRANULARITY: u32 = 512;

/// Most channels a mixer output or a sound may have (7.1 surround).
pub const MAX_CHANNELS: u32 = 8;
