//! SoLouder device output through CPAL.
//!
//! [`open`] picks a configuration of the default output device that the
//! mixer can render, splits a mixer into a controller and a renderer, and runs
//! the renderer in the device callback. The game keeps the [`Output`] alive
//! and plays through the returned [`Controller`](solouder_core::Controller).
//!
//! The callback never allocates, locks or waits. Device errors are reported
//! through [`Output::state`]; a panic in the renderer silences the output
//! instead of unwinding into the backend.

mod features;

pub use features::output_lifecycle::{
    MAX_BUFFER_FRAMES, Output, OutputConfig, OutputError, OutputInfo, OutputState,
    StreamPreferences, open,
};
