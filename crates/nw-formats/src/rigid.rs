// SPDX-License-Identifier: MIT OR Apache-2.0
//! Rigid transforms of the client's animation files (build 25810808, 2026-09-18): a unit
//! quaternion `[x, y, z, w]` (CryEngine's `Quat`: the vector part first) and a translation, in
//! metres, of a right-handed space with z up.

use serde::{Deserialize, Serialize};

/// A unit quaternion `[x, y, z, w]`.
pub type Quat = [f32; 4];
/// A point or a vector `[x, y, z]`.
pub type Vec3 = [f32; 3];

/// The quaternion of no rotation.
pub const IDENTITY_QUAT: Quat = [0.0, 0.0, 0.0, 1.0];
/// The zero vector.
pub const ZERO: Vec3 = [0.0, 0.0, 0.0];
/// Above this cosine two rotations are near enough to blend linearly.
const SLERP_LINEAR_COS: f32 = 0.9995;
/// Half, for half angles.
const HALF: f32 = 0.5;
/// Twice, for the rotation of a vector by a quaternion.
const TWICE: f32 = 2.0;
/// A quarter, for the matrix-to-quaternion conversion.
const QUARTER: f32 = 0.25;

/// The sum of `a` and `b`.
pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a` minus `b`.
pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a` times `factor`.
pub fn scale(a: Vec3, factor: f32) -> Vec3 {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

/// The dot product of `a` and `b`.
pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The cross product of `a` and `b`.
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The length of `a`.
pub fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

/// The product `a * b`: `b` first, then `a`.
pub fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The inverse of the unit quaternion `q`.
pub fn quat_conjugate(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

/// `q` scaled to unit length; [`IDENTITY_QUAT`] for a zero `q`.
pub fn quat_normalize(q: Quat) -> Quat {
    let norm = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if norm > 0.0 {
        q.map(|part| part / norm)
    } else {
        IDENTITY_QUAT
    }
}

/// `v` rotated by the unit quaternion `q`.
pub fn rotate(q: Quat, v: Vec3) -> Vec3 {
    let axis = [q[0], q[1], q[2]];
    let t = scale(cross(axis, v), TWICE);
    add(add(v, scale(t, q[3])), cross(axis, t))
}

/// The rotation between `a` (at 0) and `b` (at 1) at `share`, along the shorter arc.
pub fn slerp(a: Quat, b: Quat, share: f32) -> Quat {
    let mut cos = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let b = if cos < 0.0 {
        cos = -cos;
        b.map(|part| -part)
    } else {
        b
    };
    if cos > SLERP_LINEAR_COS {
        return quat_normalize(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * share));
    }
    let angle = cos.acos();
    let sin = angle.sin();
    let from = ((1.0 - share) * angle).sin() / sin;
    let to = (share * angle).sin() / sin;
    quat_normalize(std::array::from_fn(|i| a[i] * from + b[i] * to))
}

/// The point between `a` and `b` at `share`.
pub fn lerp(a: Vec3, b: Vec3, share: f32) -> Vec3 {
    add(a, scale(sub(b, a), share))
}

/// The rotation of `angle_rad` about the z axis (a yaw: counter-clockwise seen from above).
pub fn quat_about_z(angle_rad: f32) -> Quat {
    let (sin, cos) = (angle_rad * HALF).sin_cos();
    [0.0, 0.0, sin, cos]
}

/// The rotation of the Euler angles `degrees` (x, y, z) as the client builds it
/// (`FUN_1463347d0`, called by `FUN_1463342e0` with the angles times pi / 180): the product of the
/// rotations about x, then y, then z, `qx * qy * qz`.
pub fn quat_from_euler_deg(degrees: Vec3) -> Quat {
    let half = degrees.map(|angle| angle.to_radians() * HALF);
    let (sx, cx) = half[0].sin_cos();
    let (sy, cy) = half[1].sin_cos();
    let (sz, cz) = half[2].sin_cos();
    [
        cy * sx * cz + sy * cx * sz,
        sy * cx * cz - cy * sx * sz,
        cy * cx * sz + sy * sx * cz,
        cy * cx * cz - sy * sx * sz,
    ]
}

/// The rotation of a row-major 3 x 3 matrix with orthonormal rows.
pub fn quat_from_rows(rows: [[f32; 3]; 3]) -> Quat {
    let [[m00, m01, m02], [m10, m11, m12], [m20, m21, m22]] = rows;
    let trace = m00 + m11 + m22;
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * TWICE;
        [
            (m21 - m12) / s,
            (m02 - m20) / s,
            (m10 - m01) / s,
            QUARTER * s,
        ]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * TWICE;
        [
            QUARTER * s,
            (m01 + m10) / s,
            (m02 + m20) / s,
            (m21 - m12) / s,
        ]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * TWICE;
        [
            (m01 + m10) / s,
            QUARTER * s,
            (m12 + m21) / s,
            (m02 - m20) / s,
        ]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * TWICE;
        [
            (m02 + m20) / s,
            (m12 + m21) / s,
            QUARTER * s,
            (m10 - m01) / s,
        ]
    };
    quat_normalize(q)
}

/// A rotation and then a translation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rigid {
    /// The rotation, a unit quaternion.
    pub rotation: Quat,
    /// The translation, metres.
    pub translation: Vec3,
}

impl Default for Rigid {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Rigid {
    /// No rotation, no translation.
    pub const IDENTITY: Self = Self {
        rotation: IDENTITY_QUAT,
        translation: ZERO,
    };

    /// A translation only.
    pub const fn translation(translation: Vec3) -> Self {
        Self {
            rotation: IDENTITY_QUAT,
            translation,
        }
    }

    /// A rotation only.
    pub const fn rotation(rotation: Quat) -> Self {
        Self {
            rotation,
            translation: ZERO,
        }
    }

    /// `self * other`: `other` first, then `self`.
    pub fn then_local(&self, other: &Self) -> Self {
        Self {
            rotation: quat_normalize(quat_mul(self.rotation, other.rotation)),
            translation: add(self.translation, rotate(self.rotation, other.translation)),
        }
    }

    /// The inverse.
    pub fn inverse(&self) -> Self {
        let rotation = quat_conjugate(self.rotation);
        Self {
            rotation,
            translation: scale(rotate(rotation, self.translation), -1.0),
        }
    }

    /// Where `point` (in this transform's local space) is.
    pub fn apply(&self, point: Vec3) -> Vec3 {
        add(self.translation, rotate(self.rotation, point))
    }

    /// The transform between `self` (at 0) and `other` (at 1) at `share`.
    pub fn blend(&self, other: &Self, share: f32) -> Self {
        Self {
            rotation: slerp(self.rotation, other.rotation, share),
            translation: lerp(self.translation, other.translation, share),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSE: f32 = 1e-5;

    fn close(a: Vec3, b: Vec3) -> bool {
        length(sub(a, b)) < CLOSE
    }

    #[test]
    fn a_yaw_turns_north_to_the_west() {
        let q = quat_about_z(std::f32::consts::FRAC_PI_2);
        assert!(close(rotate(q, [0.0, 1.0, 0.0]), [-1.0, 0.0, 0.0]));
    }

    #[test]
    fn a_transform_and_its_inverse_cancel() {
        let a = Rigid {
            rotation: quat_from_euler_deg([30.0, -20.0, 75.0]),
            translation: [1.0, 2.0, 3.0],
        };
        let point = [0.3, -0.7, 1.1];
        assert!(close(a.inverse().apply(a.apply(point)), point));
        let both = a.then_local(&a.inverse());
        assert!(close(both.apply(point), point));
    }

    #[test]
    fn the_euler_rotation_is_x_then_y_then_z_in_one_product() {
        let x = quat_from_euler_deg([90.0, 0.0, 0.0]);
        let y = quat_from_euler_deg([0.0, 90.0, 0.0]);
        let z = quat_from_euler_deg([0.0, 0.0, 90.0]);
        let all = quat_from_euler_deg([90.0, 90.0, 90.0]);
        let product = quat_mul(quat_mul(x, y), z);
        assert!((0..4).all(|i| (all[i] - product[i]).abs() < CLOSE));
        assert!(close(rotate(x, [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn a_matrix_gives_its_rotation() {
        // `Hips` of `player_male.chr`: x to -z, y stays, z to x.
        let q = quat_from_rows([[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]]);
        assert!(close(rotate(q, [1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]));
        assert!(close(rotate(q, [0.0, 0.0, 1.0]), [-1.0, 0.0, 0.0]));
    }

    #[test]
    fn a_blend_halfway_is_half_the_turn_and_the_way() {
        let a = Rigid::IDENTITY;
        let b = Rigid {
            rotation: quat_about_z(std::f32::consts::FRAC_PI_2),
            translation: [2.0, 0.0, 0.0],
        };
        let half = a.blend(&b, HALF);
        assert!(close(half.translation, [1.0, 0.0, 0.0]));
        let q = quat_about_z(std::f32::consts::FRAC_PI_4);
        assert!((0..4).all(|i| (half.rotation[i] - q[i]).abs() < CLOSE));
        let opposite = slerp(a.rotation, b.rotation.map(|part| -part), HALF);
        assert!(
            (0..4).all(|i| (opposite[i] - q[i]).abs() < CLOSE),
            "the shorter arc"
        );
    }
}
