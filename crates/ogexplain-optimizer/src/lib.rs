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
#[cfg(feature = "verify")]
pub mod verify;

/// Stub verify module when verify feature is disabled (e.g., musl cross-compile).
#[cfg(not(feature = "verify"))]
pub mod verify {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum VerifyEngine {
        Qed,
        VeriEql,
    }

    impl std::fmt::Display for VerifyEngine {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Qed => write!(f, "qed"),
                Self::VeriEql => write!(f, "verieql"),
            }
        }
    }

    impl std::str::FromStr for VerifyEngine {
        type Err = String;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s.to_ascii_lowercase().as_str() {
                "qed" => Ok(Self::Qed),
                "verieql" => Ok(Self::VeriEql),
                other => Err(format!("unknown verify engine: {other}")),
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize)]
    #[serde(tag = "status", rename_all = "snake_case")]
    pub enum VerifyStatus {
        Equivalent,
        NotEquivalent { counterexample: Option<String> },
        Unknown { reason: String },
        Timeout { seconds: u64 },
        Skipped { reason: SkipReason },
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    pub enum SkipReason {
        NoSchema,
        UserFlagSkipVerify,
        RuleNotVerifiable,
    }

    #[derive(Debug, Clone, Serialize)]
    pub struct VerifyResult {
        pub engine: VerifyEngine,
        pub status: VerifyStatus,
        pub elapsed_ms: Option<u64>,
        pub original_sql: String,
        pub rewritten_sql: String,
        pub raw_output: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum VerificationDecision {
        Accept,
        Reject { counterexample: Option<String> },
    }

    pub fn decide_verification_outcome(_: &VerifyResult) -> VerificationDecision {
        VerificationDecision::Accept
    }
}
