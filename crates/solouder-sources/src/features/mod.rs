//! One module per source.

#[cfg(feature = "monotone")]
pub mod monotone;
#[cfg(feature = "noise")]
pub mod noise;
#[cfg(feature = "sfxr")]
pub mod sfxr;
#[cfg(feature = "tedsid")]
pub mod tedsid;
#[cfg(feature = "vic")]
pub mod vic;
