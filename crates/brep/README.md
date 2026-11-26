# nodalia-brep

Nodalia's own boundary-representation (B-rep) kernel, written from scratch in pure Rust. It is the
first milestone of a long project: replacing the vendored third-party kernel behind `crates/kernel`
(`solidcore`) with a kernel that Nodalia owns end to end.

**Status: experimental. Nothing in Torno uses it.** It is a standalone crate (`cargo xtask layers`
lists it as such), with no dependencies (std only), no `unsafe` code and no panics on input. The
plan for how Torno could one day switch is in [`docs/brep-roadmap.md`](../../docs/brep-roadmap.md);
it starts with parity gates that are not met.

## Clean room

The crate was written from textbooks and papers: Shewchuk's adaptive-precision predicates
(1997), Piegl and Tiller's NURBS book, Sloan's constrained-triangulation flips, Bowyer–Watson
insertion, Green's theorem. The sources of solidcore, NVIDIA's SMLib and truck were not read and
nothing was copied from them (`AGENTS.md`, "Independent"). Nothing here depends on solidcore or
truck.

## What exists

| Module | What it does |
|---|---|
| `tol` | The tolerance model: linear `1e-6 mm`, angular `1e-9 rad`, what each means, the coordinate range |
| `math` | `Vec3`, `Vec2`, `Mat3`, `Frame`, `Transform`; units mm and radians, Z up |
| `robust` | Exact `orient2d`, `orient3d`, `incircle` (floating-point filter, then expansion arithmetic) |
| `quad` | Gauss–Legendre quadrature with nodes computed at run time |
| `nurbs` | NURBS curves and surfaces: Cox–de Boor evaluation, derivatives, knot insertion, validated on construction |
| `curve` | Line, circle, ellipse, NURBS curve: points, derivatives, closest point, sampling |
| `surface` | Plane, cylinder, cone, sphere, torus, NURBS surface: points, partials, normals, inverse parametrisation, closest point |
| `topology` | An arena B-rep (vertex, edge, coedge, loop, face, shell, solid) with typed ids, checked on every use |
| `uv` | Face loops unwrapped into the parameter plane, with pole closure, area and exact point-in-polygon |
| `check` | `Brep::validate`: an exhaustive validity checker (see below) |
| `profile` | Planar line-and-arc profiles with holes, resolved into oriented pieces |
| `build` | Constructors: box, cylinder, cone, sphere, torus, extrude (with holes), revolve |
| `mass` | Volume, surface area and centroid by Green's theorem, with Gauss–Legendre quadrature |
| `cdt` | Constrained Delaunay triangulation with exact predicates |
| `mesh` | Triangle meshes: area, volume, centroid, watertightness check |
| `tessellate` | Watertight meshes of a solid, with a chord tolerance |

### The validity checker

`Brep::validate(&Tolerance)` returns every finding, not the first. It checks, per shell: that
every edge is used by exactly two coedges of opposite orientation; that loops close and chain
vertex to vertex; that every edge lies on both of its faces' surfaces (sampled, within the linear
tolerance); that vertices lie on their edges and faces; that outer loops and holes have the right
orientation and holes lie inside their outer loop (exact point-in-polygon); that the Euler–Poincaré
formula `V − E + F − R = 2 − 2g` holds with an integral genus; that the enclosed volume is
positive; and that ids, ownership and finite numbers are sound. Each defect class has a test that
builds a solid, damages it in exactly that way, and expects exactly that finding.

## What is verified, and how

Every number below comes from a test in this crate that was run (`cargo test -p nodalia-brep`:
81 unit tests, 7 in `tests/check.rs`, 11 in `tests/primitives.rs`, 5 in `tests/tessellate.rs`,
all passing at the last run). "Verified" means: agrees with the stated reference within the stated
tolerance on the cases tested. It does not mean proved.

| Claim | Reference | Tolerance / result |
|---|---|---|
| `orient2d`, `orient3d`, `incircle` return the exact sign | an `i128` integer evaluation on random, near-degenerate, coplanar and cocircular integer input scaled by powers of two; the tests also count that plain floating point gets some of those cases wrong | zero disagreements on the cases tested |
| Predicate input domain | finite, `|x| ≤ 1e30`; nonzero `|x| < 1e-30` is flushed to zero; anything else is an error | tested |
| Expansion arithmetic is exact | sum and scale against `i128` | exact |
| Quadrature | polynomials up to degree `2n − 1` | rounding |
| Curve and surface evaluation | closed forms; first derivatives against finite differences; analytic normals against `Sᵤ × Sᵥ` | derivatives `1e-6` relative (second derivatives `1e-3`, finite-difference limited); normals `1e-9`; inverse parametrisation `1e-10` |
| Closest point | brute-force sampling and closed forms | per test |
| Primitive volume, area, centroid (box, cylinder, cone, frustum, sphere, torus, extrudes, revolves, placed frames) | closed-form values (`πr²h`, `4πr²`, `2π²Rr²`, ...) | relative error about `1e-12` |
| Euler characteristic and genus of every constructor | by hand (sphere V=1 E=1 F=2; torus V=1 E=2 F=1, genus 1) | exact |
| Validity checker | every constructor passes; each of the defect tests fails with its own finding | as tested |
| Tessellation is watertight | `Mesh::check`: every directed edge has its opposite, none repeats | every tested solid: box, plate with round and square holes, cylinder, cone, inverted cone, frustum, sphere, torus |
| Tessellation volume and area | the exact values above | within `1.5 · chord / radius` relative (`3.0` for sphere and torus); measured about `4e-3` at chord `1e-2` on a sphere of radius 1 |
| Tessellation deviation from the surface | triangle centroids and edge midpoints projected with `Surface::closest_point` | at most `0.83–0.99` of the requested chord on the tested solids, **at sampled points only** |

## What does not exist, or is limited

- **No booleans.** No face/face intersection, no classification, no healing. A design for planar
  booleans is in the roadmap; none is implemented.
- **No fillets, chamfers, shells, drafts, offsets, sweeps, lofts, patterns, sheet metal, threads.**
- **No STEP, IGES or native-record I/O.** The kernel cannot read or write a file.
- **NURBS faces are not supported by mass properties or tessellation** (they return
  `BrepError::Unsupported`). NURBS curves and surfaces exist and evaluate, nothing builds a solid
  from them yet.
- Revolves: only full revolutions; only profiles on one side of the axis; spindle and apple tori
  (a circle touching or crossing the axis) are refused; a loop that passes through a pole is refused.
- Extrude and revolve profiles are lines and arcs only; hole nesting is checked on polygonal
  approximations of arcs.
- Tessellation cannot yet mesh a face on a sphere or cone whose hole touches the pole.
- Faces on different surfaces are not checked against each other (no self-intersection test), and a
  void inside an outer shell is not verified to lie inside it.
- The tessellation deviation bound is measured, not proved: the grid step is derived from the
  sagitta of a circle of the surface's largest radius, times safety factors found by experiment
  (`GRID_SAFETY`, `TORUS_SHRINK`). Do not rely on it for surfaces outside the tested set.
- Performance has not been measured at all.
- Single-threaded; no incremental or parallel anything.

## Maturity

Scale: not implemented / planned / experimental / partially functional / functional / validated /
production-mature. Nothing goes beyond "experimental" without evidence from real use, and none
exists yet.

| Part | Maturity | Why |
|---|---|---|
| Tolerance model | experimental | documented, but never exercised by a long operation chain |
| Exact predicates | experimental | strong agreement with an exact oracle; no fuzzing against an independent library, no performance data |
| Vectors, frames, transforms | experimental | unit-tested |
| Quadrature | experimental | |
| Curves (line, circle, ellipse) | experimental | |
| Surfaces (plane, cylinder, cone, sphere, torus) | experimental | |
| NURBS curve and surface | experimental | evaluation and knot insertion only; no fitting, no intersection |
| Topology arena | experimental | |
| Validity checker | experimental | defect tests are synthetic; never run on a foreign model |
| Constructors | experimental | closed-form agreement on primitives only |
| Mass properties | experimental | analytic faces only |
| Constrained Delaunay | experimental | five tests; no stress on adversarial point sets |
| Tessellation | experimental | tested solids only, measured not proved |
| Booleans | not implemented | design only |
| Intersection (surface/surface) | not implemented | |
| Fillets, chamfers, shells, drafts, offsets | not implemented | |
| Sweeps, lofts | not implemented | |
| STEP and file I/O | not implemented | |
| Sheet metal | not implemented | |

## Rules

- std only: no dependencies, so nothing to audit and nothing that can pull in a copyleft licence.
- `unsafe_code` is forbidden; `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`,
  `unreachable!` are denied outside tests. Every constructor validates its input and returns
  `BrepError`; indices and sizes derived from input are checked or capped.
- Every source file carries `SPDX-License-Identifier: MIT OR Apache-2.0`.
- It builds for `wasm32-unknown-unknown` (`cargo check -p nodalia-brep --target wasm32-unknown-unknown` passed).

## Conventions

- Units: millimetres and radians. Right-handed, Z up.
- The natural normal of a surface is `Sᵤ × Sᵥ`. A face's outward normal is the natural normal when
  `same_sense` is true and its opposite otherwise.
- Seen from outside, outer loops run counter-clockwise and holes clockwise (material on the left).
  Every edge of a closed shell is used by two coedges of opposite orientation.
- Cone `v` is signed distance along the axis from the apex; `v < 0` is the opposite nappe. Sphere
  `v` is latitude. Torus parameters are both periodic angles.

## Using it

```rust
use nodalia_brep::{build, mass, tessellate, Frame, Tolerance};

let tol = Tolerance::default();
let solid = build::cylinder(&Frame::WORLD, 10.0, 40.0, &tol)?;
assert!(solid.validate(&tol).is_valid());
let props = mass::solid_properties(&solid, solid.solid_ids().next().ok_or("no solid")?)?;
let mesh = tessellate::tessellate(&solid, 0.01)?;
assert!(mesh.check().is_watertight());
```

The same flow is the test `the_readme_example_runs` in `tests/primitives.rs`; the rustdoc has the exact signatures.
