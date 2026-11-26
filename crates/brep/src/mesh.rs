// SPDX-License-Identifier: MIT OR Apache-2.0
//! Triangle meshes and their checks.

use crate::math::{Point3, Vec3};
use std::collections::HashMap;

/// An indexed triangle mesh; triangles are counter-clockwise seen from outside.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Point3>,
    pub triangles: Vec<[u32; 3]>,
}

/// What [`Mesh::check`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshReport {
    /// Directed edges whose opposite edge is missing (holes, T-junctions).
    pub open_edges: usize,
    /// Directed edges used by more than one triangle (overlaps, or inconsistent orientation).
    pub repeated_edges: usize,
    /// Triangles with a repeated vertex or an index out of range.
    pub degenerate_triangles: usize,
}

impl MeshReport {
    /// A closed, consistently oriented, manifold surface.
    pub fn is_watertight(&self) -> bool {
        self.open_edges == 0 && self.repeated_edges == 0 && self.degenerate_triangles == 0
    }
}

impl Mesh {
    fn corners(&self, t: [u32; 3]) -> Option<(Point3, Point3, Point3)> {
        Some((*self.vertices.get(t[0] as usize)?, *self.vertices.get(t[1] as usize)?, *self.vertices.get(t[2] as usize)?))
    }

    /// Total surface area.
    pub fn area(&self) -> f64 {
        self.triangles.iter().filter_map(|&t| self.corners(t)).map(|(a, b, c)| 0.5 * (b - a).cross(c - a).norm()).sum()
    }

    /// The signed enclosed volume (positive when the triangles face outward), by the divergence theorem.
    pub fn volume(&self) -> f64 {
        self.triangles.iter().filter_map(|&t| self.corners(t)).map(|(a, b, c)| a.dot(b.cross(c)) / 6.0).sum()
    }

    /// The centroid of the enclosed volume (`None` for a mesh enclosing no volume).
    pub fn centroid(&self) -> Option<Vec3> {
        let mut moment = Vec3::ZERO;
        let mut volume = 0.0;
        for (a, b, c) in self.triangles.iter().filter_map(|&t| self.corners(t)) {
            let v = a.dot(b.cross(c)) / 6.0;
            volume += v;
            moment += (a + b + c) * (v / 4.0);
        }
        (volume.abs() > 1.0e-300).then(|| moment / volume)
    }

    /// Check that every directed edge occurs once and its opposite once.
    pub fn check(&self) -> MeshReport {
        let mut report = MeshReport::default();
        let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
        for &t in &self.triangles {
            let valid = t.iter().all(|&i| (i as usize) < self.vertices.len());
            if !valid || t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
                report.degenerate_triangles += 1;
                continue;
            }
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *directed.entry((a, b)).or_insert(0) += 1;
            }
        }
        for (&(a, b), &n) in &directed {
            if n > 1 {
                report.repeated_edges += 1;
            }
            if !directed.contains_key(&(b, a)) {
                report.open_edges += 1;
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra(flip: bool) -> Mesh {
        let vertices = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)];
        let mut triangles = vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];
        if flip {
            for t in &mut triangles {
                t.swap(1, 2);
            }
        }
        Mesh { vertices, triangles }
    }

    #[test]
    fn a_tetrahedron_has_the_closed_form_volume_area_and_centroid() {
        let m = tetra(false);
        assert!(m.check().is_watertight());
        assert!((m.volume() - 1.0 / 6.0).abs() < 1e-15);
        assert!((m.area() - (1.5 + 0.5 * 3f64.sqrt())).abs() < 1e-14);
        assert!(m.centroid().unwrap().distance(Vec3::new(0.25, 0.25, 0.25)) < 1e-15);
        assert!((tetra(true).volume() + 1.0 / 6.0).abs() < 1e-15, "inside-out has negative volume");
    }

    #[test]
    fn holes_overlaps_and_bad_indices_are_counted() {
        let mut m = tetra(false);
        m.triangles.pop();
        let r = m.check();
        assert_eq!(r.open_edges, 3);
        assert!(!r.is_watertight());
        let mut m = tetra(false);
        m.triangles.push([0, 2, 1]);
        assert_eq!(m.check().repeated_edges, 3);
        let mut m = tetra(false);
        m.triangles.push([0, 0, 1]);
        m.triangles.push([0, 1, 99]);
        assert_eq!(m.check().degenerate_triangles, 2);
        assert!(Mesh::default().centroid().is_none());
    }
}
