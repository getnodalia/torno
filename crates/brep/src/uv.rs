// SPDX-License-Identifier: MIT OR Apache-2.0
//! Face boundaries in parameter space.
//!
//! The validity checker, the mass-property integrals and the tessellator all need a face's loops
//! as closed polygons in the surface's `(u, v)` plane. Periodic surfaces make that delicate: a
//! loop around a cylinder must be followed continuously (`u` running from `0` to `2π`, not
//! jumping back), and a loop around a sphere's pole or a cone's apex does not close in `(u, v)`
//! at all, because the pole is a whole line of the plane collapsed to a point.
//!
//! This module unwraps a sampled 3D loop into a continuous `(u, v)` path and decides what kind of
//! loop it is:
//!
//! * **closed**: the path returns to its start (every loop of a plane, cylinder or torus face made
//!   of a disk, a seam-cut rectangle, ...);
//! * **around a pole**: on a sphere or cone the path advances by exactly `±2π` in `u`; the loop
//!   is closed through the pole, a vertical run up to the pole, a horizontal run across, and a
//!   vertical run back. Which pole of a sphere is decided by the loop's direction and the face's
//!   sense (the face lies to the left of its boundary);
//! * anything else is refused: a loop that winds the wrong way round a cylinder or torus, or
//!   passes through a pole, cannot be represented and says so.

use crate::error::{BrepError, Result};
use crate::math::Vec2;
use crate::robust::{Sign, orient2d};
use crate::surface::Surface;
use crate::topology::{Brep, LoopId};
use std::f64::consts::{FRAC_PI_2, TAU};

/// How near (in parameter units) a loop sample may come to a pole before it is refused.
const POLE_REFUSAL: f64 = 1e-7;
/// How close to a whole turn the winding of a pole loop must be.
const WINDING_TOLERANCE: f64 = 1e-6;

/// A loop that wraps once around a pole, closed through it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoleClosure {
    /// The first point again, unwrapped one turn along (`u₀ ± 2π`).
    pub first_unwrapped: Vec2,
    /// The `v` of the pole the loop closes through.
    pub v_pole: f64,
}

/// A loop in parameter space.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopUv {
    /// The unwrapped sample points, first point not repeated.
    pub points: Vec<Vec2>,
    pub closure: Option<PoleClosure>,
}

impl LoopUv {
    /// The closed polygon: the points, followed by the pole detour when there is one.
    pub fn polygon(&self) -> Vec<Vec2> {
        let mut poly = self.points.clone();
        if let (Some(c), Some(first)) = (&self.closure, self.points.first()) {
            poly.push(c.first_unwrapped);
            poly.push(Vec2::new(c.first_unwrapped.x, c.v_pole));
            poly.push(Vec2::new(first.x, c.v_pole));
        }
        poly
    }
    /// Signed area of the polygon (positive when counter-clockwise in `(u, v)`).
    pub fn signed_area(&self) -> f64 {
        polygon_area(&self.polygon())
    }
}

/// The shoelace area of a closed polygon, about its first vertex (so it is accurate for polygons
/// far from the origin).
pub fn polygon_area(poly: &[Vec2]) -> f64 {
    let Some(&o) = poly.first() else { return 0.0 };
    let mut sum = 0.0;
    for w in poly.windows(2) {
        sum += (w[0] - o).cross(w[1] - o);
    }
    0.5 * sum
}

/// Unwrap the 3D points of a loop (in traversal order, the first point not repeated at the end)
/// into the parameter plane of `surface`. `same_sense` is the face's sense, used to pick the
/// pole of a sphere.
pub fn loop_uv(surface: &Surface, pts: &[crate::math::Point3], same_sense: bool) -> Result<LoopUv> {
    if pts.len() < 2 {
        return Err(BrepError::degenerate("a loop needs at least two sample points"));
    }
    let singular = surface.singular_v();
    let (period_u, period_v) = surface.periods();
    let mut uv: Vec<Vec2> = Vec::with_capacity(pts.len());
    for p in pts {
        let next = surface.parameters(*p, uv.last().copied())?;
        if singular.iter().any(|&vs| (next.y - vs).abs() < POLE_REFUSAL) {
            return Err(BrepError::unsupported("a boundary passes through a pole of its surface (sphere pole or cone apex)"));
        }
        uv.push(next);
    }
    let first = uv[0];
    let first_unwrapped = surface.parameters(pts[0], uv.last().copied())?;
    let net = first_unwrapped - first;
    let scale_ok = |x: f64| x.abs() < WINDING_TOLERANCE;
    if scale_ok(net.x) && scale_ok(net.y) {
        return Ok(LoopUv { points: uv, closure: None });
    }
    let winds_u = period_u.is_some_and(|per| scale_ok(net.x.abs() - per)) && scale_ok(net.y);
    if winds_u && !singular.is_empty() {
        let v_pole = match surface {
            Surface::Sphere(_) => {
                let dir = if net.x > 0.0 { 1.0 } else { -1.0 };
                dir * if same_sense { FRAC_PI_2 } else { -FRAC_PI_2 }
            }
            _ => singular[0],
        };
        return Ok(LoopUv { points: uv, closure: Some(PoleClosure { first_unwrapped, v_pole }) });
    }
    let _ = period_v;
    Err(BrepError::topology(format!(
        "a loop does not close in the parameter plane (it advances by ({:.6}, {:.6}) around the surface): a face on a periodic surface needs seam edges",
        net.x, net.y
    )))
}

/// Whether `p` is inside the polygons taken together under the even-odd rule: an exact test (the
/// sign of every crossing comes from [`orient2d`]). Points exactly on an edge may go either way.
pub fn inside_even_odd(polygons: &[&[Vec2]], p: Vec2) -> Result<bool> {
    let mut inside = false;
    for poly in polygons {
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            if (a.y <= p.y) != (b.y <= p.y) {
                // Orientation of (a, b, p) relative to the edge's direction decides which side p is on.
                let s = orient2d(a.to_array(), b.to_array(), p.to_array())?;
                let upward = b.y > a.y;
                if (upward && s == Sign::Positive) || (!upward && s == Sign::Negative) {
                    inside = !inside;
                }
            }
        }
    }
    Ok(inside)
}

/// Samples needed by callers to tell whether a loop winds once around the axis of a periodic
/// surface: a full turn is `2π`.
pub const FULL_TURN: f64 = TAU;

/// The 3D sample points of a loop: every coedge cut at `chord` tolerance, endpoints taken from
/// the vertices, the final point of each coedge left out (it starts the next one).
pub fn loop_points(brep: &Brep, loop_id: LoopId, chord: f64) -> Result<Vec<crate::math::Point3>> {
    let lp = brep.loop_(loop_id)?;
    let mut pts = Vec::new();
    for &c in &lp.coedges {
        let co = brep.coedge(c)?;
        let edge = brep.edge(co.edge)?;
        let mut ts = edge.curve.sample_params(edge.t0, edge.t1, chord)?;
        if co.reversed {
            ts.reverse();
        }
        let last = ts.len().saturating_sub(1);
        let start = brep.vertex(brep.coedge_start(c)?)?.point;
        for (i, &t) in ts.iter().enumerate().take(last) {
            pts.push(if i == 0 { start } else { edge.curve.point(t) });
        }
    }
    Ok(pts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Frame, Vec3};
    use crate::surface::{Cone, Cylinder, Plane, Sphere};

    fn circle_pts(r: f64, z: f64, n: usize, ccw: bool) -> Vec<Vec3> {
        (0..n)
            .map(|i| {
                let a = TAU * i as f64 / n as f64 * if ccw { 1.0 } else { -1.0 };
                Vec3::new(r * a.cos(), r * a.sin(), z)
            })
            .collect()
    }

    #[test]
    fn plane_loops_close_and_have_the_orientation_of_their_points() {
        let s = Surface::Plane(Plane::new(Frame::WORLD).unwrap());
        let ccw = loop_uv(&s, &circle_pts(2.0, 0.0, 32, true), true).unwrap();
        assert!(ccw.closure.is_none());
        assert!((ccw.signed_area() - 0.5 * 32.0 * 4.0 * (TAU / 32.0).sin()).abs() < 1e-12);
        let cw = loop_uv(&s, &circle_pts(2.0, 0.0, 32, false), true).unwrap();
        assert!(cw.signed_area() < 0.0);
    }

    #[test]
    fn a_cylinder_loop_with_a_seam_closes_as_a_rectangle() {
        let s = Surface::Cylinder(Cylinder::new(Frame::WORLD, 1.0).unwrap());
        let n = 16;
        let height = 1.5;
        // Bottom circle forward, the seam point again (u = 2π), up the seam, the top circle
        // backward, and the loop closes down the seam (u = 0) onto the first point.
        let mut pts = circle_pts(1.0, 0.0, n, true);
        pts.push(Vec3::new(1.0, 0.0, 0.0));
        pts.push(Vec3::new(1.0, 0.0, height));
        let mut top = circle_pts(1.0, height, n, true);
        top.reverse();
        pts.extend(top);
        let uv = loop_uv(&s, &pts, true).unwrap();
        assert!(uv.closure.is_none());
        let width = uv.points.iter().map(|p| p.x).fold(f64::MIN, f64::max) - uv.points.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        assert!((width - TAU).abs() < 1e-12, "{width}");
        assert!((uv.signed_area() - TAU * height).abs() < 1e-12, "{}", uv.signed_area());
    }

    #[test]
    fn a_cylinder_equator_alone_is_refused() {
        let s = Surface::Cylinder(Cylinder::new(Frame::WORLD, 1.0).unwrap());
        let err = loop_uv(&s, &circle_pts(1.0, 0.0, 16, true), true).unwrap_err();
        assert!(matches!(err, BrepError::Topology(_)), "{err}");
    }

    #[test]
    fn a_hemisphere_loop_closes_through_the_pole_its_direction_and_sense_select() {
        let s = Surface::Sphere(Sphere::new(Frame::WORLD, 2.0).unwrap());
        let eq = circle_pts(2.0, 0.0, 32, true);
        let north = loop_uv(&s, &eq, true).unwrap();
        assert!((north.closure.unwrap().v_pole - FRAC_PI_2).abs() < 1e-15);
        assert!(north.signed_area() > 0.0);
        let south = loop_uv(&s, &circle_pts(2.0, 0.0, 32, false), true).unwrap();
        assert!((south.closure.unwrap().v_pole + FRAC_PI_2).abs() < 1e-15);
        assert!(south.signed_area() > 0.0);
        // An inside-out hemisphere (same_sense false) traversed the same way is the other cap, CW in (u, v).
        let flipped = loop_uv(&s, &eq, false).unwrap();
        assert!((flipped.closure.unwrap().v_pole + FRAC_PI_2).abs() < 1e-15);
        assert!(flipped.signed_area() < 0.0);
    }

    #[test]
    fn a_cone_loop_closes_through_the_apex() {
        let s = Surface::Cone(Cone::new(Frame::WORLD, 0.5).unwrap());
        let r = 2.0;
        let z = r / 0.5f64.tan();
        let ring = circle_pts(r, z, 32, true);
        let uv = loop_uv(&s, &ring, true).unwrap();
        assert_eq!(uv.closure.unwrap().v_pole, 0.0);
        // The ring runs right at v = v_ring and the pole detour comes back along v = 0: a
        // rectangle of width 2π traversed clockwise when the ring is at positive v.
        let v_ring = uv.points[0].y;
        assert!(v_ring > 0.0);
        assert!((uv.signed_area() + TAU * v_ring).abs() < 1e-9, "{}", uv.signed_area());
    }

    #[test]
    fn a_loop_through_a_pole_is_refused() {
        let s = Surface::Sphere(Sphere::new(Frame::WORLD, 1.0).unwrap());
        let pts = vec![Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        assert!(matches!(loop_uv(&s, &pts, true), Err(BrepError::Unsupported(_))));
        assert!(loop_uv(&s, &pts[..1], true).is_err());
    }

    #[test]
    fn even_odd_inside_test_with_a_hole() {
        let outer = [Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)];
        let hole = [Vec2::new(1.0, 1.0), Vec2::new(1.0, 3.0), Vec2::new(3.0, 3.0), Vec2::new(3.0, 1.0)];
        let polys: [&[Vec2]; 2] = [&outer, &hole];
        assert!(inside_even_odd(&polys, Vec2::new(0.5, 0.5)).unwrap());
        assert!(!inside_even_odd(&polys, Vec2::new(2.0, 2.0)).unwrap());
        assert!(!inside_even_odd(&polys, Vec2::new(5.0, 2.0)).unwrap());
        assert!(!inside_even_odd(&polys, Vec2::new(-1.0, 2.0)).unwrap());
        assert!(inside_even_odd(&polys, Vec2::new(f64::NAN, 2.0)).is_err());
    }
}
