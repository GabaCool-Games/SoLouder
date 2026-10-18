//! Render-time benchmarks for SoLouder, and the pieces its soak test shares.
//!
//! Not published. Every case builds a mixer, splits it into a
//! [`Controller`](solouder_core::Controller) and a
//! [`Renderer`](solouder_core::Renderer), starts its voices through the
//! controller, and times each [`Renderer::render`](solouder_core::Renderer::render)
//! call. `reference/tests/reference_bench.cpp` builds the same cases on the
//! C++ reference and times `Soloud::mix`; `scripts/run_bench.py` runs both
//! and prints them side by side against the callback budget.
//!
//! A new kind of source adds a module of cases and one line in
//! [`cases::all`].

pub mod cases;
mod cases_io;
#[cfg(feature = "sources")]
pub mod cases_sources;
pub mod session;
pub mod signal;
pub mod timing;
