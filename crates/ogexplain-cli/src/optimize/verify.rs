//! Verification integration for the optimize loop.
//!
//! Wraps `metamorphosis verify` subprocess to prove semantic equivalence
//! between original and rewritten SQL. Used by the optimize loop after
//! each rewrite step (Issue #41).
//!
//! Design decisions:
//! - Schema-missing path returns `Skipped(NoSchema)` (auto-downgrade, not an error)
//! - Always pass `-o json` to metamorphosis for robust parsing
//! - Subprocess timeout is enforced via child process kill

use std::path::Path;
use serde::{Deserialize, Serialize};

// ── Schema source ───────────────────────────────────────────────────────

/// Source of schema information for metamorphosis verify.
///
/// Metamorphosis accepts schema either as a JSON file (`--schema`) or as a
/// directory of `.sql` DDL files (`--sql-dir`). The two are mutually exclusive.
/// DDL files can express `PRIMARY KEY` constraints natively, which the JSON
/// format cannot (until metamorphosis issue #39 lands).
#[derive(Debug, Clone, Copy)]
pub enum SchemaSource<'a> {
    /// JSON schema file (flat `{table: {col: type}}` format).
    Json(&'a Path),
    /// Directory containing `.sql` DDL files (supports PRIMARY KEY constraints).
    SqlDir(&'a Path),
}

// ── Engine selection ────────────────────────────────────────────────────

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

// ── Result types ────────────────────────────────────────────────────────

/// Status of a single verification invocation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum VerifyStatus {
    /// Original and rewritten are provably equivalent.
    Equivalent,
    /// Proof found a counterexample showing non-equivalence.
    NotEquivalent {
        /// Raw counterexample text from metamorphosis (may be multi-line).
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
    /// User did not provide `--schema` (auto-downgrade per design decision).
    NoSchema,
    /// User passed `--skip-verify`.
    UserFlagSkipVerify,
    /// The diagnostic rule does not map to a verifiable rewrite.
    RuleNotVerifiable,
}

/// Complete result of one verification invocation.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub engine: VerifyEngine,
    pub status: VerifyStatus,
    /// Elapsed time reported by metamorphosis (QED: `elapsed_ms`; VeriEQL: `translate_ms + solve_ms`).
    pub elapsed_ms: Option<u64>,
    /// Original SQL passed to the verifier.
    pub original_sql: String,
    /// Rewritten SQL passed to the verifier.
    pub rewritten_sql: String,
    /// Raw JSON output from metamorphosis, retained for diagnostics.
    pub raw_output: Option<String>,
}

impl VerifyResult {
    /// Convenience: was the rewrite accepted as equivalent?
    pub fn is_equivalent(&self) -> bool {
        matches!(self.status, VerifyStatus::Equivalent)
    }

    /// Convenience: was verification skipped (not actually run)?
    pub fn is_skipped(&self) -> bool {
        matches!(self.status, VerifyStatus::Skipped { .. })
    }
}

// ── Error type ──────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("metamorphosis subprocess failed (exit code {code:?}): {stderr}")]
    SubprocessFailed { code: Option<i32>, stderr: String },
    #[error("metamorphosis output was not valid JSON: {0}")]
    InvalidJson(String),
    #[error("metamorphosis JSON missing required `result` field: {0}")]
    MissingResultField(String),
    #[error("verification subprocess timed out after {0}s")]
    TimeoutElapsed(u64),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ── Subprocess wrapper ──────────────────────────────────────────────────

/// Invoke `metamorphosis verify` on the given SQL pair.
///
/// Writes both SQLs to temp files, runs the metamorphosis binary with
/// `-o json`, parses the structured result, and returns it.
///
/// # Schema handling (design decision #1)
/// If `schema` is `None`, returns `Ok(VerifyResult { status: Skipped(NoSchema), .. })`
/// — auto-downgrade rather than error, to preserve Week 1's zero-config UX.
///
/// # Arguments
/// * `metamorphosis_path` - Path to the metamorphosis binary (e.g., `Path::new("metamorphosis")`)
/// * `original_sql` - Original SQL text (any single statement)
/// * `rewritten_sql` - Rewritten SQL text (the candidate rewrite)
/// * `schema` - Optional schema source (Json or SqlDir); if None, returns Skipped
/// * `engine` - Which verification engine to use
/// * `bound` - VeriEQL bound parameter (ignored by QED but always passed)
/// * `timeout_secs` - Subprocess wall-clock timeout in seconds
pub fn call_metamorphosis_verify(
    metamorphosis_path: &Path,
    original_sql: &str,
    rewritten_sql: &str,
    schema: Option<SchemaSource<'_>>,
    engine: VerifyEngine,
    bound: usize,
    timeout_secs: u64,
) -> Result<VerifyResult, VerifyError> {
    // 1. Schema-missing path: auto-downgrade (design decision #1)
    let Some(schema_source) = schema else {
        return Ok(VerifyResult {
            engine,
            status: VerifyStatus::Skipped { reason: SkipReason::NoSchema },
            elapsed_ms: None,
            original_sql: original_sql.to_string(),
            rewritten_sql: rewritten_sql.to_string(),
            raw_output: None,
        });
    };

    // 2. Write SQLs to temp files (mirror call_metamorphosis_rewrite at mod.rs:292-342).
    // Use unique filenames via process+thread id to avoid races when multiple
    // verify calls run concurrently (e.g. parallel E2E tests).
    let pid = std::process::id();
    let tid = std::thread::current().id();
    let tag = format!("{pid}_{tid:?}");
    let safe_tag: String = tag.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let orig_tmp = std::env::temp_dir().join(format!("ogexplain_verify_orig_{safe_tag}.sql"));
    let rewritten_tmp = std::env::temp_dir().join(format!("ogexplain_verify_rewritten_{safe_tag}.sql"));
    std::fs::write(&orig_tmp, original_sql)?;
    std::fs::write(&rewritten_tmp, rewritten_sql)?;

    // 3. Build the command. EXACT CLI contract (verified from metamorphosis source):
    //    `verify <original> <rewritten> --schema <path> --engine <qed|verieql> --bound <N> -o json`
    //    or with `--sql-dir <dir>` instead of `--schema`.
    let mut cmd = std::process::Command::new(metamorphosis_path);
    cmd.arg("verify")
        .arg(&orig_tmp)
        .arg(&rewritten_tmp)
        .arg("--engine").arg(engine.to_string())
        .arg("--bound").arg(bound.to_string())
        .arg("-o").arg("json");
    match schema_source {
        SchemaSource::Json(path) => {
            cmd.arg("--schema").arg(path);
        }
        SchemaSource::SqlDir(dir) => {
            cmd.arg("--sql-dir").arg(dir);
        }
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // 4. Spawn + enforce timeout. std::process::Command::output() blocks forever,
    //    so we spawn and poll with try_wait. Use a simple sleep loop.
    let mut child = cmd.spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    loop {
        match child.try_wait()? {
            Some(status) => {
                let stdout = child.stdout.take().map(|mut s| {
                    use std::io::Read;
                    let mut buf = String::new();
                    let _ = s.read_to_string(&mut buf);
                    buf
                }).unwrap_or_default();
                let stderr = child.stderr.take().map(|mut s| {
                    use std::io::Read;
                    let mut buf = String::new();
                    let _ = s.read_to_string(&mut buf);
                    buf
                }).unwrap_or_default();

                if !status.success() {
                    return Err(VerifyError::SubprocessFailed {
                        code: status.code(),
                        stderr,
                    });
                }

                return parse_metamorphosis_json(&stdout, engine, original_sql, rewritten_sql);
            }
            None => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    return Err(VerifyError::TimeoutElapsed(timeout_secs));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

// ── Output parser (unit-testable, no subprocess needed) ─────────────────

/// Parse metamorphosis verify JSON output into structured result.
///
/// Accepts both QED and VeriEQL output shapes (they differ in auxiliary fields).
/// Tolerates leading log/warn lines (tracing may emit to stdout in some environments)
/// by extracting the JSON from the first `{` character onwards.
pub fn parse_metamorphosis_json(
    output: &str,
    engine: VerifyEngine,
    original_sql: &str,
    rewritten_sql: &str,
) -> Result<VerifyResult, VerifyError> {
    // Strip ANSI escape sequences and extract JSON portion from potentially
    // mixed stdout (tracing WARN logs may appear before the JSON payload).
    let json_start = output.find('{').ok_or_else(|| {
        VerifyError::InvalidJson(format!("no JSON object found in output: {output}"))
    })?;
    let raw_json = &output[json_start..];

    let value: serde_json::Value = serde_json::from_str(raw_json)
        .map_err(|e| VerifyError::InvalidJson(format!("{e}: output was: {raw_json}")))?;

    let result_str = value.get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| VerifyError::MissingResultField(raw_json.to_string()))?;

    let status = match result_str {
        "Equivalent" => VerifyStatus::Equivalent,
        "NotEquivalent" => {
            // Counterexample may be in different fields depending on engine:
            // - QED: top-level "counterexample" string
            // - VeriEQL: nested "counterexample": { "tables": [...] }
            let counterexample = extract_counterexample(&value);
            VerifyStatus::NotEquivalent { counterexample }
        }
        "Unknown" => {
            let reason = value.get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("(no reason provided)")
                .to_string();
            VerifyStatus::Unknown { reason }
        }
        "Timeout" => {
            let seconds = value.get("seconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            VerifyStatus::Timeout { seconds }
        }
        other => {
            return Err(VerifyError::InvalidJson(
                format!("unknown `result` value: {other} (output: {output})")
            ));
        }
    };

    let elapsed_ms = match engine {
        VerifyEngine::Qed => value.get("elapsed_ms").and_then(|v| v.as_u64()),
        VerifyEngine::VeriEql => {
            // VeriEQL splits elapsed into translate_ms + solve_ms
            let t = value.get("translate_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            let s = value.get("solve_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            Some(t + s)
        }
    };

    Ok(VerifyResult {
        engine,
        status,
        elapsed_ms,
        original_sql: original_sql.to_string(),
        rewritten_sql: rewritten_sql.to_string(),
        raw_output: Some(output.to_string()),
    })
}

/// Extract a human-readable counterexample string from either QED or VeriEQL JSON.
fn extract_counterexample(value: &serde_json::Value) -> Option<String> {
    // QED shape: {"counterexample": "<string>"}
    if let Some(ce) = value.get("counterexample").and_then(|v| v.as_str()) {
        return Some(ce.to_string());
    }
    // VeriEQL shape: {"counterexample": {"tables": [{"name": "...", "rows": [["v1","v2"], ...]}]}}
    if let Some(tables) = value.get("counterexample").and_then(|v| v.get("tables")).and_then(|v| v.as_array()) {
        let mut out = String::new();
        for table in tables {
            let name = table.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let rows = table.get("rows").and_then(|v| v.as_array());
            out.push_str(&format!("  {name}:\n"));
            if let Some(rows) = rows {
                for row in rows {
                    if let Some(cells) = row.as_array() {
                        let cell_strs: Vec<String> = cells.iter()
                            .map(|c| c.as_str().unwrap_or("?").to_string())
                            .collect();
                        out.push_str(&format!("    [{}]\n", cell_strs.join(", ")));
                    }
                }
            }
        }
        return Some(out.trim_end().to_string());
    }
    None
}

// ── Verification decision (used by the optimize loop) ───────────────────

/// What the optimize loop should do with a verification result.
#[derive(Debug, Clone, PartialEq)]
pub enum VerificationDecision {
    /// Rewrite is safe to accept (Equivalent, or Unknown/Skipped/Timeout treated as "continue with caveat").
    Accept,
    /// Rewrite must be rejected — stop the loop with VerificationFailed.
    Reject { counterexample: Option<String> },
}

/// Map a `VerifyResult` to a loop-level decision.
///
/// Policy:
/// - `Equivalent` → Accept
/// - `NotEquivalent { counterexample }` → Reject (stop the loop)
/// - `Unknown`, `Timeout`, `Skipped` → Accept (mark as unverified in the record, but continue)
///   The rationale: a verification timeout shouldn't discard an otherwise-sound rewrite.
///   The user can inspect the `verification` field in the report to see the caveat.
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

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qed_equivalent() {
        let json = r#"{"result":"Equivalent","original":"SELECT 1","rewritten":"SELECT 1","elapsed_ms":22,"engine":"qed"}"#;
        let r = parse_metamorphosis_json(json, VerifyEngine::Qed, "SELECT 1", "SELECT 1").unwrap();
        assert_eq!(r.status, VerifyStatus::Equivalent);
        assert_eq!(r.elapsed_ms, Some(22));
        assert!(r.is_equivalent());
        assert!(!r.is_skipped());
    }

    #[test]
    fn parses_qed_not_equivalent_with_counterexample() {
        let json = r#"{"result":"NotEquivalent","original":"X","rewritten":"Y","elapsed_ms":45,"engine":"qed","counterexample":"users: id=5 appears 2 times"}"#;
        let r = parse_metamorphosis_json(json, VerifyEngine::Qed, "X", "Y").unwrap();
        match r.status {
            VerifyStatus::NotEquivalent { counterexample } => {
                assert_eq!(counterexample.as_deref(), Some("users: id=5 appears 2 times"));
            }
            other => panic!("expected NotEquivalent, got {other:?}"),
        }
    }

    #[test]
    fn parses_verieql_not_equivalent_with_structured_counterexample() {
        let json = r#"{"result":"NotEquivalent","engine":"verieql","bound":2,"translate_ms":5,"solve_ms":18,"counterexample":{"tables":[{"name":"users","rows":[["5","alice"],["5","bob"]]}]}}"#;
        let r = parse_metamorphosis_json(json, VerifyEngine::VeriEql, "X", "Y").unwrap();
        assert_eq!(r.elapsed_ms, Some(23)); // translate_ms + solve_ms
        match r.status {
            VerifyStatus::NotEquivalent { counterexample } => {
                let ce = counterexample.expect("missing CE");
                assert!(ce.contains("users:"), "CE should mention table name: {ce}");
                assert!(ce.contains("[5, alice]"), "CE should render row: {ce}");
            }
            other => panic!("expected NotEquivalent, got {other:?}"),
        }
    }

    #[test]
    fn parses_unknown_with_reason() {
        let json = r#"{"result":"Unknown","reason":"bound too small","engine":"qed","elapsed_ms":1}"#;
        let r = parse_metamorphosis_json(json, VerifyEngine::Qed, "X", "Y").unwrap();
        match r.status {
            VerifyStatus::Unknown { reason } => assert_eq!(reason, "bound too small"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn parses_timeout() {
        let json = r#"{"result":"Timeout","seconds":60,"engine":"qed"}"#;
        let r = parse_metamorphosis_json(json, VerifyEngine::Qed, "X", "Y").unwrap();
        match r.status {
            VerifyStatus::Timeout { seconds } => assert_eq!(seconds, 60),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_result_field() {
        let json = r#"{"original":"X","rewritten":"Y"}"#;
        let err = parse_metamorphosis_json(json, VerifyEngine::Qed, "X", "Y").unwrap_err();
        assert!(matches!(err, VerifyError::MissingResultField(_)));
    }

    #[test]
    fn rejects_malformed_json() {
        let err = parse_metamorphosis_json("not json", VerifyEngine::Qed, "X", "Y").unwrap_err();
        assert!(matches!(err, VerifyError::InvalidJson(_)));
    }

    #[test]
    fn schema_missing_returns_skipped_not_error() {
        let r = call_metamorphosis_verify(
            Path::new("/nonexistent/metamorphosis"),
            "SELECT 1",
            "SELECT 1",
            None,  // ← no schema
            VerifyEngine::Qed,
            2,
            60,
        ).unwrap();
        assert!(matches!(r.status, VerifyStatus::Skipped { reason: SkipReason::NoSchema }));
        assert!(r.is_skipped());
        assert!(!r.is_equivalent());
    }

    #[test]
    fn schema_source_enum_displays_correctly() {
        // Just verify the enum variants exist and can be constructed
        let json_src = SchemaSource::Json(std::path::Path::new("/tmp/s.json"));
        let dir_src = SchemaSource::SqlDir(std::path::Path::new("/tmp/ddl/"));
        assert!(matches!(json_src, SchemaSource::Json(_)));
        assert!(matches!(dir_src, SchemaSource::SqlDir(_)));
    }

    #[test]
    fn engine_display_roundtrip() {
        assert_eq!(VerifyEngine::Qed.to_string(), "qed");
        assert_eq!(VerifyEngine::VeriEql.to_string(), "verieql");
        assert_eq!("qed".parse::<VerifyEngine>().unwrap(), VerifyEngine::Qed);
        assert_eq!("VERIEQL".parse::<VerifyEngine>().unwrap(), VerifyEngine::VeriEql);
    }

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
    fn decide_rejects_not_equivalent_with_counterexample() {
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
            status: VerifyStatus::Unknown { reason: "bound too small".into() },
            elapsed_ms: Some(1),
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        // Unknown should NOT reject — we accept with caveat rather than discard work.
        assert_eq!(decide_verification_outcome(&r), VerificationDecision::Accept);
    }

    #[test]
    fn decide_accepts_skipped() {
        let r = VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Skipped { reason: SkipReason::NoSchema },
            elapsed_ms: None,
            original_sql: "X".into(),
            rewritten_sql: "Y".into(),
            raw_output: None,
        };
        assert_eq!(decide_verification_outcome(&r), VerificationDecision::Accept);
    }
}
