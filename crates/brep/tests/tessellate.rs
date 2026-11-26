// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tessellation: watertightness, volume and area against the exact values, deviation from the surfaces.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nodalia_brep::build::{cone, cuboid, cylinder, extrude, sphere, torus};
use nodalia_brep::mass::solid_properties;
use nodalia_brep::profile::{Profile, Profile2};
use nodalia_brep::tessellate::{tessellate, tessellate_faces};
use nodalia_brep::topology::Brep;
use nodalia_brep::{Frame, Tolerance, Vec2, Vec3};
use std::f64::consts::PI;

const T: Tolerance = Tolerance::DEFAULT;

/// The largest distance of triangle centroids and edge midpoints from the true surface of their face.
fn max_deviation(b: &Brep, chord: f64) -> f64 {
    let t = tessellate_faces(b, chord).unwrap();
    let mut worst = 0.0f64;
    for (tri, &f) in t.mesh.triangles.iter().zip(&t.triangle_face) {
        let surface = &b.face(f).unwrap().surface;
        let p: Vec<Vec3> = tri.iter().map(|&i| t.mesh.vertices[i as usize]).collect();
        let samples = [(p[0] + p[1] + p[2]) / 3.0, (p[0] + p[1]) / 2.0, (p[1] + p[2]) / 2.0, (p[2] + p[0]) / 2.0];
        for s in samples {
            worst = worst.max(surface.closest_point(s).unwrap().distance);
        }
    }
    worst
}

/// `convex`: the solid has no concave curved walls, so the inscribed mesh can only be smaller.
fn check_solid(name: &str, b: &Brep, chord: f64, radius: f64, convex: bool) {
    // A surface curved in both directions loses volume in both: twice the budget of a ruled one.
    let doubly_curved = name == "sphere" || name == "torus";
    let k = if doubly_curved { 3.0 } else { 1.5 };
    let (volume_tol, area_tol) = (k * chord / radius, k * chord / radius);
    let exact = solid_properties(b, b.solid_ids().next().unwrap()).unwrap();
    let mesh = tessellate(b, chord).unwrap();
    let report = mesh.check();
    assert!(report.is_watertight(), "{name}: {report:?}");
    let (v, a) = (mesh.volume(), mesh.area());
    assert!(v > 0.0, "{name}: the mesh faces inward ({v})");
    assert!(((v - exact.volume) / exact.volume).abs() < volume_tol, "{name}: volume {v} vs {}", exact.volume);
    assert!(((a - exact.area) / exact.area).abs() < area_tol, "{name}: area {a} vs {}", exact.area);
    if convex {
        assert!(v <= exact.volume * (1.0 + 1e-9), "{name}: inscribed volume {v} exceeds {}", exact.volume);
    }
    println!(
        "{name}: volume error {:.3e}, area error {:.3e}, budget {volume_tol:.3e}",
        (v - exact.volume) / exact.volume,
        (a - exact.area) / exact.area
    );
    let dev = max_deviation(b, chord);
    println!("{name}: deviation {dev:.3e} (chord {chord:e})");
    assert!(dev <= chord, "{name}: deviation {dev:.3e} exceeds the chord {chord:e}");
}

#[test]
fn a_box_is_twelve_triangles_and_exact() {
    let b = cuboid(&Frame::WORLD, 2.0, 3.0, 5.0, &T).unwrap();
    let mesh = tessellate(&b, 0.01).unwrap();
    assert!(mesh.check().is_watertight());
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(mesh.vertices.len(), 8);
    assert!((mesh.volume() - 30.0).abs() < 1e-12);
    assert!((mesh.area() - 62.0).abs() < 1e-12);
}

#[test]
fn curved_primitives_are_watertight_and_within_the_chord() {
    let w = Frame::WORLD;
    let chord = 0.01;
    check_solid("cylinder", &cylinder(&w, 3.0, 8.0, &T).unwrap(), chord, 3.0, true);
    check_solid("cone", &cone(&w, 3.0, 0.0, 5.0, &T).unwrap(), chord, 3.0, true);
    check_solid("inverted cone", &cone(&w, 0.0, 3.0, 5.0, &T).unwrap(), chord, 3.0, true);
    check_solid("frustum", &cone(&w, 3.0, 1.5, 5.0, &T).unwrap(), chord, 1.5, true);
    check_solid("sphere", &sphere(&w, 3.0, &T).unwrap(), chord, 3.0, true);
    check_solid("torus", &torus(&w, 6.0, 2.0, &T).unwrap(), chord, 2.0, false);
}

#[test]
fn a_plate_with_round_and_square_holes_is_watertight() {
    let outer = Profile2::rectangle(0.0, 0.0, 20.0, 10.0).unwrap();
    let holes = vec![Profile2::circle(Vec2::new(5.0, 5.0), 2.0).unwrap(), Profile2::rectangle(12.0, 3.0, 16.0, 7.0).unwrap()];
    let b = extrude(&Profile { outer, holes }, &Frame::WORLD, 3.0, &T).unwrap();
    check_solid("plate", &b, 0.01, 2.0, false);
}

#[test]
fn the_error_shrinks_as_the_chord_does() {
    let b = sphere(&Frame::WORLD, 2.0, &T).unwrap();
    let exact = 4.0 / 3.0 * PI * 8.0;
    let coarse = (tessellate(&b, 0.05).unwrap().volume() - exact).abs();
    let fine = (tessellate(&b, 0.005).unwrap().volume() - exact).abs();
    assert!(fine < coarse / 5.0, "{fine:e} vs {coarse:e}");
}

#[test]
fn bad_chords_are_refused() {
    let b = cuboid(&Frame::WORLD, 1.0, 1.0, 1.0, &T).unwrap();
    for chord in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(tessellate(&b, chord).is_err());
    }
    let s = sphere(&Frame::WORLD, 1000.0, &T).unwrap();
    assert!(tessellate(&s, 1e-9).is_err(), "an absurd chord must be refused, not run forever");
}
