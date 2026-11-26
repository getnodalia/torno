// SPDX-License-Identifier: MIT OR Apache-2.0
//! Planar profiles: closed chains of line segments and circular arcs, the input of
//! [`extrude`](crate::build::extrude) and [`revolve`](crate::build::revolve).
//!
//! A [`Profile2`] is described by a start point and segments that each continue from the end of
//! the previous one: a line to a point, or an arc about a centre by a signed sweep (positive is
//! counter-clockwise). [`Profile2::resolve`] turns it into [`Piece`]s with every endpoint
//! computed, checks that the chain closes, and rejects degenerate pieces. A full circle is one arc
//! with a sweep of `±2π` and must be the only segment of its loop.
//!
//! Nesting (holes inside the outer loop, no loop crossing another) is checked on polygonal
//! approximations of the arcs (`π/16` per step), so a hole that pokes out of the outer loop by less
//! than the sagitta of such a step is not detected here; [`Brep::validate`](crate::check) checks
//! the finished solid against its true boundary.

use crate::error::{BrepError, Result};
use crate::math::Vec2;
use crate::robust::segments_cross_properly;
use crate::tol::Tolerance;
use crate::uv::inside_even_odd;
use std::f64::consts::{PI, TAU};

/// Largest coordinate accepted in a profile.
const MAX_COORD: f64 = 1.0e9;
/// Angular step of the polygonal approximation used for the nesting checks.
const NESTING_STEP: f64 = PI / 16.0;

/// One segment of a profile, continuing from the previous end point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    /// A straight line to `to`.
    Line { to: Vec2 },
    /// An arc about `center` (the radius is the distance from the current point to it) turning by
    /// `sweep` radians, counter-clockwise when positive; `|sweep| ≤ 2π`.
    Arc { center: Vec2, sweep: f64 },
}

/// A closed chain of segments as the caller describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile2 {
    pub start: Vec2,
    pub segs: Vec<Seg>,
}

/// The shape of a resolved piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PieceKind {
    Line,
    /// `a0` is the angle of `from` about the centre (`atan2`), `sweep` the signed turn.
    Arc {
        center: Vec2,
        radius: f64,
        a0: f64,
        sweep: f64,
    },
}

/// A segment with both end points known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub from: Vec2,
    pub to: Vec2,
    pub kind: PieceKind,
}

impl Piece {
    /// A full circle (its end point is its start point).
    pub fn is_full_circle(&self) -> bool {
        matches!(self.kind, PieceKind::Arc { sweep, .. } if sweep.abs() >= TAU - 1e-12)
    }

    /// The same piece traversed the other way.
    pub fn reversed(&self) -> Piece {
        match self.kind {
            PieceKind::Line => Piece { from: self.to, to: self.from, kind: PieceKind::Line },
            PieceKind::Arc { center, radius, a0, sweep } => {
                Piece { from: self.to, to: self.from, kind: PieceKind::Arc { center, radius, a0: a0 + sweep, sweep: -sweep } }
            }
        }
    }

    /// The point at fraction `s ∈ [0, 1]` along the piece.
    pub fn at(&self, s: f64) -> Vec2 {
        match self.kind {
            PieceKind::Line => self.from + (self.to - self.from) * s,
            PieceKind::Arc { center, radius, a0, sweep } => {
                let a = a0 + sweep * s;
                center + Vec2::new(radius * a.cos(), radius * a.sin())
            }
        }
    }

    /// Points along the piece from its start, the end excluded, at most `π/16` of arc apart.
    fn sample(&self) -> Vec<Vec2> {
        match self.kind {
            PieceKind::Line => vec![self.from],
            PieceKind::Arc { sweep, .. } => {
                let n = ((sweep.abs() / NESTING_STEP).ceil() as usize).max(1);
                (0..n).map(|k| self.at(k as f64 / n as f64)).collect()
            }
        }
    }

    /// `∫ ½ (x dy − y dx)` along the piece: its contribution to the signed area of the loop.
    fn area_term(&self) -> f64 {
        match self.kind {
            PieceKind::Line => 0.5 * self.from.cross(self.to),
            PieceKind::Arc { center, radius, a0, sweep } => {
                let a1 = a0 + sweep;
                0.5 * (radius * radius * sweep + radius * center.x * (a1.sin() - a0.sin()) - radius * center.y * (a1.cos() - a0.cos()))
            }
        }
    }
}

/// A resolved loop's signed area (positive when counter-clockwise).
pub fn signed_area(pieces: &[Piece]) -> f64 {
    pieces.iter().map(Piece::area_term).sum()
}

/// The same loop traversed the other way.
pub fn reverse_loop(pieces: &[Piece]) -> Vec<Piece> {
    pieces.iter().rev().map(Piece::reversed).collect()
}

fn finite2(p: Vec2, what: &str) -> Result<()> {
    if !p.is_finite() {
        return Err(BrepError::non_finite(what));
    }
    if p.x.abs().max(p.y.abs()) > MAX_COORD {
        return Err(BrepError::OutOfRange(format!("{what} beyond the supported extent")));
    }
    Ok(())
}

impl Profile2 {
    /// A closed polygon through the points in order (the closing segment is implied).
    pub fn polygon(points: &[Vec2]) -> Result<Profile2> {
        let (first, rest) = points.split_first().ok_or_else(|| BrepError::invalid("a polygon needs at least three points"))?;
        if points.len() < 3 {
            return Err(BrepError::invalid("a polygon needs at least three points"));
        }
        let mut segs: Vec<Seg> = rest.iter().map(|&to| Seg::Line { to }).collect();
        segs.push(Seg::Line { to: *first });
        Ok(Profile2 { start: *first, segs })
    }

    /// The axis-aligned rectangle with opposite corners `(x0, y0)` and `(x1, y1)`, counter-clockwise.
    pub fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Profile2> {
        let (xa, xb) = (x0.min(x1), x0.max(x1));
        let (ya, yb) = (y0.min(y1), y0.max(y1));
        Profile2::polygon(&[Vec2::new(xa, ya), Vec2::new(xb, ya), Vec2::new(xb, yb), Vec2::new(xa, yb)])
    }

    /// A full circle, counter-clockwise, starting on the positive `x` side of the centre.
    pub fn circle(center: Vec2, radius: f64) -> Result<Profile2> {
        finite2(center, "circle centre")?;
        if !(radius.is_finite() && radius > 0.0 && radius <= MAX_COORD) {
            return Err(BrepError::OutOfRange(format!("circle radius {radius} must be positive and finite")));
        }
        Ok(Profile2 { start: center + Vec2::new(radius, 0.0), segs: vec![Seg::Arc { center, sweep: TAU }] })
    }

    /// Compute every end point, check the chain closes and has no degenerate piece.
    pub fn resolve(&self, tol: &Tolerance) -> Result<Vec<Piece>> {
        finite2(self.start, "profile start")?;
        if self.segs.is_empty() {
            return Err(BrepError::invalid("a profile needs at least one segment"));
        }
        let mut pieces: Vec<Piece> = Vec::with_capacity(self.segs.len());
        let mut cur = self.start;
        for seg in &self.segs {
            let piece = match *seg {
                Seg::Line { to } => {
                    finite2(to, "profile line end")?;
                    if (to - cur).norm() <= tol.linear {
                        return Err(BrepError::degenerate("a profile line has zero length"));
                    }
                    Piece { from: cur, to, kind: PieceKind::Line }
                }
                Seg::Arc { center, sweep } => {
                    finite2(center, "profile arc centre")?;
                    if !sweep.is_finite() {
                        return Err(BrepError::non_finite("profile arc sweep"));
                    }
                    let d = cur - center;
                    let radius = d.norm();
                    if radius <= tol.linear {
                        return Err(BrepError::degenerate("a profile arc has zero radius"));
                    }
                    if sweep == 0.0 || sweep.abs() > TAU + 1e-12 {
                        return Err(BrepError::OutOfRange(format!("arc sweep {sweep} must be non-zero with magnitude at most 2π")));
                    }
                    let full = sweep.abs() >= TAU - 1e-12;
                    let sweep = if full { TAU.copysign(sweep) } else { sweep };
                    let a0 = d.y.atan2(d.x);
                    let to = if full { cur } else { center + Vec2::new(radius * (a0 + sweep).cos(), radius * (a0 + sweep).sin()) };
                    Piece { from: cur, to, kind: PieceKind::Arc { center, radius, a0, sweep } }
                }
            };
            cur = piece.to;
            pieces.push(piece);
        }
        if (cur - self.start).norm() > tol.linear {
            return Err(BrepError::Topology(format!("the profile does not close: it ends {:.3e} away from its start", (cur - self.start).norm())));
        }
        if let Some(last) = pieces.last_mut() {
            last.to = self.start;
        }
        if pieces.iter().any(Piece::is_full_circle) && pieces.len() != 1 {
            return Err(BrepError::Topology("a full circle must be the only segment of its loop".into()));
        }
        for (i, a) in pieces.iter().enumerate() {
            for b in pieces.iter().skip(i + 1) {
                if pieces.len() > 1 && (a.from - b.from).norm() <= tol.linear {
                    return Err(BrepError::Topology("a profile touches itself at a vertex".into()));
                }
            }
        }
        if signed_area(&pieces).abs() <= tol.linear * tol.linear {
            return Err(BrepError::degenerate("the profile encloses no area"));
        }
        Ok(pieces)
    }
}

/// An outer loop with holes, as the caller describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub outer: Profile2,
    pub holes: Vec<Profile2>,
}

/// A profile with every loop resolved and oriented: the outer loop counter-clockwise, the holes
/// clockwise.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedProfile {
    pub outer: Vec<Piece>,
    pub holes: Vec<Vec<Piece>>,
}

fn polygon_of(pieces: &[Piece]) -> Vec<Vec2> {
    pieces.iter().flat_map(Piece::sample).collect()
}

impl Profile {
    pub fn new(outer: Profile2) -> Profile {
        Profile { outer, holes: Vec::new() }
    }

    /// Resolve and orient the loops and check their nesting (see the module documentation for
    /// what the check covers).
    pub fn resolve(&self, tol: &Tolerance) -> Result<ResolvedProfile> {
        let mut outer = self.outer.resolve(tol)?;
        if signed_area(&outer) < 0.0 {
            outer = reverse_loop(&outer);
        }
        let mut holes = Vec::with_capacity(self.holes.len());
        for h in &self.holes {
            let mut p = h.resolve(tol)?;
            if signed_area(&p) > 0.0 {
                p = reverse_loop(&p);
            }
            holes.push(p);
        }
        let polys: Vec<Vec<Vec2>> = std::iter::once(&outer).chain(holes.iter()).map(|l| polygon_of(l)).collect();
        let total: usize = polys.iter().map(Vec::len).sum();
        if total > 20_000 {
            return Err(BrepError::OutOfRange("profile has too many segments for the nesting check".into()));
        }
        let outer_poly: &[Vec2] = &polys[0];
        for (i, hole) in polys.iter().enumerate().skip(1) {
            for &p in hole {
                if !inside_even_odd(&[outer_poly], p)? {
                    return Err(BrepError::Topology(format!("hole {} is not inside the outer loop", i - 1)));
                }
            }
            for (j, other) in polys.iter().enumerate().skip(1) {
                if i != j {
                    if let Some(&p) = hole.first() {
                        if inside_even_odd(&[other], p)? {
                            return Err(BrepError::Topology(format!("hole {} lies inside hole {}", i - 1, j - 1)));
                        }
                    }
                }
            }
        }
        for (i, a) in polys.iter().enumerate() {
            for b in polys.iter().skip(i) {
                if crosses(a, b, std::ptr::eq(a, b))? {
                    return Err(BrepError::Topology("two profile loops cross each other".into()));
                }
            }
        }
        Ok(ResolvedProfile { outer, holes })
    }
}

/// Whether two closed polygons cross properly (`same`: they are the same polygon, so a polygon
/// is checked against its own non-adjacent edges).
fn crosses(a: &[Vec2], b: &[Vec2], same: bool) -> Result<bool> {
    let (na, nb) = (a.len(), b.len());
    for i in 0..na {
        let (p, q) = (a[i], a[(i + 1) % na]);
        for j in 0..nb {
            if same && (j <= i || j == (i + 1) % na || (j + 1) % nb == i) {
                continue;
            }
            let (r, s) = (b[j], b[(j + 1) % nb]);
            if segments_cross_properly(p.to_array(), q.to_array(), r.to_array(), s.to_array())? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Tolerance = Tolerance::DEFAULT;

    #[test]
    fn rectangle_and_circle_areas_are_exact() {
        let r = Profile2::rectangle(0.0, 0.0, 4.0, 3.0).unwrap().resolve(&T).unwrap();
        assert!((signed_area(&r) - 12.0).abs() < 1e-12);
        let c = Profile2::circle(Vec2::new(5.0, -2.0), 2.0).unwrap().resolve(&T).unwrap();
        assert!((signed_area(&c) - PI * 4.0).abs() < 1e-12, "{}", signed_area(&c));
        assert!((signed_area(&reverse_loop(&c)) + PI * 4.0).abs() < 1e-12);
        assert!((signed_area(&reverse_loop(&r)) + 12.0).abs() < 1e-12);
    }

    #[test]
    fn a_half_disk_has_the_analytic_area() {
        // The arc runs from (r, 0) to (-r, 0) about the origin, the line closes it.
        let r = 3.0;
        let p = Profile2 { start: Vec2::new(r, 0.0), segs: vec![Seg::Arc { center: Vec2::ZERO, sweep: PI }, Seg::Line { to: Vec2::new(r, 0.0) }] };
        let pieces = p.resolve(&T).unwrap();
        assert!((signed_area(&pieces) - 0.5 * PI * r * r).abs() < 1e-12);
        assert!((pieces[0].to - Vec2::new(-r, 0.0)).norm() < 1e-12);
    }

    #[test]
    fn bad_profiles_are_refused_with_a_reason() {
        let nan = Profile2 { start: Vec2::ZERO, segs: vec![Seg::Line { to: Vec2::new(f64::NAN, 0.0) }] };
        assert!(matches!(nan.resolve(&T), Err(BrepError::NonFinite(_))));
        let open = Profile2 { start: Vec2::ZERO, segs: vec![Seg::Line { to: Vec2::new(1.0, 0.0) }, Seg::Line { to: Vec2::new(1.0, 1.0) }] };
        assert!(matches!(open.resolve(&T), Err(BrepError::Topology(_))));
        let zero = Profile2 { start: Vec2::ZERO, segs: vec![Seg::Line { to: Vec2::ZERO }] };
        assert!(matches!(zero.resolve(&T), Err(BrepError::Degenerate(_))));
        let flat = Profile2::polygon(&[Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)]).unwrap();
        assert!(flat.resolve(&T).is_err());
        let sweep = Profile2 { start: Vec2::new(1.0, 0.0), segs: vec![Seg::Arc { center: Vec2::ZERO, sweep: 7.0 }] };
        assert!(matches!(sweep.resolve(&T), Err(BrepError::OutOfRange(_))));
        assert!(Profile2::polygon(&[]).is_err());
        assert!(Profile2::circle(Vec2::ZERO, -1.0).is_err());
        let bow = Profile2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0)]).unwrap();
        let nested = Profile::new(bow);
        assert!(nested.resolve(&T).is_err(), "a bow tie crosses itself");
    }

    #[test]
    fn nesting_is_checked_and_orientation_is_normalised() {
        let outer = Profile2::rectangle(0.0, 0.0, 10.0, 10.0).unwrap();
        let hole = Profile2::circle(Vec2::new(5.0, 5.0), 2.0).unwrap();
        let ok = Profile { outer: outer.clone(), holes: vec![hole] }.resolve(&T).unwrap();
        assert!(signed_area(&ok.outer) > 0.0);
        assert!(signed_area(&ok.holes[0]) < 0.0);
        let poking = Profile { outer: outer.clone(), holes: vec![Profile2::circle(Vec2::new(9.0, 5.0), 2.0).unwrap()] };
        assert!(poking.resolve(&T).is_err());
        let outside = Profile { outer: outer.clone(), holes: vec![Profile2::circle(Vec2::new(20.0, 5.0), 1.0).unwrap()] };
        assert!(outside.resolve(&T).is_err());
        let nested_holes =
            Profile { outer, holes: vec![Profile2::circle(Vec2::new(5.0, 5.0), 3.0).unwrap(), Profile2::circle(Vec2::new(5.0, 5.0), 1.0).unwrap()] };
        assert!(nested_holes.resolve(&T).is_err());
    }
}
