# Structured Summary Table Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a one-line-per-SQL structured summary table to the CLI output, showing SQL complexity features + EXPLAIN plan metrics + diagnostic signals in a fixed-column format.

**Architecture:** A new `summary` module in `ogexplain-core` computes a `SummaryRow` struct from `(ExplainPlan, DiagnosticReport, Option<ComplexityReport>)`. The CLI renders the table. Extensibility via the `SummaryRow` struct — new columns are added as fields, rendering is a separate concern.

**Tech Stack:** Rust, ogexplain-core (model + analyzer), ogsql-complexity (ComplexityReport), colored (CLI rendering)

---

## Target Output

```
#  SQL Preview                  Tbl  Join  SubQ  Score  Level     Cost      Time     Rows   EstΔ    Spill     Mem    C/W/I
1  SELECT u.id, u.name, o.t…     2     1     0    8.0  Simple    63.85    0.2ms       10   1.0x      --      --    0/0/0
2  SELECT d.name, COUNT(*)…      2     1     0   10.5  Simple    45.67    0.3ms        5   0.5x      --      --    0/0/0
3  WITH cte AS (...) SELEC…      5     4     3   42.0  Complex  275.35   55.8ms    50000  1000x  5840kB    8MB    2/1/0
```

## Column Definitions

| Col     | Width | Source             | Field / Computation                                |
|---------|-------|--------------------|-----------------------------------------------------|
| #       | 2     | CLI loop           | block index                                         |
| SQL     | 30    | ComplexityReport   | `statements[0].sql_text` truncated                  |
| Tbl     | 3     | ComplexityMetrics  | `table_count`                                       |
| Join    | 4     | ComplexityMetrics  | `join_count`                                        |
| SubQ    | 4     | ComplexityMetrics  | `subquery_count`                                    |
| Score   | 5     | ComplexityReport   | `overall_score`                                     |
| Level   | 8     | ComplexityReport   | `overall_level.label()`                             |
| Cost    | 8     | PlanNode           | `root.estimated.total_cost`                         |
| Time    | 8     | PlanSummary/PlanNode | `summary.total_runtime_ms` or `root.actual.total_time_ms` |
| Rows    | 7     | PlanNode           | `root.actual.rows`                                  |
| EstΔ    | 6     | Computed           | `worst_estimation_ratio()` — max(actual/est) all nodes |
| Spill   | 8     | Computed           | `total_spill_kb()` from findings + structured_props |
| Mem     | 6     | PlanSummary        | `peak_memory_kb` formatted                          |
| C/W/I   | 5     | DiagnosticReport   | findings grouped by severity count                  |
| Push    | 4     | Computed           | `pushdown_status()` — Streaming node presence      |

---

## Task 1: Add `summary` module to ogexplain-core

**Files:**
- Create: `crates/ogexplain-core/src/summary.rs`
- Modify: `crates/ogexplain-core/src/lib.rs`

**Step 1: Write the failing test**

Create test file `crates/ogexplain-core/tests/summary_tests.rs`:

```rust
use ogexplain_core::summary::SummaryRow;

#[test]
fn summary_row_from_simple_plan() {
    let input = "\
Seq Scan on t1  (cost=0.00..12.00 rows=100 width=4) (actual time=0.015..0.052 rows=100 loops=1)
  Filter: (status = 'active')
  Rows Removed by Filter: 50
Total runtime: 0.089 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert_eq!(row.tables, 0);
    assert_eq!(row.joins, 0);
    assert_eq!(row.subqueries, 0);
    assert!(row.score.is_none());
    assert!(row.total_cost > 0.0);
    assert!(row.total_time_ms > 0.0);
    assert_eq!(row.actual_rows, Some(100.0));
    assert_eq!(row.critical_count, 0);
    assert_eq!(row.warning_count, 0);
}

#[test]
fn summary_row_with_complexity() {
    let input = "\
SELECT u.id FROM users u JOIN orders o ON u.id = o.user_id WHERE u.age > 18;
                              QUERY PLAN
----------------------------------------------------------------------
 Hash Join  (cost=25.38..63.85 rows=200 width=16) (actual time=0.082..0.198 rows=200 loops=1)
   Hash Cond: (o.customer_id = c.id)
   ->  Seq Scan on orders o  (cost=0.00..18.50 rows=850 width=12) (actual time=0.008..0.042 rows=850 loops=1)
   ->  Hash  (cost=1.10..1.10 rows=10 width=4) (actual time=0.008..0.008 rows=10 loops=1)
         ->  Seq Scan on users u  (cost=0.00..1.00 rows=10 width=4) (actual time=0.005..0.005 rows=10 loops=1)
(5 rows)
Total runtime: 0.350 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let sql_text = "SELECT u.id FROM users u JOIN orders o ON u.id = o.user_id WHERE u.age > 18";
    let complexity = ogsql_complexity::analyze(sql_text).unwrap();
    let row = SummaryRow::compute(&plan, &diag, Some(&complexity));

    assert_eq!(row.tables, 2);
    assert_eq!(row.joins, 1);
    assert!(row.score.unwrap() > 0.0);
    assert!(row.total_time_ms > 0.0);
    assert_eq!(row.actual_rows, Some(200.0));
}

#[test]
fn summary_row_estimation_ratio() {
    let input = "\
Sort  (cost=263.85..275.35 rows=5000 width=48) (actual time=48.123..52.456 rows=50000 loops=1)
  Sort Key: l.created_at
  ->  Seq Scan on line_items  (cost=0.00..98.50 rows=50000 width=24) (actual time=0.015..12.345 rows=500000 loops=1)
Total runtime: 55.789 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    // Root: est 5000 vs actual 50000 = 10x
    // SeqScan: est 50000 vs actual 500000 = 10x
    // Worst = 10x
    let ratio = row.worst_est_ratio.unwrap();
    assert!(ratio >= 9.0 && ratio <= 11.0, "expected ~10x, got {}", ratio);
}

#[test]
fn summary_row_spill_detection() {
    let input = "\
Sort  (cost=63.85..66.35 rows=1000 width=44) (actual time=5.432..5.876 rows=1000 loops=1)
  Sort Key: created_at
  Sort Method: external merge  Disk: 48kB
Total runtime: 6.200 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert!(row.spill_kb.unwrap() > 0.0, "expected spill > 0, got {:?}", row.spill_kb);
}

#[test]
fn summary_row_pushdown_status() {
    let input = "\
Streaming(type: GATHER)  (cost=12.34..45.67 rows=500 width=28) (actual time=1.234..2.567 rows=500 loops=1)
  Node/s: All datanodes
  ->  Seq Scan on products  (cost=0.00..15.20 rows=500 width=28) (actual time=0.045..0.234 rows=500 loops=1)
Total runtime: 3.000 ms";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert_eq!(row.pushdown, ogexplain_core::summary::PushdownStatus::NotPushed);
}

#[test]
fn summary_row_finding_counts() {
    let input = "\
Sort  (cost=263.85..275.35 rows=5000 width=48) (actual time=48.123..52.456 rows=50000 loops=1)
  Sort Key: l.created_at
  Sort Method: external merge  Disk: 5840kB
  ->  Seq Scan on line_items  (cost=0.00..98.50 rows=50000 width=24) (actual time=0.015..12.345 rows=500000 loops=1)
        Filter: (created_at > '2024-01-01'::timestamp without time zone)
        Rows Removed by Filter: 1000000
Total runtime: 55.789 ms
Peak memory: 8192 kB";
    let plan = ogexplain_core::parse(input).unwrap();
    let diag = ogexplain_core::analyze(&plan);
    let row = SummaryRow::compute(&plan, &diag, None);
    assert!(row.critical_count > 0, "expected critical findings");
    assert!(row.warning_count > 0, "expected warning findings");
    assert!(row.peak_memory_kb.unwrap() > 0.0, "expected peak memory");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ogexplain-core --test summary_tests`
Expected: FAIL — module `summary` does not exist

**Step 3: Write SummaryRow struct + compute implementation**

Create `crates/ogexplain-core/src/summary.rs`:

```rust
use serde::Serialize;

use crate::analyzer::report::{DiagnosticReport, Severity};
use crate::model::{ExplainPlan, PlanNode};

/// Pushdown status for distributed query execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PushdownStatus {
    Pushed,
    NotPushed,
    Local,
}

/// Computed fields derived from plan tree — not stored in model directly.
#[derive(Debug, Clone, Serialize)]
pub struct SummaryRow {
    // SQL complexity features (None if no SQL extracted)
    pub sql_preview: Option<String>,
    pub tables: usize,
    pub joins: usize,
    pub subqueries: usize,
    pub score: Option<f64>,
    pub level: Option<String>,

    // Plan metrics
    pub root_op: String,
    pub total_cost: f64,
    pub total_time_ms: f64,
    pub actual_rows: Option<f64>,
    pub plan_depth: usize,
    pub node_count: usize,

    // Computed diagnostics
    pub worst_est_ratio: Option<f64>,
    pub spill_kb: Option<f64>,
    pub peak_memory_kb: Option<f64>,
    pub pushdown: PushdownStatus,

    // Finding counts
    pub critical_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
}

impl SummaryRow {
    pub fn compute(
        plan: &ExplainPlan,
        diag: &DiagnosticReport,
        complexity: Option<&ogsql_complexity::ComplexityReport>,
    ) -> Self {
        let root = &plan.root;
        let sql_preview = complexity.and_then(|c| {
            c.statements.first().map(|s| {
                let text: String = s.sql_text.lines().take(1).collect();
                if text.len() > 30 { format!("{}…", &text[..29]) } else { text }
            })
        });

        let (tables, joins, subqueries) = complexity
            .map(|c| {
                let m = &c.statements.first().map(|s| &s.metrics);
                match m {
                    Some(m) => (m.table_count, m.join_count, m.subquery_count),
                    None => (0, 0, 0),
                }
            })
            .unwrap_or((0, 0, 0));

        let (score, level) = complexity
            .map(|c| (Some(c.overall_score), Some(c.overall_level.label().to_string())))
            .unwrap_or((None, None));

        let root_op = format!("{}", root.node_type);
        let total_cost = root.estimated.as_ref().map(|e| e.total_cost).unwrap_or(0.0);

        let total_time_ms = plan.summary.as_ref()
            .and_then(|s| s.total_runtime_ms)
            .unwrap_or_else(|| root.actual.as_ref().map(|a| a.total_time_ms).unwrap_or(0.0));

        let actual_rows = root.actual.as_ref().map(|a| a.rows);

        let (worst_est_ratio, spill_kb) = compute_tree_metrics(root);

        let peak_memory_kb = plan.summary.as_ref()
            .and_then(|s| s.peak_memory_kb.map(|v| v as f64))
            .or(spill_kb) // fallback: if no summary, try node peak
            .or_else(|| find_peak_memory(root));

        let pushdown = compute_pushdown_status(root);

        let (critical_count, warning_count, info_count) = diag.findings.iter().fold(
            (0usize, 0usize, 0usize),
            |(c, w, i), f| match f.severity {
                Severity::Critical => (c + 1, w, i),
                Severity::Warning => (c, w + 1, i),
                Severity::Info => (c, w, i + 1),
            },
        );

        Self {
            sql_preview,
            tables,
            joins,
            subqueries,
            score,
            level,
            root_op,
            total_cost,
            total_time_ms,
            actual_rows,
            plan_depth: diag.stats.max_depth,
            node_count: diag.stats.total_nodes,
            worst_est_ratio,
            spill_kb,
            peak_memory_kb,
            pushdown,
            critical_count,
            warning_count,
            info_count,
        }
    }
}

fn compute_tree_metrics(node: &PlanNode) -> (Option<f64>, Option<f64>) {
    let mut worst_ratio: Option<f64> = None;
    let mut total_spill: Option<f64> = None;

    fn walk(node: &PlanNode, ratio: &mut Option<f64>, spill: &mut Option<f64>) {
        if let (Some(est), Some(act)) = (&node.estimated, &node.actual) {
            if est.plan_rows > 0.0 && act.rows > 0.0 {
                let r = act.rows / est.plan_rows;
                if r >= 1.0 { // only report overestimation
                    *ratio = Some(ratio.unwrap_or(0.0).max(r));
                }
            }
        }
        if let Some(props) = &node.structured_props {
            if let Some(disk) = &props.sort_disk {
                if let Ok(kb) = disk.trim().trim_end_matches("kB").parse::<f64>() {
                    *spill = Some(spill.unwrap_or(0.0) + kb);
                }
            }
        }
        for child in &node.children {
            walk(child, ratio, spill);
        }
    }

    walk(node, &mut worst_ratio, &mut total_spill);
    (worst_ratio, total_spill.filter(|&v| v > 0.0))
}

fn find_peak_memory(node: &PlanNode) -> Option<f64> {
    node.structured_props.as_ref()
        .and_then(|p| p.peak_memory_kb)
        .or_else(|| {
            node.children.iter().filter_map(find_peak_memory).next()
        })
}

fn compute_pushdown_status(root: &PlanNode) -> PushdownStatus {
    fn has_streaming(node: &PlanNode) -> bool {
        matches!(node.node_type.category(), crate::model::node_type::NodeTypeCategory::Streaming)
            || node.children.iter().any(has_streaming)
    }
    if has_streaming(root) {
        PushdownStatus::NotPushed
    } else {
        PushdownStatus::Local
    }
}
```

**Step 4: Register module in lib.rs**

Add `pub mod summary;` to `crates/ogexplain-core/src/lib.rs`.

**Step 5: Run tests to verify they pass**

Run: `cargo test -p ogexplain-core --test summary_tests`
Expected: ALL PASS

**Step 6: Run full test suite for regression**

Run: `cargo test --workspace`
Expected: ALL PASS (existing 57 tests + new summary tests)

---

## Task 2: Add summary table rendering to CLI

**Files:**
- Modify: `crates/ogexplain-cli/src/lib.rs`

**Step 1: Write rendering function**

Add `print_summary_table()` function to `crates/ogexplain-cli/src/lib.rs` that:
1. Takes `&[(SummaryRow, usize, usize)]` (rows + block num + total)
2. Renders header row
3. Renders separator
4. Renders each data row with aligned columns
5. Uses colored output for severity and level

```rust
fn print_summary_table(rows: &[(ogexplain_core::summary::SummaryRow, usize, usize)]) {
    if rows.is_empty() {
        return;
    }

    println!("{}", "Summary".bright_cyan().bold());
    println!("{}", "───────".bright_cyan());

    // Header
    println!(
        "{:>2}  {:<30} {:>3} {:>4} {:>4} {:>5} {:<8} {:>8} {:>8} {:>7} {:>6} {:>8} {:>6} {:>5}",
        "#", "SQL Preview", "Tbl", "Join", "SubQ", "Score", "Level", "Cost", "Time", "Rows", "EstΔ", "Spill", "Mem", "C/W/I"
    );
    println!("{}", "─".repeat(115));

    for (row, num, _total) in rows {
        let sql = row.sql_preview.as_deref().unwrap_or("--");
        let score = row.score.map(|s| format!("{:.1}", s)).unwrap_or_else(|| "--".to_string());
        let level = row.level.as_deref().unwrap_or("--");
        let cost = format_cost(row.total_cost);
        let time = format_time(row.total_time_ms);
        let rows_str = row.actual_rows.map(|r| format_rows(r)).unwrap_or_else(|| "--".to_string());
        let est = row.worst_est_ratio.map(|r| format!("{:.0}x", r)).unwrap_or_else(|| "--".to_string());
        let spill = row.spill_kb.map(|s| format_spill(s)).unwrap_or_else(|| "--".to_string());
        let mem = row.peak_memory_kb.map(|m| format_memory(m)).unwrap_or_else(|| "--".to_string());
        let findings = format!("{}:{}:{}", row.critical_count, row.warning_count, row.info_count);

        println!(
            "{:>2}  {:<30} {:>3} {:>4} {:>4} {:>5} {:<8} {:>8} {:>8} {:>7} {:>6} {:>8} {:>6} {:>5}",
            num, sql, row.tables, row.joins, row.subqueries,
            score, level, cost, time, rows_str, est, spill, mem, findings
        );
    }
}
```

Also add helper formatting functions:
- `format_cost(f64) -> String` — e.g. "12.34", "2.6K", "1.2M"
- `format_time(f64) -> String` — e.g. "0.052ms", "55.8ms", "1.23s"
- `format_rows(f64) -> String` — e.g. "100", "50K", "1.2M"
- `format_spill(f64) -> String` — e.g. "48kB", "5.8MB"
- `format_memory(f64) -> String` — e.g. "8MB", "512MB"

**Step 2: Integrate into output flow**

In `output_text()`, after the plan tree and complexity sections, add:
```rust
// Summary table is printed at the END after all detail blocks
// Only for multi-block output
```

In `run()`, collect `SummaryRow` for each block, then call `print_summary_table()` at the end.

**Step 3: Build and test manually**

Run: `cargo run -p ogexplain-cli -- analyze /tmp/test_multi.txt`
Expected: See plan trees per block, THEN summary table at the end.

**Step 4: Run full test suite**

Run: `cargo test --workspace && cargo clippy --workspace --examples`
Expected: ALL PASS, zero warnings

---

## Task 3: Verify with real multi-block input

Create a test fixture with 3+ SQL+EXPLAIN blocks and verify the summary table renders correctly. This is a manual smoke test, not an automated test (TUI/CLI output is hard to assert automatically).

Run: `cargo run -p ogexplain-cli -- analyze /tmp/test_3blocks.txt`
Expected: 3 detail blocks + summary table at bottom.
