// SPDX-License-Identifier: MIT OR Apache-2.0
//! The error type of the crate: every failure on hostile input is a value, never a panic.

use std::fmt;

/// Why an operation refused or failed. The message says what was wrong and, where it helps,
/// what is allowed.
#[derive(Debug, Clone, PartialEq)]
pub enum BrepError {
    /// A number was NaN or infinite where a finite one is required.
    NonFinite(String),
    /// A number was finite but outside the range the operation supports.
    OutOfRange(String),
    /// An argument is invalid (wrong count, not positive, ids that do not exist, ...).
    Invalid(String),
    /// The geometry is degenerate for this operation (zero length, parallel axes, ...).
    Degenerate(String),
    /// The operation is not implemented for this input yet. Never a panic: a clear refusal.
    Unsupported(String),
    /// An iterative method did not converge within its iteration cap.
    NotConverged(String),
    /// The topology is inconsistent in a way the operation cannot work around.
    Topology(String),
    /// Tessellation could not produce a mesh for the input.
    Tessellation(String),
}

impl BrepError {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        BrepError::Invalid(msg.into())
    }
    pub(crate) fn non_finite(what: impl Into<String>) -> Self {
        BrepError::NonFinite(what.into())
    }
    pub(crate) fn degenerate(msg: impl Into<String>) -> Self {
        BrepError::Degenerate(msg.into())
    }
    pub(crate) fn unsupported(msg: impl Into<String>) -> Self {
        BrepError::Unsupported(msg.into())
    }
    pub(crate) fn topology(msg: impl Into<String>) -> Self {
        BrepError::Topology(msg.into())
    }
}

impl fmt::Display for BrepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, msg) = match self {
            BrepError::NonFinite(m) => ("not a finite number", m),
            BrepError::OutOfRange(m) => ("out of range", m),
            BrepError::Invalid(m) => ("invalid input", m),
            BrepError::Degenerate(m) => ("degenerate geometry", m),
            BrepError::Unsupported(m) => ("not supported yet", m),
            BrepError::NotConverged(m) => ("did not converge", m),
            BrepError::Topology(m) => ("inconsistent topology", m),
            BrepError::Tessellation(m) => ("tessellation failed", m),
        };
        write!(f, "{kind}: {msg}")
    }
}

impl std::error::Error for BrepError {}

/// The crate's result type.
pub type Result<T> = std::result::Result<T, BrepError>;
