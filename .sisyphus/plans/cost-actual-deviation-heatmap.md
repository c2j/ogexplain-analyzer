# Cost-Actual Deviation Heatmap 实施方案

> **版本**: v1.0
> **日期**: 2026-06-13
> **状态**: Draft
> **前置分析**: 基于对 PlanNode 模型、EST-001/004 规则、SummaryRow 偏差计算、CLI/TUI 输出管线的完整代码审查

---

## 0. 设计决策摘要

基于可行性分析的关键结论：

| 决策点 | 方案提议 | 修订结论 | 理由 |
|--------|----------|----------|------|
| 数据存储 | 修改 PlanNode 添加 `deviation`/`subtree_qerror` | **独立 `PlanHeatmap` 结构体** | PlanNode 在分析阶段只有 `&PlanNode`（不可变），且 `ogexplain-core` 约束无 UI 依赖 |
| 节点标识 | `node.node_id` | `node.line_number` | `node_id` 不存在；`line_number: usize` 已存在且 EXPLAIN 行号天然唯一 |
| 父节点访问 | `node.parent` | DFS 传参 `ancestors: &[&PlanNode]` | PlanNode 无父指针，同反模式方案的祖先栈模式 |
| time_ratio | `actual_time / total_cost` | **Phase 1 不实现** | `total_cost` 是无量纲代价单位，`actual_time_ms` 是毫秒，直接相除无意义 |
| io_ratio | `shared_read / (rows * width / 8192)` | **Phase 1 不实现** | 估算 IO 代价无法从 EXPLAIN 中提取 |
| mem_ratio | `peak_memory / work_mem` | **Phase 1 不实现** | `work_mem` 是 session 级参数，不出现在 EXPLAIN 输出中 |
| 偏差度量 | Q-Error + ratio 混用 | **Q-Error 为主指标，direction 单独标记** | Q-Error 是 VLDB 文献标准（对称、>= 1.0），direction 保留语义信息 |
| 与 EST-001 关系 | 替换 EST-001 | **独立模块，并行运行** | 避免回归风险；热力图提供全局视图，EST-001 提供单点报警 |
| Q-Error 阈值 | 无明确说明 | 采用与 `estimation_skew_factor` 对齐的分级 | 现有默认 100x → 对应 Extreme；新增 2/5/10/50 四级 |

### Phase 1 范围（本方案）

仅实现 **row_qerror**（行数偏差）维度的全树热力图：

- ✅ `NodeDeviation`：每个有 EXPLAIN ANALYZE 统计的节点的 Q-Error + 方向 + 分级
- ✅ `subtree_geo_qerror`：子树几何平均 Q-Error（后序遍历）
- ✅ `path_cumulative_qerror`：从根到叶的乘积累积 Q-Error（前序遍历）
- ✅ `critical_path`：最大偏差路径（line_number 序列）
- ✅ `hotspots`：按 Q-Error 降序排列的热点节点
- ✅ JSON 输出扩展
- ✅ CLI `--format=heatmap` ANSI 彩色输出

### 暂缓内容

| 内容 | 暂缓原因 |
|------|----------|
| `time_ratio` | 跨单位不可比 |
| `io_ratio` | 估算 IO 不可获取 |
| `mem_ratio` | `work_mem` 不可获取 |
| TUI 热力图色条 | 需要更多 UI 设计 |
| EST-001/004 升级 | 等热力图稳定后再考虑 |

---

## 1. 新增文件结构

```
crates/ogexplain-core/src/
├── analyzer/
│   ├── heatmap/                   # 新增模块
│   │   ├── mod.rs                 # 模块入口 + 公共 API
│   │   ├── types.rs               # NodeDeviation, HeatmapEntry, PlanHeatmap, 枚举
│   │   └── engine.rs              # HeatmapEngine: 后序+前序遍历, 关键路径, 热点排序
│   ├── config.rs                  # 不修改
│   ├── context.rs                 # 不修改
│   ├── report.rs                  # 不修改
│   └── rules/                     # 不修改

crates/ogexplain-cli/src/
├── lib.rs                         # 修改: 添加 "heatmap" 输出格式 + output_heatmap() 函数
```

---

## 2. 核心数据结构

### 2.1 types.rs

```rust
//! Cost-Actual deviation heatmap types.
//!
//! All types are independent of PlanNode — computed from immutable plan references.

use serde::Serialize;

/// 偏差严重程度分级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum DeviationSeverity {
    /// Q-Error < 2x
    Negligible,
    /// 2x ~ 5x
    Mild,
    /// 5x ~ 10x
    Moderate,
    /// 10x ~ 50x
    Severe,
    /// > 50x
    Extreme,
}

impl DeviationSeverity {
    pub fn from_qerror(q: f64) -> Self {
        if q < 2.0 {
            Self::Negligible
        } else if q < 5.0 {
            Self::Mild
        } else if q < 10.0 {
            Self::Moderate
        } else if q < 50.0 {
            Self::Severe
        } else {
            Self::Extreme
        }
    }

    /// CLI/TUI 渲染用图标
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Extreme => "🔴",
            Self::Severe => "🟠",
            Self::Moderate => "🟡",
            Self::Mild => "🟢",
            Self::Negligible => "⚪",
        }
    }
}

/// 偏差方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DeviationDirection {
    /// 实际 > 估算（优化器低估）→ 更危险，可能导致选错连接顺序
    Underestimate,
    /// 实际 < 估算（优化器高估）
    Overestimate,
    /// 基本吻合（Q-Error 接近 1.0）
    Accurate,
}

/// 单个节点的偏差指标
#[derive(Debug, Clone, Serialize)]
pub struct NodeDeviation {
    /// EXPLAIN 中的行号（唯一标识）
    pub line_number: usize,
    /// 节点类型名
    pub node_type: String,
    /// 关联的表名（scan 节点）
    pub relation: Option<String>,

    // --- 原始数据 ---
    /// 估算行数
    pub estimated_rows: f64,
    /// 实际行数
    pub actual_rows: f64,

    // --- 计算指标 ---
    /// 行数 Q-Error: max(A,E)/min(A,E)，>= 1.0
    /// VLDB 文献标准度量，对称，不区分方向
    pub row_qerror: f64,
    /// 行数比率: A/E
    /// 正值表示低估，< 1.0 表示高估
    pub row_ratio: f64,
    /// 偏差方向
    pub direction: DeviationDirection,
    /// 严重程度分级
    pub severity: DeviationSeverity,
}

/// 路径感知的热力图条目
#[derive(Debug, Clone, Serialize)]
pub struct HeatmapEntry {
    pub deviation: NodeDeviation,
    /// 子树几何平均 Q-Error（后序遍历计算）
    pub subtree_geo_qerror: f64,
    /// 从根到该节点的乘积累积 Q-Error（前序遍历计算）
    pub path_cumulative_qerror: f64,
    /// 是否位于最大偏差路径上
    pub on_critical_path: bool,
}

/// 热力图全局摘要
#[derive(Debug, Clone, Serialize)]
pub struct HeatmapSummary {
    /// 全树最大 Q-Error
    pub max_qerror: f64,
    /// 最大 Q-Error 节点的行号
    pub max_qerror_line: usize,
    /// 偏差 >= Severe 的节点数
    pub severe_count: usize,
    /// 有 EXPLAIN ANALYZE 统计的节点总数
    pub total_nodes: usize,
    /// 关键路径长度（节点数）
    pub critical_path_length: usize,
    /// 有偏差的节点数（Q-Error >= 2.0）
    pub deviated_count: usize,
}

/// 全计划热力图
#[derive(Debug, Clone, Serialize)]
pub struct PlanHeatmap {
    /// 所有有统计数据的节点的热力图条目
    pub entries: Vec<HeatmapEntry>,
    /// 最大偏差路径（line_number 序列，从根到叶）
    pub critical_path: Vec<usize>,
    /// 热点节点（line_number 序列，按 Q-Error 降序）
    pub hotspots: Vec<usize>,
    /// 全局摘要
    pub summary: HeatmapSummary,
}
```

### 2.2 engine.rs

```rust
//! Heatmap computation engine.
//!
//! Two-phase algorithm:
//!   Phase 1 (post-order): Compute per-node deviation + subtree geometric mean Q-Error
//!   Phase 2 (pre-order):  Compute path cumulative Q-Error from root
//!
//! All computation is read-only on PlanNode — no mutation required.

use std::collections::HashMap;

use super::types::*;
use crate::model::{ExplainPlan, PlanNode};

pub struct HeatmapEngine;

impl HeatmapEngine {
    /// 主入口：从计划生成热力图。
    /// 返回 None 当计划无 EXPLAIN ANALYZE 数据时。
    pub fn generate(plan: &ExplainPlan) -> Option<PlanHeatmap> {
        // Phase 1: 后序遍历 — 计算 per-node deviation + subtree_geo_qerror
        let mut node_data: HashMap<usize, (HeatmapEntry, f64)> = HashMap::new();
        let _ = Self::post_order(&plan.root, &mut node_data);

        if node_data.is_empty() {
            return None;
        }

        // Phase 2: 前序遍历 — 计算 path_cumulative_qerror
        let mut entries: Vec<HeatmapEntry> = node_data.into_values().map(|(e, _)| e).collect();
        let mut line_to_idx: HashMap<usize, usize> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            line_to_idx.insert(e.deviation.line_number, i);
        }
        Self::pre_order(&plan.root, 1.0, &line_to_idx, &mut entries);

        // Phase 3: 确定关键路径
        let critical_path = Self::find_critical_path(&plan.root, &entries, &line_to_idx);

        // Phase 4: 标记关键路径 + 排序热点
        let critical_set: std::collections::HashSet<usize> = critical_path.iter().copied().collect();
        for entry in &mut entries {
            entry.on_critical_path = critical_set.contains(&entry.deviation.line_number);
        }

        let mut hotspots: Vec<usize> = entries
            .iter()
            .filter(|e| e.deviation.severity >= DeviationSeverity::Moderate)
            .map(|e| e.deviation.line_number)
            .collect();
        hotspots.sort_by(|a, b| {
            let ea = entries.get(*line_to_idx.get(a).unwrap_or(&0)).map(|e| e.deviation.row_qerror).unwrap_or(1.0);
            let eb = entries.get(*line_to_idx.get(b).unwrap_or(&0)).map(|e| e.deviation.row_qerror).unwrap_or(1.0);
            eb.partial_cmp(&ea).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Phase 5: 摘要
        let max_entry = entries.iter().max_by(|a, b| {
            a.deviation.row_qerror.partial_cmp(&b.deviation.row_qerror).unwrap()
        });
        let summary = HeatmapSummary {
            max_qerror: max_entry.map(|e| e.deviation.row_qerror).unwrap_or(1.0),
            max_qerror_line: max_entry.map(|e| e.deviation.line_number).unwrap_or(0),
            severe_count: entries.iter().filter(|e| e.deviation.severity >= DeviationSeverity::Severe).count(),
            total_nodes: entries.len(),
            critical_path_length: critical_path.len(),
            deviated_count: entries.iter().filter(|e| e.deviation.row_qerror >= 2.0).count(),
        };

        Some(PlanHeatmap {
            entries,
            critical_path,
            hotspots,
            summary,
        })
    }

    // ---- Phase 1: 后序遍历 ----

    /// 返回 (self_qerror, subtree_geo_qerror)
    fn post_order(
        node: &PlanNode,
        data: &mut HashMap<usize, (HeatmapEntry, f64)>,
    ) -> (f64, f64) {
        let mut child_geo_qerrors = Vec::new();
        for child in &node.children {
            let (_, child_geo) = Self::post_order(child, data);
            child_geo_qerrors.push(child_geo);
        }

        let self_qerror = Self::qerror(node);
        let all_q: Vec<f64> = std::iter::once(self_qerror).chain(child_geo_qerrors).collect();
        let subtree_geo = Self::geometric_mean(&all_q);

        // 仅记录有实际统计数据的节点
        if let Some(deviation) = Self::make_deviation(node) {
            data.insert(node.line_number, (
                HeatmapEntry {
                    deviation,
                    subtree_geo_qerror: subtree_geo,
                    path_cumulative_qerror: 1.0, // Phase 2 填充
                    on_critical_path: false,
                },
                subtree_geo,
            ));
        }

        (self_qerror, subtree_geo)
    }

    // ---- Phase 2: 前序遍历 ----

    fn pre_order(
        node: &PlanNode,
        parent_cumulative: f64,
        line_to_idx: &HashMap<usize, usize>,
        entries: &mut [HeatmapEntry],
    ) {
        let self_q = Self::qerror(node);
        let cumulative = parent_cumulative * self_q;

        if let Some(&idx) = line_to_idx.get(&node.line_number) {
            entries[idx].path_cumulative_qerror = cumulative;
        }

        for child in &node.children {
            Self::pre_order(child, cumulative, line_to_idx, entries);
        }
    }

    // ---- Phase 3: 关键路径 ----

    /// 找到 path_cumulative_qerror 最大的叶到根路径。
    /// 由于无父指针，从根开始 DFS，在每个分支选择 subtree_geo_qerror 最大的子节点。
    fn find_critical_path(
        root: &PlanNode,
        entries: &[HeatmapEntry],
        line_to_idx: &HashMap<usize, usize>,
    ) -> Vec<usize> {
        let mut path = Vec::new();
        Self::greedy_critical(root, entries, line_to_idx, &mut path);
        path
    }

    fn greedy_critical(
        node: &PlanNode,
        entries: &[HeatmapEntry],
        line_to_idx: &HashMap<usize, usize>,
        path: &mut Vec<usize>,
    ) {
        path.push(node.line_number);

        if node.children.is_empty() {
            return; // 叶子节点，路径完成
        }

        // 选择 subtree_geo_qerror 最大的子节点
        let best_child = node.children.iter().max_by(|a, b| {
            let qa = line_to_idx.get(&a.line_number)
                .and_then(|&i| entries.get(i))
                .map(|e| e.subtree_geo_qerror)
                .unwrap_or(1.0);
            let qb = line_to_idx.get(&b.line_number)
                .and_then(|&i| entries.get(i))
                .map(|e| e.subtree_geo_qerror)
                .unwrap_or(1.0);
            qa.partial_cmp(&qb).unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(child) = best_child {
            Self::greedy_critical(child, entries, line_to_idx, path);
        }
    }

    // ---- 辅助方法 ----

    fn qerror(node: &PlanNode) -> f64 {
        match (&node.estimated, &node.actual) {
            (Some(est), Some(act)) if est.plan_rows > 0.0 && act.rows > 0.0 => {
                let a = act.rows;
                let e = est.plan_rows;
                a.max(e) / a.min(e)
            }
            _ => 1.0,
        }
    }

    fn make_deviation(node: &PlanNode) -> Option<NodeDeviation> {
        let est = node.estimated.as_ref()?;
        let act = node.actual.as_ref()?;
        if est.plan_rows <= 0.0 || act.rows <= 0.0 {
            return None;
        }

        let a = act.rows;
        let e = est.plan_rows;
        let row_qerror = a.max(e) / a.min(e);
        let row_ratio = a / e;

        let direction = if row_ratio > 1.5 {
            DeviationDirection::Underestimate
        } else if row_ratio < 0.67 {
            DeviationDirection::Overestimate
        } else {
            DeviationDirection::Accurate
        };

        let severity = DeviationSeverity::from_qerror(row_qerror);

        Some(NodeDeviation {
            line_number: node.line_number,
            node_type: node.node_type.to_string(),
            relation: node.relation.clone(),
            estimated_rows: e,
            actual_rows: a,
            row_qerror,
            row_ratio,
            direction,
            severity,
        })
    }

    fn geometric_mean(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 1.0;
        }
        let product: f64 = values.iter().product();
        if product <= 0.0 {
            return 1.0;
        }
        product.powf(1.0 / values.len() as f64)
    }
}
```

### 2.3 mod.rs

```rust
//! Cost-Actual Deviation Heatmap module.
//!
//! Provides full-tree deviation analysis for EXPLAIN ANALYZE plans.
//! Independent of the rule engine — computes quantitative metrics, not boolean findings.

pub mod engine;
pub mod types;

pub use engine::HeatmapEngine;
pub use types::*;
```

---

## 3. 集成修改点

### 3.1 analyzer/mod.rs — 注册 heatmap 模块

**修改文件**: `crates/ogexplain-core/src/analyzer/mod.rs`

```rust
pub mod config;
pub mod context;
pub mod heatmap;       // 新增
pub mod report;
pub mod rules;
```

### 3.2 lib.rs — 公共 API 扩展

**修改文件**: `crates/ogexplain-core/src/lib.rs`

新增公共函数：

```rust
/// Generate deviation heatmap for the plan.
/// Returns None if the plan has no EXPLAIN ANALYZE data.
pub fn heatmap(plan: &model::ExplainPlan) -> Option<analyzer::heatmap::PlanHeatmap> {
    analyzer::HeatmapEngine::generate(plan)
}
```

### 3.3 CLI — 添加 heatmap 输出格式

**修改文件**: `crates/ogexplain-cli/src/lib.rs`

在 `analyze_and_output()` 的 match 中添加：

```rust
match output {
    "json" => output_json(...)?,
    "heatmap" => output_heatmap(plan, &filtered_findings)?,  // 新增
    _ => output_text(...)?,
}
```

新增 `output_heatmap()` 函数（ANSI 彩色树）：

```rust
fn output_heatmap(
    plan: &ogexplain_core::model::ExplainPlan,
    findings: &[&ogexplain_core::analyzer::report::Finding],
) -> Result<()> {
    let heatmap = match ogexplain_core::heatmap(plan) {
        Some(h) => h,
        None => {
            println!("{}", "No EXPLAIN ANALYZE data found. Heatmap requires EXPLAIN ANALYZE output.".yellow());
            return Ok(());
        }
    };

    // 摘要头部
    println!("{}", "═".repeat(60).bright_blue());
    println!("{}", "  Cost-Actual Deviation Heatmap".bold());
    println!("{}", "═".repeat(60).bright_blue());
    println!();

    println!("  {} Max Q-Error: {:.1}x at {} (line {})",
        heatmap.summary.max_qerror_line,
        heatmap.summary.max_qerror,
        heatmap.entries.iter()
            .find(|e| e.deviation.line_number == heatmap.summary.max_qerror_line)
            .map(|e| e.deviation.node_type.as_str())
            .unwrap_or("?"),
        heatmap.summary.max_qerror_line,
    );
    println!("  📍 Critical Path: {} nodes", heatmap.summary.critical_path_length);
    println!("  ⚠  Severe deviations: {}/{} nodes",
        heatmap.summary.severe_count,
        heatmap.summary.total_nodes,
    );
    println!();

    // 构建 line_number → HeatmapEntry 索引
    let entry_map: HashMap<usize, &HeatmapEntry> = heatmap.entries.iter()
        .map(|e| (e.deviation.line_number, e))
        .collect();
    let critical_set: HashSet<usize> = heatmap.critical_path.iter().copied().collect();

    // 递归打印树
    print_heatmap_node(&plan.root, &entry_map, &critical_set, 0, true, "");

    Ok(())
}

fn print_heatmap_node(
    node: &PlanNode,
    entry_map: &HashMap<usize, &HeatmapEntry>,
    critical_set: &HashSet<usize>,
    depth: usize,
    is_last: bool,
    prefix: &str,
) {
    use colored::*;

    let branch = if is_last { "└── " } else { "├── " };

    if let Some(entry) = entry_map.get(&node.line_number) {
        let d = &entry.deviation;
        let icon = d.severity.icon();

        let dir_str = match d.direction {
            DeviationDirection::Underestimate => "↓低估",
            DeviationDirection::Overestimate => "↑高估",
            DeviationDirection::Accurate => "",
        };

        let node_str = format!(
            "{}{}[{}] {} (est={:.0} actual={:.0} Q={:.1}x{})",
            prefix, branch, icon,
            d.node_type,
            d.estimated_rows, d.actual_rows,
            d.row_qerror, dir_str,
        );

        let colored = match d.severity {
            DeviationSeverity::Extreme => node_str.red().bold(),
            DeviationSeverity::Severe => node_str.red(),
            DeviationSeverity::Moderate => node_str.yellow(),
            DeviationSeverity::Mild => node_str.green(),
            DeviationSeverity::Negligible => node_str.white(),
        };
        println!("{}", colored);

        // 关键路径详情
        if critical_set.contains(&node.line_number) && d.row_qerror >= 2.0 {
            let detail_prefix = format!("{}{}    ", prefix, if is_last { " " } else { "│" });
            let detail = format!(
                "{}📊 Path-Q: {:.1}x | Subtree-Q: {:.1}x",
                detail_prefix,
                entry.path_cumulative_qerror,
                entry.subtree_geo_qerror,
            );
            println!("{}", detail.dimmed());
        }
    } else {
        // 无统计数据的节点（纯 EXPLAIN，无 ANALYZE）
        let node_str = format!(
            "{}{}{} {}",
            prefix, branch, "⚪",
            node.node_type,
        );
        println!("{}", node_str);
    }

    let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
    for (i, child) in node.children.iter().enumerate() {
        let last = i == node.children.len() - 1;
        print_heatmap_node(child, entry_map, critical_set, depth + 1, last, &child_prefix);
    }
}
```

### 3.4 JSON 输出扩展

**修改文件**: `crates/ogexplain-cli/src/lib.rs`

在 `output_json()` 的 `JsonOutput` struct 中添加：

```rust
struct JsonOutput<'a> {
    plan: &'a ExplainPlan,
    complexity: ...,
    gauss_complexity: ...,
    findings: ...,
    suggestions: ...,
    stats: ...,
    summary: ...,
    heatmap: Option<ogexplain_core::analyzer::heatmap::PlanHeatmap>,  // 新增
}
```

在构建时计算：

```rust
let heatmap = ogexplain_core::heatmap(plan);
let output = JsonOutput { ..., heatmap };
```

---

## 4. 测试策略

### 4.1 单元测试

```rust
// heatmap/engine.rs 底部

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_node_with_stats(
        line: usize,
        est_rows: f64,
        actual_rows: f64,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type: NodeType::SeqScan,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0,
                total_cost: 100.0,
                plan_rows: est_rows,
                plan_width: 100,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: 10.0,
                rows: actual_rows,
                loops: 1.0,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children,
            indent_level: 0,
            line_number: line,
        }
    }

    #[test]
    fn test_qerror_accurate() {
        let node = make_node_with_stats(1, 100.0, 100.0, vec![]);
        assert!((HeatmapEngine::qerror(&node) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_qerror_underestimate() {
        let node = make_node_with_stats(1, 100.0, 10000.0, vec![]);
        assert!((HeatmapEngine::qerror(&node) - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_qerror_overestimate() {
        let node = make_node_with_stats(1, 10000.0, 100.0, vec![]);
        assert!((HeatmapEngine::qerror(&node) - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_qerror_symmetry() {
        let a = make_node_with_stats(1, 10.0, 1000.0, vec![]);
        let b = make_node_with_stats(2, 1000.0, 10.0, vec![]);
        assert!((HeatmapEngine::qerror(&a) - HeatmapEngine::qerror(&b)).abs() < 0.001);
    }

    #[test]
    fn test_no_stats_returns_none() {
        let node = PlanNode {
            node_type: NodeType::SeqScan,
            relation: None, join_type: None,
            estimated: None, actual: None,
            properties: vec![], structured_props: None,
            buffers: None, children: vec![],
            indent_level: 0, line_number: 1,
        };
        let plan = ExplainPlan { root: node, summary: None };
        assert!(HeatmapEngine::generate(&plan).is_none());
    }

    #[test]
    fn test_critical_path_picks_worst_branch() {
        // Root (accurate) → [Child A (10x), Child B (100x)]
        // Critical path should go through Child B
        let child_a = make_node_with_stats(2, 100.0, 1000.0, vec![]);
        let child_b = make_node_with_stats(3, 100.0, 10000.0, vec![]);
        let root = make_node_with_stats(1, 100.0, 100.0, vec![child_a, child_b]);
        let plan = ExplainPlan { root, summary: None };

        let heatmap = HeatmapEngine::generate(&plan).unwrap();
        assert!(heatmap.critical_path.contains(&3)); // Child B 应在关键路径上
    }

    #[test]
    fn test_cumulative_path_multiplication() {
        // Root(10x) → Child(10x): cumulative = 1.0 * 10 * 10 = 100
        let child = make_node_with_stats(2, 100.0, 1000.0, vec![]);
        let root = make_node_with_stats(1, 100.0, 1000.0, vec![child]);
        let plan = ExplainPlan { root, summary: None };

        let heatmap = HeatmapEngine::generate(&plan).unwrap();
        let leaf_entry = heatmap.entries.iter()
            .find(|e| e.deviation.line_number == 2)
            .unwrap();
        assert!(leaf_entry.path_cumulative_qerror > 90.0);
    }

    #[test]
    fn test_geometric_mean() {
        let values = vec![10.0, 10.0, 10.0];
        let mean = HeatmapEngine::geometric_mean(&values);
        assert!((mean - 10.0).abs() < 0.001);

        // 1000, 1, 1 → geo_mean ≈ 10 (vs arithmetic mean ≈ 334)
        let skewed = vec![1000.0, 1.0, 1.0];
        let mean = HeatmapEngine::geometric_mean(&skewed);
        assert!(mean < 20.0); // 几何平均不被极端值主导
    }

    #[test]
    fn test_severity_classification() {
        assert_eq!(DeviationSeverity::from_qerror(1.5), DeviationSeverity::Negligible);
        assert_eq!(DeviationSeverity::from_qerror(3.0), DeviationSeverity::Mild);
        assert_eq!(DeviationSeverity::from_qerror(7.0), DeviationSeverity::Moderate);
        assert_eq!(DeviationSeverity::from_qerror(25.0), DeviationSeverity::Severe);
        assert_eq!(DeviationSeverity::from_qerror(100.0), DeviationSeverity::Extreme);
    }
}
```

### 4.2 测试夹具

```
tests/fixtures/heatmap/
├── h01_uniform.txt              # 所有节点偏差 < 2x，应全绿/白
├── h02_single_hotspot.txt       # 单个 IndexScan 偏差 50x
├── h03_cumulative_path.txt      # 3 层嵌套，每层偏差 10x，累积 1000x
├── h04_no_analyze.txt           # 纯 EXPLAIN（无 ANALYZE），应返回 None
```

### 4.3 集成测试

```rust
// 在 tests/analyzer_tests.rs 中新增

#[test]
fn test_heatmap_uniform() {
    let input = include_str!("fixtures/heatmap/h01_uniform.txt");
    let plan = ogexplain_core::parse(input).expect("parse");
    let heatmap = ogexplain_core::heatmap(&plan);
    // 纯 EXPLAIN 无 ANALYZE 数据时返回 None
    // 或如果 fixture 有 ANALYZE，所有节点 Q-Error 应 < 2.0
}

#[test]
fn test_heatmap_cumulative_path() {
    let input = include_str!("fixtures/heatmap/h03_cumulative_path.txt");
    let plan = ogexplain_core::parse(input).expect("parse");
    let heatmap = ogexplain_core::heatmap(&plan).expect("should have heatmap");

    // 关键路径至少 3 个节点
    assert!(heatmap.critical_path.len() >= 3);

    // 最大路径累积偏差应显著
    let max_cum = heatmap.entries.iter()
        .map(|e| e.path_cumulative_qerror)
        .fold(1.0_f64, f64::max);
    assert!(max_cum > 100.0);
}
```

---

## 5. 实施任务清单

### Task 1: 创建 heatmap 模块骨架 + types.rs

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/heatmap/mod.rs`
- Create: `crates/ogexplain-core/src/analyzer/heatmap/types.rs`
- Modify: `crates/ogexplain-core/src/analyzer/mod.rs` — 添加 `pub mod heatmap;`

**Step 1**: 创建 `types.rs`，定义所有类型。

**Step 2**: 创建 `mod.rs`，导出公共类型。

**Step 3**: 在 `analyzer/mod.rs` 中注册。

**Step 4**: 编译验证 `cargo build -p ogexplain-core`。

### Task 2: 实现 HeatmapEngine + 单元测试

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/heatmap/engine.rs`

**Step 1**: 编写单元测试（`test_qerror_*`, `test_no_stats`, `test_critical_path`, `test_cumulative_path`, `test_geometric_mean`, `test_severity_classification`）。

**Step 2**: 运行测试确认失败。

**Step 3**: 实现 `HeatmapEngine`（`post_order`, `pre_order`, `find_critical_path`, 辅助方法）。

**Step 4**: 运行单元测试确认通过。

**Step 5**: 全量测试 `cargo test --workspace`。

### Task 3: 公共 API + JSON 输出

**Files:**
- Modify: `crates/ogexplain-core/src/lib.rs` — 添加 `heatmap()` 公共函数
- Modify: `crates/ogexplain-cli/src/lib.rs` — `output_json()` 添加 heatmap 字段

**Step 1**: 在 `lib.rs` 中添加 `pub fn heatmap()`。

**Step 2**: 在 CLI `output_json()` 的 `JsonOutput` struct 中添加 `heatmap` 字段。

**Step 3**: 运行 `cargo test --workspace` + `cargo clippy --workspace`。

### Task 4: CLI `--format=heatmap` ANSI 输出

**Files:**
- Modify: `crates/ogexplain-cli/src/lib.rs` — 添加 `output_heatmap()` + `print_heatmap_node()` + match arm

**Step 1**: 实现 `output_heatmap()`（摘要头部 + 树遍历）。

**Step 2**: 在 `analyze_and_output()` 中添加 `"heatmap"` match arm。

**Step 3**: 手动测试：`cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt -o heatmap`。

**Step 4**: 手动测试 JSON：`cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt -o json | jq '.heatmap'`。

### Task 5: 测试夹具 + 集成测试 + 最终验证

**Files:**
- Create: `tests/fixtures/heatmap/h01_uniform.txt`
- Create: `tests/fixtures/heatmap/h02_single_hotspot.txt`
- Create: `tests/fixtures/heatmap/h03_cumulative_path.txt`
- Create: `tests/fixtures/heatmap/h04_no_analyze.txt`

**Step 1**: 创建测试夹具文件。

**Step 2**: 编写集成测试。

**Step 3**: `cargo test --workspace` — 全部通过。

**Step 4**: `cargo clippy --workspace` — 零警告。

**Step 5**: `cargo fmt --all -- --check` — 格式正确。

**Step 6**: 手动验证各输出模式。

---

## 6. 与反模式子树匹配的联动

两个方案完全互补，且共享相同的基础设施模式：

| 维度 | 热力图 | 反模式 |
|------|--------|--------|
| 回答的问题 | 「哪里偏差大」 | 「偏差大的节点是什么模式」 |
| 输出类型 | 定量指标（Q-Error 数值） | 定性判断（匹配/不匹配） |
| 核心算法 | 后序+前序遍历 + 几何平均 | DFS 子树匹配 |
| 共享模式 | 祖先栈（`&[&PlanNode]`）、`line_number` 标识 | 同左 |

**未来联动路径**：热力图的 `hotspots` 可作为反模式匹配的优先搜索目标——先检查 Q-Error 最大的路径上的子树结构，而非盲目遍历全树。

---

## 7. 未来扩展路径（不在本方案范围内）

- **`time_ratio`**：需建立 `total_cost → actual_time` 的校准系数（Phase 2 需要统计回归）
- **`io_ratio`**：需从 OG 源码理解 `total_cost` 中 IO 代价的权重
- **`mem_ratio`**：需在 `DiagnosticConfig` 中添加 `work_mem_kb` 参数
- **TUI 热力图色条**：在树面板左侧渲染偏差色条
- **EST-001 升级**：将热力图数据注入 EST-001 的 detail，增加路径上下文
- **与反模式联动**：`hotspots` → `PatternEngine` 优先搜索
- **TOML 配置**：`Q-Error` 阈值、`critical_path` 阈值参数化
