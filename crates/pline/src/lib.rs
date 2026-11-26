// SPDX-License-Identifier: MIT OR Apache-2.0
//! Nodalia's own closed polylines with arc segments.
//!
//! A [`Pline`] is a closed chain of vertices; each vertex carries a *bulge* that describes the
//! segment leaving it: `0` is a straight line, otherwise the tangent of a quarter of the arc's
//! sweep (positive counter-clockwise, `1` is a half circle). The crate measures them (area,
//! extents, winding number) and combines two of them with [`Pline::boolean`].
//!
//! Everything from outside is hostile: non-finite numbers, degenerate or tiny inputs and huge
//! polylines make `boolean` answer [`BooleanInfo::InvalidInput`] instead of panicking.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod boolean;
mod geom;
mod measure;

pub use boolean::{BooleanInfo, BooleanOp, BooleanResult};
pub use measure::Extents;

/// One vertex: where a segment starts and the bulge of that segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    /// `0` for a line; `tan(sweep / 4)` for an arc to the next vertex, positive counter-clockwise.
    pub bulge: f64,
}

/// A closed polyline of lines and arcs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pline {
    vertices: Vec<Vertex>,
}

impl Pline {
    /// An empty closed polyline.
    pub fn new_closed() -> Pline {
        Pline { vertices: Vec::new() }
    }

    /// Appends a vertex.
    pub fn add(&mut self, x: f64, y: f64, bulge: f64) {
        self.vertices.push(Vertex { x, y, bulge });
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    /// The vertex at `i` (wrapping around), if there are any.
    pub fn at(&self, i: usize) -> Option<Vertex> {
        if self.vertices.is_empty() { None } else { self.vertices.get(i % self.vertices.len()).copied() }
    }

    /// The index after `i`, wrapping to 0.
    pub fn next_wrapping_index(&self, i: usize) -> usize {
        if self.vertices.is_empty() { 0 } else { (i + 1) % self.vertices.len() }
    }

    /// The polyline with consecutive vertices closer than `eps` merged (the first position with
    /// the last bulge, since the bulge of a zero-length segment means nothing), or `None` when
    /// nothing was merged.
    pub fn remove_repeat_pos(&self, eps: f64) -> Option<Pline> {
        let n = self.vertices.len();
        if n < 2 {
            return None;
        }
        let near = |a: &Vertex, b: &Vertex| (a.x - b.x).hypot(a.y - b.y) <= eps;
        let mut out: Vec<Vertex> = Vec::with_capacity(n);
        for v in &self.vertices {
            match out.last_mut() {
                Some(last) if near(last, v) => last.bulge = v.bulge,
                _ => out.push(*v),
            }
        }
        while out.len() > 1 && near(&out[0], &out[out.len() - 1]) {
            if let Some(last) = out.pop() {
                out[0].bulge = last.bulge;
            }
        }
        if out.len() == n { None } else { Some(Pline { vertices: out }) }
    }

    /// Reverses the direction of travel, keeping the same outline.
    pub fn invert_direction_mut(&mut self) {
        let n = self.vertices.len();
        if n < 2 {
            return;
        }
        // The segment leaving vertex i goes to i+1; reversed, the vertex i+1 leaves towards i
        // with the negated bulge.
        let old = self.vertices.clone();
        for (k, v) in self.vertices.iter_mut().enumerate() {
            let src = (n - k) % n;
            let prev = (src + n - 1) % n;
            *v = Vertex { x: old[src].x, y: old[src].y, bulge: -old[prev].bulge };
        }
    }

    /// Scales every coordinate by `factor` about the origin.
    pub fn scale_mut(&mut self, factor: f64) {
        for v in &mut self.vertices {
            v.x *= factor;
            v.y *= factor;
        }
    }

    pub fn translate_mut(&mut self, dx: f64, dy: f64) {
        for v in &mut self.vertices {
            v.x += dx;
            v.y += dy;
        }
    }

    /// Whether every coordinate and bulge is finite.
    pub fn is_finite(&self) -> bool {
        self.vertices.iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.bulge.is_finite())
    }
}

#[cfg(test)]
mod tests;
