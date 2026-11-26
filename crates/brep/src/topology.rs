// SPDX-License-Identifier: MIT OR Apache-2.0
//! The B-rep data structure: an arena of typed entities.
//!
//! ```text
//! Solid ── Shell ── Face ── Loop ── Coedge ── Edge ── Vertex
//!                     └─ surface    └─ orientation   └─ curve + parameter interval
//! ```
//!
//! * Entities live in `Vec`s inside [`Brep`] and refer to each other by typed ids
//!   ([`VertexId`], [`EdgeId`], ...). There are no reference cycles, no `Rc`, no `RefCell`; a
//!   `Brep` is `Clone` and `Send` by construction.
//! * A **coedge** is one use of an edge by a loop, with an orientation: `reversed == false`
//!   traverses the edge from its start vertex to its end vertex (increasing curve parameter).
//! * A **loop** is a closed, ordered cycle of coedges. A **face** has one outer loop and any
//!   number of inner loops (holes) on a surface.
//! * **Orientation convention.** A face's outward normal is the surface's natural normal when
//!   `same_sense` is true, the opposite otherwise. Looking down on a face from outside (against
//!   the outward normal), its outer loop runs counter-clockwise and its inner loops clockwise,
//!   so the material of the face is always on the left of a coedge. In a closed shell every edge
//!   is used by exactly two coedges with opposite orientation.
//! * A **shell** is a set of faces; a **solid** has an outer shell first, then void shells.
//!
//! Ids are only meaningful for the `Brep` that issued them: every accessor validates its id and
//! returns an error rather than indexing blindly.

use crate::curve::Curve;
use crate::error::{BrepError, Result};
use crate::math::Point3;
use crate::surface::Surface;

/// The most entities of one kind a `Brep` accepts (so ids fit in `u32` with room to spare).
pub const MAX_ENTITIES: usize = 1 << 26;

macro_rules! define_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u32);
        impl $name {
            /// The position in the arena (for use as an array index by the caller).
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}
define_id!(
    /// Identifies a [`Vertex`].
    VertexId
);
define_id!(
    /// Identifies an [`Edge`].
    EdgeId
);
define_id!(
    /// Identifies a [`Coedge`].
    CoedgeId
);
define_id!(
    /// Identifies a [`Loop`].
    LoopId
);
define_id!(
    /// Identifies a [`Face`].
    FaceId
);
define_id!(
    /// Identifies a [`Shell`].
    ShellId
);
define_id!(
    /// Identifies a [`Solid`].
    SolidId
);

#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    pub point: Point3,
}

/// A trimmed curve between two vertices (which may be the same vertex for a closed curve).
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub curve: Curve,
    /// The parameter interval `[t0, t1]` of `curve` the edge covers (`t0 < t1`).
    pub t0: f64,
    pub t1: f64,
    /// The vertex at `curve(t0)`.
    pub start: VertexId,
    /// The vertex at `curve(t1)`.
    pub end: VertexId,
}

/// One use of an edge in a loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coedge {
    pub edge: EdgeId,
    /// Traversed against the edge's own direction.
    pub reversed: bool,
    pub owner: LoopId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub coedges: Vec<CoedgeId>,
    /// Set when a face takes the loop.
    pub face: Option<FaceId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub surface: Surface,
    /// The face's outward normal is the surface's natural normal (true) or its opposite (false).
    pub same_sense: bool,
    pub outer: LoopId,
    pub inner: Vec<LoopId>,
    /// Set when a shell takes the face.
    pub shell: Option<ShellId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    pub faces: Vec<FaceId>,
    /// Set when a solid takes the shell.
    pub solid: Option<SolidId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    /// The outer shell first, then the shells of voids.
    pub shells: Vec<ShellId>,
}

/// The B-rep: all entities of one or more solids.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Brep {
    vertices: Vec<Vertex>,
    edges: Vec<Edge>,
    coedges: Vec<Coedge>,
    loops: Vec<Loop>,
    faces: Vec<Face>,
    shells: Vec<Shell>,
    solids: Vec<Solid>,
}

fn get<'a, T>(items: &'a [T], index: usize, what: &str) -> Result<&'a T> {
    items.get(index).ok_or_else(|| BrepError::invalid(format!("{what} id {index} does not exist in this B-rep ({} of them)", items.len())))
}

fn next_id(len: usize, what: &str) -> Result<u32> {
    if len >= MAX_ENTITIES {
        return Err(BrepError::OutOfRange(format!("more than {MAX_ENTITIES} {what}s")));
    }
    u32::try_from(len).map_err(|_| BrepError::OutOfRange(format!("more than {MAX_ENTITIES} {what}s")))
}

macro_rules! accessors {
    ($($get:ident, $count:ident, $ids:ident, $field:ident, $id:ident, $t:ident, $what:literal;)+) => {
        impl Brep {
            $(
                #[doc = concat!("The ", $what, " with this id, or an error if the id is not from this B-rep.")]
                pub fn $get(&self, id: $id) -> Result<&$t> {
                    get(&self.$field, id.index(), $what)
                }
                pub fn $count(&self) -> usize {
                    self.$field.len()
                }
                #[doc = concat!("The ids of every ", $what, ", in creation order.")]
                pub fn $ids(&self) -> impl Iterator<Item = $id> + '_ {
                    (0..self.$field.len()).map(|i| $id(i as u32))
                }
            )+
        }
    };
}
accessors! {
    vertex, vertex_count, vertex_ids, vertices, VertexId, Vertex, "vertex";
    edge, edge_count, edge_ids, edges, EdgeId, Edge, "edge";
    coedge, coedge_count, coedge_ids, coedges, CoedgeId, Coedge, "coedge";
    loop_, loop_count, loop_ids, loops, LoopId, Loop, "loop";
    face, face_count, face_ids, faces, FaceId, Face, "face";
    shell, shell_count, shell_ids, shells, ShellId, Shell, "shell";
    solid, solid_count, solid_ids, solids, SolidId, Solid, "solid";
}

impl Brep {
    pub fn new() -> Brep {
        Brep::default()
    }

    pub fn add_vertex(&mut self, point: Point3) -> Result<VertexId> {
        if !point.is_finite() {
            return Err(BrepError::non_finite("vertex coordinate"));
        }
        if point.x.abs().max(point.y.abs()).max(point.z.abs()) > crate::curve::MAX_SIZE {
            return Err(BrepError::OutOfRange("vertex coordinate beyond the supported extent".into()));
        }
        let id = VertexId(next_id(self.vertices.len(), "vertex")?);
        self.vertices.push(Vertex { point });
        Ok(id)
    }

    /// Add the edge of `curve` over `[t0, t1]` from `start` to `end`. The geometry is *not*
    /// checked against the vertices here; [`Brep::validate`] does that.
    pub fn add_edge(&mut self, curve: Curve, t0: f64, t1: f64, start: VertexId, end: VertexId) -> Result<EdgeId> {
        if !(t0.is_finite() && t1.is_finite() && t0 < t1) {
            return Err(BrepError::invalid(format!("edge parameter interval [{t0}, {t1}] must be finite with t0 < t1")));
        }
        self.vertex(start)?;
        self.vertex(end)?;
        let id = EdgeId(next_id(self.edges.len(), "edge")?);
        self.edges.push(Edge { curve, t0, t1, start, end });
        Ok(id)
    }

    /// Add a loop from `(edge, reversed)` uses in traversal order. Connectivity is checked by
    /// [`Brep::validate`], not here.
    pub fn add_loop(&mut self, uses: &[(EdgeId, bool)]) -> Result<LoopId> {
        if uses.is_empty() {
            return Err(BrepError::invalid("a loop needs at least one coedge"));
        }
        for (e, _) in uses {
            self.edge(*e)?;
        }
        let loop_id = LoopId(next_id(self.loops.len(), "loop")?);
        let mut ids = Vec::with_capacity(uses.len());
        for (edge, reversed) in uses {
            let id = CoedgeId(next_id(self.coedges.len(), "coedge")?);
            self.coedges.push(Coedge { edge: *edge, reversed: *reversed, owner: loop_id });
            ids.push(id);
        }
        self.loops.push(Loop { coedges: ids, face: None });
        Ok(loop_id)
    }

    /// Add a face on `surface`. The loops must not already belong to a face.
    pub fn add_face(&mut self, surface: Surface, same_sense: bool, outer: LoopId, inner: &[LoopId]) -> Result<FaceId> {
        let face_id = FaceId(next_id(self.faces.len(), "face")?);
        let mut all = vec![outer];
        all.extend_from_slice(inner);
        for (i, l) in all.iter().enumerate() {
            if self.loop_(*l)?.face.is_some() {
                return Err(BrepError::topology(format!("loop {} already belongs to a face", l.index())));
            }
            if all[..i].contains(l) {
                return Err(BrepError::topology(format!("loop {} listed twice", l.index())));
            }
        }
        for l in &all {
            if let Some(slot) = self.loops.get_mut(l.index()) {
                slot.face = Some(face_id);
            }
        }
        self.faces.push(Face { surface, same_sense, outer, inner: inner.to_vec(), shell: None });
        Ok(face_id)
    }

    pub fn add_shell(&mut self, faces: &[FaceId]) -> Result<ShellId> {
        if faces.is_empty() {
            return Err(BrepError::invalid("a shell needs at least one face"));
        }
        let id = ShellId(next_id(self.shells.len(), "shell")?);
        for (i, f) in faces.iter().enumerate() {
            if self.face(*f)?.shell.is_some() {
                return Err(BrepError::topology(format!("face {} already belongs to a shell", f.index())));
            }
            if faces[..i].contains(f) {
                return Err(BrepError::topology(format!("face {} listed twice", f.index())));
            }
        }
        for f in faces {
            if let Some(slot) = self.faces.get_mut(f.index()) {
                slot.shell = Some(id);
            }
        }
        self.shells.push(Shell { faces: faces.to_vec(), solid: None });
        Ok(id)
    }

    /// Add a solid from its shells (outer shell first).
    pub fn add_solid(&mut self, shells: &[ShellId]) -> Result<SolidId> {
        if shells.is_empty() {
            return Err(BrepError::invalid("a solid needs at least one shell"));
        }
        let id = SolidId(next_id(self.solids.len(), "solid")?);
        for (i, s) in shells.iter().enumerate() {
            if self.shell(*s)?.solid.is_some() {
                return Err(BrepError::topology(format!("shell {} already belongs to a solid", s.index())));
            }
            if shells[..i].contains(s) {
                return Err(BrepError::topology(format!("shell {} listed twice", s.index())));
            }
        }
        for s in shells {
            if let Some(slot) = self.shells.get_mut(s.index()) {
                slot.solid = Some(id);
            }
        }
        self.solids.push(Solid { shells: shells.to_vec() });
        Ok(id)
    }

    /// The vertex a coedge starts at, in its traversal direction.
    pub fn coedge_start(&self, c: CoedgeId) -> Result<VertexId> {
        let co = self.coedge(c)?;
        let e = self.edge(co.edge)?;
        Ok(if co.reversed { e.end } else { e.start })
    }
    /// The vertex a coedge ends at, in its traversal direction.
    pub fn coedge_end(&self, c: CoedgeId) -> Result<VertexId> {
        let co = self.coedge(c)?;
        let e = self.edge(co.edge)?;
        Ok(if co.reversed { e.start } else { e.end })
    }

    /// The coedges of every edge, indexed by edge: `result[e.index()]`.
    pub fn edge_uses(&self) -> Vec<Vec<CoedgeId>> {
        let mut uses = vec![Vec::new(); self.edges.len()];
        for (i, co) in self.coedges.iter().enumerate() {
            if let Some(list) = uses.get_mut(co.edge.index()) {
                list.push(CoedgeId(i as u32));
            }
        }
        uses
    }

    /// The loops of a face, outer first.
    pub fn face_loops(&self, f: FaceId) -> Result<Vec<LoopId>> {
        let face = self.face(f)?;
        let mut v = vec![face.outer];
        v.extend_from_slice(&face.inner);
        Ok(v)
    }

    /// The point of an edge at parameter `t` on its curve.
    pub fn edge_point(&self, e: EdgeId, t: f64) -> Result<Point3> {
        Ok(self.edge(e)?.curve.point(t))
    }

    /// The bounding box of the vertices (`None` when there are none).
    pub fn vertex_bounds(&self) -> Option<(Point3, Point3)> {
        let mut it = self.vertices.iter().map(|v| v.point);
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| {
            (Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z)), Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z)))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::Line;
    use crate::math::Vec3;

    fn line_edge(b: &mut Brep, a: Vec3, c: Vec3) -> (EdgeId, VertexId, VertexId) {
        let va = b.add_vertex(a).unwrap();
        let vc = b.add_vertex(c).unwrap();
        let (line, len) = Line::through(a, c).unwrap();
        (b.add_edge(Curve::Line(line), 0.0, len, va, vc).unwrap(), va, vc)
    }

    #[test]
    fn ids_are_validated_on_every_use() {
        let mut b = Brep::new();
        let other = {
            let mut o = Brep::new();
            for i in 0..8 {
                o.add_vertex(Vec3::new(i as f64, 0.0, 0.0)).unwrap();
            }
            o.vertex_ids().nth(7).unwrap()
        };
        assert!(b.vertex(other).is_err());
        let (e, _, _) = line_edge(&mut b, Vec3::ZERO, Vec3::X);
        assert!(b.edge(e).is_ok());
        assert!(b.add_edge(Curve::Line(Line::new(Vec3::ZERO, Vec3::X).unwrap()), 0.0, 1.0, other, other).is_err());
        assert!(b.add_loop(&[(EdgeId(99), false)]).is_err());
        assert!(b.coedge_start(CoedgeId(5)).is_err());
    }

    #[test]
    fn rejects_bad_vertices_and_intervals() {
        let mut b = Brep::new();
        assert!(b.add_vertex(Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
        assert!(b.add_vertex(Vec3::new(1e12, 0.0, 0.0)).is_err());
        let v = b.add_vertex(Vec3::ZERO).unwrap();
        let line = Curve::Line(Line::new(Vec3::ZERO, Vec3::X).unwrap());
        assert!(b.add_edge(line.clone(), 1.0, 1.0, v, v).is_err());
        assert!(b.add_edge(line.clone(), 2.0, 1.0, v, v).is_err());
        assert!(b.add_edge(line, 0.0, f64::INFINITY, v, v).is_err());
        assert!(b.add_loop(&[]).is_err());
        assert!(b.add_shell(&[]).is_err());
        assert!(b.add_solid(&[]).is_err());
    }

    #[test]
    fn ownership_is_exclusive() {
        let mut b = Brep::new();
        let (e, _, _) = line_edge(&mut b, Vec3::ZERO, Vec3::X);
        let l = b.add_loop(&[(e, false), (e, true)]).unwrap();
        let s = Surface::Plane(crate::surface::Plane::new(crate::math::Frame::WORLD).unwrap());
        let f = b.add_face(s.clone(), true, l, &[]).unwrap();
        assert!(b.add_face(s.clone(), true, l, &[]).is_err());
        let l2 = b.add_loop(&[(e, false)]).unwrap();
        assert!(b.add_face(s, true, l2, &[l2]).is_err());
        let sh = b.add_shell(&[f]).unwrap();
        assert!(b.add_shell(&[f]).is_err());
        let so = b.add_solid(&[sh]).unwrap();
        assert!(b.add_solid(&[sh]).is_err());
        assert_eq!(b.shell(sh).unwrap().solid, Some(so));
        assert_eq!(b.loop_(l).unwrap().face, Some(f));
    }

    #[test]
    fn edge_uses_and_coedge_ends_follow_orientation() {
        let mut b = Brep::new();
        let (e, va, vc) = line_edge(&mut b, Vec3::ZERO, Vec3::X);
        let l = b.add_loop(&[(e, false), (e, true)]).unwrap();
        let cos = b.loop_(l).unwrap().coedges.clone();
        assert_eq!(b.coedge_start(cos[0]).unwrap(), va);
        assert_eq!(b.coedge_end(cos[0]).unwrap(), vc);
        assert_eq!(b.coedge_start(cos[1]).unwrap(), vc);
        assert_eq!(b.edge_uses()[e.index()], cos);
        assert_eq!(b.vertex_bounds().unwrap(), (Vec3::ZERO, Vec3::X));
        assert!(Brep::new().vertex_bounds().is_none());
    }
}
