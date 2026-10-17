// Ported from SoLoud src/core/soloud_core_3d.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

/// How a 3D voice's volume falls with its distance from the listener
/// (`AudioSource::ATTENUATION_MODELS`). Distances are first clamped to the
/// voice's minimum and maximum distance; `rolloff` scales the fall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Attenuation {
    /// The volume does not depend on distance (`NO_ATTENUATION`), the
    /// default.
    #[default]
    None,
    /// `min / (min + rolloff * (distance - min))` (`INVERSE_DISTANCE`).
    InverseDistance,
    /// `1 - rolloff * (distance - min) / (max - min)` (`LINEAR_DISTANCE`).
    LinearDistance,
    /// `(distance / min) ^ -rolloff` (`EXPONENTIAL_DISTANCE`).
    ExponentialDistance,
}

impl Attenuation {
    /// The model SoLoud numbers `index` (0 to 3). SoLoud treats every other
    /// number as no attenuation, and so does this.
    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::InverseDistance,
            2 => Self::LinearDistance,
            3 => Self::ExponentialDistance,
            _ => Self::None,
        }
    }

    /// The volume factor at `distance`.
    pub(crate) fn attenuate(self, distance: f32, min: f32, max: f32, rolloff: f32) -> f32 {
        let d = clamp(distance, min, max);
        match self {
            Self::None => 1.0,
            Self::InverseDistance => min / (min + rolloff * (d - min)),
            Self::LinearDistance => 1.0 - rolloff * (d - min) / (max - min),
            // `pow` of floats runs in double precision on MSVC.
            Self::ExponentialDistance => f64::from(d / min).powf(f64::from(-rolloff)) as f32,
        }
    }
}

/// `MIN(MAX(distance, min), max)` with SoLoud's macros, whose comparisons
/// decide what a NaN gives.
fn clamp(distance: f32, min: f32, max: f32) -> f32 {
    let d = if distance > min { distance } else { min };
    if d < max { d } else { max }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_clamp_to_the_distance_range() {
        let inverse = Attenuation::InverseDistance;
        assert_eq!(inverse.attenuate(0.5, 1.0, 100.0, 1.0), 1.0);
        assert_eq!(inverse.attenuate(4.0, 1.0, 100.0, 1.0), 0.25);
        assert_eq!(inverse.attenuate(400.0, 1.0, 100.0, 1.0), 0.01);
        let linear = Attenuation::LinearDistance;
        assert_eq!(linear.attenuate(6.0, 1.0, 11.0, 1.0), 0.5);
        assert_eq!(linear.attenuate(60.0, 1.0, 11.0, 1.0), 0.0);
        let exponential = Attenuation::ExponentialDistance;
        assert_eq!(exponential.attenuate(4.0, 2.0, 100.0, 2.0), 0.25);
        assert_eq!(Attenuation::None.attenuate(1e9, 1.0, 2.0, 5.0), 1.0);
    }

    #[test]
    fn unknown_indices_do_not_attenuate() {
        assert_eq!(Attenuation::from_index(3), Attenuation::ExponentialDistance);
        assert_eq!(Attenuation::from_index(4), Attenuation::None);
    }

    #[test]
    fn nan_distance_takes_the_minimum() {
        assert_eq!(clamp(f32::NAN, 1.0, 10.0), 1.0);
    }
}
