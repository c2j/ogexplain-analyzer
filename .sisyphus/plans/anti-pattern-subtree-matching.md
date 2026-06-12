# Anti-Pattern Subtree Matching 实施方案

> **版本**: v1.0
> **日期**: 2026-06-13
> **状态**: Draft
> **前置分析**: 本文件基于对现有架构的完整代码审查（25 条规则、DiagnosticEngine、PlanNode 模型、Suggester 系统）

---

## 0. 设计决策摘要

基于可行性分析的关键结论：

| 决策点 | 结论 | 理由 |
|--------|------|------|
| 集成方式 | `check_global()` 全树匹配 | 利用已有模式（VEC-001/SKEW-001 先例），零修改 DiagnosticEngine |
| Pattern 表示 | 编译时 Rust 结构体 + 闭包 | 类型安全、与 `Send + Sync` 兼容、无需 DSL 解析器 |
| Finding 扩展 | 增量添加 `Option<Evidence>` | 向后兼容，现有规则不受影响 |
| 反模式优先级 | ANTI-005 > ANTI-003 > ANTI-007 | 按实现难度和价值排序，避免与现有规则重叠 |
| 去重 | `related_classic_rules` 标记 | 先标记后优化，不急于实现抑制逻辑 |
| 模板渲染 | `str::replace` 简单替换 | 当前需求足够，无需引入模板引擎 |

### 反模式选择理由

10 个提议反模式中，优先实施 3 个**无重叠、高价值、可立即实现**的反模式：

| ID | 名称 | 为什么优先 | 依赖 |
|----|------|-----------|------|
| **ANTI-005** | 多层物化堆砌 | 纯结构匹配（Materialize → Materialize → NestedLoop），零属性依赖 | 无 |
| **ANTI-003** | 索引回表放大 | 需解析 Index Cond + Filter 列名，展示属性谓词匹配能力 | `utils::get_property_value` |
| **ANTI-007** | CN 端大排序 | 需父节点感知（Sort 的祖先含 Streaming(GATHER)），展示祖先路径追踪 | 无 |

**暂缓实施**（与现有规则重叠或依赖未提取属性）：

| ID | 暂缓原因 |
|----|----------|
| ANTI-001 | 与 NET-001 几乎完全重叠，应先增强 NET-001 |
| ANTI-002 | 与 VEC-001 高度重叠 |
| ANTI-004 | 需确认 `Skew Optimization` 属性是否被解析器提取 |
| ANTI-006 | `column_count` 在当前模型中不存在，需扩展 |
| ANTI-008 | 与 PUSH-001/002 部分重叠，语义复杂 |
| ANTI-009 | `Sonic Hash` 属性未被解析器提取 |
| ANTI-010 | 与 PART-001 几乎完全重叠 |

---

## 1. 新增文件结构

```
crates/ogexplain-core/src/
├── analyzer/
│   ├── rules/                    # 现有，不修改
│   │   └── mod.rs                # 仅在 all_rules() 中添加一行注册
│   ├── pattern/                  # 新增模块
│   │   ├── mod.rs                # 模块入口 + AntiPatternRule
│   │   ├── types.rs              # AntiPattern, MatchResult, Evidence, FieldPath
│   │   ├── engine.rs             # PatternEngine：DFS 匹配骨架
│   │   ├── predicates.rs         # FieldAccessor + 谓词评估
│   │   ├── templates.rs          # 简单模板渲染
│   │   └── patterns/             # 各反模式定义
│   │       ├── mod.rs
│   │       ├── materialize_cascade.rs   # ANTI-005
│   │       ├── index_scan_amplify.rs    # ANTI-003
│   │       └── gather_then_sort.rs      # ANTI-007
```

---

## 2. 核心数据结构

### 2.1 types.rs — 匹配结果与证据

```rust
//! Anti-pattern matching types.

use serde::Serialize;
use std::collections::HashMap;

/// 一次成功匹配的完整结果
#[derive(Debug, Clone)]
pub struct MatchResult<'a> {
    pub pattern_id: String,
    pub captures: HashMap<String, &'a crate::model::PlanNode>,
    /// 从匹配根到 Plan 根的路径（祖先栈，不含匹配根本身）
    pub ancestors: Vec<&'a crate::model::PlanNode>,
    /// 匹配起始节点（子树根）
    pub matched_node: &'a crate::model::PlanNode,
}

/// Finding 上的证据附加信息（增量扩展）
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Evidence {
    pub pattern_id: String,
    pub confidence: f64,
    pub matched_nodes: Vec<MatchedNode>,
    /// 被此反模式覆盖的经典规则 ID（用于去重标记）
    pub related_classic_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MatchedNode {
    pub capture_name: String,
    pub line_number: usize,
    pub node_type: String,
    pub relation: Option<String>,
}
```

### 2.2 predicates.rs — 统一字段访问

```rust
//! Unified field accessor for pattern predicates.

use crate::model::{NodeType, PlanNode, StreamingType};

/// 节点上可查询的字段路径
#[derive(Debug, Clone)]
pub enum FieldPath {
    ActualRows,
    ActualLoops,
    ActualTimeMs,
    EstimatedRows,
    EstimatedTotalCost,
    Relation,
    PeakMemoryKb,
    RowsRemovedByFilter,
    HashBatches,
    SortMethod,
    SelectedPartitions,
    /// 原始 NodeProperty 按标签查找
    Property(String),
    /// Streaming 子类型
    StreamingType,
    /// 子节点数量
    ChildCount,
}

pub struct FieldAccessor;

impl FieldAccessor {
    /// 从节点提取字段值，返回 f64（数值型）或 None
    pub fn get_f64(node: &PlanNode, path: &FieldPath) -> Option<f64> {
        match path {
            FieldPath::ActualRows => node.actual.as_ref().map(|a| a.rows),
            FieldPath::ActualLoops => node.actual.as_ref().map(|a| a.loops),
            FieldPath::ActualTimeMs => node.actual.as_ref().map(|a| a.total_time_ms),
            FieldPath::EstimatedRows => node.estimated.as_ref().map(|e| e.plan_rows),
            FieldPath::EstimatedTotalCost => node.estimated.as_ref().map(|e| e.total_cost),
            FieldPath::PeakMemoryKb => node
                .structured_props
                .as_ref()
                .and_then(|p| p.peak_memory_kb),
            FieldPath::RowsRemovedByFilter => node
                .structured_props
                .as_ref()
                .and_then(|p| p.rows_removed_by_filter),
            FieldPath::HashBatches => node
                .structured_props
                .as_ref()
                .and_then(|p| p.hash_batches.map(|b| b as f64)),
            FieldPath::ChildCount => Some(node.children.len() as f64),
            _ => None,
        }
    }

    /// 从节点提取字段值，返回 &str（字符串型）或 None
    pub fn get_str<'a>(node: &'a PlanNode, path: &FieldPath) -> Option<&'a str> {
        match path {
            FieldPath::Relation => node.relation.as_deref(),
            FieldPath::SortMethod => node
                .structured_props
                .as_ref()
                .and_then(|p| p.sort_method.as_deref()),
            FieldPath::SelectedPartitions => node
                .structured_props
                .as_ref()
                .and_then(|p| p.selected_partitions.as_deref()),
            FieldPath::Property(label) => {
                crate::analyzer::rules::utils::get_property_value(node, label)
            }
            _ => None,
        }
    }

    /// 提取 Streaming 子类型
    pub fn get_streaming_type(node: &PlanNode) -> Option<&StreamingType> {
        match &node.node_type {
            NodeType::Streaming(st) | NodeType::VectorStreaming(st) => Some(st),
            _ => None,
        }
    }
}
```

### 2.3 engine.rs — 匹配引擎骨架

```rust
//! Anti-pattern matching engine.

use super::types::MatchResult;
use crate::model::{ExplainPlan, PlanNode};

/// 单个反模式的匹配逻辑
pub trait AntiPatternDef: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> crate::analyzer::report::Severity;
    fn category(&self) -> crate::analyzer::report::DiagnosticCategory;
    fn related_classic_rules(&self) -> Vec<String>;

    /// 在以 `root` 为根的子树中尝试匹配。
    /// `ancestors` 包含从 Plan 根到 root 父节点的路径（可能为空）。
    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>>;
}

/// DFS 遍历计划树，在每个节点上尝试所有已注册的反模式
pub struct PatternEngine {
    patterns: Vec<Box<dyn AntiPatternDef>>,
}

impl PatternEngine {
    pub fn new(patterns: Vec<Box<dyn AntiPatternDef>>) -> Self {
        Self { patterns }
    }

    /// 在整棵计划树上运行所有反模式匹配
    pub fn match_plan<'a>(&self, plan: &'a ExplainPlan) -> Vec<MatchResult<'a>> {
        let mut results = Vec::new();
        self.walk(&plan.root, &[], &mut results);
        results
    }

    fn walk<'a>(
        &self,
        node: &'a PlanNode,
        ancestors: &[&'a PlanNode],
        results: &mut Vec<MatchResult<'a>>,
    ) {
        for pattern in &self.patterns {
            if let Some(result) = pattern.try_match(node, ancestors) {
                results.push(result);
            }
        }
        for child in &node.children {
            let mut extended = ancestors.to_vec();
            extended.push(node);
            self.walk(child, &extended, results);
        }
    }
}
```

### 2.4 mod.rs — AntiPatternRule（DiagnosticRule 实现）

```rust
//! Anti-pattern subtree matching module.

pub mod engine;
pub mod predicates;
pub mod patterns;
pub mod templates;
pub mod types;

use crate::analyzer::context::{GlobalStats, PlanContext};
use crate::analyzer::report::{
    DiagnosticCategory, DiagnosticReport, Finding, Severity,
};
use crate::analyzer::rules::DiagnosticRule;
use crate::model::{ExplainPlan, PlanNode};

use engine::PatternEngine;
use types::Evidence;

/// 单一 DiagnosticRule，内部运行所有反模式匹配
pub struct AntiPatternRule {
    engine: PatternEngine,
}

impl AntiPatternRule {
    pub fn new() -> Self {
        let patterns: Vec<Box<dyn engine::AntiPatternDef>> = vec![
            Box::new(patterns::materialize_cascade::MaterializeCascade),
            Box::new(patterns::index_scan_amplify::IndexScanAmplify::default()),
            Box::new(patterns::gather_then_sort::GatherThenSort::default()),
        ];
        Self {
            engine: PatternEngine::new(patterns),
        }
    }
}

impl DiagnosticRule for AntiPatternRule {
    fn id(&self) -> &str { "ANTI" }
    fn name(&self) -> &str { "反模式子树检测" }
    fn severity(&self) -> Severity { Severity::Warning }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::General }

    fn check(&self, _node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        None // 反模式不参与逐节点扫描
    }

    fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        self.engine
            .match_plan(plan)
            .into_iter()
            .map(|result| {
                let pattern = self.engine.find_pattern(&result.pattern_id);
                let detail = templates::render_detail(pattern, &result);
                let suggestion = templates::render_suggestion(pattern, &result);

                Finding {
                    rule_id: result.pattern_id.clone(),
                    severity: pattern.severity(),
                    category: pattern.category(),
                    title: pattern.name().to_string(),
                    detail,
                    node_line: Some(result.matched_node.line_number),
                    node_type: Some(result.matched_node.node_type.to_string()),
                    suggestion: Some(suggestion),
                    sql_rewrite: None,
                    evidence: Some(Evidence {
                        pattern_id: result.pattern_id,
                        confidence: 1.0, // 结构匹配默认高置信
                        matched_nodes: result
                            .captures
                            .iter()
                            .map(|(name, node)| types::MatchedNode {
                                capture_name: name.clone(),
                                line_number: node.line_number,
                                node_type: node.node_type.to_string(),
                                relation: node.relation.clone(),
                            })
                            .collect(),
                        related_classic_rules: pattern.related_classic_rules(),
                    }),
                }
            })
            .collect()
    }
}
```

### 2.5 Finding 扩展（report.rs 增量修改）

```rust
// 在现有 Finding struct 末尾新增一个字段：
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    // ... 现有字段不变 ...
    pub suggestion: Option<String>,
    pub sql_rewrite: Option<crate::rewriter::types::RewriteResult>,

    // 新增：反模式证据（经典规则为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<crate::analyzer::pattern::types::Evidence>,
}
```

### 2.6 templates.rs — 简单模板渲染

```rust
//! Simple template rendering for anti-pattern diagnostics.

use std::collections::HashMap;

use super::engine::AntiPatternDef;
use super::types::MatchResult;
use crate::model::PlanNode;

pub fn render_detail(pattern: &dyn AntiPatternDef, result: &MatchResult) -> String {
    let detail_template = pattern.detail_template();
    render_template(&detail_template, &result.captures)
}

pub fn render_suggestion(pattern: &dyn AntiPatternDef, result: &MatchResult) -> String {
    let suggestion_template = pattern.suggestion_template();
    render_template(&suggestion_template, &result.captures)
}

/// 替换 {capture_name.property} 占位符
fn render_template(
    template: &str,
    captures: &HashMap<String, &PlanNode>,
) -> String {
    let mut result = template.to_string();
    for (name, node) in captures {
        // {name} → node type
        result = result.replace(
            &format!("{{{name}}}"),
            &node.node_type.to_string(),
        );
        // {name.relation} → relation name
        result = result.replace(
            &format!("{{{name}.relation}}"),
            node.relation.as_deref().unwrap_or("?"),
        );
        // {name.actual_rows} → actual rows (formatted)
        if let Some(a) = &node.actual {
            result = result.replace(
                &format!("{{{name}.actual_rows}}"),
                &format!("{}", a.rows as u64),
            );
            result = result.replace(
                &format!("{{{name}.loops}}"),
                &format!("{}", a.loops as u64),
            );
            result = result.replace(
                &format!("{{{name}.total_work}}"),
                &format!("{}", (a.rows * a.loops) as u64),
            );
        }
        // {name.line} → line number
        result = result.replace(
            &format!("{{{name}.line}}"),
            &format!("{}", node.line_number),
        );
    }
    result
}
```

---

## 3. 反模式定义

### 3.1 ANTI-005: 多层物化堆砌（Materialize Cascade）

**最简单的反模式——纯结构匹配，无属性谓词**

```rust
// patterns/materialize_cascade.rs

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode};

/// ANTI-005: Materialize → Materialize → NestedLoop 嵌套
///
/// 优化器对 NestedLoop 内表做了双重物化，通常意味着内表被重复扫描多次
/// 或优化器过度保守，未选择 Hash Join。
pub struct MaterializeCascade;

impl AntiPatternDef for MaterializeCascade {
    fn id(&self) -> &str { "ANTI-005" }
    fn name(&self) -> &str { "多层物化堆砌" }
    fn severity(&self) -> Severity { Severity::Warning }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::MemoryUsage }
    fn related_classic_rules(&self) -> Vec<String> { vec![] }

    fn detail_template(&self) -> String {
        "Materialize → Materialize → {nl} 三层嵌套结构，优化器对 NestedLoop 内表做了双重物化。\
         这通常意味着内表被重复扫描多次或优化器过度保守。".to_string()
    }

    fn suggestion_template(&self) -> String {
        "1. 检查 enable_hashjoin 是否被关闭\n\
         2. 尝试强制 Hash Join: SET enable_nestloop = off;\n\
         3. 确认内表是否有合适索引支持 Index Scan".to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        _ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        // 匹配：Materialize → Materialize → (NestedLoop | VectorNestLoop)
        if root.node_type != NodeType::Materialize
            && root.node_type != NodeType::VectorMaterialize
        {
            return None;
        }

        let mat2 = root.children.first()?;
        if mat2.node_type != NodeType::Materialize
            && mat2.node_type != NodeType::VectorMaterialize
        {
            return None;
        }

        let nl = mat2.children.first()?;
        if nl.node_type != NodeType::NestedLoop
            && nl.node_type != NodeType::VectorNestLoop
        {
            return None;
        }

        let mut captures = std::collections::HashMap::new();
        captures.insert("mat1".to_string(), root);
        captures.insert("mat2".to_string(), mat2);
        captures.insert("nl".to_string(), nl);

        Some(MatchResult {
            pattern_id: self.id().to_string(),
            captures,
            ancestors: _ancestors.to_vec(),
            matched_node: root,
        })
    }
}
```

**测试夹具** (`tests/fixtures/anti/anti005_materialize_cascade.txt`):

```
                          Query Plan
 ----------------------------------------------------------
  Materialize
    cost=0.00..100000.00 rows=10000 width=100
    Actual time=0.010..50.000 rows=5000 loops=1
    Materialize
      cost=0.00..50000.00 rows=5000 width=80
      Actual time=0.005..25.000 rows=5000 loops=10
      Nested Loop
        cost=0.00..50000.00 rows=5000 width=80
        Actual time=0.005..25.000 rows=5000 loops=10
        ->  Seq Scan on outer_table
              cost=0.00..100.00 rows=100 width=40
              Actual time=0.001..0.500 rows=100 loops=10
        ->  Index Scan using idx_inner on inner_table
              Index Cond: (id = outer_table.id)
              cost=0.00..50.00 rows=50 width=40
              Actual time=0.001..0.200 rows=50 loops=1000
```

### 3.2 ANTI-003: 索引回表放大（Index Scan Amplification）

**展示属性谓词匹配——解析 Index Cond + Filter 列名**

```rust
// patterns/index_scan_amplify.rs

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::analyzer::rules::utils::get_property_value;
use crate::model::{NodeType, PlanNode};

/// ANTI-003: NestedLoop 驱动大量循环 + IndexScan 回表后仍有 Filter
///
/// Index Cond 过滤不够精确，回表后又过滤大量行。
pub struct IndexScanAmplify {
    threshold: f64,
}

impl Default for IndexScanAmplify {
    fn default() -> Self {
        Self { threshold: 10000.0 }
    }
}

impl AntiPatternDef for IndexScanAmplify {
    fn id(&self) -> &str { "ANTI-003" }
    fn name(&self) -> &str { "索引回表放大" }
    fn severity(&self) -> Severity { Severity::Warning }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::ScanEfficiency }
    fn related_classic_rules(&self) -> Vec<String> {
        vec!["JOIN-001".to_string()]  // JOIN-001 也检测 NestedLoop 大数据集
    }

    fn detail_template(&self) -> String {
        "{nl} 驱动 {nl.actual_rows} 次循环，内表 {idx} 通过 Index Scan 访问 \
         {idx.relation}，但 Index Cond 后仍有 Filter 过滤大量行（回表后过滤）。\
         总回表次数: {idx.total_work}".to_string()
    }

    fn suggestion_template(&self) -> String {
        "1. 创建覆盖索引以消除回表后过滤\n\
         2. 或改写 SQL 将 Filter 条件纳入索引列\n\
         3. 若驱动表过大，考虑改用 Hash Join".to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        _ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        if root.node_type != NodeType::NestedLoop {
            return None;
        }

        let actual = root.actual.as_ref()?;
        if actual.rows < self.threshold {
            return None;
        }

        // 在子节点中查找 IndexScan + Filter 组合
        for child in &root.children {
            let is_index_scan = matches!(
                child.node_type,
                NodeType::IndexScan
                    | NodeType::PartitionedIndexScan
                    | NodeType::IndexOnlyScan
                    | NodeType::PartitionedIndexOnlyScan
            );
            if !is_index_scan {
                continue;
            }

            // 检查：同时有 Index Cond 和 Filter → 回表后还在过滤
            let has_index_cond = get_property_value(child, "Index Cond").is_some();
            let has_filter = get_property_value(child, "Filter").is_some();
            if !has_index_cond || !has_filter {
                continue;
            }

            // 检查回表后过滤量
            let rows_removed = child
                .structured_props
                .as_ref()
                .and_then(|p| p.rows_removed_by_filter);
            if rows_removed.unwrap_or(0.0) <= 0.0 {
                continue;
            }

            let mut captures = HashMap::new();
            captures.insert("nl".to_string(), root);
            captures.insert("idx".to_string(), child);

            return Some(MatchResult {
                pattern_id: self.id().to_string(),
                captures,
                ancestors: _ancestors.to_vec(),
                matched_node: root,
            });
        }

        None
    }
}
```

**测试夹具** (`tests/fixtures/anti/anti003_index_amplify.txt`):

```
                          Query Plan
 ----------------------------------------------------------
  Nested Loop
    cost=0.00..500000.00 rows=50000 width=100
    Actual time=0.100..5000.000 rows=50000 loops=1
    ->  Seq Scan on orders
          cost=0.00..1000.00 rows=50000 width=40
          Actual time=0.010..100.000 rows=50000 loops=1
    ->  Index Scan using idx_order_items_order_id on order_items
          Index Cond: (order_id = orders.id)
          Filter: (status = 'pending'::text)
          Rows Removed by Filter: 45000
          cost=0.00..10.00 rows=1 width=60
          Actual time=0.001..0.050 rows=1 loops=50000
```

### 3.3 ANTI-007: CN 端大排序（Gather-Then-Sort）

**展示祖先路径追踪——Sort 的祖先含 Streaming(GATHER)**

```rust
// patterns/gather_then_sort.rs

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode, StreamingType};

/// ANTI-007: Streaming(GATHER) → Sort 大量行
///
/// 数据从 DN 汇聚到 CN 后执行 Sort，所有排序在单节点完成，无法利用 DN 并行。
pub struct GatherThenSort {
    threshold: f64,
}

impl Default for GatherThenSort {
    fn default() -> Self {
        Self { threshold: 100000.0 }
    }
}

impl AntiPatternDef for GatherThenSort {
    fn id(&self) -> &str { "ANTI-007" }
    fn name(&self) -> &str { "CN端大排序" }
    fn severity(&self) -> Severity { Severity::Warning }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::DistributionIssue }
    fn related_classic_rules(&self) -> Vec<String> { vec![] }

    fn detail_template(&self) -> String {
        "数据从 DN 汇聚到 CN 后执行 Sort（{sort.actual_rows} 行），\
         所有排序在单节点完成，无法利用 DN 并行。".to_string()
    }

    fn suggestion_template(&self) -> String {
        "1. 若 ORDER BY 列与分布键一致，改为 DN 本地排序 + CN 合并\n\
         2. 或调整分布键为排序列\n\
         3. 若允许，使用 LIMIT + 子查询减少排序数据量".to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        // 当前节点必须是 Sort
        if root.node_type != NodeType::Sort && root.node_type != NodeType::VectorSort {
            return None;
        }

        let actual = root.actual.as_ref()?;
        if actual.rows < self.threshold {
            return None;
        }

        // 检查祖先路径中是否存在 Streaming(GATHER)
        let gather_node = ancestors.iter().find(|&a| {
            matches!(
                &a.node_type,
                NodeType::Streaming(StreamingType::Gather)
                    | NodeType::VectorStreaming(StreamingType::Gather)
            )
        })?;

        let mut captures = HashMap::new();
        captures.insert("sort".to_string(), root);
        captures.insert("gather".to_string(), gather_node);

        Some(MatchResult {
            pattern_id: self.id().to_string(),
            captures,
            ancestors: ancestors.to_vec(),
            matched_node: root,
        })
    }
}
```

**测试夹具** (`tests/fixtures/anti/anti007_gather_sort.txt`):

```
                          Query Plan
 ----------------------------------------------------------
  Streaming(type: GATHER dop: 1/4)
    cost=0.00..500000.00 rows=500000 width=100
    Actual time=100.000..5000.000 rows=500000 loops=1
    Sort
      Sort Key: created_at DESC
      cost=0.00..500000.00 rows=500000 width=100
      Actual time=100.000..4800.000 rows=500000 loops=1
      Sort Method: external merge  Disk: 50000kB
      ->  Seq Scan on orders
            cost=0.00..100000.00 rows=500000 width=100
            Actual time=0.010..500.000 rows=500000 loops=1
```

---

## 4. 集成修改点

### 4.1 analyzer/report.rs — Finding 增量扩展

**修改文件**: `crates/ogexplain-core/src/analyzer/report.rs`

```rust
// 在 Finding struct 末尾新增：
#[serde(skip_serializing_if = "Option::is_none")]
pub evidence: Option<crate::analyzer::pattern::types::Evidence>,
```

**同时修改** `rules/mod.rs` 中的 `make_finding()` 函数：

```rust
fn make_finding(
    rule: &dyn DiagnosticRule,
    detail: String,
    node: &PlanNode,
    suggestion: Option<String>,
) -> Finding {
    Finding {
        // ... 现有字段不变 ...
        evidence: None,  // 经典规则无证据
    }
}
```

### 4.2 analyzer/mod.rs — 注册 pattern 模块

**修改文件**: `crates/ogexplain-core/src/analyzer/mod.rs`

```rust
pub mod config;
pub mod context;
pub mod pattern;      // 新增
pub mod report;
pub mod rules;
```

### 4.3 analyzer/rules/mod.rs — all_rules() 注册

**修改文件**: `crates/ogexplain-core/src/analyzer/rules/mod.rs`

在 `all_rules()` 函数末尾新增：

```rust
Box::new(super::pattern::AntiPatternRule::new()),
```

### 4.4 disabled_rules 支持

现有 `DiagnosticEngine::analyze()` 的 `findings.retain()` 已经按 `rule_id` 过滤。
需要确认反模式的 `rule_id` 是否在 disabled 列表中：

```rust
// engine.rs 的 check_global 实现中
fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
    // disabled_rules 过滤在 DiagnosticEngine::analyze() 中统一处理
    // 但 AntiPatternRule 注册为 rule_id = "ANTI"
    // 各反模式的 rule_id 是 "ANTI-005" 等
    // 需要确保 disabled_rules 匹配逻辑正确
    self.engine.match_plan(plan)
        .into_iter()
        .filter(|r| /* 检查 disabled */)
        .map(|r| self.render_finding(r))
        .collect()
}
```

**注意**：当前 `DiagnosticEngine::analyze()` 的过滤逻辑是：
```rust
findings.retain(|f| !self.config.disabled_rules.contains(&f.rule_id));
```

由于 `AntiPatternRule::check_global()` 返回的 Finding 的 `rule_id` 是 `"ANTI-005"` 而非 `"ANTI"`，
用户可以通过 `disabled_rules: ["ANTI-005"]` 单独禁用某个反模式，或 `"ANTI"` 禁用全部（但 `"ANTI"` 不会匹配 `"ANTI-005"`）。

**建议**：在 `AntiPatternRule::check_global()` 内部自行过滤：

```rust
fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
    // 注意：此处无法访问 config.disabled_rules
    // 需要将 disabled_rules 传入或在 DiagnosticEngine 层面处理
    // 当前最简方案：由 DiagnosticEngine::analyze() 的 retain 处理
    // 用户禁用 "ANTI-005" 即可精确控制
    self.engine.match_plan(plan)
        .into_iter()
        .map(|result| self.render_finding(result))
        .collect()
}
```

---

## 5. AntiPatternDef trait 的模板方法

为了让 `AntiPatternRule` 能统一渲染，`AntiPatternDef` 需要提供模板：

```rust
pub trait AntiPatternDef: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> Severity;
    fn category(&self) -> DiagnosticCategory;
    fn related_classic_rules(&self) -> Vec<String>;

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>>;

    /// 详情模板：支持 {capture.property} 占位符
    fn detail_template(&self) -> String;

    /// 建议模板：支持 {capture.property} 占位符
    fn suggestion_template(&self) -> String;
}
```

---

## 6. 测试策略

### 6.1 单元测试

每个反模式一个测试文件，位于 `crates/ogexplain-core/src/analyzer/pattern/patterns/` 内部：

```rust
// patterns/materialize_cascade.rs 底部
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_node(nt: NodeType, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: 50.0,
                rows: 5000.0,
                loops: 1.0,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children,
            indent_level: 0,
            line_number: 1,
        }
    }

    #[test]
    fn test_match_materialize_cascade() {
        let inner = make_node(NodeType::NestedLoop, vec![]);
        let mat2 = make_node(NodeType::Materialize, vec![inner]);
        let mat1 = make_node(NodeType::Materialize, vec![mat2]);

        let pattern = MaterializeCascade;
        let result = pattern.try_match(&mat1, &[]);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.pattern_id, "ANTI-005");
        assert!(r.captures.contains_key("mat1"));
        assert!(r.captures.contains_key("mat2"));
        assert!(r.captures.contains_key("nl"));
    }

    #[test]
    fn test_no_match_single_materialize() {
        let nl = make_node(NodeType::NestedLoop, vec![]);
        let mat = make_node(NodeType::Materialize, vec![nl]);

        let pattern = MaterializeCascade;
        let result = pattern.try_match(&mat, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_no_match_materialize_sort_nestedloop() {
        let nl = make_node(NodeType::NestedLoop, vec![]);
        let sort = make_node(NodeType::Sort, vec![nl]);
        let mat = make_node(NodeType::Materialize, vec![sort]);

        let pattern = MaterializeCascade;
        let result = pattern.try_match(&mat, &[]);
        assert!(result.is_none());
    }
}
```

### 6.2 集成测试

在 `tests/analyzer_tests.rs` 中新增反模式测试：

```rust
#[test]
fn test_anti005_materialize_cascade() {
    let input = include_str!("fixtures/anti/anti005_materialize_cascade.txt");
    let plan = ogexplain_core::parse(input).expect("parse");
    let report = ogexplain_core::analyze(&plan);

    let anti005 = report.findings.iter()
        .find(|f| f.rule_id == "ANTI-005")
        .expect("Should detect materialize cascade");

    assert_eq!(anti005.severity, Severity::Warning);
    assert!(anti005.detail.contains("Materialize"));
    assert!(anti005.detail.contains("NestedLoop"));
    assert!(anti005.evidence.is_some());
    assert!(anti005.evidence.as_ref().unwrap().matched_nodes.len() >= 3);
}
```

### 6.3 回归测试

```bash
# 确保现有 317 测试全部通过
cargo test --workspace

# 确保 clippy 零警告
cargo clippy --workspace

# 确保 fmt 无变更
cargo fmt --all -- --check
```

---

## 7. 实施任务清单

### Task 1: 创建 pattern 模块骨架

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/pattern/mod.rs`
- Create: `crates/ogexplain-core/src/analyzer/pattern/types.rs`
- Create: `crates/ogexplain-core/src/analyzer/pattern/engine.rs`
- Create: `crates/ogexplain-core/src/analyzer/pattern/predicates.rs`
- Create: `crates/ogexplain-core/src/analyzer/pattern/templates.rs`
- Create: `crates/ogexplain-core/src/analyzer/pattern/patterns/mod.rs`
- Modify: `crates/ogexplain-core/src/analyzer/mod.rs` — 添加 `pub mod pattern;`

**Step 1**: 创建 `types.rs`，定义 `MatchResult`, `Evidence`, `MatchedNode`。

**Step 2**: 创建 `engine.rs`，定义 `AntiPatternDef` trait + `PatternEngine` DFS 遍历。

**Step 3**: 创建 `predicates.rs`，定义 `FieldPath` 枚举 + `FieldAccessor`。

**Step 4**: 创建 `templates.rs`，实现 `render_template()`。

**Step 5**: 创建 `patterns/mod.rs`（空模块注册）。

**Step 6**: 创建 `mod.rs`，定义 `AntiPatternRule`（此时 patterns 为空）。

**Step 7**: 在 `analyzer/mod.rs` 中注册 `pub mod pattern;`。

**Step 8**: 编译验证 `cargo build -p ogexplain-core`。

### Task 2: Finding 增量扩展 + 现有测试回归

**Files:**
- Modify: `crates/ogexplain-core/src/analyzer/report.rs` — Finding 新增 `evidence` 字段
- Modify: `crates/ogexplain-core/src/analyzer/rules/mod.rs` — `make_finding()` 新增 `evidence: None`

**Step 1**: 在 `report.rs` 的 `Finding` struct 末尾添加 `evidence` 字段。

**Step 2**: 在 `rules/mod.rs` 的 `make_finding()` 中添加 `evidence: None`。

**Step 3**: 运行 `cargo test --workspace`，确认 317 测试全部通过。

**Step 4**: 运行 `cargo clippy --workspace`，确认零警告。

### Task 3: ANTI-005 多层物化堆砌

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/pattern/patterns/materialize_cascade.rs`
- Modify: `crates/ogexplain-core/src/analyzer/pattern/patterns/mod.rs` — 注册
- Modify: `crates/ogexplain-core/src/analyzer/pattern/mod.rs` — 在 `AntiPatternRule::new()` 中注册
- Create: `tests/fixtures/anti/anti005_materialize_cascade.txt`

**Step 1**: 编写 `materialize_cascade.rs` 的单元测试（`test_match`, `test_no_match_single`, `test_no_match_wrong_inner`）。

**Step 2**: 运行测试确认失败。

**Step 3**: 实现 `MaterializeCascade` struct + `AntiPatternDef` impl。

**Step 4**: 运行单元测试确认通过。

**Step 5**: 创建测试夹具文件。

**Step 6**: 在 `patterns/mod.rs` 和 `AntiPatternRule::new()` 中注册。

**Step 7**: 在 `rules/mod.rs` 的 `all_rules()` 中注册 `AntiPatternRule`。

**Step 8**: 编写集成测试并运行。

**Step 9**: 运行全量测试 `cargo test --workspace`。

### Task 4: ANTI-003 索引回表放大

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/pattern/patterns/index_scan_amplify.rs`
- Create: `tests/fixtures/anti/anti003_index_amplify.txt`

**Step 1**: 编写单元测试。

**Step 2**: 实现 `IndexScanAmplify`。

**Step 3**: 创建夹具文件 + 集成测试。

**Step 4**: 全量测试回归。

### Task 5: ANTI-007 CN 端大排序

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/pattern/patterns/gather_then_sort.rs`
- Create: `tests/fixtures/anti/anti007_gather_sort.txt`

**Step 1**: 编写单元测试。

**Step 2**: 实现 `GatherThenSort`。

**Step 3**: 创建夹具文件 + 集成测试。

**Step 4**: 全量测试回归。

### Task 6: 最终验证与清理

**Step 1**: `cargo test --workspace` — 全部测试通过。

**Step 2**: `cargo clippy --workspace` — 零警告。

**Step 3**: `cargo fmt --all -- --check` — 格式正确。

**Step 4**: 手动验证：`cargo run -p ogexplain-cli -- analyze tests/fixtures/anti/anti005_materialize_cascade.txt`。

**Step 5**: 手动验证 JSON 输出包含 `evidence` 字段。

---

## 8. 未来扩展路径（不在本方案范围内）

- **YAML/DSL 声明式反模式定义**：供非 Rust 用户编写模式
- **去重逻辑**：基于 `related_classic_rules` 的抑制策略
- **更多反模式**：ANTI-004（倾斜）、ANTI-006（CStore 全列）、ANTI-009（Sonic 回退）等
- **TUI 证据链可视化**：在树视图中高亮匹配路径
- **置信度评分**：根据匹配偏差动态调整
- **DiagnosticConfig 扩展**：反模式阈值参数化
