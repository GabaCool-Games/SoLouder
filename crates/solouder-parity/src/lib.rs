//! Parity harness: runs `.scn` scenarios through `solouder-core` and compares
//! the result with output rendered by the C++ SoLoud reference.
//!
//! The scenario format is specified in `tests/scenarios/README.md` at the
//! repository root; the C++ side is `reference/tests/parity_runner.cpp`.

mod bus_ops;
mod compare;
mod fade_ops;
mod fft_ops;
mod files;
mod filter_kinds;
mod filter_ops;
mod generator_ops;
mod ops;
pub mod pattern;
mod queue_ops;
pub mod run;
pub mod scenario;
mod sfxr_ops;
mod spatial_ops;
mod stream_ops;
mod vic_ops;
mod voice_ops;

pub use compare::{Golden, Stats, compare, load_golden};
pub use run::{MAX_MIX_FRAMES, Rendered, run};
pub use scenario::{Scenario, ScenarioError, Step, parse};
