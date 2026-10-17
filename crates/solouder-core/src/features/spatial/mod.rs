//! Spatial audio: voices placed in 3D around one listener, attenuated with
//! distance and spread over the output's speakers.
//!
//! As in SoLoud, a 3D voice's gains change only when they are computed:
//! [`Mixer::play_3d`](crate::Mixer::play_3d) computes them before the voice's
//! first frame (#95), and [`Mixer::update_3d_audio`](crate::Mixer::update_3d_audio)
//! recomputes every 3D voice's after positions or settings change. A voice
//! whose volume falls below the inaudibility threshold is not mixed (or, if
//! it ticks, advances silently), and is stopped if its inaudible behavior
//! says so.
//!
//! The speaker gain includes the attenuation and the voice's overall volume
//! does too, so the attenuation applies twice to the output, as in SoLoud.
//! Doppler, distance delay, colliders, attenuators, `LEFT_HANDED_3D` and
//! clocked or bus 3D plays come in milestone E unit 7; 3D through the
//! real-time controller in unit 8.

mod attenuation;
mod control;
mod play;
mod scene;
mod settings;
mod vector;

pub use attenuation::Attenuation;
pub(crate) use scene::Scene;
pub(crate) use settings::{SpatialSettings, VoiceSpatial, spatial_settings_api};

#[cfg(test)]
mod tests;
