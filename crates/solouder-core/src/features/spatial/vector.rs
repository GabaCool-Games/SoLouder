// Ported from SoLoud src/core/soloud_core_3d.cpp (zlib/libpng,
// Copyright (c) 2013-2020 Jari Komppa); altered for SoLouder.

//! SoLoud's `vec3` and `mat3` helpers. Every operation is in `f32` in the
//! order the C++ writes it, except `sqrt`, which the reference's MSVC build
//! runs in double precision (`math.h` has no `float` overload there).

/// A 3D vector (`vec3`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<[f32; 3]> for Vec3 {
    fn from([x, y, z]: [f32; 3]) -> Self {
        Self { x, y, z }
    }
}

impl From<Vec3> for [f32; 3] {
    fn from(v: Vec3) -> Self {
        [v.x, v.y, v.z]
    }
}

impl Vec3 {
    /// All three components are zero (`vec3::null`).
    pub fn is_null(self) -> bool {
        self.x == 0.0 && self.y == 0.0 && self.z == 0.0
    }

    /// The length (`vec3::mag`).
    pub fn mag(self) -> f32 {
        let squared = self.x * self.x + self.y * self.y + self.z * self.z;
        f64::from(squared).sqrt() as f32
    }

    /// Scaled to length 1; the zero vector stays zero (`vec3::normalize`).
    pub fn normalize(self) -> Self {
        let m = self.mag();
        if m == 0.0 {
            return Self::default();
        }
        Self {
            x: self.x / m,
            y: self.y / m,
            z: self.z / m,
        }
    }

    pub fn dot(self, a: Self) -> f32 {
        self.x * a.x + self.y * a.y + self.z * a.z
    }

    pub fn sub(self, a: Self) -> Self {
        Self {
            x: self.x - a.x,
            y: self.y - a.y,
            z: self.z - a.z,
        }
    }

    pub fn cross(self, a: Self) -> Self {
        Self {
            x: self.y * a.z - a.y * self.z,
            y: self.z * a.x - a.z * self.x,
            z: self.x * a.y - a.x * self.y,
        }
    }
}

/// A 3x3 matrix of rows (`mat3`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Mat3 {
    pub rows: [Vec3; 3],
}

impl Mat3 {
    pub fn mul(&self, a: Vec3) -> Vec3 {
        let [r0, r1, r2] = self.rows;
        Vec3 {
            x: r0.x * a.x + r0.y * a.y + r0.z * a.z,
            y: r1.x * a.x + r1.y * a.y + r1.z * a.z,
            z: r2.x * a.x + r2.y * a.y + r2.z * a.z,
        }
    }

    /// The right-handed listener basis (`mat3::lookatRH`): rows are the
    /// listener's x (`up × at`), y and z (`at`) axes.
    pub fn look_at_rh(at: Vec3, up: Vec3) -> Self {
        let z = at.normalize();
        let x = up.cross(z).normalize();
        let y = z.cross(x);
        Self { rows: [x, y, z] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_keeps_the_zero_vector() {
        assert_eq!(Vec3::default().normalize(), Vec3::default());
        let v = Vec3::from([3.0, 0.0, 4.0]);
        assert_eq!(v.mag(), 5.0);
        assert_eq!(<[f32; 3]>::from(v.normalize()), [0.6, 0.0, 0.8]);
    }

    #[test]
    fn default_listener_basis() {
        // SoLoud's default listener looks down -z with +y up.
        let m = Mat3::look_at_rh([0.0, 0.0, -1.0].into(), [0.0, 1.0, 0.0].into());
        assert_eq!(
            <[f32; 3]>::from(m.mul([5.0, 0.0, 0.0].into())),
            [-5.0, 0.0, 0.0]
        );
        assert_eq!(
            <[f32; 3]>::from(m.mul([0.0, 0.0, -2.0].into())),
            [0.0, 0.0, 2.0]
        );
    }
}
