# Roadmap: `nodalia-brep`, Nodalia's own B-rep kernel

`crates/brep` (package `nodalia-brep`) is a pure-Rust, std-only B-rep kernel written from scratch,
clean room. It is meant to replace, one day and only through `crates/kernel`, the vendored
`solidcore` (a port of a third-party kernel). Its [README](../crates/brep/README.md) says what exists
and how it was verified. This page says what is missing, in what order to build it, and how Torno
could switch.

**Where it stands (first milestone):** numerics, curves, surfaces, topology, a validity checker,
primitive constructors, exact mass properties and watertight tessellation, all marked
*experimental*. No booleans, no surface intersection, no fillets, no file I/O. Torno does not use
it, and nothing in Torno changes because of it.

Maturity scale used here and in the README: not implemented, planned, experimental, partially
functional, functional, validated, production-mature. A part moves up only with evidence, and the
evidence for "validated" is real designs and the parity gates below, not unit tests.

## 1. What Torno uses from solidcore today

`crates/kernel` is the one place Torno talks to solidcore (`solidcore-ops`, `solidcore-tess`,
`solidcore-record`; see [layers.md](layers.md)). The module list and re-exports of
`crates/kernel/src/lib.rs` (about 65,000 lines in `crates/kernel/src`) are the surface that a
replacement has to meet. Grouped by what each needs from a kernel:

| Group | Torno's API (examples from `crates/kernel`) | What a native kernel needs | Estimate |
|---|---|---|---|
| Primitives and profiles | `box_solid`, `cylinder`, `sphere`, `torus`, `extrude`, `extrude_tapered`, `revolve`, `revolve_whole` | Constructors on analytic surfaces; tapers need cones/planes at an angle; partial revolves need partial seams | Done for full primitives, extrude with holes and full revolve (experimental). Partial revolve, taper: small |
| Booleans | `boolean`, `BoolOp`, `union_all`, `split_by_plane`, `split_body`, `faceted_boolean`, `half_space`, `apart` | Surface/surface intersection for every pair of analytic types and NURBS; edge/face splitting; inside/outside classification; coincident, tangent and near-tangent handling (the kernel ladder's tier 02 has 25 such cases, down to 1e-6 mm gaps); healing | The core of the project. Planar-only: months. Analytic pairs: many months more. NURBS: open-ended |
| Fillets, chamfers | `fillet`, `fillet_variable`, `fillet_g2`, `fillet_tapered`, `chamfer`, `chamfer_sides`, setback and corner variants | Rolling-ball blend surfaces, trimming and corner patches, rebuilding topology, failure reporting. Needs booleans' intersection machinery and NURBS | Unknown. The hardest single area; plausibly a person-year or more on its own for the cases Torno's ladder tier 07 has (21) |
| Shells and offsets | `shell`, `offset_faces`, `offset_surface`, `draft`, `draft_corners` | Offset surfaces (exact for analytic ones), self-intersection removal, local boolean-like repair | Analytic offsets: moderate. General: unknown |
| Sweeps and lofts | `sweep`, `sweep_with`, `sweep_helix`, `loft`, `loft_with_rails`, `loft_blend`, `bridge`, threads (ladder tier 06, 30 cases) | NURBS surface construction and fitting, frames along a path, interpolation, trimming | Large; needs NURBS beyond evaluation |
| Free-form surfaces | `boundary_surface`, `fill`, `match_surface`, `surface_ruled`, `surface_extend`, `surface_trim`, `untrim`, `fair`, curvature comb, continuity analysis, `tspline` | NURBS algebra: knot insertion (exists), degree elevation, fitting, trimming, intersection, fairing | Large, and mostly separable from solids |
| Measurement | `measure`, `measure_mass`, `body_distance`, `point_closest`, `edges_closest` | Mass properties (exist for analytic faces), closest point (exists for single curves and surfaces), body-to-body distance | Mass: extend to NURBS faces. Distance: moderate |
| Interchange | `step_export`, `records_import`, `iges_import`, `iges_export_bodies`, native record encode/decode (`brep_record`, `decode_native_body`) | A mapping between this crate's arena and Torno's native `solidcore_record::brep` data model (and STEP's entities) | Moderate; mostly data work. STEP geometry beyond the analytic types needs the NURBS items above |
| Repair and sewing | `sew`, `sew_touching`, `repair`, `meshbody`, `mesh_convert`, `delete_face` | Tolerant topology merging, gap filling, face rebuilding. The checker's findings are the starting point | Moderate to large |
| Display tessellation | `solidcore-tess`: meshes and display plans, with trimmed faces | Exists here for analytic faces (watertight); NURBS faces, view-dependent plans and per-face meshing for the GPU plan are missing | Moderate |
| Sheet metal | ladder tier 12 (14 cases): flanges, closed corners, reliefs, hems, folds, flat pattern | Built on booleans, offsets and cylinder/plane bends; the flat pattern is mostly 2D geometry | Depends on booleans; then moderate |
| Assemblies and history | persistent naming, provenance, face sources (`provenance`, `history`) | A naming scheme for faces/edges that survives operations; the arena ids here are not stable across operations | Design work, not yet started |

**Honest estimate to full parity with what solidcore does for Torno today:** unknown. The sensible
reading of the table is that booleans plus intersection are the gate for everything else, that
fillets and free-form work are each larger than everything built so far, and that parity on the
general case is a multi-year effort for a small team. A narrower goal is much closer: *a native
kernel that does what Torno's prismatic parts need* (planar and analytic-quadric booleans,
chamfers, simple fillets, shells, holes, extrudes and revolves, mass properties, tessellation and
STEP in/out) is, as a guess to be revised after milestone M2, one to two person-years.

## 2. Milestones

Each milestone ends with the evidence listed. A milestone's parts stay "experimental" until Torno's
own cases pass on them (section 3).

| # | Milestone | Content | Evidence required |
|---|---|---|---|
| M1 (done) | Foundation | Tolerance, vectors, exact predicates, curves, surfaces, topology arena, validity checker, constructors, mass properties, tessellation | The README's verification table; 104 tests |
| M2 | Intersection of analytic surfaces | Plane/plane, plane/quadric, quadric/quadric curve tracing with exact classification of tangent and coincident cases; edge/face and edge/edge splitting; the arena operations to split a face by a curve | Analytic answers for each pair, including the degenerate classes in ladder tier 02; property tests: split then rejoin is the identity |
| M3 | Booleans on planar and analytic-quadric solids | Imprint, classify, select, stitch, heal; coincident and near-tangent faces; results checked by `validate` | Ladder tiers 01-03 and the boolean cases of `tests/kernel-cases/booleans`, with volumes against analytic values; `validate` clean on every result; fuzzing against the mesh-based oracle (`faceted_boolean`) |
| M4 | Native record bridge | Convert this crate's solids to and from the native B-rep record; STEP export of what the kernel builds; tessellation behind the display-plan interface | Round-trip identical to the record on the ladder's bodies; the rendered meshes match the current ones within the chord |
| M5 | Chamfers, shells, offsets, drafts | Analytic cases first: planar chamfers, cylinder/cone/sphere offsets | Ladder tiers 08-09 |
| M6 | NURBS foundations | Degree elevation, fitting, trimming, NURBS surface intersection, NURBS faces in mass properties and tessellation | Analytic NURBS representations of quadrics agree with the analytic versions; ladder tier 04 |
| M7 | Fillets | Constant radius on analytic edges, then variable, corners, G2 | Ladder tier 07 and `tests/kernel-cases/fillets` |
| M8 | Sweeps, lofts, threads, sheet metal | Ladder tiers 04, 06, 12 | The tiers |
| M9 | Persistent naming and history | Stable ids across operations | The timeline's rebuild tests (document layer) |

Order matters: M2 is the gate for M3, M5, M7 and M8. If M2 or M3 stalls, the later milestones are
not worth starting; say so rather than build around it.

### Booleans: design (not implemented)

This milestone was out of scope for the first one because nothing below it is mature enough
(surface/surface intersection does not exist, and the validity checker has only seen this crate's
own solids). The design that M2 and M3 would implement:

1. **Imprint.** For two solids, compute the intersection curves of every pair of faces whose
   bounding boxes overlap, using exact predicates for the topological decisions (is a vertex on
   which side) and tolerances only for the geometric ones. Split edges at curve ends and faces
   along curves; share the new vertices and edges between the solids.
2. **Classify.** Each face piece of one solid is inside, outside, or on the boundary of the other:
   by a ray cast with exact orientation tests for a point of the piece, and by comparing normals for
   pieces lying on the other's boundary (same direction or opposite).
3. **Select and stitch.** Keep the pieces the operation asks for (union: outside plus same-direction
   on; difference: outside of A, inside of B reversed, and so on), and glue shared edges into closed
   shells with the arena's ownership rules.
4. **Heal and check.** Merge coplanar neighbours when asked, drop zero-area pieces, then run
   `validate`; an invalid result is an error, never a returned solid.
5. **Degenerate classes** (tangent along a line or circle, vertex-on-face, edges touching, gaps of
   1e-6 mm) are decided in step 1 by exact sign evaluation where the geometry allows it; where it
   does not (curved tangency), the tolerance model decides and the decision is reported.

The risks are in step 1 for curved pairs (tracing, singular points, closed loops that miss every
sample) and in step 5. They are why the plan starts with planar and quadric solids and uses the
ladder's degenerate cases as the specification.

## 3. Switching Torno over: only behind `crates/kernel`, only after parity gates

Nothing switches at the end of M1, and the switch is never a flag-day. The rules:

1. **Seam first.** The new kernel is used only through `crates/kernel` (the one seam `cargo xtask
   layers` enforces); no other crate depends on `nodalia-brep`. It is a standalone L0 crate until
   then, and no Torno crate imports it today.
2. **Parity gates.** The existing regression rig is the specification: the kernel ladder
   ([kernel-ladder.md](kernel-ladder.md), 12 tiers, about 170 cases) and `tests/kernel-cases`
   (booleans, fillets, construction, continuity, crashes, assemblies). A tier is passed when every
   case passes on the new kernel with the same analytic or invariant answers, `validate` is clean on
   every result, and no case is more than a stated factor slower (to be set after M3 from measured
   times). No gate may be weakened to pass.
3. **Operation by operation, not wholesale.** The adapter in `crates/kernel` selects the kernel per
   operation. A preference (name to be chosen), default the current kernel, per
   `skills/torno-preferences`: a changed default keeps the old behaviour behind a preference.
4. **Shadow mode.** Before any default changes, run the new kernel beside solidcore on the same
   timeline inputs in CI and in a debug setting, compare volume, area, centroid, topology counts and
   the validity verdict, and record every disagreement as an issue. Disagreements where solidcore is
   the wrong one are findings too.
5. **Files.** The native record stays the interchange (`docs/native-brep.md`); design files keep
   opening. A saved model cache from either kernel must decode with both.
6. **Rollback.** The old path stays in the tree until a full release cycle has passed with the new
   one as the default and no gate regressed.
7. **Evidence of use** before any "validated" label: real designs from Torno's own corpus, not only
   the rig.

## 4. What would change the plan

- A mathematical shortcut that does not work: if curved/curved intersection with tolerant
  classification cannot be made reliable, booleans stay planar and the kernel does not replace
  solidcore for curved parts.
- Measured performance. None is measured yet; the predicates and the tessellator are written for
  correctness first.
- Persistent naming (M9). The document layer depends on it, and the arena here does not provide it.

This page is a plan, and the estimates are guesses. Revisit it after M2.
