# SUBQ-006: 关联子查询自引用 UPDATE 检测与 SQL 重写

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 添加 SUBQ-006 诊断规则检测关联子查询自引用 UPDATE 反模式，提供模板建议，并在原始 SQL 可用时通过 AST 重写生成精确的修复 SQL。

**Architecture:** 三层递进。Phase 1 新增 `subquery_rules.rs` 中的 SUBQ-006 规则（EXPLAIN 检测 + 模板建议）。Phase 2 扩展 ogsql-parser 支持行构造器赋值（`SET (col1,col2) = (...)`）。Phase 3 在 ogexplain-core 中新增 `rewriter` 模块，利用 ogsql-parser AST 实现精确 SQL 重写。

**Tech Stack:** Rust, ogexplain-core (analyzer rules), ogsql-parser (SQL AST), SqlFormatter (SQL 输出)

**前置依赖:** ogsql-parser issue #164 (https://github.com/c2j/ogsql-parser/issues/164) — 行构造器 UPDATE 支持。Phase 1/2 不依赖此 issue，Phase 3 的行构造器模式依赖。

---

## Phase 1: SUBQ-006 规则 + EXPLAIN 模板建议

> 无外部依赖，可立即开始。

### Task 1.1: 创建测试 fixture

**Files:**
- Create: `tests/fixtures/23_correlated_subquery_update.txt`
- Create: `tests/fixtures/24_correlated_subquery_update_distributed.txt`

**Step 1: 创建基础关联子查询 UPDATE fixture**

这是关联子查询自引用 UPDATE 的典型 EXPLAIN 输出。关键信号：
- 顶层是 `Update on employees`
- 子树含 `SubPlan 1`（作为 Raw 属性）
- `Index Scan` 引用同一张表 `employees`
- `Index Cond: (emp_id = employees.emp_id)` — 关联条件引用外表

```plain
QUERY PLAN
----------------------------------------------------
Update on employees  (cost=0.00..35000.00 rows=1000 width=100)
  ->  Seq Scan on employees  (cost=0.00..15.00 rows=1000 width=50) (actual time=0.010..5.230 rows=1000 loops=1)
        SubPlan 1
          ->  Index Scan using emp_pkey on employees e  (cost=0.00..35.00 rows=1 width=20) (actual time=0.003..0.005 rows=1 loops=1000)
                Index Cond: (emp_id = employees.emp_id)
Total runtime: 5023.456 ms
```

**Step 2: 创建分布式场景 fixture**

包含 Streaming(BROADCAST/REDISTRIBUTE) 信号：

```plain
QUERY PLAN
----------------------------------------------------
Update on orders  (cost=0.00..150000.00 rows=50000 width=120)
  ->  Streaming(type: REDISTRIBUTE dop: 1/1)  (cost=0.00..50000.00 rows=50000 width=80)
        ->  Seq Scan on orders  (cost=0.00..10000.00 rows=50000 width=80) (actual time=0.010..20.500 rows=50000 loops=1)
              SubPlan 1
                ->  Index Scan using orders_pkey on orders o  (cost=0.00..3.00 rows=1 width=40) (actual time=0.002..0.004 rows=1 loops=50000)
                      Index Cond: (order_id = orders.order_id)
Total runtime: 85420.123 ms
```

**Step 3: 创建负面 fixture（不应触发）**

命名为 `tests/fixtures/25_normal_update.txt`，普通 UPDATE 不含关联子查询：

```plain
QUERY PLAN
----------------------------------------------------
Update on employees  (cost=0.00..25.00 rows=1000 width=50)
  ->  Seq Scan on employees  (cost=0.00..15.00 rows=1000 width=50)
Total runtime: 1.234 ms
```

### Task 1.2: 激活已有的 subquery_rules 模块

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/mod.rs`

**Step 1: 注册 subquery_rules 模块和现有规则**

在 `mod.rs` 中添加 `mod subquery_rules;` 并将已有的 `SubqueryNotPulledUp` (SUBQ-001) 和 `LargeInListNotConverted` (REW-001) 注册到 `all_rules()`：

```rust
// mod.rs — 添加模块声明
mod subquery_rules;

// 在 all_rules() 中添加
Box::new(subquery_rules::SubqueryNotPulledUp),
Box::new(subquery_rules::LargeInListNotConverted),
```

**Step 2: 运行已有测试确认不破坏**

Run: `cargo test --workspace`
Expected: 所有 30 个测试仍然通过

### Task 1.3: 实现 SUBQ-006 规则

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs`

**Step 1: 编写失败测试**

在 `tests/analyzer_tests.rs` 中添加：

```rust
// ---------------------------------------------------------------------------
// SUBQ-006 — Correlated subquery self-referencing UPDATE
// ---------------------------------------------------------------------------

#[test]
fn subq_006_triggers_on_correlated_subquery_update() {
    let report = analyze_fixture("23_correlated_subquery_update.txt");
    let finding = get_finding(&report, "SUBQ-006")
        .expect("Expected SUBQ-006 for correlated subquery self-referencing UPDATE");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::SubqueryStructure);
    assert!(finding.detail.contains("employees"), "detail should mention table name");
}

#[test]
fn subq_006_does_not_trigger_on_normal_update() {
    let report = analyze_fixture("25_normal_update.txt");
    assert!(
        !has_finding(&report, "SUBQ-006"),
        "SUBQ-006 should not fire for normal UPDATE without correlated subquery"
    );
}

#[test]
fn subq_006_finding_contains_template_suggestion() {
    let report = analyze_fixture("23_correlated_subquery_update.txt");
    let finding = get_finding(&report, "SUBQ-006").expect("SUBQ-006 should be present");
    let suggestion = finding.suggestion.as_ref().expect("SUBQ-006 should have a suggestion");
    assert!(
        suggestion.contains("UPDATE") && suggestion.contains("FROM"),
        "suggestion should include UPDATE FROM rewrite template, got: {}",
        suggestion
    );
    assert!(
        suggestion.contains("employees"),
        "suggestion should include actual table name"
    );
}

#[test]
fn subq_006_triggers_with_streaming_in_distributed() {
    let report = analyze_fixture("24_correlated_subquery_update_distributed.txt");
    let finding = get_finding(&report, "SUBQ-006")
        .expect("Expected SUBQ-006 for distributed correlated subquery UPDATE");
    assert!(
        finding.detail.contains("Streaming") || finding.detail.contains("分布式"),
        "detail should mention distributed scenario"
    );
}
```

**Step 2: 运行测试确认失败**

Run: `cargo test subq_006`
Expected: 编译失败（SUBQ-006 规则不存在）

**Step 3: 实现 SUBQ-006 规则**

在 `subquery_rules.rs` 中添加 `CorrelatedSubquerySelfUpdate` 结构体。规则逻辑：

1. `check()` 方法（逐节点）：
   - 匹配 `NodeType::Update` 或 `NodeType::ModifyTable` 节点
   - 遍历子树收集信号：SubPlan 属性、NestedLoop、同表 relation
   - 如果有 SubPlan + 同表 relation → 返回 Warning Finding

2. `check_global()` 方法（全局）：
   - 检测整个计划树中 Update/ModifyTable 下是否有 Streaming(BROADCAST/REDISTRIBUTE)
   - 如果有，升级严重度或在 detail 中标注分布式风险

3. 同表检测逻辑：
   - 从 Update/ModifyTable 节点提取目标表名（`node.relation` 或第一个子节点）
   - 递归检查子树中所有 Scan 节点的 `relation`
   - 比较表名是否相同（忽略别名，即 `employees` vs `employees e` 取第一个标识符）

4. 建议模板生成：
   - 提取表名、关联条件列名（从 Index Cond 解析）
   - 生成 UPDATE FROM 和 CTE 两种改写模板
   - 使用实际表名列名填充模板

```rust
// 检测逻辑伪代码
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    // 1. 必须是 DML 节点
    if !matches!(node.node_type, NodeType::Update | NodeType::ModifyTable | NodeType::VectorUpdate) {
        return None;
    }

    // 2. 提取目标表名
    let target_table = node.relation.as_deref()
        .or_else(|| first_child_relation(node))?;

    // 3. 递归检查子树
    let signals = collect_signals(node, target_table);

    // 4. 必须有 SubPlan
    if !signals.has_subplan { return None; }

    // 5. 必须有同表引用
    if !signals.same_table_scan { return None; }

    // 6. 生成 Finding（带模板建议）
    let detail = format_detail(target_table, &signals);
    let suggestion = build_rewrite_template(target_table, &signals.correlation_column);
    Some(make_finding(self, detail, node, Some(suggestion)))
}
```

**Step 4: 运行测试确认通过**

Run: `cargo test subq_006`
Expected: 全部 4 个测试通过

**Step 5: 运行全量测试**

Run: `cargo test --workspace`
Expected: 所有测试通过（包括之前的 30 个 + 新增 4 个）

**Step 6: 提交**

```bash
git add tests/fixtures/23_correlated_subquery_update.txt \
        tests/fixtures/24_correlated_subquery_update_distributed.txt \
        tests/fixtures/25_normal_update.txt \
        crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs \
        crates/ogexplain-core/src/analyzer/rules/mod.rs \
        tests/analyzer_tests.rs
git commit -m "feat: add SUBQ-006 correlated subquery self-referencing UPDATE detection"
```

### Task 1.4: 增强 SuggestionEngine 集成

**Files:**
- Modify: `crates/ogexplain-core/src/suggester/mapper.rs`

**Step 1: 添加 SUBQ-006 → QueryRewrite 建议映射**

在 `SuggestionEngine::suggest()` 中添加对 SUBQ-006 finding 的处理：

```rust
// SUBQ-006 触发时，添加 QueryRewrite 类型的建议
let subq_findings: Vec<&Finding> = findings
    .iter()
    .filter(|f| f.rule_id == "SUBQ-006")
    .collect();
if !subq_findings.is_empty() {
    suggestions.push(Suggestion {
        related_rules: subq_findings.iter().map(|f| f.rule_id.clone()).collect(),
        category: SuggestionCategory::QueryRewrite,
        message: "检测到关联子查询自引用UPDATE，建议改写为 UPDATE ... FROM 或 CTE 形式以避免逐行执行".to_string(),
        confidence: 0.9,
    });
}
```

**Step 2: 编写失败测试 → 实现 → 通过**

在 `tests/analyzer_tests.rs` 添加测试：

```rust
#[test]
fn suggestion_engine_produces_query_rewrite_for_subq006() {
    let report = analyze_fixture("23_correlated_subquery_update.txt");
    let suggestions = SuggestionEngine::suggest(&report.findings);
    let qr = suggestions.iter().find(|s| matches!(s.category, SuggestionCategory::QueryRewrite));
    assert!(qr.is_some(), "SUBQ-006 should produce a QueryRewrite suggestion");
    assert!(qr.unwrap().confidence >= 0.85);
}
```

**Step 3: 提交**

```bash
git add crates/ogexplain-core/src/suggester/mapper.rs tests/analyzer_tests.rs
git commit -m "feat: integrate SUBQ-006 with SuggestionEngine QueryRewrite category"
```

---

## Phase 2: 扩展 ogsql-parser 行构造器赋值

> 需要修改 ogsql-parser 仓库。依赖 issue #164。

### Task 2.1: 扩展 AST 类型

**Files:**
- Modify: `ogsql-parser/src/ast/mod.rs`

**Step 1: 编写 AST 变更的失败测试**

在 ogsql-parser 的测试文件中添加：

```rust
#[test]
fn parse_update_row_constructor_assignment() {
    let sql = "UPDATE t SET (a, b) = (1, 2)";
    let stmt = parse_statement(sql).unwrap();
    match stmt {
        Statement::Update(u) => {
            assert_eq!(u.assignments.len(), 1);
            // 验证多列赋值
            assert!(matches!(u.assignments[0].target, UpdateTarget::Multiple(_)));
        }
        _ => panic!("Expected Update statement"),
    }
}

#[test]
fn parse_update_row_constructor_subquery() {
    let sql = "UPDATE employees SET (salary, bonus) = (SELECT salary * 1.1, bonus + 100 FROM employees e WHERE e.id = employees.id)";
    let stmt = parse_statement(sql).unwrap();
    match stmt {
        Statement::Update(u) => {
            assert_eq!(u.assignments.len(), 1);
            // 验证值是 Subquery
        }
        _ => panic!("Expected Update statement"),
    }
}

#[test]
fn parse_update_mixed_assignments() {
    let sql = "UPDATE t SET a = 1, (b, c) = (2, 3)";
    let stmt = parse_statement(sql).unwrap();
    match stmt {
        Statement::Update(u) => {
            assert_eq!(u.assignments.len(), 2);
            assert!(matches!(u.assignments[0].target, UpdateTarget::Single(_)));
            assert!(matches!(u.assignments[1].target, UpdateTarget::Multiple(_)));
        }
        _ => panic!("Expected Update statement"),
    }
}

#[test]
fn format_update_row_constructor_roundtrip() {
    let sql = "UPDATE t SET (a, b) = (1, 2)";
    let stmt = parse_statement(sql).unwrap();
    let formatted = SqlFormatter::new().format_statement(&stmt);
    let reparsed = parse_statement(&formatted).unwrap();
    assert_eq!(stmt, reparsed, "parse → format → parse should be stable");
}
```

**Step 2: 运行测试确认失败**

Run: `cargo test -p ogsql-parser parse_update_row_constructor`
Expected: 编译错误（UpdateTarget 类型不存在）

**Step 3: 添加 UpdateTarget 枚举和修改 UpdateAssignment**

在 `ast/mod.rs` 中：

```rust
/// Target of an UPDATE assignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UpdateTarget {
    /// Single column: `SET col = expr`
    Single(ObjectName),
    /// Multiple columns (row constructor): `SET (col1, col2) = expr`
    Multiple(Vec<ObjectName>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateAssignment {
    pub target: UpdateTarget,
    pub value: Expr,
}
```

注意：需要决定是否保留 `column` 字段用于向后兼容。查看 `ogsql-complexity` 中对 `UpdateAssignment` 的使用（`visitor.rs:138-140`），需要同步适配。

**Step 4: 运行测试确认通过**

Run: `cargo test -p ogsql-parser`
Expected: 新增测试通过

**Step 5: 提交 ogsql-parser 修改**

```bash
git add src/ast/mod.rs src/parser/dml.rs src/formatter.rs tests/
git commit -m "feat: support SET (col1, col2) = (...) row constructor UPDATE syntax

Implements #164: UpdateTarget enum replaces single column ObjectName
in UpdateAssignment. Supports:
- SET (a, b) = (1, 2)         -- row constructor values
- SET (a, b) = (SELECT ...)   -- subquery values
- SET a = 1, (b, c) = (2, 3) -- mixed assignments
- Formatter roundtrip stable"
```

### Task 2.2: 更新 ogsql-complexity 适配 AST 变更

**Files:**
- Modify: `crates/ogsql-complexity/src/visitor.rs`

**Step 1: 适配 UpdateAssignment 新结构**

```rust
// visitor.rs — 修改 assignments 遍历部分
Statement::Update(u) => {
    // ...
    for assignment in &u.assignments {
        match &assignment.target {
            UpdateTarget::Single(name) => {
                self.metrics.column_count += 1;
            }
            UpdateTarget::Multiple(names) => {
                self.metrics.column_count += names.len();
            }
        }
        self.walk_expr(&assignment.value);
    }
    // ...
}
```

**Step 2: 运行 ogsql-complexity 测试**

Run: `cargo test -p ogsql-complexity`
Expected: 全部通过

**Step 3: 提交**

```bash
git add crates/ogsql-complexity/src/visitor.rs
git commit -m "chore: adapt ogsql-complexity to UpdateTarget enum in ogsql-parser"
```

---

## Phase 3: AST 精确 SQL 重写

> 依赖 Phase 1 和 Phase 2 完成。

### Task 3.1: 新增 rewriter 模块基础结构

**Files:**
- Create: `crates/ogexplain-core/src/rewriter/mod.rs`
- Create: `crates/ogexplain-core/src/rewriter/types.rs`
- Create: `crates/ogexplain-core/src/rewriter/detector.rs`
- Create: `crates/ogexplain-core/src/rewriter/transform.rs`
- Modify: `crates/ogexplain-core/Cargo.toml` (添加 ogsql-parser 依赖)
- Modify: `crates/ogexplain-core/src/lib.rs` (导出 rewriter 模块)

**Step 1: 添加 ogsql-parser 依赖到 ogexplain-core**

在 `crates/ogexplain-core/Cargo.toml` 中添加：

```toml
[dependencies]
ogsql-parser = { git = "https://github.com/c2j/ogsql-parser" }
```

**Step 2: 定义 rewriter 类型**

`crates/ogexplain-core/src/rewriter/types.rs`:

```rust
use serde::Serialize;

/// Detected anti-pattern in a SQL statement.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AntiPatternInfo {
    /// Target table name
    pub target_table: String,
    /// Subquery table name (same as target for self-referencing)
    pub subquery_table: String,
    /// Correlation column(s)
    pub correlation_columns: Vec<String>,
    /// SET columns from the assignment
    pub set_columns: Vec<String>,
    /// Whether row constructor syntax is used
    pub uses_row_constructor: bool,
}

/// Result of a SQL rewrite operation.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RewriteResult {
    /// The rewrite strategy applied
    pub strategy: RewriteStrategy,
    /// The rewritten SQL statement
    pub rewritten_sql: String,
    /// Human-readable explanation of what changed
    pub explanation: String,
    /// The detected anti-pattern info
    pub pattern_info: AntiPatternInfo,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum RewriteStrategy {
    /// UPDATE t SET ... FROM (SELECT ...) sub WHERE t.key = sub.key
    UpdateFrom,
    /// WITH cte AS (SELECT ...) UPDATE t SET ... FROM cte sub WHERE t.key = sub.key
    UpdateCte,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum RewriteError {
    /// SQL parsing failed
    ParseError(String),
    /// The anti-pattern was not detected in the AST
    PatternNotFound,
    /// The SQL uses unsupported syntax (e.g., multi-table UPDATE)
    UnsupportedSyntax(String),
}
```

**Step 3: 编写 detector 的失败测试**

`crates/ogexplain-core/src/rewriter/detector.rs` 负责从 AST 检测反模式：

```rust
use ogsql_parser::ast::*;

/// Detect correlated subquery self-referencing UPDATE in a parsed AST.
pub fn detect_correlated_subquery_update(stmt: &Statement) -> Result<Option<AntiPatternInfo>, RewriteError> {
    let update = match stmt {
        Statement::Update(u) => u,
        _ => return Ok(None),
    };

    let target_table = extract_table_name(&update.tables)
        .ok_or(RewriteError::UnsupportedSyntax("无法提取目标表名".into()))?;

    for assignment in &update.assignments {
        let subquery = extract_subquery_from_value(&assignment.value);
        if let Some(subquery) = subquery {
            let subquery_tables = extract_from_tables(&subquery.from);
            if subquery_tables.contains(&target_table) {
                // 找到关联子查询自引用
                let correlation_columns = extract_correlation_columns(
                    &subquery.where_clause,
                    &target_table,
                );
                let set_columns = extract_set_columns(&assignment.target);
                return Ok(Some(AntiPatternInfo {
                    target_table: target_table.clone(),
                    subquery_table: target_table,
                    correlation_columns,
                    set_columns,
                    uses_row_constructor: matches!(assignment.target, UpdateTarget::Multiple(_)),
                }));
            }
        }
    }

    Ok(None)
}
```

**Step 4: 编写 transform 的失败测试**

`crates/ogexplain-core/src/rewriter/transform.rs` 负责 AST 变换：

```rust
use ogsql_parser::ast::*;
use ogsql_parser::formatter::SqlFormatter;

/// Rewrite a correlated subquery UPDATE to UPDATE...FROM form.
pub fn rewrite_update_from(stmt: &Statement) -> Result<RewriteResult, RewriteError> {
    // 1. 检测反模式
    let info = detect_correlated_subquery_update(stmt)?
        .ok_or(RewriteError::PatternNotFound)?;

    // 2. 克隆 AST 并变换
    let mut rewritten = stmt.clone();
    let update = match &mut rewritten {
        Statement::Update(u) => u,
        _ => unreachable!(),
    };

    // 3. 提取子查询，构造 FROM 子查询
    let assignment = &mut update.assignments[0]; // 简化：处理第一个赋值
    let subquery = extract_subquery_from_value(&assignment.value)
        .ok_or(RewriteError::PatternNotFound)?;

    // 4. 在子查询中添加关联列到 SELECT 列表
    let subquery_alias = "t".to_string();
    let col_prefix = subquery_alias.clone();

    // 5. 修改 assignment.value 为列引用
    // 6. 添加 WHERE 子句
    // 7. 将子查询移到 FROM 子句

    let formatter = SqlFormatter::new();
    let rewritten_sql = formatter.format_statement(&rewritten);

    Ok(RewriteResult {
        strategy: RewriteStrategy::UpdateFrom,
        rewritten_sql,
        explanation: format!("将关联子查询 UPDATE 改写为 UPDATE ... FROM 形式"),
        pattern_info: info,
    })
}
```

**Step 5: 编写集成测试**

在 `tests/` 目录下创建 `rewriter_tests.rs`：

```rust
use ogexplain_core::rewriter::types::*;
use ogsql_parser::parser::Parser;

fn parse_sql(sql: &str) -> ogsql_parser::ast::Statement {
    Parser::parse_sql(sql).unwrap().into_iter().next().unwrap()
}

#[test]
fn detect_single_column_correlated_update() {
    let sql = "UPDATE employees SET salary = (SELECT salary * 1.15 FROM employees e WHERE e.emp_id = employees.emp_id)";
    let stmt = parse_sql(sql);
    let result = ogexplain_core::rewriter::detector::detect_correlated_subquery_update(&stmt)
        .expect("detection should succeed");
    let info = result.expect("should detect anti-pattern");
    assert_eq!(info.target_table, "employees");
    assert_eq!(info.subquery_table, "employees");
    assert!(info.correlation_columns.contains(&"emp_id".to_string()));
    assert!(!info.uses_row_constructor);
}

#[test]
fn detect_row_constructor_correlated_update() {
    let sql = "UPDATE employees SET (salary, bonus) = (SELECT salary * 1.15, bonus + 100 FROM employees e WHERE e.emp_id = employees.emp_id)";
    let stmt = parse_sql(sql);
    let result = ogexplain_core::rewriter::detector::detect_correlated_subquery_update(&stmt)
        .expect("detection should succeed");
    let info = result.expect("should detect anti-pattern");
    assert_eq!(info.target_table, "employees");
    assert!(info.uses_row_constructor);
    assert_eq!(info.set_columns, vec!["salary", "bonus"]);
}

#[test]
fn no_detection_for_normal_update() {
    let sql = "UPDATE employees SET salary = 50000 WHERE dept = 'engineering'";
    let stmt = parse_sql(sql);
    let result = ogexplain_core::rewriter::detector::detect_correlated_subquery_update(&stmt)
        .expect("detection should succeed");
    assert!(result.is_none(), "should not detect anti-pattern for normal UPDATE");
}

#[test]
fn rewrite_single_column_to_update_from() {
    let sql = "UPDATE employees SET salary = (SELECT salary * 1.15 FROM employees e WHERE e.emp_id = employees.emp_id)";
    let stmt = parse_sql(sql);
    let result = ogexplain_core::rewriter::transform::rewrite_update_from(&stmt)
        .expect("rewrite should succeed");
    assert_eq!(result.strategy, RewriteStrategy::UpdateFrom);
    assert!(result.rewritten_sql.contains("FROM"));
    assert!(result.rewritten_sql.contains("employees"));
    assert!(result.rewritten_sql.contains("WHERE"));
    // 重写后的 SQL 应该可以再次解析
    let reparsed = parse_sql(&result.rewritten_sql);
    assert!(matches!(reparsed, ogsql_parser::ast::Statement::Update(_)));
}

#[test]
fn rewrite_row_constructor_to_update_from() {
    let sql = "UPDATE employees SET (salary, bonus) = (SELECT salary * 1.15, bonus + 100 FROM employees e WHERE e.emp_id = employees.emp_id)";
    let stmt = parse_sql(sql);
    let result = ogexplain_core::rewriter::transform::rewrite_update_from(&stmt)
        .expect("rewrite should succeed");
    assert!(result.rewritten_sql.contains("FROM"));
    assert!(result.rewritten_sql.contains("salary"));
    assert!(result.rewritten_sql.contains("bonus"));
}
```

**Step 6: 实现完整逻辑直到测试通过**

逐步实现 `detector.rs` 和 `transform.rs` 中的辅助函数，确保每个测试通过。

**Step 7: 提交**

```bash
git add crates/ogexplain-core/src/rewriter/ \
        crates/ogexplain-core/Cargo.toml \
        crates/ogexplain-core/src/lib.rs \
        tests/rewriter_tests.rs
git commit -m "feat: add SQL rewriter module for correlated subquery UPDATE

Implements AST-based SQL rewriting:
- detector: identifies correlated subquery self-referencing UPDATE pattern
- transform: rewrites to UPDATE...FROM form
- Supports both single-column and row constructor forms
- Roundtrip safe: rewritten SQL can be re-parsed"
```

### Task 3.2: 集成 rewriter 到分析流程

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/report.rs` (扩展 Finding)
- Modify: `crates/ogexplain-core/src/lib.rs` (新增 analyze_with_rewrite API)
- Modify: `crates/ogexplain-cli/src/lib.rs` (CLI 输出重写结果)

**Step 1: 扩展 Finding 支持 SQL 重写结果**

在 `report.rs` 的 `Finding` 中添加：

```rust
pub struct Finding {
    // ... 现有字段 ...
    /// SQL rewrite result (populated when original SQL is available and rewrite succeeds)
    pub sql_rewrite: Option<crate::rewriter::types::RewriteResult>,
}
```

**Step 2: 新增 `analyze_with_rewrite()` API**

在 `lib.rs` 中：

```rust
pub fn analyze_with_rewrite(
    plan: &model::ExplainPlan,
    sql_text: Option<&str>,
) -> analyzer::report::DiagnosticReport {
    let mut report = analyze(plan);

    // 如果有原始 SQL，尝试精确重写
    if let Some(sql) = sql_text {
        for finding in &mut report.findings {
            if finding.rule_id == "SUBQ-006" {
                if let Ok(stmt) = ogsql_parser::parser::Parser::parse_sql(sql) {
                    if let Some(stmt) = stmt.into_iter().next() {
                        if let Ok(Some(_info)) = rewriter::detector::detect_correlated_subquery_update(&stmt) {
                            if let Ok(result) = rewriter::transform::rewrite_update_from(&stmt) {
                                finding.sql_rewrite = Some(result);
                            }
                        }
                    }
                }
            }
        }
    }

    report
}
```

**Step 3: CLI 输出适配**

在 `output_block_with_diag()` 中，检查 finding 是否有 `sql_rewrite`，如果有则在建议后输出重写的 SQL。

**Step 4: 编写端到端测试**

```rust
#[test]
fn analyze_with_rewrite_produces_sql_for_correlated_update() {
    let explain_text = read_fixture("23_correlated_subquery_update.txt");
    let plan = parse(&explain_text).unwrap();
    let sql = "UPDATE employees SET salary = (SELECT salary * 1.15 FROM employees e WHERE e.emp_id = employees.emp_id)";
    let report = analyze_with_rewrite(&plan, Some(sql));
    let finding = report.findings.iter().find(|f| f.rule_id == "SUBQ-006").expect("SUBQ-006 should be present");
    let rewrite = finding.sql_rewrite.as_ref().expect("Should have SQL rewrite");
    assert!(rewrite.rewritten_sql.contains("FROM"));
}
```

**Step 5: 提交**

```bash
git add crates/ogexplain-core/src/analyzer/report.rs \
        crates/ogexplain-core/src/lib.rs \
        crates/ogexplain-cli/src/lib.rs \
        tests/rewriter_tests.rs
git commit -m "feat: integrate SQL rewriter into analysis pipeline

- Extend Finding with sql_rewrite field
- Add analyze_with_rewrite() public API
- CLI displays rewritten SQL when available"
```

---

## 测试清单汇总

| 测试 | 位置 | Phase | 类型 |
|------|------|-------|------|
| `subq_006_triggers_on_correlated_subquery_update` | analyzer_tests.rs | 1 | 正向触发 |
| `subq_006_does_not_trigger_on_normal_update` | analyzer_tests.rs | 1 | 负向守护 |
| `subq_006_finding_contains_template_suggestion` | analyzer_tests.rs | 1 | 模板质量 |
| `subq_006_triggers_with_streaming_in_distributed` | analyzer_tests.rs | 1 | 分布式场景 |
| `suggestion_engine_produces_query_rewrite_for_subq006` | analyzer_tests.rs | 1 | 集成 |
| `parse_update_row_constructor_assignment` | ogsql-parser | 2 | Parser |
| `parse_update_row_constructor_subquery` | ogsql-parser | 2 | Parser |
| `parse_update_mixed_assignments` | ogsql-parser | 2 | Parser |
| `format_update_row_constructor_roundtrip` | ogsql-parser | 2 | Formatter 往返 |
| `detect_single_column_correlated_update` | rewriter_tests.rs | 3 | 检测正向 |
| `detect_row_constructor_correlated_update` | rewriter_tests.rs | 3 | 检测行构造器 |
| `no_detection_for_normal_update` | rewriter_tests.rs | 3 | 检测负向 |
| `rewrite_single_column_to_update_from` | rewriter_tests.rs | 3 | 重写单列 |
| `rewrite_row_constructor_to_update_from` | rewriter_tests.rs | 3 | 重写行构造器 |
| `analyze_with_rewrite_produces_sql` | rewriter_tests.rs | 3 | 端到端 |

总计 **16 个测试**，覆盖正向触发、负向守护、模板质量、分布式场景、集成、Parser、Formatter 往返、检测、重写、端到端。
