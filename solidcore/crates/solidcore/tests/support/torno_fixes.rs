//! Torno's fix preset (`core::fixes()` in Torno's `crates/smlib/src/core.rs`): every
//! `UpstreamFixes` flag on. A fix that is broken is fixed or removed in solidcore, never held
//! off in Torno.
//!
//! Other crates' tests include this file by path (`#[path]`), each using part of it.
#![allow(dead_code)] // a test helper

use solidcore::kernel::UpstreamFixes;

/// Every `UpstreamFixes` flag on, as Torno runs.
pub fn torno_fixes() -> UpstreamFixes {
    UpstreamFixes::all()
}

/// The names of the flags [`torno_fixes`] turns on, sorted by name (as
/// `UpstreamFixes::NAMES`).
pub fn torno_fix_names() -> Vec<&'static str> {
    torno_fixes()
        .flags()
        .filter(|&(_, on)| on)
        .map(|(name, _)| name)
        .collect()
}
