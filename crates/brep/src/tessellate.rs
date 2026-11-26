// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tessellation of a B-rep into a watertight triangle mesh.
//!
//! * Every **edge** is sampled once ([`Curve::sample_params`](crate::curve::Curve::sample_params)
//!   at the requested chord tolerance); the samples are shared by the two faces that use it, so
//!   neighbouring faces meet vertex to vertex and the mesh has no cracks or T-junctions.
//! * Every **face** is triangulated in its `(u, v)` plane, scaled by a local metric so that
//!   parameter distances are close to 3D distances, with the exact constrained Delaunay
//!   triangulation of [`crate::cdt`]: the boundary loops are the constraints, holes work through
//!   parity, and for curved surfaces a grid of interior points (spacing derived from the chord
//!   tolerance, kept clear of the boundary) keeps every triangle small enough in the curved
//!   directions.
//! * A loop that closes through a **pole** (a sphere's pole, a cone's apex) gets a run of
//!   coincident pole points and a subdivided meridian on each side; the two meridian copies share
//!   mesh vertices, and the triangles that collapse onto the pole (two corners on the same mesh
//!   vertex) are dropped.
//!
//! Orientation: counter-clockwise seen from outside for every face (`same_sense` decides the flip).
//!
//! Guarantee and limit. The mesh is watertight whenever every face triangulates (checked by
//! [`Mesh::check`]); the chord tolerance bounds the deviation of *edge* samples, and the interior
//! grid uses a spacing a safe fraction of it. The tests measure the deviation of triangle
//! interiors against the true surfaces and the volume and area against the exact values; the
//! deviation is not a proved bound for every trimmed face. Faces on NURBS surfaces are refused
//! ([`BrepError::Unsupported`]).

use crate::cdt::Cdt;
use crate::error::{BrepError, Result};
use crate::math::{Point3, Vec2};
use crate::mesh::Mesh;
use crate::surface::Surface;
use crate::topology::{Brep, EdgeId, FaceId, LoopId, VertexId};
use crate::uv::{LoopUv, inside_even_odd, loop_uv};
use std::collections::HashMap;
use std::f64::consts::FRAC_PI_4;

/// Most interior grid points per face.
const MAX_GRID_POINTS: usize = 60_000;
/// Fraction of the chord tolerance the interior grid is designed for (triangles deviate more than their edges).
const GRID_SAFETY: f64 = 0.4;
/// A torus grid is this much finer than the sphere's: its diagonals cross a doubly curved patch.
const TORUS_SHRINK: f64 = 0.65;

struct Ctx<'a> {
    brep: &'a Brep,
    chord: f64,
    mesh: Mesh,
    owner: Vec<FaceId>,
    vertex_ids: HashMap<VertexId, u32>,
    edge_ids: HashMap<EdgeId, Vec<u32>>,
}

impl Ctx<'_> {
    fn new_vertex(&mut self, p: Point3) -> Result<u32> {
        let id = u32::try_from(self.mesh.vertices.len()).map_err(|_| BrepError::OutOfRange("more than 2^32 mesh vertices".into()))?;
        self.mesh.vertices.push(p);
        Ok(id)
    }

    fn vertex(&mut self, v: VertexId) -> Result<u32> {
        if let Some(&id) = self.vertex_ids.get(&v) {
            return Ok(id);
        }
        let p = self.brep.vertex(v)?.point;
        let id = self.new_vertex(p)?;
        self.vertex_ids.insert(v, id);
        Ok(id)
    }

    /// The mesh vertices along an edge in increasing curve parameter, end points included.
    fn edge(&mut self, e: EdgeId) -> Result<Vec<u32>> {
        if let Some(ids) = self.edge_ids.get(&e) {
            return Ok(ids.clone());
        }
        let edge = self.brep.edge(e)?;
        let ts = edge.curve.sample_params(edge.t0, edge.t1, self.chord)?;
        let mut ids = Vec::with_capacity(ts.len());
        let last = ts.len().saturating_sub(1);
        for (i, &t) in ts.iter().enumerate() {
            ids.push(if i == 0 {
                self.vertex(edge.start)?
            } else if i == last {
                self.vertex(edge.end)?
            } else {
                self.new_vertex(edge.curve.point(t))?
            });
        }
        self.edge_ids.insert(e, ids.clone());
        Ok(ids)
    }

    /// The mesh vertices of a loop in traversal order, the first not repeated at the end.
    fn loop_ids(&mut self, l: LoopId) -> Result<Vec<u32>> {
        let mut out = Vec::new();
        for &c in &self.brep.loop_(l)?.coedges {
            let co = self.brep.coedge(c)?;
            let mut ids = self.edge(co.edge)?;
            if co.reversed {
                ids.reverse();
            }
            ids.pop();
            out.extend(ids);
        }
        Ok(out)
    }
}

/// The largest angle (radians) whose chord sagitta on a circle of `radius` stays within `chord`.
fn angle_step(chord: f64, radius: f64) -> f64 {
    if !(radius > 0.0) || chord >= radius {
        return FRAC_PI_4;
    }
    (2.0 * (1.0 - chord / radius).acos()).clamp(1.0e-4, FRAC_PI_4)
}

/// How a surface's parameter plane is scaled and gridded.
///
/// The scale makes the interior grid cells squares (`sx = 1/hu`, `sy = 1/hv`): the Delaunay
/// triangulation of a stretched grid joins points several columns apart, which is exactly the
/// long chord across a curved direction the chord tolerance forbids.
struct Metric {
    sx: f64,
    sy: f64,
    /// Grid step in `u` (radians); `None` when no interior points are wanted (a plane).
    hu: Option<f64>,
    /// Grid step in `v` (a length for a cylinder or cone, radians for a sphere or torus).
    hv: f64,
}

fn metric(surface: &Surface, chord: f64, lo: Vec2, hi: Vec2) -> Result<Metric> {
    let step = |r: f64| angle_step(chord * GRID_SAFETY, r);
    let span_v = (hi.y - lo.y).max(0.0);
    // Rows of a surface that is straight in `v`: cells about as tall as they are wide.
    let straight = |radius: f64, hu: f64, length_per_v: f64| {
        let cell = (radius * hu).max(1.0e-12);
        let rows = ((span_v * length_per_v) / cell).ceil().clamp(1.0, 1.0e6);
        span_v.max(1.0e-12) / rows
    };
    Ok(match surface {
        Surface::Plane(_) => Metric { sx: 1.0, sy: 1.0, hu: None, hv: 1.0 },
        Surface::Cylinder(c) => {
            let hu = step(c.radius);
            let hv = straight(c.radius, hu, 1.0);
            Metric { sx: 1.0 / hu, sy: 1.0 / hv, hu: Some(hu), hv }
        }
        Surface::Cone(c) => {
            let t = c.tan();
            let r_max = (lo.y.abs().max(hi.y.abs()) * t).max(1.0e-12);
            let hu = step(r_max);
            let hv = straight(r_max, hu, (1.0 + t * t).sqrt());
            Metric { sx: 1.0 / hu, sy: 1.0 / hv, hu: Some(hu), hv }
        }
        Surface::Sphere(s) => {
            let h = step(s.radius);
            Metric { sx: 1.0 / h, sy: 1.0 / h, hu: Some(h), hv: h }
        }
        Surface::Torus(t) => {
            // Both principal curvatures bend the same way on the outer half, and a diagonal sees their sum.
            let (hu, hv) = (step(t.major + t.minor) * TORUS_SHRINK, step(t.minor) * TORUS_SHRINK);
            Metric { sx: 1.0 / hu, sy: 1.0 / hv, hu: Some(hu), hv }
        }
        Surface::Nurbs(_) => return Err(BrepError::Unsupported("tessellation of faces on NURBS surfaces".into())),
    })
}

struct PolyPt {
    uv: Vec2,
    gid: u32,
}

fn bounds(polys: &[Vec<PolyPt>]) -> (Vec2, Vec2) {
    let flat: Vec<&PolyPt> = polys.iter().flatten().collect();
    extent(flat.into_iter())
}

fn poly_bounds(poly: &[PolyPt]) -> (Vec2, Vec2) {
    extent(poly.iter())
}

fn extent<'a>(points: impl Iterator<Item = &'a PolyPt>) -> (Vec2, Vec2) {
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for p in points {
        lo = Vec2::new(lo.x.min(p.uv.x), lo.y.min(p.uv.y));
        hi = Vec2::new(hi.x.max(p.uv.x), hi.y.max(p.uv.y));
    }
    (lo, hi)
}

/// The boundary polygon of an unwrapped loop, with the pole detour (and its subdivided meridians) when it closes through a pole.
fn polygon(ctx: &mut Ctx, surface: &Surface, uv: &LoopUv, gids: &[u32], metric_h: (f64, f64)) -> Result<Vec<PolyPt>> {
    let mut poly: Vec<PolyPt> = uv.points.iter().zip(gids).map(|(&uv, &gid)| PolyPt { uv, gid }).collect();
    let (Some(closure), Some(first)) = (uv.closure, uv.points.first().copied()) else { return Ok(poly) };
    let Some(&first_gid) = gids.first() else { return Ok(poly) };
    let vp = closure.v_pole;
    let (hu, hv) = metric_h;
    let pole = ctx.new_vertex(surface.point(first.x, vp))?;
    let meridian = (((vp - first.y).abs() / hv.max(1.0e-9)).ceil() as usize).clamp(1, 4096);
    let mut mer: Vec<u32> = Vec::with_capacity(meridian.saturating_sub(1));
    for k in 1..meridian {
        let v = first.y + (vp - first.y) * k as f64 / meridian as f64;
        mer.push(ctx.new_vertex(surface.point(first.x, v))?);
    }
    let right_u = closure.first_unwrapped.x;
    // Right side, from the unwrapped copy of the first point up the meridian to the pole.
    poly.push(PolyPt { uv: Vec2::new(right_u, first.y), gid: first_gid });
    for (k, &g) in mer.iter().enumerate() {
        let v = first.y + (vp - first.y) * (k + 1) as f64 / meridian as f64;
        poly.push(PolyPt { uv: Vec2::new(right_u, v), gid: g });
    }
    // The pole, seen from every longitude.
    let steps = ((right_u - first.x).abs() / hu.max(1.0e-3)).ceil() as usize;
    let steps = steps.clamp(2, 100_000);
    for k in 0..=steps {
        let u = right_u + (first.x - right_u) * k as f64 / steps as f64;
        poly.push(PolyPt { uv: Vec2::new(u, vp), gid: pole });
    }
    // Left side, back down the meridian (the first point itself closes the polygon).
    for (k, &g) in mer.iter().enumerate().rev() {
        let v = first.y + (vp - first.y) * (k + 1) as f64 / meridian as f64;
        poly.push(PolyPt { uv: Vec2::new(first.x, v), gid: g });
    }
    Ok(poly)
}

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).norm()
}

fn tessellate_face(ctx: &mut Ctx, f: FaceId) -> Result<()> {
    let brep = ctx.brep;
    let face = brep.face(f)?;
    let surface = &face.surface;
    // Boundary samples and their unwrapped parameters.
    let mut raw: Vec<(LoopUv, Vec<u32>, bool)> = Vec::new();
    let loops = brep.face_loops(f)?;
    for &l in &loops {
        let gids = ctx.loop_ids(l)?;
        let pts: Vec<Point3> = gids.iter().filter_map(|&g| ctx.mesh.vertices.get(g as usize).copied()).collect();
        let uv = loop_uv(surface, &pts, face.same_sense).map_err(|e| BrepError::Tessellation(format!("face {}: {e}", f.index())))?;
        raw.push((uv, gids, l == face.outer));
    }
    // A rough extent to choose the metric (closure detours extend it to the pole).
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for (uv, _, _) in &raw {
        for p in uv.polygon() {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
    }
    let m = metric(surface, ctx.chord, lo, hi)?;
    let mut polys: Vec<Vec<PolyPt>> = Vec::new();
    for (uv, gids, _) in &raw {
        let hu = m.hu.unwrap_or(FRAC_PI_4);
        polys.push(polygon(ctx, surface, uv, gids, (hu, m.hv))?);
    }
    // Shift every hole into the window of the outer loop on periodic surfaces.
    let (per_u, per_v) = surface.periods();
    if polys.len() > 1 {
        let outer_idx = raw.iter().position(|r| r.2).unwrap_or(0);
        let centre = |poly: &[PolyPt]| {
            let (lo, hi) = poly_bounds(poly);
            (lo + hi) * 0.5
        };
        let oc = polys.get(outer_idx).map_or(Vec2::ZERO, |p| centre(p));
        for (i, poly) in polys.iter_mut().enumerate() {
            if i == outer_idx {
                continue;
            }
            let hc = centre(poly);
            let shift = Vec2::new(per_u.map_or(0.0, |p| ((oc.x - hc.x) / p).round() * p), per_v.map_or(0.0, |p| ((oc.y - hc.y) / p).round() * p));
            for p in poly.iter_mut() {
                p.uv += shift;
            }
        }
    }
    let (lo, hi) = bounds(&polys);
    let scale = |uv: Vec2| Vec2::new(uv.x * m.sx, uv.y * m.sy);
    let scaled: Vec<Vec<Vec2>> = polys.iter().map(|p| p.iter().map(|q| scale(q.uv)).collect()).collect();
    // Interior points for curved surfaces: a grid kept clear of the boundary.
    let mut steiner: Vec<Vec2> = Vec::new();
    if let Some(hu) = m.hu {
        let span = hi - lo;
        let (cols, rows) = ((span.x / hu).ceil(), (span.y / m.hv).ceil());
        if cols * rows > MAX_GRID_POINTS as f64 {
            return Err(BrepError::Tessellation(format!(
                "face {}: the chord tolerance {:e} needs more than {MAX_GRID_POINTS} interior points",
                f.index(),
                ctx.chord
            )));
        }
        let margin = 0.3;
        let refs: Vec<&[Vec2]> = scaled.iter().map(Vec::as_slice).collect();
        let segments: Vec<(Vec2, Vec2)> =
            scaled.iter().flat_map(|poly| poly.iter().enumerate().map(|(i, &a)| (a, poly[(i + 1) % poly.len()]))).collect();
        for j in 0..rows as usize {
            for i in 0..cols as usize {
                let uv = Vec2::new(lo.x + hu * (i as f64 + 0.5), lo.y + m.hv * (j as f64 + 0.5));
                let s = scale(uv);
                if inside_even_odd(&refs, s)? && !segments.iter().any(|&(a, b)| dist_to_segment(s, a, b) < margin) {
                    steiner.push(uv);
                }
            }
        }
    }
    let (slo, shi) = (scale(lo), scale(hi));
    let mut cdt = Cdt::new(slo.to_array(), shi.to_array())?;
    let mut gid_of: Vec<u32> = Vec::new();
    let mut ring_starts: Vec<usize> = Vec::new();
    for poly in &polys {
        ring_starts.push(gid_of.len());
        for p in poly {
            let idx = cdt.insert(scale(p.uv).to_array())? as usize;
            if idx != gid_of.len() {
                return Err(BrepError::Tessellation(format!("face {}: two boundary samples coincide in parameter space", f.index())));
            }
            gid_of.push(p.gid);
        }
    }
    for s in &steiner {
        let idx = cdt.insert(scale(*s).to_array())? as usize;
        if idx != gid_of.len() {
            return Err(BrepError::Tessellation(format!("face {}: an interior point coincides with another point", f.index())));
        }
        gid_of.push(ctx.new_vertex(surface.point(s.x, s.y))?);
    }
    for (r, poly) in polys.iter().enumerate() {
        let base = ring_starts.get(r).copied().unwrap_or(0);
        let n = poly.len();
        for i in 0..n {
            cdt.insert_constraint((base + i) as u32, (base + (i + 1) % n) as u32)?;
        }
    }
    for t in cdt.finish()? {
        let g: Vec<u32> = t.iter().filter_map(|&i| gid_of.get(i as usize).copied()).collect();
        let [a, b, c] = g[..] else { return Err(BrepError::Tessellation("triangle index out of range".into())) };
        if a == b || b == c || c == a {
            continue; // collapsed onto a pole
        }
        ctx.mesh.triangles.push(if face.same_sense { [a, b, c] } else { [a, c, b] });
        ctx.owner.push(f);
    }
    Ok(())
}

/// A mesh and, for every triangle, the face it tessellates.
#[derive(Debug, Clone, PartialEq)]
pub struct Tessellation {
    pub mesh: Mesh,
    pub triangle_face: Vec<FaceId>,
}

/// Tessellate every face of the B-rep so that edge samples deviate at most `chord` (mm) from
/// their curves, into one mesh whose triangles face outward, remembering each triangle's face.
pub fn tessellate_faces(brep: &Brep, chord: f64) -> Result<Tessellation> {
    if !(chord.is_finite() && chord > 0.0) {
        return Err(BrepError::invalid("chord tolerance must be positive and finite"));
    }
    let mut ctx = Ctx { brep, chord, mesh: Mesh::default(), owner: Vec::new(), vertex_ids: HashMap::new(), edge_ids: HashMap::new() };
    for f in brep.face_ids() {
        tessellate_face(&mut ctx, f)?;
    }
    Ok(Tessellation { mesh: ctx.mesh, triangle_face: ctx.owner })
}

/// [`tessellate_faces`] without the triangle-to-face map.
pub fn tessellate(brep: &Brep, chord: f64) -> Result<Mesh> {
    Ok(tessellate_faces(brep, chord)?.mesh)
}
