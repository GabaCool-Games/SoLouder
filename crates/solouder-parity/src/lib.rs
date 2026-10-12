//! Parity harness: runs `.scn` scenarios through `solouder-core` and compares
//! the result with output rendered by the C++ SoLoud reference.
//!
//! The scenario format is specified in `tests/scenarios/README.md` at the
//! repository root; the C++ side is `reference/tests/parity_runner.cpp`.

pub mod compare;
mod fade_ops;
mod files;
mod ops;
pub mod pattern;
pub mod run;
pub mod scenario;
mod voice_ops;

pub use compare::{Golden, Stats, compare, load_golden};
pub use run::{MAX_MIX_FRAMES, Rendered, run};
pub use scenario::{Scenario, ScenarioError, Step, parse};
