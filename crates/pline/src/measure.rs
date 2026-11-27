// SPDX-License-Identifier: MIT OR Apache-2.0
//! Area, extents and winding number of a [`Pline`].

use crate::Pline;
use crate::geom::{P, Seg};

/// Axis-aligned bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extents {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Pline {
    /// The segments, closing back to the first vertex.
    pub(crate) fn segments(&self) -> Vec<Seg> {
        let n = self.vertices().len();
        let mut out = Vec::with_capacity(n);
        for (i, v) in self.vertices().iter().enumerate() {
            if let Some(w) = self.vertices().get((i + 1) % n) {
                out.push(Seg { a: [v.x, v.y], b: [w.x, w.y], bulge: v.bulge });
            }
        }
        out
    }

    /// Signed area: positive when the polyline runs counter-clockwise.
    pub fn area(&self) -> f64 {
        if self.vertices().len() < 2 {
            return 0.0;
        }
        let mut twice = 0.0;
        let mut segs = 0.0;
        for s in self.segments() {
            twice += s.a[0] * s.b[1] - s.b[0] * s.a[1];
            if let Some(arc) = s.arc() {
                // The circular segment between the chord and the arc.
                segs += 0.5 * arc.r * arc.r * (arc.sweep - arc.sweep.sin());
            }
        }
        twice / 2.0 + segs
    }

    /// The bounds of the outline, or `None` when empty.
    pub fn extents(&self) -> Option<Extents> {
        let segs = self.segments();
        let mut b = segs.first()?.bounds();
        for s in &segs {
            let q = s.bounds();
            b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[2]), b[3].max(q[3])];
        }
        Some(Extents { min_x: b[0], min_y: b[1], max_x: b[2], max_y: b[3] })
    }

    /// How many times the outline winds counter-clockwise around the point (negative for
    /// clockwise); 0 outside. A point on the outline gives an unspecified answer.
    pub fn winding_number(&self, x: f64, y: f64) -> i32 {
        let mut w = 0;
        for s in self.segments() {
            w += crossings(&s, [x, y]);
        }
        w
    }
}

/// Signed crossings of the ray going right from `p` with one segment, upward positive.
fn crossings(s: &Seg, p: P) -> i32 {
    let Some(arc) = s.arc() else {
        return line_crossing(s.a, s.b, p);
    };
    // Split the arc at its extreme-y angles so each part is monotone in y.
    let mut cuts = vec![0.0, 1.0];
    for k in 0..2 {
        let ang = std::f64::consts::FRAC_PI_2 + k as f64 * std::f64::consts::PI;
        if let Some(f) = arc.param_of_angle(ang)
            && f > 0.0
            && f < 1.0
        {
            cuts.push(f);
        }
    }
    cuts.sort_by(f64::total_cmp);
    let mut w = 0;
    for pair in cuts.windows(2) {
        let (f0, f1) = (pair[0], pair[1]);
        // The segment's own ends, so neighbouring segments agree on a shared vertex's height.
        let pt = |f: f64| {
            if f == 0.0 {
                s.a
            } else if f == 1.0 {
                s.b
            } else {
                arc.at(f)
            }
        };
        let (q0, q1) = (pt(f0), pt(f1));
        let (lo, hi) = (q0[1].min(q1[1]), q0[1].max(q1[1]));
        // Half-open in y, so a ray through a vertex counts once.
        if !(p[1] >= lo && p[1] < hi) {
            continue;
        }
        let dy = p[1] - arc.c[1];
        let half = (arc.r * arc.r - dy * dy).max(0.0).sqrt();
        let mid_ang = arc.start + arc.sweep * (f0 + f1) / 2.0;
        let x = if mid_ang.cos() >= 0.0 { arc.c[0] + half } else { arc.c[0] - half };
        if x > p[0] {
            w += if q1[1] > q0[1] { 1 } else { -1 };
        }
    }
    w
}

fn line_crossing(a: P, b: P, p: P) -> i32 {
    let (lo, hi) = (a[1].min(b[1]), a[1].max(b[1]));
    if !(p[1] >= lo && p[1] < hi) {
        return 0;
    }
    let t = (p[1] - a[1]) / (b[1] - a[1]);
    let x = a[0] + t * (b[0] - a[0]);
    if x > p[0] { if b[1] > a[1] { 1 } else { -1 } } else { 0 }
}
