# Resource Waterfall 实施方案

> **版本**: v1.0
> **日期**: 2026-06-13
> **状态**: Draft
> **前置分析**: 基于对 PlanNode 模型、parser tree_builder、BufferStats 解析状态、NodeType 分类、memory/sort/join 规则数据访问模式的完整代码审查

---

## 0. 设计决策摘要

基于可行性分析的关键结论：

| 决策点 | 提案原方案 | 修订结论 | 理由 |
|--------|-----------|----------|------|
| 四维同时实现 | CPU/Memory/IO/Network 全部 | **Phase 1 仅 CPU + Memory** | IO: `buffers` 永远为 None（解析器未实现）；Network: 无逐节点真实数据 |
| 资源画像查找 | `phf::Map<NodeType, ResourceProfile>` | **`match` 表达式** | `NodeType::Streaming(StreamingType)` 含变体数据，不能 derive `Hash`；104 变体 match 编译期保证完备性 |
| 父节点访问 | `node.parent` | **DFS 后序遍历** | PlanNode 无父指针；后序遍历天然支持 "叶子先处理" 的自底向上归约 |
| 节点标识 | `node.node_id` | `node.line_number` | `node_id` 不存在；`line_number` 已存在且 EXPLAIN 行号天然唯一 |
| 次要资源权重 | `secondary_weight = 0.3` | **Phase 1 不做权重** | 权重缺乏 OG 负载标定依据；Hash Join 的 CPU/Memory 占比因数据量而异，不能静态绑定 |
| Network 估算 | `rows × plan_width × 1.2` | **Phase 1 不实现** | `plan_width` 是优化器估计；`× 1.2` 无依据；Streaming 节点属性中的网络流量未解析 |
| 与现有规则关系 | 重新检测 spill/peak memory | **复用规则输出作为维度注解** | MEM-001/004、JOIN-002 已检测 spill；瀑布图仅标注 "此维度异常"，不重复检测 |
| 渲染方式 | 提案未指定 | **Phase 1: CLI text + JSON** | TUI 无图表组件，需单独设计 |

### Phase 1 范围（本方案）

仅实现 **CPU + Memory** 双维度资源瀑布：

- ✅ `ResourceProfile`：NodeType → 资源维度分类（match 表达式，非 HashMap）
- ✅ `NodeResourceMetrics`：逐节点 CPU 时间 + Memory 消耗的绝对值提取
- ✅ `WaterfallEntry`：每个节点的资源分解（CPU time, Memory, 子树归约）
- ✅ `PlanWaterfall`：全计划瀑布（含全局摘要、瓶颈节点、关键路径）
- ✅ DFS 后序遍历：自底向上归约子树资源
- ✅ CLI `--format=waterfall` 水平条形图 ANSI 输出
- ✅ JSON 输出扩展
- ✅ 与已有规则输出的注解联动（spill 节点在 Memory 维度标注 ⚠️）

### 暂缓内容

| 内容 | 暂缓原因 |
|------|----------|
| IO 维度（Buffer Stats） | 需先实现 `tree_builder.rs` 的 Buffers 解析（约 100-150 行） |
| Network 维度 | `PlanSummary.total_network_kb` 仅摘要级；逐节点估算公式不可靠 |
| 多维归一化（归一分数 0-100） | 权重缺乏标定依据，需 OG 负载测试数据 |
| TUI 瀑布图渲染 | 当前 TUI 无图表组件，需新增 Canvas 渲染模块 |
| 自适应权重 | 需要数据量感知（小数据 Nested Loop 是 CPU 瓶颈，大数据是 IO 瓶颈） |

---

## 1. 新增文件结构

```
crates/ogexplain-core/src/
├── analyzer/
│   ├── waterfall/                   # 新增模块
│   │   ├── mod.rs                   # 模块入口 + 公共 API
│   │   ├── types.rs                 # ResourceType, ResourceProfile, NodeResourceMetrics, WaterfallEntry, PlanWaterfall
│   │   ├── profile.rs              # NodeType → ResourceProfile 的 match 映射
│   │   └── engine.rs               # WaterfallEngine: DFS 后序遍历, 子树归约, 瓶颈检测
│   ├── config.rs                    # 不修改
│   ├── context.rs                   # 不修改
│   ├── report.rs                    # 不修改
│   └── rules/                       # 不修改

crates/ogexplain-cli/src/
├── lib.rs                           # 修改: 添加 "waterfall" 输出格式 + output_waterfall() 函数
```

---

## 2. 核心数据结构

### 2.1 types.rs

```rust
//! Resource waterfall types.
//!
//! All types are independent of PlanNode — computed from immutable plan references.

use serde::Serialize;

/// 资源维度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ResourceDimension {
    /// CPU 时间（actual.total_time_ms × loops）
    CpuTime,
    /// 内存使用（peak_memory_kb, sort/hash spill）
    Memory,
    /// IO 操作（Buffer reads/writes）— Phase 2
    #[serde(skip)]
    Io,
    /// 网络传输（Streaming 节点数据量）— Phase 2
    #[serde(skip)]
    Network,
}

/// 节点级资源消耗指标
#[derive(Debug, Clone, Serialize)]
pub struct NodeResourceMetrics {
    /// EXPLAIN 中的行号（唯一标识）
    pub line_number: usize,
    /// 节点类型名
    pub node_type: String,
    /// 关联的表名（scan 节点）
    pub relation: Option<String>,

    // --- CPU 维度 ---
    /// 实际 CPU 时间（ms）= total_time_ms × loops
    /// None 表示无 EXPLAIN ANALYZE 数据
    pub cpu_time_ms: Option<f64>,

    // --- Memory 维度 ---
    /// 峰值内存使用（KB），来自 structured_props.peak_memory_kb
    pub peak_memory_kb: Option<f64>,
    /// 排序溢写到磁盘（KB），来自 structured_props.sort_disk
    pub sort_spill_kb: Option<f64>,
    /// Hash 溢出批次（> 1 表示溢出），来自 structured_props.hash_batches
    pub hash_spill_batches: Option<i64>,
    /// Hash 内存使用（原始字符串，如 "48kB"）
    pub hash_memory_usage: Option<String>,
    /// 是否检测到内存溢出（sort 或 hash）
    pub has_memory_spill: bool,

    // --- 子树归约 ---
    /// 子树总 CPU 时间（ms）（所有后代 + 自身的 cpu_time_ms 之和）
    pub subtree_cpu_time_ms: f64,
    /// 子树总峰值内存（KB）（所有后代 + 自身的 peak_memory_kb 最大值）
    pub subtree_peak_memory_kb: f64,
}

/// 节点在瀑布图中的条目
#[derive(Debug, Clone, Serialize)]
pub struct WaterfallEntry {
    pub metrics: NodeResourceMetrics,
    /// 该节点涉及哪些资源维度
    pub dimensions: Vec<ResourceDimension>,
    /// CPU 占全计划总时间的百分比
    pub cpu_percent: f64,
    /// Memory 占全计划最大峰值内存的百分比
    pub memory_percent: f64,
    /// 是否为瓶颈节点（CPU 或 Memory 中任一维度占比 > 阈值）
    pub is_bottleneck: bool,
    /// 瓶颈维度
    pub bottleneck_dimensions: Vec<ResourceDimension>,
    /// 深度（根节点 = 0）
    pub depth: usize,
}

/// 瓶颈节点摘要
#[derive(Debug, Clone, Serialize)]
pub struct BottleneckSummary {
    /// CPU 瓶颈节点（按 CPU 时间降序，最多 5 个）
    pub cpu_bottlenecks: Vec<usize>,  // line_number
    /// Memory 瓶颈节点（按峰值内存降序，最多 5 个）
    pub memory_bottlenecks: Vec<usize>,  // line_number
    /// 总 CPU 时间（ms）
    pub total_cpu_time_ms: f64,
    /// 最大单节点峰值内存（KB）
    pub max_peak_memory_kb: f64,
    /// 总溢出节点数
    pub spill_node_count: usize,
}

/// 全计划资源瀑布
#[derive(Debug, Clone, Serialize)]
pub struct PlanWaterfall {
    /// 所有节点的瀑布条目（DFS 后序遍历顺序）
    pub entries: Vec<WaterfallEntry>,
    /// 瓶颈摘要
    pub bottlenecks: BottleneckSummary,
    /// 节点总数
    pub total_nodes: usize,
    /// 有 EXPLAIN ANALYZE 数据的节点数
    pub nodes_with_stats: usize,
}
```

### 2.2 profile.rs

```rust
//! NodeType → resource profile mapping.
//!
//! Uses match expression (not HashMap) because NodeType contains variant data
//! (Streaming(StreamingType), Unknown(String)) which cannot derive Hash.

use crate::model::node_type::NodeType;
use super::types::ResourceDimension;

/// 节点类型的资源特征画像
#[derive(Debug, Clone)]
pub struct ResourceProfile {
    /// 该类型主要消耗的资源维度
    pub primary_dimensions: &'static [ResourceDimension],
    /// 人类可读的资源特征描述
    pub description: &'static str,
}

impl ResourceProfile {
    /// 获取 NodeType 对应的资源画像。
    /// 使用 match 表达式确保编译期完备性（覆盖所有 104+ 变体）。
    pub fn for_node(node_type: &NodeType) -> &'static ResourceProfile {
        match node_type {
            // ---- Scan 节点：IO-bound + CPU (filter evaluation) ----
            NodeType::SeqScan
            | NodeType::PartitionedSeqScan => &SCAN_IO_BOUND,
            NodeType::IndexScan
            | NodeType::IndexOnlyScan => &INDEX_IO_CPU,
            NodeType::BitmapIndexScan => &BITMAP_CPU,
            NodeType::BitmapHeapScan => &BITMAP_HEAP_IO_MEM,
            NodeType::SampleScan => &SCAN_IO_BOUND,
            NodeType::TidScan
            | NodeType::TidRangeScan => &SCAN_IO_BOUND,
            NodeType::SubqueryScan
            | NodeType::CteScan => &SUBQUERY_CPU_MEM,
            NodeType::FunctionScan => &FUNCTION_CPU,
            NodeType::ValuesScan => &VALUES_CPU_MEM,
            NodeType::WorkTableScan => &WORKTABLE_MEM,
            NodeType::ForeignScan
            | NodeType::PartitionedForeignScan => &FOREIGN_NETWORK_IO,
            NodeType::CStoreScan
            | NodeType::PartitionedCStoreScan => &CSTORE_IO_CPU,
            NodeType::TsStoreScan => &CSTORE_IO_CPU,
            NodeType::AnnIndexScan => &INDEX_IO_CPU,
            NodeType::CStoreIndexScan
            | NodeType::CStoreIndexCtidScan
            | NodeType::CStoreIndexHeapScan => &INDEX_IO_CPU,
            NodeType::ImCStoreScan => &CSTORE_IO_CPU,
            NodeType::VectorSubqueryScan => &SUBQUERY_CPU_MEM,
            NodeType::VectorForeignScan => &FOREIGN_NETWORK_IO,
            NodeType::DataNodeScan => &DATANODE_NETWORK,

            // ---- Join 节点：CPU + Memory-intensive ----
            NodeType::NestedLoop
            | NodeType::VectorNestLoop => &NESTED_LOOP_CPU_IO,
            NodeType::HashJoin
            | NodeType::VectorHashJoin
            | NodeType::VectorSonicHashJoin => &HASH_JOIN_CPU_MEM,
            NodeType::MergeJoin
            | NodeType::VectorMergeJoin => &MERGE_JOIN_CPU_MEM,
            NodeType::VectorAsofJoin => &MERGE_JOIN_CPU_MEM,

            // ---- Aggregate 节点：Memory + CPU ----
            NodeType::Aggregate
            | NodeType::GroupAggregate => &GROUP_AGG_CPU_MEM,
            NodeType::HashAggregate
            | NodeType::DummyHashAggregate
            | NodeType::VectorHashAggregate
            | NodeType::VectorSonicHashAggregate => &HASH_AGG_CPU_MEM,
            NodeType::VectorAggregate
            | NodeType::VectorSortAggregate => &HASH_AGG_CPU_MEM,
            NodeType::Group
            | NodeType::VectorGroup => &GROUP_AGG_CPU_MEM,

            // ---- Sort 节点：Memory + IO (spill) ----
            NodeType::Sort
            | NodeType::GroupSort
            | NodeType::VectorSort => &SORT_MEM_IO,

            // ---- DML 节点：IO (write path) ----
            NodeType::Insert
            | NodeType::Update
            | NodeType::Delete
            | NodeType::Merge
            | NodeType::Replace => &DML_IO_CPU,
            NodeType::VectorInsert
            | NodeType::VectorUpdate
            | NodeType::VectorDelete
            | NodeType::VectorMerge => &DML_IO_CPU,

            // ---- SetOp 节点：CPU + Memory ----
            NodeType::SetOp
            | NodeType::HashSetOp
            | NodeType::VectorSetOp
            | NodeType::VectorHashSetOp => &SETOP_CPU_MEM,
            NodeType::Append
            | NodeType::VectorAppend => &APPEND_CPU,
            NodeType::MergeAppend => &MERGE_APPEND_IO_MEM,
            NodeType::RecursiveUnion => &RECURSIVE_MEM_CPU,
            NodeType::BitmapAnd
            | NodeType::BitmapOr
            | NodeType::CStoreIndexAnd
            | NodeType::CStoreIndexOr => &BITMAP_OP_CPU,

            // ---- Streaming 节点：Network-bound ----
            NodeType::Streaming(_)
            | NodeType::VectorStreaming(_) => &STREAMING_NETWORK,

            // ---- 其他节点：Mixed ----
            NodeType::Result
            | NodeType::VectorResult => &RESULT_CPU,
            NodeType::ProjectSet => &PROJECTSET_CPU_MEM,
            NodeType::Hash => &HASH_MEM,
            NodeType::Materialize
            | NodeType::VectorMaterialize => &MATERIALIZE_MEM,
            NodeType::Limit
            | NodeType::VectorLimit => &RESULT_CPU,
            NodeType::LockRows => &LOCKROWS_CPU,
            NodeType::PartitionIterator
            | NodeType::VectorPartitionIterator => &PARTITION_ITER_IO_CPU,
            NodeType::RowAdapter
            | NodeType::VectorAdapter => &ADAPTER_CPU_MEM,
            NodeType::StartWithOp => &RECURSIVE_MEM_CPU,
            NodeType::RemoteSubplanScan => &DATANODE_NETWORK,
            NodeType::ModifyTable => &DML_IO_CPU,
            NodeType::Gather
            | NodeType::GatherMerge => &GATHER_NETWORK,

            // ---- Unknown ----
            NodeType::Unknown(_) => &UNKNOWN,
        }
    }
}

// ---- 静态画像实例 ----

static SCAN_IO_BOUND: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Sequential scan — IO-bound, filter evaluation costs CPU",
};

static INDEX_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Index scan — random IO + index cache memory",
};

static BITMAP_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Bitmap index scan — CPU for bitmap construction",
};

static BITMAP_HEAP_IO_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Bitmap heap scan — IO via bitmap + memory for bitmap",
};

static SUBQUERY_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Subquery scan — CPU for evaluation + memory for result cache",
};

static FUNCTION_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Function scan — CPU for function execution",
};

static VALUES_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Values scan — CPU + memory for in-memory rows",
};

static WORKTABLE_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory, ResourceDimension::CpuTime],
    description: "Work table scan — memory for recursive query state",
};

static FOREIGN_NETWORK_IO: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Foreign scan — network for external data access",
};

static CSTORE_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "CStore scan — columnar IO + CPU for decompression",
};

static DATANODE_NETWORK: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Data node scan — network for remote data fetch",
};

static NESTED_LOOP_CPU_IO: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Nested loop — O(n×m) CPU, inner index IO",
};

static HASH_JOIN_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Hash join — memory for hash table, CPU for probing, may spill",
};

static MERGE_JOIN_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Merge join — CPU for sorted merge, memory for sort state",
};

static GROUP_AGG_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Group aggregate — CPU for grouping, memory for sort state",
};

static HASH_AGG_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Hash aggregate — memory for hash table, CPU for hashing, may spill",
};

static SORT_MEM_IO: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory, ResourceDimension::CpuTime],
    description: "Sort — memory-intensive, may spill to disk (IO)",
};

static DML_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "DML — write path IO + WAL logging",
};

static SETOP_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Set operation — CPU for comparison + memory for dedup",
};

static APPEND_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Append — CPU for concatenation",
};

static MERGE_APPEND_IO_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Merge append — IO + memory for sorted merge",
};

static RECURSIVE_MEM_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory, ResourceDimension::CpuTime],
    description: "Recursive union — memory for CTE state + CPU for recursion",
};

static BITMAP_OP_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Bitmap operation — CPU for bitmap AND/OR",
};

static STREAMING_NETWORK: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Streaming — network data motion between datanodes",
};

static RESULT_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Result/Limit — CPU for projection/filtering",
};

static PROJECTSET_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "ProjectSet — CPU for SRF evaluation + memory for results",
};

static HASH_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory],
    description: "Hash — memory for hash table construction",
};

static MATERIALIZE_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::Memory],
    description: "Materialize — memory for materialized result cache",
};

static LOCKROWS_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Lock rows — CPU for row-level locking",
};

static PARTITION_ITER_IO_CPU: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Partition iterator — IO for partition access + CPU for pruning",
};

static ADAPTER_CPU_MEM: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime, ResourceDimension::Memory],
    description: "Row/Vector adapter — CPU for format conversion + memory buffer",
};

static GATHER_NETWORK: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Gather — network for result collection",
};

static UNKNOWN: ResourceProfile = ResourceProfile {
    primary_dimensions: &[ResourceDimension::CpuTime],
    description: "Unknown node type — resource profile uncertain",
};
```

### 2.3 engine.rs

```rust
//! Resource waterfall computation engine.
//!
//! Algorithm: DFS post-order traversal (bottom-up reduction).
//! - Phase 1: Post-order → compute per-node metrics + subtree reduction
//! - Phase 2: Compute percentages relative to plan totals
//! - Phase 3: Identify bottlenecks
//!
//! All computation is read-only on PlanNode — no mutation required.

use std::collections::HashMap;

use super::profile::ResourceProfile;
use super::types::*;
use crate::model::{ExplainPlan, PlanNode};

/// 瓶颈检测阈值：CPU 占比超过此值的节点视为瓶颈
const CPU_BOTTLENECK_THRESHOLD: f64 = 0.20; // 20%
/// Memory 占比超过此值的节点视为瓶颈
const MEMORY_BOTTLENECK_THRESHOLD: f64 = 0.25; // 25%

pub struct WaterfallEngine;

impl WaterfallEngine {
    /// 主入口：从计划生成资源瀑布。
    /// 返回 None 当计划完全无 EXPLAIN ANALYZE 数据时。
    pub fn generate(plan: &ExplainPlan) -> Option<PlanWaterfall> {
        // Phase 1: DFS 后序遍历 — 收集所有节点指标
        let mut entries: Vec<(WaterfallEntry, usize)> = Vec::new(); // (entry, depth)
        let (total_cpu, max_mem) = Self::post_order(&plan.root, 0, &mut entries);

        if entries.is_empty() {
            return None;
        }

        // Phase 2: 计算百分比
        let nodes_with_stats = entries.iter()
            .filter(|(e, _)| e.metrics.cpu_time_ms.is_some() || e.metrics.peak_memory_kb.is_some())
            .count();

        for (entry, _) in &mut entries {
            if total_cpu > 0.0 {
                let cpu = entry.metrics.cpu_time_ms.unwrap_or(0.0);
                entry.cpu_percent = cpu / total_cpu * 100.0;
            }
            if max_mem > 0.0 {
                let mem = entry.metrics.peak_memory_kb.unwrap_or(0.0);
                entry.memory_percent = mem / max_mem * 100.0;
            }
        }

        // Phase 3: 标记瓶颈
        for (entry, _) in &mut entries {
            let mut bottleneck_dims = Vec::new();
            if entry.cpu_percent >= CPU_BOTTLENECK_THRESHOLD * 100.0 {
                bottleneck_dims.push(ResourceDimension::CpuTime);
            }
            if entry.memory_percent >= MEMORY_BOTTLENECK_THRESHOLD * 100.0 {
                bottleneck_dims.push(ResourceDimension::Memory);
            }
            // 溢出节点也标记为 Memory 瓶颈
            if entry.metrics.has_memory_spill {
                if !bottleneck_dims.contains(&ResourceDimension::Memory) {
                    bottleneck_dims.push(ResourceDimension::Memory);
                }
            }
            entry.is_bottleneck = !bottleneck_dims.is_empty();
            entry.bottleneck_dimensions = bottleneck_dims;
        }

        // Phase 4: 瓶颈摘要
        let mut cpu_sorted: Vec<_> = entries.iter()
            .filter(|(e, _)| e.metrics.cpu_time_ms.is_some())
            .collect();
        cpu_sorted.sort_by(|a, b| {
            b.0.metrics.cpu_time_ms.unwrap_or(0.0)
                .partial_cmp(&a.0.metrics.cpu_time_ms.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut mem_sorted: Vec<_> = entries.iter()
            .filter(|(e, _)| e.metrics.peak_memory_kb.is_some())
            .collect();
        mem_sorted.sort_by(|a, b| {
            b.0.metrics.peak_memory_kb.unwrap_or(0.0)
                .partial_cmp(&a.0.metrics.peak_memory_kb.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let spill_count = entries.iter()
            .filter(|(e, _)| e.metrics.has_memory_spill)
            .count();

        let bottlenecks = BottleneckSummary {
            cpu_bottlenecks: cpu_sorted.iter().take(5).map(|(e, _)| e.metrics.line_number).collect(),
            memory_bottlenecks: mem_sorted.iter().take(5).map(|(e, _)| e.metrics.line_number).collect(),
            total_cpu_time_ms: total_cpu,
            max_peak_memory_kb: max_mem,
            spill_node_count: spill_count,
        };

        let final_entries: Vec<WaterfallEntry> = entries.into_iter().map(|(e, _)| e).collect();

        Some(PlanWaterfall {
            entries: final_entries,
            bottlenecks,
            total_nodes: final_entries.len(),
            nodes_with_stats,
        })
    }

    // ---- Phase 1: DFS 后序遍历 ----

    /// 返回 (subtree_total_cpu, subtree_max_memory)
    fn post_order(
        node: &PlanNode,
        depth: usize,
        entries: &mut Vec<(WaterfallEntry, usize)>,
    ) -> (f64, f64) {
        let mut child_total_cpu = 0.0_f64;
        let mut child_max_mem = 0.0_f64;

        for child in &node.children {
            let (c_cpu, c_mem) = Self::post_order(child, depth + 1, entries);
            child_total_cpu += c_cpu;
            child_max_mem = child_max_mem.max(c_mem);
        }

        // 提取当前节点资源指标
        let cpu_time_ms = Self::extract_cpu_time(node);
        let peak_memory_kb = Self::extract_peak_memory(node);
        let sort_spill_kb = Self::extract_sort_spill(node);
        let hash_spill_batches = Self::extract_hash_batches(node);
        let hash_memory_usage = Self::extract_hash_memory(node);

        let has_memory_spill = sort_spill_kb.is_some()
            || hash_spill_batches.map_or(false, |b| b > 1);

        let self_cpu = cpu_time_ms.unwrap_or(0.0);
        let self_mem = peak_memory_kb.unwrap_or(0.0);

        let subtree_cpu = child_total_cpu + self_cpu;
        let subtree_mem = child_max_mem.max(self_mem);

        // 获取资源画像
        let profile = ResourceProfile::for_node(&node.node_type);

        let metrics = NodeResourceMetrics {
            line_number: node.line_number,
            node_type: node.node_type.to_string(),
            relation: node.relation.clone(),
            cpu_time_ms,
            peak_memory_kb,
            sort_spill_kb,
            hash_spill_batches,
            hash_memory_usage,
            has_memory_spill,
            subtree_cpu_time_ms: subtree_cpu,
            subtree_peak_memory_kb: subtree_mem,
        };

        let entry = WaterfallEntry {
            metrics,
            dimensions: profile.primary_dimensions.to_vec(),
            cpu_percent: 0.0,       // Phase 2 填充
            memory_percent: 0.0,    // Phase 2 填充
            is_bottleneck: false,   // Phase 3 填充
            bottleneck_dimensions: vec![], // Phase 3 填充
            depth,
        };

        entries.push((entry, depth));

        (subtree_cpu, subtree_mem)
    }

    // ---- 指标提取辅助方法 ----

    /// CPU 时间 = actual.total_time_ms × loops
    fn extract_cpu_time(node: &PlanNode) -> Option<f64> {
        let actual = node.actual.as_ref()?;
        if !actual.executed {
            return None;
        }
        Some(actual.total_time_ms * actual.loops)
    }

    /// 峰值内存（KB）
    fn extract_peak_memory(node: &PlanNode) -> Option<f64> {
        node.structured_props.as_ref()?.peak_memory_kb
    }

    /// Sort 溢写到磁盘大小（KB），从 structured_props.sort_disk 解析
    fn extract_sort_spill(node: &PlanNode) -> Option<f64> {
        let sp = node.structured_props.as_ref()?;
        let disk_str = sp.sort_disk.as_ref()?;
        // "5840kB" → 5840.0
        let num_str = disk_str.trim_end_matches("kB").trim();
        num_str.parse().ok()
    }

    /// Hash 溢出批次（Batches > 1 表示溢出）
    fn extract_hash_batches(node: &PlanNode) -> Option<i64> {
        node.structured_props.as_ref()?.hash_batches
    }

    /// Hash 内存使用（原始字符串）
    fn extract_hash_memory(node: &PlanNode) -> Option<String> {
        node.structured_props.as_ref()?.hash_memory_usage.clone()
    }
}
```

### 2.4 mod.rs

```rust
//! Resource Waterfall module.
//!
//! Provides per-node resource consumption analysis for EXPLAIN ANALYZE plans.
//! Phase 1 covers CPU time and Memory; IO and Network are deferred.

pub mod engine;
pub mod profile;
pub mod types;

pub use engine::WaterfallEngine;
pub use profile::ResourceProfile;
pub use types::*;
```

---

## 3. 集成修改点

### 3.1 analyzer/mod.rs — 注册 waterfall 模块

**修改文件**: `crates/ogexplain-core/src/analyzer/mod.rs`

```rust
pub mod config;
pub mod context;
pub mod heatmap;       // 已有（heatmap 方案）
pub mod report;
pub mod rules;
pub mod waterfall;     // 新增
```

### 3.2 lib.rs — 公共 API 扩展

**修改文件**: `crates/ogexplain-core/src/lib.rs`

新增公共函数：

```rust
/// Generate resource waterfall for the plan.
/// Returns None if the plan has no EXPLAIN ANALYZE data.
pub fn waterfall(plan: &model::ExplainPlan) -> Option<analyzer::waterfall::PlanWaterfall> {
    analyzer::WaterfallEngine::generate(plan)
}
```

### 3.3 CLI — 添加 waterfall 输出格式

**修改文件**: `crates/ogexplain-cli/src/lib.rs`

在 `analyze_and_output()` 的 match 中添加：

```rust
match output {
    "json" => output_json(...)?,
    "heatmap" => output_heatmap(plan, &filtered_findings)?,
    "waterfall" => output_waterfall(plan)?,  // 新增
    _ => output_text(...)?,
}
```

新增 `output_waterfall()` 函数（ANSI 水平条形图）：

```rust
fn output_waterfall(plan: &ogexplain_core::model::ExplainPlan) -> Result<()> {
    use colored::*;

    let waterfall = match ogexplain_core::waterfall(plan) {
        Some(w) => w,
        None => {
            println!("{}", "No EXPLAIN ANALYZE data found. Waterfall requires EXPLAIN ANALYZE output.".yellow());
            return Ok(());
        }
    };

    let bar_width = 40; // 条形图宽度（字符）

    // 摘要头部
    println!("{}", "═".repeat(60).bright_blue());
    println!("{}", "  Resource Waterfall".bold());
    println!("{}", "═".repeat(60).bright_blue());
    println!();

    let bn = &waterfall.bottlenecks;
    println!("  ⏱  Total CPU Time: {:.2} ms", bn.total_cpu_time_ms);
    println!("  🧠 Max Peak Memory: {:.0} KB", bn.max_peak_memory_kb);
    println!("  💾 Spill Nodes: {}", bn.spill_node_count);
    println!("  📊 Nodes: {} total, {} with stats",
        waterfall.total_nodes, waterfall.nodes_with_stats);
    println!();

    // CPU 瓶颈 Top-5
    if !bn.cpu_bottlenecks.is_empty() {
        println!("{}", "  Top CPU Consumers:".bold());
        let entry_map: HashMap<usize, &WaterfallEntry> = waterfall.entries.iter()
            .map(|e| (e.metrics.line_number, e))
            .collect();

        for line in &bn.cpu_bottlenecks {
            if let Some(entry) = entry_map.get(line) {
                let cpu = entry.metrics.cpu_time_ms.unwrap_or(0.0);
                let pct = entry.cpu_percent;
                let bar_len = ((pct / 100.0) * bar_width as f64).round() as usize;
                let bar_len = bar_len.max(1).min(bar_width);

                let bar = "█".repeat(bar_len);
                let label = format!(
                    "  {} {:<30} {:>8.2}ms ({:>5.1}%)",
                    if entry.is_bottleneck { "🔴" } else { "  " },
                    format!("{}:{}", entry.metrics.node_type, line),
                    cpu, pct,
                );
                println!("{} {}", label, bar.bright_red());
            }
        }
        println!();
    }

    // Memory 瓶颈 Top-5
    if !bn.memory_bottlenecks.is_empty() {
        println!("{}", "  Top Memory Consumers:".bold());
        let entry_map: HashMap<usize, &WaterfallEntry> = waterfall.entries.iter()
            .map(|e| (e.metrics.line_number, e))
            .collect();

        for line in &bn.memory_bottlenecks {
            if let Some(entry) = entry_map.get(line) {
                let mem = entry.metrics.peak_memory_kb.unwrap_or(0.0);
                let pct = entry.memory_percent;
                let bar_len = ((pct / 100.0) * bar_width as f64).round() as usize;
                let bar_len = bar_len.max(1).min(bar_width);

                let spill_marker = if entry.metrics.has_memory_spill { " ⚠️SPILL" } else { "" };
                let bar = "█".repeat(bar_len);

                let label = format!(
                    "  {} {:<30} {:>8.0}KB ({:>5.1}%){}",
                    if entry.is_bottleneck { "🔴" } else { "  " },
                    format!("{}:{}", entry.metrics.node_type, line),
                    mem, pct, spill_marker,
                );
                println!("{} {}", label, bar.bright_yellow());
            }
        }
        println!();
    }

    // 全节点瀑布（DFS 后序）
    println!("{}", "  Full Waterfall (bottom-up order):".bold());
    println!("{}", "  ┌──────────────────────────────────────────────────┐");

    for entry in &waterfall.entries {
        let indent = "  ".repeat(entry.depth.min(10));
        let cpu_bar_len = if waterfall.bottlenecks.total_cpu_time_ms > 0.0 {
            let pct = entry.cpu_percent / 100.0;
            (pct * 20.0).round() as usize
        } else { 0 };
        let mem_bar_len = if waterfall.bottlenecks.max_peak_memory_kb > 0.0 {
            let pct = entry.memory_percent / 100.0;
            (pct * 20.0).round() as usize
        } else { 0 };

        let cpu_bar = "▓".repeat(cpu_bar_len.max(1).min(20));
        let mem_bar = "▓".repeat(mem_bar_len.max(1).min(20));

        let bottleneck_marker = if entry.is_bottleneck { "🔴" } else { "  " };
        let spill_marker = if entry.metrics.has_memory_spill { " 💾" } else { "" };

        println!(
            "  {}{} {} CPU:[{:<20}] MEM:[{:<20}]{}",
            bottleneck_marker,
            indent,
            format!("{:<20}", format!("{}:{}", entry.metrics.node_type, entry.metrics.line_number)),
            cpu_bar.bright_red(),
            mem_bar.bright_yellow(),
            spill_marker,
        );
    }

    println!("{}", "  └──────────────────────────────────────────────────┘");

    Ok(())
}
```

### 3.4 JSON 输出扩展

**修改文件**: `crates/ogexplain-cli/src/lib.rs`

在 `output_json()` 的 `JsonOutput` struct 中添加：

```rust
struct JsonOutput<'a> {
    plan: &'a ExplainPlan,
    // ...existing fields...
    heatmap: Option<ogexplain_core::analyzer::heatmap::PlanHeatmap>,
    waterfall: Option<ogexplain_core::analyzer::waterfall::PlanWaterfall>,  // 新增
}
```

---

## 4. 测试策略

### 4.1 单元测试

```rust
// waterfall/engine.rs 底部

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_node(
        line: usize,
        node_type: NodeType,
        cpu_time_ms: Option<f64>,
        peak_mem_kb: Option<f64>,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0, total_cost: 100.0,
                plan_rows: 1000.0, plan_width: 32,
                pred_time: None, pred_rows: None, distinct: None,
            }),
            actual: cpu_time_ms.map(|t| ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: t,
                rows: 1000.0,
                loops: 1.0,
                executed: true,
            }),
            properties: peak_mem_kb.map(|m| vec![NodeProperty {
                label: "Peak Memory".to_string(),
                value: format!("{}kB", m),
            }]).unwrap_or_default(),
            structured_props: None, // 将由 extract 自动处理（测试中需手动构建）
            buffers: None,
            children,
            indent_level: 0,
            line_number: line,
        }
    }

    fn make_node_full(
        line: usize,
        node_type: NodeType,
        cpu_time_ms: f64,
        peak_mem_kb: f64,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0, total_cost: 100.0,
                plan_rows: 1000.0, plan_width: 32,
                pred_time: None, pred_rows: None, distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: cpu_time_ms,
                rows: 1000.0,
                loops: 1.0,
                executed: true,
            }),
            properties: vec![NodeProperty {
                label: "Peak Memory".to_string(),
                value: format!("{}kB", peak_mem_kb),
            }],
            structured_props: Some(NodeProperties {
                peak_memory_kb: Some(peak_mem_kb),
                ..Default::default()
            }),
            buffers: None,
            children,
            indent_level: 0,
            line_number: line,
        }
    }

    #[test]
    fn test_cpu_time_extraction() {
        let node = make_node_full(1, NodeType::SeqScan, 50.0, 0.0, vec![]);
        assert_eq!(WaterfallEngine::extract_cpu_time(&node), Some(50.0));
    }

    #[test]
    fn test_cpu_time_with_loops() {
        let mut node = make_node(1, NodeType::SeqScan, Some(10.0), None, vec![]);
        if let Some(ref mut actual) = node.actual {
            actual.loops = 5.0;
        }
        // cpu_time = 10.0 × 5 = 50.0
        assert_eq!(WaterfallEngine::extract_cpu_time(&node), Some(50.0));
    }

    #[test]
    fn test_peak_memory_extraction() {
        let node = make_node_full(1, NodeType::HashJoin, 10.0, 8192.0, vec![]);
        assert_eq!(WaterfallEngine::extract_peak_memory(&node), Some(8192.0));
    }

    #[test]
    fn test_no_analyze_returns_none() {
        let node = PlanNode {
            node_type: NodeType::SeqScan,
            relation: None, join_type: None,
            estimated: None, actual: None,
            properties: vec![], structured_props: None,
            buffers: None, children: vec![],
            indent_level: 0, line_number: 1,
        };
        let plan = ExplainPlan { root: node, summary: None };
        assert!(WaterfallEngine::generate(&plan).is_none());
    }

    #[test]
    fn test_subtree_cpu_reduction() {
        // Root(10ms) → [Child A(30ms), Child B(20ms)]
        // subtree_cpu for Root = 10 + 30 + 20 = 60
        let child_a = make_node_full(2, NodeType::SeqScan, 30.0, 0.0, vec![]);
        let child_b = make_node_full(3, NodeType::SeqScan, 20.0, 0.0, vec![]);
        let root = make_node_full(1, NodeType::HashJoin, 10.0, 0.0, vec![child_a, child_b]);
        let plan = ExplainPlan { root, summary: None };

        let waterfall = WaterfallEngine::generate(&plan).unwrap();
        assert_eq!(waterfall.bottlenecks.total_cpu_time_ms, 60.0);
    }

    #[test]
    fn test_bottleneck_detection() {
        // Root(10ms) → [BigChild(80ms), SmallChild(5ms)]
        // BigChild is 80/95 ≈ 84% → bottleneck
        let big = make_node_full(2, NodeType::SeqScan, 80.0, 0.0, vec![]);
        let small = make_node_full(3, NodeType::IndexScan, 5.0, 0.0, vec![]);
        let root = make_node_full(1, NodeType::HashJoin, 10.0, 0.0, vec![big, small]);
        let plan = ExplainPlan { root, summary: None };

        let waterfall = WaterfallEngine::generate(&plan).unwrap();
        let big_entry = waterfall.entries.iter()
            .find(|e| e.metrics.line_number == 2)
            .unwrap();
        assert!(big_entry.is_bottleneck);
        assert!(big_entry.bottleneck_dimensions.contains(&ResourceDimension::CpuTime));
    }

    #[test]
    fn test_spill_detection() {
        let mut node = make_node_full(1, NodeType::Sort, 10.0, 8192.0, vec![]);
        // Add sort spill
        if let Some(ref mut sp) = node.structured_props {
            sp.sort_method = Some("external merge".to_string());
            sp.sort_disk = Some("5840kB".to_string());
        }

        let spill_kb = WaterfallEngine::extract_sort_spill(&node);
        assert_eq!(spill_kb, Some(5840.0));
    }

    #[test]
    fn test_profile_for_streaming() {
        use crate::model::StreamingType;
        let st = NodeType::Streaming(StreamingType::Gather);
        let profile = ResourceProfile::for_node(&st);
        assert!(profile.description.contains("network"));
    }

    #[test]
    fn test_profile_for_all_scans() {
        // 确保所有 scan 类型都有 profile
        let scans = [
            NodeType::SeqScan, NodeType::IndexScan, NodeType::CStoreScan,
            NodeType::BitmapIndexScan, NodeType::ForeignScan,
        ];
        for nt in &scans {
            let profile = ResourceProfile::for_node(nt);
            assert!(!profile.primary_dimensions.is_empty(), "No profile for {:?}", nt);
        }
    }
}
```

### 4.2 测试夹具

```
tests/fixtures/waterfall/
├── w01_simple_cpu.txt              # 简单 3 节点计划，清晰的 CPU 分布
├── w02_memory_spill.txt            # Sort 溢出 + Hash 溢出
├── w03_deep_tree.txt               # 5+ 层深度，测试子树归约
├── w04_no_analyze.txt              # 纯 EXPLAIN，应返回 None
├── w05_streaming_network.txt       # 含 Streaming 节点（Phase 1 仅标注，不计算 network）
```

### 4.3 集成测试

```rust
// 在 tests/analyzer_tests.rs 中新增

#[test]
fn test_waterfall_simple_cpu() {
    let input = include_str!("fixtures/waterfall/w01_simple_cpu.txt");
    let plan = ogexplain_core::parse(input).expect("parse");
    let waterfall = ogexplain_core::waterfall(&plan);
    // 纯 EXPLAIN 无 ANALYZE → None
    // 或如果 fixture 有 ANALYZE → 验证 CPU 总时间
    if let Some(wf) = waterfall {
        assert!(wf.bottlenecks.total_cpu_time_ms > 0.0);
    }
}

#[test]
fn test_waterfall_memory_spill() {
    let input = include_str!("fixtures/waterfall/w02_memory_spill.txt");
    let plan = ogexplain_core::parse(input).expect("parse");
    let waterfall = ogexplain_core::waterfall(&plan).expect("should have waterfall");

    // 至少一个节点检测到 spill
    assert!(waterfall.bottlenecks.spill_node_count > 0);

    // spill 节点标记为 Memory 瓶颈
    let spill_nodes: Vec<_> = waterfall.entries.iter()
        .filter(|e| e.metrics.has_memory_spill)
        .collect();
    for node in spill_nodes {
        assert!(node.bottleneck_dimensions.contains(&ResourceDimension::Memory));
    }
}
```

---

## 5. 实施任务清单

### Task 1: 创建 waterfall 模块骨架 + types.rs + profile.rs

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/waterfall/mod.rs`
- Create: `crates/ogexplain-core/src/analyzer/waterfall/types.rs`
- Create: `crates/ogexplain-core/src/analyzer/waterfall/profile.rs`
- Modify: `crates/ogexplain-core/src/analyzer/mod.rs` — 添加 `pub mod waterfall;`

**Step 1**: 创建 `types.rs`，定义 `ResourceDimension`、`NodeResourceMetrics`、`WaterfallEntry`、`BottleneckSummary`、`PlanWaterfall`。

**Step 2**: 创建 `profile.rs`，实现 `ResourceProfile` struct + `for_node()` match 表达式（覆盖所有 104+ NodeType 变体）。

**Step 3**: 创建 `mod.rs`，导出公共类型。

**Step 4**: 在 `analyzer/mod.rs` 中注册。

**Step 5**: 编译验证 `cargo build -p ogexplain-core`。

**验证**: 编译通过。`profile.rs` 的 match 必须覆盖所有 NodeType 变体（编译器强制）。

### Task 2: 实现 WaterfallEngine + 单元测试

**Files:**
- Create: `crates/ogexplain-core/src/analyzer/waterfall/engine.rs`

**Step 1**: 编写单元测试（`test_cpu_time_extraction`, `test_cpu_time_with_loops`, `test_peak_memory_extraction`, `test_no_analyze_returns_none`, `test_subtree_cpu_reduction`, `test_bottleneck_detection`, `test_spill_detection`, `test_profile_for_streaming`, `test_profile_for_all_scans`）。

**Step 2**: 运行测试确认失败。

**Step 3**: 实现 `WaterfallEngine`（`post_order`, `extract_cpu_time`, `extract_peak_memory`, `extract_sort_spill`, `extract_hash_batches`, 瓶颈检测逻辑）。

**Step 4**: 运行单元测试确认通过。

**Step 5**: 全量测试 `cargo test --workspace`。

**验证**: 所有单元测试通过。现有 317 测试不回归。

### Task 3: 公共 API + JSON 输出

**Files:**
- Modify: `crates/ogexplain-core/src/lib.rs` — 添加 `waterfall()` 公共函数
- Modify: `crates/ogexplain-cli/src/lib.rs` — `output_json()` 添加 waterfall 字段

**Step 1**: 在 `lib.rs` 中添加 `pub fn waterfall()`。

**Step 2**: 在 CLI `output_json()` 的 `JsonOutput` struct 中添加 `waterfall` 字段。

**Step 3**: 运行 `cargo test --workspace` + `cargo clippy --workspace`。

**验证**: JSON 输出包含 `waterfall` 字段，含 `entries`、`bottlenecks`、`total_nodes`。

### Task 4: CLI `--format=waterfall` ANSI 输出

**Files:**
- Modify: `crates/ogexplain-cli/src/lib.rs` — 添加 `output_waterfall()` + match arm

**Step 1**: 实现 `output_waterfall()`（摘要头部 + CPU Top-5 + Memory Top-5 + 全节点瀑布条形图）。

**Step 2**: 在 `analyze_and_output()` 中添加 `"waterfall"` match arm。

**Step 3**: 手动测试：`cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt -o waterfall`。

**Step 4**: 手动测试 JSON：`cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt -o json | jq '.waterfall'`。

**Step 5**: 手动测试 spill 场景：`cargo run -p ogexplain-cli -- analyze tests/fixtures/05_sort_external_merge.txt -o waterfall`。

**验证**: ANSI 输出显示水平条形图，瓶颈节点标红，spill 节点标注 ⚠️。

### Task 5: 测试夹具 + 集成测试 + 最终验证

**Files:**
- Create: `tests/fixtures/waterfall/w01_simple_cpu.txt`
- Create: `tests/fixtures/waterfall/w02_memory_spill.txt`
- Create: `tests/fixtures/waterfall/w03_deep_tree.txt`
- Create: `tests/fixtures/waterfall/w04_no_analyze.txt`
- Create: `tests/fixtures/waterfall/w05_streaming_network.txt`

**Step 1**: 创建测试夹具文件（基于真实 OG EXPLAIN ANALYZE 格式）。

**Step 2**: 编写集成测试。

**Step 3**: `cargo test --workspace` — 全部通过。

**Step 4**: `cargo clippy --workspace` — 零警告。

**Step 5**: `cargo fmt --all -- --check` — 格式正确。

**Step 6**: 手动验证各输出模式（text, json, heatmap, waterfall）。

**验证**: 全量测试通过。clippy 零警告。格式正确。

---

## 6. 与其他方案的联动

三个方案共享同一套基础设施模式：

| 维度 | 热力图 (Heatmap) | 反模式 (Anti-Pattern) | 瀑布图 (Waterfall) |
|------|------------------|----------------------|-------------------|
| 回答的问题 | 「哪里偏差大」 | 「偏差大的节点是什么模式」 | 「资源花在哪里」 |
| 输出类型 | 定量（Q-Error 数值） | 定性（匹配/不匹配） | 定量（时间/内存数值） |
| 核心算法 | 后序+前序遍历 | DFS 子树匹配 | DFS 后序遍历 |
| 共享模式 | DFS 遍历, `line_number` 标识 | 同左 | 同左 |

**联动路径**：

1. **热力图 → 瀑布图**：Q-Error 最大的节点通常也是 CPU 瓶颈（低估 → Nested Loop → CPU 爆炸）
2. **瀑布图 → 反模式**：CPU/Memory 瓶颈节点是反模式匹配的优先搜索目标
3. **三合一视图**：未来可在 TUI 中叠加显示 — 树节点同时显示 Q-Error 色条 + 资源条形图 + 反模式标记

### 与现有规则的协作

| 规则 | 瀑布图如何复用 |
|------|---------------|
| MEM-001 SortSpillToDisk | 瀑布图的 `has_memory_spill` 标注 sort spill 节点 |
| MEM-004 HighPeakMemory | 瀑布图的 Memory Top-5 自然包含高内存节点 |
| JOIN-002 HashSpillToDisk | 瀑布图的 `hash_spill_batches` 标注 hash spill 节点 |
| NET-001 BroadcastLargeTable | 瀑布图的 Streaming 节点标注（Phase 2 可加入 Network 维度） |
| EST-001 SevereEstimation | 热力图 + 瀑布图交叉分析 — 高 Q-Error 节点的 CPU 消耗验证 |

---

## 7. 未来扩展路径（不在本方案范围内）

### Phase 2: IO 维度

- **前置条件**：在 `tree_builder.rs` 中实现 Buffers 解析（约 100-150 行代码）
  - 解析 `Buffers: shared hit=100 read=50, temp read=10 written=5`
  - 解析 `I/O Timings: read=0.123 write=0.045`
  - 填充 `PlanNode.buffers: Option<BufferStats>`
- **新增夹具**：需要含 `EXPLAIN (ANALYZE, BUFFERS)` 输出的真实 OG 数据
- **数据提取**：`node.buffers.as_ref()?.shared_read`, `temp_read`, `temp_written`, `io_read_time_ms`
- **瀑布图扩展**：在 `NodeResourceMetrics` 中添加 `io_reads`, `io_writes`, `io_time_ms` 字段

### Phase 3: Network 维度

- **前置条件**：解析 Streaming 节点的 `Data Size` 属性（OG 5.x 特性）
- **估算公式**：Streaming 节点 `actual.rows × estimated.plan_width`（粗略，标注为估算值）
- **或**：从 `PlanSummary.total_network_kb` 按比例分配到 Streaming 节点

### Phase 4: 多维归一化

- **归一分数**：将 CPU/Memory/IO/Network 各维度归一化到 0-100
- **权重分配**：需要 OG 实际负载标定，不能随意设定
- **自适应权重**：根据数据量动态调整（小数据 Nested Loop → CPU 权重高，大数据 → IO 权重高）

### Phase 5: TUI 集成

- **瀑布图面板**：在 TUI 中新增瀑布图视图（水平条形图）
- **切换模式**：Tab 在树视图 / 热力图 / 瀑布图之间切换
- **交互**：点击瓶颈节点跳转到树视图的对应位置
