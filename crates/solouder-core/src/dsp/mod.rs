//! Reusable mixing kernels. They operate on planar buffers and hold no state
//! beyond their arguments.
//!
//! The arithmetic follows SoLoud's x86 SSE code paths operation by operation,
//! so results match the C++ reference bit for bit apart from denormals (SoLoud
//! flushes them to zero) and `cos`/`sin` rounding in the pan law.
//!
//! SoLoud includes `math.h`, not `cmath`, so in its MSVC build `sin`, `cos`,
//! `atan2` and `sqrt` of a `float` run in double precision: ports widen the
//! argument to `f64` and round the result back to `f32`.

pub(crate) mod clip;
pub(crate) mod fader;
pub mod fft;
pub(crate) mod pan;
pub(crate) mod resample;
pub mod waveform;

pub use clip::Clipper;
pub use resample::Resampler;

/// C's `(int)` conversion as x86 performs it: truncation toward zero, and
/// `INT_MIN` for NaN and for values outside the `int` range.
pub(crate) fn c_int(value: f64) -> i32 {
    let truncated = value.trunc();
    if (-2_147_483_648.0..=2_147_483_647.0).contains(&truncated) {
        truncated as i32
    } else {
        i32::MIN
    }
}
