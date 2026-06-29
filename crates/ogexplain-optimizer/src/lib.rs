//! Closed-loop SQL optimization orchestrator.
//!
//! Pipeline: EXPLAIN → diagnose → map → metamorphosis rewrite → re-EXPLAIN →
//! converge.  Uses metamorphosis library API (not subprocess).
//!
//! # Architecture
//!
//! - [`converge`] — convergence detection for the optimization loop
//! - [`mapper`] — maps diagnostic findings to remediation actions
//! - [`verify`] — semantic equivalence verification (QED / VeriEQL)
//! - [`rewrite`] — SQL↔AST encapsulation for metamorphosis integration
//! - [`orchestrator`] — main optimization loop orchestrator

pub mod converge;
pub mod mapper;
pub mod orchestrator;
pub mod rewrite;
pub mod verify;
