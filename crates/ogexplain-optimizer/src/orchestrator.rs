//! Closed-loop optimization orchestrator.
//!
//! Core pipeline: EXPLAIN → diagnose → map → rewrite → verify → converge.
//! Migrated from `ogexplain-cli/src/optimize/mod.rs` with DB abstraction via
//! [`ExplainExecutor`] trait.
//!
//! # Usage
//!
//! ```no_run
//! use ogexplain_optimizer::orchestrator::{OptimizeConfig, run_optimize, ExplainExecutor};
//!
//! struct MyDb;
//! impl ExplainExecutor for MyDb {
//!     fn fetch_explain(&self, sql: &str, analyze: bool) -> Result<String, String> {
//!         Ok(format!("EXPLAIN output for: {sql}"))
//!     }
//! }
//!
//! let config = OptimizeConfig {
//!     sql: "SELECT * FROM t WHERE id IN (SELECT id FROM u)".into(),
//!     ..Default::default()
//! };
//! let result = run_optimize(config, &MyDb);
//! ```

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use ogexplain_core::summary::SummaryRow;
use ogexplain_core::{analyze, parse};
use tracing::warn;

use crate::converge::{should_continue, LoopConfig, LoopDecision, MetricsSnapshot, StopReason};
use crate::mapper::{filter_rewritable, map_diagnostic, RemediationAction};
use crate::rewrite::rewrite_sql;
use crate::verify::{self, VerifyEngine, VerifyStatus};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Trait abstracting database EXPLAIN execution for testability.
pub trait ExplainExecutor {
    fn fetch_explain(&self, sql: &str, analyze: bool) -> Result<String, String>;
}

/// Configuration for the optimization loop.
#[derive(Debug, Clone)]
pub struct OptimizeConfig {
    /// SQL statement to optimize.
    pub sql: String,
    /// Path to a JSON schema file (passed to metamorphosis for context).
    pub schema_json_path: Option<String>,
    /// Directory of `.sql` DDL files (alternative to schema_json_path).
    pub sql_dir: Option<String>,
    /// Maximum iterations before forced stop.
    pub max_iterations: usize,
    /// Run EXPLAIN ANALYZE (executes the query). Requires `--i-know-the-risks`.
    pub analyze_enabled: bool,
    /// Skip semantic equivalence verification entirely.
    pub skip_verify: bool,
    /// Verification engine: "qed" or "verieql".
    pub verify_engine: String,
    /// Per-rewrite verification timeout in seconds.
    pub verify_timeout: u64,
    /// VeriEQL bound (max rows per table in counterexample search).
    pub verify_bound: usize,
    /// Enable verbose status/debug output.
    pub verbose: bool,
}

impl Default for OptimizeConfig {
    fn default() -> Self {
        Self {
            sql: String::new(),
            schema_json_path: None,
            sql_dir: None,
            max_iterations: 10,
            analyze_enabled: false,
            skip_verify: false,
            verify_engine: "qed".into(),
            verify_timeout: 60,
            verify_bound: 2,
            verbose: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug)]
#[allow(dead_code)]
struct IterationRecord {
    iteration: usize,
    rule_id: String,
    action: RemediationAction,
    snapshot_before: Option<MetricsSnapshot>,
    snapshot_after: MetricsSnapshot,
    rewritten_sql: Option<String>,
    notes: Vec<String>,
    verification: Option<verify::VerifyResult>,
}

// ---------------------------------------------------------------------------
// Schema loading
// ---------------------------------------------------------------------------

/// PK-aware table schema entry deserialized from JSON.
///
/// JSON format:
/// ```json
/// { "table": { "columns": { "col": "TYPE" }, "primary_key": ["col"] } }
/// ```
///
/// NOTE: This is a simplified schema (table→column→type). The full
/// `ogsql_parser::analyzer::schema::SchemaMap` format is not used here
/// because the metamorphosis `RewriteContext` accepts this simplified
/// format directly.
#[derive(Debug, Clone, serde::Deserialize)]
struct TableSchemaEntry {
    columns: std::collections::HashMap<String, String>,
    #[serde(default)]
    primary_key: Vec<String>,
}

/// Load a schema map from a JSON file or SQL DDL directory.
///
/// Expects PK-aware JSON format with optional `primary_key` per table.
fn load_schema(
    schema_json_path: &Option<String>,
    sql_dir: &Option<String>,
) -> Option<std::collections::HashMap<String, TableSchemaEntry>> {
    if let Some(path) = schema_json_path {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                match serde_json::from_str::<
                    std::collections::HashMap<String, TableSchemaEntry>,
                >(&content)
                {
                    Ok(map) => {
                        if !map.is_empty() {
                            return Some(map);
                        }
                        warn!("Schema JSON file '{}' contained no tables", path);
                    }
                    Err(e) => {
                        warn!("Failed to parse schema JSON from '{}': {}", path, e);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read schema file '{}': {}", path, e);
            }
        }
    }

    if let Some(dir) = sql_dir {
        warn!("SQL directory schema loading not yet implemented: '{}'", dir);
    }

    None
}

// ---------------------------------------------------------------------------
// Main optimization loop
// ---------------------------------------------------------------------------

/// Run the closed-loop optimization pipeline.
///
/// The pipeline is:
/// 1. EXPLAIN via [`ExplainExecutor`]
/// 2. Parse plan via [`ogexplain_core::parse`]
/// 3. Analyze via [`ogexplain_core::analyze`]
/// 4. Filter rewritable findings
/// 5. Map to rewrite rules
/// 6. Rewrite via metamorphosis library API
/// 7. Verify equivalence (optional)
/// 8. Converge (check continue/stop conditions)
/// 9. Repeat until convergence
///
/// Returns a text report of the optimization run.
pub fn run_optimize(config: OptimizeConfig, executor: &dyn ExplainExecutor) -> Result<String, String> {
    let loop_config = LoopConfig {
        max_iterations: config.max_iterations,
        require_equivalence_proof: false,
        auto_run_analyze: false,
        ..Default::default()
    };

    // Load schema for rewriting
    let table_entries = load_schema(&config.schema_json_path, &config.sql_dir);
    let schema: Option<std::collections::HashMap<String, std::collections::HashMap<String, String>>> = table_entries.as_ref().map(|entries| {
        entries.iter().map(|(t, e)| (t.clone(), e.columns.clone())).collect()
    });

    let mut current_sql = config.sql.clone();
    let mut prev_snapshot: Option<MetricsSnapshot> = None;
    let mut plateau_count = 0usize;
    let mut sql_history: HashSet<u64> = HashSet::new();
    sql_history.insert(hash_sql(&current_sql));

    let mut history: Vec<IterationRecord> = Vec::new();

    for iteration in 1..=loop_config.max_iterations {
        // 1. EXPLAIN
        let explain_text = executor
            .fetch_explain(&current_sql, config.analyze_enabled)
            .map_err(|e| format!("EXPLAIN failed at iteration {}: {}", iteration, e))?;

        // 2. Parse
        let plan = parse(&explain_text).map_err(|e| format!("Parse failed at iteration {}: {}", iteration, e))?;

        // 3. Analyze
        let report = analyze(&plan);
        let summary = SummaryRow::compute(&plan, &report, None);
        let curr_snapshot = MetricsSnapshot::from_summary(&summary);

        // 4. Convergence check
        if let Some(prev) = &prev_snapshot {
            let rewritable = filter_rewritable(&report.findings);
            let sql_unchanged = sql_history.contains(&hash_sql(&current_sql));
            let decision = should_continue(
                prev,
                &curr_snapshot,
                &loop_config,
                iteration,
                plateau_count,
                !rewritable.is_empty(),
                sql_unchanged,
            );
            if let LoopDecision::Stop(reason) = decision {
                return finalize(&history, reason, &current_sql, &config);
            }
            update_plateau(prev, &curr_snapshot, &loop_config, &mut plateau_count);
        }

        // 5. Filter rewritable findings
        let rewritable = filter_rewritable(&report.findings);
        if rewritable.is_empty() {
            return finalize(
                &history,
                StopReason::NoRewritableFindings,
                &current_sql,
                &config,
            );
        }

        let finding = &rewritable[0];
        let action = map_diagnostic(&finding.rule_id);

        // 6. Rewrite
        let rewritten_sql = match &action {
            RemediationAction::Rewrite { rules } => {
                match rewrite_sql(&current_sql, schema.as_ref(), rules) {
                    Ok(Some(sql)) => Some(sql),
                    Ok(None) => None,
                    Err(e) => {
                        history.push(IterationRecord {
                            iteration,
                            rule_id: finding.rule_id.clone(),
                            action,
                            snapshot_before: prev_snapshot.clone(),
                            snapshot_after: curr_snapshot.clone(),
                            rewritten_sql: None,
                            notes: vec![format!("Rewrite error: {}", e)],
                            verification: None,
                        });
                        return finalize(&history, StopReason::FixedPoint, &current_sql, &config);
                    }
                }
            }
            _ => None,
        };

        let Some(rewritten) = rewritten_sql else {
            history.push(IterationRecord {
                iteration,
                rule_id: finding.rule_id.clone(),
                action,
                snapshot_before: prev_snapshot.clone(),
                snapshot_after: curr_snapshot.clone(),
                rewritten_sql: None,
                notes: vec!["No rewrite produced".into()],
                verification: None,
            });
            break;
        };

        // 7. Verification step
        let verification: Option<verify::VerifyResult> = if config.skip_verify {
            None
        } else {
            #[cfg(feature = "verify")]
            {
                let engine: VerifyEngine = config
                    .verify_engine
                    .parse()
                    .unwrap_or(VerifyEngine::Qed);

                let result = match engine {
                    VerifyEngine::Qed => {
                        let rich_schema = build_rich_schema(table_entries.as_ref());
                        verify::verify_qed(&current_sql, &rewritten, &rich_schema, config.verify_timeout)
                    }
                    VerifyEngine::VeriEql => {
                        let rich_schema = build_rich_schema(table_entries.as_ref());
                        let tables = verify::rich_schema_to_verieql(&rich_schema);
                        let constraints = serde_json::json!({});
                        verify::verify_verieql(
                            &current_sql,
                            &rewritten,
                            &tables,
                            &constraints,
                            config.verify_bound,
                        )
                    }
                };

                match result {
                    Ok(r) => {
                        match verify::decide_verification_outcome(&r) {
                            verify::VerificationDecision::Reject { counterexample } => {
                                history.push(IterationRecord {
                                    iteration,
                                    rule_id: finding.rule_id.clone(),
                                    action: action.clone(),
                                    snapshot_before: prev_snapshot.clone(),
                                    snapshot_after: curr_snapshot.clone(),
                                    rewritten_sql: Some(rewritten.clone()),
                                    notes: vec![format!(
                                        "Verification rejected: {}",
                                        counterexample.as_deref().unwrap_or("(no counterexample)")
                                    )],
                                    verification: Some(r),
                                });
                                return finalize(
                                    &history,
                                    StopReason::VerificationFailed { counterexample },
                                    &current_sql,
                                    &config,
                                );
                            }
                            verify::VerificationDecision::Accept => Some(r),
                        }
                    }
                    Err(e) => {
                        let err_result = verify::VerifyResult {
                            engine,
                            status: verify::VerifyStatus::Unknown {
                                reason: format!("verify error: {e}"),
                            },
                            elapsed_ms: None,
                            original_sql: current_sql.clone(),
                            rewritten_sql: rewritten.clone(),
                            raw_output: None,
                        };
                        Some(err_result)
                    }
                }
            }
            #[cfg(not(feature = "verify"))]
            {
                None
            }
        };

        // Check for fixed-point after rewrite
        let rewritten_hash = hash_sql(&rewritten);
        if sql_history.contains(&rewritten_hash) {
            return finalize(&history, StopReason::FixedPoint, &current_sql, &config);
        }
        sql_history.insert(rewritten_hash);

        history.push(IterationRecord {
            iteration,
            rule_id: finding.rule_id.clone(),
            action: action.clone(),
            snapshot_before: prev_snapshot.clone(),
            snapshot_after: curr_snapshot.clone(),
            rewritten_sql: Some(rewritten.clone()),
            notes: Vec::new(),
            verification,
        });

        prev_snapshot = Some(curr_snapshot);
        current_sql = rewritten;
    }

    finalize(&history, StopReason::MaxIterations, &current_sql, &config)
}

// ---------------------------------------------------------------------------
// Output (text report)
// ---------------------------------------------------------------------------

fn finalize(
    history: &[IterationRecord],
    reason: StopReason,
    final_sql: &str,
    _config: &OptimizeConfig,
) -> Result<String, String> {
    Ok(render_report(history, &reason, final_sql))
}

fn render_report(history: &[IterationRecord], reason: &StopReason, final_sql: &str) -> String {
    let mut out = String::new();
    out.push_str("=== Optimization Report ===\n");
    out.push_str(&format!("Stop reason: {:?}\n", reason));
    out.push_str(&format!("Iterations: {}\n", history.len()));
    for record in history {
        out.push_str(&format!("\n--- Iteration {} ---\n", record.iteration));
        out.push_str(&format!(
            "Triggered by: {} ({:?})\n",
            record.rule_id, record.action
        ));
        if let Some(before) = &record.snapshot_before {
            let after = &record.snapshot_after;
            if let (Some(b), Some(a)) = (before.total_cost, after.total_cost) {
                let delta = if b > 0.0 {
                    ((a - b) / b) * 100.0
                } else {
                    0.0
                };
                out.push_str(&format!("Cost: {:.2} → {:.2} ({:+.1}%)\n", b, a, delta));
            }
            out.push_str(&format!(
                "Critical findings: {} → {}\n",
                before.critical_count, after.critical_count
            ));
        }
        // Verification status
        match &record.verification {
            Some(v) => match &v.status {
                VerifyStatus::Equivalent => {
                    out.push_str(&format!(
                        "Verification: ✓ {} Equivalent ({}ms)\n",
                        v.engine,
                        v.elapsed_ms.unwrap_or(0)
                    ));
                }
                VerifyStatus::NotEquivalent { counterexample } => {
                    out.push_str(&format!("Verification: ✗ {} NotEquivalent\n", v.engine));
                    if let Some(ce) = counterexample {
                        for line in ce.lines() {
                            out.push_str(&format!("  {}\n", line));
                        }
                    }
                }
                VerifyStatus::Unknown { reason } => {
                    out.push_str(&format!(
                        "Verification: ? {} Unknown: {}\n",
                        v.engine, reason
                    ));
                }
                VerifyStatus::Timeout { seconds } => {
                    out.push_str(&format!(
                        "Verification: ⏱ {} Timeout after {}s\n",
                        v.engine, seconds
                    ));
                }
                VerifyStatus::Skipped { reason } => {
                    out.push_str(&format!(
                        "Verification: ⏭ {} Skipped ({:?})\n",
                        v.engine, reason
                    ));
                }
            },
            None => {
                out.push_str("Verification: (not performed)\n");
            }
        }
        for note in &record.notes {
            out.push_str(&format!("Note: {}\n", note));
        }
    }
    out.push_str(&format!("\n=== Final SQL ===\n{}\n", final_sql));
    out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn update_plateau(
    prev: &MetricsSnapshot,
    curr: &MetricsSnapshot,
    config: &LoopConfig,
    plateau_count: &mut usize,
) {
    if let (Some(p), Some(c)) = (prev.total_cost, curr.total_cost) {
        if p > 0.0 && (p - c) / p < config.min_improvement_pct {
            *plateau_count += 1;
        } else {
            *plateau_count = 0;
        }
    }
}

fn hash_sql(sql: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    sql.hash(&mut hasher);
    hasher.finish()
}

/// Build a [`RichSchema`] from an optional schema map with PK constraints.
#[cfg(feature = "verify")]
fn build_rich_schema(
    schema: Option<&std::collections::HashMap<String, TableSchemaEntry>>,
) -> metamorphosis_qed::schema::RichSchema {
    use metamorphosis_qed::schema::{ColumnInfo, TableConstraints, TableInfo};
    use std::collections::HashMap;

    let Some(schema_map) = schema else {
        return metamorphosis_qed::schema::RichSchema {
            tables: HashMap::new(),
        };
    };

    let tables: HashMap<String, TableInfo> = schema_map
        .iter()
        .map(|(table_name, entry)| {
            let pk_set: std::collections::HashSet<&str> =
                entry.primary_key.iter().map(|s| s.as_str()).collect();
            let cols: Vec<ColumnInfo> = entry
                .columns
                .iter()
                .map(|(col_name, col_type)| {
                    let is_pk = pk_set.contains(col_name.as_str());
                    ColumnInfo {
                        name: col_name.clone(),
                        data_type: col_type.clone(),
                        nullable: !is_pk,
                        is_primary_key: is_pk,
                        is_unique: is_pk,
                    }
                })
                .collect();
            (
                table_name.clone(),
                TableInfo {
                    columns: cols,
                    constraints: TableConstraints {
                        primary_key: entry.primary_key.clone(),
                        ..Default::default()
                    },
                },
            )
        })
        .collect();

    metamorphosis_qed::schema::RichSchema { tables }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── test-only executor that returns a minimal plan ─────────────────

    struct MockExecutor;

    impl ExplainExecutor for MockExecutor {
        fn fetch_explain(&self, _sql: &str, _analyze: bool) -> Result<String, String> {
            Ok("Seq Scan on t  (cost=0.00..35.00 rows=100 width=4)".into())
        }
    }

    // ── hash_sql ───────────────────────────────────────────────────────

    #[test]
    fn hash_sql_distinguishes_inputs() {
        assert_ne!(hash_sql("SELECT 1"), hash_sql("SELECT 2"));
        assert_eq!(hash_sql("SELECT 1"), hash_sql("SELECT 1"));
    }

    #[test]
    fn hash_sql_whitespace_sensitive() {
        assert_ne!(hash_sql("SELECT 1"), hash_sql("SELECT  1"));
    }

    // ── update_plateau ─────────────────────────────────────────────────

    #[test]
    fn plateau_count_increments_on_small_improvement() {
        let prev = MetricsSnapshot {
            total_cost: Some(100.0),
            ..Default::default()
        };
        let curr = MetricsSnapshot {
            total_cost: Some(99.5),
            ..Default::default()
        };
        let config = LoopConfig::default();
        let mut count = 0;
        update_plateau(&prev, &curr, &config, &mut count);
        assert_eq!(count, 1);
    }

    #[test]
    fn plateau_count_resets_on_real_improvement() {
        let prev = MetricsSnapshot {
            total_cost: Some(100.0),
            ..Default::default()
        };
        let curr = MetricsSnapshot {
            total_cost: Some(50.0),
            ..Default::default()
        };
        let config = LoopConfig::default();
        let mut count = 5;
        update_plateau(&prev, &curr, &config, &mut count);
        assert_eq!(count, 0);
    }

    // ── render_report ──────────────────────────────────────────────────

    #[test]
    fn render_report_handles_empty_history() {
        let report = render_report(&[], &StopReason::Success, "SELECT 1");
        assert!(report.contains("Iterations: 0"));
        assert!(report.contains("SELECT 1"));
    }

    #[test]
    fn render_report_includes_verification_field() {
        let verified_record = IterationRecord {
            iteration: 1,
            rule_id: "SUBQ-001".into(),
            action: RemediationAction::Log,
            snapshot_before: None,
            snapshot_after: MetricsSnapshot::default(),
            rewritten_sql: Some("SELECT 1".into()),
            notes: Vec::new(),
            verification: Some(verify::VerifyResult {
                engine: VerifyEngine::Qed,
                status: VerifyStatus::Equivalent,
                elapsed_ms: Some(22),
                original_sql: "SELECT 1".into(),
                rewritten_sql: "SELECT 1".into(),
                raw_output: None,
            }),
        };

        let report = render_report(&[verified_record], &StopReason::Success, "SELECT 1");
        assert!(
            report.contains("Verification: ✓ qed Equivalent (22ms)"),
            "report must include verification line; got:\n{report}"
        );

        let unverified_record = IterationRecord {
            iteration: 2,
            rule_id: "SUBQ-001".into(),
            action: RemediationAction::Log,
            snapshot_before: None,
            snapshot_after: MetricsSnapshot::default(),
            rewritten_sql: None,
            notes: Vec::new(),
            verification: None,
        };
        let report2 = render_report(&[unverified_record], &StopReason::Success, "SELECT 1");
        assert!(
            report2.contains("Verification: (not performed)"),
            "report must show not-performed for None; got:\n{report2}"
        );
    }

    #[test]
    fn render_report_shows_stop_reason() {
        let report = render_report(&[], &StopReason::Success, "SELECT 1");
        assert!(report.contains("Stop reason: Success"));

        let report2 = render_report(&[], &StopReason::MaxIterations, "SELECT 1");
        assert!(report2.contains("Stop reason: MaxIterations"));

        let report3 = render_report(&[], &StopReason::Regression, "SELECT 1");
        assert!(report3.contains("Stop reason: Regression"));
    }

    #[test]
    fn render_report_with_cost_delta() {
        let record = IterationRecord {
            iteration: 1,
            rule_id: "TYPE-001".into(),
            action: RemediationAction::Log,
            snapshot_before: Some(MetricsSnapshot {
                total_cost: Some(100.0),
                critical_count: 2,
                ..Default::default()
            }),
            snapshot_after: MetricsSnapshot {
                total_cost: Some(80.0),
                critical_count: 1,
                ..Default::default()
            },
            rewritten_sql: Some("SELECT 1".into()),
            notes: Vec::new(),
            verification: None,
        };
        let report = render_report(&[record], &StopReason::Success, "SELECT 1");
        assert!(report.contains("Cost: 100.00 → 80.00 (-20.0%)"));
        assert!(report.contains("Critical findings: 2 → 1"));
    }

    // ── build_rich_schema ──────────────────────────────────────────────

    #[cfg(feature = "verify")]
    #[test]
    fn build_rich_schema_none_returns_empty() {
        let schema = build_rich_schema(None);
        assert!(schema.tables.is_empty());
    }

    #[cfg(feature = "verify")]
    #[test]
    fn build_rich_schema_with_tables() {
        use std::collections::HashMap;

        let mut entries: HashMap<String, TableSchemaEntry> = HashMap::new();
        let mut cols = HashMap::new();
        cols.insert("id".into(), "INTEGER".into());
        entries.insert(
            "users".into(),
            TableSchemaEntry {
                columns: cols,
                primary_key: vec![],
            },
        );

        let schema = build_rich_schema(Some(&entries));
        assert_eq!(schema.tables.len(), 1);
        let table_info = schema.tables.get("users").unwrap();
        assert_eq!(table_info.columns.len(), 1);
        assert_eq!(table_info.columns[0].name, "id");
        assert_eq!(table_info.columns[0].data_type, "INTEGER");
    }

    // ── load_schema ────────────────────────────────────────────────────

    #[test]
    fn load_schema_both_none_returns_none() {
        let result = load_schema(&None, &None);
        assert!(result.is_none());
    }

    #[test]
    fn load_schema_nonexistent_file_returns_none() {
        let result = load_schema(
            &Some("/nonexistent/schema.json".into()),
            &None,
        );
        // Should not panic, just warn and return None
        assert!(result.is_none());
    }

    // ── Decide-verification integration ─────────────────────────────────

    #[test]
    fn verification_reject_triggers_verification_failed_stop() {
        // This is an integration test for the decision mapping:
        // NotEquivalent → Reject → VerificationFailed stop
        let result = verify::VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::NotEquivalent {
                counterexample: Some("id mismatch".into()),
            },
            elapsed_ms: Some(5),
            original_sql: "SELECT * FROM t".into(),
            rewritten_sql: "SELECT id FROM t".into(),
            raw_output: None,
        };

        let decision = verify::decide_verification_outcome(&result);
        assert_eq!(
            decision,
            verify::VerificationDecision::Reject {
                counterexample: Some("id mismatch".into())
            }
        );
    }

    #[test]
    fn verification_equivalent_accepts() {
        let result = verify::VerifyResult {
            engine: VerifyEngine::Qed,
            status: VerifyStatus::Equivalent,
            elapsed_ms: Some(10),
            original_sql: "SELECT * FROM t".into(),
            rewritten_sql: "SELECT id FROM t".into(),
            raw_output: None,
        };

        let decision = verify::decide_verification_outcome(&result);
        assert_eq!(decision, verify::VerificationDecision::Accept);
    }

    // ── Convergence integration (decisions that orchestrator can emit) ──

    #[test]
    fn converge_continue_on_progress() {
        let prev = MetricsSnapshot {
            total_cost: Some(100.0),
            critical_count: 2,
            ..Default::default()
        };
        let curr = MetricsSnapshot {
            total_cost: Some(80.0),
            critical_count: 1,
            ..Default::default()
        };
        let decision = should_continue(
            &prev, &curr, &LoopConfig::default(), 1, 0, true, false,
        );
        assert!(matches!(decision, LoopDecision::Continue));
    }

    #[test]
    fn converge_success_on_zero_critical() {
        let prev = MetricsSnapshot {
            total_cost: Some(100.0),
            critical_count: 2,
            ..Default::default()
        };
        let curr = MetricsSnapshot {
            total_cost: Some(80.0),
            critical_count: 0,
            ..Default::default()
        };
        let decision = should_continue(
            &prev, &curr, &LoopConfig::default(), 1, 0, true, false,
        );
        assert!(matches!(decision, LoopDecision::Stop(StopReason::Success)));
    }

    #[test]
    fn converge_regression_on_cost_increase() {
        let prev = MetricsSnapshot {
            total_cost: Some(100.0),
            critical_count: 2,
            ..Default::default()
        };
        let curr = MetricsSnapshot {
            total_cost: Some(120.0),
            critical_count: 2,
            ..Default::default()
        };
        let decision = should_continue(
            &prev, &curr, &LoopConfig::default(), 1, 0, true, false,
        );
        assert!(matches!(decision, LoopDecision::Stop(StopReason::Regression)));
    }

    #[test]
    fn converge_max_iterations() {
        let prev = MetricsSnapshot {
            total_cost: Some(100.0),
            critical_count: 2,
            ..Default::default()
        };
        let curr = MetricsSnapshot {
            total_cost: Some(95.0),
            critical_count: 2,
            ..Default::default()
        };
        let cfg = LoopConfig {
            max_iterations: 5,
            ..Default::default()
        };
        let decision = should_continue(&prev, &curr, &cfg, 5, 0, true, false);
        assert!(matches!(
            decision,
            LoopDecision::Stop(StopReason::MaxIterations)
        ));
    }

    #[test]
    fn converge_fixed_point() {
        let prev = MetricsSnapshot {
            total_cost: Some(100.0),
            critical_count: 2,
            ..Default::default()
        };
        let curr = MetricsSnapshot {
            total_cost: Some(100.0),
            critical_count: 2,
            ..Default::default()
        };
        let decision = should_continue(
            &prev, &curr, &LoopConfig::default(), 1, 0, true, true,
        );
        assert!(matches!(
            decision,
            LoopDecision::Stop(StopReason::FixedPoint)
        ));
    }
}
