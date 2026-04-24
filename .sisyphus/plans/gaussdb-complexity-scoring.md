# GaussDB Complexity Scoring Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the GaussDB SQL complexity scoring engine per `docs/SQL复杂度计算规则.md`, covering SQL statement-level metrics, stored procedure-level metrics, and the full 11-step GaussDB scoring formula.

**Architecture:** Extend the existing `crates/ogsql-complexity` crate. Add a PL/pgSQL visitor (`pl_visitor.rs`) for stored procedure metrics, rewrite the scoring engine (`engine.rs`) with the GaussDB formula, and extend the data model (`model.rs`) with all required metric fields and user configuration.

**Tech Stack:** Rust, ogsql-parser (AST), serde, insta (snapshot tests)

---

## Reference: Key Files

| File | Purpose |
|------|---------|
| `docs/SQL复杂度计算规则.md` | The specification document (477 lines) |
| `crates/ogsql-complexity/src/model.rs` | Data model — needs major extension |
| `crates/ogsql-complexity/src/visitor.rs` | SQL statement visitor — needs fixes |
| `crates/ogsql-complexity/src/engine.rs` | Scoring engine — needs rewrite |
| `crates/ogsql-complexity/src/lib.rs` | Public API — needs update |
| `crates/ogsql-complexity/tests/complexity_tests.rs` | Tests — needs expansion |

### ogsql-parser AST types used (from cargo cache)

| Type | Location | Used for |
|------|----------|----------|
| `Statement` (150+ variants) | `ogsql_parser::ast::Statement` | Statement dispatch |
| `SelectStatement.hints: Vec<String>` | `ogsql_parser::ast::SelectStatement` | Hint counting |
| `InsertStatement.hints: Vec<String>` | `ogsql_parser::ast::InsertStatement` | Hint counting |
| `UpdateStatement.hints: Vec<String>` | `ogsql_parser::ast::UpdateStatement` | Hint counting |
| `DeleteStatement.hints: Vec<String>` | `ogsql_parser::ast::DeleteStatement` | Hint counting |
| `MergeStatement.hints: Vec<String>` | `ogsql_parser::ast::MergeStatement` | Hint counting |
| `CreateTableStatement` | `ogsql_parser::ast::CreateTableStatement` | CREATE TABLE scoring |
| `CreateFunctionStatement` | `ogsql_parser::ast::CreateFunctionStatement` | Function body extraction |
| `CreateProcedureStatement` | `ogsql_parser::ast::CreateProcedureStatement` | Procedure body extraction |
| `AnonyBlockStatement` | `ogsql_parser::ast::AnonyBlockStatement` | Anonymous blocks |
| `DoStatement` | `ogsql_parser::ast::DoStatement` | DO blocks |
| `CreatePackageStatement` | `ogsql_parser::ast::CreatePackageStatement` | Package metrics |
| `CreatePackageBodyStatement` | `ogsql_parser::ast::CreatePackageBodyStatement` | Package body metrics |
| `PlBlock` | `ogsql_parser::ast::plpgsql::PlBlock` | PL/pgSQL block traversal |
| `PlStatement` (20 variants) | `ogsql_parser::ast::plpgsql::PlStatement` | Loop/cursor/execute/tx detection |
| `PlDeclaration` (6 variants) | `ogsql_parser::ast::plpgsql::PlDeclaration` | Cursor/var/type detection |
| `PlExecuteStmt` | `ogsql_parser::ast::plpgsql::PlExecuteStmt` | Dynamic SQL + param binding |
| `PlCursorDecl` | `ogsql_parser::ast::plpgsql::PlCursorDecl` | Cursor declaration |
| `PackageItem` | `ogsql_parser::ast::PackageItem` | Package procedures/functions |

---

## Task 1: Extend Data Model (`model.rs`)

**Files:**
- Modify: `crates/ogsql-complexity/src/model.rs`

**Goal:** Add all GaussDB-specific metric fields, weight constants, user configuration struct, and report types.

### Step 1: Add GaussDB weight constants

At the top of `model.rs`, add:

```rust
/// GaussDB weight constants per the complexity scoring specification.
pub mod gauss_weights {
    pub const TABLE: i64 = 10;
    pub const JOIN: i64 = 15;
    pub const WHERE_CONDITION: i64 = 5;
    pub const SUBQUERY: i64 = 20;
    pub const AGGREGATE_FUNCTION: i64 = 10;
    pub const CASE_EXPRESSION: i64 = 5;
    pub const SET_OPERATION: i64 = 15;
    pub const GROUP_BY: i64 = 5;
    pub const ORDER_BY: i64 = 5;
    pub const LOOP: i64 = 15;
    pub const NESTED_LOOP: i64 = 20;
    pub const CUSTOM_FUNCTION: i64 = 10;
    pub const HIGH_WEIGHT_TABLE: i64 = 20;
    pub const HIGH_WEIGHT_PROCEDURE: i64 = 20;
    pub const NESTED_PROCEDURE: i64 = 15;
    pub const HINT: i64 = 3;
    pub const CURSOR_DECLARATION: i64 = 10;
    pub const CURSOR_OPERATION: i64 = 5;
    pub const NESTED_CURSOR: i64 = 15;
    pub const DYNAMIC_SQL: i64 = 15;
    pub const PARAMETER_BINDING: i64 = 5;
    pub const NESTED_DYNAMIC_SQL: i64 = 25;
    pub const TRANSACTION_CONTROL: i64 = 10;
    pub const AUTONOMOUS_TRANSACTION: i64 = 15;
    pub const NESTED_TRANSACTION: i64 = 20;
    pub const JAVA_PROCEDURE: i64 = 25;
    pub const TYPE_CONVERSION: i64 = 5;
    pub const TABLE_WEIGHT: i64 = 10;
    pub const COLUMN: i64 = 2;
    pub const COMPUTED_COLUMN: i64 = 15;
    pub const CHECK_CONSTRAINT: i64 = 10;
}
```

### Step 2: Add user configuration struct

```rust
/// User-provided configuration for complexity scoring.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ComplexityConfig {
    /// User-provided custom function names (matched via `\bname\s*\(`).
    pub custom_functions: Vec<String>,
    /// User-provided high-weight table names.
    pub high_weight_tables: Vec<String>,
    /// User-provided high-weight procedure names.
    pub high_weight_procedures: Vec<String>,
    /// List of known built-in functions to exclude from nested procedure detection.
    pub builtin_functions: Vec<String>,
}
```

### Step 3: Extend ComplexityMetrics

Add these fields to the existing `ComplexityMetrics` struct:

```rust
    // SQL statement level additions
    pub hint_count: usize,

    // Stored procedure level metrics
    pub loop_count: usize,
    pub max_loop_nesting_level: usize,
    pub cursor_count: usize,
    pub cursor_operation_count: usize,
    pub max_cursor_nesting_level: usize,
    pub dynamic_sql_count: usize,
    pub param_binding_count: usize,
    pub nested_dynamic_sql_count: usize,
    pub transaction_control_count: usize,
    pub transaction_nesting_level: usize,
    pub uses_autonomous_transactions: bool,
    pub subtransaction_count: usize,
    pub max_subtransaction_nesting_level: usize,

    // Counted from user config matching
    pub custom_function_count: usize,
    pub high_weight_table_count: usize,
    pub nested_procedure_count: usize,
    pub high_weight_procedure_count: usize,

    // Java stored procedure metrics
    pub java_stored_procedure_count: usize,
    pub java_type_conversion_count: usize,

    // CREATE TABLE metrics
    pub column_count: usize,
    pub computed_column_count: usize,
    pub check_constraint_count: usize,

    // Source metrics
    pub line_count: usize,

    // Package metrics
    pub package_procedure_count: usize,
    pub package_variable_count: usize,
    pub package_has_java: bool,
```

### Step 4: Add package metrics struct

```rust
/// Package-level metrics for GaussDB packages.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PackageMetrics {
    pub total_procedures: usize,
    pub package_level_variables: usize,
    pub contains_java_procedures: bool,
}
```

### Step 5: Add GaussDB-specific report types

```rust
/// Input type for the scoring engine — identifies what kind of input we have.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// A standalone SQL statement (SELECT, INSERT, etc.)
    SqlStatement,
    /// A stored procedure/function/package body
    StoredProcedure,
    /// An anonymous PL/pgSQL block (DO / BEGIN...END)
    AnonymousBlock,
}

/// The final GaussDB complexity report with detailed scoring breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct GaussDbComplexityReport {
    /// The input kind
    pub input_kind: InputKind,
    /// All SQL statement scores (for stored procedures, these come from extracted SQL)
    pub statement_scores: Vec<StatementComplexity>,
    /// Accumulated metrics from PL/pgSQL analysis
    pub pl_metrics: ComplexityMetrics,
    /// Raw score before GaussDB enhancement (steps 1-7)
    pub pre_enhanced_score: i64,
    /// Final GaussDB score after all 11 steps
    pub overall_score: i64,
    /// Complexity level
    pub level: ComplexityLevel,
    /// Detailed scoring breakdown for each step
    pub score_breakdown: GaussDbScoreBreakdown,
}

/// Step-by-step breakdown of the GaussDB scoring formula.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GaussDbScoreBreakdown {
    // Step 1: SQL statements sum
    pub sql_statements_sum: i64,
    // Step 2: Loop complexity
    pub loop_complexity: i64,
    // Step 3: Custom function complexity
    pub custom_function_complexity: i64,
    // Step 4: High-weight table complexity
    pub high_weight_table_complexity: i64,
    // Step 5: Nested procedure complexity
    pub nested_procedure_complexity: i64,
    // Step 6: High-weight procedure complexity
    pub high_weight_procedure_complexity: i64,
    // Step 7: Cursor complexity
    pub cursor_complexity: i64,
    // Step 8: Enhanced complexity override
    pub enhanced_complexity: i64,
    // Step 9: Additional GaussDB items
    pub dynamic_sql_complexity: i64,
    pub param_binding_complexity: i64,
    pub nested_dynamic_sql_complexity: i64,
    pub transaction_complexity: i64,
    pub autonomous_transaction_bonus: i64,
    pub java_procedure_complexity: i64,
    pub java_type_conversion_complexity: i64,
    pub hint_complexity: i64,
    pub package_complexity: i64,
}
```

### Step 6: Keep existing types for backward compatibility

Keep `WeightProfile`, `ComplexityLevel`, `StatementComplexity`, `ComplexityReport` intact. The new GaussDB scoring runs in parallel as a separate code path. The `ComplexityLevel::from_score` thresholds should be updated to match GaussDB document expectations (or add a `from_gauss_score` method).

---

## Task 2: Fix SQL Statement Visitor (`visitor.rs`)

**Files:**
- Modify: `crates/ogsql-complexity/src/visitor.rs`

**Goal:** Fix WHERE condition counting (0/1 for GaussDB), add hint counting, add CREATE TABLE handling, add stored procedure extraction.

### Step 1: Add hint counting

In `visit_select()`, add hint extraction:
```rust
// Count hints
self.metrics.hint_count += select.hints.len();
```

In `visit_statement()`, for Insert/Update/Delete/Merge, count their hints:
```rust
Statement::Insert(i) => {
    self.metrics.hint_count += i.hints.len();
    // ... rest of existing logic
}
```

### Step 2: Add a GaussDB WHERE mode flag

Add a field to `ComplexityVisitor`:
```rust
struct ComplexityVisitor {
    metrics: ComplexityMetrics,
    cte_names: HashSet<String>,
    current_depth: usize,
    /// GaussDB mode: WHERE condition count is 0 or 1 (existence only)
    gaussdb_where_mode: bool,
}
```

In `count_conditions()`, add:
```rust
fn count_conditions(&self, expr: &Expr) -> usize {
    if self.gaussdb_where_mode {
        return 1; // GaussDB: just check existence
    }
    // existing AND/OR counting logic...
}
```

### Step 3: Add CREATE TABLE handling

```rust
Statement::CreateTable(ct) => {
    self.metrics.table_count += 1;
    self.metrics.column_count = ct.columns.len();
    for col in &ct.columns {
        for constraint in &col.constraints {
            if matches!(constraint, ColumnConstraint::Check(_)) {
                self.metrics.check_constraint_count += 1;
            }
        }
        // Computed column: has a DEFAULT with non-trivial expression
        for constraint in &col.constraints {
            if matches!(constraint, ColumnConstraint::Default(_)) {
                self.metrics.computed_column_count += 1;
            }
        }
    }
    for constraint in &ct.constraints {
        if matches!(constraint, TableConstraint::Check(_)) {
            self.metrics.check_constraint_count += 1;
        }
    }
}
```

### Step 4: Add stored procedure/function/DO block extraction

```rust
Statement::CreateFunction(cf) => {
    if let Some(block) = &cf.block {
        // Delegate to PL visitor (Task 3)
    }
    if cf.options.language.as_deref() == Some("java") || cf.options.language.as_deref() == Some("JAVA") {
        self.metrics.java_stored_procedure_count += 1;
    }
}
Statement::CreateProcedure(cp) => {
    if let Some(block) = &cp.block {
        // Delegate to PL visitor (Task 3)
    }
}
Statement::Do(d) => {
    if let Some(block) = &d.block {
        // Delegate to PL visitor (Task 3)
    }
}
Statement::AnonyBlock(ab) => {
    // Delegate to PL visitor (Task 3)
}
Statement::CreatePackage(cp) => {
    // Extract package metrics
}
Statement::CreatePackageBody(cpb) => {
    // Extract package body metrics
}
```

### Step 5: Add public analyze functions

Add a `analyze_statement_gauss()` function that returns `ComplexityMetrics` with GaussDB WHERE mode enabled.

---

## Task 3: Create PL/pgSQL Visitor (`pl_visitor.rs`)

**Files:**
- Create: `crates/ogsql-complexity/src/pl_visitor.rs`

**Goal:** Walk `PlBlock` AST and extract all stored procedure level metrics.

### Key implementation:

```rust
use std::collections::HashSet;
use crate::model::ComplexityMetrics;
use ogsql_parser::ast::plpgsql::{
    PlBlock, PlDeclaration, PlStatement, PlOpenKind,
};
use ogsql_parser::ast::Statement;

pub struct PlComplexityVisitor<'a> {
    metrics: &'a mut ComplexityMetrics,
    custom_functions: &'a [String],
    high_weight_tables: &'a [String],
    high_weight_procedures: &'a [String],
    builtin_functions: &'a [String],
    current_loop_depth: usize,
    current_cursor_depth: usize,
    current_savepoint_depth: usize,
    source_tables: HashSet<String>,
}

impl<'a> PlComplexityVisitor<'a> {
    pub fn new(
        metrics: &'a mut ComplexityMetrics,
        custom_functions: &'a [String],
        high_weight_tables: &'a [String],
        high_weight_procedures: &'a [String],
        builtin_functions: &'a [String],
    ) -> Self { ... }

    pub fn visit_block(&mut self, block: &PlBlock) {
        self.process_declarations(&block.declarations);
        self.process_statements(&block.body);
        if let Some(exc) = &block.exception_block {
            // Exception blocks create implicit subtransactions
            self.metrics.subtransaction_count += 1;
            for handler in &exc.handlers {
                self.process_statements(&handler.statements);
            }
        }
    }

    fn process_declarations(&mut self, decls: &[PlDeclaration]) {
        for decl in decls {
            match decl {
                PlDeclaration::Cursor(c) => {
                    self.metrics.cursor_count += 1;
                }
                PlDeclaration::Pragma { name, .. } => {
                    if name.to_lowercase().contains("autonomous_transaction") {
                        self.metrics.uses_autonomous_transactions = true;
                    }
                }
                PlDeclaration::NestedProcedure(p) => {
                    if let Some(block) = &p.block {
                        self.visit_block(block);
                    }
                }
                PlDeclaration::NestedFunction(f) => {
                    if let Some(block) = &f.block {
                        self.visit_block(block);
                    }
                }
                _ => {}
            }
        }
    }

    fn process_statements(&mut self, stmts: &[PlStatement]) {
        for stmt in stmts {
            self.process_statement(stmt);
        }
    }

    fn process_statement(&mut self, stmt: &PlStatement) {
        match stmt {
            PlStatement::Loop(l) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&l.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::While(w) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&w.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::For(f) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&f.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::ForEach(f) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&f.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::ForAll(_) => {
                self.metrics.loop_count += 1;
            }
            PlStatement::Block(b) => {
                self.current_cursor_depth += 1;
                self.visit_block(b);
                self.current_cursor_depth -= 1;
            }
            PlStatement::Open(o) => {
                self.metrics.cursor_operation_count += 1;
                if matches!(o.kind, PlOpenKind::ForQuery { .. }) {
                    self.metrics.dynamic_sql_count += 1;
                }
            }
            PlStatement::Fetch(_) => {
                self.metrics.cursor_operation_count += 1;
            }
            PlStatement::Close { .. } => {
                self.metrics.cursor_operation_count += 1;
            }
            PlStatement::Move { .. } => {
                self.metrics.cursor_operation_count += 1;
            }
            PlStatement::Execute(e) => {
                self.metrics.dynamic_sql_count += 1;
                if !e.into_targets.is_empty() {
                    self.metrics.dynamic_sql_count += 0; // count is already incremented
                }
                self.metrics.param_binding_count += e.using_args.len();
                // Check for nested dynamic SQL in the parsed query
                if e.parsed_query.is_some() {
                    self.metrics.nested_dynamic_sql_count += 1;
                }
            }
            PlStatement::Commit => {
                self.metrics.transaction_control_count += 1;
            }
            PlStatement::Rollback { to_savepoint } => {
                self.metrics.transaction_control_count += 1;
                if to_savepoint.is_some() {
                    // ROLLBACK TO counts as subtransaction
                    self.metrics.subtransaction_count += 1;
                }
            }
            PlStatement::Savepoint { .. } => {
                self.metrics.transaction_control_count += 1;
                self.metrics.subtransaction_count += 1;
                self.current_savepoint_depth += 1;
                self.update_max_savepoint_depth();
                self.current_savepoint_depth -= 1;
            }
            PlStatement::ProcedureCall(call) => {
                // Check against custom functions
                let name_str = call.name.last().map(|s| s.to_lowercase()).unwrap_or_default();
                if self.custom_functions.iter().any(|f| f.to_lowercase() == name_str) {
                    self.metrics.custom_function_count += 1;
                } else if !self.is_builtin(&name_str) {
                    self.metrics.nested_procedure_count += 1;
                    if self.high_weight_procedures.iter().any(|p| p.to_lowercase() == name_str) {
                        self.metrics.high_weight_procedure_count += 1;
                    }
                }
            }
            PlStatement::Sql(s) | PlStatement::SqlStatement { sql_text: s, .. } => {
                // SQL within PL block — count as dynamic SQL source
                self.metrics.line_count += s.lines().count();
            }
            // IF, CASE, etc. recurse into their bodies
            PlStatement::If(i) => {
                self.process_statements(&i.then_stmts);
                for e in &i.elsifs {
                    self.process_statements(&e.stmts);
                }
                self.process_statements(&i.else_stmts);
            }
            PlStatement::Case(c) => {
                for w in &c.whens {
                    self.process_statements(&w.stmts);
                }
                self.process_statements(&c.else_stmts);
            }
            _ => {}
        }
    }

    fn update_max_loop_depth(&mut self) {
        if self.current_loop_depth > self.metrics.max_loop_nesting_level {
            self.metrics.max_loop_nesting_level = self.current_loop_depth;
        }
    }

    fn update_max_savepoint_depth(&mut self) {
        if self.current_savepoint_depth > self.metrics.max_subtransaction_nesting_level {
            self.metrics.max_subtransaction_nesting_level = self.current_savepoint_depth;
        }
    }

    fn is_builtin(&self, name: &str) -> bool {
        self.builtin_functions.iter().any(|f| f.to_lowercase() == name)
    }
}
```

---

## Task 4: Rewrite Scoring Engine (`engine.rs`)

**Files:**
- Modify: `crates/ogsql-complexity/src/engine.rs`

**Goal:** Implement the full GaussDB 11-step scoring formula. Keep the existing `compute_score`/`analyze` functions for backward compatibility, add new GaussDB-specific scoring functions.

### Key implementation:

Add new public functions:

```rust
/// Score a standalone SQL statement (GaussDB formula).
pub fn gauss_score_statement(metrics: &ComplexityMetrics) -> i64 {
    use gauss_weights::*;
    (metrics.table_count as i64 * TABLE)
        + (metrics.join_count as i64 * JOIN)
        + (metrics.where_condition_count as i64 * WHERE_CONDITION)
        + (metrics.subquery_count as i64 * SUBQUERY)
        + (metrics.aggregate_function_count as i64 * AGGREGATE_FUNCTION)
        + (metrics.case_expression_count as i64 * CASE_EXPRESSION)
        + (metrics.set_operation_count as i64 * SET_OPERATION)
        + (if metrics.has_group_by { GROUP_BY } else { 0 })
        + (if metrics.has_order_by { ORDER_BY } else { 0 })
        + (metrics.hint_count as i64 * HINT)
}

/// Score a non-SELECT statement (GaussDB: tableCount × 10 + hintCount × 3).
pub fn gauss_score_non_select(metrics: &ComplexityMetrics) -> i64 {
    use gauss_weights::*;
    (metrics.table_count as i64 * TABLE) + (metrics.hint_count as i64 * HINT)
}

/// Score a CREATE TABLE statement.
pub fn gauss_score_create_table(metrics: &ComplexityMetrics) -> i64 {
    use gauss_weights::*;
    TABLE_WEIGHT
        + (metrics.column_count as i64 * COLUMN)
        + (metrics.computed_column_count as i64 * COMPUTED_COLUMN)
        + (metrics.check_constraint_count as i64 * CHECK_CONSTRAINT)
}

/// Full GaussDB stored procedure scoring (11-step formula).
pub fn gauss_score_procedure(
    sql_statement_scores: &[i64],
    metrics: &ComplexityMetrics,
    package_metrics: Option<&PackageMetrics>,
) -> GaussDbComplexityReport {
    use gauss_weights::*;

    let mut breakdown = GaussDbScoreBreakdown::default();

    // Step 1: Sum of SQL statement scores
    breakdown.sql_statements_sum = sql_statement_scores.iter().sum();

    // Step 2: Loop complexity
    breakdown.loop_complexity =
        (metrics.loop_count as i64 * LOOP) + (metrics.max_loop_nesting_level as i64 * NESTED_LOOP);

    // Step 3: Custom functions
    breakdown.custom_function_complexity = metrics.custom_function_count as i64 * CUSTOM_FUNCTION;

    // Step 4: High-weight tables
    breakdown.high_weight_table_complexity = metrics.high_weight_table_count as i64 * HIGH_WEIGHT_TABLE;

    // Step 5: Nested procedures
    breakdown.nested_procedure_complexity = metrics.nested_procedure_count as i64 * NESTED_PROCEDURE;

    // Step 6: High-weight procedures
    if metrics.high_weight_procedure_count > 0 {
        breakdown.high_weight_procedure_complexity =
            metrics.high_weight_procedure_count as i64 * HIGH_WEIGHT_PROCEDURE;
    }

    // Step 7: Cursor complexity
    let cursor_base = (metrics.cursor_count as i64 * CURSOR_DECLARATION)
        + (metrics.cursor_operation_count as i64 * CURSOR_OPERATION);
    breakdown.cursor_complexity = if metrics.max_cursor_nesting_level > 1 {
        let multiplier = 1 + (metrics.max_cursor_nesting_level as i64 - 1) * NESTED_CURSOR;
        cursor_base * multiplier
    } else {
        cursor_base
    };

    // Pre-enhanced score (steps 1-7)
    let pre_enhanced = breakdown.sql_statements_sum
        + breakdown.loop_complexity
        + breakdown.custom_function_complexity
        + breakdown.high_weight_table_complexity
        + breakdown.nested_procedure_complexity
        + breakdown.high_weight_procedure_complexity
        + breakdown.cursor_complexity;

    // Step 8: Enhanced complexity override
    breakdown.enhanced_complexity = (metrics.table_count as i64 * TABLE)
        + (metrics.join_count as i64 * JOIN)
        + (metrics.where_condition_count as i64 * WHERE_CONDITION)
        + (metrics.subquery_count as i64 * SUBQUERY)
        + (metrics.set_operation_count as i64 * SET_OPERATION)
        + (metrics.loop_count as i64 * LOOP);

    let mut base_score = breakdown.enhanced_complexity;

    // Step 9: Additional GaussDB items
    breakdown.dynamic_sql_complexity = metrics.dynamic_sql_count as i64 * DYNAMIC_SQL;
    breakdown.param_binding_complexity = metrics.param_binding_count as i64 * PARAMETER_BINDING;
    breakdown.nested_dynamic_sql_complexity = metrics.nested_dynamic_sql_count as i64 * NESTED_DYNAMIC_SQL;
    breakdown.transaction_complexity =
        (metrics.transaction_control_count as i64 * TRANSACTION_CONTROL)
            + (metrics.transaction_nesting_level as i64 * NESTED_TRANSACTION);
    breakdown.autonomous_transaction_bonus = if metrics.uses_autonomous_transactions {
        AUTONOMOUS_TRANSACTION
    } else {
        0
    };
    breakdown.java_procedure_complexity = metrics.java_stored_procedure_count as i64 * JAVA_PROCEDURE;
    breakdown.java_type_conversion_complexity = metrics.java_type_conversion_count as i64 * TYPE_CONVERSION;
    breakdown.hint_complexity = metrics.hint_count as i64 * HINT;

    if let Some(pm) = package_metrics {
        breakdown.package_complexity = (pm.total_procedures as i64 * 5)
            + (pm.package_level_variables as i64 * 2)
            + if pm.contains_java_procedures { JAVA_PROCEDURE } else { 0 };
    }

    base_score += breakdown.dynamic_sql_complexity
        + breakdown.param_binding_complexity
        + breakdown.nested_dynamic_sql_complexity
        + breakdown.transaction_complexity
        + breakdown.autonomous_transaction_bonus
        + breakdown.java_procedure_complexity
        + breakdown.java_type_conversion_complexity
        + breakdown.hint_complexity
        + breakdown.package_complexity;

    // Step 10: Final score (integer)
    let overall_score = base_score;

    // Step 11: Java minimum score guarantee
    let final_score = if metrics.java_stored_procedure_count > 0 {
        std::cmp::max(overall_score, 50)
    } else {
        overall_score
    };

    GaussDbComplexityReport {
        input_kind: InputKind::StoredProcedure,
        statement_scores: vec![], // populated by caller
        pl_metrics: metrics.clone(),
        pre_enhanced_score: pre_enhanced,
        overall_score: final_score,
        level: ComplexityLevel::from_score(final_score as f64),
        score_breakdown: breakdown,
    }
}
```

---

## Task 5: Update `lib.rs` and Write Tests

**Files:**
- Modify: `crates/ogsql-complexity/src/lib.rs`
- Modify: `crates/ogsql-complexity/tests/complexity_tests.rs`

### Step 1: Update lib.rs

Add new public exports:
```rust
pub mod pl_visitor;

pub use engine::{
    analyze, gauss_score_statement, gauss_score_non_select,
    gauss_score_create_table, gauss_score_procedure,
};
pub use model::{
    ComplexityConfig, ComplexityLevel, ComplexityMetrics, ComplexityReport,
    GaussDbComplexityReport, GaussDbScoreBreakdown, InputKind,
    PackageMetrics, StatementComplexity, StatementTypeMultiplier,
    WeightProfile,
};
```

### Step 2: Add GaussDB-specific test cases

At minimum, add these tests to `complexity_tests.rs`:

1. **test_gauss_select_score** — Verify a simple SELECT scores `1*10 + 1*5 = 15` (1 table + WHERE)
2. **test_gauss_join_score** — Verify `2*10 + 1*15 + 1*5 = 40` (2 tables + 1 join + WHERE)
3. **test_gauss_non_select_score** — Verify INSERT scores `1*10 + 0 = 10` (1 table, no hints)
4. **test_gauss_non_select_with_hint** — Verify INSERT with hint scores `1*10 + 1*3 = 13`
5. **test_gauss_create_table_score** — Verify CREATE TABLE with 5 columns + 1 CHECK scores `10 + 5*2 + 1*10 = 30`
6. **test_gauss_where_exists_only** — Verify WHERE condition count is 1 (not AND/OR count)
7. **test_gauss_hint_counting** — Verify `/*+ tablescan(t1) */` counts as 1 hint
8. **test_pl_loop_counting** — Verify LOOP/WHILE/FOR each counted
9. **test_pl_loop_nesting** — Verify nested loops produce correct max depth
10. **test_pl_cursor_counting** — Verify cursor declarations + operations
11. **test_pl_dynamic_sql** — Verify EXECUTE IMMEDIATE counting + USING args
12. **test_pl_transaction_control** — Verify COMMIT/ROLLBACK/SAVEPOINT counting
13. **test_pl_autonomous_transaction** — Verify PRAGMA detection
14. **test_gauss_procedure_full_formula** — End-to-end stored procedure scoring
15. **test_gauss_java_minimum_score** — Verify Java SP minimum 50 score
16. **test_custom_functions** — Verify user-provided function matching
17. **test_high_weight_tables** — Verify high-weight table detection
18. **test_nested_procedures** — Verify nested procedure call detection
19. **test_complex_stored_procedure** — Snapshot test for a realistic SP

---

## Task 6: Verify Build + Tests

**Steps:**
1. `cargo build -p ogsql-complexity` — ensure it compiles
2. `cargo test -p ogsql-complexity` — ensure all tests pass
3. `cargo clippy -p ogsql-complexity` — ensure no warnings
4. `cargo fmt --all -- --check` — ensure formatting is clean
