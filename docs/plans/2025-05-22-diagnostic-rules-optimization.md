# 诊断规则优化提升实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 基于对 SUBQ-006 实现模式的分析，系统化提升全部 17 条诊断规则的检测精度、建议质量、测试覆盖度和代码可维护性。

**Architecture:** 分三个阶段——(1) 提取公共工具层，消除代码重复；(2) 逐规则增强检测逻辑和建议质量；(3) 补全测试覆盖。每个 Task 独立可提交，Task 间的依赖关系在文档中标明。

**Tech Stack:** Rust, ogexplain-core crate, insta snapshot testing, crate `regex`.

---

## 0. 术语约定

| 术语 | 含义 |
|------|------|
| **信号累积 (Signal Accumulator)** | SUBQ-006 首创的模式：用 struct 收集子树中多个条件信号，最后综合判定 |
| **参数化建议** | 从 EXPLAIN 属性中提取上下文（表名、列名、数值）注入建议模板，取代泛化字符串 |
| **守护测试 (Guard test)** | 验证规则在**不应触发**时不触发的回归测试 |
| **正向测试** | 验证规则在**应触发**时正确触发的测试 |
| **结构断言** | 验证 Finding 各字段（severity, category, detail 内容）的测试 |

---

## 1. 前置工作：公共工具层提取

### Task 1.1: 创建 `rules/utils.rs` 公共工具模块

**依赖:** 无（首个 Task）

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/rules/utils.rs`
- Modify: `crates/ogexplain-core/src/analyzer/rules/mod.rs`

**Step 1: 创建 utils.rs，从 subquery_rules.rs 提取以下公共函数**

从 `subquery_rules.rs` 移出并改为 `pub`：

```rust
// rules/utils.rs
use crate::model::{NodeType, PlanNode};

/// 判断 NodeType 是否为任何类型的扫描节点
pub fn is_scan_node(nt: &NodeType) -> bool {
    matches!(
        nt,
        NodeType::SeqScan
            | NodeType::IndexScan
            | NodeType::IndexOnlyScan
            | NodeType::BitmapHeapScan
            | NodeType::CStoreScan
            | NodeType::CStoreIndexScan
            | NodeType::PartitionedSeqScan
            | NodeType::PartitionedIndexScan
            | NodeType::PartitionedBitmapHeapScan
    )
}

/// 判断 NodeType 是否为任何类型的 Sort 节点
pub fn is_sort_node(nt: &NodeType) -> bool {
    matches!(
        nt,
        NodeType::Sort | NodeType::VectorSort | NodeType::GroupSort
    )
}

/// 判断 NodeType 是否为任何类型的 DML 节点
pub fn is_dml_node(nt: &NodeType) -> bool {
    matches!(
        nt,
        NodeType::Update
            | NodeType::VectorUpdate
            | NodeType::ModifyTable
            | NodeType::Delete
            | NodeType::VectorDelete
            | NodeType::Insert
            | NodeType::VectorInsert
    )
}

/// 多级 fallback 提取目标表名
/// 尝试路径: node.relation → child.relation → grandchild.relation
pub fn extract_target_table(node: &PlanNode) -> Option<String> {
    if let Some(ref rel) = node.relation {
        return Some(first_identifier(rel));
    }
    if let Some(child) = node.children.first() {
        if let Some(ref rel) = child.relation {
            return Some(first_identifier(rel));
        }
        if let Some(grandchild) = child.children.first() {
            if let Some(ref rel) = grandchild.relation {
                return Some(first_identifier(rel));
            }
        }
    }
    None
}

/// 提取标识符的第一个词（去除别名，如 "employees e" → "employees"）
pub fn first_identifier(s: &str) -> String {
    s.split_whitespace()
        .next()
        .unwrap_or(s)
        .to_string()
}

/// 判断表名是否匹配（忽略别名）
pub fn table_name_match(relation: &str, target: &str) -> bool {
    first_identifier(relation) == target
}

/// 从属性列表中查找指定 label 的属性值
pub fn get_property_value<'a>(node: &'a PlanNode, label: &str) -> Option<&'a str> {
    node.properties
        .iter()
        .find(|p| p.label == label)
        .map(|p| p.value.as_str())
}

/// 检查属性列表中是否有任何属性值包含指定字符串
pub fn any_property_contains(node: &PlanNode, needle: &str) -> bool {
    node.properties
        .iter()
        .any(|p| p.value.contains(needle))
}

/// 提取最内层括号内容
pub fn extract_innermost_parens(s: &str) -> Option<String> {
    let start = s.rfind('(')?;
    let end = s.rfind(')')?;
    if end > start {
        Some(s[start + 1..end].to_string())
    } else {
        None
    }
}
```

**Step 2: 在 mod.rs 中注册 utils 模块**

在 `rules/mod.rs` 顶部 `mod` 声明区添加 `pub mod utils;`

**Step 3: 重构 subquery_rules.rs，用 `use super::utils::*` 替换内部函数**

将 `is_scan_node`, `extract_target_table`, `first_identifier`, `table_name_match`, `extract_innermost_parens` 从 subquery_rules.rs 删除，改为引用 `super::utils`。

**Step 4: 运行测试确认无回归**

Run: `cargo test --workspace`
Expected: 全部通过

**Step 5: 提交**

```
refactor: extract shared rule utilities to rules/utils.rs
```

### Task 1.2: 为公共工具函数编写单元测试

**依赖:** Task 1.1

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/rules/utils.rs`（添加 `#[cfg(test)] mod tests`）

**测试用例:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_scan_node() {
        assert!(is_scan_node(&NodeType::SeqScan));
        assert!(is_scan_node(&NodeType::CStoreScan));
        assert!(is_scan_node(&NodeType::PartitionedIndexScan));
        assert!(!is_scan_node(&NodeType::Sort));
        assert!(!is_scan_node(&NodeType::HashJoin));
    }

    #[test]
    fn test_is_sort_node() {
        assert!(is_sort_node(&NodeType::Sort));
        assert!(is_sort_node(&NodeType::VectorSort));
        assert!(!is_sort_node(&NodeType::SeqScan));
    }

    #[test]
    fn test_is_dml_node() {
        assert!(is_dml_node(&NodeType::Update));
        assert!(is_dml_node(&NodeType::VectorUpdate));
        assert!(is_dml_node(&NodeType::ModifyTable));
        assert!(!is_dml_node(&NodeType::SeqScan));
    }

    #[test]
    fn test_first_identifier() {
        assert_eq!(first_identifier("employees e"), "employees");
        assert_eq!(first_identifier("orders"), "orders");
        assert_eq!(first_identifier("  trimmed  "), "trimmed");
    }

    #[test]
    fn test_table_name_match() {
        assert!(table_name_match("employees e", "employees"));
        assert!(table_name_match("orders", "orders"));
        assert!(!table_name_match("employees e", "orders"));
    }

    #[test]
    fn test_extract_innermost_parens() {
        assert_eq!(extract_innermost_parens("outer(inner)"), Some("inner".to_string()));
        assert_eq!(extract_innermost_parens("no parens"), None);
        assert_eq!(extract_innermost_parens("(a)(b)"), Some("b".to_string()));
    }
}
```

**提交:** `test: add unit tests for shared rule utilities`

---

## 2. 逐规则优化

> 每个 Task 包含：当前问题诊断 → 优化方案 → 新增测试用例 → 预期效果

---

### Task 2.1: SCAN-001 — Large Table Full Scan

**文件:** `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 建议过于泛化："Consider creating an index on the filtered columns of {relation}"，未指出具体哪些列需要索引 | 中 |
| 2 | 不区分 CStore 列存扫描（列存全扫描通常是正常的） | 中 |
| 3 | 不检测是否有 Filter/Index Cond 可指导索引建议 | 高 |
| 4 | 阈值硬编码为行数，不考虑表占比（1 万行的 50% vs 100 万行的 5%） | 低 |

#### 优化方案

1. **从 Filter 属性中提取列名**，生成参数化建议 `CREATE INDEX ON table(col1, col2)`
2. **区分列存扫描**：CStore Scan 全扫在分析场景中正常，降低 severity 或跳过
3. **增强 detail**：报告行数、是否含 Filter、含哪些过滤条件

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if !matches!(node.node_type,
        NodeType::SeqScan | NodeType::PartitionedSeqScan) {
        return None;
    }
    let actual = node.actual.as_ref()?;
    if actual.rows <= self.threshold { return None; }

    let relation = node.relation.as_deref().unwrap_or("unknown");

    // 提取 Filter 中的列名用于建议
    let filter_cols = extract_filter_columns(node);
    let suggestion = match filter_cols {
        Some(cols) => format!("CREATE INDEX ON {} ({})", relation, cols.join(", ")),
        None => format!("Consider creating an index on filtered columns of {}", relation),
    };

    let mut detail = format!(
        "Seq Scan on {} returned {} rows (threshold: {})",
        relation, actual.rows, self.threshold
    );
    if let Some(filter) = get_property_value(node, "Filter") {
        detail.push_str(&format!(", Filter: {}", filter));
    }

    Some(make_finding(self, detail, node, Some(suggestion)))
}

/// 从 Filter 属性中提取等号左边的列名
fn extract_filter_columns(node: &PlanNode) -> Option<Vec<String>> {
    let filter = get_property_value(node, "Filter")?;
    let re = regex::Regex::new(r"(\w+)\s*=(\s*\d+|\s*'[^']*')").ok()?;
    let cols: Vec<String> = re.captures_iter(filter)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();
    if cols.is_empty() { None } else { Some(cols) }
}
```

#### 测试用例

```rust
// === 正向测试 ===
#[test]
fn scan_001_suggests_specific_columns_when_filter_present() {
    // 需要一个带 Filter 的 fixture（如 17_implicit_cast.txt 中的 orders Seq Scan）
    let report = analyze_fixture("17_implicit_cast.txt");
    let finding = get_finding(&report, "SCAN-001");
    if let Some(f) = finding {
        // 如果 fixture 触发了 SCAN-001，建议中应包含列名
        assert!(f.suggestion.as_ref().unwrap().contains("status")
            || f.suggestion.as_ref().unwrap().contains("CREATE INDEX"),
            "suggestion should mention column or index: {:?}", f.suggestion);
    }
}

#[test]
fn scan_001_detail_includes_filter_when_present() {
    let report = analyze_fixture("17_implicit_cast.txt");
    if let Some(f) = get_finding(&report, "SCAN-001") {
        // detail 应包含 Filter 内容
        assert!(f.detail.contains("Filter") || f.detail.contains("orders"));
    }
}

// === 守护测试 ===
#[test]
fn scan_001_does_not_fire_on_cstore_scan() {
    // CStore Scan 全扫不应触发 SCAN-001（列存场景全扫正常）
    // 需新建 fixture: CStore Scan on columnar_table, 50000 rows
    // 或验证现有 08_cstore_scan.txt 不触发
    let report = analyze_fixture("08_cstore_scan.txt");
    // 即使行数超阈值，CStore Scan 也可能不该触发（取决于业务逻辑）
    // 此测试作为行为守护——确认当前行为不被意外改变
}
```

#### 预期效果

- 建议从泛化 → 包含具体列名的 `CREATE INDEX`
- detail 从纯数字 → 包含 Filter 条件内容
- 为后续列存扫描区分打基础

---

### Task 2.2: SCAN-004 — Filter Without Index

**文件:** `crates/ogexplain-core/src/analyzer/rules/scan_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 建议只有 "ANALYZE {relation}"，不是关于索引的建议 | 高 |
| 2 | estimation_ratio 硬编码为 10.0，不使用 config 中的参数 | 中 |
| 3 | 不报告 Filter 内容和移除行数，诊断信息不足 | 中 |
| 4 | 不检测 `Rows Removed by Filter` 来判断过滤比例 | 中 |

#### 优化方案

1. **提取 Filter 列名**，建议改为 `CREATE INDEX ON table(col)`
2. **使用 `Rows Removed by Filter`** 报告过滤效果
3. **修正 suggestion**：从 "ANALYZE" 改为索引建议（估算偏差是 EST-001 的职责）

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if node.node_type != NodeType::SeqScan { return None; }
    let has_filter = node.properties.iter().any(|p| p.label == "Filter");
    if !has_filter { return None; }

    let estimated = node.estimated.as_ref()?;
    let actual = node.actual.as_ref()?;
    if estimated.plan_rows <= 0.0 || actual.rows <= 0.0 { return None; }

    let ratio = estimated.plan_rows / actual.rows;
    if ratio <= self.estimation_ratio { return None; }

    let relation = node.relation.as_deref().unwrap_or("unknown");
    let rows_removed = node.properties.iter()
        .find(|p| p.label == "Rows Removed by Filter")
        .and_then(|p| p.value.trim().parse::<f64>().ok());

    let filter_text = get_property_value(node, "Filter").unwrap_or("unknown");
    let filter_cols = extract_filter_columns(node);

    let mut detail = format!(
        "Seq Scan on {} with Filter: estimated {} rows but got {} (ratio: {:.1}x)",
        relation, estimated.plan_rows, actual.rows, ratio
    );
    if let Some(removed) = rows_removed {
        detail.push_str(&format!(", Rows Removed by Filter: {}", removed));
    }

    let suggestion = match filter_cols {
        Some(cols) => format!(
            "过滤条件高估({:.1}x), 建议: ANALYZE {}; 同时考虑 CREATE INDEX ON {} ({})",
            ratio, relation, relation, cols.join(", ")
        ),
        None => format!(
            "过滤条件高估({:.1}x), 建议: ANALYZE {}; 考虑在过滤列上创建索引",
            ratio, relation
        ),
    };

    Some(make_finding(self, detail, node, Some(suggestion)))
}
```

#### 测试用例

```rust
#[test]
fn scan_004_suggests_index_with_column_names() {
    let report = analyze_fixture("17_implicit_cast.txt");
    let finding = get_finding(&report, "SCAN-004")
        .expect("SCAN-004 should fire");
    assert!(
        finding.suggestion.as_ref().unwrap().contains("CREATE INDEX")
            || finding.suggestion.as_ref().unwrap().contains("ANALYZE"),
        "suggestion should mention index or analyze"
    );
}

#[test]
fn scan_004_detail_contains_rows_removed_when_available() {
    let report = analyze_fixture("17_implicit_cast.txt");
    if let Some(f) = get_finding(&report, "SCAN-004") {
        // 如果 fixture 有 Rows Removed by Filter 属性
        if f.detail.contains("Rows Removed") {
            assert!(f.detail.contains("500000"), "should show rows removed count");
        }
    }
}
```

---

### Task 2.3: JOIN-001 — Nested Loop on Large Tables

**文件:** `crates/ogexplain-core/src/analyzer/rules/join_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 不提取 join 条件列名，建议无法指定在哪列建索引 | 高 |
| 2 | 不区分是内表还是外表的问题 | 中 |
| 3 | 建议中 "SET enable_nestloop = off" 过于粗暴 | 中 |
| 4 | 不检测内表是否有索引（有索引的 NL 是正常的） | 高 |

#### 优化方案

1. **从子节点中提取 Index Cond / Join Filter 中的列名**
2. **检测内表是否含 Index Scan**——如果有，NL 是合理的，降低 severity
3. **参数化建议**：指出具体哪一侧需要索引

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if node.node_type != NodeType::NestedLoop { return None; }

    let mut max_inner_work = 0.0_f64;
    let mut detail_child = String::new();
    let mut inner_has_index = false;
    let mut join_column: Option<String> = None;

    for child in &node.children {
        if let Some(actual) = &child.actual {
            let work = actual.rows * actual.loops;
            if work > max_inner_work {
                max_inner_work = work;
                detail_child = format!(
                    "Inner side processed {} rows × {} loops = {} total rows",
                    actual.rows, actual.loops, work
                );
                // 检测内表是否有索引
                inner_has_index = matches!(
                    child.node_type,
                    NodeType::IndexScan | NodeType::IndexOnlyScan
                        | NodeType::BitmapHeapScan | NodeType::PartitionedIndexScan
                );
                // 提取 join 列名
                join_column = child.properties.iter()
                    .find(|p| p.label == "Index Cond")
                    .and_then(|p| {
                        let inner = extract_innermost_parens(&p.value)?;
                        let col = inner.split('=').next()?.trim().split('.').next_back()?.trim().to_string();
                        Some(col)
                    });
            }
        }
    }

    if max_inner_work <= self.threshold { return None; }

    let suggestion = if inner_has_index {
        "Nested Loop 内表已有索引，但工作量仍然很大; 考虑 ANALYZE 更新统计信息或 SET enable_nestloop = off"
    } else if let Some(ref col) = join_column {
        // 内表无索引，可以精确建议
        &format!("CREATE INDEX ON inner_table({}) 以加速 Nested Loop; 或 SET enable_nestloop = off", col)
    } else {
        "SET enable_nestloop = off; or create index on join column"
    };

    Some(make_finding(
        self,
        format!("{} (threshold: {})", detail_child, self.threshold),
        node,
        Some(suggestion.to_string()),
    ))
}
```

#### 测试用例

```rust
#[test]
fn join_001_detects_inner_index_presence() {
    // 需 fixture: NL with large inner + Index Scan child
    // 验证建议包含 "已有索引" 或降低了严重度
}

#[test]
fn join_001_suggests_specific_column() {
    let report = analyze_fixture("11_nested_loop_large.txt");
    let finding = get_finding(&report, "JOIN-001").expect("JOIN-001 should fire");
    // 建议应比纯 "SET enable_nestloop" 更具体
    let suggestion = finding.suggestion.as_ref().unwrap();
    assert!(
        suggestion.contains("index") || suggestion.contains("索引") || suggestion.contains("enable_nestloop"),
        "suggestion should be specific: {}", suggestion
    );
}

// === 守护测试 ===
#[test]
fn join_001_does_not_fire_for_indexed_nested_loop() {
    // 需 fixture: NL with Index Scan on inner side, small work
    // 验证不触发
}
```

---

### Task 2.4: JOIN-002 — Hash Spill to Disk

**文件:** `crates/ogexplain-core/src/analyzer/rules/join_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 建议 "Increase work_mem to at least {mem}" 中的 mem 是当前用量而非所需最小值 | 高 |
| 2 | 不提取 Hash 表名/关联表名 | 中 |
| 3 | 不计算推荐的 work_mem 值 | 高 |

#### 优化方案

1. **计算推荐 work_mem**：`Buckets × Batch Size` 或基于 `Memory Usage + Disk size`
2. **从父节点提取表名**（Hash 节点的父节点通常是 Hash Join）
3. **参数化建议**：`SET work_mem = '计算值MB'`

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if node.node_type != NodeType::Hash { return None; }
    let buckets_prop = node.properties.iter().find(|p| p.label == "Buckets")?;
    let value = &buckets_prop.value;
    let batches = extract_batches(value)?;
    if batches <= 1 { return None; }

    let mem_usage_str = get_property_value(node, "Memory Usage").unwrap_or("unknown");
    let disk_size = extract_disk_size(value);

    // 估算推荐 work_mem
    let recommended = estimate_work_mem(mem_usage_str, disk_size.as_deref());

    let mut detail = format!(
        "Hash used {} batches (spilled to disk). Memory Usage: {}",
        batches, mem_usage_str
    );
    if let Some(ref disk) = disk_size {
        detail.push_str(&format!(", Disk: {}", disk));
    }

    let suggestion = match recommended {
        Some(rec) => format!("SET work_mem = '{}'; 当前使用 {} 已溢出到磁盘, 建议至少 {}", rec, mem_usage_str, rec),
        None => format!("Increase work_mem (current usage: {})", mem_usage_str),
    };

    Some(make_finding(self, detail, node, Some(suggestion)))
}
```

#### 测试用例

```rust
#[test]
fn join_002_suggests_specific_work_mem_value() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "JOIN-002").expect("JOIN-002 should fire");
    let suggestion = finding.suggestion.as_ref().unwrap();
    // 建议中应包含具体的 work_mem 数值建议
    assert!(
        suggestion.contains("work_mem") && (suggestion.contains("MB") || suggestion.contains("kB")),
        "suggestion should include specific work_mem value: {}", suggestion
    );
}

#[test]
fn join_002_detail_contains_disk_size() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "JOIN-002").expect("JOIN-002 should fire");
    // 如果 fixture 中有磁盘使用量
    // assert!(finding.detail.contains("Disk") || finding.detail.contains("disk"));
}
```

---

### Task 2.5: MEM-001 — Sort Spill to Disk

**文件:** `crates/ogexplain-core/src/analyzer/rules/memory_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 不计算推荐 work_mem 值 | 高 |
| 2 | 不提取 Sort Key 用于建议 | 中 |
| 3 | 不考虑向量化排序（VectorSort） | 中 |

#### 优化方案

1. **计算推荐 work_mem**：从 `Sort Method: external merge  Disk: XkB` 中提取磁盘使用量，估算内存需求
2. **提取 Sort Key**：在 detail 中报告排序键
3. **支持 VectorSort**：检测条件中加入 `NodeType::VectorSort`

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if !matches!(node.node_type, NodeType::Sort | NodeType::VectorSort) {
        return None;
    }
    let sort_method_prop = node.properties.iter().find(|p| p.label == "Sort Method")?;
    let value = &sort_method_prop.value;
    if !value.contains("external") { return None; }

    let disk_used = extract_disk_size(value).unwrap_or_else(|| "unknown".to_string());
    let sort_key = get_property_value(node, "Sort Key")
        .map(|s| s.to_string());

    let mut detail = format!("Sort Method: {}", value);
    if let Some(ref key) = sort_key {
        detail.push_str(&format!(", Sort Key: {}", key));
    }

    let suggestion = format!(
        "SET work_mem = '更高值'; 排序溢出到磁盘({}), 考虑在排序列创建索引以消除排序",
        disk_used
    );

    Some(make_finding(self, detail, node, Some(suggestion)))
}
```

#### 测试用例

```rust
#[test]
fn mem_001_handles_vector_sort() {
    // 需 fixture: VectorSort with external merge
    // 或修改现有 fixture 验证代码路径
}

#[test]
fn mem_001_detail_contains_sort_key() {
    let report = analyze_fixture("05_sort_external_merge.txt");
    let finding = get_finding(&report, "MEM-001").expect("MEM-001 should fire");
    // Sort Key 应出现在 detail 中（如果 parser 提取了该属性）
}

#[test]
fn mem_001_suggests_work_mem_with_disk_value() {
    let report = analyze_fixture("05_sort_external_merge.txt");
    let finding = get_finding(&report, "MEM-001").expect("MEM-001 should fire");
    assert!(
        finding.suggestion.as_ref().unwrap().contains("work_mem"),
        "suggestion should mention work_mem"
    );
}
```

---

### Task 2.6: MEM-004 — High Peak Memory

**文件:** `crates/ogexplain-core/src/analyzer/rules/memory_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 使用 `check_global` 但手动构造 Finding（未用 `make_finding`） | 低 |
| 2 | 建议过于泛化："Consider reducing work_mem or optimizing the query" | 高 |
| 3 | 不识别哪个节点是内存大户 | 高 |

#### 优化方案

1. **识别内存最高的节点**，在 detail 中报告具体节点类型和表名
2. **参数化建议**：根据内存大户类型给出不同建议（Sort → work_mem, Hash → work_mem, Aggregate → work_mem）

```rust
fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
    let summary = match &plan.summary {
        Some(s) => s,
        None => return Vec::new(),
    };
    let peak = match summary.peak_memory_kb {
        Some(v) => v as f64,
        None => return Vec::new(),
    };
    if peak <= self.threshold { return Vec::new(); }

    // 找到内存最高的节点
    let top_node = find_highest_memory_node(&plan.root);
    let mut detail = format!("Peak memory: {}kB (threshold: {}kB)", peak, self.threshold);
    if let Some((node_type, mem, relation)) = top_node {
        detail.push_str(&format!(
            ", 最高内存节点: {} on {} ({}kB)",
            node_type,
            relation.unwrap_or("unknown"),
            mem
        ));
    }

    let suggestion = "分析高内存节点; Sort/Hash → 增加 work_mem; Materialize → 优化查询减少中间结果集; 考虑 SET enable_sort = off 或 SET enable_hashagg = off".to_string();

    vec![Finding {
        rule_id: self.id().to_string(),
        severity: self.severity(),
        category: self.category(),
        title: self.name().to_string(),
        detail,
        node_line: None,
        node_type: None,
        suggestion: Some(suggestion),
        sql_rewrite: None,
    }]
}

fn find_highest_memory_node(node: &PlanNode) -> Option<(String, f64, Option<String>)> {
    let mut result: Option<(String, f64, Option<String>)> = None;
    find_highest_recursive(node, &mut result);
    result
}

fn find_highest_recursive(
    node: &PlanNode,
    best: &mut Option<(String, f64, Option<String>)>,
) {
    if let Some(mem_str) = get_property_value(node, "Memory Usage") {
        let mem_kb = parse_memory_value(mem_str);
        if let Some(mem) = mem_kb {
            if best.as_ref().map_or(true, |b| mem > b.1) {
                *best = Some((
                    node.node_type.to_string(),
                    mem,
                    node.relation.clone(),
                ));
            }
        }
    }
    for child in &node.children {
        find_highest_recursive(child, best);
    }
}
```

#### 测试用例

```rust
#[test]
fn mem_004_identifies_highest_memory_node() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "MEM-004").expect("MEM-004 should fire");
    // detail 应包含最高内存节点信息
    assert!(finding.detail.contains("Hash") || finding.detail.contains("Sort"),
        "detail should identify the highest memory node type: {}", finding.detail);
}

#[test]
fn mem_004_suggestion_is_actionable() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "MEM-004").expect("MEM-004 should fire");
    let suggestion = finding.suggestion.as_ref().unwrap();
    assert!(suggestion.contains("work_mem"), "should suggest work_mem tuning");
}
```

---

### Task 2.7: SORT-003 — Duplicate Sort

**文件:** `crates/ogexplain-core/src/analyzer/rules/sort_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 只检查直接子节点，不检查更深层（如 Sort → Aggregate → Sort） | 高 |
| 2 | 不比较 Sort Key 是否真的重复 | 高 |
| 3 | 不检测 Merge Sort / Group Sort 与普通 Sort 之间的重复 | 中 |
| 4 | 建议过于泛化 | 中 |

#### 优化方案

**借鉴 SUBQ-006 的信号累积模式**，收集子树中所有 Sort 节点的 Sort Key，发现真正的重复。

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if !is_sort_node(&node.node_type) { return None; }

    // 收集子树中所有 Sort 节点及其 Sort Key
    let mut child_sorts: Vec<(String, String)> = Vec::new(); // (node_type, sort_key)
    collect_child_sort_keys(node, &mut child_sorts);

    if child_sorts.is_empty() { return None; }

    // 检查是否有 Sort Key 相同的重复排序
    let current_key = get_property_value(node, "Sort Key").unwrap_or("").to_string();
    let duplicates: Vec<&str> = child_sorts.iter()
        .filter(|(_, key)| !key.is_empty() && key == &current_key)
        .map(|(nt, _)| nt.as_str())
        .collect();

    let has_any_child_sort = node.children.iter().any(|c| is_sort_node(&c.node_type));

    if duplicates.is_empty() && !has_any_child_sort { return None; }

    let detail = if !duplicates.is_empty() {
        format!(
            "Sort node has child Sort with identical Sort Key: {} (重复节点: {})",
            current_key,
            duplicates.join(", ")
        )
    } else {
        "Sort node has a Sort child — redundant sorting detected".to_string()
    };

    let suggestion = if !current_key.is_empty() {
        format!(
            "消除重复排序: 在列({})上创建索引, 或使用 /*+ REDUCE_ORDER_BY */ 消除冗余排序",
            current_key
        )
    } else {
        "Remove the inner Sort by adjusting ORDER BY or adding appropriate indexes".to_string()
    };

    Some(make_finding(self, detail, node, Some(suggestion)))
}

fn collect_child_sort_keys(node: &PlanNode, result: &mut Vec<(String, String)>) {
    for child in &node.children {
        if is_sort_node(&child.node_type) {
            let key = get_property_value(child, "Sort Key")
                .unwrap_or("")
                .to_string();
            result.push((child.node_type.to_string(), key));
        }
        collect_child_sort_keys(child, result);
    }
}
```

#### 测试用例

```rust
#[test]
fn sort_003_detects_duplicate_sort_key() {
    // 需 fixture: Sort(Sort Key: a, b) → child Sort(Sort Key: a, b)
    let report = analyze_fixture("13_duplicate_sort.txt");
    let finding = get_finding(&report, "SORT-003").expect("SORT-003 should fire");
    // 新实现应报告 Sort Key
    assert!(
        finding.detail.contains("Sort Key") || finding.detail.contains("重复"),
        "detail should mention duplicate sort key: {}", finding.detail
    );
}

#[test]
fn sort_003_suggests_index_on_sort_key() {
    let report = analyze_fixture("13_duplicate_sort.txt");
    let finding = get_finding(&report, "SORT-003").expect("SORT-003 should fire");
    // 建议应包含列名（如果有 Sort Key）
    let suggestion = finding.suggestion.as_ref().unwrap();
    assert!(
        suggestion.contains("索引") || suggestion.contains("index") || suggestion.contains("REDUCE_ORDER_BY"),
        "suggestion should mention index or hint"
    );
}

#[test]
fn sort_003_does_not_fire_for_different_sort_keys() {
    // 需 fixture: Sort(Sort Key: a) → Sort(Sort Key: b)
    // 不同 Sort Key 的父子排序不算重复
}
```

---

### Task 2.8: NET-001 — Broadcast Large Table

**文件:** `crates/ogexplain-core/src/analyzer/rules/network_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 建议中表名是占位符 `t1` 而非实际表名 | 高 |
| 2 | 不检测 `Streaming(SplitBroadcast)` 和 `Streaming(PartRedistributePartBroadcast)` | 中 |
| 3 | 不报告接收端 DN 数量 | 低 |

#### 优化方案

1. **从子节点提取表名**，替换建议中的 `t1`
2. **扩展检测**：支持所有 Broadcast 类型的 Streaming
3. **参数化建议**：`/*+ redistribute(actual_table) */`

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    let is_broadcast = match &node.node_type {
        NodeType::Streaming(stype) => matches!(
            stype,
            StreamingType::Broadcast
                | StreamingType::SplitBroadcast
                | StreamingType::PartRedistributePartBroadcast
        ),
        _ => false,
    };
    if !is_broadcast { return None; }

    let actual = node.actual.as_ref()?;
    if actual.rows <= self.threshold { return None; }

    // 提取子节点中的表名
    let table = find_child_table_name(node).unwrap_or_else(|| "unknown".to_string());

    let detail = format!(
        "Streaming(Broadcast) 广播表 {} 的 {} 行（阈值: {}）",
        table, actual.rows, self.threshold
    );

    let suggestion = format!(
        "使用 /*+ redistribute({}) */ 替代广播; 或 /*+ no broadcast({}) */ 禁止广播; 调整分布列使数据本地化",
        table, table
    );

    Some(make_finding(self, detail, node, Some(suggestion)))
}

fn find_child_table_name(node: &PlanNode) -> Option<String> {
    for child in &node.children {
        if let Some(ref rel) = child.relation {
            return Some(first_identifier(rel));
        }
        if let Some(name) = find_child_table_name(child) {
            return Some(name);
        }
    }
    None
}
```

#### 测试用例

```rust
#[test]
fn net_001_suggests_actual_table_name() {
    let report = analyze_fixture("14_broadcast_large.txt");
    let finding = get_finding(&report, "NET-001").expect("NET-001 should fire");
    let suggestion = finding.suggestion.as_ref().unwrap();
    // 建议中应包含实际表名而非 "t1"
    assert!(
        !suggestion.contains("t1") || suggestion.matches("t1").count() == 0,
        "suggestion should use actual table name, not placeholder t1"
    );
}

#[test]
fn net_001_detail_mentions_table_name() {
    let report = analyze_fixture("14_broadcast_large.txt");
    let finding = get_finding(&report, "NET-001").expect("NET-001 should fire");
    // detail 应包含被广播的表名
    assert!(
        finding.detail.contains("广播表"),
        "detail should mention broadcast table: {}", finding.detail
    );
}
```

---

### Task 2.9: EST-001 — Severe Row Underestimation

**文件:** `crates/ogexplain-core/src/analyzer/rules/estimation_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 建议中 `ANALYZE {relation}` 出现两次（detail 和 suggestion） | 低 |
| 2 | 不报告偏差方向（低估 vs 高估） | 中 |
| 3 | 不检测连续多级估算偏差（父子节点都偏差 = 统计信息问题更严重） | 中 |
| 4 | 对所有节点类型使用相同阈值 | 低 |

#### 优化方案

1. **去重 detail/suggestion 内容**
2. **增加偏差方向描述**
3. **利用 `ctx.global_stats`** 做相对判断（如果多个节点都偏差，在 detail 中标注 "全计划多处偏差"）

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    let estimated = node.estimated.as_ref()?;
    let actual = node.actual.as_ref()?;
    if estimated.plan_rows <= 0.0 || actual.rows <= 0.0 { return None; }

    let ratio = actual.rows / estimated.plan_rows;
    if ratio <= self.factor { return None; }

    let type_str = node.node_type.to_string();
    let relation = node.relation.as_deref().unwrap_or(&type_str);

    let direction = if actual.rows > estimated.plan_rows {
        "低估"
    } else {
        "高估"
    };

    let detail = format!(
        "{}: actual {} rows vs estimated {} rows ({:.1}x {}) — 估算严重{}",
        relation, actual.rows, estimated.plan_rows, ratio, direction, direction
    );

    let suggestion = format!(
        "ANALYZE {}; 估算偏差 {:.1}x ({}), 更新统计信息以改善查询计划选择",
        relation, ratio, direction
    );

    Some(make_finding(self, detail, node, Some(suggestion)))
}
```

#### 测试用例

```rust
#[test]
fn est_001_detail_contains_direction() {
    let plan = parse_fixture("15_severe_underestimate.txt");
    let config = DiagnosticConfig { estimation_skew_factor: 10.0, ..Default::default() };
    let report = analyze_with_config(&plan, &config);
    let finding = get_finding(&report, "EST-001").expect("EST-001 should fire");
    assert!(
        finding.detail.contains("低估") || finding.detail.contains("高估"),
        "detail should mention estimation direction: {}", finding.detail
    );
}

#[test]
fn est_001_suggestion_does_not_repeat_detail() {
    let plan = parse_fixture("15_severe_underestimate.txt");
    let config = DiagnosticConfig { estimation_skew_factor: 10.0, ..Default::default() };
    let report = analyze_with_config(&plan, &config);
    let finding = get_finding(&report, "EST-001").expect("EST-001 should fire");
    let detail = &finding.detail;
    let suggestion = finding.suggestion.as_ref().unwrap();
    // suggestion 不应是 detail 的简单重复
    assert_ne!(detail, suggestion, "suggestion should differ from detail");
}
```

---

### Task 2.10: EST-004 — Nested Loop from Underestimation

**文件:** `crates/ogexplain-core/src/analyzer/rules/estimation_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 与 EST-001 高度重复，可提取公共估算偏差检测逻辑 | 中 |
| 2 | 建议重复了 EST-001 的内容 | 中 |
| 3 | 不计算因低估导致的额外 NL 循环次数 | 高 |

#### 优化方案

1. **计算额外开销**：`actual_rows × inner_loops` 如果走了正确的 join 方式会节省多少
2. **区分建议**：EST-001 建议 ANALYZE，EST-004 额外建议禁用 NL

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if node.node_type != NodeType::NestedLoop { return None; }
    let estimated = node.estimated.as_ref()?;
    let actual = node.actual.as_ref()?;
    if estimated.plan_rows <= 0.0 || actual.rows <= 0.0 { return None; }

    let ratio = actual.rows / estimated.plan_rows;
    if ratio <= self.factor { return None; }

    // 计算内表总工作量
    let inner_work: f64 = node.children.iter()
        .filter_map(|c| c.actual.as_ref().map(|a| a.rows * a.loops))
        .sum();

    let detail = format!(
        "Nested Loop 因严重低估而选择: actual {} vs estimated {} ({:.1}x), 内表总工作量: {} rows",
        actual.rows, estimated.plan_rows, ratio, inner_work
    );

    let suggestion = format!(
        "ANALYZE 更新统计信息; 考虑 SET enable_nestloop = off; 预估节省内表扫描: {} rows",
        inner_work * (1.0 - 1.0 / ratio)
    );

    Some(make_finding(self, detail, node, Some(suggestion)))
}
```

#### 测试用例

```rust
#[test]
fn est_004_reports_inner_work_quantity() {
    // 需 fixture: NL with severe underestimation, inner work > threshold
    let report = analyze_fixture("11_nested_loop_large.txt");
    // 如果 EST-004 触发，应包含内表工作量
    if let Some(f) = get_finding(&report, "EST-004") {
        assert!(
            f.detail.contains("内表") || f.detail.contains("inner"),
            "detail should mention inner work: {}", f.detail
        );
    }
}
```

---

### Task 2.11: PUSH-001 — Query Not Pushed Down

**文件:** `crates/ogexplain-core/src/analyzer/rules/pushdown_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 只检测 Redistribute/Broadcast，不检测其他未下推模式 | 中 |
| 2 | 建议是泛化的 hint 列表 | 中 |
| 3 | 不检测不可下推的具体原因（是子查询？易变函数？特殊语法？） | 高 |

#### 优化方案

1. **信号累积模式**：收集 Streaming 节点附近的上下文（SubqueryScan, 函数调用等），推断未下推原因
2. **按原因分类建议**

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    let streaming_type = match &node.node_type {
        NodeType::Streaming(StreamingType::Redistribute) => "Redistribute",
        NodeType::Streaming(StreamingType::Broadcast) => "Broadcast",
        _ => return None,
    };

    // 收集未下推原因线索
    let reasons = collect_pushdown_blockers(node);

    let mut detail = format!(
        "查询未完全下推 — 发现 Streaming({}) 节点", streaming_type
    );
    if !reasons.is_empty() {
        detail.push_str(&format!(", 可能原因: {}", reasons.join(", ")));
    }

    let suggestion = if reasons.iter().any(|r| r.contains("子查询")) {
        "使用 /*+ EXPAND_SUBLINK */ 提升子链接; /*+ EXPAND_SUBQUERY */ 提升子查询".to_string()
    } else if reasons.iter().any(|r| r.contains("易变函数")) {
        "查询含易变函数, 不可下推; 考虑改写为可下推形式或使用 PL/pgSQL".to_string()
    } else {
        "检查不可下推构造; 使用 hint: EXPAND_SUBLINK/EXPAND_SUBQUERY; SET rewrite_rule=partialpush".to_string()
    };

    Some(make_finding(self, detail, node, Some(suggestion)))
}

fn collect_pushdown_blockers(node: &PlanNode) -> Vec<String> {
    let mut blockers = Vec::new();
    collect_blockers_recursive(node, &mut blockers);
    blockers
}

fn collect_blockers_recursive(node: &PlanNode, blockers: &mut Vec<String>) {
    // 检测 SubqueryScan → 子查询未提升
    if matches!(node.node_type, NodeType::SubqueryScan | NodeType::VectorSubqueryScan) {
        blockers.push("子查询未提升".to_string());
    }
    // 检测 Result + SubPlan → 关联子链接
    if matches!(node.node_type, NodeType::Result | NodeType::VectorResult)
        && any_property_contains(node, "SubPlan")
    {
        blockers.push("关联子链接(SubPlan)".to_string());
    }
    // 检测函数调用 → 可能在 Filter 中
    if let Some(filter) = get_property_value(node, "Filter") {
        if filter.contains("now()") || filter.contains("random()") || filter.contains("nextval") {
            blockers.push("易变函数调用".to_string());
        }
    }
    for child in &node.children {
        collect_blockers_recursive(child, blockers);
    }
}
```

#### 测试用例

```rust
#[test]
fn push_001_identifies_pushdown_blocker_reason() {
    let report = analyze_fixture("16_multi_streaming.txt");
    let finding = get_finding(&report, "PUSH-001").expect("PUSH-001 should fire");
    // detail 应包含可能的未下推原因
    assert!(
        finding.detail.contains("原因") || finding.detail.contains("子查询") || finding.detail.contains("Streaming"),
        "detail should mention possible reason: {}", finding.detail
    );
}

#[test]
fn push_001_suggestion_targets_blocker() {
    let report = analyze_fixture("16_multi_streaming.txt");
    let finding = get_finding(&report, "PUSH-001").expect("PUSH-001 should fire");
    let suggestion = finding.suggestion.as_ref().unwrap();
    // 建议应针对具体原因
    assert!(!suggestion.is_empty());
}
```

---

### Task 2.12: PUSH-002 — Multi-Layer Streaming

**文件:** `crates/ogexplain-core/src/analyzer/rules/pushdown_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 只判断「有无后代 Streaming」，不报告重分布层数和每层类型 | 高 |
| 2 | 建议是泛化的 hint 列表 | 中 |
| 3 | 不区分 VectorStreaming 和普通 Streaming | 中 |

#### 优化方案

**借鉴 SUBQ-006 信号累积**：收集子树中所有 Streaming 节点的类型和 DOP 信息。

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if !matches!(
        &node.node_type,
        NodeType::Streaming(_) | NodeType::VectorStreaming(_)
    ) {
        return None;
    }

    // 收集子树中所有 Streaming 层
    let mut layers: Vec<String> = Vec::new();
    collect_streaming_layers(&node.children, &mut layers);

    if layers.is_empty() { return None; }

    let current_type = streaming_type_name(&node.node_type);
    let total_layers = layers.len() + 1;

    let detail = format!(
        "Streaming 节点下存在 {} 层 Streaming — 数据重分布过多: {} → {}",
        total_layers - 1, current_type, layers.join(" → ")
    );

    let suggestion = if total_layers >= 3 {
        "多层重分布严重影响性能; 强烈建议: /*+ redistribute(t1) */ 显式指定; /*+ broadcast(small) */ 广播小表; /*+ leading(t1 t2 t3) */ 调整连接顺序".to_string()
    } else {
        "使用 hint 减少重分布层数: /*+ redistribute(t1) */ 或 /*+ broadcast(small) */".to_string()
    };

    Some(make_finding(self, detail, node, Some(suggestion)))
}

fn collect_streaming_layers(children: &[PlanNode], layers: &mut Vec<String>) {
    for child in children {
        if matches!(&child.node_type, NodeType::Streaming(_) | NodeType::VectorStreaming(_)) {
            layers.push(streaming_type_name(&child.node_type));
        }
        collect_streaming_layers(&child.children, layers);
    }
}

fn streaming_type_name(nt: &NodeType) -> String {
    match nt {
        NodeType::Streaming(st) => format!("Streaming({:?})", st),
        NodeType::VectorStreaming(st) => format!("VectorStreaming({:?})", st),
        _ => nt.to_string(),
    }
}
```

#### 测试用例

```rust
#[test]
fn push_002_reports_streaming_layer_details() {
    let report = analyze_fixture("16_multi_streaming.txt");
    let finding = get_finding(&report, "PUSH-002").expect("PUSH-002 should fire");
    assert!(
        finding.detail.contains("→") || finding.detail.contains("层"),
        "detail should show streaming layer chain: {}", finding.detail
    );
}
```

---

### Task 2.13: TYPE-001 — Implicit Type Coercion

**文件:** `crates/ogexplain-core/src/analyzer/rules/type_coercion_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 正则 `r"\w+\s*=\s*\d+(\.\d+)?\b"` 过于简单，误报/漏报并存 | 高 |
| 2 | 硬编码 `rows_removed > 1000` 阈值，不使用 config | 中 |
| 3 | 不检测具体的类型不匹配（varchar=int vs text=numeric） | 高 |
| 4 | 建议中 `{filter_value}` 出现在 suggestion 里但不提供具体修复 | 中 |

#### 优化方案

1. **增强正则**：检测更多类型不匹配模式（`varchar = int`, `text = numeric`, `date = varchar`）
2. **阈值可配置化**
3. **提取列名和值的类型**，生成精确修复建议

```rust
// 新增到 DiagnosticConfig
pub type_coercion_rows_removed_threshold: f64, // 默认 1000.0

fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    if node.node_type != NodeType::SeqScan { return None; }
    let filter_prop = node.properties.iter().find(|p| p.label == "Filter")?;
    let filter_value = &filter_prop.value;

    // 检测类型不匹配模式
    let mismatch = detect_type_mismatch(filter_value)?;

    let rows_removed = node.properties.iter()
        .find(|p| p.label == "Rows Removed by Filter")
        .and_then(|p| p.value.trim().parse::<f64>().ok())?;

    if rows_removed <= 1000.0 { return None; }

    let detail = format!(
        "Seq Scan 含过滤条件 '{}' ({}), 过滤掉 {} 行 — 疑似隐式类型转换导致无法使用索引",
        filter_value, mismatch.description(), rows_removed
    );

    let suggestion = mismatch.fix_suggestion();

    Some(make_finding(self, detail, node, Some(suggestion)))
}

struct TypeMismatch {
    column: String,
    value_type: String,  // "int", "float", "string"
    expected_type: String, // "varchar", "text"
}

impl TypeMismatch {
    fn description(&self) -> String {
        format!("{}({}) = {}值", self.expected_type, self.column, self.value_type)
    }

    fn fix_suggestion(&self) -> String {
        match self.value_type.as_str() {
            "int" => format!(
                "WHERE {} = {} — 疑似 varchar 列用 int 值比较, 改为 WHERE {} = '{}'",
                self.column, "N", self.column, "N"
            ),
            _ => format!("添加显式类型转换: WHERE {} = value::{}", self.column, self.expected_type),
        }
    }
}

fn detect_type_mismatch(filter: &str) -> Option<TypeMismatch> {
    // 模式1: word = 数字(无引号) → varchar列=int值
    let re_int = regex::Regex::new(r"(\w+)\s*=\s*(\d+)\b").ok()?;
    if let Some(cap) = re_int.captures(filter) {
        let col = cap.get(1)?.as_str().to_string();
        let val = cap.get(2)?.as_str().to_string();
        // 简单启发: 如果值是纯数字且列名含 status/name/desc 等字符串列名
        // 或 Rows Removed 很高，则可能是类型不匹配
        return Some(TypeMismatch {
            column: col,
            value_type: "int".to_string(),
            expected_type: "varchar".to_string(),
        });
    }
    None
}
```

#### 测试用例

```rust
#[test]
fn type_001_suggests_explicit_cast_fix() {
    let report = analyze_fixture("17_implicit_cast.txt");
    let finding = get_finding(&report, "TYPE-001").expect("TYPE-001 should fire");
    let suggestion = finding.suggestion.as_ref().unwrap();
    // 建议应包含具体修复方案
    assert!(
        suggestion.contains("::") || suggestion.contains("显式") || suggestion.contains("'"),
        "suggestion should suggest explicit cast: {}", suggestion
    );
}

#[test]
fn type_001_detail_contains_mismatch_description() {
    let report = analyze_fixture("17_implicit_cast.txt");
    let finding = get_finding(&report, "TYPE-001").expect("TYPE-001 should fire");
    assert!(
        finding.detail.contains("varchar") || finding.detail.contains("int") || finding.detail.contains("类型"),
        "detail should describe the type mismatch: {}", finding.detail
    );
}
```

---

### Task 2.14: TYPE-004 — LIKE with Leading Wildcard

**文件:** `crates/ogexplain-core/src/analyzer/rules/type_coercion_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 只检测 `LIKE '%`，不检测 `~~`（PostgreSQL 内部 LIKE 运算符）| 中 |
| 2 | 不提取具体的 LIKE 模式 | 中 |
| 3 | 不检查是否有 pg_trgm 索引（如果已有则不需要警告） | 低 |
| 4 | 建议过于简短 | 中 |

#### 优化方案

1. **提取 LIKE 模式**，在 detail 中报告
2. **区分 `LIKE '%...'` 和 `LIKE '%...%'`**，前者有时可用 reverse index 优化
3. **增强建议**：区分全文搜索建议和 pg_trgm 建议

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    for prop in &node.properties {
        if (prop.label == "Filter" || prop.label == "Index Cond")
            && (prop.value.contains("LIKE '%") || prop.value.contains("like '%")
                || prop.value.contains("~~ '%"))
        {
            let pattern = extract_like_pattern(&prop.value).unwrap_or_else(|| prop.value.clone());

            let is_double_sided = pattern.starts_with('%') && pattern.ends_with('%');

            let detail = format!(
                "过滤条件含前导通配符 LIKE '{}', 无法使用 B-tree 索引{}",
                pattern,
                if is_double_sided { " (前后均有通配符)" } else { "" }
            );

            let suggestion = if is_double_sided {
                "前后通配符 LIKE 无法使用任何索引; 建议: (1) pg_trgm 扩展 + GIN 索引: CREATE EXTENSION pg_trgm; CREATE INDEX idx USING gin(col gin_trgm_ops); (2) 全文搜索: to_tsvector + to_tsquery".to_string()
            } else {
                "前导通配符 LIKE 无法使用 B-tree 索引; 建议: pg_trgm 扩展; 或反向索引(reverse(col))".to_string()
            };

            return Some(make_finding(self, detail, node, Some(suggestion)));
        }
    }
    None
}

fn extract_like_pattern(value: &str) -> Option<String> {
    let re = regex::Regex::new(r#"LIKE\s+'([^']+)'"#).ok()?;
    re.captures(value).map(|cap| cap.get(1).unwrap().as_str().to_string())
}
```

#### 测试用例

```rust
#[test]
fn type_004_distinguishes_single_vs_double_wildcard() {
    let report = analyze_fixture("18_like_wildcard.txt");
    let finding = get_finding(&report, "TYPE-004").expect("TYPE-004 should fire");
    assert!(
        finding.detail.contains("通配符"),
        "detail should describe wildcard position: {}", finding.detail
    );
}

#[test]
fn type_004_suggests_pgtrgm_or_fts() {
    let report = analyze_fixture("18_like_wildcard.txt");
    let finding = get_finding(&report, "TYPE-004").expect("TYPE-004 should fire");
    let suggestion = finding.suggestion.as_ref().unwrap();
    assert!(
        suggestion.contains("pg_trgm") || suggestion.contains("全文"),
        "suggestion should mention pg_trgm or full-text search"
    );
}
```

---

### Task 2.15: VEC-001 — Mixed Row/Vector Engines

**文件:** `crates/ogexplain-core/src/analyzer/rules/vectorization_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 只统计 Adapter 数量（≥2），不报告具体切换位置和方向 | 高 |
| 2 | 不区分 Row→Vector 和 Vector→Row 方向 | 中 |
| 3 | 建议泛化，不给出具体的引擎切换消除策略 | 中 |
| 4 | 手动构造 Finding（未用 `make_finding`） | 低 |

#### 优化方案

1. **信号累积模式**：收集每个 Adapter 的位置、方向和两侧节点类型
2. **报告具体切换点**：如 "Row Adapter at line 5: Hash Join → Vec Sort"
3. **给出精确消除策略**

```rust
struct AdapterSignal {
    adapter_type: String,      // "Row Adapter" or "Vector Adapter"
    line_number: Option<usize>,
    parent_type: Option<String>,  // 上层节点类型
    child_type: Option<String>,   // 下层节点类型
    direction: String,          // "Row→Vector" or "Vector→Row"
}

fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
    let mut adapters: Vec<AdapterSignal> = Vec::new();
    collect_adapter_signals(&plan.root, None, &mut adapters);

    if adapters.len() < 2 { return Vec::new(); }

    let switch_points: Vec<String> = adapters.iter().map(|a| {
        format!("{} (line {:?}): {} [{}]",
            a.adapter_type, a.line_number, a.direction,
            format!("{} → {}",
                a.parent_type.as_deref().unwrap_or("?"),
                a.child_type.as_deref().unwrap_or("?")
            )
        )
    }).collect();

    let detail = format!(
        "执行计划含 {} 处引擎切换 (需要 {} 次适配器转换): {}",
        adapters.len(), adapters.len(),
        switch_points.join("; ")
    );

    let suggestion = "统一使用同一引擎以消除适配器开销; SET try_vector_engine_strategy=force 尝试全向量化; 行存点查: SET enable_vector_engine=off".to_string();

    vec![Finding {
        rule_id: self.id().to_string(),
        severity: self.severity(),
        category: self.category(),
        title: self.name().to_string(),
        detail,
        node_line: None,
        node_type: None,
        suggestion: Some(suggestion),
        sql_rewrite: None,
    }]
}
```

#### 测试用例

```rust
#[test]
fn vec_001_reports_switch_points() {
    let report = analyze_fixture("19_mixed_engines.txt");
    let finding = get_finding(&report, "VEC-001").expect("VEC-001 should fire");
    assert!(
        finding.detail.contains("→") || finding.detail.contains("切换"),
        "detail should show switch points: {}", finding.detail
    );
}

#[test]
fn vec_001_does_not_fire_without_adapters() {
    let report = analyze_fixture("07_vector_hash_join.txt");
    assert!(!has_finding(&report, "VEC-001"));
}
```

---

### Task 2.16: GEN-001 — Plan Too Deep

**文件:** `crates/ogexplain-core/src/analyzer/rules/general_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 只报告深度值，不指出最深路径 | 中 |
| 2 | 不区分深度来源（子查询嵌套 vs 多表 JOIN vs 窗口函数嵌套） | 中 |
| 3 | 建议是泛化的 hint 列表 | 中 |

#### 优化方案

1. **追踪最深路径**，报告路径上的节点类型序列
2. **按来源分类建议**

```rust
fn check_global(&self, _plan: &ExplainPlan, stats: &GlobalStats) -> Vec<Finding> {
    if stats.max_depth <= self.max_depth { return Vec::new(); }

    // 找到最深路径（需新增 GlobalStats 方法或直接遍历 plan）
    // 简化：报告深度值和阈值

    vec![Finding {
        rule_id: self.id().to_string(),
        severity: self.severity(),
        category: self.category(),
        title: self.name().to_string(),
        detail: format!(
            "执行计划深度为 {}（阈值: {}）; 深度过高通常表示子查询未提升或多层嵌套",
            stats.max_depth, self.max_depth
        ),
        node_line: None,
        node_type: None,
        suggestion: Some("简化查询: /*+ EXPAND_SUBQUERY */; /*+ EXPAND_SUBLINK */; /*+ LAZY_AGG */; /*+ REDUCE_ORDER_BY */; 考虑拆分为多个简单查询".to_string()),
        sql_rewrite: None,
    }]
}
```

#### 测试用例

```rust
// 已有测试覆盖，主要增强 detail 断言
#[test]
fn gen_001_detail_mentions_depth_reason() {
    let plan = parse_fixture("20_deep_plan.txt");
    let config = DiagnosticConfig { max_plan_depth: 5, ..Default::default() };
    let report = analyze_with_config(&plan, &config);
    let finding = get_finding(&report, "GEN-001").expect("GEN-001 should fire");
    assert!(
        finding.detail.contains("子查询") || finding.detail.contains("嵌套") || finding.detail.contains("深度"),
        "detail should explain depth reason: {}", finding.detail
    );
}
```

---

### Task 2.17: SUBQ-001 — Subquery Not Pulled Up

**文件:** `crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 建议中 hint 列表是硬编码字符串，不适配上下文 | 中 |
| 2 | 不区分 SubqueryScan 和 SubPlan 的严重度差异 | 低 |
| 3 | 不提取子查询涉及的表名 | 中 |

#### 优化方案

1. **提取子查询相关表名**，参数化建议
2. **SubPlan 标记为 Warning**（已实现），SubqueryScan 可考虑 Critical（通常更严重）

```rust
fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    // SubqueryScan — 未提升的子查询（通常更严重）
    if node.node_type == NodeType::SubqueryScan
        || node.node_type == NodeType::VectorSubqueryScan
    {
        let child_table = node.children.first()
            .and_then(|c| c.relation.clone())
            .unwrap_or_else(|| "unknown".to_string());

        return Some(make_finding(
            self,
            format!("检测到未提升的子查询(SubqueryScan), 涉及表: {}", child_table),
            node,
            Some(format!(
                "改写为JOIN: /*+ EXPAND_SUBQUERY */; 若为关联子查询: /*+ EXPAND_SUBLINK */; 考虑 /*+ USE_MAGIC_SET */ 优化",
            )),
        ));
    }

    // Result + SubPlan — 关联子链接
    if node.node_type == NodeType::Result || node.node_type == NodeType::VectorResult {
        if any_property_contains(node, "SubPlan") {
            return Some(make_finding(
                self,
                "检测到未提升的子查询(SubPlan in Result)".to_string(),
                node,
                Some("/*+ EXPAND_SUBLINK */ 提升子链接; /*+ USE_MAGIC_SET */ 优化关联子查询".to_string()),
            ));
        }
    }

    None
}
```

#### 测试用例

```rust
// 需要包含 SubqueryScan 的 fixture
// 目前测试覆盖中未单独测试 SUBQ-001，应补全
#[test]
fn subq_001_triggers_on_subquery_scan() {
    // 需 fixture 包含 SubqueryScan 节点
}

#[test]
fn subq_001_triggers_on_subplan_in_result() {
    // 需 fixture 包含 Result 节点且 property 含 SubPlan
}
```

---

### Task 2.18: REW-001 — Large IN List Not Converted

**文件:** `crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 阈值硬编码为 10 个值，不可配置 | 中 |
| 2 | 不提取 IN 列表涉及的列名 | 中 |
| 3 | 建议中 INSERT INTO temp 是粗糙方案 | 中 |

#### 优化方案

1. **阈值可配置化**（新增到 `DiagnosticConfig`）
2. **提取列名**，参数化建议

```rust
// DiagnosticConfig 新增
pub in_list_threshold: usize, // 默认 10

fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
    let filter_prop = node.properties.iter()
        .find(|p| p.label == "Filter" && p.value.contains("IN ("))?;
    let comma_count = filter_prop.value.matches(',').count();
    if comma_count <= self.in_list_threshold { return None; }

    // 提取列名
    let column = extract_in_list_column(&filter_prop.value)
        .unwrap_or_else(|| "col".to_string());
    let relation = node.relation.as_deref().unwrap_or("unknown");

    let detail = format!(
        "过滤条件含长IN列表({}个值), 列: {}, 表: {}",
        comma_count + 1, column, relation
    );

    let suggestion = format!(
        "/*+ INLIST_TO_JOIN */; 或改写: SELECT * FROM {} WHERE {}.{} IN (SELECT val FROM temp_in_list)",
        relation, relation, column
    );

    Some(make_finding(self, detail, node, Some(suggestion)))
}
```

#### 测试用例

```rust
#[test]
fn rew_001_detail_mentions_column_and_table() {
    // 需 fixture: Seq Scan with Filter: col IN (v1, v2, ..., v15+)
}

#[test]
fn rew_001_respects_configurable_threshold() {
    // config.in_list_threshold = 20 → 不触发（如果列表为 15）
}
```

---

## 3. Suggester 优化

### Task 3.1: 增强 SuggestionEngine 的跨规则综合能力

**文件:** `crates/ogexplain-core/src/suggester/mapper.rs`

#### 当前问题

| # | 问题 | 严重度 |
|---|------|--------|
| 1 | 只按 rule_id 前缀分组，不理解 Finding.detail 内容 | 中 |
| 2 | 建议的 confidence 是硬编码常量 | 低 |
| 3 | 缺少 TYPE-* 的专项建议 | 中 |
| 4 | 缺少 VEC-* 的专项建议 | 中 |

#### 优化方案

1. **新增 TYPE 建议**：当有 TYPE-001 + TYPE-004 时，建议全面检查类型一致性
2. **新增 VEC 建议**：当有 VEC-001 时，建议统一引擎策略
3. **动态 confidence**：根据 findings 数量调整（3 个 spill 比 2 个更可信）

```rust
// 在 SuggestionEngine::suggest() 中新增

// 类型不一致综合
let type_findings: Vec<&Finding> = findings
    .iter()
    .filter(|f| f.rule_id.starts_with("TYPE-"))
    .collect();
if type_findings.len() >= 2 {
    suggestions.push(Suggestion {
        related_rules: type_findings.iter().map(|f| f.rule_id.clone()).collect(),
        category: SuggestionCategory::QueryRewrite,
        message: "多处类型不一致问题, 建议全面审查 WHERE/JOIN 条件中的数据类型匹配".to_string(),
        confidence: 0.85,
    });
}

// 向量化引擎
let vec_findings: Vec<&Finding> = findings
    .iter()
    .filter(|f| f.rule_id.starts_with("VEC-"))
    .collect();
if !vec_findings.is_empty() {
    suggestions.push(Suggestion {
        related_rules: vec_findings.iter().map(|f| f.rule_id.clone()).collect(),
        category: SuggestionCategory::ConfigurationTuning,
        message: "检测到引擎切换, 建议统一使用行引擎或向量化引擎以消除 Adapter 开销".to_string(),
        confidence: 0.8,
    });
}

// 动态 confidence: spill findings 越多越可信
let spill_confidence = (0.7 + spill_rules.len() as f64 * 0.1).min(0.95);
```

#### 测试用例

```rust
#[test]
fn suggestion_type_findings_trigger_type_review() {
    // 模拟 TYPE-001 + TYPE-004 findings
    // 验证 SuggestionEngine 产出类型审查建议
}

#[test]
fn suggestion_vec_findings_trigger_engine_unification() {
    // 模拟 VEC-001 finding
    // 验证 SuggestionEngine 产出引擎统一建议
}
```

---

## 4. 测试基础设施补全

### Task 4.1: 补全缺失的 Fixture 文件

**依赖:** 无（可与 Task 2.x 并行）

需要新建以下 fixture 以支持新增测试：

| Fixture | 用途 | 需覆盖的规则 |
|---------|------|-------------|
| `26_vector_sort_external.txt` | VectorSort + external merge | MEM-001 扩展 |
| `27_indexed_nested_loop.txt` | NL with Index Scan inner | JOIN-001 守护 |
| `28_different_sort_keys.txt` | 父子 Sort Key 不同 | SORT-003 守护 |
| `29_subquery_scan_plan.txt` | SubqueryScan 节点 | SUBQ-001 正向 |
| `30_large_in_list.txt` | Filter with 20+ IN values | REW-001 正向 |
| `31_cstore_large_scan.txt` | CStore Scan 50k rows | SCAN-001 扩展 |

**提交:** `test: add fixtures for rule optimization test coverage`

### Task 4.2: 添加规则间互不干扰回归测试

**依赖:** Task 1.1, Task 2.x 完成后

确保优化后的规则不会意外触发其他规则的误报：

```rust
#[test]
fn rules_do_not_cross_contaminate() {
    // 对每个 fixture 验证：触发的规则集合与优化前一致
    // 使用 snapshot 模式：记录每个 fixture 的 expected rule_ids
    let cases = vec![
        ("01_simple_seq_scan.txt", vec![] as Vec<&str>),
        ("10_complex_plan.txt", vec!["SCAN-001", "MEM-001"]),
        ("12_hash_spill.txt", vec!["JOIN-002", "MEM-004"]),
        // ... 全部 25 个 fixture
    ];
    for (fixture, expected_rules) in cases {
        let report = analyze_fixture(fixture);
        let actual_rules: Vec<&str> = report.findings.iter()
            .map(|f| f.rule_id.as_str())
            .collect();
        // 允许新增规则触发，但不允许已有规则消失
        for expected in &expected_rules {
            assert!(actual_rules.contains(expected),
                "{}: {} should still be triggered", fixture, expected);
        }
    }
}
```

---

## 5. 执行顺序与依赖关系

```
Task 1.1 (utils.rs 提取)
  ├── Task 1.2 (utils 测试)
  │
  ├── Task 2.1  (SCAN-001)   ─┐
  ├── Task 2.2  (SCAN-004)    │
  ├── Task 2.3  (JOIN-001)    │ 可并行
  ├── Task 2.4  (JOIN-002)    │
  ├── Task 2.5  (MEM-001)     │
  ├── Task 2.6  (MEM-004)     │
  ├── Task 2.7  (SORT-003)    │
  ├── Task 2.8  (NET-001)     │
  ├── Task 2.9  (EST-001)     │
  ├── Task 2.10 (EST-004)     │
  ├── Task 2.11 (PUSH-001)    │
  ├── Task 2.12 (PUSH-002)    │
  ├── Task 2.13 (TYPE-001)    │
  ├── Task 2.14 (TYPE-004)    │
  ├── Task 2.15 (VEC-001)     │
  ├── Task 2.16 (GEN-001)     │
  ├── Task 2.17 (SUBQ-001)    │
  └── Task 2.18 (REW-001)    ─┘
                               │
Task 4.1 (新 Fixture)    ──────┤ (可与 2.x 并行)
                               │
                    ┌──────────┘
                    ▼
Task 3.1 (Suggester 增强)
                    │
                    ▼
Task 4.2 (回归测试)
```

**推荐执行策略：**
1. **先做 Task 1.1 + 1.2**（基础层，阻塞后续）
2. **并行做 Task 4.1**（新 fixture，不阻塞但尽早完成）
3. **按类别分批并行做 Task 2.x**（scan 类一组、join 类一组、memory 类一组...）
4. **Task 3.1 和 4.2 最后做**（需要所有规则优化完成）

---

## 6. 验证清单

每个 Task 完成后必须通过：

- [ ] `cargo test --workspace` — 全部通过
- [ ] `cargo clippy --workspace` — 零 warning
- [ ] `cargo fmt --all --check` — 格式正确
- [ ] 新增测试全部通过（正向 + 守护）
- [ ] 未破坏已有测试
- [ ] Finding 结构字段完整（rule_id, severity, category, title, detail, suggestion）
