//! The colliders and attenuators `sound3d` selects by name. Keep identical
//! to the classes of the same names in `reference/tests/parity_runner.cpp`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

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

/// Counts its calls: the n-th (from 0) gives 1 / (1 + n % 4) plus user data
/// / 1000, so a call skipped or added changes the gains of the voices
/// computed after it. A new one for each `sound3d collider=count`.
#[derive(Debug, Default)]
struct CountCollider {
    calls: AtomicU32,
}

impl Collider for CountCollider {
    fn collide(&self, _voice: &SpatialVoice, user_data: i32) -> f32 {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        1.0 / (1.0 + (n % 4) as f32) + user_data as f32 / 1000.0
    }
}

/// Depends on everything a collider sees but the position and handle: the
/// velocity, the listener-relative flag, the distances, the rolloff and
/// Doppler factors, and the user data.
#[derive(Debug)]
struct MetaCollider;

impl Collider for MetaCollider {
    fn collide(&self, voice: &SpatialVoice, user_data: i32) -> f32 {
        let mut g = 1.0f32;
        for (i, &v) in voice.velocity.iter().enumerate() {
            let v = if v < 0.0 { -v } else { v };
            g += v * (i + 1) as f32 / 100.0;
        }
        if voice.listener_relative {
            g += 0.5;
        }
        g += voice.min_distance / 10.0;
        g += voice.max_distance / 100000.0;
        g += voice.rolloff / 5.0;
        g += voice.doppler_factor / 3.0;
        g += user_data as f32 / 100.0;
        2.0 / g
    }
}

/// Depends on the voice's handle: (handle % 61 + 3) / 64. Through the
/// real-time controller the handle is invalid, so scenarios use it last.
#[derive(Debug)]
struct HandleCollider;

impl Collider for HandleCollider {
    fn collide(&self, voice: &SpatialVoice, _user_data: i32) -> f32 {
        (voice.handle.raw() % 61 + 3) as f32 / 64.0
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

/// Depends on the distance as given, unclamped, and on all three
/// parameters: rolloff * 4 / (4 + distance + min / 2 + max / 1000).
#[derive(Debug)]
struct RawAttenuator;

impl Attenuator for RawAttenuator {
    fn attenuate(&self, distance: f32, min: f32, max: f32, rolloff: f32) -> f32 {
        rolloff * 4.0 / (4.0 + distance + min / 2.0 + max / 1000.0)
    }
}

/// `collider=`: a collider by name, or `None` for `none`.
pub(crate) fn collider(step: &Step) -> Result<Option<Arc<dyn Collider>>, ScenarioError> {
    Ok(match step.text("collider")? {
        "wall" => Some(Arc::new(WallCollider)),
        "shell" => Some(Arc::new(ShellCollider)),
        "count" => Some(Arc::new(CountCollider::default())),
        "meta" => Some(Arc::new(MetaCollider)),
        "handle" => Some(Arc::new(HandleCollider)),
        "none" => None,
        other => return Err(step.error(format!("unknown collider `{other}`"))),
    })
}

/// `attenuator=`: an attenuator by name, or `None` for `none`.
pub(crate) fn attenuator(step: &Step) -> Result<Option<Arc<dyn Attenuator>>, ScenarioError> {
    Ok(match step.text("attenuator")? {
        "square" => Some(Arc::new(SquareAttenuator)),
        "cutoff" => Some(Arc::new(CutoffAttenuator)),
        "raw" => Some(Arc::new(RawAttenuator)),
        "none" => None,
        other => return Err(step.error(format!("unknown attenuator `{other}`"))),
    })
}
