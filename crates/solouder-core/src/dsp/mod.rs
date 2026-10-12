//! Reusable mixing kernels. They operate on planar buffers and hold no state
//! beyond their arguments.
//!
//! The arithmetic follows SoLoud's x86 SSE code paths operation by operation,
//! so results match the C++ reference bit for bit apart from denormals (SoLoud
//! flushes them to zero) and `cos`/`sin` rounding in the pan law.

pub(crate) mod clip;
pub(crate) mod fader;
pub(crate) mod pan;
pub(crate) mod resample;

pub use clip::Clipper;
pub use resample::Resampler;
