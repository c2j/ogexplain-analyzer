# 闭环 SQL 优化 Pilot — ogexplain 侧实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 在 ogexplain-analyzer 中实现 Heptadecagon 闭环 SQL 优化的 ogexplain 侧能力——Finding 结构化字段、收敛检测、诊断→重写映射、`optimize` 子命令端到端编排，并以 SUBQ-001 → subquery-to-join pilot 端到端验证。

**Architecture:** 五阶段递增——(1) Finding 加 `table`/`columns`，迁移 5 条已计算该数据的规则；(2) `convergence` 模块对比指标快照；(3) `optimize` 子命令内嵌的 mapper（避开 `suggester/mapper.rs` 命名冲突）；(4) `optimize` 编排循环 + 安全保护；(5) 端到端测试（静态优先，可选 live DB）。每阶段独立可提交，TDD 驱动。

**Tech Stack:** Rust, ogexplain-core（`analyzer/`、`summary.rs`、`rewriter/`）、ogexplain-cli（`lib.rs` derive `Subcommand`）、ogexplain-mcp（可选暴露 `optimize`）、insta 快照测试、metamorphosis 二进制（子进程调用，非 Cargo 依赖）。

---

## 0. 前置与依赖状态

### 0.1 跨仓库依赖

| 依赖 | 状态 | 对本计划的影响 |
|---|---|---|
| metamorphosis#33 (`RewriteContext.diagnostic_hints`) | PR #35 OPEN，**有 merge conflicts**，零 review | ogexplain 侧 #33 任务 1 不依赖；#34 端到端需该 PR 合入或本地 checkout |
| metamorphosis#34 (regress 框架 + Qed EXISTS/IN decorrelation) | **已 MERGED** | Pilot 文档 Week 2 已完成——本计划可跳过验证补全，直接用 Qed 验证 |
| Heptadecagon pilot 文档 | 已合入 main | 合约清晰，按文档执行 |
| ogexplain 诊断质量 | v4 case-level F1=69.1%，**SUBQ-001 precision 仍为 0.43** | mapper 必须加质量门槛（见任务 3.2） |

### 0.2 PR #35 审核（informational，不在本仓库修复）

**核心改动（合理）：**
- `crates/core/src/types.rs` 新增 `DiagnosticHint { rule_id, table, columns, severity, detail }`
- `crates/core/src/context.rs` `RewriteContext` 新增 `diagnostic_hints: Option<&'a Vec<DiagnosticHint>>`
- 28 处构造站点机械更新（CLI 6 + MCP 2 + tests 20）
- 遵循 `Option<&'a T>` 模式，与 `schema`/`known_variables` 一致

**需在 PR #35 解决的问题（提交评论，不阻塞本计划）：**
1. **范围蔓延**：夹带了 `InlineValue::Cast` 类型字面量支持（`'20260101'::date`），与 #33 无关，应单独 PR
2. **Merge conflicts** 未解（base: main，head: feat/sql-tuning）
3. **无 CI checks**：feat/sql-tuning 分支未配置 CI
4. **`subquery-to-join` 的 hint-aware 改动**仅在 `matches()` 加 `tracing::debug!`，符合 Week 1 "日志 only" 决策，可接受

**契约确认（与 ogexplain 侧一致）：**
- ogexplain `Finding.table: Option<String>` ↔ metamorphosis `DiagnosticHint.table: Option<String>` ✅
- ogexplain `Finding.columns: Vec<String>` ↔ metamorphosis `DiagnosticHint.columns: Vec<String>` ✅
- 字段名、类型、Option-ness 完全对齐，无需适配层

### 0.3 范围

**In Scope：**
- ogexplain #33 任务 1（Finding `table`/`columns` + 5 条规则迁移）
- ogexplain #33 任务 2（convergence 模块）
- ogexplain #33 任务 3（mapper 放 CLI 侧，避开 suggester 命名冲突）
- ogexplain #34（optimize 子命令，含振荡检测、安全保护、Phase 0 占位）
- 端到端测试（静态优先 + 可选 live DB）

**Out of Scope（后续工作）：**
- Phase 0 Stats 检查 + 自动 ANALYZE 完整实现（本计划仅占位）
- QED/VeriEQL 验证集成（metamorphosis#34 已提供，接入留作 follow-up）
- `add-explicit-cast`、`suggest-trgm-index`、`rewrite-group-agg` 新重写规则
- MCP 暴露 `optimize` 工具
- 批量模式 `--batch`
- lib.rs 拆分为多文件（保持单文件，新增模块放 core）

### 0.4 显式假设

- 不修改 `DiagnosticRule` trait 签名
- `make_finding` 签名保持不变，新增 `make_finding_ext` 扩展
- `SummaryRow` 不 derive `PartialEq`（避免 f64-NaN 风险），改用 `MetricsSnapshot` 子集
- metamorphosis 作为子进程调用，**不引入 Cargo 依赖**
- 所有新 pub 项需 doc comment
- 每阶段独立提交，可单独 review

---

## 1. 验证策略

### 1.1 单元测试（每 Task 必须有）

- **正向测试**：应触发结构化字段的样本断言 `finding.table == Some("foo")`、`finding.columns == vec!["bar"]`
- **守护测试**：默认 `make_finding` 产出的 `table == None`、`columns.is_empty()`
- **结构断言**：JSON 序列化后字段存在、`skip_serializing_if` 生效

### 1.2 回归测试

**主基准**：`cargo test --workspace`（含 317+ 现有测试 + per-rule regress 套件 35 case）

**JSON 输出稳定性**：新增字段必须 `#[serde(skip_serializing_if = ...)]`，确保旧消费者的 JSON 不变

### 1.3 端到端测试（Phase 5）

- **静态 E2E**（CI-friendly）：预录 EXPLAIN 文本 + 模拟 re-EXPLAIN，验证编排逻辑
- **Live E2E**（手动/`--features live-db`）：ogagila docker-compose，真实 DB 跑 SUBQ-001 场景

### 1.4 编译与 lint

每阶段完成后：
```bash
cargo build --workspace 2>&1 | grep -E "error|warning" | head
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
cargo fmt --all -- --check
```

---

## 2. 依赖关系

```
Phase 1 (Finding 字段) ─────────────────┐
                                        │
Phase 2 (convergence) ──────────────────┤
                                        ├──→ Phase 4 (optimize) ─→ Phase 5 (E2E)
Phase 3 (mapper in CLI) ────────────────┘
```

- Phase 1/2/3 互相独立，可并行（建议串行以便逐步 review）
- Phase 4 依赖 Phase 1+2+3
- Phase 5 依赖 Phase 4 + metamorphosis PR #35 合入（或本地 checkout）

---

## Phase 1: Finding 结构化字段（5 规则迁移）

**目标**：给 Finding 加 `table`/`columns`，把诊断质量修复已计算但丢弃的数据接到正确出口。

**为什么先做**：零计算成本（数据已被 `extract_column_from_filter` / `find_first_scan_descendant` / `relation` 算出），纯接口整理；后续 mapper/optimize 都依赖这些字段。

**范围**：5 条规则迁移（**不**做全 27 条规则覆盖）：
- SCAN-001（已用 `node.relation`）
- SCAN-004（已用 `extract_column_from_filter`）
- SUBQ-001（已用 `find_first_scan_descendant`）
- TYPE-001（已用 `extract_column_from_filter`）
- JOIN-001（已有 `join_column` 局部变量，`join_rules.rs:42`）

### Task 1.1: Finding 加 `table` / `columns` 字段

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/report.rs:55-68`（Finding struct）

**Step 1: 写失败的测试**

新增 `crates/ogexplain-core/src/analyzer/report_tests.rs`（若不存在）：

```rust
use super::*;
use crate::analyzer::context::GlobalStats;

#[test]
fn finding_table_defaults_to_none() {
    let f = Finding {
        rule_id: "X".into(),
        severity: Severity::Info,
        category: DiagnosticCategory::General,
        title: "t".into(),
        detail: "d".into(),
        node_line: None,
        node_type: None,
        suggestion: None,
        sql_rewrite: None,
        evidence: None,
        table: None,
        columns: Vec::new(),
    };
    assert!(f.table.is_none());
    assert!(f.columns.is_empty());
}

#[test]
fn finding_json_skips_none_table_and_empty_columns() {
    let f = Finding {
        rule_id: "X".into(),
        severity: Severity::Info,
        category: DiagnosticCategory::General,
        title: "t".into(),
        detail: "d".into(),
        node_line: None,
        node_type: None,
        suggestion: None,
        sql_rewrite: None,
        evidence: None,
        table: None,
        columns: Vec::new(),
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(!json.contains("table"), "must skip None table, got: {}", json);
    assert!(!json.contains("columns"), "must skip empty columns, got: {}", json);
}
```

**Step 2: 运行确认失败**

```bash
cargo test -p ogexplain-core --test report_tests
```

预期：编译失败（`table`/`columns` 字段不存在）。

**Step 3: 实现字段**

修改 `report.rs:55-68`，在 `evidence` 之后添加两个字段（与 `evidence` 同款 `#[serde(skip_serializing_if)]`）：

```rust
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub category: DiagnosticCategory,
    pub title: String,
    pub detail: String,
    pub node_line: Option<usize>,
    pub node_type: Option<String>,
    pub suggestion: Option<String>,
    pub sql_rewrite: Option<crate::rewriter::types::RewriteResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<crate::analyzer::pattern::types::Evidence>,
    /// 关联表名（从计划节点提取，用于下游工具定向重写）。
    /// None 表示该规则未提取表名（向后兼容）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// 关联列名（从过滤/连接条件提取）。
    /// 空表示该规则未提取列名（向后兼容）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
}
```

**Step 4: 更新 `make_finding` body**

`crates/ogexplain-core/src/analyzer/rules/mod.rs:83-101`，函数签名不变，body 加两行：

```rust
fn make_finding(
    rule: &dyn DiagnosticRule,
    detail: String,
    node: &PlanNode,
    suggestion: Option<String>,
) -> Finding {
    Finding {
        rule_id: rule.id().to_string(),
        severity: rule.severity(),
        category: rule.category(),
        title: rule.name(),
        detail,
        node_line: Some(node.line_number),
        node_type: Some(node.node_type.to_string()),
        suggestion,
        sql_rewrite: None,
        evidence: None,
        table: None,
        columns: Vec::new(),
    }
}
```

**Step 5: 新增 `make_finding_ext`**

紧邻 `make_finding` 添加：

```rust
/// Extended `make_finding` with structured table/columns metadata.
///
/// Rules that already extract table/column info (SCAN-001 via `node.relation`,
/// SCAN-004/TYPE-001 via `extract_column_from_filter`, SUBQ-001 via
/// `find_first_scan_descendant`, JOIN-001 via join-column detection) should
/// call this instead of `make_finding` to populate the structured fields.
/// Other rules continue using `make_finding` (defaults to `None`/empty).
fn make_finding_ext(
    rule: &dyn DiagnosticRule,
    detail: String,
    node: &PlanNode,
    suggestion: Option<String>,
    table: Option<String>,
    columns: Vec<String>,
) -> Finding {
    let mut f = make_finding(rule, detail, node, suggestion);
    f.table = table;
    f.columns = columns;
    f
}
```

**Step 6: 运行测试确认通过**

```bash
cargo test -p ogexplain-core report_tests -- --nocapture
cargo test --workspace
```

预期：全通过；`make_finding` 调用点零修改（仍编译）。

**Step 7: 提交**

```
feat(analyzer): add Finding.table/columns structured fields

Adds Option<String> table and Vec<String> columns to Finding struct.
Both fields are skip_serializing_if'd to keep JSON output stable for
existing consumers (CLI, MCP, suggester).

make_finding signature unchanged (backward compatible). New
make_finding_ext helper allows rules to populate structured metadata.
Existing 26 call sites continue using make_finding (defaults None/empty).

This unblocks Heptadecagon closed-loop optimization by giving downstream
tools (suggester, mapper, metamorphosis via JSON) programmatic access to
table/column data that rules already compute but currently discard into
i18n strings.

Part of #33.
```

---

### Task 1.2: SUBQ-001 迁移到 `make_finding_ext`

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs:36-91`（SubqueryNotPulledUp::check）
- Test: 同文件 `#[cfg(test)] mod tests`

**Step 1: 写失败的测试**

在 `subquery_rules.rs` 末尾添加 test module（若已有则追加）：

```rust
#[cfg(test)]
mod tests_task_1_2 {
    use super::*;
    use crate::model::{NodeType, PlanNode};

    fn make_subquery_scan_with_table(table: &str) -> PlanNode {
        // 构造 SubqueryScan → SeqScan(table)
        // 参考现有 utils 测试的 fixture 风格
        // 实现者参照 crates/ogexplain-core/src/model/plan.rs::tests
        todo!("construct SubqueryScan wrapping SeqScan on `table`")
    }

    #[test]
    fn subq_001_populates_table_field() {
        let node = make_subquery_scan_with_table("items");
        let finding = SubqueryNotPulledUp.check(&node, &Default::default()).unwrap();
        assert_eq!(finding.table.as_deref(), Some("items"));
        assert!(finding.columns.is_empty(), "SUBQ-001 does not extract columns yet");
    }

    #[test]
    fn subq_001_table_unknown_when_no_scan_descendant() {
        // SubqueryScan → Sort → Result (无 scan)
        let node = make_subquery_scan_no_scan();
        let finding = SubqueryNotPulledUp.check(&node, &Default::default()).unwrap();
        // 当前 find_first_scan_descendant 返回 None → child_table = "unknown"
        // 但我们传给 make_finding_ext 的应该是原始 Option，不是 "unknown" 字符串
        assert_eq!(finding.table, None, "must be None, not Some(\"unknown\")");
    }
}
```

> **关键设计决策**：`find_first_scan_descendant` 返回 `Option<String>`。当前代码把 None 包装成 `"unknown"` 喂给 i18n 字符串。迁移后**直接传 `Option<String>`**：Some 时填充 `table` 字段 + i18n 字符串照旧；None 时 `table=None` + i18n 字符串用 `"unknown"`。这样结构化字段不会出现 `Some("unknown")` 这种无意义值。

**Step 2: 运行确认失败**

```bash
cargo test -p ogexplain-core subquery_rules::tests_task_1_2 -- --nocapture
```

预期：编译失败或断言失败（`table` 仍为 `None`）。

**Step 3: 修改 SUBQ-001 `check`**

修改 `subquery_rules.rs:62-76`，从：

```rust
let child_table = find_first_scan_descendant(node)
    .map(|r| first_identifier(&r))
    .unwrap_or_else(|| "unknown".to_string());

return Some(make_finding(
    self,
    t!("finding.SUBQ-001.detail_subquery_scan", table = &child_table).to_string(),
    node,
    Some(t!("finding.SUBQ-001.suggestion_subquery_scan").to_string()),
));
```

改为：

```rust
let child_table_opt = find_first_scan_descendant(node).map(|r| first_identifier(&r));
let child_table_display = child_table_opt
    .clone()
    .unwrap_or_else(|| "unknown".to_string());

return Some(make_finding_ext(
    self,
    t!("finding.SUBQ-001.detail_subquery_scan", table = &child_table_display).to_string(),
    node,
    Some(t!("finding.SUBQ-001.suggestion_subquery_scan").to_string()),
    child_table_opt,           // ← Some(table) 或 None
    Vec::new(),                // ← 列名待后续（SUBQ-001 当前不提取列）
));
```

**Step 4-6: 运行测试 / snapshot review / 提交**

```bash
cargo test -p ogexplain-core subquery_rules -- --nocapture
cargo insta review   # 受影响的 SUBQ-001 snapshot 需人工核对（应仅有 table 字段新增）
```

提交：
```
feat(rules): SUBQ-001 populates Finding.table structured field

Migrates SubqueryNotPulledUp to make_finding_ext. table is Some(name)
when find_first_scan_descendant locates a scan child, None otherwise
(was previously stringified to "unknown" and lost).
```

---

### Task 1.3: SCAN-001 迁移

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs:37-79`（LargeTableFullScan::check）
- Test: 同文件 test module

**Step 1: 写失败的测试**

```rust
#[test]
fn scan_001_populates_table_field_from_relation() {
    // SeqScan with relation="orders"
    let node = make_seq_scan_with_relation("orders", /* rows */ 1_000_000.0);
    let finding = LargeTableFullScan::default().check(&node, &Default::default()).unwrap();
    assert_eq!(finding.table.as_deref(), Some("orders"));
}
```

**Step 2-6: 标准流程**

修改 `scan_rules.rs:79`，把 `make_finding(...)` 改为 `make_finding_ext(..., node.relation.clone().map(first_identifier), Vec::new())`。`first_identifier` 已存在；`LargeTableFullScan` 已用 `node.relation` 计算 detail。

提交：`feat(rules): SCAN-001 populates Finding.table`

---

### Task 1.4: SCAN-004 迁移

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs:128-210`（FilterWithoutIndex::check）

**Step 1: 写失败的测试**

```rust
#[test]
fn scan_004_populates_table_and_column_fields() {
    // SeqScan on orders, Filter: (status = '42')
    let node = make_seq_scan_with_filter("orders", "status = '42'", /* rows_removed */ 100_000.0);
    let finding = FilterWithoutIndex::default().check(&node, &Default::default()).unwrap();
    assert_eq!(finding.table.as_deref(), Some("orders"));
    assert_eq!(finding.columns, vec!["status".to_string()]);
}
```

**Step 2-6: 标准流程**

SCAN-004 已用 `extract_column_from_filter` 提取列名（注入 detail/suggestion）。把它同时填入 `columns: vec![col.into()]`。`table` 来自 `node.relation`。

提交：`feat(rules): SCAN-004 populates Finding.table/columns`

---

### Task 1.5: TYPE-001 迁移

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/type_coercion_rules.rs:71`（SuspectedImplicitTypeCast::check）

**Step 1: 写失败的测试**

```rust
#[test]
fn type_001_populates_table_and_column_fields() {
    // SeqScan on accounts, Filter: (facctcode = '1002')
    let node = make_seq_scan_with_filter("accounts", "facctcode = '1002'", 1_000_000.0);
    let finding = SuspectedImplicitTypeCast.check(&node, &Default::default()).unwrap();
    assert_eq!(finding.table.as_deref(), Some("accounts"));
    assert_eq!(finding.columns, vec!["facctcode".to_string()]);
}
```

**Step 2-6: 标准流程**

TYPE-001 已用 `extract_column_from_filter` + `node.relation`。直接迁移。

提交：`feat(rules): TYPE-001 populates Finding.table/columns`

---

### Task 1.6: JOIN-001 迁移

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/join_rules.rs:35-106`（NestedLoopLargeDataset::check）

**Step 1: 写失败的测试**

```rust
#[test]
fn join_001_populates_join_column() {
    // NestedLoop with HashCond: o.customer_id = c.customer_id
    let node = make_nested_loop_with_join_col("customer_id");
    let finding = NestedLoopLargeDataset::default().check(&node, &Default::default()).unwrap();
    assert_eq!(finding.columns, vec!["customer_id".to_string()]);
    // table 留 None（JOIN-001 涉及两表，单一 table 字段不合适）
    assert!(finding.table.is_none());
}
```

**Step 2-6: 标准流程**

JOIN-001 在 `join_rules.rs:42` 已有 `join_column: Option<String>` 局部变量。`columns` 填 `join_column.into_iter().collect()`。`table` 保持 `None`（多表场景，单一 table 字段语义不清——后续若有 `tables: Vec<String>` 字段再迁移）。

提交：`feat(rules): JOIN-001 populates Finding.columns with join column`

---

### Task 1.7: Phase 1 回归验证

```bash
cargo test --workspace
cargo run -p ogexplain-cli -- analyze tests/fixtures/04_nested_loop.txt --format json | jq '.findings[0] | {rule_id, table, columns}'
```

**预期 JSON 输出**（示例）：
```json
{
  "rule_id": "JOIN-001",
  "columns": ["customer_id"]
}
```

`table` 字段在 SCAN/SUBQ 触发的 finding 中可见。

---

## Phase 2: Convergence 模块

**目标**：对比两轮 metrics snapshot，决定 Continue/Stop。

**为什么需要**：闭环迭代的"何时停"判断。不能直接对比 `SummaryRow`（含 f64，无 PartialEq，有 NaN 风险）。

**设计决策**：
1. 引入 `MetricsSnapshot` 子集（只含收敛判断需要的字段，可 derive PartialEq）
2. `LoopConfig` 包含 `require_equivalence_proof` 和 `auto_run_analyze`（即使 Week 1 不用，也建模完整）
3. 模块位置：`crates/ogexplain-core/src/convergence.rs`（顶层）——与 `summary.rs`、`suggester/` 同级

### Task 2.1: 新建 `convergence.rs` 骨架 + `MetricsSnapshot`

**Files:**
- Create: `crates/ogexplain-core/src/convergence.rs`
- Modify: `crates/ogexplain-core/src/lib.rs`（添加 `pub mod convergence;`）

**Step 1: 写失败的测试**

`crates/ogexplain-core/src/convergence.rs` 内嵌 `#[cfg(test)] mod tests`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_snapshot_partial_eq_works() {
        let a = MetricsSnapshot {
            total_cost: Some(100.0),
            total_time_ms: Some(50.0),
            critical_count: 2,
            warning_count: 3,
            spill_kb: None,
            peak_memory_kb: None,
            worst_est_ratio: None,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn stop_when_critical_zero() {
        let prev = MetricsSnapshot::default();
        let curr = MetricsSnapshot { critical_count: 0, ..Default::default() };
        let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true);
        assert!(matches!(decision, LoopDecision::Stop(StopReason::Success)));
    }

    // ... 更多测试见 Step 3
}
```

**Step 2: 运行确认失败**

```bash
cargo test -p ogexplain-core convergence
```

预期：编译失败（模块不存在）。

**Step 3: 实现 `convergence.rs`**

```rust
//! Convergence detection for closed-loop optimization.
//!
//! Compares two [`MetricsSnapshot`]s across iterations and decides whether
//! to continue the rewrite→verify→re-evaluate loop or stop.
//!
//! See `docs/pilot-subquery-to-join.md` §T3 and
//! `docs/closed-loop-optimization-design.md` §7 for design context.

use serde::Serialize;

/// Snapshot of plan metrics relevant to convergence detection.
///
/// Subset of [`crate::summary::SummaryRow`] that:
/// (a) is sufficient for convergence decisions,
/// (b) derives `PartialEq` safely (no f64 NaN risk on real plan data).
///
/// Construct via [`MetricsSnapshot::from_summary`] in the orchestrator.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct MetricsSnapshot {
    pub total_cost: Option<f64>,
    pub total_time_ms: Option<f64>,
    pub critical_count: usize,
    pub warning_count: usize,
    pub spill_kb: Option<f64>,
    pub peak_memory_kb: Option<f64>,
    pub worst_est_ratio: Option<f64>,
}

impl MetricsSnapshot {
    /// Extract convergence-relevant fields from a full SummaryRow.
    pub fn from_summary(s: &crate::summary::SummaryRow) -> Self {
        Self {
            total_cost: Some(s.total_cost),
            total_time_ms: Some(s.total_time_ms),
            critical_count: s.critical_count,
            warning_count: s.warning_count,
            spill_kb: s.spill_kb,
            peak_memory_kb: s.peak_memory_kb,
            worst_est_ratio: s.worst_est_ratio,
        }
    }
}

/// Loop configuration. All thresholds are inclusive (>=).
#[derive(Debug, Clone, Serialize)]
pub struct LoopConfig {
    /// Maximum iterations before forced stop. Default 10.
    pub max_iterations: usize,
    /// Minimum cost improvement fraction to count as progress. Default 0.05 (5%).
    pub min_improvement_pct: f64,
    /// Consecutive non-improving iterations before plateau stop. Default 3.
    pub max_plateau_count: usize,
    /// Cost increase fraction that triggers regression rollback. Default 0.10 (10%).
    pub regression_threshold_pct: f64,
    /// Whether to require QED/VeriEQL equivalence proof before accepting a rewrite.
    /// Week 1 pilot may set this to false (rely on metamorphosis Conditional safety).
    /// Production use should be true.
    pub require_equivalence_proof: bool,
    /// Whether to auto-run ANALYZE when stale statistics detected (Phase 0).
    /// Week 1 pilot sets this to false (manual ANALYZE required).
    pub auto_run_analyze: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            min_improvement_pct: 0.05,
            max_plateau_count: 3,
            regression_threshold_pct: 0.10,
            require_equivalence_proof: true,
            auto_run_analyze: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum LoopDecision {
    Continue,
    Stop(StopReason),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum StopReason {
    /// `critical_count == 0` — all critical findings resolved.
    Success,
    /// Cost improvement < `min_improvement_pct` for `max_plateau_count` iterations.
    Plateau,
    /// Cost increased > `regression_threshold_pct` — rollback and stop.
    Regression,
    /// Reached `max_iterations`.
    MaxIterations,
    /// Remaining findings have no rewrite mapping (DDL/Config/Log only).
    NoRewritableFindings,
    /// Rewritten SQL equals previous SQL (fixed-point reached, no progress possible).
    FixedPoint,
}

/// Decide whether to continue the optimization loop.
///
/// **Evaluation order** (first match wins):
/// 1. Fixed-point: `sql_unchanged == true` → FixedPoint
/// 2. Success: `curr.critical_count == 0` → Success
/// 3. Regression: cost increased beyond threshold → Regression
/// 4. MaxIterations: `iteration >= config.max_iterations` → MaxIterations
/// 5. Plateau: non-improving for `max_plateau_count` → Plateau
/// 6. NoRewritableFindings: no rewritable findings remain → NoRewritableFindings
/// 7. Otherwise → Continue
pub fn should_continue(
    prev: &MetricsSnapshot,
    curr: &MetricsSnapshot,
    config: &LoopConfig,
    iteration: usize,
    plateau_count: usize,
    has_rewritable: bool,
    sql_unchanged: bool,
) -> LoopDecision {
    if sql_unchanged {
        return LoopDecision::Stop(StopReason::FixedPoint);
    }
    if curr.critical_count == 0 {
        return LoopDecision::Stop(StopReason::Success);
    }
    if let (Some(p), Some(c)) = (prev.total_cost, curr.total_cost) {
        if c > p * (1.0 + config.regression_threshold_pct) {
            return LoopDecision::Stop(StopReason::Regression);
        }
    }
    if iteration >= config.max_iterations {
        return LoopDecision::Stop(StopReason::MaxIterations);
    }
    if let (Some(p), Some(c)) = (prev.total_cost, curr.total_cost) {
        if p > 0.0 {
            let improvement = (p - c) / p;
            if improvement < config.min_improvement_pct
                && plateau_count >= config.max_plateau_count
            {
                return LoopDecision::Stop(StopReason::Plateau);
            }
        }
    }
    if !has_rewritable {
        return LoopDecision::Stop(StopReason::NoRewritableFindings);
    }
    LoopDecision::Continue
}
```

**Step 4: 测试覆盖所有 StopReason + Continue**

至少 7 个测试：每种 StopReason 一个 + Continue 场景 + FixedPoint。

**Step 5: 提交**

```
feat(core): add convergence module with MetricsSnapshot

New convergence.rs provides:
- MetricsSnapshot: PartialEq-able subset of SummaryRow
- LoopConfig: includes require_equivalence_proof + auto_run_analyze
- should_continue(): 7-way StopReason + Continue, with FixedPoint guard

This is the decision layer for the closed-loop optimization orchestrator.
Week 1 pilot uses require_equivalence_proof=false (relies on metamorphosis
Conditional safety); production should set it to true once QED integration
is wired through.

Part of #33.
```

---

## Phase 3: Mapper 模块（CLI 侧）

**目标**：把 finding.rule_id 映射到 metamorphosis 规则名 / ogexplain 内置 rewrite / DDL/Config 建议。

**关键设计决策**：
- **位置：`crates/ogexplain-cli/src/optimize/mapper.rs`**（**不**放 `crates/ogexplain-core/src/mapper.rs`）
- 理由：(1) 避开 `suggester/mapper.rs` 命名冲突；(2) mapper 知道 metamorphosis 规则名（外部 repo 概念），不属于 core 库契约；(3) `closed-loop-optimization-design.md` §附录 A 也把"映射引擎"归到 Orchestrator 列
- core 侧只加 `DiagnosticHint` shim（serde 兼容 metamorphosis PR #35）

### Task 3.1: core 侧加 `DiagnosticHint` 类型

**Files:**
- Create: `crates/ogexplain-core/src/diagnostic_hint.rs`
- Modify: `crates/ogexplain-core/src/lib.rs`

**Step 1-3: 实现**

```rust
//! Diagnostic hint passed to metamorphosis's RewriteContext.diagnostic_hints.
//!
//! This is the cross-tool contract: ogexplain populates this from Finding
//! data; metamorphosis consumes it to direct rewrite rules.
//! Field names/types align with metamorphosis's DiagnosticHint (PR #35).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticHint {
    pub rule_id: String,
    pub table: Option<String>,
    pub columns: Vec<String>,
    pub severity: String,
    pub detail: String,
}

impl DiagnosticHint {
    /// Build a hint from a Finding. Skips findings without a rule_id.
    /// Severity is lowercased string form of the Finding's Severity.
    pub fn from_finding(f: &crate::analyzer::report::Finding) -> Option<Self> {
        Some(Self {
            rule_id: f.rule_id.clone(),
            table: f.table.clone(),
            columns: f.columns.clone(),
            severity: f.severity.as_str().to_string(),
            detail: f.detail.clone(),
        })
    }
}
```

`lib.rs` 加 `pub mod diagnostic_hint; pub use diagnostic_hint::DiagnosticHint;`

**Step 4-6: 标准流程**

测试：`from_finding` 转换、None table 保留、columns 透传。

提交：`feat(core): add DiagnosticHint type for cross-tool contract`

---

### Task 3.2: CLI 侧 mapper 模块 + 质量门槛

**Files:**
- Create: `crates/ogexplain-cli/src/optimize/mod.rs`
- Create: `crates/ogexplain-cli/src/optimize/mapper.rs`
- Modify: `crates/ogexplain-cli/src/lib.rs`（添加 `#[cfg(feature = "db")] pub mod optimize;`）

**Step 1: 写失败的测试**

`crates/ogexplain-cli/src/optimize/mapper.rs`：

```rust
//! Maps ogexplain Finding.rule_id to metamorphosis rewrite rules or
//! advisory actions. Used by the optimize subcommand.

use ogexplain_core::analyzer::report::Finding;
use ogexplain_core::DiagnosticHint;

/// Action to take for a finding.
#[derive(Debug, Clone, PartialEq)]
pub enum RemediationAction {
    /// Call metamorphosis with the listed rule IDs.
    Rewrite { rules: Vec<&'static str> },
    /// Use ogexplain's built-in sql_rewrite (e.g. SUBQ-006).
    UseBuiltinRewrite,
    /// Output a DDL suggestion (CREATE INDEX, etc.) — no auto-execution.
    DdlAdvice,
    /// Output a configuration suggestion (SET work_mem, etc.).
    ConfigAdvice,
    /// Run ANALYZE then retry (Phase 0).
    RunAnalyze,
    /// Architectural — record warning, requires human.
    Log,
}

/// Mapping table. Consult `docs/closed-loop-optimization-design.md` §5.2.
pub fn map_diagnostic(rule_id: &str) -> RemediationAction {
    match rule_id {
        "SUBQ-001" | "REW-001" => RemediationAction::Rewrite { rules: vec!["subquery-to-join"] },
        "SUBQ-006" => RemediationAction::UseBuiltinRewrite,
        "TYPE-001" => RemediationAction::Rewrite { rules: vec!["add-explicit-cast"] },
        "TYPE-004" => RemediationAction::Rewrite { rules: vec!["suggest-trgm-index"] },
        "AGG-001" => RemediationAction::Rewrite { rules: vec!["rewrite-group-agg"] },
        "SCAN-001" | "SCAN-004" | "JOIN-001" => RemediationAction::DdlAdvice,
        "MEM-001" | "MEM-004" | "JOIN-002" | "AGG-002" => RemediationAction::ConfigAdvice,
        "STATS-001" | "EST-001" | "EST-004" => RemediationAction::RunAnalyze,
        _ => RemediationAction::Log,
    }
}

/// Filter findings to those with a rewrite action.
///
/// **Quality gate**: SUBQ-001 findings are only included when `table` is
/// present (not None). This filters false positives where the rule fires
/// on a SubqueryScan but no scan descendant was located.
///
/// Rationale: v4 benchmark shows SUBQ-001 precision = 0.43 (4 FP / 7 total).
/// Requiring non-None table is a necessary (not sufficient) filter — the
/// recursive lookup now populates table for most real subqueries, while FPs
/// often lack a clear scan descendant.
pub fn filter_rewritable(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|f| {
            let action = map_diagnostic(&f.rule_id);
            if !matches!(
                action,
                RemediationAction::Rewrite { .. } | RemediationAction::UseBuiltinRewrite
            ) {
                return false;
            }
            // Quality gate for SUBQ-001
            if f.rule_id == "SUBQ-001" && f.table.is_none() {
                return false;
            }
            true
        })
        .collect()
}

/// Convert a finding to a DiagnosticHint for metamorphosis.
pub fn finding_to_hint(f: &Finding) -> Option<DiagnosticHint> {
    DiagnosticHint::from_finding(f)
}
```

**Step 2: 写测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ogexplain_core::analyzer::report::{DiagnosticCategory, Finding, Severity};

    fn make_finding(rule_id: &str, table: Option<&str>) -> Finding {
        Finding {
            rule_id: rule_id.into(),
            severity: Severity::Warning,
            category: DiagnosticCategory::SubqueryStructure,
            title: "t".into(),
            detail: "d".into(),
            node_line: None,
            node_type: None,
            suggestion: None,
            sql_rewrite: None,
            evidence: None,
            table: table.map(String::from),
            columns: Vec::new(),
        }
    }

    #[test]
    fn map_subq_001_to_subquery_to_join() {
        assert!(matches!(
            map_diagnostic("SUBQ-001"),
            RemediationAction::Rewrite { rules } if rules == vec!["subquery-to-join"]
        ));
    }

    #[test]
    fn filter_subq_001_without_table() {
        let f = make_finding("SUBQ-001", None);
        assert!(filter_rewritable(&[f]).is_empty(), "SUBQ-001 without table must be filtered");
    }

    #[test]
    fn filter_subq_001_with_table_passes() {
        let f = make_finding("SUBQ-001", Some("items"));
        assert_eq!(filter_rewritable(&[f]).len(), 1);
    }

    #[test]
    fn map_unknown_rule_to_log() {
        assert!(matches!(map_diagnostic("UNKNOWN-999"), RemediationAction::Log));
    }

    #[test]
    fn filter_excludes_ddl_only_rules() {
        let f = make_finding("SCAN-001", Some("orders"));
        assert!(filter_rewritable(&[f]).is_empty(), "SCAN-001 is DdlAdvice");
    }
}
```

**Step 3-6: 标准流程**

提交：`feat(cli): add optimize/mapper module with rule→rewrite mapping`

---

## Phase 4: Optimize 子命令

**目标**：实现 `ogexplain optimize` 子命令，串联 EXPLAIN → 诊断 → 映射 → metamorphosis 重写 → re-EXPLAIN → 收敛。

**关键设计决策**：
1. **保留单文件 lib.rs 风格**：复杂逻辑放 `optimize/` 子模块，但 `Optimize` clap variant 加到 `Commands` enum + dispatch arm 放 lib.rs（与 Analyze/Explain 一致）
2. **metamorphosis 子进程调用**：用 `std::process::Command`，不引入 Cargo 依赖
3. **振荡检测**：维护 `HashSet<u64>` 记录 SQL 哈希，命中即 FixedPoint 停止
4. **安全保护**：
   - `--analyze`（真实执行）需配合 `--i-know-the-risks` flag，否则默认只 EXPLAIN 不 ANALYZE
   - metamorphosis 不在 PATH → 友好错误
   - 重写后 SQL 执行失败 → 立即停止并回滚到上一版本
5. **Phase 0 Stats check 占位**：默认打印警告"stats check not yet implemented"，`--skip-stats-check` 显式确认

### Task 4.1: clap Optimize variant + dispatch

**Files:**
- Modify: `crates/ogexplain-cli/src/lib.rs`（Commands enum + dispatch）

**Step 1: 加 Optimize variant 到 Commands enum**

在 `Commands::Explain` 之后添加：

```rust
Optimize {
    /// SQL 语句（与 --sql-file 二选一）
    #[arg(short = 's', long)]
    sql: Option<String>,
    /// 包含 SQL 的文件路径
    #[arg(short = 'f', long = "sql-file")]
    sql_file: Option<String>,
    /// 数据库连接配置文件（默认 ~/.gaussdb-mcp.toml）
    #[arg(long)]
    config: Option<String>,
    /// Named connection from config file
    #[arg(long)]
    name: Option<String>,
    /// Schema JSON 文件路径（传给 metamorphosis）
    #[arg(long)]
    schema: Option<String>,
    /// Path to metamorphosis binary (default: lookup in PATH)
    #[arg(long)]
    metamorphosis: Option<String>,
    /// 最大迭代次数
    #[arg(long, default_value = "10")]
    max_iterations: usize,
    /// Run EXPLAIN ANALYZE (executes query). Requires --i-know-the-risks.
    #[arg(long)]
    analyze: bool,
    /// Acknowledge that --analyze executes rewritten (possibly-unverified) SQL.
    #[arg(long)]
    i_know_the_risks: bool,
    /// Skip Phase 0 stats check (currently always skips — stats check is future work).
    #[arg(long)]
    skip_stats_check: bool,
    /// Output format: text, json
    #[arg(long, default_value = "text")]
    format: String,
    /// Output file path
    #[arg(short, long)]
    output: Option<String>,
    /// Language
    #[arg(long, default_value = "auto")]
    lang: String,
},
```

**Step 2: dispatch arm**

在 dispatch match 中添加（参照 Explain arm 的 cfg-gate 模式）：

```rust
Some(("optimize", args)) => {
    #[cfg(feature = "db")]
    {
        let sql = resolve_sql_arg(args)?;
        let config_path = resolve_config_path(args);
        let schema_path = args.get_one::<String>("schema").cloned();
        let metamorphosis_path = args.get_one::<String>("metamorphosis")
            .cloned().unwrap_or_else(|| "metamorphosis".to_string());
        let max_iter = args.get_one::<usize>("max_iterations").copied().unwrap_or(10);
        let analyze = args.get_flag("analyze");
        let i_know = args.get_flag("i_know_the_risks");
        let skip_stats = args.get_flag("skip_stats_check");
        let format = args.get_one::<String>("format").cloned().unwrap_or_default();
        return crate::optimize::run_optimize(crate::optimize::OptimizeArgs {
            sql, config_path, schema_path, metamorphosis_path,
            max_iterations: max_iter,
            analyze_enabled: analyze && i_know,
            skip_stats_check: skip_stats,
            format,
        });
    }
    #[cfg(not(feature = "db"))]
    anyhow::bail!("Database support not compiled. Rebuild with --features db");
}
```

**Step 3-6: 提交骨架（不含主循环）**

```
feat(cli): add optimize subcommand skeleton

Registers clap variant and dispatch arm. Loop body to follow in
subsequent commits.

Part of #34.
```

---

### Task 4.2: `run_optimize` 主循环

**Files:**
- Create/Modify: `crates/ogexplain-cli/src/optimize/mod.rs`（添加 `OptimizeArgs`、`run_optimize`）

**Step 1: 实现 OptimizeArgs + 主循环**

```rust
use std::collections::HashSet;
use std::path::PathBuf;
use anyhow::{Context, Result};
use ogexplain_core::{analyze, convergence::{self, LoopConfig, LoopDecision, MetricsSnapshot}, parse, DiagnosticHint};
use ogexplain_core::summary::SummaryRow;

use crate::optimize::mapper::{filter_rewritable, finding_to_hint, map_diagnostic, RemediationAction};

pub struct OptimizeArgs {
    pub sql: String,
    pub config_path: Option<PathBuf>,
    pub schema_path: Option<String>,
    pub metamorphosis_path: String,
    pub max_iterations: usize,
    pub analyze_enabled: bool,
    pub skip_stats_check: bool,
    pub format: String,
}

#[derive(Debug)]
pub struct IterationRecord {
    pub iteration: usize,
    pub rule_id: String,
    pub action: RemediationAction,
    pub snapshot_before: Option<MetricsSnapshot>,
    pub snapshot_after: Option<MetricsSnapshot>,
    pub rewritten_sql: Option<String>,
    pub notes: Vec<String>,
}

pub fn run_optimize(args: OptimizeArgs) -> Result<()> {
    // 0. Pre-flight: metamorphosis in PATH?
    check_metamorphosis_available(&args.metamorphosis_path)?;

    // 1. Phase 0 stats check (placeholder)
    if !args.skip_stats_check {
        eprintln!("⚠️  Warning: Phase 0 stats check not yet implemented.");
        eprintln!("   Stale statistics may produce misleading diagnostics.");
        eprintln!("   Run ANALYZE manually on involved tables, or pass --skip-stats-check to silence.");
    }

    let loop_config = LoopConfig {
        max_iterations: args.max_iterations,
        require_equivalence_proof: false, // Week 1: rely on metamorphosis Conditional safety
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
        // Step 1: EXPLAIN + analyze
        let explain_text = crate::db::fetch_explain(
            args.config_path.as_deref(),
            None, // name (TODO: thread through)
            &current_sql,
            args.analyze_enabled,
        ).context("EXPLAIN failed")?;

        let plan = parse(&explain_text).context("parse EXPLAIN failed")?;
        let report = analyze(&plan);

        let summary = SummaryRow::compute(&plan, &report, None);
        let curr_snapshot = MetricsSnapshot::from_summary(&summary);

        // Step 2: Convergence check
        if let Some(prev) = &prev_snapshot {
            let rewritable = filter_rewritable(&report.findings);
            let sql_unchanged = sql_history.contains(&hash_sql(&current_sql)) && iteration > 1;
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
                eprintln!("Stop: {:?}", reason);
                print_final_report(&history, reason, &current_sql, &args.format)?;
                return Ok(());
            }
            // Update plateau_count
            if let (Some(p), Some(c)) = (prev.total_cost, curr_snapshot.total_cost) {
                if p > 0.0 && (p - c) / p < loop_config.min_improvement_pct {
                    plateau_count += 1;
                } else {
                    plateau_count = 0;
                }
            }
        }

        // Step 3: Filter + pick first rewritable finding
        let rewritable = filter_rewritable(&report.findings);
        if rewritable.is_empty() {
            eprintln!("Stop: NoRewritableFindings");
            print_final_report(&history, convergence::StopReason::NoRewritableFindings, &current_sql, &args.format)?;
            return Ok(());
        }
        let finding = &rewritable[0]; // Week 1: pick first
        let action = map_diagnostic(&finding.rule_id);

        // Step 4: Rewrite
        let rewritten_sql = match &action {
            RemediationAction::UseBuiltinRewrite => {
                finding.sql_rewrite.as_ref().map(|r| r.rewritten_sql.clone())
            }
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
                iteration, rule_id: finding.rule_id.clone(), action,
                snapshot_before: prev_snapshot.clone(), snapshot_after: Some(curr_snapshot.clone()),
                rewritten_sql: None, notes: vec!["No rewrite produced".into()],
            });
            break;
        };

        // Step 5: Detect oscillation / fixed-point
        let rewritten_hash = hash_sql(&rewritten);
        if sql_history.contains(&rewritten_hash) {
            eprintln!("Stop: FixedPoint (rewritten SQL seen before — oscillation detected)");
            print_final_report(&history, convergence::StopReason::FixedPoint, &current_sql, &args.format)?;
            return Ok(());
        }
        sql_history.insert(rewritten_hash);

        history.push(IterationRecord {
            iteration, rule_id: finding.rule_id.clone(), action: action.clone(),
            snapshot_before: prev_snapshot.clone(), snapshot_after: Some(curr_snapshot.clone()),
            rewritten_sql: Some(rewritten.clone()), notes: vec![],
        });

        prev_snapshot = Some(curr_snapshot);
        current_sql = rewritten;
    }

    // Hit max iterations
    print_final_report(&history, convergence::StopReason::MaxIterations, &current_sql, &args.format)?;
    Ok(())
}

fn hash_sql(sql: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    sql.hash(&mut hasher);
    hasher.finish()
}

fn check_metamorphosis_available(path: &str) -> Result<()> {
    let resolved = which::which(path).map_err(|_| {
        anyhow::anyhow!(
            "metamorphosis binary '{path}' not found in PATH.\n\
             Install: git clone https://github.com/c2j/metamorphosis && cd metamorphosis && cargo build --release\n\
             Then either put `target/release/metamorphosis` in PATH or pass --metamorphosis <path>"
        )
    })?;
    eprintln!("Using metamorphosis: {}", resolved.display());
    Ok(())
}

fn call_metamorphosis_rewrite(
    sql: &str,
    rules: &[&str],
    schema_path: Option<&str>,
    hint: Option<&DiagnosticHint>,
    metamorphosis_path: &str,
) -> Result<String> {
    use std::process::Command;
    use std::fs;

    // Write SQL to temp file
    let input_path = std::env::temp_dir().join("ogexplain_optimize_input.sql");
    fs::write(&input_path, sql)?;

    let mut cmd = Command::new(metamorphosis_path);
    cmd.arg("rewrite")
        .arg("--file").arg(&input_path)
        .arg("--rules").arg(rules.join(","))
        .arg("--input-format").arg("sql");
    if let Some(schema) = schema_path {
        cmd.arg("--schema").arg(schema);
    }

    let output = cmd.output()
        .with_context(|| format!("Failed to spawn {}", metamorphosis_path))?;
    if !output.status.success() {
        anyhow::bail!(
            "metamorphosis rewrite failed (exit {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // metamorphosis emits optional comment headers (lines starting with -- or #);
    // filter them and trim trailing semicolons/newlines
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
    let _ = hint; // hint currently logged by subquery_to_join rule; not yet passed via CLI (TODO)
    Ok(cleaned)
}

fn print_final_report(
    history: &[IterationRecord],
    reason: convergence::StopReason,
    final_sql: &str,
    format: &str,
) -> Result<()> {
    // Minimal text report — extend per format
    println!("=== Optimization Report ===");
    println!("Stop reason: {:?}", reason);
    println!("Iterations: {}", history.len());
    for record in history {
        println!("\n--- Iteration {} ---", record.iteration);
        println!("Triggered by: {} ({:?})", record.rule_id, record.action);
        if let (Some(before), Some(after)) = (&record.snapshot_before, &record.snapshot_after) {
            if let (Some(b), Some(a)) = (before.total_cost, after.total_cost) {
                let delta = if b > 0.0 { ((a - b) / b) * 100.0 } else { 0.0 };
                println!("Cost: {:.2} → {:.2} ({:+.1}%)", b, a, delta);
            }
            println!("Critical findings: {} → {}", before.critical_count, after.critical_count);
        }
    }
    println!("\n=== Final SQL ===\n{}\n", final_sql);
    let _ = format;
    Ok(())
}
```

**Step 2: 写集成测试**（mock db::fetch_explain，避免真实 DB）

放 `crates/ogexplain-cli/src/optimize/mod.rs` 内 `#[cfg(test)] mod tests`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_sql_distinguishes_inputs() {
        assert_ne!(hash_sql("SELECT 1"), hash_sql("SELECT 2"));
        assert_eq!(hash_sql("SELECT 1"), hash_sql("SELECT 1"));
    }

    #[test]
    fn oscillation_detection_works() {
        let mut history: HashSet<u64> = HashSet::new();
        let h1 = hash_sql("SELECT 1");
        history.insert(h1);
        assert!(history.contains(&h1));
    }
}
```

**Step 3: 提交**

```
feat(cli): implement optimize loop with oscillation + safety gates

The loop:
1. Pre-flight: verifies metamorphosis in PATH
2. Phase 0 stats check (placeholder: warns, requires --skip-stats-check)
3. Per iteration: EXPLAIN → analyze → convergence check → filter →
   map → rewrite (subprocess) → oscillation check → next iteration
4. Convergence: all 6 StopReason paths + FixedPoint for oscillation

Safety:
- --analyze requires --i-know-the-risks (else defaults to EXPLAIN-only)
- metamorphosis-not-in-PATH → actionable error
- rewrite-failure → bail with stderr
- empty rewrite output → bail
- oscillation (rewritten SQL seen before) → FixedPoint stop

Part of #34.
```

---

## Phase 5: 端到端测试

**目标**：验证 SUBQ-001 → subquery-to-join 完整闭环。

### Task 5.1: 静态 E2E（CI 友好，不依赖 DB）

**Files:**
- Create: `tests/optimize_static_e2e.rs`

**Step 1: 测试设计**

无法直接测 `run_optimize`（依赖 db::fetch_explain），改为分步测试：

```rust
//! Static end-to-end test for the optimize loop's decision logic.
//! Does NOT require a database — uses pre-recorded EXPLAIN text.

#[test]
fn subq_001_finding_maps_to_subquery_to_join_rewrite() {
    // Step 1: parse pre-recorded EXPLAIN that triggers SUBQ-001
    let explain_text = include_str!("fixtures/subq_001_sample.txt");
    let plan = ogexplain_core::parse(explain_text).unwrap();
    let report = ogexplain_core::analyze(&plan);

    // Step 2: find SUBQ-001 finding
    let subq = report.findings.iter()
        .find(|f| f.rule_id == "SUBQ-001")
        .expect("fixture should trigger SUBQ-001");

    // Step 3: assert structured table field is populated
    assert!(subq.table.is_some(), "SUBQ-001 must populate table field");

    // Step 4: map to remediation
    use ogexplain_cli::optimize::mapper::{map_diagnostic, filter_rewritable, RemediationAction};
    assert!(matches!(
        map_diagnostic(&subq.rule_id),
        RemediationAction::Rewrite { rules } if rules == vec!["subquery-to-join"]
    ));

    // Step 5: filter_rewritable includes it
    let rewritable = filter_rewritable(&report.findings);
    assert!(rewritable.iter().any(|f| f.rule_id == "SUBQ-001"));
}

#[test]
fn convergence_loop_decision_logic() {
    use ogexplain_core::convergence::*;
    let prev = MetricsSnapshot { total_cost: Some(100.0), critical_count: 2, ..Default::default() };
    let curr = MetricsSnapshot { total_cost: Some(80.0), critical_count: 1, ..Default::default() };
    let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, false);
    assert!(matches!(decision, LoopDecision::Continue));
}
```

**Step 2-4: 标准流程**

需要一个 fixture `tests/fixtures/subq_001_sample.txt`——从 `tests/regress/subquery/` 现有 case 复制一个真实触发 SUBQ-001 的 EXPLAIN 文本。

提交：`test(optimize): static E2E for SUBQ-001 → subquery-to-join mapping`

---

### Task 5.2: Live E2E（手动执行，需 docker-compose ogagila）

**Files:**
- Create: `tests/optimize_live_e2e.sh`（shell 脚本，非 cargo test）

**Step 1: 脚本**

```bash
#!/bin/bash
# Live end-to-end test for ogexplain optimize.
# Prerequisites:
#   1. ogagila docker-compose running (provides orders/items/customers schema)
#   2. metamorphosis binary in PATH (built from PR #35 branch)
#   3. ~/.gaussdb-mcp.toml configured for the docker DB
#   4. ogexplain built with --features db

set -euo pipefail

SQL='SELECT o.order_id, o.customer_id FROM orders o WHERE o.order_id IN (SELECT i.order_id FROM items i WHERE i.amount > 100)'

echo "=== Step 1: Baseline EXPLAIN ==="
ogexplain explain -s "$SQL" --analyze --format json -o /tmp/baseline.json
jq '.summary | {total_cost, critical_count, warning_count}' /tmp/baseline.json

echo "=== Step 2: Run optimize loop ==="
ogexplain optimize \
    --sql "$SQL" \
    --schema schema.json \
    --max-iterations 5 \
    --skip-stats-check \
    --format json \
    -o /tmp/optimization_result.json

echo "=== Step 3: Verify result ==="
jq '.iterations | length' /tmp/optimization_result.json
jq '.final_sql' /tmp/optimization_result.json
jq '.stop_reason' /tmp/optimization_result.json

echo "=== Step 4: (Optional) QED-verify final SQL equivalence ==="
metamorphosis verify \
    --original <(echo "$SQL") \
    --rewritten <(jq -r '.final_sql' /tmp/optimization_result.json) \
    --schema schema.json \
    --engine qed \
    --timeout 60
```

**Step 2: 文档化**

`tests/optimize_live_e2e.sh` 顶部包含完整前置条件说明。手动执行：

```bash
# In ogagila dir
docker-compose up -d
# Wait for DB to be ready...
cd /path/to/ogexplain-analyzer
bash tests/optimize_live_e2e.sh
```

**Step 3: 提交**

```
test(optimize): live E2E shell script for SUBQ-001 pilot

Manual execution only — requires docker-compose ogagila + metamorphosis
PR #35 binary. Validates end-to-end: EXPLAIN → diagnose → rewrite →
re-EXPLAIN → converge, plus optional QED equivalence check.

Not part of cargo test (no live DB assumption).
```

---

## 3. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| SUBQ-001 v4 precision 0.43 触发错误重写 | 高 | 中 | mapper 加 `table.is_some()` 门槛；Week 1 不接 QED（pilot 决策） |
| metamorphosis PR #35 长期不合并 | 中 | 高 | 子进程调用解耦；本地 checkout 也能跑；contract 已确认 |
| `metamorphosis rewrite` stdout 含注释头导致 SQL parse 失败 | 中 | 低 | `call_metamorphosis_rewrite` 已按行过滤 `--`/`#` 开头 |
| 静态 E2E 与 live E2E 行为不一致 | 中 | 中 | 静态只测决策逻辑；live 测全链路；分别维护 |
| 重写后 SQL 执行失败（DB 错误） | 中 | 高 | `fetch_explain` 已返回 Result；失败即 bail，保留 prev SQL |
| 振荡 A→B→A 不被检测 | 低 | 中 | `sql_history: HashSet<u64>` 已实现；rewritten 哈希命中即 FixedPoint |
| `--analyze` 执行未验证 SQL 损坏生产库 | 高 | 严重 | `--analyze` 需 `--i-know-the-risks`；默认 EXPLAIN-only；live E2E 限定测试库 |
| Finding 加字段破坏 JSON 消费者 | 低 | 低 | 两个新字段都 `skip_serializing_if`，旧消费者 JSON 不变 |

---

## 4. 时间预估

| Phase | Tasks | 预估时间 |
|---|---|---|
| Phase 1 | 1.1-1.7 | 2.5h（含 fixture 构造调试） |
| Phase 2 | 2.1 | 1h |
| Phase 3 | 3.1, 3.2 | 1.5h |
| Phase 4 | 4.1, 4.2 | 3h（主循环最复杂） |
| Phase 5 | 5.1, 5.2 | 1.5h（不含 live DB 调试） |
| **总计** | 12 tasks | **~9.5 小时**（不含 live DB 环境搭建） |

---

## 5. 成功标准（最终验收）

✅ **必须满足**：
1. `cargo test --workspace` 全通过（317 现有 + 新增 ~30 测试）
2. `cargo clippy --workspace -- -D warnings` 零警告
3. `cargo fmt --all -- --check` 通过
4. `ogexplain analyze tests/fixtures/04_nested_loop.txt --format json | jq '.findings[0].columns'` 返回非 null 数组
5. `ogexplain optimize --help` 显示完整选项
6. 静态 E2E 测试通过：SUBQ-001 fixture 正确映射到 subquery-to-join 规则
7. `convergence::should_continue` 单元测试覆盖 7 种决策路径

🎯 **加分项**：
8. live E2E 在 docker-compose ogagila 上跑通至少 1 轮迭代
9. QED 验证最终 SQL 等价性（依赖 metamorphosis#34 已 merged 的能力）
10. SUBQ-001 fixture 上 `filter_rewritable` 成功过滤掉至少 1 个 table=None 的 FP

---

## 6. 后续工作（不在本期）

按优先级排队：

1. **Phase 0 Stats 完整实现**：查询 pg_stat_user_tables，自动 ANALYZE
2. **QED/VeriEQL 集成**：Week 1 跳过的等价性验证层接入（metamorphosis#34 能力已 ready）
3. **lib.rs 拆分**：lib.rs 已 2768 行，建议拆 `cli/analyze_cmd.rs`、`cli/explain_cmd.rs`、`cli/optimize_cmd.rs`，但**单独 PR** 不夹带
4. **MCP 暴露 `optimize` 工具**
5. **批量模式 `--batch` CSV**
6. **更多规则迁移到 `make_finding_ext`**：REW-001、DIST-001、PART-001 等剩余 22 条
7. **pilot 文档 §2.2 列出的工程挑战**：NOT EXISTS/NOT IN 变体、QED 假测试修复
8. **mapper 完整覆盖**：当前映射表覆盖 9 条规则，剩 18 条按 §5.2 补全
