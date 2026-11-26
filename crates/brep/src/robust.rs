// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exact geometric predicates: [`orient2d`], [`orient3d`], [`incircle`].
//!
//! Each predicate returns the **exact sign** of a determinant of the input coordinates, as if it
//! were evaluated in infinite precision. Two stages:
//!
//! 1. a floating-point *filter*: the determinant in `f64` plus a rigorous bound on its rounding
//!    error (the constants of Shewchuk, "Adaptive Precision Floating-Point Arithmetic and Fast
//!    Robust Geometric Predicates", 1997); if the result is larger than the bound its sign is
//!    certain and it is returned (this answers almost every call);
//! 2. otherwise an *exact* evaluation with floating-point **expansions**: sums of `f64` components
//!    that are non-overlapping, so the sum is represented without rounding. The error-free
//!    transformations (`two_sum`, `two_product` through a fused multiply-add) and the sum and
//!    scale operations are written here from the paper's descriptions of them.
//!
//! It is a *filtered exact* scheme, not Shewchuk's fully staged adaptive one (which has
//! intermediate precision levels): the exact stage always evaluates from scratch. That is slower
//! for the rare near-degenerate input and exactly as correct.
//!
//! # Domain
//!
//! Inputs must be finite. Magnitudes above [`MAX_ABS`] (`1e30`) are refused with an error; a
//! non-zero magnitude below [`FLUSH_BELOW`] (`1e-30`) is treated as zero. With that, no
//! intermediate product of the exact stage (degree at most four in the differences) can overflow
//! or underflow, which is what the error-free transformations need to stay exact. The
//! perturbation of the flush is below `1e-30 mm`, many orders under any tolerance.
//!
//! # Sign conventions
//!
//! * [`orient2d`]: `Positive` when `a, b, c` turn counter-clockwise.
//! * [`orient3d`]: `Positive` when `d` is on the side of plane `abc` that the normal
//!   `(b − a) × (c − a)` points to.
//! * [`incircle`]: `Positive` when `d` is strictly inside the circle through `a, b, c`, which must
//!   be counter-clockwise.

use crate::error::{BrepError, Result};

/// The sign of an exactly evaluated quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Negative,
    Zero,
    Positive,
}

impl Sign {
    fn of(x: f64) -> Sign {
        if x > 0.0 {
            Sign::Positive
        } else if x < 0.0 {
            Sign::Negative
        } else {
            Sign::Zero
        }
    }
    /// −1, 0 or 1.
    pub fn as_i32(self) -> i32 {
        match self {
            Sign::Negative => -1,
            Sign::Zero => 0,
            Sign::Positive => 1,
        }
    }
    pub fn flipped(self) -> Sign {
        match self {
            Sign::Negative => Sign::Positive,
            Sign::Zero => Sign::Zero,
            Sign::Positive => Sign::Negative,
        }
    }
}

/// Largest coordinate magnitude the predicates accept.
pub const MAX_ABS: f64 = 1.0e30;
/// Non-zero magnitudes below this are treated as zero.
pub const FLUSH_BELOW: f64 = 1.0e-30;

// ε = 2⁻⁵³, half the unit roundoff, as in the paper.
const EPS: f64 = 1.110_223_024_625_156_5e-16;
const CCW_ERRBOUND_A: f64 = (3.0 + 16.0 * EPS) * EPS;
const O3D_ERRBOUND_A: f64 = (7.0 + 56.0 * EPS) * EPS;
const ICC_ERRBOUND_A: f64 = (10.0 + 96.0 * EPS) * EPS;

// ----- error-free transformations ---------------------------------------------------------

/// `a + b = x + y` exactly, with `x = fl(a + b)`. (Knuth's branch-free two-sum.)
#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    let bv = x - a;
    let av = x - bv;
    (x, (a - av) + (b - bv))
}

/// `a − b = x + y` exactly, with `x = fl(a − b)`.
#[inline]
fn two_diff(a: f64, b: f64) -> (f64, f64) {
    let x = a - b;
    let bv = a - x;
    let av = x + bv;
    (x, (a - av) + (bv - b))
}

/// `a · b = x + y` exactly, with `x = fl(a · b)`; the error comes from a fused multiply-add.
#[inline]
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let x = a * b;
    (x, a.mul_add(b, -x))
}

// ----- expansions -------------------------------------------------------------------------

/// A non-overlapping floating-point expansion, components in increasing magnitude, zeros removed.
/// The value is the exact sum of the components; the sign is the sign of the last one.
#[derive(Clone, Debug, Default)]
struct Exp(Vec<f64>);

impl Exp {
    fn diff(a: f64, b: f64) -> Exp {
        let (x, y) = two_diff(a, b);
        Exp::from_pair(y, x)
    }
    fn prod(a: f64, b: f64) -> Exp {
        let (x, y) = two_product(a, b);
        Exp::from_pair(y, x)
    }
    fn from_pair(lo: f64, hi: f64) -> Exp {
        let mut v = Vec::with_capacity(2);
        if lo != 0.0 {
            v.push(lo);
        }
        if hi != 0.0 {
            v.push(hi);
        }
        Exp(v)
    }
    /// Grow by one `f64`: Shewchuk's GROW-EXPANSION.
    fn grow(e: &[f64], b: f64) -> Vec<f64> {
        let mut out = Vec::with_capacity(e.len() + 1);
        let mut q = b;
        for &ei in e {
            let (s, h) = two_sum(q, ei);
            q = s;
            if h != 0.0 {
                out.push(h);
            }
        }
        if q != 0.0 {
            out.push(q);
        }
        out
    }
    fn add(&self, o: &Exp) -> Exp {
        let mut cur = self.0.clone();
        for &f in &o.0 {
            cur = Exp::grow(&cur, f);
        }
        Exp(cur)
    }
    fn neg(&self) -> Exp {
        Exp(self.0.iter().map(|x| -x).collect())
    }
    fn sub(&self, o: &Exp) -> Exp {
        self.add(&o.neg())
    }
    /// Multiply by one `f64`: Shewchuk's SCALE-EXPANSION.
    fn scale(&self, b: f64) -> Exp {
        let mut it = self.0.iter().copied();
        let Some(first) = it.next() else { return Exp::default() };
        if b == 0.0 {
            return Exp::default();
        }
        let mut out = Vec::with_capacity(2 * self.0.len());
        let (mut q, h) = two_product(first, b);
        if h != 0.0 {
            out.push(h);
        }
        for ei in it {
            let (t, tt) = two_product(ei, b);
            let (q2, h1) = two_sum(q, tt);
            if h1 != 0.0 {
                out.push(h1);
            }
            let (q3, h2) = two_sum(t, q2);
            q = q3;
            if h2 != 0.0 {
                out.push(h2);
            }
        }
        if q != 0.0 {
            out.push(q);
        }
        Exp(out)
    }
    fn mul(&self, o: &Exp) -> Exp {
        let mut acc = Exp::default();
        for &f in &o.0 {
            acc = acc.add(&self.scale(f));
        }
        acc
    }
    fn sign(&self) -> Sign {
        self.0.last().map_or(Sign::Zero, |&x| Sign::of(x))
    }
    /// The value rounded to one `f64`-ish number (components summed smallest first); its sign is exact.
    fn approx(&self) -> f64 {
        self.0.iter().sum()
    }
}

// ----- domain -----------------------------------------------------------------------------

fn domain(x: f64) -> Result<f64> {
    if !x.is_finite() {
        return Err(BrepError::non_finite("coordinate given to a predicate"));
    }
    let m = x.abs();
    if m > MAX_ABS {
        return Err(BrepError::OutOfRange(format!("coordinate {x:e} exceeds the predicate range of ±{MAX_ABS:e}")));
    }
    Ok(if m < FLUSH_BELOW { 0.0 } else { x })
}

fn domain2(p: [f64; 2]) -> Result<[f64; 2]> {
    Ok([domain(p[0])?, domain(p[1])?])
}
fn domain3(p: [f64; 3]) -> Result<[f64; 3]> {
    Ok([domain(p[0])?, domain(p[1])?, domain(p[2])?])
}

// ----- orient2d ---------------------------------------------------------------------------

/// A number whose sign is the exact sign of the orientation determinant of `a, b, c`.
/// Inputs must already be inside the predicate domain (finite, `|x| ≤ 1e30`, no tiny non-zeros);
/// callers that cannot promise that use [`orient2d`].
pub(crate) fn orient2d_value(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    let detleft = (a[0] - c[0]) * (b[1] - c[1]);
    let detright = (a[1] - c[1]) * (b[0] - c[0]);
    let det = detleft - detright;
    let detsum = if detleft > 0.0 {
        if detright <= 0.0 {
            return det;
        }
        detleft + detright
    } else if detleft < 0.0 {
        if detright >= 0.0 {
            return det;
        }
        -detleft - detright
    } else {
        return det;
    };
    let bound = CCW_ERRBOUND_A * detsum;
    if det >= bound || -det >= bound {
        return det;
    }
    orient2d_exact(a, b, c).approx()
}

fn orient2d_exact(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Exp {
    let acx = Exp::diff(a[0], c[0]);
    let acy = Exp::diff(a[1], c[1]);
    let bcx = Exp::diff(b[0], c[0]);
    let bcy = Exp::diff(b[1], c[1]);
    acx.mul(&bcy).sub(&acy.mul(&bcx))
}

/// The orientation of the triangle `a, b, c`: `Positive` for counter-clockwise, `Negative` for
/// clockwise, `Zero` for exactly collinear. Exact (see the [module documentation](self)).
pub fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Result<Sign> {
    Ok(Sign::of(orient2d_value(domain2(a)?, domain2(b)?, domain2(c)?)))
}

// ----- orient3d ---------------------------------------------------------------------------

/// Shewchuk's determinant of the rows `a−d, b−d, c−d`; its sign is the *opposite* of [`orient3d`]'s.
fn orient3d_det(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> f64 {
    let (adx, ady, adz) = (a[0] - d[0], a[1] - d[1], a[2] - d[2]);
    let (bdx, bdy, bdz) = (b[0] - d[0], b[1] - d[1], b[2] - d[2]);
    let (cdx, cdy, cdz) = (c[0] - d[0], c[1] - d[1], c[2] - d[2]);
    let bdxcdy = bdx * cdy;
    let cdxbdy = cdx * bdy;
    let cdxady = cdx * ady;
    let adxcdy = adx * cdy;
    let adxbdy = adx * bdy;
    let bdxady = bdx * ady;
    let det = adz * (bdxcdy - cdxbdy) + bdz * (cdxady - adxcdy) + cdz * (adxbdy - bdxady);
    let permanent =
        ((bdxcdy.abs() + cdxbdy.abs()) * adz.abs()) + ((cdxady.abs() + adxcdy.abs()) * bdz.abs()) + ((adxbdy.abs() + bdxady.abs()) * cdz.abs());
    let bound = O3D_ERRBOUND_A * permanent;
    if det > bound || -det > bound {
        return det;
    }
    orient3d_exact(a, b, c, d).approx()
}

fn orient3d_exact(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> Exp {
    let ad = [Exp::diff(a[0], d[0]), Exp::diff(a[1], d[1]), Exp::diff(a[2], d[2])];
    let bd = [Exp::diff(b[0], d[0]), Exp::diff(b[1], d[1]), Exp::diff(b[2], d[2])];
    let cd = [Exp::diff(c[0], d[0]), Exp::diff(c[1], d[1]), Exp::diff(c[2], d[2])];
    let cross_z = |p: &[Exp; 3], q: &[Exp; 3]| p[0].mul(&q[1]).sub(&q[0].mul(&p[1]));
    let cross_x = |p: &[Exp; 3], q: &[Exp; 3]| p[1].mul(&q[2]).sub(&q[1].mul(&p[2]));
    let cross_y = |p: &[Exp; 3], q: &[Exp; 3]| p[2].mul(&q[0]).sub(&q[2].mul(&p[0]));
    // ad · (bd × cd)
    ad[0].mul(&cross_x(&bd, &cd)).add(&ad[1].mul(&cross_y(&bd, &cd))).add(&ad[2].mul(&cross_z(&bd, &cd)))
}

/// Which side of the plane through `a, b, c` the point `d` is on: `Positive` on the side the
/// normal `(b − a) × (c − a)` points to, `Zero` when exactly coplanar. Exact.
pub fn orient3d(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> Result<Sign> {
    let v = orient3d_det(domain3(a)?, domain3(b)?, domain3(c)?, domain3(d)?);
    Ok(Sign::of(v).flipped())
}

// ----- incircle ---------------------------------------------------------------------------

/// A number whose sign is the exact sign of the in-circle determinant (domain as in [`orient2d_value`]).
pub(crate) fn incircle_value(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    let (adx, ady) = (a[0] - d[0], a[1] - d[1]);
    let (bdx, bdy) = (b[0] - d[0], b[1] - d[1]);
    let (cdx, cdy) = (c[0] - d[0], c[1] - d[1]);
    let bdxcdy = bdx * cdy;
    let cdxbdy = cdx * bdy;
    let alift = adx * adx + ady * ady;
    let cdxady = cdx * ady;
    let adxcdy = adx * cdy;
    let blift = bdx * bdx + bdy * bdy;
    let adxbdy = adx * bdy;
    let bdxady = bdx * ady;
    let clift = cdx * cdx + cdy * cdy;
    let det = alift * (bdxcdy - cdxbdy) + blift * (cdxady - adxcdy) + clift * (adxbdy - bdxady);
    let permanent = (bdxcdy.abs() + cdxbdy.abs()) * alift + (cdxady.abs() + adxcdy.abs()) * blift + (adxbdy.abs() + bdxady.abs()) * clift;
    let bound = ICC_ERRBOUND_A * permanent;
    if det > bound || -det > bound {
        return det;
    }
    incircle_exact(a, b, c, d).approx()
}

fn incircle_exact(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Exp {
    let (adx, ady) = (Exp::diff(a[0], d[0]), Exp::diff(a[1], d[1]));
    let (bdx, bdy) = (Exp::diff(b[0], d[0]), Exp::diff(b[1], d[1]));
    let (cdx, cdy) = (Exp::diff(c[0], d[0]), Exp::diff(c[1], d[1]));
    let lift = |x: &Exp, y: &Exp| x.mul(x).add(&y.mul(y));
    let (alift, blift, clift) = (lift(&adx, &ady), lift(&bdx, &bdy), lift(&cdx, &cdy));
    let t_a = bdx.mul(&cdy).sub(&cdx.mul(&bdy));
    let t_b = cdx.mul(&ady).sub(&adx.mul(&cdy));
    let t_c = adx.mul(&bdy).sub(&bdx.mul(&ady));
    alift.mul(&t_a).add(&blift.mul(&t_b)).add(&clift.mul(&t_c))
}

/// Whether `d` is inside the circle through the counter-clockwise triangle `a, b, c`:
/// `Positive` strictly inside, `Negative` strictly outside, `Zero` exactly on it. Exact.
pub fn incircle(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Result<Sign> {
    Ok(Sign::of(incircle_value(domain2(a)?, domain2(b)?, domain2(c)?, domain2(d)?)))
}

// ----- derived helpers --------------------------------------------------------------------

/// Whether the open segments `ab` and `cd` cross at a single point interior to both (a proper
/// crossing). Touching at an endpoint, overlapping collinear segments and disjoint segments are
/// `false`. Exact.
pub fn segments_cross_properly(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Result<bool> {
    let o1 = orient2d(a, b, c)?;
    let o2 = orient2d(a, b, d)?;
    let o3 = orient2d(c, d, a)?;
    let o4 = orient2d(c, d, b)?;
    let strict = |x: Sign, y: Sign| x != Sign::Zero && y != Sign::Zero && x != y;
    Ok(strict(o1, o2) && strict(o3, o4))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift64*: deterministic, no dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn int(&mut self, bound: i64) -> i64 {
            (self.next() % (2 * bound as u64 + 1)) as i64 - bound
        }
    }

    fn sgn(x: i128) -> Sign {
        match x.signum() {
            1 => Sign::Positive,
            -1 => Sign::Negative,
            _ => Sign::Zero,
        }
    }

    // Exact references in i128 on integer coordinates (then scaled by a power of two into f64).
    fn orient2d_ref(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> Sign {
        let (acx, acy) = ((a[0] - c[0]) as i128, (a[1] - c[1]) as i128);
        let (bcx, bcy) = ((b[0] - c[0]) as i128, (b[1] - c[1]) as i128);
        sgn(acx * bcy - acy * bcx)
    }
    fn orient3d_ref(a: [i64; 3], b: [i64; 3], c: [i64; 3], d: [i64; 3]) -> Sign {
        let u = [(b[0] - a[0]) as i128, (b[1] - a[1]) as i128, (b[2] - a[2]) as i128];
        let v = [(c[0] - a[0]) as i128, (c[1] - a[1]) as i128, (c[2] - a[2]) as i128];
        let w = [(d[0] - a[0]) as i128, (d[1] - a[1]) as i128, (d[2] - a[2]) as i128];
        let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        sgn(cross[0] * w[0] + cross[1] * w[1] + cross[2] * w[2])
    }
    fn incircle_ref(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> Sign {
        let f = |p: [i64; 2]| ((p[0] - d[0]) as i128, (p[1] - d[1]) as i128);
        let ((adx, ady), (bdx, bdy), (cdx, cdy)) = (f(a), f(b), f(c));
        let (al, bl, cl) = (adx * adx + ady * ady, bdx * bdx + bdy * bdy, cdx * cdx + cdy * cdy);
        sgn(al * (bdx * cdy - cdx * bdy) + bl * (cdx * ady - adx * cdy) + cl * (adx * bdy - bdx * ady))
    }

    const SCALE: f64 = 1.0 / (1u64 << 30) as f64;
    fn f2(p: [i64; 2]) -> [f64; 2] {
        [p[0] as f64 * SCALE, p[1] as f64 * SCALE]
    }
    fn f3(p: [i64; 3]) -> [f64; 3] {
        [p[0] as f64 * SCALE, p[1] as f64 * SCALE, p[2] as f64 * SCALE]
    }

    #[test]
    fn expansion_arithmetic_is_exact() {
        // (1 + 2⁻⁶⁰) − 1 is invisible to f64 but not to an expansion.
        let tiny = 2f64.powi(-60);
        let e = Exp::diff(1.0, 0.0).add(&Exp::diff(tiny, 0.0)).sub(&Exp::diff(1.0, 0.0));
        assert_eq!(e.0, vec![tiny]);
        // (2²⁷+1)² = 2⁵⁴ + 2²⁸ + 1: 55 bits, one more than f64 holds.
        let x = (1u64 << 27) as f64 + 1.0;
        let sq = Exp::prod(x, x);
        let int_sq = ((1u128 << 27) + 1).pow(2);
        let sum: u128 = sq.0.iter().map(|&c| c as u128).sum();
        assert_eq!(sum, int_sq);
        // Scaling then un-scaling by a power of two is the identity.
        let e = Exp::prod(0.1, 0.3).add(&Exp::diff(1e-20, 0.0));
        let back = e.scale(8.0).scale(0.125);
        assert_eq!(back.0, e.0);
        assert_eq!(Exp::default().sign(), Sign::Zero);
        assert_eq!(Exp::diff(1.0, 1.0).sign(), Sign::Zero);
    }

    #[test]
    fn orient2d_basic_signs() {
        assert_eq!(orient2d([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]).unwrap(), Sign::Positive);
        assert_eq!(orient2d([0.0, 0.0], [0.0, 1.0], [1.0, 0.0]).unwrap(), Sign::Negative);
        assert_eq!(orient2d([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]).unwrap(), Sign::Zero);
    }

    #[test]
    fn orient3d_basic_signs() {
        let (a, b, c) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert_eq!(orient3d(a, b, c, [0.0, 0.0, 1.0]).unwrap(), Sign::Positive);
        assert_eq!(orient3d(a, b, c, [0.0, 0.0, -1.0]).unwrap(), Sign::Negative);
        assert_eq!(orient3d(a, b, c, [5.0, -3.0, 0.0]).unwrap(), Sign::Zero);
    }

    #[test]
    fn incircle_basic_signs() {
        let (a, b, c) = ([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]);
        assert_eq!(incircle(a, b, c, [0.0, 0.0]).unwrap(), Sign::Positive);
        assert_eq!(incircle(a, b, c, [2.0, 0.0]).unwrap(), Sign::Negative);
        assert_eq!(incircle(a, b, c, [0.0, -1.0]).unwrap(), Sign::Zero);
    }

    #[test]
    fn domain_errors() {
        assert!(orient2d([f64::NAN, 0.0], [0.0, 0.0], [1.0, 1.0]).is_err());
        assert!(orient2d([f64::INFINITY, 0.0], [0.0, 0.0], [1.0, 1.0]).is_err());
        assert!(orient3d([1e31, 0.0, 0.0], [0.0; 3], [1.0; 3], [2.0; 3]).is_err());
        assert!(incircle([0.0; 2], [1.0, 0.0], [0.0, 1.0], [f64::NAN, 0.0]).is_err());
        // Tiny non-zeros are flushed to zero rather than refused.
        assert_eq!(orient2d([1e-40, 0.0], [1.0, 0.0], [0.0, 1.0]).unwrap(), Sign::Positive);
    }

    #[test]
    fn orient2d_matches_the_integer_reference_on_random_and_near_collinear_input() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut naive_wrong = 0;
        let mut zeros = 0;
        for i in 0..40_000 {
            let range = 1i64 << 51;
            let a = [rng.int(range), rng.int(range)];
            let (b, c) = if i % 2 == 0 {
                ([rng.int(range), rng.int(range)], [rng.int(range), rng.int(range)])
            } else {
                // b and c on one lattice line through a (exactly collinear), c nudged by -1, 0 or 1.
                let d = [rng.int(1 << 25), rng.int(1 << 25)];
                let (m, n) = (rng.int(1 << 24), rng.int(1 << 24));
                ([a[0] + m * d[0], a[1] + m * d[1]], [a[0] + n * d[0] + rng.int(1), a[1] + n * d[1] + rng.int(1)])
            };
            let want = orient2d_ref(a, b, c);
            zeros += (want == Sign::Zero) as usize;
            assert_eq!(orient2d(f2(a), f2(b), f2(c)).unwrap(), want, "a={a:?} b={b:?} c={c:?}");
            let (pa, pb, pc) = (f2(a), f2(b), f2(c));
            let naive = (pa[0] - pc[0]) * (pb[1] - pc[1]) - (pa[1] - pc[1]) * (pb[0] - pc[0]);
            naive_wrong += (Sign::of(naive) != want) as usize;
        }
        assert!(zeros > 100, "the test must hit exactly collinear input ({zeros})");
        assert!(naive_wrong > 0, "the near-degenerate cases must defeat naive floating point, or the test proves nothing");
    }

    #[test]
    fn orient2d_classic_perturbation_grid_is_consistent() {
        // Points 0.5 + i·ε on the diagonal: the standard demonstration of failure in naive code.
        let eps = 2f64.powi(-53);
        let (a, b) = ([0.5, 0.5], [12.0, 12.0]);
        let mut prev_row: Vec<Sign> = Vec::new();
        for j in 0..32 {
            let mut row = Vec::new();
            for i in 0..32 {
                let c = [24.0 + i as f64 * eps * 16.0, 24.0 + j as f64 * eps * 16.0];
                row.push(orient2d(a, b, c).unwrap());
            }
            // Exactly: orientation is the sign of (c.x − c.y)·(11.5)... monotone in i for fixed j.
            for w in row.windows(2) {
                assert!(w[0].as_i32() >= w[1].as_i32() || w[0] == w[1], "not monotone: {row:?}");
            }
            if !prev_row.is_empty() {
                for (p, r) in prev_row.iter().zip(&row) {
                    assert!(p.as_i32() <= r.as_i32() || p == r);
                }
            }
            prev_row = row;
        }
    }

    #[test]
    fn orient2d_is_antisymmetric_and_cyclic() {
        let mut rng = Rng(42);
        for _ in 0..5000 {
            let range = 1i64 << 35;
            let p: Vec<[f64; 2]> = (0..3).map(|_| f2([rng.int(range), rng.int(range)])).collect();
            let s = orient2d(p[0], p[1], p[2]).unwrap();
            assert_eq!(orient2d(p[1], p[2], p[0]).unwrap(), s);
            assert_eq!(orient2d(p[1], p[0], p[2]).unwrap(), s.flipped());
        }
    }

    #[test]
    fn orient3d_matches_the_integer_reference_including_coplanar_input() {
        let mut rng = Rng(7);
        let mut zeros = 0;
        for i in 0..20_000 {
            let range = 1i64 << 28;
            let mut p = || [rng.int(range), rng.int(range), rng.int(range)];
            let (a, b, c) = (p(), p(), p());
            let d = if i % 2 == 0 {
                p()
            } else {
                // d = a + 3(b − a)/1 ... on the plane when exact; nudge by -1/0/1 along one axis
                let comb =
                    [a[0] + 2 * (b[0] - a[0]) - (c[0] - a[0]), a[1] + 2 * (b[1] - a[1]) - (c[1] - a[1]), a[2] + 2 * (b[2] - a[2]) - (c[2] - a[2])];
                let nudge = (i / 2 % 3) as i64 - 1;
                [comb[0], comb[1], comb[2] + nudge]
            };
            let want = orient3d_ref(a, b, c, d);
            zeros += (want == Sign::Zero) as usize;
            assert_eq!(orient3d(f3(a), f3(b), f3(c), f3(d)).unwrap(), want, "{a:?} {b:?} {c:?} {d:?}");
        }
        assert!(zeros > 1000, "coplanar input must be exercised ({zeros})");
    }

    #[test]
    fn incircle_matches_the_integer_reference_including_cocircular_input() {
        let mut rng = Rng(99);
        let mut zeros = 0;
        for i in 0..20_000 {
            let range = 1i64 << 22;
            let mut p = || [rng.int(range), rng.int(range)];
            let (mut a, mut b, mut c) = (p(), p(), p());
            if orient2d_ref(a, b, c) == Sign::Negative {
                std::mem::swap(&mut b, &mut c);
            }
            let d = if i % 3 == 0 {
                p()
            } else if i % 3 == 1 {
                // Four lattice points of a Pythagorean circle: exactly cocircular.
                a = [3, 4];
                b = [-5, 0];
                c = [0, -5];
                [4, 3]
            } else {
                // Near the circle through a, b, c: nudge a vertex's reflection.
                let nudge = (i / 3 % 3) as i64 - 1;
                [a[0] + nudge, a[1]]
            };
            let (a2, b2, c2) = if orient2d_ref(a, b, c) == Sign::Negative { (a, c, b) } else { (a, b, c) };
            if orient2d_ref(a2, b2, c2) == Sign::Zero {
                continue;
            }
            let want = incircle_ref(a2, b2, c2, d);
            zeros += (want == Sign::Zero) as usize;
            assert_eq!(incircle(f2(a2), f2(b2), f2(c2), f2(d)).unwrap(), want, "{a2:?} {b2:?} {c2:?} {d:?}");
        }
        assert!(zeros > 100, "cocircular input must be exercised ({zeros})");
    }

    #[test]
    fn incircle_survives_huge_and_tiny_scales() {
        // Same configuration at very different scales gives the same answer (powers of two are exact).
        for k in [-90, -40, 0, 40, 90] {
            let s = 2f64.powi(k);
            let (a, b, c) = ([s, 0.0], [0.0, s], [-s, 0.0]);
            assert_eq!(incircle(a, b, c, [0.0, 0.0]).unwrap(), if k.abs() > 90 { Sign::Zero } else { Sign::Positive });
            assert_eq!(incircle(a, b, c, [2.0 * s, 0.0]).unwrap(), Sign::Negative);
            assert_eq!(incircle(a, b, c, [0.0, -s]).unwrap(), Sign::Zero);
        }
    }

    #[test]
    fn segments_cross_properly_cases() {
        let x = segments_cross_properly([0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]).unwrap();
        assert!(x);
        // T junction: touching, not crossing.
        assert!(!segments_cross_properly([0.0, 0.0], [2.0, 0.0], [1.0, 0.0], [1.0, 1.0]).unwrap());
        // Shared endpoint.
        assert!(!segments_cross_properly([0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 1.0]).unwrap());
        // Collinear overlap.
        assert!(!segments_cross_properly([0.0, 0.0], [2.0, 0.0], [1.0, 0.0], [3.0, 0.0]).unwrap());
        // Disjoint.
        assert!(!segments_cross_properly([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]).unwrap());
        assert!(segments_cross_properly([f64::NAN, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]).is_err());
    }
}
