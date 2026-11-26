// SPDX-License-Identifier: MIT OR Apache-2.0
//! Curves: line, circle, ellipse and NURBS.
//!
//! Every curve has a parametrisation `t ↦ point(t)`:
//!
//! * [`Line`]: `origin + t · direction` with a unit direction, so `t` is arc length;
//! * [`Circle`]: `origin + r (cos t · x + sin t · y)` in its frame, counter-clockwise about `z`;
//! * [`Ellipse`]: `origin + a cos t · x + b sin t · y` (`a` along the frame's `x`);
//! * [`NurbsCurve`]: its knot domain.
//!
//! Closest-point projection is closed form for the line and the circle, and a bracketed Newton
//! iteration seeded from a dense sampling for the ellipse and the NURBS curve (see
//! [`Curve::closest_point`] for what is promised).

use crate::error::{BrepError, Result};
use crate::math::{Frame, Point3, Vec3, nearest_periodic};
use crate::nurbs::NurbsCurve;
use std::f64::consts::{PI, TAU};

/// Largest radius or semi-axis accepted (mm), well beyond [`crate::tol::MAX_MODEL_EXTENT`].
pub const MAX_SIZE: f64 = 1.0e9;
/// Most samples a curve is cut into by [`Curve::sample_params`].
pub const MAX_SAMPLES: usize = 200_000;

/// An infinite straight line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub origin: Point3,
    pub direction: Vec3,
}

/// A circle in the plane of its frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle {
    pub frame: Frame,
    pub radius: f64,
}

/// An ellipse with semi-axis `major` along the frame's `x` and `minor` along its `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    pub frame: Frame,
    pub major: f64,
    pub minor: f64,
}

/// A curve of one of the supported kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve {
    Line(Line),
    Circle(Circle),
    Ellipse(Ellipse),
    Nurbs(NurbsCurve),
}

/// The result of projecting a point on a curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveProjection {
    /// The parameter of the closest point.
    pub t: f64,
    pub point: Point3,
    pub distance: f64,
    /// `false` when an iteration hit its cap and the answer is the best found, not a certified minimum.
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

impl Line {
    /// A line through `origin` along `direction` (normalised; must be non-zero).
    pub fn new(origin: Point3, direction: Vec3) -> Result<Line> {
        if !origin.is_finite() || !direction.is_finite() {
            return Err(BrepError::non_finite("line"));
        }
        let direction = direction.normalized().ok_or_else(|| BrepError::degenerate("line direction has zero length"))?;
        Ok(Line { origin, direction })
    }
    /// The line through two distinct points, and the parameter of the second one.
    pub fn through(a: Point3, b: Point3) -> Result<(Line, f64)> {
        let line = Line::new(a, b - a)?;
        Ok((line, a.distance(b)))
    }
}

impl Circle {
    pub fn new(frame: Frame, radius: f64) -> Result<Circle> {
        frame.validate()?;
        Ok(Circle { frame, radius: size(radius, "circle radius")? })
    }
}

impl Ellipse {
    pub fn new(frame: Frame, major: f64, minor: f64) -> Result<Ellipse> {
        frame.validate()?;
        Ok(Ellipse { frame, major: size(major, "ellipse major semi-axis")?, minor: size(minor, "ellipse minor semi-axis")? })
    }
}

impl Curve {
    /// The point at parameter `t`.
    pub fn point(&self, t: f64) -> Point3 {
        match self {
            Curve::Line(l) => l.origin + l.direction * t,
            Curve::Circle(c) => {
                let (s, co) = t.sin_cos();
                c.frame.origin + c.frame.x * (c.radius * co) + c.frame.y * (c.radius * s)
            }
            Curve::Ellipse(e) => {
                let (s, co) = t.sin_cos();
                e.frame.origin + e.frame.x * (e.major * co) + e.frame.y * (e.minor * s)
            }
            Curve::Nurbs(n) => n.point(t),
        }
    }

    /// The first derivative `dP/dt`.
    pub fn derivative(&self, t: f64) -> Vec3 {
        self.derivatives(t, 1).get(1).copied().unwrap_or(Vec3::ZERO)
    }

    /// `P(t), P'(t), …` up to `order` (`order + 1` vectors).
    pub fn derivatives(&self, t: f64, order: usize) -> Vec<Vec3> {
        match self {
            Curve::Line(l) => (0..=order)
                .map(|k| match k {
                    0 => l.origin + l.direction * t,
                    1 => l.direction,
                    _ => Vec3::ZERO,
                })
                .collect(),
            Curve::Circle(Circle { frame, radius }) => trig_derivatives(frame, *radius, *radius, t, order),
            Curve::Ellipse(e) => trig_derivatives(&e.frame, e.major, e.minor, t, order),
            Curve::Nurbs(n) => n.derivatives(t, order),
        }
    }

    /// The period when the curve is closed by periodicity of its parametrisation (`2π`).
    pub fn period(&self) -> Option<f64> {
        match self {
            Curve::Circle(_) | Curve::Ellipse(_) => Some(TAU),
            _ => None,
        }
    }

    /// The parameter interval the curve naturally lives on; infinite for a line.
    pub fn natural_domain(&self) -> (f64, f64) {
        match self {
            Curve::Line(_) => (f64::NEG_INFINITY, f64::INFINITY),
            Curve::Circle(_) | Curve::Ellipse(_) => (0.0, TAU),
            Curve::Nurbs(n) => n.domain(),
        }
    }

    /// The closest point of the curve to `p` among parameters in `[t0, t1]`.
    ///
    /// * Line and circle: closed form (exact to rounding).
    /// * Ellipse and NURBS: the parameter range is sampled densely, every local minimum of the
    ///   sampled distance (and both ends) is refined by a bracketed Newton iteration on
    ///   `(P(t) − p)·P'(t) = 0`, and the best is returned. This finds the global minimum unless two
    ///   distinct minima fall between the same pair of samples, which a dense sampling makes
    ///   implausible but does not exclude. The parameter is accurate to about `1e-13` of the range
    ///   (the iteration stops at that width) when `converged` is true.
    ///
    /// For a point on the axis of a circle every parameter is equally close; the start of the
    /// range is returned.
    pub fn closest_point(&self, p: Point3, t0: f64, t1: f64) -> Result<CurveProjection> {
        if !p.is_finite() {
            return Err(BrepError::non_finite("projected point"));
        }
        if !(t0.is_finite() && t1.is_finite() && t0 <= t1) {
            return Err(BrepError::invalid("parameter range must be finite with t0 <= t1"));
        }
        let make = |t: f64, converged: bool| {
            let point = self.point(t);
            CurveProjection { t, point, distance: point.distance(p), converged }
        };
        match self {
            Curve::Line(l) => Ok(make((p - l.origin).dot(l.direction).clamp(t0, t1), true)),
            Curve::Circle(c) => {
                let local = c.frame.to_local(p);
                if local.x.hypot(local.y) <= 1e-300 {
                    return Ok(make(t0, true));
                }
                Ok(make(arc_clamp(local.y.atan2(local.x), t0, t1, |t| self.point(t).distance(p)), true))
            }
            Curve::Ellipse(_) | Curve::Nurbs(_) => self.closest_numeric(p, t0, t1),
        }
    }

    fn closest_numeric(&self, p: Point3, t0: f64, t1: f64) -> Result<CurveProjection> {
        let range = t1 - t0;
        let make = |t: f64, converged: bool| {
            let point = self.point(t);
            CurveProjection { t, point, distance: point.distance(p), converged }
        };
        if range == 0.0 {
            return Ok(make(t0, true));
        }
        let full = self.period().is_some_and(|per| range >= per - 1e-12);
        let seeds: usize = match self {
            Curve::Nurbs(n) => (24 * n.control_points().len()).clamp(128, 8192),
            _ => 256,
        };
        let ts: Vec<f64> = (0..=seeds).map(|i| t0 + range * i as f64 / seeds as f64).collect();
        let d2: Vec<f64> = ts.iter().map(|&t| (self.point(t) - p).norm_sq()).collect();
        let cyclic_ts = |i: isize| -> f64 {
            // Sample i may be -1 or seeds+1 when the curve is a full period: continue periodically.
            if i < 0 {
                ts[0] - (ts[1] - ts[0])
            } else if i as usize > seeds {
                ts[seeds] + (ts[1] - ts[0])
            } else {
                ts[i as usize]
            }
        };
        let mut best = make(t0, true);
        let mut consider = |c: CurveProjection| {
            if c.distance < best.distance {
                best = c;
            }
        };
        consider(make(t1, true));
        let (mut all_converged, n) = (true, seeds as isize);
        for i in 0..=n {
            let prev = if i == 0 { if full { Some(d2[seeds - 1]) } else { None } } else { Some(d2[i as usize - 1]) };
            let next = if i == n { if full { Some(d2[1]) } else { None } } else { Some(d2[i as usize + 1]) };
            let here = d2[i as usize];
            let is_min = prev.is_none_or(|v| here <= v) && next.is_none_or(|v| here <= v);
            if !is_min {
                continue;
            }
            let lo = if i == 0 && !full { ts[0] } else { cyclic_ts(i - 1) };
            let hi = if i == n && !full { ts[seeds] } else { cyclic_ts(i + 1) };
            match self.refine_minimum(p, lo, hi) {
                Some((t, ok)) => {
                    all_converged &= ok;
                    // Wrap into the requested range for a full period; otherwise clamp.
                    let t = if full { t0 + (t - t0).rem_euclid(range) } else { t.clamp(t0, t1) };
                    consider(make(t, ok));
                }
                None => consider(make(ts[i as usize], true)),
            }
        }
        let mut out = best;
        out.converged = out.converged && all_converged;
        Ok(out)
    }

    /// Refine a minimum of the distance to `p` known to lie in `[lo, hi]` (a bracket of the sampled
    /// minimum). Returns `(t, converged)`, or `None` when the derivative does not change sign.
    fn refine_minimum(&self, p: Point3, mut lo: f64, mut hi: f64) -> Option<(f64, bool)> {
        let f = |t: f64| -> (f64, f64) {
            let d = self.derivatives(t, 2);
            let (c, c1, c2) = (d[0], d[1], d[2]);
            let r = c - p;
            (r.dot(c1), c1.norm_sq() + r.dot(c2))
        };
        let (flo, _) = f(lo);
        let (fhi, _) = f(hi);
        if !(flo <= 0.0 && fhi >= 0.0) {
            return None;
        }
        let width0 = hi - lo;
        let mut t = 0.5 * (lo + hi);
        for _ in 0..200 {
            let (ft, dft) = f(t);
            if ft < 0.0 {
                lo = t;
            } else if ft > 0.0 {
                hi = t;
            } else {
                return Some((t, true));
            }
            let newton = t - ft / dft;
            t = if dft > 0.0 && newton.is_finite() && newton > lo && newton < hi { newton } else { 0.5 * (lo + hi) };
            if hi - lo <= 1e-13 * width0.max(1e-300) || hi - lo <= f64::EPSILON * t.abs() {
                return Some((t, true));
            }
        }
        Some((t, false))
    }

    /// Parameters `t0 = s₀ < s₁ < … < sₙ = t1` such that the polyline through the curve points is
    /// within `chord` (mm) of the curve (the sagitta of every segment is at most `chord`, checked at
    /// segment midpoints for ellipses and NURBS curves). Circles and ellipses also never turn more
    /// than `π/4` per segment, and a line is one segment.
    ///
    /// Fails if the curve would need more than [`MAX_SAMPLES`] segments.
    pub fn sample_params(&self, t0: f64, t1: f64, chord: f64) -> Result<Vec<f64>> {
        if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
            return Err(BrepError::invalid("sampling range must be finite with t0 < t1"));
        }
        if !(chord.is_finite() && chord > 0.0) {
            return Err(BrepError::invalid("chord tolerance must be positive and finite"));
        }
        match self {
            Curve::Line(_) => Ok(vec![t0, t1]),
            Curve::Circle(c) => {
                let step = if chord >= c.radius { PI / 4.0 } else { (2.0 * (1.0 - chord / c.radius).acos()).min(PI / 4.0) };
                uniform(t0, t1, step)
            }
            Curve::Ellipse(e) => {
                // The sagitta of a segment of angular width Δ is at most a·(1 − cos(Δ/2)) with a the major semi-axis.
                let step = if chord >= e.major { PI / 4.0 } else { (2.0 * (1.0 - chord / e.major).acos()).min(PI / 4.0) };
                uniform(t0, t1, step)
            }
            Curve::Nurbs(n) => {
                let mut breaks: Vec<f64> = vec![t0];
                breaks.extend(n.knots().iter().copied().filter(|&k| k > t0 && k < t1));
                breaks.dedup();
                breaks.push(t1);
                let mut out = vec![t0];
                let per_span = 2 * (n.degree() + 1);
                for w in breaks.windows(2) {
                    for i in 1..=per_span {
                        out.push(w[0] + (w[1] - w[0]) * i as f64 / per_span as f64);
                    }
                }
                // Bisect any segment whose midpoint is farther than `chord` from its chord.
                let mut result = Vec::with_capacity(out.len());
                let mut stack: Vec<(f64, f64, u32)> = out.windows(2).rev().map(|w| (w[0], w[1], 0)).collect();
                result.push(t0);
                while let Some((a, b, depth)) = stack.pop() {
                    let (pa, pb, pm) = (self.point(a), self.point(b), self.point(0.5 * (a + b)));
                    let seg = pb - pa;
                    let len2 = seg.norm_sq();
                    let off = if len2 > 0.0 { (pm - pa - seg * ((pm - pa).dot(seg) / len2)).norm() } else { pm.distance(pa) };
                    if off > chord && depth < 24 {
                        let m = 0.5 * (a + b);
                        stack.push((m, b, depth + 1));
                        stack.push((a, m, depth + 1));
                    } else {
                        result.push(b);
                    }
                    if result.len() + stack.len() > MAX_SAMPLES {
                        return Err(BrepError::OutOfRange(format!("curve needs more than {MAX_SAMPLES} segments at chord tolerance {chord}")));
                    }
                }
                Ok(result)
            }
        }
    }
}

fn uniform(t0: f64, t1: f64, max_step: f64) -> Result<Vec<f64>> {
    let n = ((t1 - t0) / max_step).ceil();
    if !(n.is_finite() && n >= 1.0 && n <= MAX_SAMPLES as f64) {
        return Err(BrepError::OutOfRange(format!("curve needs more than {MAX_SAMPLES} segments")));
    }
    let n = n as usize;
    Ok((0..=n).map(|i| if i == n { t1 } else { t0 + (t1 - t0) * i as f64 / n as f64 }).collect())
}

/// The point of the angular interval `[t0, t1]` closest to the angle `theta` (given mod 2π),
/// by the metric `dist` used only to choose between the two ends.
fn arc_clamp(theta: f64, t0: f64, t1: f64, dist: impl Fn(f64) -> f64) -> f64 {
    if t1 - t0 >= TAU - 1e-12 {
        return t0 + (theta - t0).rem_euclid(TAU);
    }
    let mid = 0.5 * (t0 + t1);
    let t = nearest_periodic(theta, mid, TAU);
    if t >= t0 && t <= t1 {
        t
    } else if dist(t0) <= dist(t1) {
        t0
    } else {
        t1
    }
}

fn trig_derivatives(frame: &Frame, a: f64, b: f64, t: f64, order: usize) -> Vec<Vec3> {
    let (s, c) = t.sin_cos();
    (0..=order)
        .map(|k| {
            // d^k/dt^k of (a cos t, b sin t).
            let (dc, ds) = match k % 4 {
                0 => (c, s),
                1 => (-s, c),
                2 => (-c, -s),
                _ => (s, -c),
            };
            let base = if k == 0 { frame.origin } else { Vec3::ZERO };
            base + frame.x * (a * dc) + frame.y * (b * ds)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::NurbsCurve;
    use std::f64::consts::FRAC_PI_2;

    fn circle(r: f64) -> Curve {
        Curve::Circle(Circle::new(Frame::WORLD, r).unwrap())
    }
    fn ellipse(a: f64, b: f64) -> Curve {
        Curve::Ellipse(Ellipse::new(Frame::WORLD, a, b).unwrap())
    }
    fn bspline() -> Curve {
        Curve::Nurbs(
            NurbsCurve::clamped_uniform(
                3,
                vec![
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(2.0, 3.0, 0.0),
                    Vec3::new(4.0, -2.0, 1.0),
                    Vec3::new(6.0, 2.0, 0.0),
                    Vec3::new(8.0, 0.0, 2.0),
                ],
            )
            .unwrap(),
        )
    }

    #[test]
    fn constructors_reject_bad_input() {
        assert!(Line::new(Vec3::ZERO, Vec3::ZERO).is_err());
        assert!(Line::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X).is_err());
        assert!(Circle::new(Frame::WORLD, 0.0).is_err());
        assert!(Circle::new(Frame::WORLD, -1.0).is_err());
        assert!(Circle::new(Frame::WORLD, f64::INFINITY).is_err());
        assert!(Circle::new(Frame::WORLD, 1e12).is_err());
        assert!(Ellipse::new(Frame::WORLD, 2.0, 0.0).is_err());
        let mut f = Frame::WORLD;
        f.y = Vec3::X;
        assert!(Circle::new(f, 1.0).is_err());
    }

    #[test]
    fn analytic_derivatives_match_finite_differences() {
        for c in [circle(3.0), ellipse(5.0, 2.0), bspline()] {
            let (t0, t1) = c.natural_domain();
            for i in 1..10 {
                let t = t0 + (t1 - t0) * i as f64 / 10.0;
                let h = 1e-5;
                let d = c.derivatives(t, 2);
                let fd1 = (c.point(t + h) - c.point(t - h)) / (2.0 * h);
                let fd2 = (c.point(t + h) - c.point(t) * 2.0 + c.point(t - h)) / (h * h);
                assert!(d[1].distance(fd1) < 1e-6 * d[1].norm().max(1.0), "{c:?}");
                assert!(d[2].distance(fd2) < 1e-3 * d[2].norm().max(1.0), "{c:?}");
            }
        }
    }

    #[test]
    fn line_projection_clamps_to_the_range() {
        let (l, len) = Line::through(Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, 5.0)).unwrap();
        assert!((len - 4.0).abs() < 1e-15);
        let c = Curve::Line(l);
        let pr = c.closest_point(Vec3::new(3.0, 1.0, 3.0), 0.0, len).unwrap();
        assert!((pr.t - 2.0).abs() < 1e-14 && (pr.distance - 2.0).abs() < 1e-14);
        assert_eq!(c.closest_point(Vec3::new(1.0, 1.0, 100.0), 0.0, len).unwrap().t, len);
        assert_eq!(c.closest_point(Vec3::new(1.0, 1.0, -100.0), 0.0, len).unwrap().t, 0.0);
    }

    #[test]
    fn circle_projection_for_full_and_partial_arcs() {
        let c = circle(2.0);
        let p = Vec3::new(0.0, 5.0, 3.0);
        let pr = c.closest_point(p, 0.0, TAU).unwrap();
        assert!((pr.t - FRAC_PI_2).abs() < 1e-14);
        assert!((pr.distance - 10f64.sqrt() * 1.0).abs() < 1e-12 || (pr.distance - (9.0 + 9.0f64).sqrt()).abs() < 1e-12);
        // A point at angle 300° with the arc restricted to [0°, 90°]: the nearer end is 0°.
        let p = Vec3::new(5.0 * 300f64.to_radians().cos(), 5.0 * 300f64.to_radians().sin(), 0.0);
        assert_eq!(c.closest_point(p, 0.0, FRAC_PI_2).unwrap().t, 0.0);
        // Full circle wraps into the requested range.
        let pr = c.closest_point(Vec3::new(1.0, -1.0, 0.0), 0.0, TAU).unwrap();
        assert!((pr.t - 7.0 * PI / 4.0).abs() < 1e-14);
        // On the axis: any answer is equally close; no failure.
        let pr = c.closest_point(Vec3::new(0.0, 0.0, 4.0), 1.0, 2.0).unwrap();
        assert_eq!(pr.t, 1.0);
        assert!((pr.distance - (4.0f64 * 4.0 + 4.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn ellipse_projection_satisfies_the_first_order_condition_and_beats_a_brute_force_scan() {
        let e = ellipse(5.0, 2.0);
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let mut r = || {
            s ^= s >> 12;
            s ^= s << 25;
            s ^= s >> 27;
            (s.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..300 {
            let p = Vec3::new(r() * 16.0 - 8.0, r() * 10.0 - 5.0, r() * 4.0 - 2.0);
            let pr = e.closest_point(p, 0.0, TAU).unwrap();
            assert!(pr.converged);
            let brute = (0..20000).map(|i| e.point(TAU * i as f64 / 20000.0).distance(p)).fold(f64::INFINITY, f64::min);
            assert!(pr.distance <= brute + 1e-12, "p={p:?} {} vs {brute}", pr.distance);
            let d1 = e.derivative(pr.t);
            assert!((pr.point - p).dot(d1).abs() < 1e-9 * d1.norm().max(1.0) * (pr.point - p).norm().max(1.0), "first order condition");
        }
    }

    #[test]
    fn ellipse_with_a_point_at_the_center_has_two_equal_minima_handled() {
        let e = ellipse(5.0, 2.0);
        let pr = e.closest_point(Vec3::ZERO, 0.0, TAU).unwrap();
        assert!((pr.distance - 2.0).abs() < 1e-9);
    }

    #[test]
    fn nurbs_projection_matches_brute_force() {
        let c = bspline();
        let (t0, t1) = c.natural_domain();
        let mut k = 12345u64;
        for _ in 0..100 {
            k = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let x = (k >> 33) as f64 / (1u64 << 31) as f64 * 10.0 - 1.0;
            k = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let y = (k >> 33) as f64 / (1u64 << 31) as f64 * 8.0 - 4.0;
            let p = Vec3::new(x, y, 0.5);
            let pr = c.closest_point(p, t0, t1).unwrap();
            let brute = (0..=40000).map(|i| c.point(t0 + (t1 - t0) * i as f64 / 40000.0).distance(p)).fold(f64::INFINITY, f64::min);
            assert!(pr.distance <= brute + 1e-12, "{} vs {brute}", pr.distance);
            assert!(pr.distance >= brute - 1e-3);
        }
    }

    #[test]
    fn projection_rejects_bad_input() {
        let c = circle(1.0);
        assert!(c.closest_point(Vec3::new(f64::NAN, 0.0, 0.0), 0.0, 1.0).is_err());
        assert!(c.closest_point(Vec3::ZERO, 2.0, 1.0).is_err());
        assert!(c.closest_point(Vec3::ZERO, f64::NAN, 1.0).is_err());
    }

    #[test]
    fn sample_params_meets_the_chord_tolerance() {
        for (c, name) in [(circle(10.0), "circle"), (ellipse(10.0, 3.0), "ellipse"), (bspline(), "nurbs")] {
            let (t0, t1) = c.natural_domain();
            let chord = 0.01;
            let ts = c.sample_params(t0, t1, chord).unwrap();
            assert_eq!(ts.first(), Some(&t0));
            assert_eq!(ts.last(), Some(&t1));
            assert!(ts.windows(2).all(|w| w[0] < w[1]));
            for w in ts.windows(2) {
                let (a, b) = (c.point(w[0]), c.point(w[1]));
                // Deviation of several interior points from the chord.
                for j in 1..8 {
                    let t = w[0] + (w[1] - w[0]) * j as f64 / 8.0;
                    let seg = b - a;
                    let p = c.point(t) - a;
                    let off = (p - seg * (p.dot(seg) / seg.norm_sq())).norm();
                    assert!(off <= chord * 1.05, "{name}: {off}");
                }
            }
        }
        assert_eq!(Curve::Line(Line::new(Vec3::ZERO, Vec3::X).unwrap()).sample_params(0.0, 5.0, 0.1).unwrap(), vec![0.0, 5.0]);
    }

    #[test]
    fn sample_params_refuses_absurd_requests() {
        assert!(circle(1e8).sample_params(0.0, TAU, 1e-9).is_err());
        assert!(circle(1.0).sample_params(0.0, TAU, 0.0).is_err());
        assert!(circle(1.0).sample_params(1.0, 1.0, 0.1).is_err());
        assert!(circle(1.0).sample_params(0.0, f64::NAN, 0.1).is_err());
        // Even a huge chord keeps each circle segment under a quarter... eighth turn.
        let ts = circle(1.0).sample_params(0.0, TAU, 100.0).unwrap();
        assert_eq!(ts.len(), 9);
    }
}
