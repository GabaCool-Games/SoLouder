//! One module per source.

#[cfg(feature = "ay")]
pub mod ay;
#[cfg(feature = "monotone")]
pub mod monotone;
#[cfg(feature = "noise")]
pub mod noise;
#[cfg(feature = "sfxr")]
pub mod sfxr;
#[cfg(feature = "speech")]
pub mod speech;
#[cfg(feature = "tedsid")]
pub mod tedsid;
#[cfg(feature = "vic")]
pub mod vic;
