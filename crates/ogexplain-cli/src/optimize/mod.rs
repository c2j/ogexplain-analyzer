//! `ogexplain optimize` subcommand — closed-loop SQL optimization orchestrator.
//!
//! Pipeline: EXPLAIN → diagnose → map → metamorphosis rewrite → re-EXPLAIN →
//! converge. See `.sisyphus/plans/2026-06-28-closed-loop-pilot.md` Phase 4.

pub mod mapper;

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result};

use ogexplain_core::convergence::{self, LoopConfig, LoopDecision, MetricsSnapshot, StopReason};
use ogexplain_core::summary::SummaryRow;
use ogexplain_core::{analyze, parse, DiagnosticHint};

use mapper::{filter_rewritable, finding_to_hint, map_diagnostic, RemediationAction};

pub struct OptimizeArgs {
    pub sql: String,
    pub config_path: Option<std::path::PathBuf>,
    pub name: Option<String>,
    pub schema_path: Option<String>,
    pub metamorphosis_path: String,
    pub max_iterations: usize,
    pub analyze_enabled: bool,
    pub skip_stats_check: bool,
    pub format: String,
    pub output: Option<String>,
}

#[derive(Debug)]
struct IterationRecord {
    iteration: usize,
    rule_id: String,
    action: RemediationAction,
    snapshot_before: Option<MetricsSnapshot>,
    snapshot_after: MetricsSnapshot,
    rewritten_sql: Option<String>,
    notes: Vec<String>,
}

pub fn run_optimize(args: OptimizeArgs) -> Result<()> {
    check_metamorphosis_available(&args.metamorphosis_path)?;

    if !args.skip_stats_check {
        eprintln!("⚠️  Warning: Phase 0 stats check not yet implemented.");
        eprintln!("   Stale statistics may produce misleading diagnostics.");
        eprintln!(
            "   Run ANALYZE manually on involved tables, or pass --skip-stats-check to silence."
        );
    }

    let loop_config = LoopConfig {
        max_iterations: args.max_iterations,
        require_equivalence_proof: false,
        auto_run_analyze: false,
        ..Default::default()
    };

    let mut current_sql = args.sql.clone();
    let mut prev_snapshot: Option<MetricsSnapshot> = None;
    let mut plateau_count = 0usize;
    let mut sql_history: HashSet<u64> = HashSet::new();
    sql_history.insert(hash_sql(&current_sql));

    let mut history: Vec<IterationRecord> = Vec::new();

    for iteration in 1..=loop_config.max_iterations {
        let explain_text = crate::db::fetch_explain(
            args.config_path.as_deref(),
            args.name.as_deref(),
            &current_sql,
            args.analyze_enabled,
        )
        .with_context(|| format!("EXPLAIN failed at iteration {}", iteration))?;

        let plan = parse(&explain_text).context("parse EXPLAIN failed")?;
        let report = analyze(&plan);
        let summary = SummaryRow::compute(&plan, &report, None);
        let curr_snapshot = MetricsSnapshot::from_summary(&summary);

        if let Some(prev) = &prev_snapshot {
            let rewritable = filter_rewritable(&report.findings);
            let sql_unchanged = sql_history.contains(&hash_sql(&current_sql));
            let decision = convergence::should_continue(
                prev,
                &curr_snapshot,
                &loop_config,
                iteration,
                plateau_count,
                !rewritable.is_empty(),
                sql_unchanged,
            );
            if let LoopDecision::Stop(reason) = decision {
                return finalize(&history, reason, &current_sql, &args);
            }
            update_plateau(prev, &curr_snapshot, &loop_config, &mut plateau_count);
        }

        let rewritable = filter_rewritable(&report.findings);
        if rewritable.is_empty() {
            return finalize(
                &history,
                StopReason::NoRewritableFindings,
                &current_sql,
                &args,
            );
        }
        let finding = &rewritable[0];
        let action = map_diagnostic(&finding.rule_id);

        let rewritten_sql = match &action {
            RemediationAction::Rewrite { rules } => {
                let hint = finding_to_hint(finding);
                Some(call_metamorphosis_rewrite(
                    &current_sql,
                    rules,
                    args.schema_path.as_deref(),
                    hint.as_ref(),
                    &args.metamorphosis_path,
                )?)
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
            });
            break;
        };

        let rewritten_hash = hash_sql(&rewritten);
        if sql_history.contains(&rewritten_hash) {
            return finalize(&history, StopReason::FixedPoint, &current_sql, &args);
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
        });

        prev_snapshot = Some(curr_snapshot);
        current_sql = rewritten;
    }

    finalize(&history, StopReason::MaxIterations, &current_sql, &args)
}

fn finalize(
    history: &[IterationRecord],
    reason: StopReason,
    final_sql: &str,
    args: &OptimizeArgs,
) -> Result<()> {
    match args.format.as_str() {
        "json" => {
            let iterations: Vec<_> = history
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "iteration": r.iteration,
                        "rule_id": r.rule_id,
                        "action": format!("{:?}", r.action),
                        "cost_before": r.snapshot_before.as_ref().and_then(|s| s.total_cost),
                        "cost_after": r.snapshot_after.total_cost,
                        "critical_before": r.snapshot_before.as_ref().map(|s| s.critical_count),
                        "critical_after": r.snapshot_after.critical_count,
                        "rewritten_sql": r.rewritten_sql,
                    })
                })
                .collect();
            let json = serde_json::json!({
                "iterations_count": history.len(),
                "stop_reason": format!("{:?}", reason),
                "final_sql": final_sql,
                "iterations": iterations,
            });
            let pretty = serde_json::to_string_pretty(&json).unwrap_or_default();
            if let Some(path) = &args.output {
                std::fs::write(path, &pretty)?;
            } else {
                println!("{}", pretty);
            }
        }
        _ => {
            let report = render_report(history, &reason, final_sql);
            if let Some(path) = &args.output {
                std::fs::write(path, &report)?;
            } else {
                print!("{}", report);
            }
        }
    }
    Ok(())
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
                let delta = if b > 0.0 { ((a - b) / b) * 100.0 } else { 0.0 };
                out.push_str(&format!("Cost: {:.2} → {:.2} ({:+.1}%)\n", b, a, delta));
            }
            out.push_str(&format!(
                "Critical findings: {} → {}\n",
                before.critical_count, after.critical_count
            ));
        }
        for note in &record.notes {
            out.push_str(&format!("Note: {}\n", note));
        }
    }
    out.push_str(&format!("\n=== Final SQL ===\n{}\n", final_sql));
    out
}

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

fn check_metamorphosis_available(path: &str) -> Result<()> {
    match std::process::Command::new(path).arg("--version").output() {
        Ok(output) => {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                eprintln!("Using metamorphosis: {} ({})", path, version.trim());
                Ok(())
            } else {
                eprintln!("Using metamorphosis: {} (version check skipped)", path);
                Ok(())
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(anyhow::anyhow!(
            "metamorphosis binary '{}' not found in PATH.\n\
             Install: git clone https://github.com/c2j/metamorphosis && cd metamorphosis && cargo build --release\n\
             Then put `target/release/metamorphosis` in PATH or pass --metamorphosis <path>",
            path
        )),
        Err(e) => Err(anyhow::anyhow!(
            "Failed to spawn metamorphosis '{}': {}",
            path,
            e
        )),
    }
}

fn call_metamorphosis_rewrite(
    sql: &str,
    rules: &[&str],
    schema_path: Option<&str>,
    hint: Option<&DiagnosticHint>,
    metamorphosis_path: &str,
) -> Result<String> {
    use std::fs;
    use std::process::Command;

    let input_path = std::env::temp_dir().join("ogexplain_optimize_input.sql");
    fs::write(&input_path, sql)?;

    let mut cmd = Command::new(metamorphosis_path);
    cmd.arg("rewrite")
        .arg("--file")
        .arg(&input_path)
        .arg("--rules")
        .arg(rules.join(","))
        .arg("--input-format")
        .arg("sql");
    if let Some(schema) = schema_path {
        cmd.arg("--schema").arg(schema);
    }

    if let Some(hint) = hint {
        let hint_path = std::env::temp_dir().join(format!(
            "ogexplain_hint_{}.json",
            hint.rule_id
        ));
        let hint_json = serde_json::to_string(hint)?;
        fs::write(&hint_path, &hint_json)?;
        cmd.arg("--diagnostic-hints").arg(&hint_path);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to spawn {}", metamorphosis_path))?;
    if !output.status.success() {
        anyhow::bail!(
            "metamorphosis rewrite failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let sql_lines: Vec<&str> = stdout
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("--") && !trimmed.starts_with('#') && !trimmed.is_empty()
        })
        .collect();
    let cleaned = sql_lines.join("\n").trim().to_string();
    if cleaned.is_empty() {
        anyhow::bail!("metamorphosis rewrite produced empty output");
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_sql_distinguishes_inputs() {
        assert_ne!(hash_sql("SELECT 1"), hash_sql("SELECT 2"));
        assert_eq!(hash_sql("SELECT 1"), hash_sql("SELECT 1"));
    }

    #[test]
    fn hash_sql_whitespace_sensitive() {
        // whitespace differences should produce different hashes (conservative)
        assert_ne!(hash_sql("SELECT 1"), hash_sql("SELECT  1"));
    }

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

    #[test]
    fn render_report_handles_empty_history() {
        let report = render_report(&[], &StopReason::Success, "SELECT 1");
        assert!(report.contains("Iterations: 0"));
        assert!(report.contains("SELECT 1"));
    }
}
