// Ported from SoLoud src/core/soloud_core_3d.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::vector::Vec3;

/// The play speed factor of a source at `offset` from the listener (its
/// position minus the listener's) with the OpenAL 1.1 formula SoLoud uses
/// (`doppler`). The velocities are projected on the source-to-listener
/// vector, so an approaching source or listener raises the pitch (reference
/// patch 0062; SoLoud had the direction backwards). Speeds are capped at the
/// speed of sound over `factor`; reaching it in either direction gives 0,
/// which holds the voice in place as the mixer does for any speed it cannot
/// play (patch 0063; SoLoud divided by zero and gave the voice an infinite or
/// NaN speed).
pub(crate) fn doppler(
    offset: Vec3,
    source: Vec3,
    listener: Vec3,
    factor: f32,
    sound_speed: f32,
) -> f32 {
    let to_listener = offset.neg();
    let distance = to_listener.mag();
    if distance == 0.0 {
        return 1.0;
    }
    let listener_speed = to_listener.dot(listener) / distance;
    let source_speed = to_listener.dot(source) / distance;
    let max_speed = sound_speed / factor;
    let source_speed = min(source_speed, max_speed);
    let listener_speed = min(listener_speed, max_speed);
    let value = (sound_speed - factor * listener_speed) / (sound_speed - factor * source_speed);
    // Infinity and NaN (at the speed of sound) hold the voice (patch 0063).
    if (0.0..=f32::MAX).contains(&value) {
        value
    } else {
        0.0
    }
}

/// SoLoud's `MIN` macro: `b` unless `a < b`, so a NaN `a` gives `b`.
fn min(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(value: f32) -> Vec3 {
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: value,
        }
    }

    #[test]
    fn approaching_raises_the_pitch() {
        let ahead = z(-10.0);
        // A source approaching at 100 and one receding at 100.
        assert_eq!(
            doppler(ahead, z(100.0), z(0.0), 1.0, 343.3),
            343.3 / (343.3 - 100.0)
        );
        assert_eq!(
            doppler(ahead, z(-100.0), z(0.0), 1.0, 343.3),
            343.3 / (343.3 + 100.0)
        );
        // The listener approaching a still source.
        assert_eq!(
            doppler(ahead, z(0.0), z(-100.0), 1.0, 343.3),
            (343.3 + 100.0) / 343.3
        );
        // A factor of 0 turns Doppler off; a source at the listener has none.
        assert_eq!(doppler(ahead, z(100.0), z(0.0), 0.0, 343.3), 1.0);
        assert_eq!(doppler(z(0.0), z(100.0), z(5.0), 1.0, 343.3), 1.0);
    }

    #[test]
    fn the_speed_of_sound_holds_the_voice() {
        let ahead = z(-10.0);
        // A source approaching at the speed of sound or faster.
        assert_eq!(doppler(ahead, z(343.3), z(0.0), 1.0, 343.3), 0.0);
        assert_eq!(doppler(ahead, z(1602.8), z(0.0), 1.0, 343.3), 0.0);
        // A listener receding that fast hears nothing either (#97's case).
        assert_eq!(doppler(ahead, z(0.0), z(1602.8), 1.0, 343.3), 0.0);
        // Both: 0 / 0.
        assert_eq!(doppler(ahead, z(400.0), z(400.0), 1.0, 343.3), 0.0);
        // NaN anywhere holds the voice too.
        assert_eq!(doppler(ahead, z(f32::NAN), z(0.0), 1.0, 343.3), 0.0);
        assert_eq!(doppler(ahead, z(0.0), z(0.0), f32::INFINITY, 343.3), 0.0);
        assert!(doppler(ahead, z(-1.0e6), z(0.0), 1.0, 343.3) > 0.0);
    }
}
