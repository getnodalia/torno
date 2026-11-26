// SPDX-License-Identifier: MIT OR Apache-2.0
//! Vectors, points, frames and affine transforms in `f64`.
//!
//! Units are millimetres and radians, right-handed, Z up (like the rest of Torno). A point and a
//! vector share one type ([`Point3`] is an alias of [`Vec3`]); which one a value is comes from how
//! it is used, and [`Transform`] has separate `point` and `vector` methods for that reason.

use crate::error::{BrepError, Result};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// A 2D vector (parameter space, profiles).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

/// A 3D vector.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A point in space. Same representation as [`Vec3`].
pub type Point3 = Vec3;

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const fn new(x: f64, y: f64) -> Vec2 {
        Vec2 { x, y }
    }
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    /// The z component of the 3D cross product.
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    pub fn to_array(self) -> [f64; 2] {
        [self.x, self.y]
    }
    pub fn distance(self, o: Vec2) -> f64 {
        (self - o).norm()
    }
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const X: Vec3 = Vec3 { x: 1.0, y: 0.0, z: 0.0 };
    pub const Y: Vec3 = Vec3 { x: 0.0, y: 1.0, z: 0.0 };
    pub const Z: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn norm_sq(self) -> f64 {
        self.dot(self)
    }
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y).hypot(self.z)
    }
    pub fn distance(self, o: Vec3) -> f64 {
        (self - o).norm()
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    /// The unit vector, or `None` when the length is zero, subnormal-small or not finite.
    pub fn normalized(self) -> Option<Vec3> {
        let n = self.norm();
        if n.is_finite() && n > 1e-290 { Some(self / n) } else { None }
    }
    pub fn lerp(self, o: Vec3, t: f64) -> Vec3 {
        self + (o - self) * t
    }
    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    pub fn from_array(a: [f64; 3]) -> Vec3 {
        Vec3::new(a[0], a[1], a[2])
    }
    /// Some unit vector perpendicular to `self` (which must be non-zero), or `None`.
    pub fn any_perpendicular(self) -> Option<Vec3> {
        let n = self.normalized()?;
        // Cross with the axis the vector is least aligned with: well conditioned.
        let axis = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
            Vec3::X
        } else if n.y.abs() <= n.z.abs() {
            Vec3::Y
        } else {
            Vec3::Z
        };
        n.cross(axis).normalized()
    }
    /// Rotate `self` about the unit axis `k` by `angle` radians (Rodrigues).
    pub fn rotated(self, k: Vec3, angle: f64) -> Vec3 {
        let (s, c) = angle.sin_cos();
        self * c + k.cross(self) * s + k * (k.dot(self) * (1.0 - c))
    }
}

macro_rules! vec_ops {
    ($t:ident { $($f:ident),+ }) => {
        impl Add for $t { type Output = $t; fn add(self, o: $t) -> $t { $t { $($f: self.$f + o.$f),+ } } }
        impl Sub for $t { type Output = $t; fn sub(self, o: $t) -> $t { $t { $($f: self.$f - o.$f),+ } } }
        impl Neg for $t { type Output = $t; fn neg(self) -> $t { $t { $($f: -self.$f),+ } } }
        impl Mul<f64> for $t { type Output = $t; fn mul(self, s: f64) -> $t { $t { $($f: self.$f * s),+ } } }
        impl Mul<$t> for f64 { type Output = $t; fn mul(self, v: $t) -> $t { v * self } }
        impl Div<f64> for $t { type Output = $t; fn div(self, s: f64) -> $t { $t { $($f: self.$f / s),+ } } }
        impl AddAssign for $t { fn add_assign(&mut self, o: $t) { $(self.$f += o.$f;)+ } }
        impl SubAssign for $t { fn sub_assign(&mut self, o: $t) { $(self.$f -= o.$f;)+ } }
    };
}
vec_ops!(Vec2 { x, y });
vec_ops!(Vec3 { x, y, z });

/// A right-handed orthonormal frame: an origin and three unit axes with `x × y = z`.
///
/// Every analytic curve and surface is placed by one. Constructors verify orthonormality to
/// `1e-9`, so a `Frame` that exists is usable without further checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: Point3,
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
}

/// How far from orthonormal a frame given by the caller may be.
pub const FRAME_TOLERANCE: f64 = 1e-9;

impl Frame {
    /// The world frame.
    pub const WORLD: Frame = Frame { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y, z: Vec3::Z };

    /// A frame with the given origin and `z` axis; `x_hint` fixes the rotation about `z` (its
    /// component along `z` is removed). With no hint an arbitrary perpendicular is used.
    pub fn new(origin: Point3, z: Vec3, x_hint: Option<Vec3>) -> Result<Frame> {
        if !origin.is_finite() || !z.is_finite() {
            return Err(BrepError::non_finite("frame origin or axis"));
        }
        let z = z.normalized().ok_or_else(|| BrepError::degenerate("frame axis has zero length"))?;
        let x = match x_hint {
            Some(h) => {
                if !h.is_finite() {
                    return Err(BrepError::non_finite("frame x hint"));
                }
                (h - z * h.dot(z)).normalized().ok_or_else(|| BrepError::degenerate("frame x hint is parallel to the axis"))?
            }
            None => z.any_perpendicular().ok_or_else(|| BrepError::degenerate("frame axis has zero length"))?,
        };
        let y = z.cross(x);
        Ok(Frame { origin, x, y, z })
    }

    /// A frame from an origin and `x`, `y` axes (made orthonormal: `y` loses its component along `x`).
    pub fn from_xy(origin: Point3, x: Vec3, y: Vec3) -> Result<Frame> {
        if !origin.is_finite() || !x.is_finite() || !y.is_finite() {
            return Err(BrepError::non_finite("frame origin or axes"));
        }
        let x = x.normalized().ok_or_else(|| BrepError::degenerate("frame x axis has zero length"))?;
        let y = (y - x * y.dot(x)).normalized().ok_or_else(|| BrepError::degenerate("frame x and y axes are parallel"))?;
        Ok(Frame { origin, x, y, z: x.cross(y) })
    }

    /// Check that this frame is orthonormal and right-handed to [`FRAME_TOLERANCE`]. Frames
    /// built through [`Frame::new`] and [`Frame::from_xy`] always are; this is for values
    /// assembled by hand.
    pub fn validate(&self) -> Result<()> {
        if !(self.origin.is_finite() && self.x.is_finite() && self.y.is_finite() && self.z.is_finite()) {
            return Err(BrepError::non_finite("frame"));
        }
        let ok = (self.x.norm() - 1.0).abs() < FRAME_TOLERANCE
            && (self.y.norm() - 1.0).abs() < FRAME_TOLERANCE
            && (self.z.norm() - 1.0).abs() < FRAME_TOLERANCE
            && self.x.dot(self.y).abs() < FRAME_TOLERANCE
            && self.x.dot(self.z).abs() < FRAME_TOLERANCE
            && self.y.dot(self.z).abs() < FRAME_TOLERANCE
            && self.x.cross(self.y).dot(self.z) > 1.0 - FRAME_TOLERANCE;
        if ok { Ok(()) } else { Err(BrepError::invalid("frame axes are not an orthonormal right-handed triple")) }
    }

    /// World coordinates of local `(x, y, z)`.
    pub fn to_world(&self, local: Vec3) -> Point3 {
        self.origin + self.x * local.x + self.y * local.y + self.z * local.z
    }
    /// Local coordinates of a world point.
    pub fn to_local(&self, p: Point3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }
    /// The frame turned about its own `z` axis by `angle` radians.
    pub fn rotated_about_z(&self, angle: f64) -> Frame {
        let (s, c) = angle.sin_cos();
        Frame { origin: self.origin, x: self.x * c + self.y * s, y: self.y * c - self.x * s, z: self.z }
    }
    /// The same frame with its `z` axis (and `y`) reversed; `x` stays. Circles in it run clockwise.
    pub fn flipped(&self) -> Frame {
        Frame { origin: self.origin, x: self.x, y: -self.y, z: -self.z }
    }
}

/// A 3×3 matrix, row-major.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat3 {
    pub m: [[f64; 3]; 3],
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3 { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] };

    pub fn mul_vec(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.m[0][0] * v.x + self.m[0][1] * v.y + self.m[0][2] * v.z,
            self.m[1][0] * v.x + self.m[1][1] * v.y + self.m[1][2] * v.z,
            self.m[2][0] * v.x + self.m[2][1] * v.y + self.m[2][2] * v.z,
        )
    }
    pub fn mul_mat(&self, o: &Mat3) -> Mat3 {
        let mut r = [[0.0; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| self.m[i][k] * o.m[k][j]).sum();
            }
        }
        Mat3 { m: r }
    }
    pub fn transpose(&self) -> Mat3 {
        let m = &self.m;
        Mat3 { m: [[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]] }
    }
    pub fn det(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
    /// The inverse, or `None` when the matrix is singular to the scale of its entries.
    pub fn inverse(&self) -> Option<Mat3> {
        let m = &self.m;
        let det = self.det();
        let scale: f64 = m.iter().flatten().map(|v| v.abs()).fold(0.0, f64::max);
        if !det.is_finite() || det.abs() <= 1e-14 * scale * scale * scale || scale == 0.0 {
            return None;
        }
        let c = |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
        let inv = [
            [c(1, 2, 1, 2), -c(0, 2, 1, 2), c(0, 1, 1, 2)],
            [-c(1, 2, 0, 2), c(0, 2, 0, 2), -c(0, 1, 0, 2)],
            [c(1, 2, 0, 1), -c(0, 2, 0, 1), c(0, 1, 0, 1)],
        ];
        let mut r = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = inv[i][j] / det;
            }
        }
        Some(Mat3 { m: r })
    }
}

/// An affine transform `p ↦ linear · p + translation`.
///
/// Rigid motions (rotation + translation) and uniform scales keep analytic curves and surfaces
/// analytic; a general linear part is allowed here as a math object, but the B-rep never applies
/// it to analytic geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub linear: Mat3,
    pub translation: Vec3,
}

impl Transform {
    pub const IDENTITY: Transform = Transform { linear: Mat3::IDENTITY, translation: Vec3::ZERO };

    pub fn translation(t: Vec3) -> Transform {
        Transform { linear: Mat3::IDENTITY, translation: t }
    }
    /// A rotation of `angle` radians about the axis through `origin` along `axis`.
    pub fn rotation(origin: Point3, axis: Vec3, angle: f64) -> Result<Transform> {
        if !origin.is_finite() || !axis.is_finite() || !angle.is_finite() {
            return Err(BrepError::non_finite("rotation"));
        }
        let k = axis.normalized().ok_or_else(|| BrepError::degenerate("rotation axis has zero length"))?;
        let cols = [Vec3::X.rotated(k, angle), Vec3::Y.rotated(k, angle), Vec3::Z.rotated(k, angle)];
        let linear = Mat3 { m: [[cols[0].x, cols[1].x, cols[2].x], [cols[0].y, cols[1].y, cols[2].y], [cols[0].z, cols[1].z, cols[2].z]] };
        Ok(Transform { linear, translation: origin - linear.mul_vec(origin) })
    }
    /// A uniform scale by `s` about the origin.
    pub fn uniform_scale(s: f64) -> Result<Transform> {
        if !s.is_finite() || s == 0.0 {
            return Err(BrepError::invalid("scale must be finite and non-zero"));
        }
        Ok(Transform { linear: Mat3 { m: [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]] }, translation: Vec3::ZERO })
    }
    /// The rigid transform taking `from` onto `to`.
    pub fn between_frames(from: &Frame, to: &Frame) -> Transform {
        let rows = |f: &Frame| Mat3 { m: [[f.x.x, f.y.x, f.z.x], [f.x.y, f.y.y, f.z.y], [f.x.z, f.y.z, f.z.z]] };
        // Columns of `to` times rows of `from`ᵀ: local coordinates in `from` become world in `to`.
        let linear = rows(to).mul_mat(&rows(from).transpose());
        Transform { linear, translation: to.origin - linear.mul_vec(from.origin) }
    }
    pub fn point(&self, p: Point3) -> Point3 {
        self.linear.mul_vec(p) + self.translation
    }
    pub fn vector(&self, v: Vec3) -> Vec3 {
        self.linear.mul_vec(v)
    }
    /// A surface normal: transformed by the inverse transpose and renormalised. `None` when the
    /// transform is singular.
    pub fn normal(&self, n: Vec3) -> Option<Vec3> {
        self.linear.inverse()?.transpose().mul_vec(n).normalized()
    }
    /// `self` applied after `first`.
    pub fn after(&self, first: &Transform) -> Transform {
        Transform { linear: self.linear.mul_mat(&first.linear), translation: self.point(first.translation) }
    }
    pub fn inverse(&self) -> Option<Transform> {
        let inv = self.linear.inverse()?;
        Some(Transform { linear: inv, translation: -inv.mul_vec(self.translation) })
    }
    pub fn determinant(&self) -> f64 {
        self.linear.det()
    }
    /// Whether the transform turns the surface inside out (a mirror).
    pub fn reverses_orientation(&self) -> bool {
        self.determinant() < 0.0
    }
}

/// The angle in `[0, π]` between two vectors, or `None` if either has no direction.
pub fn angle_between(a: Vec3, b: Vec3) -> Option<f64> {
    let (na, nb) = (a.normalized()?, b.normalized()?);
    // atan2 of |a×b| and a·b is accurate for small and large angles alike (acos is not).
    Some(na.cross(nb).norm().atan2(na.dot(nb)))
}

/// Wrap an angle into `(-π, π]`.
pub fn wrap_pi(a: f64) -> f64 {
    use std::f64::consts::PI;
    if !a.is_finite() {
        return a;
    }
    let mut r = a % (2.0 * PI);
    if r > PI {
        r -= 2.0 * PI;
    } else if r <= -PI {
        r += 2.0 * PI;
    }
    r
}

/// `a`, shifted by a multiple of `period`, to be the closest value to `reference`.
pub fn nearest_periodic(a: f64, reference: f64, period: f64) -> f64 {
    if !(a.is_finite() && reference.is_finite() && period > 0.0) {
        return a;
    }
    a + ((reference - a) / period).round() * period
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI};

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-12
    }

    #[test]
    fn cross_and_dot_follow_the_right_hand_rule() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::X.dot(Vec3::Y), 0.0);
        assert!((Vec3::new(3.0, 4.0, 12.0).norm() - 13.0).abs() < 1e-14);
    }

    #[test]
    fn normalizing_zero_or_nan_gives_none() {
        assert!(Vec3::ZERO.normalized().is_none());
        assert!(Vec3::new(f64::NAN, 0.0, 1.0).normalized().is_none());
        assert!(Vec3::new(f64::INFINITY, 0.0, 1.0).normalized().is_none());
        assert!(Vec3::new(1e-300, 0.0, 0.0).normalized().is_none());
    }

    #[test]
    fn perpendicular_is_unit_and_orthogonal_for_many_directions() {
        let mut s = 0x1234_5678_9abc_def0u64;
        for _ in 0..500 {
            let mut r = || {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
            };
            let v = Vec3::new(r(), r(), r());
            if let Some(p) = v.any_perpendicular() {
                assert!((p.norm() - 1.0).abs() < 1e-12);
                assert!(p.dot(v).abs() < 1e-12 * v.norm().max(1.0));
            }
        }
    }

    #[test]
    fn frame_new_is_orthonormal_and_right_handed() {
        let f = Frame::new(Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 1.0, 1.0), None).unwrap();
        f.validate().unwrap();
        let g = Frame::new(Vec3::ZERO, Vec3::Z, Some(Vec3::new(1.0, 1.0, 5.0))).unwrap();
        assert!(close(g.x, Vec3::new(1.0, 1.0, 0.0).normalized().unwrap()));
        assert!(Frame::new(Vec3::ZERO, Vec3::ZERO, None).is_err());
        assert!(Frame::new(Vec3::ZERO, Vec3::Z, Some(Vec3::Z)).is_err());
        assert!(Frame::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z, None).is_err());
    }

    #[test]
    fn frame_validate_rejects_a_left_handed_or_skewed_frame() {
        let mut f = Frame::WORLD;
        f.z = -f.z;
        assert!(f.validate().is_err());
        let mut g = Frame::WORLD;
        g.y = Vec3::new(0.1, 1.0, 0.0);
        assert!(g.validate().is_err());
    }

    #[test]
    fn local_world_round_trip() {
        let f = Frame::new(Vec3::new(5.0, -2.0, 1.0), Vec3::new(0.3, 0.4, 0.5), Some(Vec3::X)).unwrap();
        let p = Vec3::new(1.5, -7.0, 2.25);
        assert!(close(f.to_world(f.to_local(p)), p));
    }

    #[test]
    fn rotation_about_z_by_a_quarter_turn() {
        let r = Transform::rotation(Vec3::ZERO, Vec3::Z, FRAC_PI_2).unwrap();
        assert!(close(r.point(Vec3::X), Vec3::Y));
        assert!(close(r.vector(Vec3::Y), -Vec3::X));
        assert!((r.determinant() - 1.0).abs() < 1e-14);
        assert!(!r.reverses_orientation());
    }

    #[test]
    fn rotation_about_an_offset_axis_fixes_the_axis() {
        let o = Vec3::new(1.0, 2.0, 3.0);
        let r = Transform::rotation(o, Vec3::new(0.0, 0.0, 2.0), 1.234).unwrap();
        assert!(close(r.point(o), o));
        assert!(close(r.point(o + Vec3::Z * 7.0), o + Vec3::Z * 7.0));
        assert!(Transform::rotation(o, Vec3::ZERO, 1.0).is_err());
        assert!(Transform::rotation(o, Vec3::Z, f64::NAN).is_err());
    }

    #[test]
    fn transform_inverse_and_composition() {
        let a = Transform::rotation(Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0), 0.7).unwrap();
        let b = Transform::translation(Vec3::new(4.0, 5.0, 6.0));
        let ab = b.after(&a);
        let p = Vec3::new(0.3, -0.2, 9.0);
        assert!(close(ab.point(p), b.point(a.point(p))));
        let inv = ab.inverse().unwrap();
        assert!(close(inv.point(ab.point(p)), p));
    }

    #[test]
    fn singular_transform_has_no_inverse() {
        let t = Transform { linear: Mat3 { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]] }, translation: Vec3::ZERO };
        assert!(t.inverse().is_none());
        assert!(t.normal(Vec3::Z).is_none());
        assert!(Transform::uniform_scale(0.0).is_err());
    }

    #[test]
    fn a_mirror_reverses_orientation() {
        let m = Transform { linear: Mat3 { m: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] }, translation: Vec3::ZERO };
        assert!(m.reverses_orientation());
    }

    #[test]
    fn between_frames_maps_local_coordinates() {
        let from = Frame::new(Vec3::new(1.0, 1.0, 0.0), Vec3::Z, Some(Vec3::X)).unwrap();
        let to = Frame::new(Vec3::new(-3.0, 2.0, 5.0), Vec3::new(0.0, 1.0, 1.0), Some(Vec3::X)).unwrap();
        let t = Transform::between_frames(&from, &to);
        let local = Vec3::new(0.4, -1.1, 2.0);
        assert!(close(t.point(from.to_world(local)), to.to_world(local)));
    }

    #[test]
    fn angle_between_is_accurate_for_tiny_angles() {
        let a = Vec3::X;
        let b = Vec3::new(1.0, 1e-9, 0.0);
        assert!((angle_between(a, b).unwrap() - 1e-9).abs() < 1e-15);
        assert!((angle_between(a, -a).unwrap() - PI).abs() < 1e-15);
        assert!(angle_between(a, Vec3::ZERO).is_none());
    }

    #[test]
    fn periodic_helpers() {
        assert!((wrap_pi(3.0 * PI) - PI).abs() < 1e-12);
        assert!((wrap_pi(-PI) - PI).abs() < 1e-12);
        assert!((nearest_periodic(0.1, 2.0 * PI, 2.0 * PI) - (0.1 + 2.0 * PI)).abs() < 1e-12);
        assert!(wrap_pi(f64::NAN).is_nan());
    }
}
