// SPDX-License-Identifier: MIT OR Apache-2.0
//! Nodalia's own B-rep kernel (experimental).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod build;
pub mod check;
pub mod curve;
pub mod error;
pub mod mass;
pub mod math;
pub mod nurbs;
pub mod profile;
pub mod quad;
pub mod robust;
pub mod surface;
pub mod tol;
pub mod topology;
pub mod uv;

pub use error::{BrepError, Result};
pub use math::{Frame, Mat3, Point3, Transform, Vec2, Vec3};
pub use tol::Tolerance;
