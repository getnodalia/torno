// SPDX-License-Identifier: MIT OR Apache-2.0
//! Constructors: extrusion, revolution and the primitive solids built from them.
//!
//! Every solid is built as a closed shell whose faces follow the orientation convention of
//! [`crate::topology`] and is meant to pass [`Brep::validate`](crate::check). The tests build
//! each primitive and check it with the validity checker and the mass properties.
//!
//! * [`extrude`]: a [`Profile`] (outer loop with holes, lines and arcs) swept along its frame's
//!   `z` by `height`. Line pieces make plane faces, arc pieces cylinder faces; a full circle
//!   gets a seam edge so that its side face is a rectangle in `(u, v)`.
//! * [`revolve`]: a closed [`Profile2`] in the `(x, z)` half-plane (`x` is the distance from the
//!   axis, `z` along it) turned a full turn about the frame's `z`. Pieces perpendicular to the
//!   axis make planes (disks, or annuli), parallel ones cylinders, inclined ones cones, arcs
//!   about a centre on the axis spheres and arcs about an off-axis centre ring tori. A piece
//!   that touches the axis at one end becomes a face whose single loop is the circle at the
//!   other end (the axis point is a pole of the surface, not a vertex); any other piece gets a
//!   seam edge. Segments lying on the axis make no face.
//!
//! Limits (each is a clear [`BrepError`], never a panic): partial revolutions, profiles crossing
//! the axis, spindle and apple tori (`major ≤ minor`), arcs about a centre on the negative side.

use crate::curve::{Circle, Curve, Line};
use crate::error::{BrepError, Result};
use crate::math::{Frame, Point3, Vec2, Vec3};
use crate::profile::{Piece, PieceKind, Profile, Profile2, Seg, reverse_loop, signed_area};
use crate::surface::{Cone, Cylinder, Plane, Sphere, Surface, Torus};
use crate::tol::Tolerance;
use crate::topology::{Brep, EdgeId, LoopId, VertexId};
use std::f64::consts::{PI, TAU};

/// Largest extent accepted for a constructor's dimensions.
const MAX_DIM: f64 = 1.0e6;

fn dimension(x: f64, what: &str) -> Result<f64> {
    if !x.is_finite() {
        return Err(BrepError::non_finite(what));
    }
    if !(x > 0.0 && x <= MAX_DIM) {
        return Err(BrepError::OutOfRange(format!("{what} {x} must be within (0, {MAX_DIM:e}]")));
    }
    Ok(x)
}

fn world(frame: &Frame, x: f64, y: f64, z: f64) -> Point3 {
    frame.to_world(Vec3::new(x, y, z))
}

/// A frame with this frame's axes and a new origin.
fn frame_at(frame: &Frame, origin: Point3) -> Result<Frame> {
    Frame::from_xy(origin, frame.x, frame.y)
}

struct Ring {
    verts: Vec<VertexId>,
    /// `uses[i]` is piece `i` traversed in the profile's direction.
    uses: Vec<(EdgeId, bool)>,
}

/// The vertices and edges of a profile loop placed at height `z` in `frame`.
fn ring(brep: &mut Brep, frame: &Frame, pieces: &[Piece], z: f64) -> Result<Ring> {
    let mut verts = Vec::with_capacity(pieces.len());
    for p in pieces {
        verts.push(brep.add_vertex(world(frame, p.from.x, p.from.y, z))?);
    }
    let n = pieces.len();
    let mut uses = Vec::with_capacity(n);
    for (i, p) in pieces.iter().enumerate() {
        let (va, vb) = (verts[i], verts[(i + 1) % n]);
        uses.push(match p.kind {
            PieceKind::Line => {
                let (line, len) = Line::through(world(frame, p.from.x, p.from.y, z), world(frame, p.to.x, p.to.y, z))?;
                (brep.add_edge(Curve::Line(line), 0.0, len, va, vb)?, false)
            }
            PieceKind::Arc { center, radius, a0, sweep } => {
                let circle = Circle::new(frame_at(frame, world(frame, center.x, center.y, z))?, radius)?;
                if sweep > 0.0 {
                    (brep.add_edge(Curve::Circle(circle), a0, a0 + sweep, va, vb)?, false)
                } else {
                    (brep.add_edge(Curve::Circle(circle), a0 + sweep, a0, vb, va)?, true)
                }
            }
        });
    }
    Ok(Ring { verts, uses })
}

/// Sweep `profile` (in the `x`–`y` plane of `frame`) along the frame's `z` by `height`.
pub fn extrude(profile: &Profile, frame: &Frame, height: f64, tol: &Tolerance) -> Result<Brep> {
    frame.validate()?;
    let height = dimension(height, "extrusion height")?;
    let resolved = profile.resolve(tol)?;
    let loops: Vec<&Vec<Piece>> = std::iter::once(&resolved.outer).chain(resolved.holes.iter()).collect();
    let mut brep = Brep::new();
    let mut faces = Vec::new();
    let mut bottom_loops: Vec<LoopId> = Vec::new();
    let mut top_loops: Vec<LoopId> = Vec::new();
    for pieces in &loops {
        let n = pieces.len();
        let bottom = ring(&mut brep, frame, pieces, 0.0)?;
        let top = ring(&mut brep, frame, pieces, height)?;
        let mut verticals = Vec::with_capacity(n);
        for i in 0..n {
            let (pb, pt) = (world(frame, pieces[i].from.x, pieces[i].from.y, 0.0), world(frame, pieces[i].from.x, pieces[i].from.y, height));
            let (line, len) = Line::through(pb, pt)?;
            verticals.push(brep.add_edge(Curve::Line(line), 0.0, len, bottom.verts[i], top.verts[i])?);
        }
        for (i, piece) in pieces.iter().enumerate() {
            let j = (i + 1) % n;
            let (eb, rb) = bottom.uses[i];
            let (et, rt) = top.uses[i];
            let side = brep.add_loop(&[(eb, rb), (verticals[j], false), (et, !rt), (verticals[i], true)])?;
            let (surface, same_sense) = match piece.kind {
                PieceKind::Line => {
                    let d = frame.x * (piece.to.x - piece.from.x) + frame.y * (piece.to.y - piece.from.y);
                    (Surface::Plane(Plane::new(Frame::from_xy(world(frame, piece.from.x, piece.from.y, 0.0), d, frame.z)?)?), true)
                }
                PieceKind::Arc { center, radius, sweep, .. } => {
                    (Surface::Cylinder(Cylinder::new(frame_at(frame, world(frame, center.x, center.y, 0.0))?, radius)?), sweep > 0.0)
                }
            };
            faces.push(brep.add_face(surface, same_sense, side, &[])?);
        }
        bottom_loops.push(brep.add_loop(&bottom.uses.iter().rev().map(|&(e, r)| (e, !r)).collect::<Vec<_>>())?);
        top_loops.push(brep.add_loop(&top.uses)?);
    }
    let plane_at = |z: f64| -> Result<Surface> { Ok(Surface::Plane(Plane::new(frame_at(frame, world(frame, 0.0, 0.0, z))?)?)) };
    if let Some((outer, holes)) = bottom_loops.split_first() {
        faces.push(brep.add_face(plane_at(0.0)?, false, *outer, holes)?);
    }
    if let Some((outer, holes)) = top_loops.split_first() {
        faces.push(brep.add_face(plane_at(height)?, true, *outer, holes)?);
    }
    let shell = brep.add_shell(&faces)?;
    brep.add_solid(&[shell])?;
    Ok(brep)
}

/// The box with one corner at the frame's origin and extents `dx`, `dy`, `dz` along its axes.
pub fn cuboid(frame: &Frame, dx: f64, dy: f64, dz: f64, tol: &Tolerance) -> Result<Brep> {
    let (dx, dy, dz) = (dimension(dx, "box width")?, dimension(dy, "box depth")?, dimension(dz, "box height")?);
    extrude(&Profile::new(Profile2::rectangle(0.0, 0.0, dx, dy)?), frame, dz, tol)
}

/// The cylinder of the given radius and height, its axis the frame's `z` from the origin.
pub fn cylinder(frame: &Frame, radius: f64, height: f64, tol: &Tolerance) -> Result<Brep> {
    let radius = dimension(radius, "cylinder radius")?;
    extrude(&Profile::new(Profile2::circle(Vec2::ZERO, radius)?), frame, dimension(height, "cylinder height")?, tol)
}

/// The cone or frustum with radius `r0` at the frame's origin and `r1` at `height` along `z`
/// (one of them may be zero, not both).
pub fn cone(frame: &Frame, r0: f64, r1: f64, height: f64, tol: &Tolerance) -> Result<Brep> {
    let height = dimension(height, "cone height")?;
    for (r, what) in [(r0, "cone base radius"), (r1, "cone top radius")] {
        if !r.is_finite() {
            return Err(BrepError::non_finite(what));
        }
        if !(0.0..=MAX_DIM).contains(&r) {
            return Err(BrepError::OutOfRange(format!("{what} {r} must be within [0, {MAX_DIM:e}]")));
        }
    }
    if r0 == 0.0 && r1 == 0.0 {
        return Err(BrepError::degenerate("a cone needs a non-zero radius at one end"));
    }
    let mut pts = vec![Vec2::ZERO];
    if r0 > 0.0 {
        pts.push(Vec2::new(r0, 0.0));
    }
    if r1 > 0.0 {
        pts.push(Vec2::new(r1, height));
    }
    pts.push(Vec2::new(0.0, height));
    revolve(&Profile2::polygon(&pts)?, frame, tol)
}

/// A sphere centred on the frame's origin.
pub fn sphere(frame: &Frame, radius: f64, tol: &Tolerance) -> Result<Brep> {
    let r = dimension(radius, "sphere radius")?;
    let profile =
        Profile2 { start: Vec2::new(0.0, -r), segs: vec![Seg::Arc { center: Vec2::ZERO, sweep: PI }, Seg::Line { to: Vec2::new(0.0, -r) }] };
    revolve(&profile, frame, tol)
}

/// A ring torus about the frame's `z` axis (`major > minor`).
pub fn torus(frame: &Frame, major: f64, minor: f64, tol: &Tolerance) -> Result<Brep> {
    let (major, minor) = (dimension(major, "torus major radius")?, dimension(minor, "torus minor radius")?);
    if major <= minor {
        return Err(BrepError::Unsupported("a torus needs major radius greater than minor radius (spindle and apple tori are not supported)".into()));
    }
    revolve(&Profile2::circle(Vec2::new(major, 0.0), minor)?, frame, tol)
}

/// The kind of surface a revolved piece lies on, with its sense.
struct Swept {
    surface: Surface,
    same_sense: bool,
}

/// Revolve a closed profile in the `(x, z)` half-plane of `frame` a full turn about its `z` axis.
pub fn revolve(profile: &Profile2, frame: &Frame, tol: &Tolerance) -> Result<Brep> {
    frame.validate()?;
    let mut pieces = profile.resolve(tol)?;
    if signed_area(&pieces) < 0.0 {
        pieces = reverse_loop(&pieces);
    }
    let on_axis = |p: Vec2| p.x.abs() <= tol.linear;
    // The profile must stay on one side of the axis.
    for p in &pieces {
        let mut min_x = p.from.x.min(p.to.x);
        if let PieceKind::Arc { center, radius, a0, sweep } = p.kind {
            let (lo, hi) = (a0.min(a0 + sweep), a0.max(a0 + sweep));
            let mut a = PI;
            while a - TAU >= lo {
                a -= TAU;
            }
            while a < lo {
                a += TAU;
            }
            if a <= hi {
                min_x = min_x.min(center.x - radius);
            }
        }
        if min_x < -tol.linear {
            return Err(BrepError::Topology("the profile crosses the axis of revolution".into()));
        }
    }
    // An arc between two axis points is cut in two at its middle so that each half has an off-axis end.
    let mut split: Vec<Piece> = Vec::with_capacity(pieces.len() + 2);
    for p in pieces {
        if let PieceKind::Arc { center, radius, a0, sweep } = p.kind {
            if !p.is_full_circle() && on_axis(p.from) && on_axis(p.to) {
                let mid = p.at(0.5);
                split.push(Piece { from: p.from, to: mid, kind: PieceKind::Arc { center, radius, a0, sweep: sweep / 2.0 } });
                split.push(Piece { from: mid, to: p.to, kind: PieceKind::Arc { center, radius, a0: a0 + sweep / 2.0, sweep: sweep / 2.0 } });
                continue;
            }
        }
        split.push(p);
    }
    let pieces = split;
    let n = pieces.len();
    let mut brep = Brep::new();
    // One vertex and one circle edge per profile point off the axis.
    let mut verts: Vec<Option<VertexId>> = Vec::with_capacity(n);
    let mut circles: Vec<Option<EdgeId>> = Vec::with_capacity(n);
    for p in &pieces {
        if on_axis(p.from) {
            verts.push(None);
            circles.push(None);
            continue;
        }
        let v = brep.add_vertex(world(frame, p.from.x, 0.0, p.from.y))?;
        let circle = Circle::new(frame_at(frame, world(frame, 0.0, 0.0, p.from.y))?, p.from.x)?;
        circles.push(Some(brep.add_edge(Curve::Circle(circle), 0.0, TAU, v, v)?));
        verts.push(Some(v));
    }
    let mut faces = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let j = (i + 1) % n;
        let (a_on, b_on) = (on_axis(p.from), on_axis(p.to));
        if matches!(p.kind, PieceKind::Line) && a_on && b_on {
            continue; // a segment on the axis: no face
        }
        let (a, b) = (p.from, p.to);
        let sense_swept = |surface: Surface, same_sense: bool| Swept { surface, same_sense };
        let swept = match p.kind {
            PieceKind::Line => {
                let (dx, dz) = (b.x - a.x, b.y - a.y);
                if dz.abs() <= tol.linear {
                    let same = -dx > 0.0;
                    let plane = Surface::Plane(Plane::new(frame_at(frame, world(frame, 0.0, 0.0, a.y))?)?);
                    let (ci, cj) = (circles[i], circles[j]);
                    let loops = match (ci, cj) {
                        (Some(ci), Some(cj)) => {
                            let (big, small) = if a.x > b.x { (ci, cj) } else { (cj, ci) };
                            let outer = brep.add_loop(&[(big, !same)])?;
                            let inner = brep.add_loop(&[(small, same)])?;
                            (outer, vec![inner])
                        }
                        (Some(c), None) | (None, Some(c)) => (brep.add_loop(&[(c, !same)])?, Vec::new()),
                        (None, None) => continue,
                    };
                    faces.push(brep.add_face(plane, same, loops.0, &loops.1)?);
                    continue;
                } else if dx.abs() <= tol.linear {
                    sense_swept(Surface::Cylinder(Cylinder::new(*frame, a.x)?), dz > 0.0)
                } else {
                    let apex = a.y - a.x * dz / dx;
                    let half = (dx / dz).abs().atan();
                    sense_swept(Surface::Cone(Cone::new(frame_at(frame, world(frame, 0.0, 0.0, apex))?, half)?), dz > 0.0)
                }
            }
            PieceKind::Arc { center, radius, sweep, .. } => {
                let origin = frame_at(frame, world(frame, 0.0, 0.0, center.y))?;
                if center.x.abs() <= tol.linear {
                    sense_swept(Surface::Sphere(Sphere::new(origin, radius)?), sweep > 0.0)
                } else if center.x > radius + tol.linear {
                    sense_swept(Surface::Torus(Torus::new(origin, center.x, radius)?), sweep > 0.0)
                } else {
                    return Err(BrepError::Unsupported(
                        "a revolved arc must be about a centre on the axis (sphere) or farther from it than its radius (torus)".into(),
                    ));
                }
            }
        };
        let Swept { surface, same_sense } = swept;
        let boundary = if a_on || b_on {
            // One end on the axis: the circle at the other end is the whole boundary.
            let off = if a_on { circles[j] } else { circles[i] };
            let c = off.ok_or_else(|| BrepError::Topology("a piece has no circle at its off-axis end".into()))?;
            brep.add_loop(&[(c, a_on)])?
        } else {
            let (Some(ci), Some(cj), Some(vi), Some(vj)) = (circles[i], circles[j], verts[i], verts[j]) else {
                return Err(BrepError::Topology("a piece is missing its end circles".into()));
            };
            let seam = match p.kind {
                PieceKind::Line => {
                    let (line, len) = Line::through(world(frame, a.x, 0.0, a.y), world(frame, b.x, 0.0, b.y))?;
                    (brep.add_edge(Curve::Line(line), 0.0, len, vi, vj)?, false)
                }
                PieceKind::Arc { center, radius, a0, sweep } => {
                    let fr = Frame::from_xy(world(frame, center.x, 0.0, center.y), frame.x, frame.z)?;
                    let circle = Circle::new(fr, radius)?;
                    if sweep > 0.0 {
                        (brep.add_edge(Curve::Circle(circle), a0, a0 + sweep, vi, vj)?, false)
                    } else {
                        (brep.add_edge(Curve::Circle(circle), a0 + sweep, a0, vj, vi)?, true)
                    }
                }
            };
            brep.add_loop(&[(ci, false), seam, (cj, true), (seam.0, !seam.1)])?
        };
        faces.push(brep.add_face(surface, same_sense, boundary, &[])?);
    }
    if faces.is_empty() {
        return Err(BrepError::Degenerate("the profile makes no faces".into()));
    }
    let shell = brep.add_shell(&faces)?;
    brep.add_solid(&[shell])?;
    Ok(brep)
}
