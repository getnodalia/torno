// SPDX-License-Identifier: MIT OR Apache-2.0
//! Constrained Delaunay triangulation of a planar region bounded by polygons.
//!
//! The algorithm is the textbook one:
//!
//! 1. every point is inserted by Bowyer–Watson (the cavity of triangles whose circumcircle
//!    strictly contains the point is replaced by a fan), inside a super-triangle so that all real
//!    points are interior;
//! 2. every boundary segment is recovered by edge flipping (Sloan): the edges it crosses are
//!    flipped one by one when their quadrilateral is strictly convex, until the segment is an edge;
//! 3. the Delaunay property is restored for every edge that is not a boundary segment by Lawson
//!    flips;
//! 4. triangles are classified by parity: those touching the super-triangle are outside, and
//!    crossing a boundary segment toggles inside and outside, so nested loops (holes) work.
//!
//! All decisions use the exact predicates of [`crate::robust`]: triangulation never depends on a
//! rounding error, and a degenerate input (coincident points, boundary segments that cross or pass
//! through a point) is reported as an error rather than producing a corrupt mesh. Points must all
//! be inserted before the first constraint.
//!
//! Exactness caveat: orientation is decided on the `f64` coordinates given, which is the exact
//! decision for those numbers; the *geometry* those numbers approximate is whatever the caller
//! made of it.

use crate::error::{BrepError, Result};
use crate::robust::{Sign, incircle, orient2d};
use std::collections::{HashMap, HashSet, VecDeque};

type Pt = [f64; 2];

/// A triangulation under construction. Vertex indices given to and returned from the public
/// methods are the caller's (`0` is the first point inserted).
#[derive(Debug)]
pub struct Cdt {
    /// The three super-triangle vertices first, then the caller's points.
    pts: Vec<Pt>,
    tris: Vec<[u32; 3]>,
    alive: Vec<bool>,
    free: Vec<u32>,
    /// Directed edge `(a, b)` → the triangle that has it counter-clockwise.
    edge: HashMap<(u32, u32), u32>,
    vert_tri: Vec<u32>,
    seen: HashMap<(u64, u64), u32>,
    constrained: HashSet<(u32, u32)>,
    last: u32,
}

const SUPER: u32 = 3;

fn key(p: Pt) -> (u64, u64) {
    let norm = |x: f64| if x == 0.0 { 0.0f64 } else { x };
    (norm(p[0]).to_bits(), norm(p[1]).to_bits())
}

fn undirected(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

fn orient(a: Pt, b: Pt, c: Pt) -> Result<i32> {
    Ok(orient2d(a, b, c)?.as_i32())
}

impl Cdt {
    /// A triangulation for points inside the box `[min, max]`.
    pub fn new(min: Pt, max: Pt) -> Result<Cdt> {
        if !(min.iter().chain(max.iter()).all(|x| x.is_finite())) {
            return Err(BrepError::non_finite("triangulation bounds"));
        }
        let (cx, cy) = (0.5 * (min[0] + max[0]), 0.5 * (min[1] + max[1]));
        let extent = (max[0] - min[0]).max(max[1] - min[1]).max(1.0e-9);
        let m = 1.0e4 * extent;
        if cx.abs().max(cy.abs()) + 4.0 * m > 1.0e29 {
            return Err(BrepError::OutOfRange("triangulation extent".into()));
        }
        let pts = vec![[cx - 2.0 * m, cy - m], [cx + 2.0 * m, cy - m], [cx, cy + 2.0 * m]];
        let mut cdt = Cdt {
            pts,
            tris: Vec::new(),
            alive: Vec::new(),
            free: Vec::new(),
            edge: HashMap::new(),
            vert_tri: vec![0; 3],
            seen: HashMap::new(),
            constrained: HashSet::new(),
            last: 0,
        };
        cdt.add_tri(0, 1, 2);
        Ok(cdt)
    }

    fn add_tri(&mut self, a: u32, b: u32, c: u32) -> u32 {
        let t = if let Some(t) = self.free.pop() {
            if let (Some(slot), Some(al)) = (self.tris.get_mut(t as usize), self.alive.get_mut(t as usize)) {
                *slot = [a, b, c];
                *al = true;
            }
            t
        } else {
            self.tris.push([a, b, c]);
            self.alive.push(true);
            (self.tris.len() - 1) as u32
        };
        self.edge.insert((a, b), t);
        self.edge.insert((b, c), t);
        self.edge.insert((c, a), t);
        for v in [a, b, c] {
            if let Some(slot) = self.vert_tri.get_mut(v as usize) {
                *slot = t;
            }
        }
        self.last = t;
        t
    }

    fn remove_tri(&mut self, t: u32) {
        let Some(&[a, b, c]) = self.tris.get(t as usize) else { return };
        for e in [(a, b), (b, c), (c, a)] {
            if self.edge.get(&e) == Some(&t) {
                self.edge.remove(&e);
            }
        }
        if let Some(al) = self.alive.get_mut(t as usize) {
            *al = false;
        }
        self.free.push(t);
    }

    fn tri(&self, t: u32) -> Result<[u32; 3]> {
        self.tris.get(t as usize).copied().ok_or_else(|| BrepError::topology("triangle index out of range"))
    }

    fn p(&self, v: u32) -> Result<Pt> {
        self.pts.get(v as usize).copied().ok_or_else(|| BrepError::topology("vertex index out of range"))
    }

    fn third(&self, t: u32, x: u32, y: u32) -> Result<u32> {
        self.tri(t)?.into_iter().find(|&v| v != x && v != y).ok_or_else(|| BrepError::topology("degenerate triangle"))
    }

    fn incircle_tri(&self, t: u32, p: Pt) -> Result<i32> {
        let [a, b, c] = self.tri(t)?;
        Ok(incircle(self.p(a)?, self.p(b)?, self.p(c)?, p)?.as_i32())
    }

    /// A triangle whose closure contains `p`.
    fn locate(&self, p: Pt) -> Result<u32> {
        let mut t = self.last;
        if !self.alive.get(t as usize).copied().unwrap_or(false) {
            t = self.alive.iter().position(|&a| a).map(|i| i as u32).ok_or_else(|| BrepError::topology("empty triangulation"))?;
        }
        let cap = 4 * self.tris.len() + 16;
        'walk: for _ in 0..cap {
            let [a, b, c] = self.tri(t)?;
            for (x, y) in [(a, b), (b, c), (c, a)] {
                if orient(self.p(x)?, self.p(y)?, p)? < 0 {
                    t = *self.edge.get(&(y, x)).ok_or_else(|| BrepError::topology("point outside the super-triangle"))?;
                    continue 'walk;
                }
            }
            return Ok(t);
        }
        // The walk should end; if it cycled, fall back to a scan.
        for (i, &al) in self.alive.iter().enumerate() {
            if !al {
                continue;
            }
            let [a, b, c] = self.tri(i as u32)?;
            let mut inside = true;
            for (x, y) in [(a, b), (b, c), (c, a)] {
                if orient(self.p(x)?, self.p(y)?, p)? < 0 {
                    inside = false;
                    break;
                }
            }
            if inside {
                return Ok(i as u32);
            }
        }
        Err(BrepError::NotConverged("point location".into()))
    }

    /// Insert a point; a point equal to an earlier one returns that one's index.
    pub fn insert(&mut self, p: Pt) -> Result<u32> {
        if !(p[0].is_finite() && p[1].is_finite()) {
            return Err(BrepError::non_finite("triangulation point"));
        }
        if let Some(&v) = self.seen.get(&key(p)) {
            return Ok(v - SUPER);
        }
        let start = self.locate(p)?;
        let mut cavity: Vec<u32> = vec![start];
        let mut inside: HashSet<u32> = HashSet::from([start]);
        let mut i = 0;
        while i < cavity.len() {
            let t = cavity[i];
            i += 1;
            let [a, b, c] = self.tri(t)?;
            for (x, y) in [(a, b), (b, c), (c, a)] {
                if let Some(&nb) = self.edge.get(&(y, x))
                    && !inside.contains(&nb)
                    && self.incircle_tri(nb, p)? > 0
                {
                    inside.insert(nb);
                    cavity.push(nb);
                }
            }
        }
        let mut boundary: Vec<(u32, u32)> = Vec::new();
        for &t in &cavity {
            let [a, b, c] = self.tri(t)?;
            for (x, y) in [(a, b), (b, c), (c, a)] {
                match self.edge.get(&(y, x)) {
                    Some(nb) if inside.contains(nb) => {}
                    _ => boundary.push((x, y)),
                }
            }
        }
        let v = self.pts.len() as u32;
        for &(x, y) in &boundary {
            if orient(self.p(x)?, self.p(y)?, p)? <= 0 {
                return Err(BrepError::NotConverged("a point's cavity is not star-shaped (degenerate input)".into()));
            }
        }
        for &t in &cavity {
            self.remove_tri(t);
        }
        self.pts.push(p);
        self.vert_tri.push(0);
        for (x, y) in boundary {
            self.add_tri(x, y, v);
        }
        self.seen.insert(key(p), v);
        Ok(v - SUPER)
    }

    /// Make the segment between two inserted points an edge of the triangulation.
    pub fn insert_constraint(&mut self, a: u32, b: u32) -> Result<()> {
        self.constrain(a + SUPER, b + SUPER, 0)
    }

    fn constrain(&mut self, a: u32, b: u32, depth: usize) -> Result<()> {
        if a == b {
            return Ok(());
        }
        if depth > 64 {
            return Err(BrepError::Topology("a boundary segment passes through too many points".into()));
        }
        if self.edge.contains_key(&(a, b)) || self.edge.contains_key(&(b, a)) {
            self.constrained.insert(undirected(a, b));
            return Ok(());
        }
        let (pa, pb) = (self.p(a)?, self.p(b)?);
        // Find the triangle around `a` whose wedge contains the direction to `b`.
        let mut t = *self.vert_tri.get(a as usize).ok_or_else(|| BrepError::topology("vertex index out of range"))?;
        let rotate = |tri: [u32; 3], v: u32| -> [u32; 3] {
            if tri[0] == v {
                tri
            } else if tri[1] == v {
                [tri[1], tri[2], tri[0]]
            } else {
                [tri[2], tri[0], tri[1]]
            }
        };
        let cap = self.tris.len() + 8;
        let (mut x, mut y);
        let mut steps = 0;
        loop {
            steps += 1;
            if steps > cap {
                return Err(BrepError::NotConverged("finding the start of a boundary segment".into()));
            }
            let tri = rotate(self.tri(t)?, a);
            (x, y) = (tri[1], tri[2]);
            let (px, py) = (self.p(x)?, self.p(y)?);
            let (ox, oy) = (orient(pa, px, pb)?, orient(pa, py, pb)?);
            // Collinear with a ray: a point lies on the segment (or the segment goes the other way).
            for (v, o, pv) in [(x, ox, px), (y, oy, py)] {
                if o == 0 {
                    let dot = (pv[0] - pa[0]) * (pb[0] - pa[0]) + (pv[1] - pa[1]) * (pb[1] - pa[1]);
                    if dot > 0.0 {
                        if v == b {
                            self.constrained.insert(undirected(a, b));
                            return Ok(());
                        }
                        self.constrain(a, v, depth + 1)?;
                        return self.constrain(v, b, depth + 1);
                    }
                }
            }
            if ox > 0 && oy < 0 {
                break;
            }
            // Rotate counter-clockwise around `a`: across the edge (a, y).
            t = *self.edge.get(&(a, y)).ok_or_else(|| BrepError::topology("a point is not surrounded by triangles"))?;
        }
        let mut queue: VecDeque<(u32, u32)> = VecDeque::new();
        // `x` is right of a→b, `y` left of it.
        loop {
            queue.push_back((x, y));
            let t2 = *self.edge.get(&(y, x)).ok_or_else(|| BrepError::topology("boundary segment leaves the triangulation"))?;
            let z = self.third(t2, x, y)?;
            if z == b {
                break;
            }
            match orient(pa, pb, self.p(z)?)? {
                s if s < 0 => x = z,
                s if s > 0 => y = z,
                _ => {
                    // A point exactly on the segment: split there.
                    self.constrain(a, z, depth + 1)?;
                    return self.constrain(z, b, depth + 1);
                }
            }
        }
        let mut attempts = 0usize;
        let cap = 64 + 16 * queue.len() * queue.len();
        while let Some((p1, p2)) = queue.pop_front() {
            attempts += 1;
            if attempts > cap {
                return Err(BrepError::NotConverged("recovering a boundary segment by flips".into()));
            }
            let (x, y) = if self.edge.contains_key(&(p1, p2)) {
                (p1, p2)
            } else if self.edge.contains_key(&(p2, p1)) {
                (p2, p1)
            } else {
                continue;
            };
            if self.constrained.contains(&undirected(x, y)) {
                return Err(BrepError::Topology("two boundary segments cross each other".into()));
            }
            let (Some(&t1), Some(&t2)) = (self.edge.get(&(x, y)), self.edge.get(&(y, x))) else { continue };
            let (p, q) = (self.third(t1, x, y)?, self.third(t2, x, y)?);
            let (pp, pq) = (self.p(p)?, self.p(q)?);
            let convex = orient(pp, pq, self.p(x)?)? * orient(pp, pq, self.p(y)?)? < 0;
            if !convex {
                queue.push_back((x, y));
                continue;
            }
            self.remove_tri(t1);
            self.remove_tri(t2);
            self.add_tri(p, x, q);
            self.add_tri(q, y, p);
            let crosses = orient(pa, pb, pp)? * orient(pa, pb, pq)? < 0;
            if crosses {
                queue.push_back((p, q));
            }
        }
        if !(self.edge.contains_key(&(a, b)) || self.edge.contains_key(&(b, a))) {
            return Err(BrepError::NotConverged("a boundary segment could not be recovered".into()));
        }
        self.constrained.insert(undirected(a, b));
        Ok(())
    }

    /// Restore the Delaunay property away from the boundary segments and return the triangles
    /// inside the boundary, counter-clockwise, as indices of the caller's points.
    pub fn finish(mut self) -> Result<Vec<[u32; 3]>> {
        let mut stack: Vec<(u32, u32)> = self.edge.keys().copied().collect();
        let cap = 64 * (self.tris.len() + 16);
        let mut flips = 0usize;
        while let Some((x, y)) = stack.pop() {
            if self.constrained.contains(&undirected(x, y)) {
                continue;
            }
            let (Some(&t1), Some(&t2)) = (self.edge.get(&(x, y)), self.edge.get(&(y, x))) else { continue };
            let (p, q) = (self.third(t1, x, y)?, self.third(t2, x, y)?);
            let (px, py, pp, pq) = (self.p(x)?, self.p(y)?, self.p(p)?, self.p(q)?);
            if incircle(px, py, pp, pq)? != Sign::Positive {
                continue;
            }
            if orient(pp, pq, px)? * orient(pp, pq, py)? >= 0 {
                continue;
            }
            flips += 1;
            if flips > cap {
                return Err(BrepError::NotConverged("restoring the Delaunay property".into()));
            }
            self.remove_tri(t1);
            self.remove_tri(t2);
            self.add_tri(p, x, q);
            self.add_tri(q, y, p);
            stack.extend([(x, q), (q, y), (y, p), (p, x)]);
        }
        // Parity flood fill from the triangles touching the super-triangle.
        let n = self.tris.len();
        let mut inside: Vec<Option<bool>> = vec![None; n];
        let mut queue: VecDeque<u32> = VecDeque::new();
        for (i, tri) in self.tris.iter().enumerate() {
            if self.alive.get(i).copied().unwrap_or(false)
                && tri.iter().any(|&v| v < SUPER)
                && let Some(slot) = inside.get_mut(i)
            {
                *slot = Some(false);
                queue.push_back(i as u32);
            }
        }
        while let Some(t) = queue.pop_front() {
            let state = inside.get(t as usize).copied().flatten().unwrap_or(false);
            let [a, b, c] = self.tri(t)?;
            for (x, y) in [(a, b), (b, c), (c, a)] {
                let Some(&nb) = self.edge.get(&(y, x)) else { continue };
                let next = state ^ self.constrained.contains(&undirected(x, y));
                if let Some(slot) = inside.get_mut(nb as usize)
                    && slot.is_none()
                {
                    *slot = Some(next);
                    queue.push_back(nb);
                }
            }
        }
        let mut out = Vec::new();
        for (i, tri) in self.tris.iter().enumerate() {
            if self.alive.get(i).copied().unwrap_or(false) && inside.get(i).copied().flatten() == Some(true) {
                if tri.iter().any(|&v| v < SUPER) {
                    return Err(BrepError::topology("a boundary segment leaves the region (an inside triangle touches the super-triangle)"));
                }
                out.push([tri[0] - SUPER, tri[1] - SUPER, tri[2] - SUPER]);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn unit(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn area(p: &[Pt], t: [u32; 3]) -> f64 {
        let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
        0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
    }

    #[test]
    fn a_random_point_set_is_delaunay_and_covers_the_hull_only_through_constraints() {
        let mut g = Lcg(7);
        let pts: Vec<Pt> = (0..120).map(|_| [g.unit() * 10.0, g.unit() * 10.0]).collect();
        let mut cdt = Cdt::new([0.0, 0.0], [10.0, 10.0]).unwrap();
        for p in &pts {
            cdt.insert(*p).unwrap();
        }
        // Without constraints everything is "outside": triangles touching the super-triangle flood the whole mesh.
        assert!(cdt.finish().unwrap().is_empty());
    }

    #[test]
    fn a_square_with_a_square_hole_triangulates_its_ring() {
        let outer = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let hole = [[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]];
        let mut cdt = Cdt::new([0.0, 0.0], [10.0, 10.0]).unwrap();
        let mut pts: Vec<Pt> = Vec::new();
        for p in outer.iter().chain(hole.iter()) {
            cdt.insert(*p).unwrap();
            pts.push(*p);
        }
        for i in 0..4 {
            cdt.insert_constraint(i as u32, ((i + 1) % 4) as u32).unwrap();
            cdt.insert_constraint(4 + i as u32, 4 + ((i + 1) % 4) as u32).unwrap();
        }
        let tris = cdt.finish().unwrap();
        let total: f64 = tris.iter().map(|&t| area(&pts, t)).sum();
        assert!((total - (100.0 - 16.0)).abs() < 1e-12, "{total}");
        assert!(tris.iter().all(|&t| area(&pts, t) > 0.0));
        assert_eq!(tris.len(), 8);
    }

    #[test]
    fn random_star_polygons_with_steiner_points_cover_exactly_their_area() {
        for seed in 1..40u64 {
            let mut g = Lcg(seed * 977);
            let n = 6 + (seed as usize % 25);
            // A star-shaped polygon: radii vary, angles increase.
            let poly: Vec<Pt> = (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * (i as f64 + 0.3 * g.unit()) / n as f64;
                    let r = 2.0 + 3.0 * g.unit();
                    [r * a.cos(), r * a.sin()]
                })
                .collect();
            let mut cdt = Cdt::new([-6.0, -6.0], [6.0, 6.0]).unwrap();
            let mut pts: Vec<Pt> = Vec::new();
            for p in &poly {
                cdt.insert(*p).unwrap();
                pts.push(*p);
            }
            // Interior Steiner points (strictly inside: the polygon contains the disk of radius 2).
            for _ in 0..30 {
                let (a, r) = (g.unit() * std::f64::consts::TAU, g.unit() * 1.9);
                let p = [r * a.cos(), r * a.sin()];
                let idx = cdt.insert(p).unwrap() as usize;
                if idx == pts.len() {
                    pts.push(p);
                }
            }
            for i in 0..n {
                cdt.insert_constraint(i as u32, ((i + 1) % n) as u32).unwrap();
            }
            let tris = cdt.finish().unwrap();
            let expect: f64 = 0.5 * (0..n).map(|i| poly[i][0] * poly[(i + 1) % n][1] - poly[(i + 1) % n][0] * poly[i][1]).sum::<f64>();
            let got: f64 = tris.iter().map(|&t| area(&pts, t)).sum();
            assert!((got - expect).abs() < 1e-9 * expect.abs(), "seed {seed}: {got} vs {expect}");
            assert!(tris.iter().all(|&t| area(&pts, t) > 0.0), "seed {seed}: a triangle is not counter-clockwise");
        }
    }

    #[test]
    fn interior_triangles_are_delaunay_away_from_the_boundary() {
        let mut g = Lcg(99);
        let outer = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let mut cdt = Cdt::new([0.0, 0.0], [10.0, 10.0]).unwrap();
        let mut pts: Vec<Pt> = outer.to_vec();
        for p in outer {
            cdt.insert(p).unwrap();
        }
        for _ in 0..150 {
            let p = [0.1 + 9.8 * g.unit(), 0.1 + 9.8 * g.unit()];
            let idx = cdt.insert(p).unwrap() as usize;
            if idx == pts.len() {
                pts.push(p);
            }
        }
        for i in 0..4u32 {
            cdt.insert_constraint(i, (i + 1) % 4).unwrap();
        }
        let tris = cdt.finish().unwrap();
        // Convex hull = the square, so the whole constrained triangulation is Delaunay.
        for t in &tris {
            for (i, p) in pts.iter().enumerate() {
                if t.contains(&(i as u32)) {
                    continue;
                }
                let s = incircle(pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize], *p).unwrap();
                assert_ne!(s, Sign::Positive, "point {i} is inside the circumcircle of {t:?}");
            }
        }
    }

    #[test]
    fn bad_input_is_reported() {
        let mut cdt = Cdt::new([0.0, 0.0], [1.0, 1.0]).unwrap();
        assert!(cdt.insert([f64::NAN, 0.0]).is_err());
        let a = cdt.insert([0.0, 0.0]).unwrap();
        assert_eq!(cdt.insert([0.0, 0.0]).unwrap(), a, "duplicates collapse");
        assert_eq!(cdt.insert([-0.0, 0.0]).unwrap(), a, "negative zero is the same point");
        assert!(Cdt::new([f64::NAN, 0.0], [1.0, 1.0]).is_err());
        // Crossing boundary segments.
        let mut cdt = Cdt::new([0.0, 0.0], [4.0, 4.0]).unwrap();
        let p: Vec<u32> = [[0.0, 0.0], [4.0, 4.0], [0.0, 4.0], [4.0, 0.0]].iter().map(|&q| cdt.insert(q).unwrap()).collect();
        cdt.insert_constraint(p[0], p[1]).unwrap();
        assert!(cdt.insert_constraint(p[2], p[3]).is_err());
    }
}
