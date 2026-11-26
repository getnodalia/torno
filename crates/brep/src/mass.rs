// SPDX-License-Identifier: MIT OR Apache-2.0
//! Mass properties: volume, surface area and centroid of a solid, from its boundary.
//!
//! Every quantity is a surface integral over the faces (volume and the first moments by the
//! divergence theorem, with `F = p/3` and `F = ½ pᵢ² eᵢ`), and every surface integral over a
//! trimmed face is turned into a line integral over its boundary by Green's theorem in the
//! face's `(u, v)` plane:
//!
//! ```text
//!   ∬_D g du dv = ∮_∂D P du,        P(u, v) = −∫_{v₀}^{v} g(u, s) ds
//! ```
//!
//! The inner integral is a composite 8-point Gauss–Legendre rule (one panel suffices for the plane,
//! cylinder and cone, whose integrands are polynomial in `v`; a panel per `π/2` for the sphere
//! and torus). The outer integral runs along the true edge curves, panel by panel, 8 points per
//! panel, so no boundary is approximated by a polygon: for analytic faces the results are exact
//! to rounding (`~1e-13` relative in the tests). A loop that closes through a pole adds the
//! horizontal run along the pole.
//!
//! `u′` along an edge comes from solving `γ′ = Sᵤ u′ + Sᵥ v′` in the least-squares sense, so an
//! edge through a singular point of the parametrisation is reported instead of integrated.
//!
//! NURBS faces are not supported (their parameter recovery is not trustworthy enough for a
//! boundary integral); asking for them returns [`BrepError::Unsupported`].

use crate::error::{BrepError, Result};
use crate::math::Vec3;
use crate::quad::rule8;
use crate::surface::Surface;
use crate::topology::{Brep, FaceId, LoopId, ShellId, SolidId};
use crate::uv::{loop_points, loop_uv};
use std::f64::consts::FRAC_PI_2;

/// Chord used to cut loops into the sample points that establish their parameter-space winding.
const OUTLINE_CHORD: f64 = 1.0e-2;
/// Components of the integrated quantity: volume, area, first moments about x, y and z.
const N: usize = 5;

/// Integrals over one face; see [`face_integrals`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceIntegrals {
    /// The face's contribution to the enclosed volume (divergence theorem with the outward normal).
    pub volume: f64,
    pub area: f64,
    /// The face's contribution to `∫ p dV`.
    pub moment: Vec3,
}

/// Volume, area and centroid of a solid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    /// Total surface area, including the walls of voids.
    pub area: f64,
    pub centroid: Vec3,
}

fn integrand(surface: &Surface, u: f64, s: f64) -> [f64; N] {
    let q = surface.point(u, s);
    let (a, b) = surface.partials(u, s);
    let n = a.cross(b);
    [q.dot(n) / 3.0, n.norm(), 0.5 * q.x * q.x * n.x, 0.5 * q.y * q.y * n.y, 0.5 * q.z * q.z * n.z]
}

/// `P(u, v) = −∫_{v0}^{v} g(u, s) ds` for every component.
fn antiderivative(surface: &Surface, u: f64, v: f64, v0: f64) -> [f64; N] {
    let span = v - v0;
    let panels = match surface {
        Surface::Sphere(_) | Surface::Torus(_) => ((span.abs() / FRAC_PI_2).ceil() as usize).clamp(1, 4096),
        _ => 1,
    };
    let rule = rule8();
    let h = span / panels as f64;
    let mut acc = [0.0; N];
    for k in 0..panels {
        let (a, b) = (v0 + h * k as f64, v0 + h * (k + 1) as f64);
        let (half, mid) = (0.5 * (b - a), 0.5 * (a + b));
        for (x, w) in rule.nodes.iter().zip(&rule.weights) {
            let g = integrand(surface, u, mid + half * x);
            for (slot, gi) in acc.iter_mut().zip(g) {
                *slot += w * half * gi;
            }
        }
    }
    acc.map(|x| -x)
}

fn loop_integral(brep: &Brep, surface: &Surface, loop_id: LoopId, same_sense: bool) -> Result<[f64; N]> {
    let pts = loop_points(brep, loop_id, OUTLINE_CHORD)?;
    let outline = loop_uv(surface, &pts, same_sense)?;
    let first = *outline.points.first().ok_or_else(|| BrepError::degenerate("empty loop"))?;
    let v0 = first.y;
    let mut last = first;
    let mut total = [0.0; N];
    let rule = rule8();
    for &c in &brep.loop_(loop_id)?.coedges {
        let co = brep.coedge(c)?;
        let edge = brep.edge(co.edge)?;
        let ts = edge.curve.sample_params(edge.t0, edge.t1, OUTLINE_CHORD)?;
        let mut panels: Vec<(f64, f64)> = ts.windows(2).map(|w| (w[0], w[1])).collect();
        let dir = if co.reversed {
            panels.reverse();
            -1.0
        } else {
            1.0
        };
        for (a, b) in panels {
            let (half, mid) = (0.5 * (b - a), 0.5 * (a + b));
            for (x, w) in rule.nodes.iter().zip(&rule.weights) {
                let t = mid + half * x;
                let p = edge.curve.point(t);
                let d = edge.curve.derivative(t);
                let uv = surface.parameters(p, Some(last))?;
                last = uv;
                let (su, sv) = surface.partials(uv.x, uv.y);
                let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
                let det = e * g - f * f;
                if !(det > 1.0e-20 * e * g) || !det.is_finite() {
                    return Err(BrepError::degenerate("an edge passes through a singular point of its surface"));
                }
                let du = (g * su.dot(d) - f * sv.dot(d)) / det;
                let pv = antiderivative(surface, uv.x, uv.y, v0);
                for (slot, pi) in total.iter_mut().zip(pv) {
                    *slot += pi * du * w * half * dir;
                }
            }
        }
    }
    if let Some(closure) = outline.closure {
        let (u_from, u_to) = (closure.first_unwrapped.x, first.x);
        for (k, slot) in total.iter_mut().enumerate() {
            *slot += rule.composite(u_from, u_to, 8, |u| antiderivative(surface, u, closure.v_pole, v0)[k]);
        }
    }
    Ok(total)
}

/// The volume, area and first-moment contributions of one face.
pub fn face_integrals(brep: &Brep, face: FaceId) -> Result<FaceIntegrals> {
    let f = brep.face(face)?;
    if matches!(f.surface, Surface::Nurbs(_)) {
        return Err(BrepError::Unsupported("mass properties of faces on NURBS surfaces".into()));
    }
    let mut total = [0.0; N];
    for l in brep.face_loops(face)? {
        let part = loop_integral(brep, &f.surface, l, f.same_sense)?;
        for (slot, p) in total.iter_mut().zip(part) {
            *slot += p;
        }
    }
    if total.iter().any(|x| !x.is_finite()) {
        return Err(BrepError::NonFinite("a face integral".into()));
    }
    let sign = if f.same_sense { 1.0 } else { -1.0 };
    Ok(FaceIntegrals { volume: total[0], area: sign * total[1], moment: Vec3::new(total[2], total[3], total[4]) })
}

/// The signed volume enclosed by a shell: positive for an outer shell, negative for a void.
pub fn shell_volume(brep: &Brep, shell: ShellId) -> Result<f64> {
    let mut v = 0.0;
    for &f in &brep.shell(shell)?.faces {
        v += face_integrals(brep, f)?.volume;
    }
    Ok(v)
}

/// Volume, area and centroid of a solid (void shells subtract).
pub fn solid_properties(brep: &Brep, solid: SolidId) -> Result<MassProperties> {
    let (mut volume, mut area, mut moment) = (0.0, 0.0, Vec3::ZERO);
    for &s in &brep.solid(solid)?.shells {
        for &f in &brep.shell(s)?.faces {
            let fi = face_integrals(brep, f)?;
            volume += fi.volume;
            area += fi.area;
            moment += fi.moment;
        }
    }
    if volume.abs() < 1.0e-300 {
        return Err(BrepError::degenerate("the solid has no volume, so it has no centroid"));
    }
    Ok(MassProperties { volume, area, centroid: moment / volume })
}
