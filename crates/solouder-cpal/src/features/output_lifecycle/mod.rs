//! Output lifecycle: device negotiation, the audio callback, device errors
//! and shutdown.

mod callback;
mod negotiate;
mod output;

pub use negotiate::StreamPreferences;
pub use output::{Output, OutputConfig, OutputError, OutputInfo, OutputState, open};
