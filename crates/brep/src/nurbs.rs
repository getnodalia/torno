// SPDX-License-Identifier: MIT OR Apache-2.0
//! NURBS curves and surfaces: evaluation, derivatives and knot insertion.
//!
//! The algorithms are the textbook ones for B-spline basis functions and their derivatives
//! (Cox–de Boor recursion), rational evaluation by working in homogeneous coordinates `(wx, wy, wz, w)`
//! and the quotient rule for derivatives, and Boehm's single knot insertion. The structures
//! validate their invariants on construction (knot count, order, positive weights, finite
//! numbers), so evaluation inside the parameter domain cannot index out of range.

use crate::error::{BrepError, Result};
use crate::math::{Point3, Vec3};

/// Highest degree accepted.
pub const MAX_DEGREE: usize = 16;
/// Most control points accepted in one direction.
pub const MAX_CONTROL_POINTS: usize = 1 << 20;

fn validate_knots(degree: usize, n: usize, knots: &[f64], what: &str) -> Result<(f64, f64)> {
    if degree == 0 || degree > MAX_DEGREE {
        return Err(BrepError::invalid(format!("{what}: degree {degree} must be within 1..={MAX_DEGREE}")));
    }
    if n < degree + 1 || n > MAX_CONTROL_POINTS {
        return Err(BrepError::invalid(format!("{what}: {n} control points for degree {degree} (need {} to {MAX_CONTROL_POINTS})", degree + 1)));
    }
    if knots.len() != n + degree + 1 {
        return Err(BrepError::invalid(format!("{what}: {} knots, expected {} (control points + degree + 1)", knots.len(), n + degree + 1)));
    }
    if knots.iter().any(|k| !k.is_finite()) {
        return Err(BrepError::non_finite(format!("{what}: knot")));
    }
    if knots.windows(2).any(|w| w[0] > w[1]) {
        return Err(BrepError::invalid(format!("{what}: knots must be non-decreasing")));
    }
    // No knot may repeat more than degree + 1 times.
    let mut run = 1;
    for w in knots.windows(2) {
        run = if w[0] == w[1] { run + 1 } else { 1 };
        if run > degree + 1 {
            return Err(BrepError::invalid(format!("{what}: a knot is repeated more than degree + 1 times")));
        }
    }
    let (lo, hi) = (knots[degree], knots[n]);
    if !(lo < hi) {
        return Err(BrepError::degenerate(format!("{what}: the parameter domain is empty")));
    }
    Ok((lo, hi))
}

fn validate_weights(weights: &[f64], what: &str) -> Result<()> {
    if weights.iter().any(|w| !w.is_finite()) {
        return Err(BrepError::non_finite(format!("{what}: weight")));
    }
    if weights.iter().any(|&w| w <= 0.0) {
        return Err(BrepError::invalid(format!("{what}: weights must be positive")));
    }
    Ok(())
}

/// The knot span index `k` with `U[k] ≤ t < U[k+1]` (the last non-empty span for `t` at the end
/// of the domain). `n` is the number of control points.
fn find_span(n: usize, p: usize, t: f64, u: &[f64]) -> usize {
    if t >= u[n] {
        return n - 1;
    }
    if t <= u[p] {
        return p;
    }
    let (mut lo, mut hi) = (p, n);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if t < u[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// The `p + 1` non-zero basis functions at `t` in span `span`, and their derivatives up to order
/// `nd` (`ders[k][j]` is the `k`-th derivative of `N_{span-p+j,p}`).
fn basis_derivatives(span: usize, t: f64, p: usize, nd: usize, u: &[f64]) -> Vec<Vec<f64>> {
    let mut ndu = vec![vec![0.0; p + 1]; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = t - u[span + 1 - j];
        right[j] = u[span + j] - t;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = if ndu[j][r] != 0.0 { ndu[r][j - 1] / ndu[j][r] } else { 0.0 };
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let nd = nd.min(p);
    let mut ders = vec![vec![0.0; p + 1]; nd + 1];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    let mut a = vec![vec![0.0; p + 1]; 2];
    for r in 0..=p {
        let (mut s1, mut s2) = (0, 1);
        a[0][0] = 1.0;
        for k in 1..=nd {
            let mut d = 0.0;
            let rk = r as isize - k as isize;
            let pk = p as isize - k as isize;
            if r >= k {
                a[s2][0] = if ndu[pk as usize + 1][rk as usize] != 0.0 { a[s1][0] / ndu[pk as usize + 1][rk as usize] } else { 0.0 };
                d = a[s2][0] * ndu[rk as usize][pk as usize];
            }
            let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
            let j2 = if (r as isize - 1) <= pk { k - 1 } else { p - r };
            for j in j1..=j2 {
                let idx = (rk + j as isize) as usize;
                a[s2][j] = if ndu[pk as usize + 1][idx] != 0.0 { (a[s1][j] - a[s1][j - 1]) / ndu[pk as usize + 1][idx] } else { 0.0 };
                d += a[s2][j] * ndu[idx][pk as usize];
            }
            if (r as isize) <= pk {
                a[s2][k] = if ndu[pk as usize + 1][r] != 0.0 { -a[s1][k - 1] / ndu[pk as usize + 1][r] } else { 0.0 };
                d += a[s2][k] * ndu[r][pk as usize];
            }
            ders[k][r] = d;
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut fac = p as f64;
    for (k, row) in ders.iter_mut().enumerate().take(nd + 1).skip(1) {
        for v in row.iter_mut() {
            *v *= fac;
        }
        fac *= (p - k) as f64;
    }
    ders
}

fn binomial(n: usize, k: usize) -> f64 {
    let mut r = 1.0;
    for i in 0..k {
        r = r * (n - i) as f64 / (i + 1) as f64;
    }
    r
}

/// A rational B-spline curve.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve {
    degree: usize,
    knots: Vec<f64>,
    points: Vec<Point3>,
    weights: Vec<f64>,
    domain: (f64, f64),
}

impl NurbsCurve {
    /// A curve of the given degree. `knots` has `points.len() + degree + 1` non-decreasing
    /// values; `weights` has one positive value per control point (pass all ones for a
    /// non-rational curve).
    pub fn new(degree: usize, knots: Vec<f64>, points: Vec<Point3>, weights: Vec<f64>) -> Result<NurbsCurve> {
        let domain = validate_knots(degree, points.len(), &knots, "NURBS curve")?;
        if weights.len() != points.len() {
            return Err(BrepError::invalid("NURBS curve: one weight per control point"));
        }
        validate_weights(&weights, "NURBS curve")?;
        if points.iter().any(|p| !p.is_finite()) {
            return Err(BrepError::non_finite("NURBS curve: control point"));
        }
        Ok(NurbsCurve { degree, knots, points, weights, domain })
    }

    /// A clamped uniform curve through the given control points' hull (non-rational).
    pub fn clamped_uniform(degree: usize, points: Vec<Point3>) -> Result<NurbsCurve> {
        let n = points.len();
        if degree == 0 || n < degree + 1 {
            return Err(BrepError::invalid("clamped curve needs at least degree + 1 control points"));
        }
        let interior = n - degree - 1;
        let mut knots = vec![0.0; degree + 1];
        knots.extend((1..=interior).map(|i| i as f64 / (interior + 1) as f64));
        knots.extend(vec![1.0; degree + 1]);
        let weights = vec![1.0; n];
        NurbsCurve::new(degree, knots, points, weights)
    }

    pub fn degree(&self) -> usize {
        self.degree
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }
    pub fn control_points(&self) -> &[Point3] {
        &self.points
    }
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
    /// The parameter interval the curve is defined on.
    pub fn domain(&self) -> (f64, f64) {
        self.domain
    }

    fn clamp_t(&self, t: f64) -> f64 {
        if t.is_nan() { self.domain.0 } else { t.clamp(self.domain.0, self.domain.1) }
    }

    /// Derivatives `C(t), C'(t), …` up to `order` (`order + 1` vectors). `t` is clamped into the
    /// domain; derivatives of order above the degree are zero.
    pub fn derivatives(&self, t: f64, order: usize) -> Vec<Vec3> {
        let t = self.clamp_t(t);
        let n = self.points.len();
        let p = self.degree;
        let span = find_span(n, p, t, &self.knots);
        let nd = self.basis_order(order);
        let ders = basis_derivatives(span, t, p, nd, &self.knots);
        // Homogeneous derivatives A^(k) (xyz weighted) and w^(k).
        let mut a = vec![Vec3::ZERO; order + 1];
        let mut w = vec![0.0; order + 1];
        for (k, row) in ders.iter().enumerate().take(nd + 1) {
            for (j, &basis) in row.iter().enumerate().take(p + 1) {
                let idx = span - p + j;
                let wi = self.weights[idx];
                a[k] += self.points[idx] * (basis * wi);
                w[k] += basis * wi;
            }
        }
        let mut c = vec![Vec3::ZERO; order + 1];
        for k in 0..=order {
            let mut v = a[k];
            for i in 1..=k {
                v -= c[k - i] * (binomial(k, i) * w[i]);
            }
            c[k] = v / w[0];
        }
        c
    }

    fn basis_order(&self, order: usize) -> usize {
        order.min(self.degree)
    }

    pub fn point(&self, t: f64) -> Point3 {
        self.derivatives(t, 0).first().copied().unwrap_or(Vec3::ZERO)
    }

    /// Insert the knot `t` once more (`t` strictly inside the domain). The curve's shape is
    /// unchanged (Boehm's algorithm on the homogeneous control points).
    pub fn insert_knot(&self, t: f64) -> Result<NurbsCurve> {
        if !t.is_finite() || !(t > self.domain.0 && t < self.domain.1) {
            return Err(BrepError::OutOfRange(format!("knot {t} must lie strictly inside the domain ({}, {})", self.domain.0, self.domain.1)));
        }
        let (n, p) = (self.points.len(), self.degree);
        let k = find_span(n, p, t, &self.knots);
        let s = self.knots.iter().filter(|&&x| x == t).count();
        if s >= p {
            return Err(BrepError::invalid(format!("knot {t} already has multiplicity {s}; degree {p} allows at most {}", p - 1)));
        }
        let mut knots = self.knots[..=k].to_vec();
        knots.push(t);
        knots.extend_from_slice(&self.knots[k + 1..]);
        let hom = |i: usize| (self.points[i] * self.weights[i], self.weights[i]);
        let mut pts = Vec::with_capacity(n + 1);
        let mut ws = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let (q, qw) = if i + p <= k {
                hom(i)
            } else if i > k - s {
                hom(i - 1)
            } else {
                let a = (t - self.knots[i]) / (self.knots[i + p] - self.knots[i]);
                let (p1, w1) = hom(i);
                let (p0, w0) = hom(i - 1);
                (p1 * a + p0 * (1.0 - a), w1 * a + w0 * (1.0 - a))
            };
            pts.push(q / qw);
            ws.push(qw);
        }
        NurbsCurve::new(p, knots, pts, ws)
    }
}

/// A rational tensor-product B-spline surface.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurface {
    degree_u: usize,
    degree_v: usize,
    knots_u: Vec<f64>,
    knots_v: Vec<f64>,
    nu: usize,
    nv: usize,
    /// Row-major: point `(i, j)` is at `i * nv + j`.
    points: Vec<Point3>,
    weights: Vec<f64>,
    domain_u: (f64, f64),
    domain_v: (f64, f64),
}

impl NurbsSurface {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        degree_u: usize,
        degree_v: usize,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
        nu: usize,
        nv: usize,
        points: Vec<Point3>,
        weights: Vec<f64>,
    ) -> Result<NurbsSurface> {
        let domain_u = validate_knots(degree_u, nu, &knots_u, "NURBS surface (u)")?;
        let domain_v = validate_knots(degree_v, nv, &knots_v, "NURBS surface (v)")?;
        let count = nu.checked_mul(nv).ok_or_else(|| BrepError::invalid("NURBS surface: too many control points"))?;
        if points.len() != count || weights.len() != count {
            return Err(BrepError::invalid(format!("NURBS surface: {nu}×{nv} control points need {count} points and weights")));
        }
        validate_weights(&weights, "NURBS surface")?;
        if points.iter().any(|p| !p.is_finite()) {
            return Err(BrepError::non_finite("NURBS surface: control point"));
        }
        Ok(NurbsSurface { degree_u, degree_v, knots_u, knots_v, nu, nv, points, weights, domain_u, domain_v })
    }

    pub fn domain_u(&self) -> (f64, f64) {
        self.domain_u
    }
    pub fn domain_v(&self) -> (f64, f64) {
        self.domain_v
    }
    pub fn degrees(&self) -> (usize, usize) {
        (self.degree_u, self.degree_v)
    }
    pub fn control_counts(&self) -> (usize, usize) {
        (self.nu, self.nv)
    }

    /// Partial derivatives `S^{(k,l)}` for `k + l ≤ order`, as `result[k][l]` (entries with
    /// `k + l > order` are zero). `(u, v)` is clamped into the domain.
    pub fn derivatives(&self, u: f64, v: f64, order: usize) -> Vec<Vec<Vec3>> {
        let clamp = |x: f64, d: (f64, f64)| if x.is_nan() { d.0 } else { x.clamp(d.0, d.1) };
        let (u, v) = (clamp(u, self.domain_u), clamp(v, self.domain_v));
        let (pu, pv) = (self.degree_u, self.degree_v);
        let su = find_span(self.nu, pu, u, &self.knots_u);
        let sv = find_span(self.nv, pv, v, &self.knots_v);
        let du = basis_derivatives(su, u, pu, order.min(pu), &self.knots_u);
        let dv = basis_derivatives(sv, v, pv, order.min(pv), &self.knots_v);
        let mut a = vec![vec![Vec3::ZERO; order + 1]; order + 1];
        let mut w = vec![vec![0.0; order + 1]; order + 1];
        for k in 0..=order.min(pu) {
            for l in 0..=(order - k).min(pv) {
                for (i, nk) in du[k].iter().enumerate() {
                    for (j, ml) in dv[l].iter().enumerate() {
                        let idx = (su - pu + i) * self.nv + (sv - pv + j);
                        let wi = self.weights[idx];
                        let f = nk * ml * wi;
                        a[k][l] += self.points[idx] * f;
                        w[k][l] += f;
                    }
                }
            }
        }
        let mut s = vec![vec![Vec3::ZERO; order + 1]; order + 1];
        for k in 0..=order {
            for l in 0..=(order - k) {
                let mut val = a[k][l];
                for j in 1..=l {
                    val -= s[k][l - j] * (binomial(l, j) * w[0][j]);
                }
                for i in 1..=k {
                    val -= s[k - i][l] * (binomial(k, i) * w[i][0]);
                    let mut inner = Vec3::ZERO;
                    for j in 1..=l {
                        inner += s[k - i][l - j] * (binomial(l, j) * w[i][j]);
                    }
                    val -= inner * binomial(k, i);
                }
                s[k][l] = val / w[0][0];
            }
        }
        s
    }

    pub fn point(&self, u: f64, v: f64) -> Point3 {
        self.derivatives(u, v, 0).first().and_then(|r| r.first()).copied().unwrap_or(Vec3::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    /// A quarter of the unit circle as a rational quadratic Bézier: exactly representable.
    fn quarter_circle() -> NurbsCurve {
        NurbsCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)],
            vec![1.0, FRAC_1_SQRT_2, 1.0],
        )
        .unwrap()
    }

    #[test]
    fn basis_functions_partition_unity_and_derivatives_sum_to_zero() {
        let u = [0.0, 0.0, 0.0, 0.0, 0.3, 0.7, 1.0, 1.0, 1.0, 1.0];
        for i in 0..=50 {
            let t = i as f64 / 50.0;
            let span = find_span(6, 3, t, &u);
            let d = basis_derivatives(span, t, 3, 3, &u);
            assert!((d[0].iter().sum::<f64>() - 1.0).abs() < 1e-14);
            for (k, row) in d.iter().enumerate().skip(1) {
                assert!(row.iter().sum::<f64>().abs() < 1e-11, "k={k} t={t}");
            }
        }
    }

    #[test]
    fn rational_quarter_circle_is_exactly_on_the_circle() {
        let c = quarter_circle();
        for i in 0..=100 {
            let p = c.point(i as f64 / 100.0);
            assert!((p.norm() - 1.0).abs() < 1e-14, "t={i}: {}", p.norm());
        }
        assert!(c.point(0.0).distance(Vec3::X) < 1e-15);
        assert!(c.point(1.0).distance(Vec3::Y) < 1e-15);
    }

    #[test]
    fn derivatives_match_finite_differences() {
        let c = quarter_circle();
        for &t in &[0.1, 0.4, 0.8] {
            let d = c.derivatives(t, 2);
            let h = 1e-5;
            let fd1 = (c.point(t + h) - c.point(t - h)) / (2.0 * h);
            let fd2 = (c.point(t + h) - c.point(t) * 2.0 + c.point(t - h)) / (h * h);
            assert!(d[1].distance(fd1) < 1e-8, "t={t}");
            assert!(d[2].distance(fd2) < 1e-4, "t={t}");
            // The curve is on the unit circle: C·C' = 0.
            assert!(d[0].dot(d[1]).abs() < 1e-13);
        }
    }

    #[test]
    fn knot_insertion_preserves_the_curve() {
        let c = NurbsCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
            vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 0.0), Vec3::new(2.0, -1.0, 1.0), Vec3::new(3.0, 1.0, 2.0), Vec3::new(4.0, 0.0, 0.0)],
            vec![1.0, 2.0, 0.5, 1.5, 1.0],
        )
        .unwrap();
        let d = c.insert_knot(0.25).unwrap().insert_knot(0.25).unwrap().insert_knot(0.75).unwrap();
        assert_eq!(d.control_points().len(), 8);
        for i in 0..=40 {
            let t = i as f64 / 40.0;
            assert!(c.point(t).distance(d.point(t)) < 1e-12, "t={t}");
        }
        // Inserting a third copy of a double knot of a cubic is allowed once (multiplicity 3 = degree is refused).
        let e = c.insert_knot(0.5).unwrap();
        assert!(e.insert_knot(0.5).is_ok());
        assert!(e.insert_knot(0.5).unwrap().insert_knot(0.5).is_err());
    }

    #[test]
    fn constructor_rejects_bad_input() {
        let pts = vec![Vec3::ZERO, Vec3::X, Vec3::Y];
        let ok_knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        assert!(NurbsCurve::new(2, ok_knots.clone(), pts.clone(), vec![1.0; 3]).is_ok());
        assert!(NurbsCurve::new(0, ok_knots.clone(), pts.clone(), vec![1.0; 3]).is_err());
        assert!(NurbsCurve::new(2, vec![0.0, 0.0, 1.0, 1.0], pts.clone(), vec![1.0; 3]).is_err());
        assert!(NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 0.5, 1.0], pts.clone(), vec![1.0; 3]).is_err());
        assert!(NurbsCurve::new(2, vec![0.0, 0.0, 0.0, f64::NAN, 1.0, 1.0], pts.clone(), vec![1.0; 3]).is_err());
        assert!(NurbsCurve::new(2, ok_knots.clone(), pts.clone(), vec![1.0, 0.0, 1.0]).is_err());
        assert!(NurbsCurve::new(2, ok_knots.clone(), pts.clone(), vec![1.0, 1.0]).is_err());
        assert!(NurbsCurve::new(2, vec![1.0; 6], pts.clone(), vec![1.0; 3]).is_err());
        assert!(NurbsCurve::new(2, ok_knots, vec![Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X, Vec3::Y], vec![1.0; 3]).is_err());
    }

    #[test]
    fn evaluation_outside_the_domain_or_at_nan_is_clamped_not_a_panic() {
        let c = quarter_circle();
        assert!(c.point(-5.0).distance(Vec3::X) < 1e-15);
        assert!(c.point(5.0).distance(Vec3::Y) < 1e-15);
        assert!(c.point(f64::NAN).distance(Vec3::X) < 1e-15);
        assert_eq!(c.derivatives(0.5, 7).len(), 8);
    }

    /// The rational bi-quadratic patch of one octant of the unit sphere would need more setup;
    /// a bilinear patch (a hyperbolic paraboloid) has closed-form derivatives to compare with.
    #[test]
    fn bilinear_patch_matches_its_closed_form() {
        let s = NurbsSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            2,
            2,
            vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0), Vec3::new(1.0, 0.0, 1.0), Vec3::new(1.0, 1.0, 0.0)],
            vec![1.0; 4],
        )
        .unwrap();
        for &(u, v) in &[(0.2, 0.3), (0.5, 0.5), (0.9, 0.1)] {
            let d = s.derivatives(u, v, 2);
            assert!(d[0][0].distance(Vec3::new(u, v, u + v - 2.0 * u * v)) < 1e-14);
            assert!(d[1][0].distance(Vec3::new(1.0, 0.0, 1.0 - 2.0 * v)) < 1e-14);
            assert!(d[0][1].distance(Vec3::new(0.0, 1.0, 1.0 - 2.0 * u)) < 1e-14);
            assert!(d[1][1].distance(Vec3::new(0.0, 0.0, -2.0)) < 1e-14);
            assert!(d[2][0].norm() < 1e-14);
        }
    }

    #[test]
    fn rational_surface_cylinder_patch_is_on_the_cylinder() {
        // A quarter cylinder of radius 1: quadratic rational arc in u, linear in v.
        let w = FRAC_1_SQRT_2;
        let arc = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let mut pts = Vec::new();
        let mut ws = Vec::new();
        for (i, a) in arc.iter().enumerate() {
            for z in [0.0, 2.0] {
                pts.push(Vec3::new(a.x, a.y, z));
                ws.push(if i == 1 { w } else { 1.0 });
            }
        }
        let s = NurbsSurface::new(2, 1, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0], 3, 2, pts, ws).unwrap();
        for i in 0..=10 {
            for j in 0..=10 {
                let (u, v) = (i as f64 / 10.0, j as f64 / 10.0);
                let p = s.point(u, v);
                assert!((p.x.hypot(p.y) - 1.0).abs() < 1e-14);
                assert!((p.z - 2.0 * v).abs() < 1e-14);
                let d = s.derivatives(u, v, 1);
                // Su is tangent to the circle; Sv is the axis direction.
                assert!(d[1][0].dot(Vec3::new(p.x, p.y, 0.0)).abs() < 1e-12);
                assert!(d[0][1].distance(Vec3::new(0.0, 0.0, 2.0)) < 1e-13);
            }
        }
    }

    #[test]
    fn surface_constructor_rejects_wrong_counts() {
        let k = vec![0.0, 0.0, 1.0, 1.0];
        assert!(NurbsSurface::new(1, 1, k.clone(), k.clone(), 2, 2, vec![Vec3::ZERO; 3], vec![1.0; 4]).is_err());
        assert!(NurbsSurface::new(1, 1, k.clone(), k.clone(), 2, 2, vec![Vec3::ZERO; 4], vec![1.0, 1.0, -1.0, 1.0]).is_err());
        assert!(NurbsSurface::new(1, 1, k.clone(), k, 2, 2, vec![Vec3::ZERO; 4], vec![1.0; 4]).is_ok());
    }
}
