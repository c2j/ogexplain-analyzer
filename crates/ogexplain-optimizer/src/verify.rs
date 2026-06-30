//! Semantic equivalence verification for SQL rewrites.
//!
//! Integrates metamorphosis QED (embedded Z3) and VeriEQL (bounded model checking)
//! as library APIs (not subprocess).
//!
//! # Verification functions
//!
//! - [`verify_qed`] — formal proof via metamorphosis-qed (Z3 SMT solver)
//! - [`verify_verieql`] — bounded model checking via metamorphosis-verieql
//!
//! # Conversion helpers
//!
//! - [`rich_schema_to_verieql`] — convert QED RichSchema to VeriEQL TableSchema[]

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Re-exports from the CLI verify module (migrated)
// ---------------------------------------------------------------------------

/// Verification engine selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifyEngine {
    Qed,
    VeriEql,
}

impl std::fmt::Display for VerifyEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyEngine::Qed => write!(f, "qed"),
            VerifyEngine::VeriEql => write!(f, "verieql"),
        }
    }
}

impl std::str::FromStr for VerifyEngine {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "qed" => Ok(Self::Qed),
            "verieql" => Ok(Self::VeriEql),
            other => Err(format!("unknown verify engine: {other} (expected qed|verieql)")),
        }
    }
}

/// Status of a single verification invocation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum VerifyStatus {
    /// Original and rewritten are provably equivalent.
    Equivalent,
    /// Proof found a counterexample showing non-equivalence.
    NotEquivalent {
        /// Raw counterexample text (may be multi-line).
        counterexample: Option<String>,
    },
    /// Prover gave up (bound too small, unsupported construct, etc.).
    Unknown {
        reason: String,
    },
    /// Prover did not finish within the timeout.
    Timeout {
        seconds: u64,
    },
    /// Verification intentionally skipped.
    Skipped {
        reason: SkipReason,
    },
}

/// Why verification was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// User did not provide schema (auto-downgrade).
    NoSchema,
    /// User passed --skip-verify.
    UserFlagSkipVerify,
    /// The diagnostic rule does not map to a verifiable rewrite.
    RuleNotVerifiable,
}

/// Complete result of one verification invocation.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub engine: VerifyEngine,
    pub status: VerifyStatus,
    /// Elapsed time reported by the prover (ms).
    pub elapsed_ms: Option<u64>,
    pub original_sql: String,
    pub rewritten_sql: String,
    pub raw_output: Option<String>,
}

impl VerifyResult {
    /// Was the rewrite accepted as equivalent?
    pub fn is_equivalent(&self) -> bool {
        matches!(self.status, VerifyStatus::Equivalent)
    }

    /// Was verification skipped (not actually run)?
    pub fn is_skipped(&self) -> bool {
        matches!(self.status, VerifyStatus::Skipped { .. })
    }
}

/// What the optimize loop should do with a verification result.
#[derive(Debug, Clone, PartialEq)]
pub enum VerificationDecision {
    /// Rewrite is safe to accept.
    Accept,
    /// Rewrite must be rejected — stop the loop.
    Reject { counterexample: Option<String> },
}

/// Map a `VerifyResult` to a loop-level decision.
///
/// Policy:
/// - `Equivalent` → Accept
/// - `NotEquivalent` → Reject (stop the loop)
/// - `Unknown`, `Timeout`, `Skipped` → Accept (continue with caveat)
pub fn decide_verification_outcome(result: &VerifyResult) -> VerificationDecision {
    match &result.status {
        VerifyStatus::NotEquivalent { counterexample } => VerificationDecision::Reject {
            counterexample: counterexample.clone(),
        },
        VerifyStatus::Equivalent
        | VerifyStatus::Unknown { .. }
        | VerifyStatus::Timeout { .. }
        | VerifyStatus::Skipped { .. } => VerificationDecision::Accept,
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum VerifyError {
    /// SQL could not be parsed.
    #[error("SQL parse error: {0}")]
    Parse(String),
    /// AST translation / conversion failed.
    #[error("Translation error: {0}")]
    Translation(String),
    /// Prover returned an error.
    #[error("Prover error: {0}")]
    Prover(String),
    /// General verification error.
    #[error("Verification error: {0}")]
    Verification(String),
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a single SQL statement from a string.
fn parse_single_sql(sql: &str) -> Result<ogsql_parser::ast::Statement, VerifyError> {
    let (stmts, errors) = ogsql_parser::parser::Parser::parse_sql(sql);
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        return Err(VerifyError::Parse(msgs.join("; ")));
    }
    stmts
        .into_iter()
        .next()
        .map(|info| info.statement)
        .ok_or_else(|| VerifyError::Parse("no SQL statement found".into()))
}

// ---------------------------------------------------------------------------
// QED verification (formal proof via Z3)
// ---------------------------------------------------------------------------

/// Verify semantic equivalence using metamorphosis-qed (embedded Z3).
///
/// Both SQL strings are parsed via [`ogsql_parser`], then sent to the QED
/// prover along with a [`RichSchema`](metamorphosis_qed::schema::RichSchema).
pub fn verify_qed(
    original_sql: &str,
    rewritten_sql: &str,
    schema: &metamorphosis_qed::schema::RichSchema,
    timeout_secs: u64,
) -> Result<VerifyResult, VerifyError> {
    let original_ast = parse_single_sql(original_sql)?;
    let rewritten_ast = parse_single_sql(rewritten_sql)?;

    let config = metamorphosis_qed::prover::ProverConfig {
        timeout_secs,
        ..Default::default()
    };

    let result = metamorphosis_qed::verify::verify_rewrite(
        "optimize",
        &original_ast,
        &rewritten_ast,
        schema,
        &config,
    )
    .map_err(|e| VerifyError::Prover(e.to_string()))?;

    let status = match &result.proof {
        metamorphosis_qed::prover::ProofResult::Equivalent => VerifyStatus::Equivalent,
        metamorphosis_qed::prover::ProofResult::NotEquivalent { counterexample } => {
            VerifyStatus::NotEquivalent {
                counterexample: counterexample.clone(),
            }
        }
        metamorphosis_qed::prover::ProofResult::Unknown { reason } => {
            VerifyStatus::Unknown {
                reason: reason.clone(),
            }
        }
        metamorphosis_qed::prover::ProofResult::Timeout { seconds } => {
            VerifyStatus::Timeout {
                seconds: *seconds,
            }
        }
        _ => VerifyStatus::Unknown {
            reason: "unexpected proof result".into(),
        },
    };

    Ok(VerifyResult {
        engine: VerifyEngine::Qed,
        status,
        elapsed_ms: Some(result.elapsed_ms),
        original_sql: original_sql.to_string(),
        rewritten_sql: rewritten_sql.to_string(),
        raw_output: None,
    })
}

// ---------------------------------------------------------------------------
// VeriEQL verification (bounded model checking)
// ---------------------------------------------------------------------------

/// Verify semantic equivalence using metamorphosis-verieql (bounded MCF).
///
/// Schema is provided as a slice of [`TableSchema`](metamorphosis_verieql::types::TableSchema).
/// See [`rich_schema_to_verieql`] to convert from QED's RichSchema format.
pub fn verify_verieql(
    original_sql: &str,
    rewritten_sql: &str,
    schema: &[metamorphosis_verieql::types::TableSchema],
    constraints: &serde_json::Value,
    bound: usize,
) -> Result<VerifyResult, VerifyError> {
    use metamorphosis_verieql::types::{Bound, Semantics};
    use metamorphosis_verieql::VeriEql;

    let report = VeriEql::verify(
        original_sql,
        rewritten_sql,
        schema,
        constraints,
        Bound(bound),
        Semantics::Bag,
    )
    .map_err(|e| VerifyError::Prover(e.to_string()))?;

    let elapsed_ms = Some(report.translate_ms + report.solve_ms);

    let status = match &report.result {
        metamorphosis_verieql::types::ProofResult::Equivalent => VerifyStatus::Equivalent,
        metamorphosis_verieql::types::ProofResult::NotEquivalent { counterexample } => {
            let ce_text = format_verieql_counterexample(counterexample);
            VerifyStatus::NotEquivalent {
                counterexample: Some(ce_text),
            }
        }
        metamorphosis_verieql::types::ProofResult::Unknown { reason } => {
            VerifyStatus::Unknown {
                reason: reason.clone(),
            }
        }
    };

    Ok(VerifyResult {
        engine: VerifyEngine::VeriEql,
        status,
        elapsed_ms,
        original_sql: original_sql.to_string(),
        rewritten_sql: rewritten_sql.to_string(),
        raw_output: None,
    })
}

/// Format a VeriEQL counterexample into a human-readable string.
fn format_verieql_counterexample(ce: &metamorphosis_verieql::types::Counterexample) -> String {
    // Try to format using Debug; Counterexample is likely a struct with table rows.
    format!("{ce:#?}")
}

// ---------------------------------------------------------------------------
// Schema conversion helpers
// ---------------------------------------------------------------------------

/// Convert a QED [`RichSchema`](metamorphosis_qed::schema::RichSchema) to
/// a Vec of VeriEQL [`TableSchema`](metamorphosis_verieql::types::TableSchema).
pub fn rich_schema_to_verieql(
    schema: &metamorphosis_qed::schema::RichSchema,
) -> Vec<metamorphosis_verieql::types::TableSchema> {
    use metamorphosis_verieql::types::{ColumnDef, TableSchema};

    schema
        .tables
        .iter()
        .map(|(name, info)| {
            let columns = info
                .columns
                .iter()
                .map(|col| {
                    let col_type = map_data_type(&col.data_type);
                    ColumnDef {
                        name: col.name.clone(),
                        col_type,
                    }
                })
                .collect();
            TableSchema {
                name: name.clone(),
                columns,
            }
        })
        .collect()
}

/// Map SQL data type string to VeriEQL ColumnType.
fn map_data_type(dt: &str) -> metamorphosis_verieql::types::ColumnType {
    match dt.to_uppercase().as_str() {
        "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "SERIAL" | "BIGSERIAL"
        | "SMALLSERIAL" | "NUMERIC" | "DECIMAL" | "FLOAT" | "DOUBLE" | "REAL" => {
            metamorphosis_verieql::types::ColumnType::Integer
        }
        "BOOLEAN" | "BOOL" => metamorphosis_verieql::types::ColumnType::Boolean,
        _ => metamorphosis_verieql::types::ColumnType::Integer,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Engine display/parse roundtrip ────────────────────────────────────

    #[test]
    fn engine_display_roundtrip() {
        assert_eq!(VerifyEngine::Qed.to_string(), "qed");
        assert_eq!(VerifyEngine::VeriEql.to_string(), "verieql");
        assert_eq!("qed".parse::<VerifyEngine>().unwrap(), VerifyEngine::Qed);
        assert_eq!(
            "VERIEQL".parse::<VerifyEngine>().unwrap(),
            VerifyEngine::VeriEql
        );
    }

    #[test]
    fn engine_from_str_unknown() {
        assert!("unknown".parse::<VerifyEngine>().is_err());
    }

    // ── Decision mapping ──────────────────────────────────────────────────

    #[test]
    fn decide_accepts_equivalent() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Equivalent,
            elapsed_ms: Some(22),
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert_eq!(decide_verification_outcome(&r), VerificationDecision::Accept);
    }

    #[test]
    fn decide_rejects_not_equivalent() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::NotEquivalent {
                counterexample: Some("users has 2 rows with id=5".into()),
            },
            elapsed_ms: Some(45),
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert_eq!(
            decide_verification_outcome(&r),
            VerificationDecision::Reject {
                counterexample: Some("users has 2 rows with id=5".into()),
            }
        );
    }

    #[test]
    fn decide_accepts_unknown_as_caveat() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Unknown {
                reason: "bound too small".into(),
            },
            elapsed_ms: Some(1),
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert_eq!(decide_verification_outcome(&r), VerificationDecision::Accept);
    }

    #[test]
    fn decide_accepts_skipped() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Skipped {
                reason: SkipReason::NoSchema,
            },
            elapsed_ms: None,
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert_eq!(decide_verification_outcome(&r), VerificationDecision::Accept);
    }

    #[test]
    fn decide_accepts_timeout() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Timeout { seconds: 60 },
            elapsed_ms: Some(60_000),
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert_eq!(decide_verification_outcome(&r), VerificationDecision::Accept);
    }

    // ── VerifyResult helper methods ───────────────────────────────────────

    #[test]
    fn verify_result_is_equivalent_true() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Equivalent,
            elapsed_ms: Some(10),
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert!(r.is_equivalent());
        assert!(!r.is_skipped());
    }

    #[test]
    fn verify_result_is_skipped_true() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Skipped {
                reason: SkipReason::NoSchema,
            },
            elapsed_ms: None,
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert!(r.is_skipped());
        assert!(!r.is_equivalent());
    }

    #[test]
    fn verify_result_not_equivalent_not_skipped() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::NotEquivalent {
                counterexample: None,
            },
            elapsed_ms: Some(5),
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert!(!r.is_equivalent());
        assert!(!r.is_skipped());
    }

    // ── SkipReason Display (via Debug) ────────────────────────────────────

    #[test]
    fn skip_reason_debug() {
        assert_eq!(format!("{:?}", SkipReason::NoSchema), "NoSchema");
        assert_eq!(
            format!("{:?}", SkipReason::UserFlagSkipVerify),
            "UserFlagSkipVerify"
        );
        assert_eq!(
            format!("{:?}", SkipReason::RuleNotVerifiable),
            "RuleNotVerifiable"
        );
    }

    // ── VerifyError Display ───────────────────────────────────────────────

    #[test]
    fn verify_error_display_parse() {
        let err = VerifyError::Parse("syntax error".into());
        assert!(err.to_string().contains("syntax error"));
    }

    #[test]
    fn verify_error_display_prover() {
        let err = VerifyError::Prover("Z3 failed".into());
        assert!(err.to_string().contains("Z3 failed"));
    }

    #[test]
    fn verify_error_display_translation() {
        let err = VerifyError::Translation("unsupported type".into());
        assert!(err.to_string().contains("unsupported type"));
    }

    #[test]
    fn verify_error_display_verification() {
        let err = VerifyError::Verification("general error".into());
        assert!(err.to_string().contains("general error"));
    }

    // ── rich_schema_to_verieql conversion ─────────────────────────────────

    #[test]
    fn rich_schema_to_verieql_converts_tables() {
        use metamorphosis_qed::schema::{ColumnInfo, RichSchema, TableInfo};
        use std::collections::HashMap;

        let mut tables = HashMap::new();
        tables.insert(
            "users".into(),
            TableInfo {
                columns: vec![
                    ColumnInfo {
                        name: "id".into(),
                        data_type: "INTEGER".into(),
                        nullable: false,
                        is_primary_key: true,
                        is_unique: true,
                    },
                    ColumnInfo {
                        name: "name".into(),
                        data_type: "TEXT".into(),
                        nullable: true,
                        is_primary_key: false,
                        is_unique: false,
                    },
                ],
                constraints: Default::default(),
            },
        );

        let schema = RichSchema { tables };
        let result = rich_schema_to_verieql(&schema);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "users");
        assert_eq!(result[0].columns.len(), 2);
        assert_eq!(result[0].columns[0].name, "id");
        // INTEGER → Integer
        assert_eq!(
            result[0].columns[0].col_type,
            metamorphosis_verieql::types::ColumnType::Integer
        );
        // TEXT (unknown) → Integer (default)
        assert_eq!(
            result[0].columns[1].col_type,
            metamorphosis_verieql::types::ColumnType::Integer
        );
    }

    #[test]
    fn rich_schema_to_verieql_handles_boolean() {
        use metamorphosis_qed::schema::{ColumnInfo, RichSchema, TableInfo};
        use std::collections::HashMap;

        let mut tables = HashMap::new();
        tables.insert(
            "flags".into(),
            TableInfo {
                columns: vec![ColumnInfo {
                    name: "is_active".into(),
                    data_type: "BOOLEAN".into(),
                    nullable: false,
                    is_primary_key: false,
                    is_unique: false,
                }],
                constraints: Default::default(),
            },
        );

        let schema = RichSchema { tables };
        let result = rich_schema_to_verieql(&schema);
        assert_eq!(
            result[0].columns[0].col_type,
            metamorphosis_verieql::types::ColumnType::Boolean
        );
    }

    #[test]
    fn rich_schema_to_verieql_empty() {
        use metamorphosis_qed::schema::RichSchema;
        use std::collections::HashMap;

        let schema = RichSchema {
            tables: HashMap::new(),
        };
        let result = rich_schema_to_verieql(&schema);
        assert!(result.is_empty());
    }

    // ── parse_single_sql (tested via verify_qed input validation) ─────────
    // Actual parsing is tested through the verify functions in integration.

    #[test]
    fn map_data_type_integer_variants() {
        for dt in &["INTEGER", "int", "BIGINT", "SMALLINT", "SERIAL", "BIGSERIAL"] {
            assert_eq!(
                map_data_type(dt),
                metamorphosis_verieql::types::ColumnType::Integer,
                "expected Integer for {dt}"
            );
        }
    }

    #[test]
    fn map_data_type_boolean_variants() {
        for dt in &["BOOLEAN", "boolean", "BOOL"] {
            assert_eq!(
                map_data_type(dt),
                metamorphosis_verieql::types::ColumnType::Boolean,
                "expected Boolean for {dt}"
            );
        }
    }

    #[test]
    fn map_data_type_unknown_defaults_to_integer() {
        assert_eq!(
            map_data_type("TEXT"),
            metamorphosis_verieql::types::ColumnType::Integer
        );
        assert_eq!(
            map_data_type("VARCHAR"),
            metamorphosis_verieql::types::ColumnType::Integer
        );
        assert_eq!(
            map_data_type("DATE"),
            metamorphosis_verieql::types::ColumnType::Integer
        );
    }
}
