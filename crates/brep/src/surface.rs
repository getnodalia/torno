// SPDX-License-Identifier: MIT OR Apache-2.0
//! Surfaces: plane, cylinder, cone, sphere, torus and NURBS.
//!
//! Every surface is a map `(u, v) ↦ S(u, v)` placed by a [`Frame`] (origin, `x`, `y`, `z`), with
//! the **natural normal** `Sᵤ × Sᵥ / |Sᵤ × Sᵥ|`. Conventions, with `e(u) = cos u · x + sin u · y`:
//!
//! | surface  | `S(u, v)` | domain |
//! |---|---|---|
//! | plane    | `o + u·x + v·y` | ℝ² |
//! | cylinder | `o + r·e(u) + v·z` | `u` periodic 2π, `v ∈ ℝ` |
//! | cone     | `o + v·z + v·tanα·e(u)` (origin is the **apex**) | `u` periodic 2π, `v ∈ ℝ`; `v < 0` is the opposite nappe |
//! | sphere   | `o + r(cos v·e(u) + sin v·z)` | `u` periodic 2π, `v ∈ [−π/2, π/2]` |
//! | torus    | `o + (R + r cos v)·e(u) + r sin v·z` | `u`, `v` periodic 2π |
//!
//! Natural normals point away from the axis (plane: along `z`; cone: away from the axis on both
//! nappes; sphere and torus: out of the solid). The sphere's poles (`v = ±π/2`) and the cone's
//! apex (`v = 0`) are singular: `u` has no meaning there.

use crate::error::{BrepError, Result};
use crate::math::{Frame, Point3, Vec2, Vec3, nearest_periodic};
use crate::nurbs::NurbsSurface;
use std::f64::consts::{FRAC_PI_2, TAU};

const MAX_SIZE: f64 = crate::curve::MAX_SIZE;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub frame: Frame,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder {
    pub frame: Frame,
    pub radius: f64,
}
/// A cone with its apex at the frame's origin and its axis along `z`; `half_angle` is the angle
/// between the axis and a generatrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone {
    pub frame: Frame,
    pub half_angle: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pub frame: Frame,
    pub radius: f64,
}
/// A ring torus: `major > minor > 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Torus {
    pub frame: Frame,
    pub major: f64,
    pub minor: f64,
}

/// A surface of one of the supported kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    Plane(Plane),
    Cylinder(Cylinder),
    Cone(Cone),
    Sphere(Sphere),
    Torus(Torus),
    Nurbs(Box<NurbsSurface>),
}

/// The result of projecting a point on a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceProjection {
    pub uv: Vec2,
    pub point: Point3,
    pub distance: f64,
    /// `false` when an iteration hit its cap (NURBS surfaces only).
    pub converged: bool,
}

fn size(x: f64, what: &str) -> Result<f64> {
    if !x.is_finite() {
        return Err(BrepError::non_finite(what));
    }
    if !(x > 0.0 && x <= MAX_SIZE) {
        return Err(BrepError::OutOfRange(format!("{what} {x} must be within (0, {MAX_SIZE:e}]")));
    }
    Ok(x)
}

impl Plane {
    pub fn new(frame: Frame) -> Result<Plane> {
        frame.validate()?;
        Ok(Plane { frame })
    }
}
impl Cylinder {
    pub fn new(frame: Frame, radius: f64) -> Result<Cylinder> {
        frame.validate()?;
        Ok(Cylinder { frame, radius: size(radius, "cylinder radius")? })
    }
}
impl Cone {
    /// A cone; `half_angle` must lie within `[1e-6, π/2 − 1e-6]` (a cylinder and a plane are not cones).
    pub fn new(frame: Frame, half_angle: f64) -> Result<Cone> {
        frame.validate()?;
        if !half_angle.is_finite() {
            return Err(BrepError::non_finite("cone half angle"));
        }
        if !(1e-6..=FRAC_PI_2 - 1e-6).contains(&half_angle) {
            return Err(BrepError::OutOfRange(format!("cone half angle {half_angle} must lie within [1e-6, π/2 − 1e-6]")));
        }
        Ok(Cone { frame, half_angle })
    }
    pub fn tan(&self) -> f64 {
        self.half_angle.tan()
    }
}
impl Sphere {
    pub fn new(frame: Frame, radius: f64) -> Result<Sphere> {
        frame.validate()?;
        Ok(Sphere { frame, radius: size(radius, "sphere radius")? })
    }
}
impl Torus {
    pub fn new(frame: Frame, major: f64, minor: f64) -> Result<Torus> {
        frame.validate()?;
        let (major, minor) = (size(major, "torus major radius")?, size(minor, "torus minor radius")?);
        if major <= minor {
            return Err(BrepError::unsupported(format!("torus with major radius {major} <= minor radius {minor} (only ring tori)")));
        }
        Ok(Torus { frame, major, minor })
    }
}

fn radial(f: &Frame, u: f64) -> Vec3 {
    let (s, c) = u.sin_cos();
    f.x * c + f.y * s
}
fn tangential(f: &Frame, u: f64) -> Vec3 {
    let (s, c) = u.sin_cos();
    f.y * c - f.x * s
}

impl Surface {
    pub fn frame(&self) -> Option<&Frame> {
        match self {
            Surface::Plane(s) => Some(&s.frame),
            Surface::Cylinder(s) => Some(&s.frame),
            Surface::Cone(s) => Some(&s.frame),
            Surface::Sphere(s) => Some(&s.frame),
            Surface::Torus(s) => Some(&s.frame),
            Surface::Nurbs(_) => None,
        }
    }

    pub fn point(&self, u: f64, v: f64) -> Point3 {
        match self {
            Surface::Plane(s) => s.frame.origin + s.frame.x * u + s.frame.y * v,
            Surface::Cylinder(s) => s.frame.origin + radial(&s.frame, u) * s.radius + s.frame.z * v,
            Surface::Cone(s) => s.frame.origin + s.frame.z * v + radial(&s.frame, u) * (v * s.tan()),
            Surface::Sphere(s) => s.frame.origin + (radial(&s.frame, u) * v.cos() + s.frame.z * v.sin()) * s.radius,
            Surface::Torus(s) => s.frame.origin + radial(&s.frame, u) * (s.major + s.minor * v.cos()) + s.frame.z * (s.minor * v.sin()),
            Surface::Nurbs(s) => s.point(u, v),
        }
    }

    /// `(Sᵤ, Sᵥ)`.
    pub fn partials(&self, u: f64, v: f64) -> (Vec3, Vec3) {
        match self {
            Surface::Plane(s) => (s.frame.x, s.frame.y),
            Surface::Cylinder(s) => (tangential(&s.frame, u) * s.radius, s.frame.z),
            Surface::Cone(s) => {
                let t = s.tan();
                (tangential(&s.frame, u) * (v * t), s.frame.z + radial(&s.frame, u) * t)
            }
            Surface::Sphere(s) => {
                let (sv, cv) = v.sin_cos();
                (tangential(&s.frame, u) * (s.radius * cv), (radial(&s.frame, u) * -sv + s.frame.z * cv) * s.radius)
            }
            Surface::Torus(s) => {
                let (sv, cv) = v.sin_cos();
                (tangential(&s.frame, u) * (s.major + s.minor * cv), (radial(&s.frame, u) * -sv + s.frame.z * cv) * s.minor)
            }
            Surface::Nurbs(s) => {
                let d = s.derivatives(u, v, 1);
                (d[1][0], d[0][1])
            }
        }
    }

    /// The unit natural normal at `(u, v)`, or `None` where the surface is singular (a cone's
    /// apex, a degenerate NURBS point). The sphere's poles have a well-defined limit and return it.
    pub fn normal(&self, u: f64, v: f64) -> Option<Vec3> {
        match self {
            Surface::Plane(s) => Some(s.frame.z),
            Surface::Cylinder(s) => Some(radial(&s.frame, u)),
            Surface::Cone(s) => {
                if v.abs() < 1e-12 {
                    return None;
                }
                let t = s.tan();
                ((radial(&s.frame, u) - s.frame.z * t) * (v.signum() / (1.0 + t * t).sqrt())).into()
            }
            Surface::Sphere(s) => Some(radial(&s.frame, u) * v.cos() + s.frame.z * v.sin()),
            Surface::Torus(s) => Some(radial(&s.frame, u) * v.cos() + s.frame.z * v.sin()),
            Surface::Nurbs(_) => {
                let (su, sv) = self.partials(u, v);
                su.cross(sv).normalized()
            }
        }
    }

    /// The periods of the parametrisation in `u` and `v` (`2π` where periodic).
    pub fn periods(&self) -> (Option<f64>, Option<f64>) {
        match self {
            Surface::Plane(_) | Surface::Nurbs(_) => (None, None),
            Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_) => (Some(TAU), None),
            Surface::Torus(_) => (Some(TAU), Some(TAU)),
        }
    }

    /// The values of `v` at which `u` is meaningless (the sphere's poles, the cone's apex).
    pub fn singular_v(&self) -> Vec<f64> {
        match self {
            Surface::Sphere(_) => vec![-FRAC_PI_2, FRAC_PI_2],
            Surface::Cone(_) => vec![0.0],
            _ => Vec::new(),
        }
    }

    /// The parameters of the point of the surface closest to `p`, with the periodic parameters
    /// shifted to be nearest to `hint` when one is given. Where `u` is meaningless (a pole) it
    /// is taken from the hint, or `0`.
    pub fn parameters(&self, p: Point3, hint: Option<Vec2>) -> Result<Vec2> {
        let mut uv = self.closest_point(p)?.uv;
        if let Some(h) = hint {
            let (pu, pv) = self.periods();
            if let Some(per) = pu {
                uv.x = nearest_periodic(uv.x, h.x, per);
            }
            if let Some(per) = pv {
                uv.y = nearest_periodic(uv.y, h.y, per);
            }
        }
        Ok(uv)
    }

    /// The closest point of the whole surface (its natural domain) to `p`.
    ///
    /// Closed form for the plane, cylinder, cone, sphere and torus (exact to rounding; the
    /// ambiguous cases, a point on an axis or at a centre, return one of the equally close
    /// answers). For a NURBS surface a Newton iteration from the best of a parameter grid: the
    /// result is a local minimum near the best grid sample, flagged `converged` only when the
    /// iteration met its step tolerance.
    pub fn closest_point(&self, p: Point3) -> Result<SurfaceProjection> {
        if !p.is_finite() {
            return Err(BrepError::non_finite("projected point"));
        }
        let make = |uv: Vec2| {
            let point = self.point(uv.x, uv.y);
            SurfaceProjection { uv, point, distance: point.distance(p), converged: true }
        };
        match self {
            Surface::Plane(s) => {
                let l = s.frame.to_local(p);
                Ok(make(Vec2::new(l.x, l.y)))
            }
            Surface::Cylinder(s) => {
                let l = s.frame.to_local(p);
                Ok(make(Vec2::new(l.y.atan2(l.x), l.z)))
            }
            Surface::Sphere(s) => {
                let l = s.frame.to_local(p);
                let rho = l.x.hypot(l.y);
                let r = rho.hypot(l.z);
                if r <= 1e-300 {
                    return Ok(make(Vec2::new(0.0, 0.0)));
                }
                Ok(make(Vec2::new(if rho > 1e-300 { l.y.atan2(l.x) } else { 0.0 }, (l.z / r).clamp(-1.0, 1.0).asin())))
            }
            Surface::Torus(s) => {
                let l = s.frame.to_local(p);
                let rho = l.x.hypot(l.y);
                let u = if rho > 1e-300 { l.y.atan2(l.x) } else { 0.0 };
                Ok(make(Vec2::new(u, l.z.atan2(rho - s.major))))
            }
            Surface::Cone(s) => {
                let l = s.frame.to_local(p);
                let rho = l.x.hypot(l.y);
                let phi = if rho > 1e-300 { l.y.atan2(l.x) } else { 0.0 };
                let t = s.tan();
                let norm = (1.0 + t * t).sqrt();
                // Project (rho, z) onto the two rays from the apex: (t, ±1)/norm.
                let candidate = |sign: f64| {
                    let dir = Vec2::new(t / norm, sign / norm);
                    let along = (Vec2::new(rho, l.z).dot(dir)).max(0.0);
                    // v is the axial coordinate of the foot; u is the angle (shifted by π on the lower nappe).
                    let v = sign * along / norm;
                    let u = if sign < 0.0 { crate::math::wrap_pi(phi - std::f64::consts::PI) } else { phi };
                    Vec2::new(u, v)
                };
                let (up, down) = (make(candidate(1.0)), make(candidate(-1.0)));
                Ok(if down.distance < up.distance { down } else { up })
            }
            Surface::Nurbs(s) => closest_nurbs(s, p),
        }
    }
}

fn closest_nurbs(s: &NurbsSurface, p: Point3) -> Result<SurfaceProjection> {
    let (du, dv) = (s.domain_u(), s.domain_v());
    let (nu, nv) = s.control_counts();
    let gu = (4 * nu).clamp(8, 96);
    let gv = (4 * nv).clamp(8, 96);
    let at = |i: usize, n: usize, d: (f64, f64)| d.0 + (d.1 - d.0) * i as f64 / n as f64;
    let mut best = (f64::INFINITY, du.0, dv.0);
    for i in 0..=gu {
        for j in 0..=gv {
            let (u, v) = (at(i, gu, du), at(j, gv, dv));
            let d = (s.point(u, v) - p).norm_sq();
            if d < best.0 {
                best = (d, u, v);
            }
        }
    }
    let (mut u, mut v) = (best.1, best.2);
    let mut converged = false;
    for _ in 0..80 {
        let d = s.derivatives(u, v, 2);
        let r = d[0][0] - p;
        let (su, sv, suu, suv, svv) = (d[1][0], d[0][1], d[2][0], d[1][1], d[0][2]);
        let g = Vec2::new(r.dot(su), r.dot(sv));
        let full = [[su.dot(su) + r.dot(suu), su.dot(sv) + r.dot(suv)], [su.dot(sv) + r.dot(suv), sv.dot(sv) + r.dot(svv)]];
        let gn = [[su.dot(su), su.dot(sv)], [su.dot(sv), sv.dot(sv)]];
        let solve = |h: [[f64; 2]; 2]| {
            let det = h[0][0] * h[1][1] - h[0][1] * h[1][0];
            let pos_def = h[0][0] > 0.0 && det > 1e-300;
            pos_def.then(|| Vec2::new(-(h[1][1] * g.x - h[0][1] * g.y) / det, -(-h[1][0] * g.x + h[0][0] * g.y) / det))
        };
        let Some(step) = solve(full).or_else(|| solve(gn)) else { break };
        let (nu_, nv_) = ((u + step.x).clamp(du.0, du.1), (v + step.y).clamp(dv.0, dv.1));
        let moved = (nu_ - u).abs() / (du.1 - du.0) + (nv_ - v).abs() / (dv.1 - dv.0);
        u = nu_;
        v = nv_;
        if moved < 1e-14 {
            converged = true;
            break;
        }
    }
    let point = s.point(u, v);
    Ok(SurfaceProjection { uv: Vec2::new(u, v), point, distance: point.distance(p), converged })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn frame() -> Frame {
        Frame::new(Vec3::new(1.0, -2.0, 3.0), Vec3::new(0.3, 0.5, 1.0), Some(Vec3::X)).unwrap()
    }
    fn all() -> Vec<(&'static str, Surface)> {
        vec![
            ("plane", Surface::Plane(Plane::new(frame()).unwrap())),
            ("cylinder", Surface::Cylinder(Cylinder::new(frame(), 2.5).unwrap())),
            ("cone", Surface::Cone(Cone::new(frame(), 0.4).unwrap())),
            ("sphere", Surface::Sphere(Sphere::new(frame(), 3.0).unwrap())),
            ("torus", Surface::Torus(Torus::new(frame(), 5.0, 1.5).unwrap())),
        ]
    }
    /// Parameters strictly inside every domain and away from singularities.
    fn samples() -> Vec<(f64, f64)> {
        let mut v = Vec::new();
        for i in 0..7 {
            for j in 0..7 {
                v.push((-3.0 + i as f64 * 0.9, -1.3 + j as f64 * 0.43));
            }
        }
        v
    }

    #[test]
    fn partials_match_finite_differences_and_normals_match_the_cross_product() {
        for (name, s) in all() {
            for (u, v) in samples() {
                if matches!(s, Surface::Cone(_)) && v.abs() < 0.05 {
                    continue;
                }
                let h = 1e-6;
                let (su, sv) = s.partials(u, v);
                let fu = (s.point(u + h, v) - s.point(u - h, v)) / (2.0 * h);
                let fv = (s.point(u, v + h) - s.point(u, v - h)) / (2.0 * h);
                assert!(su.distance(fu) < 1e-6 * (1.0 + su.norm()), "{name} su at {u},{v}");
                assert!(sv.distance(fv) < 1e-6 * (1.0 + sv.norm()), "{name} sv");
                let n = s.normal(u, v).unwrap();
                assert!((n.norm() - 1.0).abs() < 1e-12, "{name}");
                let c = su.cross(sv);
                if c.norm() > 1e-6 {
                    assert!(n.distance(c / c.norm()) < 1e-9, "{name}: analytic normal {n:?} vs cross {:?} at {u},{v}", c / c.norm());
                }
            }
        }
    }

    #[test]
    fn parameters_round_trip_on_the_surface() {
        for (name, s) in all() {
            for (u, v) in samples() {
                let (u, v) = match &s {
                    Surface::Sphere(_) => (u, v.clamp(-1.4, 1.4)),
                    Surface::Cone(_) if v.abs() < 0.05 => continue,
                    _ => (u, v),
                };
                let p = s.point(u, v);
                let uv = s.parameters(p, Some(Vec2::new(u, v))).unwrap();
                assert!(s.point(uv.x, uv.y).distance(p) < 1e-10, "{name}");
                // With the hint, periodic parameters come back unwrapped to the hint's turn.
                if !matches!(s, Surface::Plane(_)) {
                    assert!((uv.x - u).abs() < 1e-8, "{name}: u {} vs {u}", uv.x);
                }
            }
        }
    }

    #[test]
    fn closest_point_is_normal_to_the_surface_and_beats_a_grid_scan() {
        let mut k = 77u64;
        let mut r = move || {
            k = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (k >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        };
        for (name, s) in all() {
            for _ in 0..60 {
                let p = frame().to_world(Vec3::new(r() * 9.0, r() * 9.0, r() * 9.0));
                let pr = s.closest_point(p).unwrap();
                assert!((s.point(pr.uv.x, pr.uv.y) - pr.point).norm() < 1e-9);
                // Brute force over the parameter grid (the cone is a double nappe: v on both sides).
                let mut brute = f64::INFINITY;
                for i in 0..400 {
                    for j in 0..=200 {
                        let u = TAU * i as f64 / 400.0;
                        let v = match &s {
                            Surface::Sphere(_) => -FRAC_PI_2 + PI * j as f64 / 200.0,
                            Surface::Torus(_) => TAU * j as f64 / 200.0,
                            Surface::Cone(_) => -12.0 + 24.0 * j as f64 / 200.0,
                            _ => -12.0 + 24.0 * j as f64 / 200.0,
                        };
                        let q = if matches!(s, Surface::Plane(_)) { s.point(-12.0 + 24.0 * i as f64 / 400.0, v) } else { s.point(u, v) };
                        brute = brute.min(q.distance(p));
                    }
                }
                assert!(pr.distance <= brute + 1e-9, "{name}: {} vs brute {brute}", pr.distance);
                // The residual is along the normal (not for the cone at the apex).
                let res = p - pr.point;
                if res.norm() > 1e-9 && pr.uv.y.abs() > 1e-9 {
                    if let Some(n) = s.normal(pr.uv.x, pr.uv.y) {
                        let cross = res.cross(n).norm() / res.norm();
                        let apex = matches!(s, Surface::Cone(_)) && pr.uv.y.abs() < 1e-6;
                        assert!(cross < 1e-7 || apex, "{name}: residual not normal ({cross})");
                    }
                }
            }
        }
    }

    #[test]
    fn closest_point_in_degenerate_positions_does_not_fail() {
        for (_, s) in all() {
            let f = frame();
            for p in [f.origin, f.origin + f.z * 4.0, f.origin + f.z * -4.0, f.to_world(Vec3::new(5.0, 0.0, 0.0))] {
                let pr = s.closest_point(p).unwrap();
                assert!(pr.distance.is_finite() && pr.uv.x.is_finite() && pr.uv.y.is_finite());
            }
        }
    }

    #[test]
    fn cone_is_a_double_nappe_with_a_consistent_parametrisation() {
        let s = Surface::Cone(Cone::new(Frame::WORLD, 0.5).unwrap());
        let up = s.point(0.0, 2.0);
        assert!((up.x - 2.0 * 0.5f64.tan()).abs() < 1e-14 && up.z == 2.0);
        let down = s.parameters(Vec3::new(1.0, 0.0, -1.0 / 0.5f64.tan()), None).unwrap();
        assert!(down.y < 0.0);
        assert!(s.point(down.x, down.y).distance(Vec3::new(1.0, 0.0, -1.0 / 0.5f64.tan())) < 1e-12);
        assert!(s.normal(0.0, 0.0).is_none());
    }

    #[test]
    fn constructors_reject_bad_input() {
        assert!(Cylinder::new(frame(), 0.0).is_err());
        assert!(Sphere::new(frame(), f64::NAN).is_err());
        assert!(Torus::new(frame(), 1.0, 2.0).is_err());
        assert!(Torus::new(frame(), 2.0, 2.0).is_err());
        assert!(Cone::new(frame(), 0.0).is_err());
        assert!(Cone::new(frame(), FRAC_PI_2).is_err());
        assert!(Cone::new(frame(), f64::NAN).is_err());
        let mut f = frame();
        f.x = f.y;
        assert!(Plane::new(f).is_err());
        assert!(Surface::Plane(Plane::new(frame()).unwrap()).closest_point(Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
    }

    #[test]
    fn nurbs_surface_projection_finds_the_foot_of_a_perpendicular() {
        // A bilinear patch in the plane z = 0.5 x: a planar NURBS surface.
        let pts = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 4.0, 0.0), Vec3::new(4.0, 0.0, 2.0), Vec3::new(4.0, 4.0, 2.0)];
        let s =
            Surface::Nurbs(Box::new(NurbsSurface::new(1, 1, vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0], 2, 2, pts, vec![1.0; 4]).unwrap()));
        let n = Vec3::new(-0.5, 0.0, 1.0).normalized().unwrap();
        let on = s.point(0.4, 0.6);
        let pr = s.closest_point(on + n * 0.7).unwrap();
        assert!(pr.converged);
        assert!(pr.point.distance(on) < 1e-9, "{:?}", pr.point);
        assert!((pr.distance - 0.7).abs() < 1e-9);
        assert!(s.normal(0.4, 0.6).unwrap().distance(n) < 1e-12 || s.normal(0.4, 0.6).unwrap().distance(-n) < 1e-12);
    }
}
