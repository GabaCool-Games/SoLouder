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
//! A [`Collider`] and an [`Attenuator`] on a source change its voices'
//! volume.
//!
//! Velocities only change pitch (Doppler, composed with the voice's play
//! speed): nothing moves voices or the listener by them. A source with
//! distance delay starts late by the time sound takes to reach the listener.
//!
//! Through the real-time [`Controller`](crate::Controller) the same math
//! runs on the game thread: the controller keeps its own copy of the
//! listener, the speakers and each 3D voice's settings, and sends the
//! results to the audio thread, so colliders and attenuators never run
//! there.

mod attenuation;
mod control;
mod custom;
mod doppler;
mod play;
mod scene;
mod settings;
mod vector;

pub use attenuation::Attenuation;
pub use custom::{Attenuator, Collider, SpatialVoice};
pub(crate) use play::{Placement, distance_delay};
pub(crate) use scene::{Gains, Scene};
pub(crate) use settings::{SpatialSettings, VoiceSpatial, spatial_settings_api};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_motion;
