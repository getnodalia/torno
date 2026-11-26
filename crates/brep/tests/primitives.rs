// SPDX-License-Identifier: MIT OR Apache-2.0
//! The primitive solids: topology counts, mass properties against closed forms.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use nodalia_brep::build::{cone, cuboid, cylinder, extrude, revolve, sphere, torus};
use nodalia_brep::mass::solid_properties;
use nodalia_brep::profile::{Profile, Profile2, Seg};
use nodalia_brep::topology::{Brep, SolidId};
use nodalia_brep::{Frame, Tolerance, Vec2, Vec3};
use std::f64::consts::{PI, TAU};

const T: Tolerance = Tolerance::DEFAULT;

fn counts(b: &Brep) -> (usize, usize, usize) {
    (b.vertex_count(), b.edge_count(), b.face_count())
}

fn solid0(b: &Brep) -> SolidId {
    b.solid_ids().next().unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-300)
}

#[test]
fn a_box_has_eight_vertices_and_exact_mass_properties() {
    let b = cuboid(&Frame::WORLD, 2.0, 3.0, 5.0, &T).unwrap();
    assert_eq!(counts(&b), (8, 12, 6));
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, 30.0) < 1e-13, "{}", m.volume);
    assert!(rel(m.area, 2.0 * (6.0 + 10.0 + 15.0)) < 1e-13, "{}", m.area);
    assert!(m.centroid.distance(Vec3::new(1.0, 1.5, 2.5)) < 1e-12, "{:?}", m.centroid);
}

#[test]
fn a_cylinder_has_the_seam_topology_and_closed_form_properties() {
    let (r, h) = (2.5, 7.0);
    let b = cylinder(&Frame::WORLD, r, h, &T).unwrap();
    assert_eq!(counts(&b), (2, 3, 3));
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, PI * r * r * h) < 1e-12, "{}", m.volume);
    assert!(rel(m.area, TAU * r * (r + h)) < 1e-12, "{}", m.area);
    assert!(m.centroid.distance(Vec3::new(0.0, 0.0, h / 2.0)) < 1e-12, "{:?}", m.centroid);
}

#[test]
fn a_cone_and_a_frustum() {
    let (r, h) = (3.0, 4.0);
    let b = cone(&Frame::WORLD, r, 0.0, h, &T).unwrap();
    assert_eq!(counts(&b), (1, 1, 2));
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, PI * r * r * h / 3.0) < 1e-12, "{}", m.volume);
    let slant = (r * r + h * h).sqrt();
    assert!(rel(m.area, PI * r * (r + slant)) < 1e-12, "{}", m.area);
    assert!(m.centroid.distance(Vec3::new(0.0, 0.0, h / 4.0)) < 1e-12, "{:?}", m.centroid);

    let (r0, r1) = (3.0, 1.0);
    let f = cone(&Frame::WORLD, r0, r1, h, &T).unwrap();
    assert_eq!(counts(&f), (2, 3, 3));
    let m = solid_properties(&f, solid0(&f)).unwrap();
    assert!(rel(m.volume, PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1)) < 1e-12, "{}", m.volume);
    let slant = ((r0 - r1).powi(2) + h * h).sqrt();
    assert!(rel(m.area, PI * (r0 * r0 + r1 * r1 + (r0 + r1) * slant)) < 1e-12, "{}", m.area);
    // The apex is above the narrow end: the cone upside down has the apex at the bottom.
    let up = cone(&Frame::WORLD, 0.0, r, h, &T).unwrap();
    let m = solid_properties(&up, solid0(&up)).unwrap();
    assert!(rel(m.volume, PI * r * r * h / 3.0) < 1e-12);
    assert!(m.centroid.distance(Vec3::new(0.0, 0.0, 3.0 * h / 4.0)) < 1e-12, "{:?}", m.centroid);
}

#[test]
fn a_sphere_is_two_hemispheres_with_one_circle() {
    let r = 1.75;
    let b = sphere(&Frame::WORLD, r, &T).unwrap();
    assert_eq!(counts(&b), (1, 1, 2));
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, 4.0 / 3.0 * PI * r.powi(3)) < 1e-12, "{}", m.volume);
    assert!(rel(m.area, 4.0 * PI * r * r) < 1e-12, "{}", m.area);
    assert!(m.centroid.norm() < 1e-12, "{:?}", m.centroid);
}

#[test]
fn a_torus_has_one_face_and_genus_one() {
    let (big, small) = (5.0, 1.25);
    let b = torus(&Frame::WORLD, big, small, &T).unwrap();
    assert_eq!(counts(&b), (1, 2, 1));
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, 2.0 * PI * PI * big * small * small) < 1e-12, "{}", m.volume);
    assert!(rel(m.area, 4.0 * PI * PI * big * small) < 1e-12, "{}", m.area);
    assert!(m.centroid.norm() < 1e-11, "{:?}", m.centroid);
    assert!(torus(&Frame::WORLD, 1.0, 1.0, &T).is_err());
}

#[test]
fn extruding_a_profile_with_holes_subtracts_them() {
    let outer = Profile2::rectangle(0.0, 0.0, 10.0, 6.0).unwrap();
    let round = Profile2::circle(Vec2::new(3.0, 3.0), 1.5).unwrap();
    let square = Profile2::rectangle(6.0, 2.0, 8.0, 4.0).unwrap();
    let p = Profile { outer, holes: vec![round, square] };
    let h = 4.0;
    let b = extrude(&p, &Frame::WORLD, h, &T).unwrap();
    let area = 60.0 - PI * 2.25 - 4.0;
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, area * h) < 1e-12, "{}", m.volume);
    let lateral = h * (32.0 + TAU * 1.5 + 8.0);
    assert!(rel(m.area, 2.0 * area + lateral) < 1e-12, "{}", m.area);
    // Moment of the cross-section about x: rectangle minus the two holes.
    let cx = (60.0 * 5.0 - PI * 2.25 * 3.0 - 4.0 * 7.0) / area;
    assert!((m.centroid.x - cx).abs() < 1e-12, "{} vs {cx}", m.centroid.x);
}

#[test]
fn an_arc_profile_extrudes_to_curved_walls() {
    // A D shape: a half disk of radius 2 on a 4 x 1 rectangle.
    let p = Profile2 {
        start: Vec2::new(-2.0, 0.0),
        segs: vec![
            Seg::Line { to: Vec2::new(2.0, 0.0) },
            Seg::Line { to: Vec2::new(2.0, 1.0) },
            Seg::Arc { center: Vec2::new(0.0, 1.0), sweep: PI },
            Seg::Line { to: Vec2::new(-2.0, 0.0) },
        ],
    };
    let b = extrude(&Profile::new(p), &Frame::WORLD, 3.0, &T).unwrap();
    let area = 4.0 + 0.5 * PI * 4.0;
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, area * 3.0) < 1e-12, "{}", m.volume);
}

#[test]
fn revolving_a_washer_profile_makes_planar_annuli() {
    // The rectangle x in [2, 5], z in [0, 3] turned about the axis: a tube.
    let r = Profile2::rectangle(2.0, 0.0, 5.0, 3.0).unwrap();
    let b = revolve(&r, &Frame::WORLD, &T).unwrap();
    assert_eq!(counts(&b), (4, 4 + 2, 4));
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, PI * (25.0 - 4.0) * 3.0) < 1e-12, "{}", m.volume);
    assert!(rel(m.area, 2.0 * PI * (25.0 - 4.0) + TAU * 3.0 * 7.0) < 1e-12, "{}", m.area);
    assert!(m.centroid.distance(Vec3::new(0.0, 0.0, 1.5)) < 1e-12);
}

#[test]
fn a_placed_frame_moves_every_property_with_it() {
    let frame = Frame::new(Vec3::new(10.0, -4.0, 2.0), Vec3::new(1.0, 2.0, 2.0), Some(Vec3::X)).unwrap();
    let b = cylinder(&frame, 1.5, 4.0, &T).unwrap();
    let m = solid_properties(&b, solid0(&b)).unwrap();
    assert!(rel(m.volume, PI * 2.25 * 4.0) < 1e-12);
    assert!(m.centroid.distance(frame.to_world(Vec3::new(0.0, 0.0, 2.0))) < 1e-11, "{:?}", m.centroid);
    let s = sphere(&frame, 2.0, &T).unwrap();
    let m = solid_properties(&s, solid0(&s)).unwrap();
    assert!(rel(m.volume, 4.0 / 3.0 * PI * 8.0) < 1e-12);
    assert!(m.centroid.distance(frame.origin) < 1e-11);
}

#[test]
fn constructors_refuse_bad_dimensions_with_errors() {
    assert!(cuboid(&Frame::WORLD, 0.0, 1.0, 1.0, &T).is_err());
    assert!(cuboid(&Frame::WORLD, f64::NAN, 1.0, 1.0, &T).is_err());
    assert!(cylinder(&Frame::WORLD, -1.0, 1.0, &T).is_err());
    assert!(cylinder(&Frame::WORLD, 1.0, f64::INFINITY, &T).is_err());
    assert!(cone(&Frame::WORLD, 0.0, 0.0, 1.0, &T).is_err());
    assert!(sphere(&Frame::WORLD, 1e12, &T).is_err());
    let crossing = Profile2::rectangle(-1.0, 0.0, 1.0, 1.0).unwrap();
    assert!(revolve(&crossing, &Frame::WORLD, &T).is_err());
}

#[test]
fn the_readme_example_runs() {
    use nodalia_brep::tessellate::tessellate;
    let solid = cylinder(&Frame::WORLD, 10.0, 40.0, &T).unwrap();
    assert!(solid.validate(&T).is_valid());
    let props = solid_properties(&solid, solid0(&solid)).unwrap();
    assert!(rel(props.volume, std::f64::consts::PI * 100.0 * 40.0) < 1e-11);
    let mesh = tessellate(&solid, 0.01).unwrap();
    assert!(mesh.check().is_watertight());
}
