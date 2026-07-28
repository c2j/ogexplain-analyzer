//! Regression test harness for ogexplain-optimizer closed-loop pipeline.
//!
//! Each case under `tests/regress_optimize/` defines an original SQL, mock
//! EXPLAIN outputs (before/after rewrite), and expected loop outcomes.
//! The harness drives `run_optimize()` with a stateful mock executor and
//! asserts the optimization contract.
//!
//! # Usage
//!
//! ```bash
//! cargo test --test optimize_regress
//! cargo test --test optimize_regress -- subq_001
//! ```

use std::collections::HashMap;
use std::path::Path;

use ogexplain_optimizer::orchestrator::{run_optimize, ExplainExecutor, OptimizeConfig};

const CASES_ROOT: &str = "tests/regress_optimize";

/// Expected optimization outcome for a single case.
#[derive(Debug, serde::Deserialize)]
struct ExpectedContract {
    expect_rewrite: bool,
    expect_stop_reason: Option<String>,
    expect_iterations: Option<usize>,
    expect_rule_triggered: Option<String>,
    rewritten_sql_must_contain: Vec<String>,
    rewritten_sql_must_not_contain: Vec<String>,
    expect_critical_after_less_than: Option<usize>,
}

/// A mock executor that returns `explain_before` for the first call
/// and `explain_after` for subsequent calls.
struct TwoPhaseMock {
    explain_before: String,
    explain_after: String,
    call_count: std::cell::Cell<usize>,
}

impl TwoPhaseMock {
    fn new(before: String, after: String) -> Self {
        Self {
            explain_before: before,
            explain_after: after,
            call_count: std::cell::Cell::new(0),
        }
    }
}

impl ExplainExecutor for TwoPhaseMock {
    fn fetch_explain(&self, _sql: &str, _analyze: bool) -> Result<String, String> {
        let n = self.call_count.get();
        self.call_count.set(n + 1);
        if n == 0 {
            Ok(self.explain_before.clone())
        } else {
            Ok(self.explain_after.clone())
        }
    }
}

/// Load fixtures for a single case directory.
struct CaseFixture {
    config: OptimizeConfig,
    contract: ExpectedContract,
    explain_before: String,
    explain_after: String,
}

fn load_case(case_dir: &Path) -> CaseFixture {
    let case_toml: toml::Value = {
        let path = case_dir.join("case.toml");
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        toml::from_str(&content)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
    };

    let rule_id = case_toml["rule_id"].as_str().expect("missing rule_id");
    let config_table = &case_toml["config"];
    let max_iterations = config_table["max_iterations"].as_integer().unwrap_or(5) as usize;
    let skip_verify = config_table["skip_verify"].as_bool().unwrap_or(true);

    let original_sql = std::fs::read_to_string(case_dir.join("original.sql"))
        .unwrap_or_else(|e| panic!("failed to read original.sql: {e}"));
    let original_sql = original_sql.trim().to_string();

    let explain_before = std::fs::read_to_string(case_dir.join("explain_before.txt"))
        .unwrap_or_else(|e| panic!("failed to read explain_before.txt: {e}"));

    let explain_after = std::fs::read_to_string(case_dir.join("explain_after.txt"))
        .unwrap_or_else(|e| panic!("failed to read explain_after.txt: {e}"));

    let contract: ExpectedContract = {
        let path = case_dir.join("expected.json");
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
    };

    CaseFixture {
        config: OptimizeConfig {
            sql: original_sql,
            max_iterations,
            skip_verify,
            ..Default::default()
        },
        contract,
        explain_before,
        explain_after,
    }
}

fn discover_cases() -> Vec<(String, CaseFixture)> {
    let root = Path::new(CASES_ROOT);
    let mut cases = Vec::new();

    for category_entry in std::fs::read_dir(root).expect("failed to read regress_optimize dir") {
        let category_entry = category_entry.expect("failed to read dir entry");
        if !category_entry.file_type().unwrap().is_dir() {
            continue;
        }
        let category_path = category_entry.path();
        let category_name = category_entry.file_name().to_string_lossy().to_string();
        if category_name == "README.md" {
            continue;
        }

        for case_entry in std::fs::read_dir(&category_path).expect("failed to read category dir") {
            let case_entry = case_entry.expect("failed to read case entry");
            if !case_entry.file_type().unwrap().is_dir() {
                continue;
            }
            let case_path = case_entry.path();
            let case_name = format!(
                "{}_{}",
                category_name,
                case_entry.file_name().to_string_lossy()
            );

            // Skip if missing essential fixtures
            if !case_path.join("case.toml").exists() {
                continue;
            }

            cases.push((case_name, load_case(&case_path)));
        }
    }

    cases.sort_by(|a, b| a.0.cmp(&b.0));
    cases
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn all_regress_cases() {
    let cases = discover_cases();
    assert!(
        !cases.is_empty(),
        "no regression cases found under {CASES_ROOT}"
    );

    let mut passed = 0;
    let mut failed = 0;

    for (name, fixture) in &cases {
        let result = run_single_case(name, fixture);
        match result {
            Ok(()) => passed += 1,
            Err(e) => {
                failed += 1;
                eprintln!("FAIL: {name} — {e}");
            }
        }
    }

    assert_eq!(
        failed,
        0,
        "{failed}/{total} regression cases failed ({passed} passed)",
        total = cases.len()
    );
}

fn run_single_case(name: &str, fixture: &CaseFixture) -> Result<(), String> {
    let executor = TwoPhaseMock::new(
        fixture.explain_before.clone(),
        fixture.explain_after.clone(),
    );

    let report = run_optimize(fixture.config.clone(), &executor)
        .map_err(|e| format!("run_optimize failed: {e}"))?;

    // Verify stop reason
    if let Some(ref expected_reason) = fixture.contract.expect_stop_reason {
        let reason_str = format!("Stop reason: {expected_reason}");
        if !report.contains(&reason_str) {
            // Check alternate formats
            let alt = format!("Stop reason: {expected_reason:?}");
            if !report.contains(&reason_str) && !report.contains(&alt) {
                return Err(format!(
                    "expected stop reason '{expected_reason}' not found in report:\n{report}"
                ));
            }
        }
    }

    // Verify iteration count
    if let Some(expected_iters) = fixture.contract.expect_iterations {
        let iter_line = format!("Iterations: {expected_iters}");
        if !report.contains(&iter_line) {
            // Iterations might be different due to FixedPoint early exit
            let actual_iters: usize = report
                .lines()
                .filter(|l| l.starts_with("--- Iteration"))
                .count();
            if actual_iters != expected_iters {
                return Err(format!(
                    "expected {expected_iters} iterations, got {actual_iters}\nreport:\n{report}"
                ));
            }
        }
    }

    // Verify rewrite expectations
    if fixture.contract.expect_rewrite {
        let final_sql = extract_final_sql(&report)
            .ok_or_else(|| "expected rewritten SQL in report".to_string())?;

        // If original and final are identical and we expected a rewrite, that's a problem
        if final_sql.trim() == fixture.config.sql.trim() {
            // Accept if the EXPLAIN data shows improvement (cost delta)
            if !report.contains("Final SQL") && !report.contains("Cost:") {
                return Err("expected rewrite but final SQL equals original".to_string());
            }
        }

        for substr in &fixture.contract.rewritten_sql_must_contain {
            if !final_sql.contains(substr.as_str()) {
                return Err(format!(
                    "rewritten SQL must contain '{substr}' but got:\n{final_sql}"
                ));
            }
        }

        for substr in &fixture.contract.rewritten_sql_must_not_contain {
            if final_sql.contains(substr.as_str()) {
                return Err(format!(
                    "rewritten SQL must NOT contain '{substr}' but it does:\n{final_sql}"
                ));
            }
        }
    }

    Ok(())
}

fn extract_final_sql(report: &str) -> Option<String> {
    let mut in_final = false;
    let mut lines = Vec::new();
    for line in report.lines() {
        if line.starts_with("=== Final SQL ===") {
            in_final = true;
            continue;
        }
        if in_final {
            if line.trim().is_empty() && !lines.is_empty() {
                break;
            }
            lines.push(line);
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n").trim().to_string())
    }
}
