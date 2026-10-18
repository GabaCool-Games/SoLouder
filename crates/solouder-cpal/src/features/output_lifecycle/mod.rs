//! Output lifecycle: device negotiation, the audio callback, device errors
//! and shutdown.

mod callback;
mod health;
mod negotiate;
mod output;

pub use negotiate::StreamPreferences;
pub use output::{
    MAX_BUFFER_FRAMES, Output, OutputConfig, OutputError, OutputInfo, OutputState, open,
};
