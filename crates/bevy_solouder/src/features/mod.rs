//! Vertical slices of the Bevy plugin.

pub(crate) mod decoded_audio;
#[cfg(feature = "sources")]
pub(crate) mod generated_audio;
pub(crate) mod output_lifecycle;
pub(crate) mod playback;
