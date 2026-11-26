// SPDX-License-Identifier: MIT OR Apache-2.0
//! The validity checker: [`Brep::validate`].
//!
//! It never stops at the first problem and never panics: every failed check becomes a
//! [`Finding`] in the [`Report`], and a check that cannot be made (a NURBS face, a loop through a
//! pole, a face too large to test for crossings) is a [`Severity::Note`] saying what was *not*
//! verified. A B-rep is valid when there is no [`Severity::Error`].
//!
//! What is checked:
//!
//! 1. **Ownership**: every coedge, loop, face and shell is held by exactly the container that
//!    claims it, and every face is in a shell and every shell in a solid.
//! 2. **Vertices**: finite, no two within the linear tolerance of each other.
//! 3. **Edges**: the curve at `t0` and `t1` is at the start and end vertex within tolerance, the
//!    edge has length, it does not wrap its (periodic) curve more than once, and an edge joining
//!    two distinct vertices is not closed in space.
//! 4. **Edge use**: every edge is used by exactly two coedges, in opposite directions, and both
//!    belong to faces of the same shell.
//! 5. **Loops**: each coedge ends at the vertex the next one starts at, and the last at the first.
//! 6. **Faces**: sampled edge points lie on the face's surface within tolerance; each loop
//!    unwraps to a closed polygon in `(u, v)`; the outer loop runs counter-clockwise when the face
//!    sense is true and clockwise when false, holes the other way; holes are inside the outer loop;
//!    no two loops of a face cross.
//! 7. **Shells**: the faces are connected through shared edges and the Euler–Poincaré relation
//!    `V − E + F − R = 2 − 2g` holds with an integer genus `g ≥ 0` (`R` the number of holes).
//! 8. **Solids**: the outer shell encloses positive volume and each void shell negative volume,
//!    computed with [`crate::mass`].
//!
//! Not checked (and reported as notes where it matters): that distinct faces do not intersect
//! each other, that void shells lie inside the outer shell, faces on NURBS surfaces.

use crate::error::BrepError;
use crate::math::{Vec2, Vec3};
use crate::robust::segments_cross_properly;
use crate::surface::Surface;
use crate::tol::Tolerance;
use crate::topology::{Brep, CoedgeId, EdgeId, FaceId, LoopId, ShellId};
use crate::uv::{LoopUv, inside_even_odd, loop_points, loop_uv, polygon_area};
use std::collections::{HashMap, HashSet};

/// Chord tolerance used to cut edges into sample points for the face checks.
const CHECK_CHORD: f64 = 1.0e-2;
/// Most segment pairs tested for crossings in one face.
const MAX_CROSSING_PAIRS: usize = 8_000_000;
/// Samples per edge for the length and wrap checks.
const EDGE_SAMPLES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The B-rep is invalid.
    Error,
    /// Something that could not be verified, or is worth knowing.
    Note,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

/// Counts and invariants of one shell.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellStats {
    pub shell: ShellId,
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
    /// Inner loops (holes) over all faces.
    pub holes: usize,
    pub components: usize,
    /// `V − E + F − R`.
    pub euler: i64,
    /// `(2 − χ)/2` when `χ` is even, `≤ 2` and the shell is connected.
    pub genus: Option<i64>,
    /// The enclosed volume (positive for an outer shell, negative for a void), when it could be computed.
    pub volume: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub shells: Vec<ShellStats>,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Error)
    }
    pub fn notes(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Note)
    }
    /// No errors (notes are allowed).
    pub fn is_valid(&self) -> bool {
        self.errors().next().is_none()
    }
    fn error(&mut self, message: impl Into<String>) {
        self.findings.push(Finding { severity: Severity::Error, message: message.into() });
    }
    fn note(&mut self, message: impl Into<String>) {
        self.findings.push(Finding { severity: Severity::Note, message: message.into() });
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for x in &self.findings {
            let tag = if x.severity == Severity::Error { "error" } else { "note" };
            writeln!(f, "{tag}: {}", x.message)?;
        }
        Ok(())
    }
}

impl Brep {
    /// Check every invariant listed in the [module documentation](crate::check).
    pub fn validate(&self, tol: &Tolerance) -> Report {
        let mut r = Report::default();
        if self.solid_count() == 0 {
            r.error("the B-rep has no solid");
        }
        check_ownership(self, &mut r);
        check_vertices(self, tol, &mut r);
        check_edges(self, tol, &mut r);
        check_edge_use(self, &mut r);
        check_loops(self, &mut r);
        for f in self.face_ids() {
            check_face(self, f, tol, &mut r);
        }
        for s in self.shell_ids() {
            check_shell(self, s, &mut r);
        }
        check_solids(self, &mut r);
        r
    }
}

fn check_ownership(b: &Brep, r: &mut Report) {
    for c in b.coedge_ids() {
        let Ok(co) = b.coedge(c) else { continue };
        match b.loop_(co.owner) {
            Ok(l) if l.coedges.contains(&c) => {}
            _ => r.error(format!("coedge {} is not in the loop {} that claims it", c.index(), co.owner.index())),
        }
    }
    for l in b.loop_ids() {
        let Ok(lp) = b.loop_(l) else { continue };
        match lp.face {
            None => r.error(format!("loop {} belongs to no face", l.index())),
            Some(f) => {
                if !b.face_loops(f).is_ok_and(|ls| ls.contains(&l)) {
                    r.error(format!("loop {} says it is in face {}, which does not list it", l.index(), f.index()));
                }
            }
        }
    }
    for f in b.face_ids() {
        let Ok(face) = b.face(f) else { continue };
        match face.shell {
            None => r.error(format!("face {} belongs to no shell", f.index())),
            Some(s) => {
                if !b.shell(s).is_ok_and(|sh| sh.faces.contains(&f)) {
                    r.error(format!("face {} says it is in shell {}, which does not list it", f.index(), s.index()));
                }
            }
        }
    }
    for s in b.shell_ids() {
        let Ok(sh) = b.shell(s) else { continue };
        match sh.solid {
            None => r.error(format!("shell {} belongs to no solid", s.index())),
            Some(so) => {
                if !b.solid(so).is_ok_and(|x| x.shells.contains(&s)) {
                    r.error(format!("shell {} says it is in solid {}, which does not list it", s.index(), so.index()));
                }
            }
        }
    }
}

fn check_vertices(b: &Brep, tol: &Tolerance, r: &mut Report) {
    let cell = (2.0 * tol.linear).max(f64::MIN_POSITIVE);
    let key = |p: Vec3| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64, i64), Vec<(usize, Vec3)>> = HashMap::new();
    let mut used: HashSet<usize> = HashSet::new();
    for e in b.edge_ids() {
        if let Ok(edge) = b.edge(e) {
            used.insert(edge.start.index());
            used.insert(edge.end.index());
        }
    }
    for v in b.vertex_ids() {
        let Ok(vx) = b.vertex(v) else { continue };
        let p = vx.point;
        if !p.is_finite() {
            r.error(format!("vertex {} is not finite", v.index()));
            continue;
        }
        if !used.contains(&v.index()) {
            r.error(format!("vertex {} is used by no edge", v.index()));
        }
        let (kx, ky, kz) = key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&(kx + dx, ky + dy, kz + dz)) {
                        for &(o, q) in list {
                            if tol.same_point(p, q) {
                                r.error(format!("vertices {o} and {} are the same point within {:e}", v.index(), tol.linear));
                            }
                        }
                    }
                }
            }
        }
        grid.entry((kx, ky, kz)).or_default().push((v.index(), p));
    }
}

fn check_edges(b: &Brep, tol: &Tolerance, r: &mut Report) {
    for e in b.edge_ids() {
        let Ok(edge) = b.edge(e) else { continue };
        let (Ok(vs), Ok(ve)) = (b.vertex(edge.start), b.vertex(edge.end)) else { continue };
        let (p0, p1) = (edge.curve.point(edge.t0), edge.curve.point(edge.t1));
        if !(p0.is_finite() && p1.is_finite()) {
            r.error(format!("edge {} has a non-finite end point", e.index()));
            continue;
        }
        if p0.distance(vs.point) > tol.linear {
            r.error(format!("edge {} starts {:.3e} away from its start vertex", e.index(), p0.distance(vs.point)));
        }
        if p1.distance(ve.point) > tol.linear {
            r.error(format!("edge {} ends {:.3e} away from its end vertex", e.index(), p1.distance(ve.point)));
        }
        let mut length = 0.0;
        let mut prev = p0;
        for k in 1..=EDGE_SAMPLES {
            let t = edge.t0 + (edge.t1 - edge.t0) * k as f64 / EDGE_SAMPLES as f64;
            let p = edge.curve.point(t);
            length += prev.distance(p);
            prev = p;
        }
        if length <= tol.linear {
            r.error(format!("edge {} has zero length", e.index()));
        }
        if let Some(period) = edge.curve.period()
            && edge.t1 - edge.t0 > period + 1e-9
        {
            r.error(format!("edge {} covers more than one turn of its curve", e.index()));
        }
        if edge.start != edge.end && p0.distance(p1) <= tol.linear {
            r.error(format!("edge {} is closed in space but joins two different vertices", e.index()));
        }
    }
}

fn check_edge_use(b: &Brep, r: &mut Report) {
    let uses = b.edge_uses();
    for (i, list) in uses.iter().enumerate() {
        match list.as_slice() {
            [] => r.error(format!("edge {i} is used by no coedge")),
            [a, c] => {
                let (Ok(ca), Ok(cc)) = (b.coedge(*a), b.coedge(*c)) else { continue };
                if ca.reversed == cc.reversed {
                    r.error(format!("edge {i} is used twice in the same direction"));
                }
            }
            other => r.error(format!("edge {i} is used by {} coedges (a closed solid needs exactly 2)", other.len())),
        }
    }
}

fn check_loops(b: &Brep, r: &mut Report) {
    for l in b.loop_ids() {
        let Ok(lp) = b.loop_(l) else { continue };
        let n = lp.coedges.len();
        for (i, &c) in lp.coedges.iter().enumerate() {
            let next = lp.coedges[(i + 1) % n];
            if let (Ok(end), Ok(start)) = (b.coedge_end(c), b.coedge_start(next))
                && end != start
            {
                r.error(format!(
                    "loop {} is not closed: coedge {} ends at vertex {} but the next starts at vertex {}",
                    l.index(),
                    c.index(),
                    end.index(),
                    start.index()
                ));
            }
        }
    }
}

/// A face loop unwrapped into parameter space.
struct Unwrapped {
    loop_id: LoopId,
    uv: LoopUv,
}

fn check_face(b: &Brep, f: FaceId, tol: &Tolerance, r: &mut Report) {
    let Ok(face) = b.face(f) else { return };
    let loops = match b.face_loops(f) {
        Ok(l) => l,
        Err(e) => {
            r.error(format!("face {}: {e}", f.index()));
            return;
        }
    };
    if matches!(face.surface, Surface::Nurbs(_)) {
        r.note(format!("face {} is on a NURBS surface: edge containment, orientation and hole nesting are not verified", f.index()));
        return;
    }
    let mut unwrapped: Vec<Unwrapped> = Vec::new();
    for &l in &loops {
        let pts = match loop_points(b, l, CHECK_CHORD) {
            Ok(p) => p,
            Err(e) => {
                r.error(format!("face {}: loop {} cannot be sampled: {e}", f.index(), l.index()));
                continue;
            }
        };
        let mut worst = 0.0f64;
        for p in &pts {
            match face.surface.closest_point(*p) {
                Ok(pr) => worst = worst.max(pr.distance),
                Err(e) => {
                    r.error(format!("face {}: {e}", f.index()));
                    break;
                }
            }
        }
        if worst > tol.linear {
            r.error(format!("face {}: loop {} leaves the surface by up to {:.3e}", f.index(), l.index(), worst));
        }
        match loop_uv(&face.surface, &pts, face.same_sense) {
            Ok(uv) => unwrapped.push(Unwrapped { loop_id: l, uv }),
            Err(e @ BrepError::Unsupported(_)) => r.note(format!("face {}: loop {} could not be verified: {e}", f.index(), l.index())),
            Err(e) => r.error(format!("face {}: loop {}: {e}", f.index(), l.index())),
        }
    }
    let Some(outer) = unwrapped.iter().find(|u| u.loop_id == face.outer) else { return };
    let outer_poly = outer.uv.polygon();
    let outer_area = polygon_area(&outer_poly);
    if outer_area == 0.0 || !outer_area.is_finite() {
        r.error(format!("face {}: outer loop encloses no area in parameter space", f.index()));
        return;
    }
    if (outer_area > 0.0) != face.same_sense {
        r.error(format!(
            "face {}: outer loop runs {} but the face sense is {} (viewed from outside it must run counter-clockwise)",
            f.index(),
            if outer_area > 0.0 { "counter-clockwise in (u, v)" } else { "clockwise in (u, v)" },
            face.same_sense
        ));
    }
    let (per_u, per_v) = face.surface.periods();
    let center = |poly: &[Vec2]| {
        let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
        for p in poly {
            lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        (lo + hi) * 0.5
    };
    let oc = center(&outer_poly);
    let mut polys: Vec<Vec<Vec2>> = vec![outer_poly.clone()];
    for u in unwrapped.iter().filter(|u| u.loop_id != face.outer) {
        let mut poly = u.uv.polygon();
        let area = polygon_area(&poly);
        if area == 0.0 {
            r.error(format!("face {}: hole loop {} encloses no area in parameter space", f.index(), u.loop_id.index()));
            continue;
        }
        if (area > 0.0) == (outer_area > 0.0) {
            r.error(format!("face {}: hole loop {} runs the same way as the outer loop", f.index(), u.loop_id.index()));
        }
        let hc = center(&poly);
        let shift = Vec2::new(per_u.map_or(0.0, |p| ((oc.x - hc.x) / p).round() * p), per_v.map_or(0.0, |p| ((oc.y - hc.y) / p).round() * p));
        for p in &mut poly {
            *p += shift;
        }
        if let Some(&p0) = poly.first() {
            match inside_even_odd(&[&outer_poly], p0) {
                Ok(true) => {}
                Ok(false) => r.error(format!("face {}: hole loop {} is not inside the outer loop", f.index(), u.loop_id.index())),
                Err(e) => r.error(format!("face {}: {e}", f.index())),
            }
        }
        polys.push(poly);
    }
    let segments: usize = polys.iter().map(Vec::len).sum();
    if segments.saturating_mul(segments) / 2 > MAX_CROSSING_PAIRS {
        r.note(format!("face {}: too many boundary samples ({segments}) to test its loops for crossings", f.index()));
        return;
    }
    for (i, a) in polys.iter().enumerate() {
        for (j, c) in polys.iter().enumerate().skip(i) {
            match crossing(a, c, i == j) {
                Ok(true) => r.error(format!("face {}: its boundary loops cross each other or themselves", f.index())),
                Ok(false) => {}
                Err(e) => r.error(format!("face {}: {e}", f.index())),
            }
        }
    }
}

fn crossing(a: &[Vec2], c: &[Vec2], same: bool) -> crate::error::Result<bool> {
    let (na, nc) = (a.len(), c.len());
    for i in 0..na {
        let (p, q) = (a[i], a[(i + 1) % na]);
        for j in 0..nc {
            if same && (j <= i || j == (i + 1) % na || (j + 1) % nc == i) {
                continue;
            }
            let (s, t) = (c[j], c[(j + 1) % nc]);
            if segments_cross_properly(p.to_array(), q.to_array(), s.to_array(), t.to_array())? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut root = x;
    while let Some(&p) = parent.get(root) {
        if p == root {
            break;
        }
        root = p;
    }
    let mut cur = x;
    while let Some(&p) = parent.get(cur) {
        if p == root {
            break;
        }
        if let Some(slot) = parent.get_mut(cur) {
            *slot = root;
        }
        cur = p;
    }
    root
}

fn check_shell(b: &Brep, s: ShellId, r: &mut Report) {
    let Ok(shell) = b.shell(s) else { return };
    let face_pos: HashMap<FaceId, usize> = shell.faces.iter().enumerate().map(|(i, &f)| (f, i)).collect();
    let mut edges: HashSet<EdgeId> = HashSet::new();
    let mut verts: HashSet<usize> = HashSet::new();
    let mut holes = 0;
    let mut edge_faces: HashMap<EdgeId, Vec<usize>> = HashMap::new();
    for (fi, &f) in shell.faces.iter().enumerate() {
        let Ok(face) = b.face(f) else { continue };
        holes += face.inner.len();
        for l in b.face_loops(f).unwrap_or_default() {
            let Ok(lp) = b.loop_(l) else { continue };
            for &c in &lp.coedges {
                let Ok(co) = b.coedge(c) else { continue };
                edges.insert(co.edge);
                edge_faces.entry(co.edge).or_default().push(fi);
                if let Ok(e) = b.edge(co.edge) {
                    verts.insert(e.start.index());
                    verts.insert(e.end.index());
                }
            }
        }
    }
    let all_uses = b.edge_uses();
    let mut parent: Vec<usize> = (0..shell.faces.len()).collect();
    for (e, fs) in &edge_faces {
        if let Some((&first, rest)) = fs.split_first() {
            for &o in rest {
                let (ra, rb) = (find(&mut parent, first), find(&mut parent, o));
                if let Some(slot) = parent.get_mut(ra) {
                    *slot = rb;
                }
            }
        }
        // Both uses of an edge must be in this shell.
        let uses: &[CoedgeId] = all_uses.get(e.index()).map_or(&[], Vec::as_slice);
        for &c in uses {
            let in_shell = b.coedge(c).ok().and_then(|co| b.loop_(co.owner).ok()).and_then(|l| l.face).is_some_and(|f| face_pos.contains_key(&f));
            if !in_shell {
                r.error(format!("edge {} of shell {} is also used by a face outside the shell", e.index(), s.index()));
            }
        }
    }
    let roots: HashSet<usize> = (0..shell.faces.len()).map(|i| find(&mut parent, i)).collect();
    let components = roots.len();
    let euler = verts.len() as i64 - edges.len() as i64 + shell.faces.len() as i64 - holes as i64;
    let mut genus = None;
    if components != 1 {
        r.error(format!("shell {} is in {} disconnected pieces", s.index(), components));
    } else if euler > 2 || euler % 2 != 0 {
        r.error(format!("shell {} violates Euler–Poincaré: V − E + F − R = {} is not 2 − 2g for a genus g ≥ 0", s.index(), euler));
    } else {
        genus = Some((2 - euler) / 2);
    }
    let volume = match crate::mass::shell_volume(b, s) {
        Ok(v) => Some(v),
        Err(BrepError::Unsupported(m)) => {
            r.note(format!("shell {}: volume not computed ({m})", s.index()));
            None
        }
        Err(e) => {
            r.error(format!("shell {}: volume could not be computed: {e}", s.index()));
            None
        }
    };
    r.shells.push(ShellStats {
        shell: s,
        vertices: verts.len(),
        edges: edges.len(),
        faces: shell.faces.len(),
        holes,
        components,
        euler,
        genus,
        volume,
    });
}

fn check_solids(b: &Brep, r: &mut Report) {
    for so in b.solid_ids() {
        let Ok(solid) = b.solid(so) else { continue };
        for (i, &s) in solid.shells.iter().enumerate() {
            let Some(stats) = r.shells.iter().find(|x| x.shell == s) else { continue };
            let Some(v) = stats.volume else { continue };
            if i == 0 && !(v > 0.0) {
                r.error(format!(
                    "solid {}: its outer shell {} encloses volume {v:.6e} (faces point inward or the shell is degenerate)",
                    so.index(),
                    s.index()
                ));
            }
            if i > 0 && !(v < 0.0) {
                r.error(format!(
                    "solid {}: void shell {} has volume {v:.6e}; a void's faces must point into the void (negative volume)",
                    so.index(),
                    s.index()
                ));
            }
        }
        if solid.shells.len() > 1 {
            r.note(format!("solid {}: that the void shells lie inside the outer shell is not verified", so.index()));
        }
    }
}
