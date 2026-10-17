//! The colliders and attenuators `sound3d` selects by name. Keep identical
//! to the classes of the same names in `reference/tests/parity_runner.cpp`.

use std::sync::Arc;

use solouder_core::{Attenuator, Collider, SpatialVoice};

use crate::scenario::{ScenarioError, Step};

/// A wall at x = user data / 10: sources past it are heard at a quarter.
#[derive(Debug)]
struct WallCollider;

impl Collider for WallCollider {
    fn collide(&self, voice: &SpatialVoice, user_data: i32) -> f32 {
        if voice.position[0] > user_data as f32 / 10.0 {
            0.25
        } else {
            1.0
        }
    }
}

/// Quieter the higher or lower the source: 1 / (1 + |y| * user data / 100).
#[derive(Debug)]
struct ShellCollider;

impl Collider for ShellCollider {
    fn collide(&self, voice: &SpatialVoice, user_data: i32) -> f32 {
        let mut y = voice.position[1];
        if y < 0.0 {
            y = -y;
        }
        1.0 / (1.0 + y * user_data as f32 / 100.0)
    }
}

/// Inverse square of the distance clamped to the minimum and maximum, times
/// the rolloff factor.
#[derive(Debug)]
struct SquareAttenuator;

impl Attenuator for SquareAttenuator {
    fn attenuate(&self, distance: f32, min: f32, max: f32, rolloff: f32) -> f32 {
        let d = if distance > min { distance } else { min };
        let d = if d < max { d } else { max };
        min * min / (d * d) * rolloff
    }
}

/// Full volume closer than the maximum distance, silent from there.
#[derive(Debug)]
struct CutoffAttenuator;

impl Attenuator for CutoffAttenuator {
    fn attenuate(&self, distance: f32, _min: f32, max: f32, _rolloff: f32) -> f32 {
        if distance < max { 1.0 } else { 0.0 }
    }
}

/// `collider=`: a collider by name, or `None` for `none`.
pub(crate) fn collider(step: &Step) -> Result<Option<Arc<dyn Collider>>, ScenarioError> {
    Ok(match step.text("collider")? {
        "wall" => Some(Arc::new(WallCollider)),
        "shell" => Some(Arc::new(ShellCollider)),
        "none" => None,
        other => return Err(step.error(format!("unknown collider `{other}`"))),
    })
}

/// `attenuator=`: an attenuator by name, or `None` for `none`.
pub(crate) fn attenuator(step: &Step) -> Result<Option<Arc<dyn Attenuator>>, ScenarioError> {
    Ok(match step.text("attenuator")? {
        "square" => Some(Arc::new(SquareAttenuator)),
        "cutoff" => Some(Arc::new(CutoffAttenuator)),
        "none" => None,
        other => return Err(step.error(format!("unknown attenuator `{other}`"))),
    })
}
