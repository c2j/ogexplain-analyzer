# 诊断规则质量提升实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 系统修复 ogexplain-analyzer 在 38-case 真实样本中暴露的诊断质量问题，将 TYPE-001 / SCAN-004 / SUBQ-001 三条规则的有效 finding 率从当前 ~30% 提升到 >80%，并补齐 BitmapHeapScan 关键覆盖盲区。

**Architecture:** 四个独立修复阶段——(1) 提取列名/表名解析公共工具；(2) 重写 TYPE-001 触发条件（asymmetric cast 检测）；(3) 修复 SUBQ-001 表名提取并按子查询树聚合降噪；(4) 扩展 SCAN-004 到 BitmapHeapScan。每阶段独立可提交，TDD 驱动。

**Tech Stack:** Rust, ogexplain-core crate（`analyzer/rules/`、`analyzer/rules/utils.rs`），insta snapshot testing，现有 benchmark 套件（`benchmark/04-evaluate/`），38-case 真实样本（`tests/cases_38.csv`）。

---

## 1. 背景与数据基线

### 1.1 真实样本分析（tests/cases_38.csv, 2026-06-27）

| 指标 | 数值 |
|---|---|
| 测试 case 数 | 38（累计 485.7s 执行时间） |
| 总 finding 数 | 700（Critical 207 / Warning 475 / Info 18） |
| 触发规则数 | 11 / 28（39% 覆盖率） |
| 完全无 finding 的 case | 2（case 7、22 — 实际都有严重问题） |

### 1.2 现有 benchmark 基线（benchmark/04-evaluate/live_results-v3/per_rule_metrics.json）

| 规则 | Precision | Recall | F1 | TP / FP / FN |
|---|---|---|---|---|
| **TYPE-001** | **0.0** | 0.0 | 0.0 | 0 / 1 / 0 |
| **SUBQ-001** | **0.43** | 1.0 | 0.6 | 3 / 4 / 0 |
| **SCAN-004** | **0.48** | 0.81 | 0.60 | 13 / 14 / 3 |
| SCAN-001 | 0.5 | 0.25 | 0.33 | 2 / 2 / 6 |

### 1.3 根因清单（代码级定位）

| ID | 规则 | 文件:行 | 根因 | 影响面 |
|---|---|---|---|---|
| **B1** | TYPE-001 | `type_coercion_rules.rs:114` | 正则 `r"(\w+)\s*=\s*'([^']*)'"` 在 `::text` 注解上误捕获 `col="text"` | 所有 TYPE-001 suggestion 输出 `WHERE text = ...` |
| **B2** | TYPE-001 | `type_coercion_rules.rs:118-126` | 触发条件只看字面量 `parse::<f64>().is_ok()`，忽略两侧 `::text` 对称 cast | 15/15 真实样本 + 1 benchmark case 全为假阳性 |
| **B3** | SCAN-004 | `scan_rules.rs` (suggestion 构造处) | 同 B1，列名提取共用同一缺陷正则 | 32 条 SCAN-004 suggestion 不可用 |
| **B4** | SUBQ-001 | `subquery_rules.rs:31-36` | `node.children.first().relation` 仅看直接子节点，嵌套子查询拿不到表名 | 133/267（50%）显示"涉及表: unknown" |
| **B5** | SUBQ-001 | `subquery_rules.rs:49-59` | 对所有 SubPlan 出现逐节点触发，无聚合 | 单 case 最多 29 条同质 finding |
| **B6** | SCAN-004 | `scan_rules.rs` (节点类型 match) | 节点白名单缺 `BitmapHeapScan` | Case 22 的 BitmapHeapScan+Filter 漏报 |

---

## 2. 范围

### In Scope（本期 6 项）

- **P0-1**: TYPE-001 列名提取正则修复（B1）
- **P0-2**: TYPE-001 触发条件重设计——asymmetric cast 检测（B2）
- **P0-3**: SCAN-004 列名提取修复——共用 P0-1 工具（B3）
- **P0-4**: SUBQ-001 表名递归查找（B4）
- **P1-1**: SCAN-004 节点白名单扩展到 BitmapHeapScan（B6）
- **P2-1**: SUBQ-001 按子查询树聚合降噪（B5）

### Out of Scope（后续迭代）

- 新规则 JOIN-003（Hash Join Filter 大量过滤）—— 需独立设计文档
- 新规则 SCAN-005（Index Scan 选择性差）—— 需独立设计文档
- MEM-004 peak_memory 兜底解析 —— 涉及 parser 改造，独立工作项
- STATS-001 阈值调整 —— 影响 benchmark 标定，需先重定 ground truth
- PUSH-001/002、SKEW-001、DIST-001 —— 这批 case 都是单节点计划，分布式规则不在覆盖范围内
- SQL 模板聚类（P2-2）—— 涉及 ogsql-complexity crate 改造
- i18n 完善（P3）—— 文案工作，独立任务

### 显式假设

- 不修改 `DiagnosticRule` trait 接口（避免影响其他 22 条规则）
- 不变更 `DiagnosticConfig` 字段（避免破坏 CLI/MCP/TUI 兼容）
- 现有 insta snapshot 中受影响的会显式 review 更新

---

## 3. 验证策略

### 3.1 单元测试（每 Task 必须有）

- **正向测试**：应触发的样本触发，并断言 suggestion 中包含真实列名/表名
- **守护测试**：不应触发的样本（对称 cast、纯 text=text）不触发
- **结构断言**：Finding 的 severity / category / detail 字段格式

新增测试文件位置：
- `crates/ogexplain-core/src/analyzer/rules/type_coercion_rules.rs`（内联 `#[cfg(test)] mod tests`）
- `crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs`（同上）
- `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs`（同上）

### 3.2 回归测试

**主基准**：38-case 真实样本（`tests/cases_38.csv`）

执行：`cargo run -p ogexplain-cli --release -- analyze tests/cases_38.csv --input-format csv --output-columns focused --output /tmp/post_fix.csv`

对比指标（前后差异表）：

| 指标 | 当前 | 目标 |
|---|---|---|
| TYPE-001 触发数 | 15（全假阳性） | ≤ 5（真阳性） |
| TYPE-001 suggestion 列名正确率 | 0% | 100% |
| SUBQ-001 单 case 最大触发数 | 29 | ≤ 5 |
| SUBQ-001 表名 = "unknown" 占比 | 50% | < 10% |
| SCAN-004 suggestion 列名正确率 | 0% | > 90% |
| Case 22 finding 数 | 0 | ≥ 1（BitmapHeapScan 触发） |
| Case 7 finding 数 | 0 | 0（保持，新增 SCAN-005 是后续工作） |

**辅助基准**：benchmark 套件（`benchmark/04-evaluate/live_results-v3/`）

执行：参照 `lib/ogagila/benchmark/` 现有脚本重跑评估，对比 `per_rule_metrics.json`。

目标：
- TYPE-001 precision: 0.0 → ≥ 0.5（保守，待 ground truth 扩充）
- SUBQ-001 precision: 0.43 → ≥ 0.7
- SCAN-004 precision: 0.48 → ≥ 0.6（不降 recall）

### 3.3 insta snapshot 处理

执行 `cargo test --workspace` 后，受影响的 snapshot 文件会出现在 `tests/snapshots/`。运行 `cargo insta review` 逐个确认：
- TYPE-001 / SCAN-004 / SUBQ-001 相关 snapshot：**必须人工核对 suggestion 文案**
- 其他 snapshot：若无变化则通过；若变化需排查是否回归

### 3.4 编译与 lint

每个 Task 完成后：
```bash
cargo build --workspace 2>&1 | grep -E "error|warning" | head
cargo clippy --workspace -- -D warnings 2>&1 | tail -5
cargo fmt --all -- --check
```

---

## 4. 依赖关系

```
Phase 1 (utils 工具层) ─────┬──→ Phase 2 (TYPE-001 修复)
                            │
                            ├──→ Phase 3 (SCAN-004 修复 + BitmapHeapScan 扩展)
                            │
                            └──→ Phase 4 (SUBQ-001 表名 + 聚合)

Phase 5 (回归验证) ←── 依赖 Phase 1-4 全部完成
```

- Phase 2/3/4 互相独立，可并行（建议串行以便逐步 review）
- Phase 1 必须先完成，提供共享工具函数

---

## 5. 实施阶段

### Phase 1: 公共工具层 — 列名/表名提取

**目标**：把 `::cast` 注解处理、递归子树查找提取为可测、可复用的工具函数，给 Phase 2-4 共用。

**为什么先做**：当前 TYPE-001、SCAN-004 各自实现正则；SUBQ-001 自己写 `children.first()`。修复时如果不抽出公共逻辑，会三处重复代码，未来更难维护。

#### Task 1.1: 添加 `extract_column_from_filter` 工具函数

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/utils.rs`（末尾追加）
- Test: 同文件内 `#[cfg(test)] mod tests { ... }`（若不存在则新增）

**Step 1: 写失败的测试**

在 `utils.rs` 末尾添加（若已有 test module 则在其中追加）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_column_from_filter_basic() {
        // 简单情况：col = 'val'
        assert_eq!(
            extract_column_from_filter("(status)::text = 'ready'::text").unwrap(),
            "status"
        );
    }

    #[test]
    fn test_extract_column_from_filter_with_cast() {
        // 关键回归点：::cast 不能被当成列名
        assert_eq!(
            extract_column_from_filter("((facctcode)::text = '1002'::text)").unwrap(),
            "facctcode"
        );
    }

    #[test]
    fn test_extract_column_from_filter_nested_parens() {
        // 多层括号包裹
        assert_eq!(
            extract_column_from_filter("(((amount)::numeric = '100'::numeric))").unwrap(),
            "amount"
        );
    }

    #[test]
    fn test_extract_column_from_filter_no_match() {
        // 无 = 比较时返回 None
        assert!(extract_column_from_filter("col ~~ '%foo'").is_none());
    }

    #[test]
    fn test_extract_column_from_filter_complex_or() {
        // OR 链中取第一个 = 比较
        assert_eq!(
            extract_column_from_filter("(a = '1' OR b = '2')").unwrap(),
            "a"
        );
    }
}
```

**Step 2: 运行测试确认失败**

```bash
cargo test -p ogexplain-core utils::tests -- --nocapture
```

预期：编译失败（`extract_column_from_filter` 未定义）。

**Step 3: 实现工具函数**

在 `utils.rs` 中（`impl` 块外、其他 pub fn 旁）添加：

```rust
/// 从过滤条件文本中提取第一个 `col = 'literal'` 或 `col = literal` 比较的列名。
///
/// 正确处理 OpenGauss `::cast` 注解 —— 不会把 `text`/`numeric` 等 cast 类型
/// 当成列名。例如 `((facctcode)::text = '1002'::text)` 返回 `"facctcode"`。
///
/// 返回 `None` 当：
/// - 文本中没有 `<标识符> = <值>` 模式
/// - 标识符无法解析（如表达式左侧）
pub fn extract_column_from_filter(filter: &str) -> Option<String> {
    // 策略：先剥除所有 `::<type>` 注解，再做正则匹配。
    // `::<type>` 可能带括号参数（如 `::numeric(10,2)`），统一移除。
    let stripped = strip_cast_annotations(filter);
    // 匹配：identifier = 'literal'  或  identifier = number
    // identifier 允许字母/数字/下划线，必须在 word boundary 处
    let re = regex::Regex::new(r"(?:^|[[(\s,])([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*(?:'[^']*'|\d+)").ok()?;
    re.captures(&stripped)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .filter(|col| !is_reserved_type_name(col))
}

/// 剥除 SQL 表达式中的 `::<type>` cast 注解。
fn strip_cast_annotations(s: &str) -> String {
    // `::<type>` 或 `::<type>(args)` —— 移除 cast 部分，保留前导表达式
    let re = regex::Regex::new(r"::[a-zA-Z_][a-zA-Z0-9_]*(\([^)]*\))?").unwrap();
    re.replace_all(s, "").to_string()
}

/// 判断字符串是否是 SQL 类型名（避免把 cast 残留误识别为列名）。
fn is_reserved_type_name(s: &str) -> bool {
    matches!(
        s.to_lowercase().as_str(),
        "text" | "numeric" | "int" | "int4" | "int8" | "bigint" | "varchar"
        | "char" | "bpchar" | "float" | "float4" | "float8" | "double"
        | "precision" | "bool" | "boolean" | "date" | "timestamp" | "time"
    )
}
```

**Step 4: 运行测试确认通过**

```bash
cargo test -p ogexplain-core utils::tests -- --nocapture
```

预期：5 个测试全通过。

**Step 5: 检查现有 utils 测试无回归**

```bash
cargo test -p ogexplain-core --lib
```

预期：全通过。

**Step 6: 提交**

```bash
git add crates/ogexplain-core/src/analyzer/rules/utils.rs
git commit -m "feat(rules): add extract_column_from_filter utility with cast-aware parsing

Adds shared utility for extracting column names from SQL filter text.
Correctly handles OpenGauss ::<type> cast annotations - the prior regex
captured 'text' (the cast target) instead of the actual column name.

This is the foundation for fixing TYPE-001 and SCAN-004 column-name
extraction bugs identified in 38-case analysis."
```

---

#### Task 1.2: 添加 `find_first_scan_descendant` 工具函数

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/utils.rs`
- Test: 同文件 test 模块

**Step 1: 写失败的测试**

```rust
#[test]
fn test_find_first_scan_descendant_direct_child() {
    // 构造: SubqueryScan → SeqScan(table=foo)
    // 期望: 返回 Some("foo")
    // 注意: 这里用真实 PlanNode 构造，参考现有 utils 测试的构造方式
    // 实现者需参考 crates/ogexplain-core/src/model/plan.rs::tests 的 fixture 风格
    let subquery = /* PlanNode with child SeqScan on "foo" */;
    assert_eq!(find_first_scan_descendant(&subquery), Some("foo".to_string()));
}

#[test]
fn test_find_first_scan_descendant_nested() {
    // 构造: SubqueryScan → Limit → HashJoin → SeqScan(table=bar)
    let subquery = /* nested tree */;
    assert_eq!(find_first_scan_descendant(&subquery), Some("bar".to_string()));
}

#[test]
fn test_find_first_scan_descendant_no_scan() {
    // 构造: SubqueryScan → Sort → Result (无 scan 节点)
    let subquery = /* no-scan tree */;
    assert_eq!(find_first_scan_descendant(&subquery), None);
}
```

> **注**：测试构造真实 PlanNode 树比较繁琐，建议参考 `crates/ogexplain-core/src/model/plan.rs` 中已有的 `#[cfg(test)] mod tests` 内的构造辅助函数。

**Step 2: 运行确认失败**

```bash
cargo test -p ogexplain-core utils::tests::test_find_first_scan_descendant -- --nocapture
```

**Step 3: 实现**

在 `utils.rs` 中添加（紧邻其他 pub fn）：

```rust
use crate::model::{NodeType, PlanNode};

/// 在节点的子树中递归查找第一个扫描节点（SeqScan/IndexScan/CStoreScan 等），
/// 返回其 `relation` 字段。用于从 SubqueryScan 等包装节点中找出底层表名。
///
/// 搜索顺序：DFS 先左子节点，深度优先。
/// 返回 `None` 当子树中没有任何扫描节点。
pub fn find_first_scan_descendant(node: &PlanNode) -> Option<String> {
    // 当前节点本身是 scan？
    if is_scan_node(&node.node_type) {
        return node.relation.clone();
    }
    // DFS 子节点
    for child in &node.children {
        if let Some(r) = find_first_scan_descendant(child) {
            return Some(r);
        }
    }
    None
}
```

> **注**：`is_scan_node` 已存在（由 2025-05-22 计划引入），不需要新增。

**Step 4-6**: 运行测试、检查回归、提交（同 Task 1.1 模板）。

提交信息：
```
feat(rules): add find_first_scan_descendant utility

Recursively finds the first scan node in a subtree and returns its
relation name. Fixes SUBQ-001's 50% 'unknown' table name extraction
rate on nested subqueries.
```

---

### Phase 2: TYPE-001 触发条件重设计

**目标**：(a) 列名提取改用 Phase 1 工具；(b) 触发条件改为只在 asymmetric cast 时触发。

**为什么不直接打补丁**：当前 `detect_type_mismatch` 的设计假设 `::text` 在列侧意味着 numeric 被转 text。但 OG 在 `showimplicit=false` 时会把所有隐式 cast 隐藏，显式 cast 显示在两侧。这种设计假设错了 —— 必须重新设计判定逻辑。

#### Task 2.1: 重写 TYPE-001 触发条件

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/type_coercion_rules.rs:24-77`（`check` 方法）和 `:112-142`（`detect_type_mismatch`）
- Test: 同文件 `#[cfg(test)] mod tests`（若不存在则新增）

**Step 1: 写失败的测试（覆盖关键场景）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NodeType, PlanNode, ActualStats, EstimatedCost};

    fn make_seqscan_with_filter(filter: &str, rows_removed: f64, actual_rows: f64) -> PlanNode {
        // 构造一个带 Filter 和 Rows Removed by Filter 的 SeqScan 节点
        // 实现者参照 model/plan.rs 中已有的 test fixture
        todo!("implement fixture builder")
    }

    // === 正向测试：应当触发 TYPE-001 ===

    #[test]
    fn test_type001_fires_on_numeric_col_with_string_literal() {
        // 场景：numeric_col 直接和字符串字面量比较，无 cast（隐式转换被 OG 隐藏）
        // Filter: numeric_col = '1002'
        // 这是 TYPE-001 真正应该检测的模式
        let node = make_seqscan_with_filter("numeric_col = '1002'", 100_000.0, 1_000.0);
        let finding = SuspectedImplicitTypeCast.check(&node, &Default::default());
        assert!(finding.is_some(), "Should fire on numeric-looking literal");
        let f = finding.unwrap();
        assert!(f.suggestion.as_ref().unwrap().contains("numeric_col"),
                "suggestion must use real column name, got: {:?}", f.suggestion);
    }

    // === 守护测试：不应触发 ===

    #[test]
    fn test_type001_does_not_fire_on_symmetric_text_cast() {
        // 关键回归 case：两侧都有 ::text，是 text=text 比较（38-case 中 15 条假阳性的模式）
        // Filter: ((facctcode)::text = '1002'::text)
        let node = make_seqscan_with_filter(
            "((facctcode)::text = '1002'::text)",
            1_000_000.0, 50_000.0
        );
        let finding = SuspectedImplicitTypeCast.check(&node, &Default::default());
        assert!(finding.is_none(),
                "Must NOT fire on symmetric ::text cast (current false positive pattern)");
    }

    #[test]
    fn test_type001_does_not_fire_on_text_col_with_text_literal() {
        // 显式 text 列和 text 字面量比较（已知类型匹配）
        // Filter: status = 'ready'  （且 status 是 text/varchar 列）
        // 由于我们无法从 plan 中得知列的实际类型，这种 case 不应触发
        // 除非字面量是 numeric 形式（'1002'）且无 ::text cast —— 才有嫌疑
        let node = make_seqscan_with_filter("status = 'ready'", 100.0, 50.0);
        let finding = SuspectedImplicitTypeCast.check(&node, &Default::default());
        // status='ready' 不是 numeric 字面量，不应触发
        assert!(finding.is_none());
    }

    #[test]
    fn test_type001_does_not_fire_on_low_row_removal() {
        // 行删除比例 < 50%，不够显著
        let node = make_seqscan_with_filter("col = '100'", 5.0, 1000.0);
        let finding = SuspectedImplicitTypeCast.check(&node, &Default::default());
        assert!(finding.is_none());
    }

    #[test]
    fn test_type001_uses_real_column_name_in_suggestion() {
        // 关键回归：38-case case 4 的 (facctcode)::text = '1002'::text
        // 当前输出 "WHERE text = '1002'"，应输出包含 "facctcode" 的建议
        // 但因为两侧 cast 对称，新逻辑下应不触发 —— 此 case 改为非对称
        // Filter: facctcode = '1002'  （假设 OG 已剥除 cast）
        let node = make_seqscan_with_filter("facctcode = '1002'", 1_000_000.0, 50_000.0);
        let finding = SuspectedImplicitTypeCast.check(&node, &Default::default());
        if let Some(f) = finding {
            assert!(f.suggestion.unwrap().contains("facctcode"));
        }
    }
}
```

**Step 2: 运行确认失败**

```bash
cargo test -p ogexplain-core type_coercion_rules::tests -- --nocapture
```

预期：测试编译失败或断言失败。

**Step 3: 重写 check 和 detect_type_mismatch**

替换 `type_coercion_rules.rs:24-77`（`check`）和 `:79-142`（TypeMismatch + detect_type_mismatch）：

```rust
impl DiagnosticRule for SuspectedImplicitTypeCast {
    fn id(&self) -> &str { "TYPE-001" }
    fn name(&self) -> &str { "疑似隐式类型转换" }
    fn severity(&self) -> Severity { Severity::Critical }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::TypeMismatch }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan {
            return None;
        }
        let filter_prop = node.properties.iter().find(|p| p.label == "Filter")?;
        let filter_value = &filter_prop.value;

        // 必须能从 filter 中提取出真实列名（剥除 ::cast 干扰）
        let column = super::utils::extract_column_from_filter(filter_value)?;

        // 检测非对称 cast —— 只有这种才真正是隐式类型转换嫌疑
        let mismatch = detect_asymmetric_cast(filter_value, &column)?;

        // 行删除阈值（保留原逻辑）
        let rows_removed = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Filter")
            .and_then(|p| p.value.trim().parse::<f64>().ok())?;
        let actual_rows = node.actual.as_ref().map(|a| a.rows).unwrap_or(0.0);
        let total_scanned = rows_removed + actual_rows;

        if rows_removed <= 10.0 { return None; }
        if total_scanned > 0.0 && rows_removed / total_scanned <= 0.5 { return None; }

        let detail = format!(
            "Seq Scan 含过滤条件 '{}' ({}), 过滤掉 {} 行 (共 {} 行) — 疑似隐式类型转换导致无法使用索引",
            filter_value,
            mismatch.description(),
            rows_removed as i64,
            total_scanned as i64
        );
        let suggestion = mismatch.fix_suggestion();

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

struct TypeMismatch {
    column: String,
    literal_value: String,
    pattern: MismatchPattern,
}

enum MismatchPattern {
    /// 列无 cast，字面量是 numeric 形式的字符串：`col = '1002'`
    /// 嫌疑：col 实际是 numeric，被隐式转换以匹配字符串
    BareColumnStringLiteral,
    /// 列有 `::numeric` cast，字面量是字符串：`(col)::numeric = '1002'`
    /// 嫌疑：col 是 text/varchar，被强转 numeric
    ColumnToNumeric,
}

impl TypeMismatch {
    fn description(&self) -> String {
        match self.pattern {
            MismatchPattern::BareColumnStringLiteral => {
                format!("{} 列与字符串字面量 '{}' 比较 (疑为 numeric 列隐式转 string)",
                        self.column, self.literal_value)
            }
            MismatchPattern::ColumnToNumeric => {
                format!("{} 列被显式 ::numeric cast 与字符串 '{}' 比较",
                        self.column, self.literal_value)
            }
        }
    }

    fn fix_suggestion(&self) -> String {
        match self.pattern {
            MismatchPattern::BareColumnStringLiteral => format!(
                "WHERE {} = {} — 若 {} 是 numeric 列, 移除字面量引号; 若是 text 列, 添加显式 cast: WHERE {}::text = '{}'",
                self.column, self.literal_value,
                self.column,
                self.column, self.literal_value
            ),
            MismatchPattern::ColumnToNumeric => format!(
                "WHERE {} = '{}' — {} 可能是 text 列被强转 numeric, 改为: WHERE {} = '{}' (确保两侧类型一致)",
                self.column, self.literal_value,
                self.column,
                self.column, self.literal_value
            ),
        }
    }
}

/// 检测**非对称** cast 模式 —— TYPE-001 触发的核心条件。
///
/// **不触发**的情况（关键：原逻辑误判的来源）：
/// - `(col)::text = 'literal'::text` —— 两侧都是 ::text，是 text=text 比较
/// - `(col)::numeric = 'literal'::numeric` —— 两侧同 cast
///
/// **触发**的情况：
/// - `col = '1002'` —— 列无 cast，字面量是 numeric 形式字符串
/// - `(col)::numeric = '1002'` —— 列 cast 到 numeric，字面量无 cast（OG 隐藏了 string→numeric）
fn detect_asymmetric_cast(filter: &str, column: &str) -> Option<TypeMismatch> {
    // 提取该 column 对应的比较右侧字面量
    // 构造匹配该 column 的正则：col<optional cast> = <literal>
    let col_pat = regex::escape(column);
    // 模式 A: col = 'literal'  （col 无 cast，literal 是字符串）
    let re_a = regex::Regex::new(&format!(
        r"{}\s*=\s*'([^']+)'", col_pat
    )).ok()?;
    if let Some(cap) = re_a.captures(filter) {
        let val = cap.get(1)?.as_str();
        // 仅当字面量是 numeric 形式时才视为嫌疑（避免 status='ready' 这种）
        if val.parse::<f64>().is_ok() {
            // 进一步检查：col 侧是否在源 filter 中带 ::text cast？
            // 若是，且字面量也带 ::text —— 已被外层守门（symmetric 检测）
            // 这里到达说明 asymmetric
            return Some(TypeMismatch {
                column: column.to_string(),
                literal_value: val.to_string(),
                pattern: MismatchPattern::BareColumnStringLiteral,
            });
        }
    }
    // 模式 B: (col)::numeric = 'literal'  （列被 cast 到 numeric）
    let re_b = regex::Regex::new(&format!(
        r"\(\s*{}\s*\)::numeric\s*=\s*'([^']+)'", col_pat
    )).ok()?;
    if let Some(cap) = re_b.captures(filter) {
        let val = cap.get(1)?.as_str().to_string();
        return Some(TypeMismatch {
            column: column.to_string(),
            literal_value: val,
            pattern: MismatchPattern::ColumnToNumeric,
        });
    }
    None
}
```

**Step 4: 运行测试确认通过**

```bash
cargo test -p ogexplain-core type_coercion_rules::tests -- --nocapture
```

**Step 5: 更新受影响的 insta snapshot**

```bash
cargo test --workspace 2>&1 | grep -E "snapshot|NEW"
cargo insta review
```

> 预期：原 17_implicit_cast.txt fixture 的 snapshot 可能变化（需核对），其他 fixture 中 TYPE-001 触发可能消失（核对是正确的假阳性移除）。

**Step 6: 提交**

```
fix(rules): TYPE-001 asymmetric cast detection + column name extraction

BREAKING CHANGE: TYPE-001 no longer fires on symmetric ::text casts
((col)::text = 'literal'::text), which are valid text-vs-text comparisons.

The rule now fires only on true asymmetric patterns:
- Bare column with numeric-looking string literal: col = '1002'
- Column cast to numeric vs uncast string literal: (col)::numeric = '1002'

Also fixes column name extraction by using the new
extract_column_from_filter utility. Prior regex captured the literal
'text' from ::text annotations instead of the actual column name.

Impact on 38-case sample:
- TYPE-001 findings: 15 (all false positives) -> expected ~0-3 (true positives)
- All suggestions now use real column names

Benchmark baseline (live_results-v3): TYPE-001 precision 0.0 -> expected ≥0.5
```

---

### Phase 3: SCAN-004 修复

#### Task 3.1: SCAN-004 列名提取改用公共工具

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs`（FilterWithoutIndex 的 suggestion 构造段）
- Test: `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs` 新增 test module

**Step 1: 写失败的测试**

```rust
#[test]
fn test_scan004_suggestion_uses_real_column_name() {
    // 关键回归 case (38-case case 4)：
    // SeqScan on dat_zl_accountinfo, Filter: ((facctcode)::text = '1002'::text)
    // 当前建议: CREATE INDEX ON dat_zl_accountinfo (text)
    // 期望建议: CREATE INDEX ON dat_zl_accountinfo (facctcode)
    let node = /* construct SeqScan with relation="dat_zl_accountinfo",
                  Filter "((facctcode)::text = '1002'::text)",
                  Rows Removed by Filter=122294417 */;
    let finding = FilterWithoutIndex.check(&node, &Default::default());
    let s = finding.unwrap().suggestion.unwrap();
    assert!(s.contains("facctcode"), "expected column name 'facctcode', got: {}", s);
    assert!(!s.contains("(text)"), "must not contain literal '(text)'");
}
```

**Step 2-6**: 失败验证、修改 SCAN-004 调用 `extract_column_from_filter`、通过测试、snapshot review、提交。

> **注**：实现者需先读 `scan_rules.rs` 中 FilterWithoutIndex 的 suggestion 构造代码，找到当前的列名提取逻辑（很可能也是手写正则），替换为 `super::utils::extract_column_from_filter(filter_value)`。

提交信息：
```
fix(rules): SCAN-004 uses shared column extraction utility

Replaces ad-hoc regex with extract_column_from_filter. Fixes suggestion
output: was 'CREATE INDEX ON tbl (text)', now uses real column name.

Aligns with Phase 2 TYPE-001 fix; same root cause (cast annotation
confuses regex).
```

#### Task 3.2: SCAN-004 节点白名单扩展到 BitmapHeapScan

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs`（FilterWithoutIndex::check 中的 `node_type` match）
- Test: 同文件 test module

**Step 1: 写失败的测试**

```rust
#[test]
fn test_scan004_fires_on_bitmap_heap_scan_with_filter() {
    // 关键回归 case (38-case case 22)：
    // Bitmap Heap Scan on par_sys_securities t, Filter: (to_char(now()...) >= ...)
    // 当前不触发（节点类型不在白名单），期望触发
    let node = /* BitmapHeapScan with Filter + Rows Removed by Filter */;
    let finding = FilterWithoutIndex.check(&node, &Default::default());
    assert!(finding.is_some(), "SCAN-004 should fire on BitmapHeapScan with Filter");
}
```

**Step 2**: 运行确认失败。

**Step 3**: 修改 `node_type` match，加入 `NodeType::BitmapHeapScan | NodeType::PartitionedBitmapHeapScan`。

> **实现者注意**：需读现有代码确认是否同时检查"是否有 Filter"逻辑能正确处理 BitmapHeapScan 的 Filter 属性。BitmapHeapScan 的 Filter 行为与 SeqScan 类似（Rows Removed by Filter 适用）。

**Step 4-6**: 通过测试、检查其他 snapshot 无回归（BitmapHeapScan 出现的 fixture 较少，预期影响小）、提交。

提交信息：
```
feat(rules): SCAN-004 covers BitmapHeapScan

Adds BitmapHeapScan and PartitionedBitmapHeapScan to SCAN-004's
node type whitelist. Previously only SeqScan/PartitionedSeqScan/CStoreScan
were checked, missing the common pattern of BitmapHeapScan with
expensive Filter (38-case sample case 22: 0 findings -> 1+ findings).
```

---

### Phase 4: SUBQ-001 表名提取 + 聚合降噪

#### Task 4.1: SUBQ-001 表名递归查找

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs:31-36`
- Test: 同文件 test module

**Step 1: 写失败的测试**

```rust
#[test]
fn test_subq001_finds_table_name_in_nested_subquery() {
    // 构造: SubqueryScan → HashJoin → SeqScan(table="orders")
    // 当前 SUBQ-001 会输出"涉及表: unknown"（仅看直接子节点）
    // 期望: 输出"涉及表: orders"
    let subquery_scan = /* nested tree with SeqScan on "orders" inside */;
    let finding = SubqueryNotPulledUp.check(&subquery_scan, &Default::default());
    assert!(finding.unwrap().detail.contains("orders"),
            "detail must mention real table 'orders'");
}
```

**Step 3: 实现**

修改 `subquery_rules.rs:31-36`，从：

```rust
let child_table = node
    .children
    .first()
    .and_then(|c| c.relation.clone())
    .map(|r| first_identifier(&r))
    .unwrap_or_else(|| "unknown".to_string());
```

改为：

```rust
let child_table = super::utils::find_first_scan_descendant(node)
    .map(|r| first_identifier(&r))
    .unwrap_or_else(|| "unknown".to_string());
```

**Step 4-6**: 通过测试、snapshot review、提交。

提交信息：
```
fix(rules): SUBQ-001 recursive table name lookup

Uses find_first_scan_descendant to locate the underlying scan node's
table name, instead of only checking the direct child. Fixes 50%
'unknown' table name extraction on nested subqueries (133/267 findings
in 38-case sample).
```

#### Task 4.2: SUBQ-001 按子查询树聚合降噪

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs`（SubqueryNotPulledUp::check 完整重写为两段式：per-DFS 收集 + check_global 聚合）
- Modify: `crates/ogexplain-core/src/analyzer/rules/mod.rs`（如 trait 需要扩展，否则不动）
- Test: 同文件

**设计决策**：当前 `check` 是 per-node 的。聚合有两种方案：

- **方案 A（推荐）**：在 `check` 内部判断"我是否是子查询树的根" —— 通过查 `node_type == SubqueryScan` 且其所有子孙中没有其他 SubqueryScan 父链。但这需要祖先信息。
- **方案 B**：保留 per-node 触发，但 `SubqueryScan` 类型触发时跳过 SubPlan 部分（让 SubPlan 触发只在独立的 Result+SubPlan 模式下出现）。

> **实现者注意**：此 Task 是本期最复杂的，建议先读：
> - `crates/ogexplain-core/src/analyzer/config.rs` 中 `check_with_ancestors` 的签名（issue-17 引入的祖先链机制）
> - `crates/ogexplain-core/src/analyzer/rules/mod.rs` 中 trait 定义
> - 现有 SUBQ-001 触发数据（38-case 中 case 13-17 各 29 条）

**Step 1: 写失败的测试**

```rust
#[test]
fn test_subq001_aggregates_subplan_into_subquery_scan() {
    // 构造: SubqueryScan → BitmapHeapScan (有 SubPlan 属性) → BitmapIndexScan (有 SubPlan 属性)
    // 期望: 仅触发 1 条 SUBQ-001（在 SubqueryScan 层级）
    // 而非当前: 触发 3 条（每个有 SubPlan 的节点都触发）
    let plan = /* ExplainPlan with the above tree */;
    let report = crate::analyze(&plan);
    let subq_findings: Vec<_> = report.findings.iter()
        .filter(|f| f.rule_id == "SUBQ-001").collect();
    assert_eq!(subq_findings.len(), 1,
               "expected 1 aggregated finding, got {}", subq_findings.len());
}
```

**Step 2-3**: 失败验证 → 设计并实现聚合（推荐方案：用 `check_with_ancestors` 判断"祖先链中是否已有 SubqueryScan"，若是则当前 SubPlan 触发被抑制）。

**关键实现思路**：

```rust
fn check_with_ancestors(
    &self,
    node: &PlanNode,
    ctx: &PlanContext,
    ancestors: &[&PlanNode],
) -> Option<Finding> {
    // SubqueryScan 节点：始终触发（这是子查询树的根）
    if node.node_type == NodeType::SubqueryScan || node.node_type == NodeType::VectorSubqueryScan {
        return self.check(node, ctx);
    }
    // SubPlan 出现：只在祖先链中**没有**SubqueryScan 时触发
    // （即 SubPlan 不属于已被 SubqueryScan 节点聚合的子树）
    if any_property_contains(node, "SubPlan") {
        let has_subquery_scan_ancestor = ancestors.iter().any(|a| {
            a.node_type == NodeType::SubqueryScan || a.node_type == NodeType::VectorSubqueryScan
        });
        if has_subquery_scan_ancestor {
            return None; // 抑制：让父级 SubqueryScan 代表整棵子树
        }
        return Some(make_finding(
            self,
            format!("检测到未提升的子查询(SubPlan in {})", node.node_type),
            node,
            Some("/*+ EXPAND_SUBLINK */ 提升子链接; /*+ USE_MAGIC_SET */ 优化关联子查询".to_string()),
        ));
    }
    None
}
```

**Step 4-6**: 通过测试、跑 38-case 对比 SUBQ-001 数量（目标从 424 降到 ~100-150）、snapshot review、提交。

提交信息：
```
feat(rules): SUBQ-001 aggregates SubPlan findings into SubqueryScan tree

When a SubPlan appears inside a SubqueryScan's subtree, only the
SubqueryScan-level finding fires (previously every SubPlan-bearing node
fired independently, producing 29 findings per case for cases 13-17).

Uses check_with_ancestors to detect SubqueryScan in ancestor chain.
Standalone SubPlans (no SubqueryScan ancestor) still fire normally.

Expected impact on 38-case sample:
- SUBQ-001 total findings: 424 -> ~100-150
- Cases 13-17 per-case: 29 -> ~5-8
```

---

### Phase 5: 回归验证

#### Task 5.1: 38-case 回归对比

**Step 1**: 重新构建

```bash
cargo build --release -p ogexplain-cli
```

**Step 2**: 重新跑 38-case CSV 分析

```bash
target/release/ogexplain analyze tests/cases_38.csv \
    --input-format csv --output-columns full \
    --output /tmp/post_fix.csv
```

**Step 3**: 对比前后

```bash
python3 << 'PY'
import csv
from collections import Counter

pre = list(csv.DictReader(open('/tmp/cases38/results_38.csv')))
post = list(csv.DictReader(open('/tmp/post_fix.csv')))

def rule_counts(rows):
    c = Counter()
    for r in rows:
        for f in (r['findings'] or '').split('; '):
            rid = f.split(':')[0].strip('[]')
            if rid: c[rid] += 1
    return c

pre_c, post_c = rule_counts(pre), rule_counts(post)
all_rules = sorted(set(pre_c) | set(post_c))
print(f"{'Rule':12} {'Before':>8} {'After':>8} {'Δ':>8}")
for r in all_rules:
    p, q = pre_c.get(r,0), post_c.get(r,0)
    if p != q:
        print(f"{r:12} {p:>8} {q:>8} {q-p:>+8}")
print(f"\nTotal: {sum(pre_c.values())} -> {sum(post_c.values())}")
PY
```

**期望输出**（基于本计划目标）：

```
Rule           Before    After        Δ
TYPE-001          15        0      -15  ← 移除所有假阳性
SUBQ-001         424      ~120     -304  ← 聚合降噪
SCAN-004          32       ~35       +3  ← BitmapHeapScan 新触发
```

**Step 4**: 失败处理 —— 若 TYPE-001 仍 > 5 或 SUBQ-001 仍 > 200，回到对应 Phase 排查。

#### Task 5.2: benchmark 套件回归

**Step 1**: 找到 benchmark 执行脚本

```bash
ls lib/ogagila/benchmark/scripts/
```

**Step 2**: 按 `lib/ogagila/benchmark/README.md`（若存在）执行评估。

**Step 3**: 对比 `benchmark/04-evaluate/live_results-v3/per_rule_metrics.json`，生成新版本（如 `live_results-v4/`）。

**Step 4**: 验收标准

| 规则 | 旧 F1 | 新 F1 | 要求 |
|---|---|---|---|
| TYPE-001 | 0.0 | ≥ 0.5 | precision 必须提升（最少 1 个 TP） |
| SUBQ-001 | 0.6 | ≥ 0.7 | precision 提升，recall 不降 |
| SCAN-004 | 0.60 | ≥ 0.65 | precision 提升，recall 不降 |

#### Task 5.3: clippy + fmt + 提交收尾

```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

若全部通过，最终提交（若使用单独 commit）：

```
chore: post-fix snapshot updates and lint

Updates insta snapshots after Phase 1-4 diagnostic rule changes.
All 317+ tests pass, zero clippy warnings.
```

---

## 6. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| TYPE-001 触发条件改严后漏报真阳性 | 中 | 中 | Task 2.1 守护测试覆盖典型场景；benchmark 重新评估；若 recall 降太多，引入 StatsNotCollected 信号联合判定 |
| SUBQ-001 聚合漏掉真正的多层独立子查询 | 中 | 中 | Task 4.2 测试构造多层独立子查询场景；保留 `--no-aggregate-subq` 选项（如时间允许） |
| insta snapshot 大量变更影响 review | 高 | 低 | 每 Phase 后立即 `cargo insta review`，不让 snapshot 积压到结尾 |
| benchmark 套件执行脚本损坏或依赖缺失 | 中 | 中 | Task 5.2 先用 38-case 验证；benchmark 失败不阻塞合并（记为 follow-up） |
| `find_first_scan_descendant` 在循环 plan 上栈溢出 | 低 | 高 | 加 visited 集合或深度上限（PlanNode 通常无环，但保险起见） |

---

## 7. 时间预估

| Phase | Tasks | 预估时间 |
|---|---|---|
| Phase 1 | 1.1, 1.2 | 1.5h（含 fixture 构造调试） |
| Phase 2 | 2.1 | 2h（重写触发逻辑 + 测试） |
| Phase 3 | 3.1, 3.2 | 1h（共用 Phase 1 工具） |
| Phase 4 | 4.1, 4.2 | 3h（4.2 聚合逻辑最复杂） |
| Phase 5 | 5.1, 5.2, 5.3 | 1.5h |
| **总计** | 9 tasks | **~9 小时**（含测试、snapshot review、回归验证） |

---

## 8. 成功标准（最终验收）

✅ **必须满足**：
1. 38-case 上 TYPE-001 触发数 ≤ 5（移除假阳性）
2. 38-case 上 TYPE-001 / SCAN-004 suggestion 100% 包含真实列名（不含 `text` 字面量）
3. 38-case 上 SUBQ-001 单 case 最大触发数 ≤ 8
4. 38-case 上 SUBQ-001 表名 = "unknown" 占比 < 10%
5. 38-case case 22 有 ≥ 1 条 SCAN-004 finding（BitmapHeapScan 触发）
6. `cargo test --workspace` 全通过
7. `cargo clippy --workspace -- -D warnings` 零警告

🎯 **加分项**：
- benchmark 上 TYPE-001 precision > 0.5
- benchmark 上 SUBQ-001 precision > 0.7

---

## 9. 后续工作（不在本期）

按优先级排队，待本期完成后单独规划：

1. **新规则 JOIN-003**：Hash Join 的 Join Filter 大量过滤（case 22）
2. **新规则 SCAN-005**：Index Scan 选择性差（case 7 — 28.5s 零 finding）
3. **MEM-004 peak_memory 兜底解析**：从 spill 推算（cases 2/3/6 312MB spill 未触发）
4. **STATS-001 阈值调整**：放宽 plan_rows == 10 限制
5. **SQL 模板聚类 + CSV template_id**：DBA 工作流提效
6. **i18n 完善**：detail/suggestion 文案中文化补齐 `--lang en` 支持
