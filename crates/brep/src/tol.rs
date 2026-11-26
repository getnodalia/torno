// SPDX-License-Identifier: MIT OR Apache-2.0
//! The tolerance model.
//!
//! # What a tolerance means here
//!
//! * **Linear tolerance** (`linear`, millimetres): two points closer than this are *the same
//!   point* for every topological decision (a vertex lies on an edge, an edge lies on a surface,
//!   a loop closes). It is an absolute distance, not relative to the model size.
//! * **Angular tolerance** (`angular`, radians): two directions whose angle is below this are
//!   *the same direction* (parallel axes, tangent faces).
//!
//! Defaults: linear `1e-6 mm` (one nanometre) and angular `1e-9 rad`. They rest on this budget:
//! the supported coordinate range is [`MAX_MODEL_EXTENT`] (`±1e6 mm`, a kilometre), where one
//! `f64` rounding step is about `1.2e-10 mm`; a chain of a few hundred operations keeps the
//! accumulated rounding well below `1e-6 mm`. Angular `1e-9` times a 1 m feature is `1e-6 mm`,
//! so the two tolerances agree at the size of an ordinary part.
//!
//! Predicates ([`crate::robust`]) do **not** use tolerances: they are exact. Tolerances enter
//! only where a decision compares measured geometry (validity checks, constructors checking
//! their input, tessellation matching).
//!
//! A tolerance is never silently widened: checks that need a looser value say so in their report.

use crate::error::{BrepError, Result};
use crate::math::Vec3;

/// The largest coordinate magnitude (mm) for which the default tolerances are meaningful.
pub const MAX_MODEL_EXTENT: f64 = 1.0e6;

/// Linear and angular tolerances; see the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// Distance (mm) below which two points are the same point.
    pub linear: f64,
    /// Angle (rad) below which two directions are the same direction.
    pub angular: f64,
}

impl Tolerance {
    /// `1e-6 mm` and `1e-9 rad`.
    pub const DEFAULT: Tolerance = Tolerance { linear: 1.0e-6, angular: 1.0e-9 };

    /// A tolerance pair; both must be finite and within `[1e-12, 1]`.
    pub fn new(linear: f64, angular: f64) -> Result<Tolerance> {
        for (name, v) in [("linear", linear), ("angular", angular)] {
            if !v.is_finite() {
                return Err(BrepError::non_finite(format!("{name} tolerance")));
            }
            if !(1.0e-12..=1.0).contains(&v) {
                return Err(BrepError::OutOfRange(format!("{name} tolerance {v} must be within [1e-12, 1]")));
            }
        }
        Ok(Tolerance { linear, angular })
    }

    /// Whether two points are the same point.
    pub fn same_point(&self, a: Vec3, b: Vec3) -> bool {
        a.distance(b) <= self.linear
    }

    /// Whether two non-zero vectors point the same way (or exactly opposite ways, when
    /// `either_sense` is true) within the angular tolerance. Zero or non-finite vectors are never
    /// parallel to anything.
    pub fn parallel(&self, a: Vec3, b: Vec3, either_sense: bool) -> bool {
        let (Some(na), Some(nb)) = (a.normalized(), b.normalized()) else { return false };
        let sin = na.cross(nb).norm();
        let along = na.dot(nb);
        sin <= self.angular && (either_sense || along > 0.0)
    }

    /// Whether a scalar is zero within the linear tolerance.
    pub fn is_zero_length(&self, x: f64) -> bool {
        x.abs() <= self.linear
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_documented_ones() {
        let t = Tolerance::default();
        assert_eq!(t.linear, 1e-6);
        assert_eq!(t.angular, 1e-9);
    }

    #[test]
    fn new_rejects_non_finite_and_out_of_range() {
        assert!(Tolerance::new(f64::NAN, 1e-9).is_err());
        assert!(Tolerance::new(1e-6, f64::INFINITY).is_err());
        assert!(Tolerance::new(0.0, 1e-9).is_err());
        assert!(Tolerance::new(1e-6, 2.0).is_err());
        assert!(Tolerance::new(-1e-6, 1e-9).is_err());
        assert!(Tolerance::new(1e-7, 1e-10).is_ok());
    }

    #[test]
    fn same_point_is_inclusive_of_the_tolerance() {
        let t = Tolerance::DEFAULT;
        assert!(t.same_point(Vec3::ZERO, Vec3::new(5e-7, 0.0, 0.0)));
        assert!(!t.same_point(Vec3::ZERO, Vec3::new(2e-6, 0.0, 0.0)));
    }

    #[test]
    fn parallel_handles_sense_and_degenerate_vectors() {
        let t = Tolerance::DEFAULT;
        assert!(t.parallel(Vec3::X, Vec3::new(2.0, 1e-12, 0.0), false));
        assert!(!t.parallel(Vec3::X, -Vec3::X, false));
        assert!(t.parallel(Vec3::X, -Vec3::X, true));
        assert!(!t.parallel(Vec3::X, Vec3::Y, true));
        assert!(!t.parallel(Vec3::ZERO, Vec3::X, true));
        assert!(!t.parallel(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X, true));
        // 1e-8 rad apart is not parallel at 1e-9 rad.
        assert!(!t.parallel(Vec3::X, Vec3::new(1.0, 1e-8, 0.0), false));
    }
}
