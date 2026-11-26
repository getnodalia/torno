// SPDX-License-Identifier: MIT OR Apache-2.0
//! The validity checker: every primitive passes, and every deliberate corruption is reported.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nodalia_brep::build::{cone, cuboid, cylinder, extrude, revolve, sphere, torus};
use nodalia_brep::curve::{Circle, Curve, Line};
use nodalia_brep::profile::{Profile, Profile2};
use nodalia_brep::surface::{Cylinder, Plane, Sphere, Surface};
use nodalia_brep::topology::{Brep, EdgeId, VertexId};
use nodalia_brep::{Frame, Tolerance, Vec2, Vec3};
use std::collections::HashMap;

const T: Tolerance = Tolerance::DEFAULT;

fn assert_valid(b: &Brep, what: &str) {
    let report = b.validate(&T);
    assert!(report.is_valid(), "{what} should be valid:\n{report}");
}

#[test]
fn every_primitive_is_valid_and_reports_its_genus() {
    let w = Frame::WORLD;
    let cases: Vec<(&str, Brep, i64, i64)> = vec![
        ("box", cuboid(&w, 2.0, 3.0, 4.0, &T).unwrap(), 2, 0),
        ("cylinder", cylinder(&w, 1.0, 2.0, &T).unwrap(), 2, 0),
        ("cone", cone(&w, 2.0, 0.0, 3.0, &T).unwrap(), 2, 0),
        ("inverted cone", cone(&w, 0.0, 2.0, 3.0, &T).unwrap(), 2, 0),
        ("frustum", cone(&w, 2.0, 1.0, 3.0, &T).unwrap(), 2, 0),
        ("sphere", sphere(&w, 2.0, &T).unwrap(), 2, 0),
        ("torus", torus(&w, 5.0, 1.0, &T).unwrap(), 0, 1),
        ("tube", revolve(&Profile2::rectangle(2.0, 0.0, 5.0, 3.0).unwrap(), &w, &T).unwrap(), 0, 1),
    ];
    for (name, b, euler, genus) in cases {
        assert_valid(&b, name);
        let report = b.validate(&T);
        let shell = &report.shells[0];
        assert_eq!(shell.euler, euler, "{name}");
        assert_eq!(shell.genus, Some(genus), "{name}");
        assert!(shell.volume.unwrap() > 0.0, "{name}");
    }
}

#[test]
fn a_plate_with_holes_has_genus_equal_to_its_hole_count() {
    let outer = Profile2::rectangle(0.0, 0.0, 20.0, 10.0).unwrap();
    let holes = vec![
        Profile2::circle(Vec2::new(5.0, 5.0), 2.0).unwrap(),
        Profile2::circle(Vec2::new(15.0, 5.0), 2.0).unwrap(),
        Profile2::rectangle(9.0, 4.0, 11.0, 6.0).unwrap(),
    ];
    let b = extrude(&Profile { outer, holes }, &Frame::WORLD, 3.0, &T).unwrap();
    let report = b.validate(&T);
    assert!(report.is_valid(), "{report}");
    assert_eq!(report.shells[0].genus, Some(3));
}

/// A box assembled by hand, with one deliberate defect.
#[derive(Clone, Copy, PartialEq)]
enum Defect {
    None,
    FlippedSense,
    MissingFace,
    WrongVertex,
    MovedVertex,
    DuplicateVertex,
    InsideOut,
    OffsetPlane,
    EdgeUsedThrice,
}

fn manual_box(defect: Defect) -> Brep {
    let mut b = Brep::new();
    let size = Vec3::new(2.0, 3.0, 4.0);
    let corner = |i: usize| Vec3::new((i & 1) as f64 * size.x, ((i >> 1) & 1) as f64 * size.y, ((i >> 2) & 1) as f64 * size.z);
    let mut vs: Vec<VertexId> = (0..8).map(|i| b.add_vertex(corner(i)).unwrap()).collect();
    if defect == Defect::MovedVertex {
        vs[7] = b.add_vertex(corner(7) + Vec3::new(0.0, 0.0, 1e-3)).unwrap();
    }
    if defect == Defect::DuplicateVertex {
        b.add_vertex(corner(3)).unwrap();
    }
    let mut edges: HashMap<(usize, usize), EdgeId> = HashMap::new();
    let mut edge = |b: &mut Brep, a: usize, c: usize| -> (EdgeId, bool) {
        let (lo, hi) = (a.min(c), a.max(c));
        let id = *edges.entry((lo, hi)).or_insert_with(|| {
            let (line, len) = Line::through(corner(lo), corner(hi)).unwrap();
            let end = if defect == Defect::WrongVertex && lo == 0 && hi == 1 { vs[3] } else { vs[hi] };
            b.add_edge(Curve::Line(line), 0.0, len, vs[lo], end).unwrap()
        });
        (id, a > c)
    };
    let normals = [(0usize, -1.0, 'x'), (1, 1.0, 'x'), (0, -1.0, 'y'), (1, 1.0, 'y'), (0, -1.0, 'z'), (1, 1.0, 'z')];
    let mut faces = Vec::new();
    for (k, &(bit, sign, axis)) in normals.iter().enumerate() {
        let shift = match axis {
            'x' => 0,
            'y' => 1,
            _ => 2,
        };
        let idx: Vec<usize> = (0..8).filter(|i| (i >> shift) & 1 == bit).collect();
        let mut n = match axis {
            'x' => Vec3::X,
            'y' => Vec3::Y,
            _ => Vec3::Z,
        } * sign;
        let c = idx.iter().fold(Vec3::ZERO, |s, &i| s + corner(i)) / 4.0;
        let e1 = (corner(idx[0]) - c).normalized().unwrap();
        let e2 = n.cross(e1);
        let mut order = idx.clone();
        order.sort_by(|&p, &q| {
            let ang = |i: usize| (corner(i) - c).dot(e2).atan2((corner(i) - c).dot(e1));
            ang(p).partial_cmp(&ang(q)).unwrap()
        });
        if defect == Defect::InsideOut {
            order.reverse();
            n = -n;
        }
        if defect == Defect::MissingFace && k == 5 {
            continue;
        }
        let uses: Vec<(EdgeId, bool)> = (0..4).map(|i| edge(&mut b, order[i], order[(i + 1) % 4])).collect();
        let l = b.add_loop(&uses).unwrap();
        let mut origin = c;
        if defect == Defect::OffsetPlane && k == 0 {
            origin += Vec3::new(0.01, 0.0, 0.0);
        }
        let plane = Surface::Plane(Plane::new(Frame::new(origin, n, Some(e1)).unwrap()).unwrap());
        let same = !(defect == Defect::FlippedSense && k == 2);
        faces.push(b.add_face(plane, same, l, &[]).unwrap());
    }
    if defect == Defect::EdgeUsedThrice {
        // A dangling extra face sharing an edge of the box.
        let uses = [(edges[&(0, 1)], false), (edges[&(0, 1)], true)];
        let l = b.add_loop(&uses).unwrap();
        let plane = Surface::Plane(Plane::new(Frame::WORLD).unwrap());
        faces.push(b.add_face(plane, true, l, &[]).unwrap());
    }
    let shell = b.add_shell(&faces).unwrap();
    b.add_solid(&[shell]).unwrap();
    b
}

#[test]
fn the_hand_built_box_is_valid() {
    assert_valid(&manual_box(Defect::None), "hand-built box");
}

#[test]
fn each_defect_is_reported_with_its_own_message() {
    let cases: [(Defect, &str); 8] = [
        (Defect::FlippedSense, "outer loop runs"),
        (Defect::MissingFace, "used by"),
        (Defect::WrongVertex, "away from its end vertex"),
        (Defect::MovedVertex, "away from its end vertex"),
        (Defect::DuplicateVertex, "same point"),
        (Defect::InsideOut, "encloses volume"),
        (Defect::OffsetPlane, "leaves the surface"),
        (Defect::EdgeUsedThrice, "coedges"),
    ];
    for (defect, needle) in cases {
        let report = manual_box(defect).validate(&T);
        assert!(!report.is_valid(), "defect must invalidate the box");
        assert!(report.errors().any(|f| f.message.contains(needle)), "expected an error containing {needle:?}, got:\n{report}");
    }
}

#[test]
fn a_cylinder_with_a_wrong_radius_edge_is_caught() {
    // A side face on a cylinder of radius 1 whose bottom edge is a circle of radius 1.1.
    let mut b = Brep::new();
    let frame = Frame::WORLD;
    let v = b.add_vertex(Vec3::new(1.1, 0.0, 0.0)).unwrap();
    let top = b.add_vertex(Vec3::new(1.1, 0.0, 2.0)).unwrap();
    let circle = |z: f64| Circle::new(Frame { origin: Vec3::new(0.0, 0.0, z), ..frame }, 1.1).unwrap();
    let e_bottom = b.add_edge(Curve::Circle(circle(0.0)), 0.0, std::f64::consts::TAU, v, v).unwrap();
    let e_top = b.add_edge(Curve::Circle(circle(2.0)), 0.0, std::f64::consts::TAU, top, top).unwrap();
    let (line, len) = Line::through(Vec3::new(1.1, 0.0, 0.0), Vec3::new(1.1, 0.0, 2.0)).unwrap();
    let seam = b.add_edge(Curve::Line(line), 0.0, len, v, top).unwrap();
    let l = b.add_loop(&[(e_bottom, false), (seam, false), (e_top, true), (seam, true)]).unwrap();
    let f = b.add_face(Surface::Cylinder(Cylinder::new(frame, 1.0).unwrap()), true, l, &[]).unwrap();
    let sh = b.add_shell(&[f]).unwrap();
    b.add_solid(&[sh]).unwrap();
    let report = b.validate(&T);
    assert!(report.errors().any(|f| f.message.contains("leaves the surface")), "{report}");
}

#[test]
fn a_hole_outside_its_face_is_caught() {
    // The unit square on the plane with a "hole" far outside it.
    let mut b = Brep::new();
    let w = Frame::WORLD;
    let c0 = b.add_vertex(Vec3::new(1.0, 0.0, 0.0)).unwrap();
    let c1 = b.add_vertex(Vec3::new(11.0, 0.0, 0.0)).unwrap();
    let outer = b.add_edge(Curve::Circle(Circle::new(w, 1.0).unwrap()), 0.0, std::f64::consts::TAU, c0, c0).unwrap();
    let hole = b
        .add_edge(Curve::Circle(Circle::new(Frame { origin: Vec3::new(10.0, 0.0, 0.0), ..w }, 1.0).unwrap()), 0.0, std::f64::consts::TAU, c1, c1)
        .unwrap();
    let lo = b.add_loop(&[(outer, false)]).unwrap();
    let lh = b.add_loop(&[(hole, true)]).unwrap();
    let f = b.add_face(Surface::Plane(Plane::new(w).unwrap()), true, lo, &[lh]).unwrap();
    let sh = b.add_shell(&[f]).unwrap();
    b.add_solid(&[sh]).unwrap();
    let report = b.validate(&T);
    assert!(report.errors().any(|f| f.message.contains("not inside the outer loop")), "{report}");
}

/// A deterministic generator (no external crates).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn unit(&mut self) -> f64 {
        self.next() as f64 / (1u64 << 31) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    fn pick(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

#[test]
fn random_garbage_never_panics_the_checker() {
    for seed in 0..300u64 {
        let mut g = Lcg(seed.wrapping_mul(7919) + 1);
        let mut b = Brep::new();
        let mut vs = Vec::new();
        for _ in 0..g.pick(8) + 1 {
            let p = Vec3::new(g.range(-5.0, 5.0), g.range(-5.0, 5.0), g.range(-5.0, 5.0));
            if let Ok(v) = b.add_vertex(p) {
                vs.push(v);
            }
        }
        let mut es = Vec::new();
        for _ in 0..g.pick(12) + 1 {
            let (a, c) = (vs[g.pick(vs.len())], vs[g.pick(vs.len())]);
            let curve = match g.pick(3) {
                0 => Line::new(Vec3::new(g.range(-2.0, 2.0), 0.0, 0.0), Vec3::new(g.range(-1.0, 1.0), g.range(-1.0, 1.0), g.range(-1.0, 1.0)))
                    .map(Curve::Line),
                _ => Circle::new(Frame::WORLD, g.range(0.0, 3.0)).map(Curve::Circle),
            };
            if let Ok(curve) = curve
                && let Ok(e) = b.add_edge(curve, g.range(-1.0, 0.5), g.range(0.5, 9.0), a, c)
            {
                es.push(e);
            }
        }
        let mut loops = Vec::new();
        if !es.is_empty() {
            for _ in 0..g.pick(5) + 1 {
                let uses: Vec<_> = (0..g.pick(4) + 1).map(|_| (es[g.pick(es.len())], g.pick(2) == 0)).collect();
                if let Ok(l) = b.add_loop(&uses) {
                    loops.push(l);
                }
            }
        }
        let mut faces = Vec::new();
        for l in &loops {
            let surface = match g.pick(3) {
                0 => Surface::Plane(Plane::new(Frame::WORLD).unwrap()),
                1 => Surface::Cylinder(Cylinder::new(Frame::WORLD, g.range(0.1, 3.0)).unwrap()),
                _ => Surface::Sphere(Sphere::new(Frame::WORLD, g.range(0.1, 3.0)).unwrap()),
            };
            if let Ok(f) = b.add_face(surface, g.pick(2) == 0, *l, &[]) {
                faces.push(f);
            }
        }
        if !faces.is_empty()
            && let Ok(sh) = b.add_shell(&faces)
        {
            let _ = b.add_solid(&[sh]);
        }
        let report = b.validate(&T);
        // Garbage is nearly always invalid; the point is that it is reported rather than crashing.
        let _ = report.is_valid();
        let _ = format!("{report}");
    }
}
