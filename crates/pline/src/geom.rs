// SPDX-License-Identifier: MIT OR Apache-2.0
//! Segments (a line or an arc), and where two of them meet.

use std::f64::consts::{PI, TAU};

pub(crate) type P = [f64; 2];

/// Bulges smaller than this are straight lines.
const LINE_BULGE: f64 = 1e-12;

/// One segment, from `a` to `b`, with the bulge of the vertex it leaves.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seg {
    pub a: P,
    pub b: P,
    pub bulge: f64,
}

/// The circle and sweep of an arc segment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arc {
    pub c: P,
    pub r: f64,
    /// Angle of the start point.
    pub start: f64,
    /// Signed sweep, counter-clockwise positive, `|sweep| < 2π`.
    pub sweep: f64,
}

pub(crate) fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

pub(crate) fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

pub(crate) fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

pub(crate) fn dist(a: P, b: P) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl Seg {
    pub fn is_arc(&self) -> bool {
        self.bulge.abs() > LINE_BULGE
    }

    pub fn arc(&self) -> Option<Arc> {
        if !self.is_arc() {
            return None;
        }
        let (dx, dy) = (self.b[0] - self.a[0], self.b[1] - self.a[1]);
        if dx == 0.0 && dy == 0.0 {
            return None;
        }
        // From the chord's midpoint to the centre, along the chord's left normal.
        let s = (1.0 - self.bulge * self.bulge) / (4.0 * self.bulge);
        let c = [(self.a[0] + self.b[0]) / 2.0 - dy * s, (self.a[1] + self.b[1]) / 2.0 + dx * s];
        let r = dist(c, self.a);
        if !(r.is_finite() && c[0].is_finite() && c[1].is_finite()) {
            return None;
        }
        Some(Arc { c, r, start: (self.a[1] - c[1]).atan2(self.a[0] - c[0]), sweep: 4.0 * self.bulge.atan() })
    }

    /// The point a fraction `f` of the way along the segment (by length for a line, by angle for
    /// an arc).
    pub fn at(&self, f: f64) -> P {
        match self.arc() {
            Some(arc) => arc.at(f),
            None => [self.a[0] + (self.b[0] - self.a[0]) * f, self.a[1] + (self.b[1] - self.a[1]) * f],
        }
    }

    /// The direction of travel at the fraction `f` (not normalised).
    pub fn tangent(&self, f: f64) -> P {
        match self.arc() {
            Some(arc) => {
                let ang = arc.start + arc.sweep * f;
                let dir = arc.sweep.signum();
                [-ang.sin() * dir, ang.cos() * dir]
            }
            None => sub(self.b, self.a),
        }
    }

    pub fn reversed(&self) -> Seg {
        Seg { a: self.b, b: self.a, bulge: -self.bulge }
    }

    /// The fraction of the segment at which `p` (a point on it, within tolerance) lies.
    pub fn param(&self, p: P) -> f64 {
        match self.arc() {
            Some(arc) => arc.param(p).unwrap_or_else(|| if dist(p, self.a) < dist(p, self.b) { 0.0 } else { 1.0 }),
            None => {
                let d = sub(self.b, self.a);
                let l2 = dot(d, d);
                if l2 == 0.0 { 0.0 } else { (dot(sub(p, self.a), d) / l2).clamp(0.0, 1.0) }
            }
        }
    }

    /// The part of the segment between fractions `f0 < f1`, from `p` to `q`.
    pub fn piece(&self, p: P, q: P, f0: f64, f1: f64) -> Seg {
        let bulge = if self.is_arc() { ((f1 - f0) * 4.0 * self.bulge.atan() / 4.0).tan() } else { 0.0 };
        Seg { a: p, b: q, bulge }
    }

    /// Distance from `p` to the segment, and the fraction of the nearest point.
    pub fn nearest(&self, p: P) -> (f64, f64) {
        match self.arc() {
            Some(arc) => {
                if let Some(f) = arc.param(p)
                    && dist(p, arc.c) > 1e-300
                {
                    let q = arc.at(f);
                    return ((dist(p, arc.c) - arc.r).abs().min(dist(p, q)), f);
                }
                let (da, db) = (dist(p, self.a), dist(p, self.b));
                if da <= db { (da, 0.0) } else { (db, 1.0) }
            }
            None => {
                let d = sub(self.b, self.a);
                let l2 = dot(d, d);
                let f = if l2 == 0.0 { 0.0 } else { (dot(sub(p, self.a), d) / l2).clamp(0.0, 1.0) };
                let q = self.at(f);
                (dist(p, q), f)
            }
        }
    }

    /// Axis-aligned bounds `(min_x, min_y, max_x, max_y)`.
    pub fn bounds(&self) -> [f64; 4] {
        let mut b = [self.a[0].min(self.b[0]), self.a[1].min(self.b[1]), self.a[0].max(self.b[0]), self.a[1].max(self.b[1])];
        if let Some(arc) = self.arc() {
            for k in 0..4 {
                let ang = k as f64 * PI / 2.0;
                if arc.param_of_angle(ang).is_some() {
                    let q = [arc.c[0] + arc.r * ang.cos(), arc.c[1] + arc.r * ang.sin()];
                    b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
                }
            }
        }
        b
    }
}

impl Arc {
    pub fn at(&self, f: f64) -> P {
        let ang = self.start + self.sweep * f;
        [self.c[0] + self.r * ang.cos(), self.c[1] + self.r * ang.sin()]
    }

    /// Fraction along the arc of the angle `ang`, when it lies on the sweep.
    pub fn param_of_angle(&self, ang: f64) -> Option<f64> {
        let dir = self.sweep.signum();
        let d = ((ang - self.start) * dir).rem_euclid(TAU);
        let total = self.sweep.abs();
        (d <= total).then_some(d / total)
    }

    /// Fraction along the arc of the point `p`, when its angle lies on the sweep (within a
    /// tolerance expressed as a length).
    pub fn param(&self, p: P) -> Option<f64> {
        let ang = (p[1] - self.c[1]).atan2(p[0] - self.c[0]);
        let dir = self.sweep.signum();
        let total = self.sweep.abs();
        let mut d = ((ang - self.start) * dir).rem_euclid(TAU);
        // Just before the start counts as the start.
        if TAU - d < 1e-9 {
            d = 0.0;
        }
        (d <= total + 1e-9).then_some((d / total).min(1.0))
    }
}

/// Points where two segments meet; for segments that run along each other, the ends of the
/// stretch they share. `tol` is the length below which points are the same.
pub(crate) fn intersect(s: &Seg, t: &Seg, tol: f64) -> Vec<P> {
    match (s.arc(), t.arc()) {
        (None, None) => line_line(s, t, tol),
        (None, Some(arc)) => line_arc(s, &arc, t, tol),
        (Some(arc), None) => line_arc(t, &arc, s, tol),
        (Some(x), Some(y)) => arc_arc(s, &x, t, &y, tol),
    }
}

fn line_line(s: &Seg, t: &Seg, tol: f64) -> Vec<P> {
    let r = sub(s.b, s.a);
    let u = sub(t.b, t.a);
    let (lr, lu) = (r[0].hypot(r[1]), u[0].hypot(u[1]));
    if lr == 0.0 || lu == 0.0 {
        return Vec::new();
    }
    let qp = sub(t.a, s.a);
    let denom = cross(r, u);
    if denom.abs() <= 1e-12 * lr * lu {
        // Parallel: they share a stretch only when collinear.
        if (cross(qp, r) / lr).abs() > tol {
            return Vec::new();
        }
        let (t0, t1) = (dot(qp, r) / (lr * lr), dot(sub(t.b, s.a), r) / (lr * lr));
        let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
        let e = tol / lr;
        if lo > hi + e {
            return Vec::new();
        }
        let (lo, hi) = (lo.min(hi), hi.max(lo));
        return vec![s.at(lo), s.at(hi)];
    }
    let ts = cross(qp, u) / denom;
    let us = cross(qp, r) / denom;
    let (es, eu) = (tol / lr, tol / lu);
    if ts < -es || ts > 1.0 + es || us < -eu || us > 1.0 + eu {
        return Vec::new();
    }
    vec![s.at(ts.clamp(0.0, 1.0))]
}

fn line_arc(line: &Seg, arc: &Arc, arc_seg: &Seg, tol: f64) -> Vec<P> {
    let d = sub(line.b, line.a);
    let l = d[0].hypot(d[1]);
    if l == 0.0 {
        return Vec::new();
    }
    let t0 = dot(sub(arc.c, line.a), d) / (l * l);
    let h = (cross(sub(arc.c, line.a), d) / l).abs();
    if h > arc.r + tol {
        return Vec::new();
    }
    let ts = if (h - arc.r).abs() <= tol {
        vec![t0]
    } else {
        let half = (arc.r * arc.r - h * h).max(0.0).sqrt() / l;
        vec![t0 - half, t0 + half]
    };
    let e = tol / l;
    let mut out = Vec::new();
    for t in ts {
        if t < -e || t > 1.0 + e {
            continue;
        }
        let p = line.at(t.clamp(0.0, 1.0));
        if on_arc(arc, arc_seg, p, tol) {
            out.push(p);
        }
    }
    out
}

/// Whether `p`, a point of the arc's circle, lies on the arc itself (within `tol` of it).
fn on_arc(arc: &Arc, seg: &Seg, p: P, tol: f64) -> bool {
    arc.param(p).is_some() || dist(p, seg.a) <= tol || dist(p, seg.b) <= tol
}

fn arc_arc(s: &Seg, x: &Arc, t: &Seg, y: &Arc, tol: f64) -> Vec<P> {
    let d = dist(x.c, y.c);
    if d <= tol && (x.r - y.r).abs() <= tol {
        // The same circle: they share the stretch between the ends that lie on both.
        let mut out = Vec::new();
        for p in [s.a, s.b] {
            if on_arc(y, t, p, tol) {
                out.push(p);
            }
        }
        for p in [t.a, t.b] {
            if on_arc(x, s, p, tol) {
                out.push(p);
            }
        }
        return out;
    }
    if d <= tol || d > x.r + y.r + tol || d < (x.r - y.r).abs() - tol {
        return Vec::new();
    }
    let a = (x.r * x.r - y.r * y.r + d * d) / (2.0 * d);
    let h2 = x.r * x.r - a * a;
    let h = if h2 <= 0.0 { 0.0 } else { h2.sqrt() };
    let u = [(y.c[0] - x.c[0]) / d, (y.c[1] - x.c[1]) / d];
    let m = [x.c[0] + u[0] * a, x.c[1] + u[1] * a];
    let mut cands = vec![[m[0] - u[1] * h, m[1] + u[0] * h]];
    if h > tol * 0.5 {
        cands.push([m[0] + u[1] * h, m[1] - u[0] * h]);
    }
    cands.into_iter().filter(|p| on_arc(x, s, *p, tol) && on_arc(y, t, *p, tol)).collect()
}
