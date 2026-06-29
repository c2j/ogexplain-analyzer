//! Anti-pattern definitions.
//!
//! Each submodule implements a single anti-pattern via the
//! [`AntiPatternDef`](super::engine::AntiPatternDef) trait.

pub mod agg_over_streaming;
pub mod gather_then_sort;
pub mod hash_join_skewed;
pub mod index_heap_fetches;
pub mod index_scan_amplify;
pub mod materialize_cascade;
pub mod multi_distinct;
pub mod nested_loop_sort;
