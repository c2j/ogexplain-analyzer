# SQL Complexity Analysis Integration Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Integrate `ogsql-parser` crate to parse SQL into AST, build a standalone `ogsql-complexity` crate that computes SQL complexity scores, and wire it into the TUI's Ctrl+P flow so mixed SQL+EXPLAIN input is analyzed end-to-end.

**Architecture:** Three-layer design: (1) `ogsql-complexity` crate depends on `ogsql-parser`, walks AST with a custom visitor to collect metrics, applies weighted Gauss scoring formula, returns a serializable `ComplexityReport`. (2) `ogexplain-core` gains a small `sql` module that extracts SQL text from mixed input (reusing existing `is_sql_line()` logic). (3) `ogexplain-tui` calls both layers on Ctrl+P, stores results in `App`, renders complexity section in detail panel.

**Tech Stack:** `ogsql-parser` (git dep), `serde` (derive), `thiserror` (errors), ratatui (TUI display)

---

## Task Dependency Graph

```
Task 1 (ogsql-complexity crate skeleton)
  └── Task 2 (Metrics model + weights)
       └── Task 3 (AST visitor / complexity engine)
            └── Task 4 (Tests for complexity engine)
Task 5 (SQL extraction in ogexplain-core)  ← independent of 1-4
  └── Task 6 (TUI integration)
       ├── depends on Task 4
       └── depends on Task 5
```

Tasks 1-4 and Task 5 can proceed in **parallel**.

---

## Task 1: Create `ogsql-complexity` Crate Skeleton

**Files:**
- Create: `crates/ogsql-complexity/Cargo.toml`
- Create: `crates/ogsql-complexity/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create the crate directory**

```bash
mkdir -p crates/ogsql-complexity/src
```

**Step 2: Write `crates/ogsql-complexity/Cargo.toml`**

```toml
[package]
name = "ogsql-complexity"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "SQL complexity analyzer for OpenGauss/GaussDB queries"

[dependencies]
ogsql-parser = { git = "https://github.com/c2j/ogsql-parser" }
serde = { version = "1", features = ["derive"] }
thiserror = "2"

[dev-dependencies]
insta = { version = "1", features = ["yaml"] }
```

**Step 3: Write `crates/ogsql-complexity/src/lib.rs` — minimal skeleton**

```rust
pub mod model;
pub mod engine;

pub use engine::analyze;
pub use model::{ComplexityReport, ComplexityLevel, ComplexityMetrics};
```

**Step 4: Write `crates/ogsql-complexity/src/model.rs` — types only, no logic yet**

```rust
use serde::Serialize;

/// Weight profile for a specific database dialect.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WeightProfile {
    pub name: String,
    pub table: f64,
    pub join: f64,
    pub where_condition: f64,
    pub subquery: f64,
    pub aggregate_function: f64,
    pub case_expression: f64,
    pub set_operation: f64,
    pub group_by: f64,
    pub order_by: f64,
    pub window_function: f64,
    pub cte: f64,
}

impl WeightProfile {
    /// GaussDB/OpenGauss weights.
    pub fn gauss() -> Self {
        Self {
            name: "gauss".to_string(),
            table: 1.0,
            join: 2.0,
            where_condition: 1.0,
            subquery: 3.0,
            aggregate_function: 1.5,
            case_expression: 1.5,
            set_operation: 2.0,
            group_by: 1.5,
            order_by: 1.0,
            window_function: 2.5,
            cte: 1.5,
        }
    }

    /// Oracle weights (for future use).
    pub fn oracle() -> Self {
        Self {
            name: "oracle".to_string(),
            table: 1.0,
            join: 2.0,
            where_condition: 1.5,
            subquery: 3.0,
            aggregate_function: 1.0,
            case_expression: 1.0,
            set_operation: 2.0,
            group_by: 1.5,
            order_by: 1.0,
            window_function: 2.5,
            cte: 1.5,
        }
    }

    /// Hive weights (for future use).
    pub fn hive() -> Self {
        Self {
            name: "hive".to_string(),
            table: 1.0,
            join: 2.0,
            where_condition: 1.5,
            subquery: 3.0,
            aggregate_function: 1.0,
            case_expression: 1.5,
            set_operation: 2.0,
            group_by: 1.5,
            order_by: 1.0,
            window_function: 2.5,
            cte: 1.5,
        }
    }
}

impl Default for WeightProfile {
    fn default() -> Self {
        Self::gauss()
    }
}

/// Statement type multiplier for non-SELECT statements.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub enum StatementTypeMultiplier {
    Select,
    Insert,
    Update,
    Delete,
    Merge,
    Other,
}

impl StatementTypeMultiplier {
    pub fn multiplier(&self) -> f64 {
        match self {
            Self::Select => 1.0,
            Self::Insert => 1.0,
            Self::Update => 1.2,
            Self::Delete => 1.1,
            Self::Merge => 1.5,
            Self::Other => 1.0,
        }
    }
}

/// Collected complexity metrics for a single SQL statement.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ComplexityMetrics {
    // Structural counts
    pub table_count: usize,
    pub join_count: usize,
    pub where_condition_count: usize,
    pub subquery_count: usize,
    pub aggregate_function_count: usize,
    pub case_expression_count: usize,
    pub set_operation_count: usize,
    pub cte_count: usize,
    pub window_function_count: usize,

    // Clause presence flags
    pub has_group_by: bool,
    pub has_order_by: bool,
    pub has_distinct: bool,

    // Depth
    pub subquery_depth: usize,
}

/// Overall complexity classification.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub enum ComplexityLevel {
    Trivial,
    Simple,
    Moderate,
    Complex,
    VeryComplex,
}

impl ComplexityLevel {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Trivial => "Trivial",
            Self::Simple => "Simple",
            Self::Moderate => "Moderate",
            Self::Complex => "Complex",
            Self::VeryComplex => "Very Complex",
        }
    }

    pub fn from_score(score: f64) -> Self {
        if score < 5.0 {
            Self::Trivial
        } else if score < 15.0 {
            Self::Simple
        } else if score < 30.0 {
            Self::Moderate
        } else if score < 50.0 {
            Self::Complex
        } else {
            Self::VeryComplex
        }
    }
}

/// Per-statement complexity result.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StatementComplexity {
    pub sql_text: String,
    pub statement_type: StatementTypeMultiplier,
    pub metrics: ComplexityMetrics,
    pub weighted_breakdown: WeightedBreakdown,
    pub raw_score: f64,
    pub adjusted_score: f64,
    pub level: ComplexityLevel,
}

/// Breakdown showing how each factor contributed to the score.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct WeightedBreakdown {
    pub tables: f64,
    pub joins: f64,
    pub where_conditions: f64,
    pub subqueries: f64,
    pub aggregate_functions: f64,
    pub case_expressions: f64,
    pub set_operations: f64,
    pub group_by: f64,
    pub order_by: f64,
    pub window_functions: f64,
    pub ctes: f64,
}

/// Complete complexity analysis report for one or more SQL statements.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ComplexityReport {
    pub statements: Vec<StatementComplexity>,
    pub overall_score: f64,
    pub overall_level: ComplexityLevel,
    pub profile: String,
}
```

**Step 5: Write `crates/ogsql-complexity/src/engine.rs` — placeholder**

```rust
use crate::model::*;

pub fn analyze(_sql: &str) -> Result<ComplexityReport, ComplexityError> {
    todo!("Task 3")
}

#[derive(Debug, thiserror::Error)]
pub enum ComplexityError {
    #[error("SQL parse error: {0}")]
    ParseError(String),
    #[error("Empty input")]
    EmptyInput,
}
```

**Step 6: Register in workspace root `Cargo.toml`**

Add `"crates/ogsql-complexity"` to the `members` array:

```toml
[workspace]
members = [
    "crates/ogexplain-core",
    "crates/ogexplain-cli",
    "crates/ogexplain-tui",
    "crates/ogsql-complexity",
]
```

**Step 7: Verify it compiles**

```bash
cargo build -p ogsql-complexity
```

Expected: compiles (with unused warnings for `todo!` is fine at this stage).

**Step 8: Commit**

```bash
git add crates/ogsql-complexity/ Cargo.toml
git commit -m "feat: scaffold ogsql-complexity crate with model types"
```

---

## Task 2: Implement Complexity Scoring Logic (Pure Function)

**Files:**
- Modify: `crates/ogsql-complexity/src/engine.rs`

**Rationale:** Separate the scoring computation from the AST visitor. The scoring is a pure function: `ComplexityMetrics + WeightProfile → f64 + WeightedBreakdown`. This makes it independently testable.

**Step 1: Add the scoring function to `engine.rs`**

Replace the entire `engine.rs` with:

```rust
use crate::model::*;

/// Compute weighted score and breakdown from metrics using the given weight profile.
pub fn compute_score(metrics: &ComplexityMetrics, profile: &WeightProfile) -> (f64, WeightedBreakdown, ComplexityLevel) {
    let breakdown = WeightedBreakdown {
        tables: metrics.table_count as f64 * profile.table,
        joins: metrics.join_count as f64 * profile.join,
        where_conditions: metrics.where_condition_count as f64 * profile.where_condition,
        subqueries: metrics.subquery_count as f64 * profile.subquery,
        aggregate_functions: metrics.aggregate_function_count as f64 * profile.aggregate_function,
        case_expressions: metrics.case_expression_count as f64 * profile.case_expression,
        set_operations: metrics.set_operation_count as f64 * profile.set_operation,
        group_by: if metrics.has_group_by { profile.group_by } else { 0.0 },
        order_by: if metrics.has_order_by { profile.order_by } else { 0.0 },
        window_functions: metrics.window_function_count as f64 * profile.window_function,
        ctes: metrics.cte_count as f64 * profile.cte,
    };

    let raw_score = breakdown.tables
        + breakdown.joins
        + breakdown.where_conditions
        + breakdown.subqueries
        + breakdown.aggregate_functions
        + breakdown.case_expressions
        + breakdown.set_operations
        + breakdown.group_by
        + breakdown.order_by
        + breakdown.window_functions
        + breakdown.ctes;

    let level = ComplexityLevel::from_score(raw_score);
    (raw_score, breakdown, level)
}

/// Apply statement-type multiplier to raw score.
pub fn adjust_score(raw_score: f64, stmt_type: StatementTypeMultiplier) -> f64 {
    raw_score * stmt_type.multiplier()
}

/// Compute overall score for multiple statements (max score).
pub fn overall_score(statements: &[StatementComplexity]) -> (f64, ComplexityLevel) {
    if statements.is_empty() {
        return (0.0, ComplexityLevel::Trivial);
    }
    let max_score = statements.iter().map(|s| s.adjusted_score).fold(0.0_f64, f64::max);
    (max_score, ComplexityLevel::from_score(max_score))
}

#[derive(Debug, thiserror::Error)]
pub enum ComplexityError {
    #[error("SQL parse error: {0}")]
    ParseError(String),
    #[error("Empty input")]
    EmptyInput,
}

/// Placeholder — will be implemented in Task 3.
pub fn analyze(_sql: &str) -> Result<ComplexityReport, ComplexityError> {
    todo!("Implemented in Task 3")
}
```

**Step 2: Verify it compiles**

```bash
cargo build -p ogsql-complexity
```

**Step 3: Commit**

```bash
git add crates/ogsql-complexity/src/engine.rs
git commit -m "feat: add pure scoring function for SQL complexity"
```

---

## Task 3: Implement AST Visitor (Complexity Engine Core)

**Files:**
- Modify: `crates/ogsql-complexity/src/engine.rs`
- Create: `crates/ogsql-complexity/src/visitor.rs`

This is the core of the crate. The visitor walks the ogsql-parser AST and collects `ComplexityMetrics`.

**Key insight from ogsql-parser analysis:**
- `ogsql_parser::ast::visitor::{Visitor, VisitorResult, walk_statement}` is the built-in visitor
- Built-in walker has **gaps**: doesn't walk joins, CTEs, set operations, group_by/having/order_by expressions
- We need a **custom traversal** that supplements the built-in walker
- `Parser::parse_sql(sql)` returns `(Vec<StatementInfo>, Vec<ParserError>)`
- `StatementInfo` has `.statement: Statement` and `.sql_text: String`

**Step 1: Create `crates/ogsql-complexity/src/visitor.rs`**

```rust
use std::collections::HashSet;

use ogsql_parser::ast::*;
use ogsql_parser::ast::visitor::{Visitor, VisitorResult, walk_statement};
use ogsql_parser::SelectStatement;

use crate::model::{ComplexityMetrics, StatementTypeMultiplier};

/// Known aggregate function names (case-insensitive).
const AGGREGATE_FUNCTIONS: &[&str] = &[
    "count", "sum", "avg", "min", "max",
    "stddev", "stddev_pop", "stddev_samp",
    "variance", "var_pop", "var_samp",
    "array_agg", "string_agg", "listagg", "group_concat",
    "bool_and", "bool_or",
    "bit_and", "bit_or", "bit_xor",
    "corr", "covar_pop", "covar_samp",
    "regr_avgx", "regr_avgy", "regr_count",
    "regr_intercept", "regr_r2", "regr_slope", "regr_sxx", "regr_sxy", "regr_syy",
    "approx_count_distinct",
    "every", "some_any",
    "json_agg", "json_object_agg", "jsonb_agg", "jsonb_object_agg",
    "xmlagg",
    "percentile_cont", "percentile_disc", "mode", "rank", "dense_rank",
    "percent_rank", "cume_dist", "ntile", "lag", "lead", "first_value", "last_value", "nth_value",
    // When used as aggregate (no OVER clause)
    "row_number",
];

pub struct ComplexityVisitor {
    pub metrics: ComplexityMetrics,
    depth: usize,
    max_depth: usize,
    cte_names: HashSet<String>,
}

impl ComplexityVisitor {
    pub fn new() -> Self {
        Self {
            metrics: ComplexityMetrics::default(),
            depth: 0,
            max_depth: 0,
            cte_names: HashSet::new(),
        }
    }

    pub fn finish(mut self) -> ComplexityMetrics {
        self.metrics.subquery_depth = self.max_depth;
        self.metrics
    }

    fn is_aggregate(&self, name: &ObjectName) -> bool {
        name.0.iter().any(|id| {
            let upper = id.to_uppercase();
            AGGREGATE_FUNCTIONS.iter().any(|agg| *agg == upper)
        })
    }

    /// Recursively count tables and joins from a TableRef tree.
    fn count_table_ref(&mut self, table_ref: &TableRef) {
        match table_ref {
            TableRef::Table { name, .. } => {
                let last_name = name.0.last().map(|id| id.to_lowercase());
                if let Some(n) = last_name {
                    if !self.cte_names.contains(&n) {
                        self.metrics.table_count += 1;
                    }
                }
            }
            TableRef::Subquery { .. } => {
                self.metrics.subquery_count += 1;
                // The inner query will be walked by the visitor
            }
            TableRef::Join { left, right, .. } => {
                self.metrics.join_count += 1;
                self.count_table_ref(left);
                self.count_table_ref(right);
            }
            TableRef::FunctionCall { .. } => {
                // Table-valued function, count as a source but not a table
            }
            TableRef::Pivot { source, .. } | TableRef::Unpivot { source, .. } => {
                self.count_table_ref(source);
            }
        }
    }

    /// Count WHERE condition nodes (AND/OR tree leaves).
    fn count_conditions(expr: &Expr) -> usize {
        match expr {
            Expr::BinaryOp { op, left, right } => {
                let op_lower = op.to_lowercase();
                if op_lower == "and" || op_lower == "or" {
                    1 + Self::count_conditions(left) + Self::count_conditions(right)
                } else {
                    1
                }
            }
            Expr::Parenthesized(inner) => Self::count_conditions(inner),
            _ => 1,
        }
    }

    /// Recursively count set operations in a SelectStatement chain.
    fn count_set_operations(select: &SelectStatement) -> usize {
        let mut count = 0;
        if let Some(ref set_op) = select.set_operation {
            count += 1;
            match set_op {
                SetOperation::Union { right, .. }
                | SetOperation::Intersect { right, .. }
                | SetOperation::Except { right, .. } => {
                    count += Self::count_set_operations(right);
                }
            }
        }
        count
    }

    /// Analyze a SelectStatement fully (supplements built-in visitor gaps).
    fn analyze_select(&mut self, select: &SelectStatement) {
        // Count tables and joins from FROM clause
        for table_ref in &select.from {
            self.count_table_ref(table_ref);
        }

        // CTEs
        if let Some(ref with) = select.with {
            for cte in &with.ctes {
                self.metrics.cte_count += 1;
                self.cte_names.insert(cte.name.to_lowercase());
            }
        }

        // Set operations
        self.metrics.set_operation_count += Self::count_set_operations(select);

        // Clause flags
        self.metrics.has_group_by = self.metrics.has_group_by || !select.group_by.is_empty();
        self.metrics.has_order_by = self.metrics.has_order_by || !select.order_by.is_empty();
        self.metrics.has_distinct = self.metrics.has_distinct || select.distinct;

        // WHERE condition count
        if let Some(ref where_clause) = select.where_clause {
            self.metrics.where_condition_count += Self::count_conditions(where_clause);
        }

        // Walk expressions in the select to find CASE, subqueries, aggregates, window funcs
        // Targets
        for target in &select.targets {
            self.walk_expr_for_metrics(target);
        }
        // WHERE
        if let Some(ref expr) = select.where_clause {
            self.walk_expr_for_metrics(expr);
        }
        // HAVING
        if let Some(ref expr) = select.having {
            self.walk_expr_for_metrics(expr);
        }
        // ORDER BY
        for item in &select.order_by {
            self.walk_expr_for_metrics(&item.expr);
        }
        // GROUP BY
        for item in &select.group_by {
            // GroupByItem may contain expressions
            match item {
                GroupByItem::Expr(expr) => self.walk_expr_for_metrics(expr),
                _ => {}
            }
        }
    }

    /// Walk an expression tree to find CASE, subqueries, aggregates, window functions.
    fn walk_expr_for_metrics(&mut self, expr: &Expr) {
        match expr {
            Expr::Case { .. } => {
                self.metrics.case_expression_count += 1;
            }
            Expr::Subquery(_) | Expr::Exists(_) => {
                self.metrics.subquery_count += 1;
                self.depth += 1;
                self.max_depth = self.max_depth.max(self.depth);
                self.depth -= 1;
            }
            Expr::InSubquery { .. } => {
                self.metrics.subquery_count += 1;
                self.depth += 1;
                self.max_depth = self.max_depth.max(self.depth);
                self.depth -= 1;
            }
            Expr::FunctionCall { name, over, .. } => {
                if over.is_some() {
                    self.metrics.window_function_count += 1;
                } else if self.is_aggregate(name) {
                    self.metrics.aggregate_function_count += 1;
                }
            }
            // Recurse into compound expressions
            Expr::BinaryOp { left, right, .. } => {
                self.walk_expr_for_metrics(left);
                self.walk_expr_for_metrics(right);
            }
            Expr::UnaryOp { expr: inner, .. } => {
                self.walk_expr_for_metrics(inner);
            }
            Expr::Parenthesized(inner) => {
                self.walk_expr_for_metrics(inner);
            }
            Expr::Between { .. } => {}
            Expr::InList { list, .. } => {
                for item in list {
                    self.walk_expr_for_metrics(item);
                }
            }
            Expr::Array(items) => {
                for item in items {
                    self.walk_expr_for_metrics(item);
                }
            }
            Expr::RowConstructor(items) => {
                for item in items {
                    self.walk_expr_for_metrics(item);
                }
            }
            Expr::TypeCast { expr: inner, .. } => {
                self.walk_expr_for_metrics(inner);
            }
            _ => {}
        }
    }
}

/// Determine statement type multiplier from ogsql-parser Statement enum.
pub fn statement_type(stmt: &Statement) -> StatementTypeMultiplier {
    match stmt {
        Statement::Select(_) => StatementTypeMultiplier::Select,
        Statement::Insert(_) => StatementTypeMultiplier::Insert,
        Statement::Update(_) => StatementTypeMultiplier::Update,
        Statement::Delete(_) => StatementTypeMultiplier::Delete,
        Statement::Merge(_) => StatementTypeMultiplier::Merge,
        _ => StatementTypeMultiplier::Other,
    }
}

/// Analyze a single parsed statement.
pub fn analyze_statement(stmt: &Statement) -> ComplexityMetrics {
    let mut visitor = ComplexityVisitor::new();

    match stmt {
        Statement::Select(select) => {
            visitor.analyze_select(select);
        }
        Statement::Insert(insert) => {
            if let Some(ref source) = insert.source {
                visitor.analyze_select(source);
            }
        }
        Statement::Update(update) => {
            // Count tables from the UPDATE target
            visitor.metrics.table_count += update.tables.len();
            // FROM clause tables
            for table_ref in &update.from {
                visitor.count_table_ref(table_ref);
            }
            // WHERE
            if let Some(ref expr) = update.where_clause {
                visitor.metrics.where_condition_count += ComplexityVisitor::count_conditions(expr);
                visitor.walk_expr_for_metrics(expr);
            }
        }
        Statement::Delete(delete) => {
            visitor.metrics.table_count += delete.tables.len();
            for table_ref in &delete.using {
                visitor.count_table_ref(table_ref);
            }
            if let Some(ref expr) = delete.where_clause {
                visitor.metrics.where_condition_count += ComplexityVisitor::count_conditions(expr);
                visitor.walk_expr_for_metrics(expr);
            }
        }
        Statement::Merge(merge) => {
            visitor.metrics.table_count += 1; // target
            visitor.metrics.table_count += 1; // source
            visitor.metrics.join_count += 1; // merge is effectively a join
        }
        Statement::Explain(explain) => {
            // Recurse into the explained statement
            return analyze_statement(&explain.query);
        }
        _ => {
            // For other statement types, use the built-in walker as fallback
            walk_statement(&mut visitor, stmt);
        }
    }

    visitor.finish()
}
```

**Step 2: Update `lib.rs` to include the visitor module**

```rust
pub mod model;
pub mod engine;
mod visitor;

pub use engine::analyze;
pub use model::{ComplexityReport, ComplexityLevel, ComplexityMetrics};
```

**Step 3: Implement the `analyze()` function in `engine.rs`**

Replace the `analyze` function and add the full engine:

```rust
use ogsql_parser::Parser;

use crate::model::*;
use crate::visitor::{self, statement_type};

/// Analyze SQL text and return a full complexity report.
pub fn analyze(sql: &str) -> Result<ComplexityReport, ComplexityError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(ComplexityError::EmptyInput);
    }

    let (infos, parse_errors) = Parser::parse_sql(trimmed);

    // If we got zero statements and have parse errors, report failure.
    if infos.is_empty() {
        if !parse_errors.is_empty() {
            return Err(ComplexityError::ParseError(
                parse_errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "),
            ));
        }
        return Err(ComplexityError::EmptyInput);
    }

    let profile = WeightProfile::default(); // Gauss
    let mut statements = Vec::new();

    for info in &infos {
        let stmt_type = statement_type(&info.statement);
        let metrics = visitor::analyze_statement(&info.statement);
        let (raw_score, weighted_breakdown, _level) = compute_score(&metrics, &profile);
        let adjusted_score = adjust_score(raw_score, stmt_type);

        statements.push(StatementComplexity {
            sql_text: info.sql_text.clone(),
            statement_type: stmt_type,
            metrics,
            weighted_breakdown,
            raw_score,
            adjusted_score,
            level: ComplexityLevel::from_score(adjusted_score),
        });
    }

    let (overall_score, overall_level) = overall_score(&statements);

    Ok(ComplexityReport {
        statements,
        overall_score,
        overall_level,
        profile: profile.name,
    })
}

// ... keep compute_score, adjust_score, overall_score, ComplexityError from Task 2
```

**Step 4: Verify it compiles**

```bash
cargo build -p ogsql-complexity 2>&1
```

**IMPORTANT:** At this stage there may be compilation errors because the ogsql-parser AST types may differ slightly from what we assumed. The implementing agent **must** read the actual ogsql-parser source to verify:
- `SelectStatement` field names (e.g., `targets` vs `projection`, `where_clause` vs `selection`)
- `GroupByItem` enum variants
- `Expr` enum variants
- `OrderByItem` struct fields
- `StatementInfo` struct fields

The agent should `cd` into the git checkout of ogsql-parser and read the actual AST definitions before finalizing this code. Key files to verify:
- `ogsql-parser/src/ast/mod.rs` — Statement enum, Expr enum, TableRef, SelectStatement
- `ogsql-parser/src/parser/mod.rs` — StatementInfo struct

**Step 5: Commit**

```bash
git add crates/ogsql-complexity/
git commit -m "feat: implement AST visitor for SQL complexity analysis"
```

---

## Task 4: Tests for Complexity Engine

**Files:**
- Create: `crates/ogsql-complexity/tests/complexity_tests.rs`

**Step 1: Write integration tests**

```rust
use ogsql_complexity::{analyze, ComplexityLevel};

#[test]
fn test_simple_select() {
    let sql = "SELECT * FROM users WHERE id = 1";
    let report = analyze(sql).unwrap();
    assert_eq!(report.statements.len(), 1);
    let s = &report.statements[0];
    assert_eq!(s.metrics.table_count, 1);
    assert_eq!(s.metrics.where_condition_count, 1);
    assert_eq!(s.metrics.join_count, 0);
    assert_eq!(s.metrics.subquery_count, 0);
    assert!(s.raw_score > 0.0);
    assert_eq!(s.level, ComplexityLevel::Trivial);
}

#[test]
fn test_join_query() {
    let sql = "SELECT u.name, o.total FROM users u JOIN orders o ON u.id = o.user_id WHERE o.total > 100";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.metrics.table_count, 2);
    assert_eq!(s.metrics.join_count, 1);
    assert!(s.raw_score > 3.0); // At least 2*1.0 (tables) + 1*2.0 (join) + conditions
}

#[test]
fn test_multi_join() {
    let sql = r#"
        SELECT u.name, o.total, p.name
        FROM users u
        JOIN orders o ON u.id = o.user_id
        JOIN products p ON o.product_id = p.id
        WHERE o.total > 100 AND u.active = true
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.metrics.table_count, 3);
    assert_eq!(s.metrics.join_count, 2);
    assert!(s.metrics.where_condition_count >= 2);
}

#[test]
fn test_subquery() {
    let sql = "SELECT * FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > 1000)";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.metrics.table_count, 2);
    assert!(s.metrics.subquery_count >= 1);
    assert!(s.raw_score > 5.0);
}

#[test]
fn test_aggregate_and_group_by() {
    let sql = "SELECT department, COUNT(*), AVG(salary) FROM employees GROUP BY department ORDER BY department";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.aggregate_function_count >= 2);
    assert!(s.metrics.has_group_by);
    assert!(s.metrics.has_order_by);
}

#[test]
fn test_case_expression() {
    let sql = r#"
        SELECT name,
            CASE WHEN score >= 90 THEN 'A'
                 WHEN score >= 80 THEN 'B'
                 ELSE 'C' END AS grade
        FROM students
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.case_expression_count >= 1);
}

#[test]
fn test_union() {
    let sql = "SELECT id, name FROM customers UNION ALL SELECT id, name FROM suppliers";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.set_operation_count >= 1);
}

#[test]
fn test_insert_statement() {
    let sql = "INSERT INTO logs (user_id, action) SELECT user_id, 'login' FROM sessions WHERE active = true";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, ogsql_complexity::StatementTypeMultiplier::Insert);
    // Insert multiplier is 1.0, so adjusted == raw in this case
    assert!(s.adjusted_score > 0.0);
}

#[test]
fn test_update_with_multiplier() {
    let sql = "UPDATE orders SET status = 'shipped' WHERE created_at < NOW()";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, ogsql_complexity::StatementTypeMultiplier::Update);
    assert!((s.statement_type.multiplier() - 1.2).abs() < 0.001);
}

#[test]
fn test_empty_input() {
    let result = analyze("");
    assert!(result.is_err());
}

#[test]
fn test_multiple_statements() {
    let sql = "SELECT * FROM t1; SELECT * FROM t2 JOIN t3 ON t2.id = t3.id";
    let report = analyze(sql).unwrap();
    assert!(report.statements.len() >= 2);
}

#[test]
fn test_weighted_breakdown_sums_correctly() {
    let sql = "SELECT * FROM users u JOIN orders o ON u.id = o.user_id WHERE u.active = true";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    let b = &s.weighted_breakdown;
    let sum = b.tables + b.joins + b.where_conditions + b.subqueries
        + b.aggregate_functions + b.case_expressions + b.set_operations
        + b.group_by + b.order_by + b.window_functions + b.ctes;
    // Sum should equal raw_score (within floating point tolerance)
    assert!((sum - s.raw_score).abs() < 0.001, "Breakdown sum {} != raw_score {}", sum, s.raw_score);
}

#[test]
fn test_complexity_levels() {
    // Trivial
    let report = analyze("SELECT 1").unwrap();
    assert_eq!(report.statements[0].level, ComplexityLevel::Trivial);

    // More complex query should be at least Simple
    let sql = "SELECT u.name, COUNT(*) FROM users u JOIN orders o ON u.id = o.user_id GROUP BY u.name ORDER BY COUNT(*) DESC";
    let report = analyze(sql).unwrap();
    let level = report.statements[0].level;
    assert!(matches!(level, ComplexityLevel::Simple | ComplexityLevel::Moderate | ComplexityLevel::Complex));
}
```

**Step 2: Run tests**

```bash
cargo test -p ogsql-complexity
```

**IMPORTANT:** The tests will likely need adjustments based on the actual ogsql-parser AST field names. The implementing agent must verify and fix compilation errors by reading the ogsql-parser source.

**Step 3: Fix any failures, iterate**

If tests fail due to incorrect metric counts (e.g., `table_count` is 0 when we expected 1), debug by:
1. Adding `println!("{:#?}", report)` in the failing test
2. Checking if the SQL is parsed correctly by ogsql-parser
3. Adjusting the visitor to match actual AST structure

**Step 4: Commit**

```bash
git add crates/ogsql-complexity/tests/
git commit -m "test: add complexity engine integration tests"
```

---

## Task 5: SQL Extraction in `ogexplain-core`

**Files:**
- Create: `crates/ogexplain-core/src/sql.rs`
- Modify: `crates/ogexplain-core/src/lib.rs`
- Modify: `crates/ogexplain-core/src/parser/mod.rs`

**Rationale:** The parser already has `is_sql_line()` and `is_explain_sql_command()` that identify SQL lines. We need to **collect** those lines instead of just skipping them, and expose a public API for SQL extraction.

**Step 1: Create `crates/ogexplain-core/src/sql.rs`**

```rust
use crate::parser::{is_sql_line, is_explain_sql_command};

/// Extracted content from mixed SQL + EXPLAIN input.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ExtractedContent {
    /// SQL statements found (e.g., the query before EXPLAIN output).
    pub sql_lines: Vec<String>,
    /// Combined SQL text (joined by newlines).
    pub sql_text: String,
    /// Whether any SQL was found.
    pub has_sql: bool,
}

impl ExtractedContent {
    pub fn from_text(text: &str) -> Self {
        let mut sql_lines: Vec<String> = Vec::new();
        let mut current_sql_block: Vec<String> = Vec::new();

        for raw_line in text.lines() {
            let line = raw_line.strip_prefix("--?").unwrap_or(raw_line);
            let trimmed = line.trim();

            if trimmed.is_empty() {
                if !current_sql_block.is_empty() {
                    current_sql_block.push(String::new());
                }
                continue;
            }

            if is_sql_line(trimmed) {
                // For EXPLAIN <sql> lines, extract the SQL part
                let sql_part = extract_sql_from_explain_line(trimmed);
                if let Some(part) = sql_part {
                    current_sql_block.push(part);
                } else {
                    current_sql_block.push(trimmed.to_string());
                }
            } else if is_explain_output_line(trimmed) {
                // Hit EXPLAIN output — flush SQL block
                if !current_sql_block.is_empty() {
                    let block: Vec<String> = current_sql_block.drain(..)
                        .filter(|l| !l.trim().is_empty())
                        .collect();
                    if !block.is_empty() {
                        sql_lines.extend(block);
                    }
                }
            }
            // Server messages, row footers, etc. — skip
        }

        // Flush remaining SQL
        if !current_sql_block.is_empty() {
            let block: Vec<String> = current_sql_block.drain(..)
                .filter(|l| !l.trim().is_empty())
                .collect();
            if !block.is_empty() {
                sql_lines.extend(block);
            }
        }

        let sql_text = sql_lines.join("\n");
        let has_sql = !sql_text.trim().is_empty();

        Self { sql_lines, sql_text, has_sql }
    }
}

/// Check if a line is part of EXPLAIN output (not SQL).
fn is_explain_output_line(s: &str) -> bool {
    s == "QUERY PLAN"
        || s.starts_with("---")
        || s.contains("(cost=")
        || s.contains("(actual time=")
        // Property lines
        || s.starts_with("Output:")
        || s.starts_with("Filter:")
        || s.starts_with("Sort Key:")
        || s.starts_with("Hash Cond:")
        || s.starts_with("Join Filter:")
        || s.starts_with("Index Cond:")
        || s.starts_with("Group By Key:")
        // Node lines
        || s.starts_with("->")
}

/// If line is `EXPLAIN [ANALYZE] [VERBOSE] SELECT ...`, extract the SQL part.
fn extract_sql_from_explain_line(s: &str) -> Option<String> {
    let lower = s.to_lowercase();
    if !lower.starts_with("explain ") {
        return None;
    }

    // Skip "explain" and optional keywords (analyze, verbose, performance, etc.)
    let rest = &s["explain ".len()..];
    let mut sql_rest = rest.trim_start();

    // Skip common EXPLAIN modifiers
    loop {
        let lower_rest = sql_rest.to_lowercase();
        if lower_rest.starts_with("analyze ") {
            sql_rest = &sql_rest["analyze ".len()..];
        } else if lower_rest.starts_with("verbose ") {
            sql_rest = &sql_rest["verbose ".len()..];
        } else if lower_rest.starts_with("performance ") {
            sql_rest = &sql_rest["performance ".len()..];
        } else if lower_rest.starts_with("(costs ") {
            // Skip EXPLAIN options in parentheses
            if let Some(end) = sql_rest.find(')') {
                sql_rest = sql_rest[end + 1..].trim_start();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if sql_rest.trim().is_empty() {
        return None;
    }

    Some(sql_rest.to_string())
}
```

**Step 2: Register the module in `lib.rs`**

Add `pub mod sql;` to `crates/ogexplain-core/src/lib.rs`:

```rust
pub mod analyzer;
pub mod model;
pub mod parser;
pub mod sql;        // ← NEW
pub mod suggester;

pub use parser::parse;
pub use parser::parse_multi;

// ... rest unchanged
```

**Step 3: Make parser helper functions pub(super) or move them**

The `is_sql_line` and `is_explain_sql_command` functions in `parser/mod.rs` are currently private. We need `sql.rs` to call them. Change their visibility:

In `crates/ogexplain-core/src/parser/mod.rs`, change:
```rust
fn is_sql_line(s: &str) -> bool {        // currently private
```
to:
```rust
pub(crate) fn is_sql_line(s: &str) -> bool {
```

And:
```rust
fn is_explain_sql_command(lower: &str) -> bool {
```
to:
```rust
pub(crate) fn is_explain_sql_command(lower: &str) -> bool {
```

**Step 4: Verify it compiles**

```bash
cargo build -p ogexplain-core
```

**Step 5: Commit**

```bash
git add crates/ogexplain-core/src/sql.rs crates/ogexplain-core/src/lib.rs crates/ogexplain-core/src/parser/mod.rs
git commit -m "feat: add SQL extraction module to ogexplain-core"
```

---

## Task 6: TUI Integration — Wire Complexity Analysis into Ctrl+P Flow

**Files:**
- Modify: `crates/ogexplain-tui/Cargo.toml` — add dependencies
- Modify: `crates/ogexplain-tui/src/app.rs` — store complexity report, call analysis in `do_parse()`
- Modify: `crates/ogexplain-tui/src/components/detail_panel.rs` — render complexity section
- Modify: `crates/ogexplain-tui/src/components/summary_bar.rs` — show complexity score badge

**Step 1: Update TUI dependencies**

Add to `crates/ogexplain-tui/Cargo.toml`:

```toml
[dependencies]
ogexplain-core = { path = "../ogexplain-core" }
ogsql-complexity = { path = "../ogsql-complexity" }    # ← NEW
ratatui = "0.30"
# ... rest unchanged
```

**Step 2: Add complexity fields to `App` struct**

In `crates/ogexplain-tui/src/app.rs`, add to the `App` struct (after `suggestions`):

```rust
pub struct App {
    // ... existing fields ...
    report: Option<DiagnosticReport>,
    suggestions: Vec<Suggestion>,

    // SQL complexity analysis
    complexity_report: Option<ogsql_complexity::ComplexityReport>,
    extracted_sql: Option<String>,
    show_complexity: bool,

    // ... rest unchanged ...
}
```

Initialize them in `App::new()`:

```rust
complexity_report: None,
extracted_sql: None,
show_complexity: false,
```

**Step 3: Modify `do_parse()` to extract SQL and run complexity analysis**

In `crates/ogexplain-tui/src/app.rs`, update `do_parse()`:

```rust
fn do_parse(&mut self) {
    let raw: String = self.textarea.lines().join("\n");
    let text = raw.replace('\r', "");
    self.error_message = None;

    // Extract SQL from mixed input
    let extracted = ogexplain_core::sql::ExtractedContent::from_text(&text);
    if extracted.has_sql {
        self.extracted_sql = Some(extracted.sql_text.clone());
        match ogsql_complexity::analyze(&extracted.sql_text) {
            Ok(report) => {
                self.complexity_report = Some(report);
                self.show_complexity = true;
            }
            Err(_) => {
                self.complexity_report = None;
            }
        }
    } else {
        self.extracted_sql = None;
        self.complexity_report = None;
        self.show_complexity = false;
    }

    // Parse EXPLAIN plans (existing logic)
    match ogexplain_core::parse_multi(&text) {
        Ok(plans) if !plans.is_empty() => {
            self.plans = plans;
            self.plan_index = 0;
            self.activate_plan(0);
        }
        Ok(_) => {
            // No EXPLAIN plans found, but we might still have SQL complexity
            if self.complexity_report.is_some() {
                // Switch to browse mode to show complexity even without EXPLAIN
                self.mode = AppMode::Browse;
                self.focus = FocusTarget::Tree;
            } else {
                self.error_message = Some("No plan nodes found".to_string());
            }
        }
        Err(e) => {
            if self.complexity_report.is_some() {
                // SQL was analyzed even if EXPLAIN parsing failed
                self.mode = AppMode::Browse;
                self.focus = FocusTarget::Tree;
            } else {
                let preview: String = text
                    .lines()
                    .take(3)
                    .map(|l| {
                        if l.len() > 60 {
                            format!("{}...", &l[..60])
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                self.error_message =
                    Some(format!("{} ({}行 '{}')", e, text.lines().count(), preview));
            }
        }
    }
}
```

**Step 4: Clear complexity state in `Action::ClearInput` handler**

In the `Action::ClearInput` match arm (around line 495), add:

```rust
Action::ClearInput => {
    self.textarea = TextArea::new(vec![String::new()]);
    self.plans.clear();
    self.report = None;
    self.suggestions.clear();
    self.complexity_report = None;    // ← NEW
    self.extracted_sql = None;        // ← NEW
    self.show_complexity = false;     // ← NEW
    self.flattened_nodes.clear();
    // ... rest unchanged
}
```

**Step 5: Add `ToggleComplexity` action**

In `crates/ogexplain-tui/src/action.rs`, add a new variant:

```rust
pub enum Action {
    // ... existing variants ...
    ToggleComplexity,    // ← NEW
}
```

In `crates/ogexplain-tui/src/event.rs`, add keybinding (e.g., `c` key when not in Input):

```rust
// Add this match arm, before the `_ => None` fallback:
(false, false, KeyCode::Char('c')) => match focus {
    FocusTarget::Input => None,
    _ => Some(Action::ToggleComplexity),
},
```

In `app.rs` `update()`, add the handler:

```rust
Action::ToggleComplexity => {
    if self.complexity_report.is_some() {
        self.show_complexity = !self.show_complexity;
        self.detail_scroll = 0;
    }
}
```

**Step 6: Render complexity section in detail panel**

In `crates/ogexplain-tui/src/components/detail_panel.rs`, update the `render` function signature and add complexity rendering.

Update the function signature to accept an optional complexity report:

```rust
use ogsql_complexity::{ComplexityReport, ComplexityLevel};

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    node: Option<&PlanNode>,
    findings: Vec<Finding>,
    suggestions: Vec<Suggestion>,
    complexity: Option<&ComplexityReport>,
    show_complexity: bool,
    scroll: u16,
    focused: bool,
    total_lines: u16,
) {
    // ... existing border/title code ...

    let mut lines = match node {
        Some(n) => build_detail_lines(n, findings, suggestions),
        None => vec![Line::from(Span::styled(
            " 粘贴 EXPLAIN 输出后按 Ctrl+P 解析",
            Style::default().fg(Color::DarkGray),
        ))],
    };

    // Append complexity section if available
    if show_complexity {
        if let Some(report) = complexity {
            lines.push(Line::from(Span::raw("")));
            lines.extend(build_complexity_lines(report));
        }
    }

    // ... rest of rendering unchanged
}
```

Add the complexity rendering function:

```rust
fn build_complexity_lines(report: &ComplexityReport) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "── SQL 复杂度分析 ──",
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )));

    // Overall score
    let (level_color, level_icon) = match report.overall_level {
        ComplexityLevel::Trivial => (Color::Green, "●"),
        ComplexityLevel::Simple => (Color::Green, "◐"),
        ComplexityLevel::Moderate => (Color::Yellow, "◑"),
        ComplexityLevel::Complex => (Color::Red, "◉"),
        ComplexityLevel::VeryComplex => (Color::Magenta, "✖"),
    };

    lines.push(Line::from(vec![
        Span::styled("  总分: ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{:.1}", report.overall_score),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", report.overall_level.label()),
            Style::default().fg(level_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", report.profile),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    // Per-statement breakdown
    for (i, stmt) in report.statements.iter().enumerate() {
        if report.statements.len() > 1 {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  语句 #{} ", i + 1),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:.1} 分", stmt.adjusted_score),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        let m = &stmt.metrics;
        let b = &stmt.weighted_breakdown;

        // Metrics summary line
        let mut parts = Vec::new();
        if m.table_count > 0 {
            parts.push(format!("{} 表(×{:.1}={:.1})", m.table_count, 1.0, b.tables));
        }
        if m.join_count > 0 {
            parts.push(format!("{} 连接(×{:.1}={:.1})", m.join_count, 2.0, b.joins));
        }
        if m.where_condition_count > 0 {
            parts.push(format!("{} 条件(×{:.1}={:.1})", m.where_condition_count, 1.0, b.where_conditions));
        }
        if m.subquery_count > 0 {
            parts.push(format!("{} 子查询(×{:.1}={:.1})", m.subquery_count, 3.0, b.subqueries));
        }
        if m.aggregate_function_count > 0 {
            parts.push(format!("{} 聚合(×{:.1}={:.1})", m.aggregate_function_count, 1.5, b.aggregate_functions));
        }
        if m.case_expression_count > 0 {
            parts.push(format!("{} CASE(×{:.1}={:.1})", m.case_expression_count, 1.5, b.case_expressions));
        }
        if m.set_operation_count > 0 {
            parts.push(format!("{} 集合操作(×{:.1}={:.1})", m.set_operation_count, 2.0, b.set_operations));
        }
        if m.has_group_by {
            parts.push(format!("GROUP BY({:.1})", b.group_by));
        }
        if m.has_order_by {
            parts.push(format!("ORDER BY({:.1})", b.order_by));
        }
        if m.window_function_count > 0 {
            parts.push(format!("{} 窗口函数(×{:.1}={:.1})", m.window_function_count, 2.5, b.window_functions));
        }
        if m.cte_count > 0 {
            parts.push(format!("{} CTE(×{:.1}={:.1})", m.cte_count, 1.5, b.ctes));
        }

        for part in parts {
            lines.push(Line::from(Span::styled(
                format!("    {}", part),
                Style::default().fg(Color::Gray),
            )));
        }

        // Depth info
        if m.subquery_depth > 0 {
            lines.push(Line::from(Span::styled(
                format!("    嵌套深度: {}", m.subquery_depth),
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Show truncated SQL preview
        let sql_preview: String = stmt.sql_text
            .lines()
            .take(2)
            .map(|l| {
                if l.len() > 60 { format!("{}...", &l[..60]) } else { l.to_string() }
            })
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(Line::from(Span::styled(
            format!("    SQL: {}", sql_preview),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Toggle hint
    lines.push(Line::from(Span::styled(
        "    [c] 切换复杂度视图",
        Style::default().fg(Color::DarkGray),
    )));

    lines
}
```

**Step 7: Update `render_main()` in `app.rs` to pass complexity report**

In the `render_main()` method, update all calls to `components::render_detail` to pass the new parameters:

```rust
// Replace all existing render_detail calls with:
components::render_detail(
    frame,
    detail_area,
    node,
    findings,
    suggestions,
    self.complexity_report.as_ref(),
    self.show_complexity,
    self.detail_scroll,
    self.focus == FocusTarget::Detail,
    self.detail_line_count,
);
```

**Step 8: Update `render_main()` calls — there are multiple call sites**

There are 3 places in `render_main()` where `render_detail` is called (raw view, all findings, per-node). All need the new parameters added.

**Step 9: Update status bar to show `c` keybinding**

In `crates/ogexplain-tui/src/components/status_bar.rs`, add the complexity toggle to browse mode status hints. Add after the `r` keybinding in both Tree and Detail focus sections:

For `FocusTarget::Tree`:
```rust
span("c", k),
span("复杂度  ", d),
```

For `FocusTarget::Detail`:
```rust
span("c", k),
span("复杂度  ", d),
```

**Step 10: Update help overlay**

In `crates/ogexplain-tui/src/components/help_overlay.rs`, add:

```
c          Toggle SQL complexity view
```

**Step 11: Build and test**

```bash
cargo build -p ogexplain-tui
```

**Step 12: Commit**

```bash
git add crates/ogexplain-tui/
git commit -m "feat: integrate SQL complexity analysis into TUI Ctrl+P flow"
```

---

## Task 7: End-to-End Verification

**Step 1: Full workspace build**

```bash
cargo build --workspace
```

**Step 2: All tests pass**

```bash
cargo test --workspace
```

**Step 3: Clippy clean**

```bash
cargo clippy --workspace
```

**Step 4: Manual TUI test with fixture**

```bash
# Create a test file with mixed SQL + EXPLAIN
cat > /tmp/test_mixed.txt << 'EOF'
explain analyze
SELECT u.name, COUNT(*) as order_count, SUM(o.total) as total_spent
FROM users u
JOIN orders o ON u.id = o.user_id
WHERE u.active = true AND o.created_at > '2024-01-01'
GROUP BY u.name
HAVING COUNT(*) > 5
ORDER BY total_spent DESC;

                                                QUERY PLAN
-------------------------------------------------------------------------------------------------
Sort  (cost=125.38..125.40 rows=10 width=72) (actual time=0.045..0.046 rows=5 loops=1)
  Sort Key: (sum(o.total)) DESC
  Sort Method: quicksort  Memory: 25kB
  ->  HashAggregate  (cost=125.20..125.30 rows=10 width=72) (actual time=0.035..0.036 rows=5 loops=1)
        Group By Key: u.name
        ->  Hash Join  (cost=52.25..112.75 rows=1000 width=48) (actual time=0.018..0.026 rows=20 loops=1)
              Hash Cond: (o.user_id = u.id)
              ->  Seq Scan on orders o  (cost=0.00..35.50 rows=2000 width=20) (actual time=0.005..0.008 rows=200 loops=1)
                    Filter: (created_at > '2024-01-01'::date)
              ->  Hash  (cost=22.50..22.50 rows=500 width=36) (actual time=0.005..0.005 rows=50 loops=1)
                    ->  Seq Scan on users u  (cost=0.00..22.50 rows=500 width=36) (actual time=0.002..0.003 rows=50 loops=1)
                          Filter: active
EOF

cargo run -p ogexplain-tui -- /tmp/test_mixed.txt
```

Expected:
- TUI opens with parsed EXPLAIN plan in browse mode
- Detail panel shows "SQL 复杂度分析" section with score breakdown
- Press `c` to toggle complexity view
- Score should reflect: 2 tables, 1 join, WHERE conditions, GROUP BY, ORDER BY, 2 aggregates

**Step 5: Commit**

```bash
git add -A
git commit -m "feat: complete SQL complexity analysis integration"
```

---

## Important Notes for Implementing Agent

1. **ogsql-parser AST verification is CRITICAL.** The field names in this plan are based on the librarian's analysis but may have inaccuracies. The implementing agent MUST read the actual source of `ogsql-parser` to verify:
   - `SelectStatement` fields (e.g., `targets` vs `projection`, `where_clause` vs `selection`)
   - `GroupByItem` enum variants
   - `Expr` enum variants (especially `FunctionCall` fields)
   - `OrderByItem` struct fields
   - `StatementInfo` struct fields (`.sql_text` vs `.sql`)

2. **Weight profile uses Gauss defaults.** The `WeightProfile::gauss()` matches the user's specification. Oracle and Hive profiles are placeholders for future use.

3. **Complexity extraction (`ExtractedContent`) lives in ogexplain-core** because it reuses `is_sql_line()` from the parser module. This keeps the dependency direction clean: `ogsql-complexity` depends only on `ogsql-parser`, not on `ogexplain-core`.

4. **The `detail_panel.rs` changes require updating all 3 call sites** in `render_main()`. The agent must ensure consistency.

5. **Test adjustments.** The test SQL strings may need to match what ogsql-parser can actually parse. If ogsql-parser has specific dialect requirements, adjust the test SQL accordingly.
