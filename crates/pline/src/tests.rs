// SPDX-License-Identifier: MIT OR Apache-2.0
use std::f64::consts::PI;

use super::*;

fn polygon(pts: &[(f64, f64)]) -> Pline {
    let mut p = Pline::new_closed();
    for (x, y) in pts {
        p.add(*x, *y, 0.0);
    }
    p
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Pline {
    polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn circle(cx: f64, cy: f64, r: f64) -> Pline {
    let mut p = Pline::new_closed();
    p.add(cx - r, cy, 1.0);
    p.add(cx + r, cy, 1.0);
    p
}

fn stadium(ax: f64, ay: f64, bx: f64, by: f64, r: f64) -> Pline {
    let len = (bx - ax).hypot(by - ay);
    let (nx, ny) = (-(by - ay) / len * r, (bx - ax) / len * r);
    let mut p = Pline::new_closed();
    p.add(ax - nx, ay - ny, 0.0);
    p.add(bx - nx, by - ny, 1.0);
    p.add(bx + nx, by + ny, 0.0);
    p.add(ax + nx, ay + ny, 1.0);
    p
}

/// The net area of what an operation left, given how the outlines relate.
fn net(a: &Pline, b: &Pline, op: BooleanOp) -> f64 {
    let r = a.boolean(b, op);
    let (aa, ab) = (a.area().abs(), b.area().abs());
    match (r.info, op) {
        (BooleanInfo::InvalidInput, _) => f64::NAN,
        (BooleanInfo::Disjoint, BooleanOp::Or) => aa + ab,
        (BooleanInfo::Disjoint, BooleanOp::And) => 0.0,
        (BooleanInfo::Disjoint, BooleanOp::Not) => aa,
        (BooleanInfo::Pline1InsidePline2, BooleanOp::Or) => ab,
        (BooleanInfo::Pline1InsidePline2, BooleanOp::And) => aa,
        (BooleanInfo::Pline1InsidePline2, BooleanOp::Not) => 0.0,
        (BooleanInfo::Pline2InsidePline1, BooleanOp::Or) => aa,
        (BooleanInfo::Pline2InsidePline1, BooleanOp::And) => ab,
        (BooleanInfo::Pline2InsidePline1, BooleanOp::Not) => aa - ab,
        (BooleanInfo::Overlapping, BooleanOp::Or | BooleanOp::And) => aa,
        (BooleanInfo::Overlapping, BooleanOp::Not) => 0.0,
        (BooleanInfo::Intersected, _) => r.pos.iter().chain(r.neg.iter()).map(Pline::area).sum(),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-7 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn area_of_shapes() {
    assert!(near(rect(0.0, 0.0, 3.0, 2.0).area(), 6.0));
    assert!(near(circle(5.0, -2.0, 1.5).area(), PI * 2.25));
    let mut cw = rect(0.0, 0.0, 3.0, 2.0);
    cw.invert_direction_mut();
    assert!(near(cw.area(), -6.0));
    let mut c = circle(0.0, 0.0, 1.0);
    c.invert_direction_mut();
    assert!(near(c.area(), -PI));
    assert!(near(stadium(0.0, 0.0, 4.0, 0.0, 1.0).area(), 8.0 + PI));
}

#[test]
fn extents_include_arc_bulges() {
    let e = circle(1.0, 2.0, 3.0).extents().unwrap();
    assert!(near(e.min_x, -2.0) && near(e.max_x, 4.0) && near(e.min_y, -1.0) && near(e.max_y, 5.0));
    assert!(Pline::new_closed().extents().is_none());
}

#[test]
fn winding_inside_and_outside() {
    let c = circle(0.0, 0.0, 2.0);
    assert_eq!(c.winding_number(0.5, 0.5), 1);
    assert_eq!(c.winding_number(1.99, 0.0), 1);
    assert_eq!(c.winding_number(2.01, 0.0), 0);
    assert_eq!(c.winding_number(0.0, 5.0), 0);
    let mut cw = rect(0.0, 0.0, 2.0, 2.0);
    cw.invert_direction_mut();
    assert_eq!(cw.winding_number(1.0, 1.0), -1);
    // A ray through a vertex counts once.
    assert_eq!(rect(0.0, 0.0, 2.0, 2.0).winding_number(-1.0, 2.0), 0);
    assert_eq!(rect(0.0, 0.0, 2.0, 2.0).winding_number(1.0, 0.0), 1);
}

#[test]
fn inverting_twice_restores_the_outline() {
    let s = stadium(0.0, 0.0, 3.0, 1.0, 0.5);
    let mut t = s.clone();
    t.invert_direction_mut();
    assert!(near(t.area(), -s.area()));
    t.invert_direction_mut();
    assert!(near(t.area(), s.area()));
    assert_eq!(t.vertex_count(), s.vertex_count());
}

#[test]
fn repeated_positions_merge() {
    let mut p = Pline::new_closed();
    for (x, y, b) in [(0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 0.0, 0.0)] {
        p.add(x, y, b);
    }
    let q = p.remove_repeat_pos(1e-9).unwrap();
    assert_eq!(q.vertex_count(), 3);
    assert!(rect(0.0, 0.0, 1.0, 1.0).remove_repeat_pos(1e-9).is_none());
}

#[test]
fn two_overlapping_rectangles() {
    let (a, b) = (rect(0.0, 0.0, 4.0, 4.0), rect(2.0, 1.0, 6.0, 3.0));
    assert!(near(net(&a, &b, BooleanOp::Or), 16.0 + 8.0 - 4.0));
    assert!(near(net(&a, &b, BooleanOp::And), 4.0));
    assert!(near(net(&a, &b, BooleanOp::Not), 12.0));
    assert!(near(net(&b, &a, BooleanOp::Not), 4.0));
}

#[test]
fn two_overlapping_circles_follow_the_lens_formula() {
    let (r, d) = (2.0, 2.5);
    let (a, b) = (circle(0.0, 0.0, r), circle(d, 0.0, r));
    let lens = 2.0 * r * r * (d / (2.0 * r)).acos() - d / 2.0 * (4.0 * r * r - d * d).sqrt();
    assert!(near(net(&a, &b, BooleanOp::And), lens));
    assert!(near(net(&a, &b, BooleanOp::Or), 2.0 * PI * r * r - lens));
    assert!(near(net(&a, &b, BooleanOp::Not), PI * r * r - lens));
}

#[test]
fn circle_and_rectangle_cut_a_segment() {
    // A circle of radius 2 cut by the line x = 1: the cap beyond it.
    let (c, h) = (circle(0.0, 0.0, 2.0), rect(1.0, -5.0, 5.0, 5.0));
    let cap = 4.0 * (0.5_f64).acos() - 1.0 * (3.0_f64).sqrt();
    assert!(near(net(&c, &h, BooleanOp::And), cap));
    assert!(near(net(&c, &h, BooleanOp::Not), PI * 4.0 - cap));
}

#[test]
fn nested_and_apart_shapes_are_told_apart() {
    let (big, small, far) = (rect(0.0, 0.0, 10.0, 10.0), circle(5.0, 5.0, 1.0), rect(20.0, 20.0, 21.0, 21.0));
    assert_eq!(big.boolean(&small, BooleanOp::Or).info, BooleanInfo::Pline2InsidePline1);
    assert_eq!(small.boolean(&big, BooleanOp::Or).info, BooleanInfo::Pline1InsidePline2);
    assert_eq!(big.boolean(&far, BooleanOp::Or).info, BooleanInfo::Disjoint);
    assert_eq!(big.boolean(&big.clone(), BooleanOp::Or).info, BooleanInfo::Overlapping);
    // The same outline written from another vertex and in the other direction.
    let mut other = polygon(&[(10.0, 10.0), (0.0, 10.0), (0.0, 0.0), (10.0, 0.0)]);
    assert_eq!(big.boolean(&other, BooleanOp::And).info, BooleanInfo::Overlapping);
    other.invert_direction_mut();
    assert_eq!(big.boolean(&other, BooleanOp::And).info, BooleanInfo::Overlapping);
}

#[test]
fn touching_at_a_point_is_apart() {
    let (a, b) = (rect(0.0, 0.0, 1.0, 1.0), rect(1.0, 1.0, 2.0, 2.0));
    assert_eq!(a.boolean(&b, BooleanOp::Or).info, BooleanInfo::Disjoint);
    let (c, d) = (circle(0.0, 0.0, 1.0), circle(2.0, 0.0, 1.0));
    assert_eq!(c.boolean(&d, BooleanOp::Or).info, BooleanInfo::Disjoint);
}

#[test]
fn rectangles_sharing_an_edge_join() {
    let (a, b) = (rect(0.0, 0.0, 2.0, 2.0), rect(2.0, 0.0, 5.0, 2.0));
    let r = a.boolean(&b, BooleanOp::Or);
    assert_eq!(r.info, BooleanInfo::Intersected);
    assert_eq!(r.pos.len(), 1);
    assert!(r.neg.is_empty());
    assert!(near(r.pos[0].area(), 10.0));
    // Nothing is common to them, and taking one from the other changes nothing.
    assert!(a.boolean(&b, BooleanOp::And).pos.is_empty());
    assert!(near(net(&a, &b, BooleanOp::Not), 4.0));
}

#[test]
fn a_hole_appears_when_an_arc_closes_a_gap() {
    // A "C" shape closed by a bar: a ring around an empty middle.
    let c_shape = polygon(&[(0.0, 0.0), (6.0, 0.0), (6.0, 1.0), (1.0, 1.0), (1.0, 5.0), (6.0, 5.0), (6.0, 6.0), (0.0, 6.0)]);
    let bar = rect(5.0, 0.5, 6.5, 5.5);
    let r = c_shape.boolean(&bar, BooleanOp::Or);
    assert_eq!(r.info, BooleanInfo::Intersected);
    assert_eq!(r.pos.len(), 1);
    assert_eq!(r.neg.len(), 1);
    // The hole is what the bar leaves of the C's hollow: x 1..5, y 1..5.
    assert!(near(r.neg[0].area(), -16.0));
    // C (16) + bar (7.5) - overlap (1) of material, plus the hole inside the outer loop.
    assert!(near(r.pos[0].area(), 22.5 + 16.0));
}

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// A random rectangle, circle, stadium or rotated polygon with a few arcs.
fn random_shape(rng: &mut Rng) -> Pline {
    let (cx, cy) = (rng.range(0.0, 10.0), rng.range(0.0, 10.0));
    match (rng.next() * 4.0) as u32 {
        0 => {
            let (w, h) = (rng.range(0.5, 5.0), rng.range(0.5, 5.0));
            rect(cx, cy, cx + w, cy + h)
        }
        1 => circle(cx, cy, rng.range(0.3, 3.0)),
        2 => {
            let (a, l, r) = (rng.range(0.0, PI), rng.range(0.5, 5.0), rng.range(0.2, 1.5));
            stadium(cx, cy, cx + l * a.cos(), cy + l * a.sin(), r)
        }
        _ => {
            let n = 3 + (rng.next() * 5.0) as usize;
            let r = rng.range(0.5, 3.0);
            let mut p = Pline::new_closed();
            for k in 0..n {
                let ang = 2.0 * PI * k as f64 / n as f64;
                p.add(cx + r * ang.cos(), cy + r * ang.sin(), if k % 2 == 0 { 0.0 } else { rng.range(-0.3, 0.3) });
            }
            p
        }
    }
}

#[test]
fn random_pairs_obey_inclusion_exclusion() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut crossed, mut checked) = (0, 0);
    for _ in 0..3000 {
        let (a, b) = (random_shape(&mut rng), random_shape(&mut rng));
        // A polygon with random bulges can fold over itself; only simple outlines are inputs.
        if a.area() <= 0.0 && a.area().abs() < 1e-9 || b.area().abs() < 1e-9 || !simple(&a) || !simple(&b) {
            continue;
        }
        let (aa, ab) = (a.area().abs(), b.area().abs());
        let (u, i) = (net(&a, &b, BooleanOp::Or), net(&a, &b, BooleanOp::And));
        let (d, e) = (net(&a, &b, BooleanOp::Not), net(&b, &a, BooleanOp::Not));
        checked += 1;
        if a.boolean(&b, BooleanOp::Or).info == BooleanInfo::Intersected {
            crossed += 1;
        }
        assert!(u.is_finite() && i.is_finite() && d.is_finite() && e.is_finite(), "invalid input for {a:?} and {b:?}");
        assert!(near(u + i, aa + ab), "union {u} + intersection {i} != {aa} + {ab} for {a:?} and {b:?}");
        assert!(near(d, aa - i), "difference {d} != {aa} - {i} for {a:?} and {b:?}");
        assert!(near(e, ab - i), "difference {e} != {ab} - {i} for {a:?} and {b:?}");
        assert!(u >= aa.max(ab) - 1e-7 && i <= aa.min(ab) + 1e-7);
    }
    assert!(checked > 1500 && crossed > 500, "checked {checked}, crossed {crossed}");
}

/// Whether no two segments of the polyline cross (so its area means what it says).
fn simple(p: &Pline) -> bool {
    let segs = p.segments();
    let n = segs.len();
    for i in 0..n {
        for j in (i + 1)..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if !crate::geom::intersect(&segs[i], &segs[j], 1e-9).is_empty() {
                return false;
            }
        }
    }
    true
}

#[test]
fn hostile_input_is_refused_not_a_panic() {
    let good = rect(0.0, 0.0, 1.0, 1.0);
    let mut nan = Pline::new_closed();
    nan.add(0.0, 0.0, 0.0);
    nan.add(f64::NAN, 1.0, 0.0);
    nan.add(1.0, 1.0, 0.0);
    let mut inf = good.clone();
    inf.add(f64::INFINITY, 0.0, 0.0);
    let mut flat = Pline::new_closed();
    flat.add(0.0, 0.0, 0.0);
    flat.add(1.0, 0.0, 0.0);
    let mut single = Pline::new_closed();
    single.add(0.0, 0.0, 0.0);
    let mut huge_bulge = good.clone();
    huge_bulge.add(2.0, 2.0, 1e300);
    for bad in [Pline::new_closed(), single, nan, inf, flat, huge_bulge] {
        for op in [BooleanOp::Or, BooleanOp::And, BooleanOp::Not] {
            let r = good.boolean(&bad, op);
            assert_eq!(r.info, BooleanInfo::InvalidInput);
            assert_eq!(bad.boolean(&good, op).info, BooleanInfo::InvalidInput);
        }
    }
    // Absurd magnitudes do not panic either.
    let big = rect(0.0, 0.0, 1e300, 1e300);
    let _ = big.boolean(&good, BooleanOp::Or);
}

/// Shapes on a coarse grid share edges, corners and whole circles with each other all the time:
/// the cases a drawn layout is full of and random shapes never hit.
fn grid_shape(rng: &mut Rng) -> Pline {
    let g = |rng: &mut Rng| (rng.next() * 6.0).floor();
    match (rng.next() * 3.0) as u32 {
        0 => {
            let (x, y) = (g(rng), g(rng));
            rect(x, y, x + 1.0 + (rng.next() * 3.0).floor(), y + 1.0 + (rng.next() * 3.0).floor())
        }
        1 => circle(g(rng), g(rng), 1.0 + (rng.next() * 2.0).floor()),
        _ => {
            let (x, y) = (g(rng), g(rng));
            stadium(x, y, x + 1.0 + (rng.next() * 3.0).floor(), y, 1.0)
        }
    }
}

#[test]
fn grid_pairs_with_shared_edges_obey_inclusion_exclusion() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let (mut touched, mut checked) = (0, 0);
    for _ in 0..4000 {
        let (a, b) = (grid_shape(&mut rng), grid_shape(&mut rng));
        let (aa, ab) = (a.area().abs(), b.area().abs());
        let (u, i) = (net(&a, &b, BooleanOp::Or), net(&a, &b, BooleanOp::And));
        let (d, e) = (net(&a, &b, BooleanOp::Not), net(&b, &a, BooleanOp::Not));
        assert!(u.is_finite() && i.is_finite() && d.is_finite() && e.is_finite(), "invalid input for {a:?} and {b:?}");
        checked += 1;
        if a.boolean(&b, BooleanOp::Or).info == BooleanInfo::Intersected {
            touched += 1;
        }
        assert!(near(u + i, aa + ab), "union {u} + intersection {i} != {aa} + {ab} for {a:?} and {b:?}");
        assert!(near(d, aa - i), "difference {d} != {aa} - {i} for {a:?} and {b:?}");
        assert!(near(e, ab - i), "difference {e} != {ab} - {i} for {a:?} and {b:?}");
    }
    assert!(checked == 4000 && touched > 1000, "touched {touched}");
}

#[test]
fn a_shape_inside_another_that_shares_outline_cuts_a_notch() {
    // B sits in A's corner and shares two of its edges: A less B is one L-shaped loop, not a
    // square with a hole.
    let (a, b) = (rect(0.0, 0.0, 4.0, 4.0), rect(0.0, 0.0, 2.0, 2.0));
    let r = a.boolean(&b, BooleanOp::Not);
    assert_eq!(r.info, BooleanInfo::Intersected);
    assert_eq!((r.pos.len(), r.neg.len()), (1, 0));
    assert!(near(r.pos[0].area(), 12.0));
    assert_eq!(r.pos[0].vertex_count(), 6);
    // The union is A again and the common part is B, both as loops.
    assert!(near(net(&a, &b, BooleanOp::Or), 16.0));
    assert!(near(net(&a, &b, BooleanOp::And), 4.0));
    // Touching only at a point (a circle in a square's side) keeps the shapes apart or nested.
    let (sq, c) = (rect(0.0, 0.0, 4.0, 4.0), circle(2.0, 2.0, 2.0));
    assert_eq!(sq.boolean(&c, BooleanOp::Not).info, BooleanInfo::Pline2InsidePline1);
}
