// SPDX-License-Identifier: MIT OR Apache-2.0
//! Gauss–Legendre quadrature, with nodes computed at run time (Newton on the Legendre polynomial)
//! rather than copied from a table, so there is nothing to mistype.
//!
//! An `n`-node rule integrates polynomials up to degree `2n − 1` exactly (to rounding). The mass
//! property code applies it panel by panel to trigonometric polynomials of low degree, where a
//! panel narrower than a quarter turn makes the truncation error negligible next to rounding.

use std::sync::OnceLock;

/// Nodes in `(-1, 1)` in increasing order and their weights.
#[derive(Debug, Clone)]
pub struct Rule {
    pub nodes: Vec<f64>,
    pub weights: Vec<f64>,
}

impl Rule {
    /// The `n`-node rule (`n` is clamped to `1..=64`).
    pub fn new(n: usize) -> Rule {
        let n = n.clamp(1, 64);
        let mut nodes = vec![0.0; n];
        let mut weights = vec![0.0; n];
        for i in 0..n {
            // Chebyshev-style initial guess, then Newton on P_n.
            let mut x = (std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
            let mut dp = 1.0;
            for _ in 0..100 {
                let (mut p0, mut p1) = (1.0, x);
                if n == 1 {
                    p1 = x;
                } else {
                    for k in 2..=n {
                        let kf = k as f64;
                        let p2 = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
                        p0 = p1;
                        p1 = p2;
                    }
                }
                // p1 = P_n(x), p0 = P_{n-1}(x) (for n == 1: P_0 = 1).
                let p_prev = if n == 1 { 1.0 } else { p0 };
                dp = n as f64 * (x * p1 - p_prev) / (x * x - 1.0);
                let dx = p1 / dp;
                x -= dx;
                if dx.abs() < 1e-16 {
                    break;
                }
            }
            // Ascending order: the guesses descend from +1.
            let j = n - 1 - i;
            nodes[j] = x;
            weights[j] = 2.0 / ((1.0 - x * x) * dp * dp);
        }
        Rule { nodes, weights }
    }

    /// `∫ₐᵇ f` by one application of the rule on `[a, b]`.
    pub fn integrate(&self, a: f64, b: f64, mut f: impl FnMut(f64) -> f64) -> f64 {
        let (half, mid) = (0.5 * (b - a), 0.5 * (a + b));
        self.nodes.iter().zip(&self.weights).map(|(x, w)| w * f(mid + half * x)).sum::<f64>() * half
    }

    /// `∫ₐᵇ f` over `panels` equal panels.
    pub fn composite(&self, a: f64, b: f64, panels: usize, mut f: impl FnMut(f64) -> f64) -> f64 {
        let panels = panels.max(1);
        let h = (b - a) / panels as f64;
        (0..panels).map(|k| self.integrate(a + h * k as f64, a + h * (k + 1) as f64, &mut f)).sum()
    }
}

/// The shared 8-node rule.
pub fn rule8() -> &'static Rule {
    static R: OnceLock<Rule> = OnceLock::new();
    R.get_or_init(|| Rule::new(8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_two_and_nodes_are_symmetric_and_ascending() {
        for n in 1..=20 {
            let r = Rule::new(n);
            assert!((r.weights.iter().sum::<f64>() - 2.0).abs() < 1e-13, "n={n}");
            assert!(r.nodes.windows(2).all(|w| w[0] < w[1]), "n={n}");
            for (a, b) in r.nodes.iter().zip(r.nodes.iter().rev()) {
                assert!((a + b).abs() < 1e-13, "n={n}");
            }
        }
    }

    #[test]
    fn integrates_polynomials_up_to_degree_2n_minus_1_exactly() {
        for n in 1..=12 {
            let r = Rule::new(n);
            let d = 2 * n - 1;
            // ∫₀² x^d = 2^(d+1)/(d+1)
            let got = r.integrate(0.0, 2.0, |x| x.powi(d as i32));
            let want = 2f64.powi(d as i32 + 1) / (d as f64 + 1.0);
            assert!((got - want).abs() < 1e-11 * want, "n={n}: {got} vs {want}");
        }
    }

    #[test]
    fn composite_integrates_a_trigonometric_polynomial_to_rounding() {
        let r = rule8();
        // ∫₀^{2π} (1 + cos x)³ dx = 2π (1 + 3/2) = 5π
        let got = r.composite(0.0, 2.0 * std::f64::consts::PI, 8, |x| (1.0 + x.cos()).powi(3));
        assert!((got - 5.0 * std::f64::consts::PI).abs() < 1e-13);
    }
}
