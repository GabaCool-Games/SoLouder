// Ported from SoLoud src/core/soloud_core_3d.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

use super::vector::Vec3;

/// The play speed factor of a source at `offset` from the listener (its
/// position minus the listener's) with the OpenAL 1.1 formula SoLoud uses
/// (`doppler`). The velocities are projected on the source-to-listener
/// vector, so an approaching source or listener raises the pitch (reference
/// patch 0062; SoLoud had the direction backwards).
///
/// A source approaching, or a listener receding, at or past the speed of
/// sound over `factor` gives 0, which holds the voice in place as the mixer
/// does for any speed it cannot play. This is decided on the scaled speeds
/// before dividing (patch 0066), so the divisor of any other case is above
/// 0. SoLoud divided by zero at the speed of sound (fixed by patch 0063,
/// which held only infinite and NaN results), and clamping the speeds left
/// the divisor a rounding error above 0 for some factors, so a source
/// faster than sound played millions of times too fast.
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
    let source_speed = factor * source_speed;
    let listener_speed = factor * listener_speed;
    // Written so that NaN holds the voice too.
    if !(source_speed < sound_speed && listener_speed < sound_speed) {
        return 0.0;
    }
    let value = (sound_speed - listener_speed) / (sound_speed - source_speed);
    // Very near the speed of sound the value can still overflow.
    if (0.0..=f32::MAX).contains(&value) {
        value
    } else {
        0.0
    }
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

    /// Clamped to the speed of sound over a factor of 1.032, a source at
    /// 400 left the divisor about 3e-5 and a speed of about 11 million.
    #[test]
    fn faster_than_sound_holds_whatever_the_rounding() {
        let ahead = z(-1.0);
        assert_eq!(doppler(ahead, z(400.0), z(0.0), 1.032, 343.3), 0.0);
        assert_eq!(doppler(ahead, z(0.0), z(400.0), 1.032, 343.3), 0.0);
        // Just slower than sound plays, however fast.
        let speed = doppler(ahead, z(332.0), z(0.0), 1.032, 343.3);
        assert!(speed > 1.0 && speed.is_finite());
        // A negative factor turns approach into recession; a still pair is
        // unchanged.
        assert_eq!(doppler(ahead, z(0.0), z(0.0), -1.0, 343.3), 1.0);
        assert!(doppler(ahead, z(400.0), z(0.0), -1.0, 343.3) < 1.0);
    }
}
