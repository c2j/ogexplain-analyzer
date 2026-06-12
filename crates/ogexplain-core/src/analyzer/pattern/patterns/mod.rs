//! Anti-pattern definitions.
//!
//! Each submodule implements a single anti-pattern via the
//! [`AntiPatternDef`](super::engine::AntiPatternDef) trait.

pub mod gather_then_sort;
pub mod index_scan_amplify;
pub mod materialize_cascade;
