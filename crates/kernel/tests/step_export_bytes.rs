//! Torno's STEP export, byte for byte, and its round trip.
//!
//! Every STEP file in the repository's STEP fixtures (`tests/step-conformance`,
//! `tests/step-regressions`, the import kernel cases and solidcore-step's fixtures) is read
//! and written back through `step_export_bodies`. The written text's digest must be the one in
//! [`DIGESTS`]. These digests were taken from the export as it was before the AP242 product layer
//! moved into solidcore-step, except the torus-ring spline whose exact torus support is now
//! proved and written analytically (#16297). A small assembly
//! with names and a colour goes through `step_export_products` the same way.
//!
//! The written file is then read again: the same bodies, each with the same face count and its
//! volume within 1e-6 (relative) of the first read.
//!
//! `STEP_EXPORT_BYTES_PRINT=1` prints the table instead of checking it.

use solidcore_record::Vec3;
use torno_kernel::{ExportBody, ExportProduct, StepHeader, box_solid, cylinder, measure, step_export_bodies, step_export_products, step_import};

/// FNV-1a, 64 bits: a stable digest of the written text.
fn digest(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The fixtures, relative to the repository root, in sorted order.
fn fixtures(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut walk = vec![
        root.join("tests/step-conformance"),
        root.join("tests/step-regressions"),
        root.join("tests/kernel-cases/import"),
        root.join("solidcore/crates/solidcore-step/tests/fixtures"),
    ];
    while let Some(dir) = walk.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("step") || x.eq_ignore_ascii_case("stp")) {
                out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// Fixtures whose export is written but does not read back: a face that winds round its axis more
/// than a turn is written on its multi-turn spline form, and the reader makes no pcurves on a
/// spline form that overlaps itself (its edge's pcurve comes out on another sheet, 6 mm off).
/// The export's bytes are still pinned below. Tracked in #17080. Shrinks only.
const NO_READ_BACK: &[&str] = &["tests/step-regressions/thread_crest_band.step"];

/// (fixture, digest of its export) as written before the move, plus new fixtures; the torus-ring spline
/// is now an exact torus (#16297), and an edge on a rational spline that is exactly a circular arc
/// is written as that circle (#17929). The header names Torno as the originating system, so every
/// digest was taken again when the program was renamed (and matched the old ones with the old name).
#[rustfmt::skip]
const DIGESTS: &[(&str, &str)] = &[
    ("solidcore/crates/solidcore-step/tests/fixtures/closed-spline-cylinder.step", "1a59b7597684f24d"),
    ("solidcore/crates/solidcore-step/tests/fixtures/discord-2026-10-30-fillet.step", "6acf06a9039ecfc2"),
    ("solidcore/crates/solidcore-step/tests/fixtures/rod_doubled_edge.step", "594ef41434879021"),
    ("tests/kernel-cases/import/1721-closed-bspline-tube-face/input.step", "de5006ebfed72e9c"),
    ("tests/kernel-cases/import/2043-outer-bound-opposing-surface-sense/input.step", "af82368b2ceaab64"),
    ("tests/kernel-cases/import/258-cone-point-vertex/input.step", "9e8a4589de016fa9"),
    ("tests/kernel-cases/import/258-cone-to-point/input.step", "f718d34b1229bb9c"),
    ("tests/kernel-cases/import/258-tiny-pin/input.step", "61592e92c1b250cc"),
    ("tests/kernel-cases/import/3098-bspline-edges-over-tiny-knot-span/input.step", "7433de290e3074bd"),
    ("tests/kernel-cases/import/3208-extruded-ellipse-face-over-parameter-seam/input.step", "485336587b1c2295"),
    ("tests/kernel-cases/import/3352-vertex-off-its-edge-curve/input.step", "0a289093f27945fc"),
    ("tests/kernel-cases/import/3730-void-oriented-shell-false/input.step", "5fdfa910e1babe73"),
    ("tests/kernel-cases/import/3880-cylinder-seam-edge/input.step", "2ab3ce3ef5d0bab0"),
    ("tests/kernel-cases/import/3880-nurbs-hemisphere-dome/input.step", "0dd1a38ac6a0314d"),
    ("tests/kernel-cases/import/3963-closed-spline-edge-vertex-past-start/input.step", "8a0cd448ec48661c"),
    ("tests/kernel-cases/import/4005-cylinder-written-as-near-zero-cone/input.step", "37011280a5d79abc"),
    ("tests/kernel-cases/import/4006-plane-written-as-cone-of-nearly-right-angle/input.step", "b709183851f9f55a"),
    ("tests/step-conformance/cone_point.step", "5634a5c67f91e70e"),
    ("tests/step-conformance/cone_point_vertex.step", "12246838f8ab4466"),
    ("tests/step-conformance/cylinder.step", "e27deefa77622dc3"),
    ("tests/step-conformance/cylinder_inch.step", "68788e1657cd9999"),
    ("tests/step-conformance/cylinder_nurbs.step", "9648c609d53509a1"),
    ("tests/step-conformance/cylinder_placed.step", "8635ebe31a2e32ba"),
    ("tests/step-conformance/cylinder_seam.step", "782ec1011c461797"),
    ("tests/step-conformance/cylinder_twice.step", "ea1e374530bb7290"),
    ("tests/step-conformance/dome.step", "0bd3d4fecafa2a20"),
    ("tests/step-conformance/dome_nurbs.step", "817ecbf91fe70491"),
    ("tests/step-conformance/far_cylinder.step", "7252b747cf909569"),
    ("tests/step-conformance/grooved_shaft.step", "6f2c642342e799ec"),
    ("tests/step-conformance/plate_100_holes.step", "71f808f6abdebdab"),
    ("tests/step-conformance/sheet_disc.step", "51de67cb876e0caf"),
    ("tests/step-conformance/tiny_pin.step", "865cf24c2b95220a"),
    ("tests/step-conformance/torus_ring_nurbs.step", "0328de3bc1a3e5da"),
    ("tests/step-conformance/washer.step", "68aeb221baddedfe"),
    ("tests/step-regressions/block_edge_off_its_face.step", "5fa49ce8e894cef0"),
    ("tests/step-regressions/cylinder_rim_off_its_wall.step", "e758ce84127f1eab"),
    ("tests/step-regressions/cylinder_spline_rims.step", "1adf7c4eaac57500"),
    ("tests/step-regressions/hole_round_revolved.step", "aa06d5eb8ce2b346"),
    ("tests/step-regressions/rod_doubled_edge.step", "594ef41434879021"),
    ("tests/step-regressions/thread_crest_band.step", "b772f64a89fef578"),
    ("tests/step-regressions/tiny_rounds_coarse_tolerance.step", "c8ea13c73c62132f"),
    ("tests/step-regressions/torus_patch_and_rest.step", "ce78a844922b2455"),
    ("tests/step-regressions/torus_patch_and_rest_revolved.step", "0f230b151c21545c"),
    ("(assembly)", "e8a5335b71cc2b14"),
];

/// Each body's (face count, volume) as read.
fn measures(bodies: &[torno_kernel::ImportedBody]) -> Vec<(usize, f64)> {
    bodies
        .iter()
        .map(|b| {
            let m = measure(&b.body).unwrap();
            (m.faces, m.volume)
        })
        .collect()
}

/// The export reads back as the same bodies: face counts equal, volumes within 1e-6.
fn round_trip(name: &str, first: &[(usize, f64)], text: &str) -> Result<(), String> {
    let again = step_import(text).map_err(|e| format!("{name}: the export does not read back: {e}"))?;
    let second = measures(&again.bodies);
    if first.len() != second.len() {
        return Err(format!("{name}: {} bodies read back as {}", first.len(), second.len()));
    }
    for (i, ((f0, v0), (f1, v1))) in first.iter().zip(&second).enumerate() {
        if f0 != f1 {
            return Err(format!("{name}: body {i}: {f0} faces read back as {f1}"));
        }
        if (v0 - v1).abs() > 1e-6 * v0.abs().max(v1.abs()) {
            return Err(format!("{name}: body {i}: volume {v0} read back as {v1}"));
        }
    }
    Ok(())
}

#[test]
fn step_export_keeps_its_bytes_and_reads_back() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let print = std::env::var_os("STEP_EXPORT_BYTES_PRINT").is_some();
    let mut table: Vec<(String, String)> = Vec::new();
    let mut failures = Vec::new();
    for name in fixtures(&root) {
        let text = std::fs::read_to_string(root.join(&name)).unwrap();
        let imp = step_import(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let bodies: Vec<ExportBody> = imp.bodies.iter().map(|b| ExportBody { name: b.name.clone(), body: &b.body, color: b.color }).collect();
        let file_name = name.rsplit('/').next().unwrap_or(&name).to_string();
        let out = step_export_bodies(&bodies, &StepHeader { file_name, ..Default::default() }).unwrap_or_else(|e| panic!("{name}: {e}"));
        table.push((name.clone(), digest(&out)));
        let read_back = round_trip(&name, &measures(&imp.bodies), &out);
        if NO_READ_BACK.contains(&name.as_str()) {
            // Still the known gap: when it is fixed this fails, and the fixture leaves the list.
            if read_back.is_ok() {
                failures.push(format!("{name}: reads back now; take it off NO_READ_BACK"));
            }
        } else if let Err(e) = read_back {
            failures.push(e);
        }
    }
    // An assembly: a part placed twice, names (one not ASCII) and a colour.
    let bx = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 5.0)).unwrap();
    let pin = cylinder(Vec3::ZERO, Vec3::Z, 2.0, 8.0).unwrap();
    let rot90 = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [50.0, 0.0, 0.0, 1.0]];
    let shift = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 5.0, 1.0]];
    let products = vec![
        ExportProduct {
            name: "Root Ä".into(),
            bodies: vec![ExportBody { name: "Base".into(), body: &bx, color: Some([1.0, 0.0, 0.0]) }],
            children: vec![(1, shift, "Pin:1".into()), (1, rot90, String::new())],
        },
        ExportProduct { name: "Pin".into(), bodies: vec![ExportBody { name: "Shaft".into(), body: &pin, color: None }], children: vec![] },
    ];
    let header = StepHeader {
        file_name: "assembly.step".into(),
        time_stamp: "2026-11-12T00:00:00".into(),
        author: "a".into(),
        organization: "o".into(),
        description: "d".into(),
    };
    let out = step_export_products(&products, 0, &header).unwrap();
    table.push(("(assembly)".into(), digest(&out)));
    if print {
        for (name, d) in &table {
            println!("    (\"{name}\", \"{d}\"),");
        }
        return;
    }
    let expected: Vec<(String, String)> = DIGESTS.iter().map(|(n, d)| ((*n).to_string(), (*d).to_string())).collect();
    assert_eq!(table, expected, "the export's bytes changed");
    assert!(failures.is_empty(), "round trips: {failures:#?}");
}
