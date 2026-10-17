//! Small contracts shared by feature slices: buffer limits, prepared PCM and
//! real-time voice IDs.

mod pcm;
mod voice_id;

pub use pcm::{Pcm, PcmError};
pub use voice_id::VoiceId;

/// Frames a source renders per block, and the smallest accepted buffer size
/// (`SAMPLE_GRANULARITY` in SoLoud).
pub const SAMPLE_GRANULARITY: u32 = 512;

/// Most channels a mixer output or a sound may have (7.1 surround).
pub const MAX_CHANNELS: u32 = 8;
