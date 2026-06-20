# Issue #17 修复实现方案

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 修复 ogexplain-analyzer v0.3.0 的诊断准确率问题(R=8.5%),主要解决 SCAN-001 双向 bug、finding 无优先级/去重、CLI 阈值不暴露三大问题。

**Architecture:** 三层修复——(1) 引擎层添加祖先上下文追踪,使规则能感知父子关系;(2) 规则层重写 SCAN-001 的行数判定逻辑;(3) 报告层添加 severity 排序 + 去重后处理。CLI 层暴露 DiagnosticConfig。

**Tech Stack:** Rust, 既有的 DiagnosticRule trait + DFS walk_node 架构, insta 快照测试, cargo test

---

## 代码核实结论(issue 根因假设 vs 实际)

| Issue 假设 | 实际代码验证 | 结论 |
|-----------|-------------|------|
| §5.1 "SCAN-001 读顶层 rows" | `scan_rules.rs:38-39` 读的是每个 SeqScan 自身的 `actual.rows`,引擎做 DFS 遍历全树 | **❌ 假设错误** |
| §5.2 "没有去重/优先级机制" | `config.rs:55-57` 仅 retain disabled_rules,无排序无去重 | **✅ 正确** |
| §5.3 "阈值不可配置" | `DiagnosticConfig` 存在且有 7 个字段,`analyze_with_config` API 可用,但 CLI/TUI/MCP 不暴露 | **⚠️ 半对** |

**真正的 SCAN-001 bug**:`actual.rows` 是节点**输出行数**(LIMIT/Filter 后),不是**扫描行数**。LIMIT 使 actual.rows=10,Filter 使 actual.rows 远小于表大小。同时 HashJoin build 侧的全扫被误报。

---

## 依赖关系

```
Phase 1 (祖先上下文) ──────┬──→ Phase 2 (SCAN-001 修复)
                           │
Phase 3 (去重/排序) ───────┘  (独立,可并行)
                               
Phase 4 (CLI 配置)  (独立,可并行)

Phase 5 (重评估) ←── 依赖 Phase 1-4 全部完成
```

---

## Phase 1: 祖先上下文基础设施

**目标**:让规则能感知"我在树里的位置"(如:是否在 HashJoin 下、是否在 Limit 下)。

**为什么不直接加 parent 指针**:`PlanNode` 是 `Serialize` 的树结构,加 parent 引用会引入循环引用和 lifetime 复杂度。引擎的 DFS walk 本身就持有路径,直接传下去最干净。

### Task 1.1: 给 DiagnosticRule trait 添加 context-aware check

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/mod.rs:19-28`

**Step 1: 添加 trait 方法(带默认实现,不破坏现有规则)**

在 `DiagnosticRule` trait 中,`check` 方法之后添加:

```rust
pub trait DiagnosticRule: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> Severity;
    fn category(&self) -> DiagnosticCategory;
    fn check(&self, node: &PlanNode, ctx: &super::context::PlanContext) -> Option<Finding>;
    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        Vec::new()
    }

    /// Context-aware check with ancestor chain (root → ... → parent).
    /// Default: delegates to `check`, ignoring ancestors.
    /// Override when a rule needs parent context (e.g., "am I under a HashJoin?").
    fn check_with_ancestors(
        &self,
        node: &PlanNode,
        ctx: &super::context::PlanContext,
        ancestors: &[&PlanNode],
    ) -> Option<Finding> {
        let _ = ancestors; // suppress unused warning
        self.check(node, ctx)
    }
}
```

**Step 2: 验证编译**

Run: `cargo build -p ogexplain-core`
Expected: 编译通过,所有 25 条规则用默认实现,行为不变。

**Step 3: Commit**

```bash
git add crates/ogexplain-core/src/analyzer/rules/mod.rs
git commit -m "feat(analyzer): add check_with_ancestors to DiagnosticRule trait

Non-breaking: default impl delegates to check(). Rules that need parent
context (ancestor chain) can override. Enables SCAN-001 join-awareness fix."
```

---

### Task 1.2: walk_node 追踪祖先链并调用 check_with_ancestors

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/config.rs:48-74`

**Step 1: 修改 walk_node 签名和 analyze 调用**

```rust
pub fn analyze(&self, plan: &ExplainPlan) -> DiagnosticReport {
    let stats = super::context::GlobalStats::compute(plan);
    let ctx = super::context::PlanContext {
        plan,
        global_stats: &stats,
    };

    let mut findings = Vec::new();
    self.walk_node(&plan.root, &ctx, &mut findings, &mut Vec::new());

    for rule in &self.rules {
        findings.extend(rule.check_global(plan, &stats));
    }

    findings.retain(|f| !self.config.disabled_rules.contains(&f.rule_id));

    DiagnosticReport { findings, stats }
}

fn walk_node(
    &self,
    node: &crate::model::PlanNode,
    ctx: &super::context::PlanContext,
    findings: &mut Vec<Finding>,
    ancestors: &mut Vec<&crate::model::PlanNode>,
) {
    for rule in &self.rules {
        if let Some(finding) = rule.check_with_ancestors(node, ctx, ancestors) {
            findings.push(finding);
        }
    }
    ancestors.push(node);
    for child in &node.children {
        self.walk_node(child, ctx, findings, ancestors);
    }
    ancestors.pop();
}
```

**Step 2: 验证全量测试通过(行为不变)**

Run: `cargo test --workspace`
Expected: 全部 317 测试通过(因为 check_with_ancestors 默认委托给 check)。

**Step 3: Commit**

```bash
git add crates/ogexplain-core/src/analyzer/config.rs
git commit -m "refactor(analyzer): track ancestor chain in walk_node DFS

Engine now passes ancestor path to check_with_ancestors(). No behavior
change — all rules still use default impl that delegates to check()."
```

---

## Phase 2: SCAN-001 修复 (P0)

**目标**:修复 SCAN-001 的 8 个 FN(LIMIT/filter 截断)+ 15 个 FP(HashJoin build 误报)。

### Task 2.1: 实现 effective_scan_size 工具函数

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/utils.rs`

**背景**:`actual.rows` 是节点输出行数。需要计算实际扫描行数。

**EXPLAIN 语义**(基于 benchmark 数据验证):
- Seq Scan **无 Filter** 时:`estimated.plan_rows` = 表大小(LIMIT 在父节点,不影响)
- Seq Scan **有 Filter** 时:`estimated.plan_rows` = 估算输出行数(非表大小),需要 `(actual.rows × loops) + Rows Removed by Filter`
- openGauss 中 `Rows Removed by Filter` 是跨所有 loop 的总和(非 per-loop)

**Step 1: 写失败测试**

在 `tests/analyzer_tests.rs` 末尾添加:

```rust
// ---------------------------------------------------------------------------
// effective_scan_size utility tests
// ---------------------------------------------------------------------------

#[test]
fn effective_scan_size_no_filter_uses_estimated() {
    // Seq Scan without filter: estimated.plan_rows = table size
    let explain = "Seq Scan on rental  (cost=0.00..318.72 rows=16472 width=40) (actual time=0.716..0.716 rows=10 loops=1)\nTotal runtime: 3.544 ms";
    let plan = parse(explain).unwrap();
    let node = &plan.root;
    let size = ogexplain_core::analyzer::rules::utils::effective_scan_size(node);
    assert!((size - 16472.0).abs() < 0.01, "expected 16472, got {}", size);
}

#[test]
fn effective_scan_size_with_filter_uses_actual_plus_removed() {
    // Seq Scan with filter: actual output + rows removed = total scanned
    let explain = "Seq Scan on payment  (cost=0.00..322.61 rows=107 width=0) (actual time=3.316..46.940 rows=114 loops=7)\n  Rows Removed by Filter: 15935\nTotal runtime: 47.0 ms";
    let plan = parse(explain).unwrap();
    let node = &plan.root;
    let size = ogexplain_core::analyzer::rules::utils::effective_scan_size(node);
    // (114 × 7) + 15935 = 16733
    assert!((size - 16733.0).abs() < 0.01, "expected 16733, got {}", size);
}
```

**Step 2: 运行测试确认失败**

Run: `cargo test --test analyzer_tests effective_scan_size --`
Expected: 编译失败,`effective_scan_size` 不存在。

**Step 3: 实现函数**

在 `crates/ogexplain-core/src/analyzer/rules/utils.rs` 中添加:

```rust
use crate::model::PlanNode;

/// Calculate the effective number of rows a scan node EXAMINED (not just output).
///
/// `actual.rows` is the OUTPUT row count, which is artificially low when:
/// - A parent LIMIT node truncates output
/// - A Filter removes most rows
///
/// This function reconstructs the true scan size:
/// - No Filter: `estimated.plan_rows` is the full table size
/// - With Filter: `(actual.rows × loops) + Rows Removed by Filter`
pub fn effective_scan_size(node: &PlanNode) -> f64 {
    let has_filter = node.properties.iter().any(|p| p.label == "Filter");

    if !has_filter {
        // No filter → estimated.plan_rows = table size (LIMIT doesn't affect child's estimate)
        return node.estimated.as_ref().map(|e| e.plan_rows).unwrap_or(0.0);
    }

    // Has filter → reconstruct from actual stats
    let actual = match node.actual.as_ref() {
        Some(a) => a,
        None => return node.estimated.as_ref().map(|e| e.plan_rows).unwrap_or(0.0),
    };

    let rows_removed: f64 = node
        .properties
        .iter()
        .find(|p| p.label == "Rows Removed by Filter")
        .and_then(|p| p.value.trim().parse::<f64>().ok())
        .unwrap_or(0.0);

    // actual.rows is per-loop average; rows_removed is total across all loops (OG behavior)
    (actual.rows * actual.loops) + rows_removed
}
```

**Step 4: 在 utils pub 导出**

确保 `utils.rs` 的 pub 可见性正确(mod.rs 中已是 `pub mod utils`)。

**Step 5: 运行测试确认通过**

Run: `cargo test --test analyzer_tests effective_scan_size --`
Expected: 2 个测试通过。

**Step 6: Commit**

```bash
git add crates/ogexplain-core/src/analyzer/rules/utils.rs tests/analyzer_tests.rs
git commit -m "feat(analyzer): add effective_scan_size utility

Reconstructs true scan size from EXPLAIN stats:
- No filter: uses estimated.plan_rows (table size)
- With filter: actual.rows × loops + Rows Removed by Filter"
```

---

### Task 2.2: 修复 SCAN-001 使用 effective_scan_size

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs:34-69`

**Step 1: 写失败测试(基于 benchmark FN case 0001)**

在 `tests/analyzer_tests.rs` 的 SCAN-001 区域添加:

```rust
#[test]
fn scan_001_triggers_for_limit_bound_seq_scan() {
    // Benchmark case OGEXP-GT-2026-0001: SELECT * FROM rental LIMIT 10
    // Seq Scan actual.rows=10 (LIMIT), but table has 16472 rows
    let explain = "\
Seq Scan on rental  (cost=0.00..318.72 rows=16472 width=40) (actual time=0.716..0.716 rows=10 loops=1)\n\
Total runtime: 3.544 ms";
    let plan = parse(explain).unwrap();
    let report = analyze(&plan);
    assert!(
        has_finding(&report, "SCAN-001"),
        "SCAN-001 should fire: table has 16472 rows even though LIMIT caps output at 10"
    );
}

#[test]
fn scan_001_triggers_for_filter_high_selectivity() {
    // Benchmark case OGEXP-GT-2026-0008: SELECT COUNT(*) FROM payment WHERE amount > 10.00
    // actual.rows=114 × loops=7, Rows Removed by Filter: 15935 → total scanned ≈ 16733
    let explain = "\
Aggregate  (cost=322.88..322.89 rows=1 width=8) (actual time=47.000..47.000 rows=1 loops=1)\n\
  ->  Partitioned Seq Scan on payment  (cost=0.00..322.61 rows=107 width=0) (actual time=3.316..46.940 rows=114 loops=7)\n\
        Filter: (amount > 10.00)\n\
        Rows Removed by Filter: 15935\n\
Total runtime: 47.0 ms";
    let plan = parse(explain).unwrap();
    let report = analyze(&plan);
    let finding = get_finding(&report, "SCAN-001")
        .expect("SCAN-001 should fire: 16733 rows scanned despite high-selectivity filter");
    assert!(finding.detail.contains("16733") || finding.detail.contains("payment"));
}
```

**Step 2: 运行确认失败**

Run: `cargo test --test analyzer_tests scan_001_triggers_for_limit scan_001_triggers_for_filter --`
Expected: FAIL — 当前 `actual.rows=10` 和 `actual.rows=114` 都 < 10000 阈值。

**Step 3: 修改 SCAN-001 的 check 方法**

将 `scan_rules.rs:34-69` 中的 `check` 改为 `check_with_ancestors`,并用 `effective_scan_size`:

```rust
impl DiagnosticRule for LargeTableFullScan {
    fn id(&self) -> &str { "SCAN-001" }
    fn name(&self) -> &str { "Large table full scan" }
    fn severity(&self) -> Severity { Severity::Warning }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::ScanEfficiency }

    fn check_with_ancestors(
        &self,
        node: &PlanNode,
        _ctx: &PlanContext,
        ancestors: &[&PlanNode],
    ) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan && node.node_type != NodeType::PartitionedSeqScan {
            return None;
        }

        // Skip scans that are inputs to a Hash Join (legitimate full scans for hash build/probe)
        if has_hash_join_ancestor(ancestors) {
            return None;
        }

        let scan_size = super::utils::effective_scan_size(node);
        if scan_size <= self.threshold {
            return None;
        }

        let relation = node.relation.as_deref().unwrap_or("unknown");
        let filter_cols = extract_filter_columns(node);

        let mut detail = format!(
            "Seq Scan on {} scanned ~{} rows (threshold: {})",
            relation, scan_size as u64, self.threshold as u64
        );
        if let Some(filter) = get_property_value(node, "Filter") {
            detail.push_str(&format!(", Filter: {}", filter));
        }

        let suggestion = match filter_cols {
            Some(cols) if !cols.is_empty() => format!(
                "CREATE INDEX ON {} ({}); 全表扫描大量行, 过滤列适合建索引",
                relation,
                cols.join(", ")
            ),
            _ => format!(
                "Consider creating an index on the filtered columns of {}",
                relation
            ),
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}
```

**Step 4: 实现 has_hash_join_ancestor 辅助函数**

在 `scan_rules.rs` 底部添加:

```rust
/// Check if any ancestor node is a Hash Join or Hash node.
/// Scans under Hash Join are legitimate (need full table for hash table build/probe).
fn has_hash_join_ancestor(ancestors: &[&PlanNode]) -> bool {
    use crate::model::NodeType;
    ancestors.iter().any(|n| {
        matches!(
            n.node_type,
            NodeType::HashJoin | NodeType::Hash | NodeType::VectorHashJoin
        )
    })
}
```

**Step 5: 运行全部 SCAN-001 测试**

Run: `cargo test --test analyzer_tests scan_001 --`
Expected: 全部通过,包括新的 LIMIT 和 filter 测试,以及已有的 `scan_001_triggers_on_large_seq_scan` 和 `scan_001_does_not_trigger_for_small_table`。

**Step 6: 检查已有 fixture 测试是否兼容**

Run: `cargo test --workspace`
Expected: 317 + 4 新测试 = 321 测试通过。

> **注意**: 如果 `scan_001_triggers_on_large_seq_scan`(fixture `10_complex_plan.txt`)失败,检查该 fixture 里 line_items 的 scan 是否在 HashJoin 下。如果是,需要调整:要么 fixture 里的 scan 不该被跳过(probe side 但不在 Hash 下),要么测试期望需要更新。

**Step 7: Commit**

```bash
git add crates/ogexplain-core/src/analyzer/rules/scan_rules.rs tests/analyzer_tests.rs
git commit -m "fix(scan-001): use effective_scan_size + skip hash join inputs

Fixes Issue #17 P0 — SCAN-001 was checking actual.rows (post-LIMIT output)
instead of true scan size. Now uses effective_scan_size() which:
- No filter: reads estimated.plan_rows (table size, unaffected by LIMIT)
- With filter: actual.rows × loops + Rows Removed by Filter

Also skips scans under HashJoin/Hash nodes (legitimate full scans for
hash table build/probe). Fixes 8 FN + 15 FP from benchmark evaluation."
```

---

### Task 2.3: 添加 HashJoin FP 回归测试

**Files:**
- Create: `tests/fixtures/32_hashjoin_seqscan_fp.txt`
- Modify: `tests/analyzer_tests.rs`

**Step 1: 创建 fixture(基于 benchmark case 0015 简化)**

```
Limit  (cost=10.00..20.00 rows=20 width=100) (actual time=5.000..5.000 rows=20 loops=1)
  ->  Hash Join  (cost=5.00..8.00 rows=100 width=80) (actual time=3.000..4.000 rows=100 loops=1)
        Hash Cond: (r.customer_id = p.customer_id)
        ->  Seq Scan on rental r  (cost=0.00..318.72 rows=16044 width=40) (actual time=0.116..2.616 rows=16044 loops=1)
        ->  Hash  (cost=2.00..2.00 rows=100 width=40) (actual time=1.000..1.000 rows=100 loops=1)
              ->  Seq Scan on customer c  (cost=0.00..2.00 rows=100 width=40) (actual time=0.010..0.500 rows=100 loops=1)
Total runtime: 5.5 ms
```

**Step 2: 写测试**

```rust
#[test]
fn scan_001_does_not_fire_for_hashjoin_build_scan() {
    let report = analyze_fixture("32_hashjoin_seqscan_fp.txt");
    // rental has 16044 rows but is under HashJoin → legitimate, not SCAN-001
    assert!(
        !has_finding(&report, "SCAN-001"),
        "SCAN-001 should NOT fire for Seq Scan under HashJoin (legitimate full scan for join)"
    );
}
```

**Step 3: 运行测试确认通过**

Run: `cargo test --test analyzer_tests scan_001_does_not_fire_for_hashjoin --`
Expected: PASS。

**Step 4: Commit**

```bash
git add tests/fixtures/32_hashjoin_seqscan_fp.txt tests/analyzer_tests.rs
git commit -m "test(scan-001): add HashJoin build-side FP regression test"
```

---

## Phase 3: Finding 后处理 — Severity 排序 + 去重 (P1)

**目标**:Critical 优先于 Warning 优先于 Info;同节点多规则按 severity 取最高。

### Task 3.1: 在 analyze() 中添加 severity 排序

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/config.rs:41-58`

**Step 1: 写失败测试**

在 `tests/analyzer_tests.rs` 添加:

```rust
#[test]
fn findings_sorted_by_severity() {
    // Fixture 10 has multiple findings of varying severity
    let report = analyze_fixture("10_complex_plan.txt");
    // Verify critical findings come before warning, warning before info
    let mut last_rank = 0usize;
    for f in &report.findings {
        let rank = match f.severity {
            Severity::Critical => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        };
        assert!(
            rank >= last_rank,
            "Findings not sorted by severity: {} ({:?}) after rank {}",
            f.rule_id,
            f.severity,
            last_rank
        );
        last_rank = rank;
    }
}
```

**Step 2: 运行确认失败**

Run: `cargo test --test analyzer_tests findings_sorted_by_severity --`
Expected: 可能 PASS 也可能 FAIL,取决于当前 fixture 的规则顺序。

**Step 3: 在 analyze() 的 return 前添加排序**

```rust
// In config.rs analyze(), before DiagnosticReport { findings, stats }:
use super::report::Severity;
findings.sort_by_key(|f| f.severity.clone());
```

> `Severity` 已实现 `Ord`(Critical=0 < Warning=1 < Info=2),可直接 sort。

**Step 4: 运行确认通过**

Run: `cargo test --test analyzer_tests findings_sorted_by_severity --`
Expected: PASS。

**Step 5: Commit**

```bash
git add crates/ogexplain-core/src/analyzer/config.rs tests/analyzer_tests.rs
git commit -m "feat(analyzer): sort findings by severity (critical first)"
```

---

### Task 3.2: 同节点高 severity 抑制低 severity(可选配置)

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/config.rs` (DiagnosticConfig)
- Modify: `crates/ogexplain-core/src/analyzer/config.rs` (analyze post-processing)

**设计决策**:不做"同节点只保留一条"的硬性去重——因为一个节点可以同时有多个真实问题(如 SCAN-001 + EST-001)。改为提供可选的 `dedup_per_node` 配置,默认关闭。

**Step 1: 在 DiagnosticConfig 添加字段**

```rust
pub struct DiagnosticConfig {
    // ... existing fields ...
    /// When true, if multiple findings target the same node (by node_line),
    /// keep only the highest-severity one. Default: false.
    pub dedup_per_node: bool,
}

impl Default for DiagnosticConfig {
    fn default() -> Self {
        Self {
            // ... existing defaults ...
            dedup_per_node: false,
        }
    }
}
```

**Step 2: 在 analyze() 中实现可选去重**

在 severity 排序之后、return 之前:

```rust
findings.sort_by_key(|f| f.severity.clone());

if self.config.dedup_per_node {
    let mut seen_nodes: std::collections::HashSet<usize> = std::collections::HashSet::new();
    findings.retain(|f| {
        match f.node_line {
            Some(line) => seen_nodes.insert(line),
            None => true, // findings without node_line are always kept
        }
    });
}
```

> 逻辑:sort 后 critical 在前。retain 用 HashSet 的 `insert` 返回值——首次见到该 node_line 时返回 true(保留),后续重复返回 false(丢弃)。效果:同节点只保留 severity 最高的那条。

**Step 3: 写测试**

```rust
#[test]
fn dedup_per_node_keeps_highest_severity() {
    let plan = parse_fixture("10_complex_plan.txt");
    let config = DiagnosticConfig {
        dedup_per_node: true,
        ..Default::default()
    };
    let report = analyze_with_config(&plan, &config);
    // No two findings should share the same node_line
    let mut lines: Vec<_> = report.findings.iter().filter_map(|f| f.node_line).collect();
    lines.sort();
    let initial_len = lines.len();
    lines.dedup();
    assert_eq!(lines.len(), initial_len, "duplicate node_lines found in dedup mode");
}
```

**Step 4: 运行测试**

Run: `cargo test --test analyzer_tests dedup_per_node --`
Expected: PASS。

**Step 5: Commit**

```bash
git add crates/ogexplain-core/src/analyzer/config.rs tests/analyzer_tests.rs
git commit -m "feat(analyzer): optional per-node dedup (keep highest severity)

Adds DiagnosticConfig::dedup_per_node (default: false). When enabled,
multiple findings on the same node are reduced to the highest-severity one.
Addresses Issue #17 §5.2 — no more low-severity noise drowning critical findings."
```

---

## Phase 4: CLI 配置暴露 (P2)

**目标**:让 CLI 用户能调阈值和加载 TOML 配置文件。

### Task 4.1: DiagnosticConfig 添加 from_toml()

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/config.rs`

**Step 1: 添加 Deserialize 和 from_toml**

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DiagnosticConfig {
    pub large_table_rows: f64,
    pub memory_threshold_kb: f64,
    pub estimation_skew_factor: f64,
    pub nested_loop_inner_rows: f64,
    pub sort_time_ratio: f64,
    pub max_plan_depth: usize,
    pub disabled_rules: Vec<String>,
    pub dedup_per_node: bool,
}

impl Default for DiagnosticConfig {
    // ... unchanged ...
}

impl DiagnosticConfig {
    /// Load config from a TOML string. Missing fields use defaults.
    pub fn from_toml_str(toml_str: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_str)
    }

    /// Load config from a TOML file.
    pub fn from_file(path: &std::path::Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
```

> **注意**: `#[serde(default)]` 确保部分 TOML 文件只覆盖指定字段,其余用 default。需要在 `Cargo.toml` 的 `ogexplain-core` features 中确认 `serde` 的 `derive` feature 已启用(应该已有)。`toml` crate 也已是依赖。

**Step 2: 写测试**

```rust
#[test]
fn config_from_toml_partial() {
    let toml = r#"
large_table_rows = 1000
dedup_per_node = true
"#;
    let config = DiagnosticConfig::from_toml_str(toml).unwrap();
    assert!((config.large_table_rows - 1000.0).abs() < 0.01);
    assert!(config.dedup_per_node);
    // Untouched fields keep defaults
    assert!((config.memory_threshold_kb - 102400.0).abs() < 0.01);
    assert!((config.estimation_skew_factor - 100.0).abs() < 0.01);
}
```

**Step 3: Commit**

```bash
git add crates/ogexplain-core/src/analyzer/config.rs tests/analyzer_tests.rs Cargo.toml
git commit -m "feat(config): add TOML deserialization for DiagnosticConfig

Supports partial config files (serde(default)). Users can now write:

  large_table_rows = 1000
  dedup_per_node = true

and load via DiagnosticConfig::from_file()."
```

---

### Task 4.2: CLI 添加 --config 和阈值 flags

**Files:**
- Modify: `crates/ogexplain-cli/src/lib.rs`

**Step 1: 添加 CLI 参数(在 analyze 子命令的 Args struct)**

找到 analyze 子命令的 clap struct,添加字段:

```rust
/// Diagnostic config file (TOML format). Overrides defaults.
#[arg(long, global = true)]
config_file: Option<PathBuf>,

/// Large table row threshold for SCAN-001 (default: 10000)
#[arg(long)]
large_table_rows: Option<f64>,

/// Nested loop inner rows threshold for JOIN-001 (default: 10000)
#[arg(long)]
nested_loop_threshold: Option<f64>,

/// Estimation skew factor for EST-001 (default: 100.0)
#[arg(long)]
estimation_skew_factor: Option<f64>,
```

**Step 2: 在 analyze 命令处理中构建 config**

```rust
fn build_config(args: &AnalyzeArgs) -> anyhow::Result<DiagnosticConfig> {
    // Start from file or default
    let mut config = match &args.config_file {
        Some(path) => DiagnosticConfig::from_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to load config from {:?}: {}", path, e))?,
        None => DiagnosticConfig::default(),
    };

    // CLI flags override file values
    if let Some(v) = args.large_table_rows {
        config.large_table_rows = v;
    }
    if let Some(v) = args.nested_loop_threshold {
        config.nested_loop_inner_rows = v;
    }
    if let Some(v) = args.estimation_skew_factor {
        config.estimation_skew_factor = v;
    }

    Ok(config)
}
```

**Step 3: 替换 analyze 调用**

将 CLI 中 `let diag = ogexplain_core::analyze(&plan);` 改为:

```rust
let config = build_config(&args)?;
let diag = ogexplain_core::analyze_with_config(&plan, &config);
```

**Step 4: 手动测试**

```bash
# Test with custom threshold
cargo run -p ogexplain-cli -- analyze tests/fixtures/01_simple_seq_scan.txt --large-table-rows 50

# Test with config file
echo 'large_table_rows = 50' > /tmp/test-config.toml
cargo run -p ogexplain-cli -- analyze tests/fixtures/01_simple_seq_scan.txt --config-file /tmp/test-config.toml
```

Expected: 小表(100 行)现在触发 SCAN-001(因为阈值降到 50)。

**Step 5: Commit**

```bash
git add crates/ogexplain-cli/src/lib.rs
git commit -m "feat(cli): expose diagnostic thresholds via CLI flags + TOML config

Adds:
  --config-file <path>       Load TOML config
  --large-table-rows <N>     SCAN-001 threshold
  --nested-loop-threshold <N> JOIN-001 threshold
  --estimation-skew-factor <F> EST-001 threshold

Addresses Issue #17 §5.3 / P2 — thresholds were configurable in library
API but not exposed to CLI users."
```

---

## Phase 5: 重评估与回归保障

### Task 5.1: 提取 benchmark 关键 case 为永久回归测试

**Files:**
- Create: `tests/fixtures/33_limit_bound_scan.txt` (from case 0001)
- Create: `tests/fixtures/34_filter_high_selectivity.txt` (from case 0008)
- Create: `tests/fixtures/35_hashjoin_legitimate_scan.txt` (from case 0015)
- Modify: `tests/analyzer_tests.rs`

**Step 1: 从 benchmark case JSON 提取 EXPLAIN 文本**

从 `benchmark/03-build/cases/OGEXP-GT-2026-{0001,0008,0015}.json` 的 `input.explain_output` 字段提取。

创建 fixture 文件(仅保留 EXPLAIN 部分,去掉 SQL 注释)。

**Step 2: 写回归测试套件**

```rust
// ---------------------------------------------------------------------------
// Issue #17 regression: benchmark-derived cases
// ---------------------------------------------------------------------------

#[test]
fn regression_0001_limit_bound_seq_scan_fires_scan_001() {
    let report = analyze_fixture("33_limit_bound_scan.txt");
    assert!(has_finding(&report, "SCAN-001"),
        "Issue #17: LIMIT-bound Seq Scan on 16K-row table must fire SCAN-001");
}

#[test]
fn regression_0008_filter_high_selectivity_fires_scan_001() {
    let report = analyze_fixture("34_filter_high_selectivity.txt");
    assert!(has_finding(&report, "SCAN-001"),
        "Issue #17: Seq Scan with high-selectivity filter on 16K-row table must fire SCAN-001");
}

#[test]
fn regression_0015_hashjoin_scan_does_not_fire_scan_001() {
    let report = analyze_fixture("35_hashjoin_legitimate_scan.txt");
    assert!(!has_finding(&report, "SCAN-001"),
        "Issue #17: Seq Scan under HashJoin is legitimate, should NOT fire SCAN-001");
}
```

**Step 3: 运行全部测试**

Run: `cargo test --workspace`
Expected: 全部通过。

**Step 4: Commit**

```bash
git add tests/fixtures/33_limit_bound_scan.txt tests/fixtures/34_filter_high_selectivity.txt tests/fixtures/35_hashjoin_legitimate_scan.txt tests/analyzer_tests.rs
git commit -m "test: add Issue #17 benchmark regression fixtures

Three permanent regression tests derived from benchmark cases:
- 0001: LIMIT-bound scan (was FN)
- 0008: filter high-selectivity (was FN)
- 0015: HashJoin scan (was FP)"
```

---

### Task 5.2: 重跑 benchmark 评估

**Step 1: 构建 release**

```bash
cargo build --release
```

**Step 2: 重跑评估**

```bash
python3 benchmark/04-evaluate/evaluate.py \
    --mode live \
    --cases benchmark/03-build/cases/ \
    --output benchmark/04-evaluate/live_results_v0.3.1/ \
    --ogexplain-binary ./target/release/ogexplain
```

**Step 3: 对比 v0.3.0 vs v0.3.1 的指标**

关注:
- Case-level Recall: 8.5% → 目标 >30%(仅 SCAN-001 修复就应大幅提升)
- SCAN-001: 0 TP → 目标 >5 TP
- SCAN-001 FP: 15 → 目标 <5(HashJoin skip 消除大部分)
- Overall FP: 30 → 应下降(排序不减少 FP,但 HashJoin skip 会)

**Step 4: Commit 结果**

```bash
git add benchmark/04-evaluate/live_results_v0.3.1/
git commit -m "chore(benchmark): re-evaluate after Issue #17 fixes (v0.3.1)"
```

---

## 验证检查清单

每个 Phase 完成后运行:

```bash
# 1. 编译
cargo build --workspace

# 2. Clippy 零警告
cargo clippy --workspace -- -D warnings

# 3. 格式
cargo fmt --all -- --check

# 4. 全量测试
cargo test --workspace

# 5. 关键回归(Phase 5 完成后)
cargo test --test analyzer_tests regression_
cargo test --test analyzer_tests scan_001
cargo test --test analyzer_tests findings_sorted
cargo test --test analyzer_tests dedup_per_node
```

---

## 不在本次范围内的事项

| 项目 | 原因 | 建议跟进 |
|------|------|---------|
| Spill 规则阈值化(JOIN-002/MEM-001) | 需要更大的 benchmark 数据集验证(当前 16K 行触发不了 spill) | 数据集 v2 扩容到 100w 行后再做 |
| TYPE-001 检测优化器已消除的 cast | 需要检查原始 SQL 而非 EXPLAIN,架构变更大 | 单独 issue 跟进 |
| EST/STATS 系列规则验证 | openGauss 7.0-RC1 不支持 DELETE STATISTICS | 等 OG 支持后或数据集 v1.1 换语法 |
| PART-001 EXTRACT() 包装识别 | 需要分区表达式解析能力 | 单独 issue |
| Suggester 适配新的 finding 排序 | Suggester 从 findings 读取,排序变化不影响其逻辑 | 验证即可,预计无需改动 |

---

## 风险评估

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| `effective_scan_size` 对 "Rows Removed by Filter" 语义假设错误(per-loop vs total) | 中 | SCAN-001 对 partitioned scan 行数算错 | Task 2.1 的单元测试用 benchmark 真实数据验证;如果算错,改为 `rows_removed` 不乘 loops |
| `has_hash_join_ancestor` 过于激进,跳过了应该报告的 scan | 低 | 漏报 | 只跳过 HashJoin/Hash,不跳 NestedLoop( NestedLoop 由 JOIN-001 管);回归测试验证 |
| Severity 排序破坏了 insta 快照 | 高 | 快照测试失败 | `cargo insta review` 接受新快照顺序 |
| `dedup_per_node` 丢失有用信息 | 低 | 漏报 | 默认 false,用户可选开启 |
