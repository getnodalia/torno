// SPDX-License-Identifier: MIT OR Apache-2.0
//! Boolean operations on two closed polylines.
//!
//! Both outlines are cut where they meet, every piece is classified against the other outline
//! (inside, outside, or along it in the same or the opposite direction) by its midpoint, the
//! pieces the operation keeps are chained into loops, and loops are told apart by their
//! direction: counter-clockwise loops are material (`pos`), clockwise ones are holes (`neg`).

use std::collections::HashMap;

use crate::Pline;
use crate::geom::{P, Seg, cross, dist, dot, intersect};

/// Most vertices either polyline may have.
const MAX_VERTICES: usize = 200_000;
/// Most segment pairs examined.
const MAX_PAIRS: u64 = 400_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    /// Union.
    Or,
    /// Intersection.
    And,
    /// The first less the second.
    Not,
}

/// How the two polylines relate, whatever the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanInfo {
    /// Fewer than two vertices, not finite, no area, too large, or loops that would not close.
    InvalidInput,
    /// Apart, or touching only at points.
    Disjoint,
    Pline1InsidePline2,
    Pline2InsidePline1,
    /// The same outline.
    Overlapping,
    /// They cross or share stretches of outline; the loops are in the result.
    Intersected,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BooleanResult {
    pub info: BooleanInfo,
    /// Loops of material, counter-clockwise.
    pub pos: Vec<Pline>,
    /// Holes, clockwise.
    pub neg: Vec<Pline>,
}

impl BooleanResult {
    fn only(info: BooleanInfo) -> BooleanResult {
        BooleanResult { info, pos: Vec::new(), neg: Vec::new() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Loc {
    Inside,
    Outside,
    Same,
    Opposite,
}

impl Pline {
    /// Combines two closed polylines. The inputs may run in either direction.
    pub fn boolean(&self, other: &Pline, op: BooleanOp) -> BooleanResult {
        let Some((a, b, tol)) = prepare(self, other) else {
            return BooleanResult::only(BooleanInfo::InvalidInput);
        };
        let (sa, sb) = (a.segments(), b.segments());
        if (sa.len() as u64) * (sb.len() as u64) > MAX_PAIRS {
            return BooleanResult::only(BooleanInfo::InvalidInput);
        }
        // Where the outlines meet.
        let (mut ia, mut ib) = (vec![Vec::<P>::new(); sa.len()], vec![Vec::<P>::new(); sb.len()]);
        let ba: Vec<[f64; 4]> = sa.iter().map(Seg::bounds).collect();
        let bb: Vec<[f64; 4]> = sb.iter().map(Seg::bounds).collect();
        for (i, s) in sa.iter().enumerate() {
            for (j, t) in sb.iter().enumerate() {
                if !boxes_meet(&ba[i], &bb[j], tol) {
                    continue;
                }
                for p in intersect(s, t, tol) {
                    ia[i].push(p);
                    ib[j].push(p);
                }
            }
        }
        let pa: Vec<Seg> = sa.iter().zip(&ia).flat_map(|(s, pts)| split(s, pts, tol)).collect();
        let pb: Vec<Seg> = sb.iter().zip(&ib).flat_map(|(s, pts)| split(s, pts, tol)).collect();
        let la: Vec<Loc> = pa.iter().map(|s| locate(s, &sb, &b, tol)).collect();
        let lb: Vec<Loc> = pb.iter().map(|s| locate(s, &sa, &a, tol)).collect();
        let count = |l: &[Loc], k: Loc| l.iter().filter(|x| **x == k).count();
        let (a_in, a_out, a_same, a_opp) = (count(&la, Loc::Inside), count(&la, Loc::Outside), count(&la, Loc::Same), count(&la, Loc::Opposite));
        let (b_in, b_out) = (count(&lb, Loc::Inside), count(&lb, Loc::Outside));
        if a_in == 0 && a_out == 0 && b_in == 0 && b_out == 0 {
            return BooleanResult::only(BooleanInfo::Overlapping);
        }
        // Stretches the outlines share make the result a new outline (a notch, a merge), so
        // inside, outside and apart are told only when they have no stretch in common.
        if a_same == 0 && a_opp == 0 {
            if a_in == 0 && b_out == 0 && b_in > 0 {
                return BooleanResult::only(BooleanInfo::Pline2InsidePline1);
            }
            if b_in == 0 && a_out == 0 && a_in > 0 {
                return BooleanResult::only(BooleanInfo::Pline1InsidePline2);
            }
            if a_in == 0 && b_in == 0 {
                return BooleanResult::only(BooleanInfo::Disjoint);
            }
        }
        let mut keep: Vec<Seg> = Vec::new();
        for (s, l) in pa.iter().zip(&la) {
            let k = match op {
                BooleanOp::Or => matches!(l, Loc::Outside | Loc::Same),
                BooleanOp::And => matches!(l, Loc::Inside | Loc::Same),
                BooleanOp::Not => matches!(l, Loc::Outside | Loc::Opposite),
            };
            if k {
                keep.push(*s);
            }
        }
        for (s, l) in pb.iter().zip(&lb) {
            match op {
                BooleanOp::Or if *l == Loc::Outside => keep.push(*s),
                BooleanOp::And if *l == Loc::Inside => keep.push(*s),
                BooleanOp::Not if *l == Loc::Inside => keep.push(s.reversed()),
                _ => {}
            }
        }
        let Some(loops) = chain(keep, tol) else {
            return BooleanResult::only(BooleanInfo::InvalidInput);
        };
        let mut out = BooleanResult::only(BooleanInfo::Intersected);
        for l in loops {
            let area = l.area();
            if area.abs() <= tol * tol {
                continue;
            }
            if area > 0.0 { out.pos.push(l) } else { out.neg.push(l) }
        }
        out
    }
}

/// Both inputs checked, cleaned and running counter-clockwise, with the tolerance to use.
fn prepare(x: &Pline, y: &Pline) -> Option<(Pline, Pline, f64)> {
    if x.vertex_count() < 2 || y.vertex_count() < 2 || x.vertex_count() > MAX_VERTICES || y.vertex_count() > MAX_VERTICES {
        return None;
    }
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let (ex, ey) = (x.extents()?, y.extents()?);
    let lo = [ex.min_x.min(ey.min_x), ex.min_y.min(ey.min_y)];
    let hi = [ex.max_x.max(ey.max_x), ex.max_y.max(ey.max_y)];
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let mag = lo.iter().chain(hi.iter()).fold(0.0_f64, |m, v| m.max(v.abs()));
    let tol = size.max(mag) * 1e-9;
    if !(tol.is_finite() && tol > 0.0) {
        return None;
    }
    let tidy = |p: &Pline| {
        let mut q = p.remove_repeat_pos(tol).unwrap_or_else(|| p.clone());
        if q.vertex_count() < 2 || !(q.area().abs() > tol * tol) {
            return None;
        }
        if q.area() < 0.0 {
            q.invert_direction_mut();
        }
        Some(q)
    };
    Some((tidy(x)?, tidy(y)?, tol))
}

fn boxes_meet(a: &[f64; 4], b: &[f64; 4], tol: f64) -> bool {
    a[0] <= b[2] + tol && b[0] <= a[2] + tol && a[1] <= b[3] + tol && b[1] <= a[3] + tol
}

/// A segment cut at the given points (those at its ends are ignored).
fn split(s: &Seg, pts: &[P], tol: f64) -> Vec<Seg> {
    let mut v: Vec<(f64, P)> = pts.iter().filter(|p| dist(**p, s.a) > tol && dist(**p, s.b) > tol).map(|p| (s.param(*p), *p)).collect();
    v.sort_by(|x, y| x.0.total_cmp(&y.0));
    v.dedup_by(|x, y| dist(x.1, y.1) <= tol);
    let mut out = Vec::with_capacity(v.len() + 1);
    let mut prev = (0.0, s.a);
    for (f, p) in v {
        if f > prev.0 {
            out.push(s.piece(prev.1, p, prev.0, f));
            prev = (f, p);
        }
    }
    out.push(s.piece(prev.1, s.b, prev.0, 1.0));
    out
}

/// Where a piece lies with respect to the other outline.
fn locate(piece: &Seg, other: &[Seg], poly: &Pline, tol: f64) -> Loc {
    let mid = piece.at(0.5);
    let mut best = (f64::INFINITY, 0usize, 0.0);
    for (i, s) in other.iter().enumerate() {
        let (d, f) = s.nearest(mid);
        if d < best.0 {
            best = (d, i, f);
        }
    }
    if best.0 <= tol {
        let t = other.get(best.1).map(|s| s.tangent(best.2)).unwrap_or([1.0, 0.0]);
        return if dot(piece.tangent(0.5), t) >= 0.0 { Loc::Same } else { Loc::Opposite };
    }
    if poly.winding_number(mid[0], mid[1]) != 0 { Loc::Inside } else { Loc::Outside }
}

/// The kept pieces chained into closed loops, or `None` when a chain cannot close.
fn chain(pieces: Vec<Seg>, tol: f64) -> Option<Vec<Pline>> {
    let cell = (tol * 8.0).max(1e-12);
    let key = |p: P| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
    let mut starts: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in pieces.iter().enumerate() {
        starts.entry(key(s.a)).or_default().push(i);
    }
    let from = |p: P, used: &[bool]| -> Vec<usize> {
        let (kx, ky) = key(p);
        let mut out = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = starts.get(&(kx + dx, ky + dy)) {
                    out.extend(v.iter().copied().filter(|i| !used[*i] && dist(pieces[*i].a, p) <= tol * 4.0));
                }
            }
        }
        out
    };
    let mut used = vec![false; pieces.len()];
    let mut loops = Vec::new();
    for first in 0..pieces.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut ring = vec![first];
        let start = pieces[first].a;
        loop {
            let last = pieces[*ring.last()?];
            let end = last.b;
            let mut cands = from(end, &used);
            let closes = dist(end, start) <= tol * 4.0;
            if cands.is_empty() {
                if closes {
                    break;
                }
                return None;
            }
            // The most left-turning continuation keeps loops that touch at a point apart.
            let turn = |s: &Seg| {
                let (t0, t1) = (last.tangent(1.0), s.tangent(0.0));
                cross(t0, t1).atan2(dot(t0, t1))
            };
            cands.sort_by(|x, y| turn(&pieces[*y]).total_cmp(&turn(&pieces[*x])));
            let best = cands[0];
            if closes {
                let close_turn = {
                    let (t0, t1) = (last.tangent(1.0), pieces[first].tangent(0.0));
                    cross(t0, t1).atan2(dot(t0, t1))
                };
                if close_turn >= turn(&pieces[best]) {
                    break;
                }
            }
            used[best] = true;
            ring.push(best);
            if ring.len() > pieces.len() {
                return None;
            }
        }
        let mut p = Pline::new_closed();
        for i in ring {
            let s = pieces[i];
            p.add(s.a[0], s.a[1], s.bulge);
        }
        if p.vertex_count() >= 2 {
            loops.push(p.remove_repeat_pos(tol).unwrap_or(p));
        }
    }
    Some(loops)
}
