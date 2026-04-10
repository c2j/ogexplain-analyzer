# OpenGauss Execution Plan Analyzer — 设计规格书

> **版本**: v2.0-draft
> **日期**: 2026-04-11
> **状态**: Draft (Updated with OG-specific diagnostics)

---

## 1. 项目概述

### 1.1 背景

OpenGauss（基于 PostgreSQL 内核增强）的 `EXPLAIN` / `EXPLAIN ANALYZE` 输出包含丰富的执行计划信息，但原始文本难以直观阅读和系统化分析。DBA 和开发人员需要一个工具来：

1. **解析** EXPLAIN 原始文本为结构化数据
2. **解读** 计划树中各节点的语义含义
3. **排查** 潜在性能隐患
4. **建议** 具体优化措施

### 1.2 目标

| 目标 | 验收标准 |
|------|---------|
| 解析 TEXT 格式 EXPLAIN 输出 | 覆盖 OpenGauss 全部 80+ 种节点类型，解析正确率 > 99% |
| 支持 JSON/XML/YAML 格式 | 可选，第二期 |
| 性能隐患自动检测 | 内置 45+ 条诊断规则，覆盖常见性能反模式（含 OG 特有：下推、向量化、隐式转换等） |
| 优化建议输出 | 每条诊断至少关联一条可操作建议 |
| CLI 可用 | 单二进制文件，支持管道输入 |
| 编程接口 | 作为 library 可被其他项目集成 |

### 1.3 非目标

- **不做** SQL 改写或自动调优
- **不做** 连接数据库直接获取 EXPLAIN（仅解析已有文本）
- **不做** GUI（CLI 优先，未来可扩展）

---

## 2. 术语定义

| 术语 | 定义 |
|------|------|
| Plan Node | 执行计划中的单个操作节点（如 Seq Scan、Hash Join） |
| Plan Tree | 由 Plan Node 构成的树形结构 |
| Estimated Cost | 优化器估算的代价（`cost=startup..total`） |
| Actual Stats | `EXPLAIN ANALYZE` 采集的实际运行统计 |
| Qual | 查询条件谓词（Filter、Index Cond、Hash Cond 等） |
| Vector Node | OpenGauss 向量化引擎节点（以 `Vector`/`Vec` 为前缀） |
| CStore | OpenGauss 列式存储引擎 |
| Streaming | OpenGauss 分布式流式算子 |
| Sonic Hash | OpenGauss 大内存哈希优化技术 |

---

## 3. EXPLAIN TEXT 格式规范

> 以下规范基于 OpenGauss 源码 `src/gausskernel/optimizer/commands/explain.cpp` 和 `src/gausskernel/optimizer/util/optcommon.cpp` 逆向分析。

### 3.1 整体结构

```
QUERY PLAN
--------------------------------------------------
{Plan Tree}
Total runtime: {time} ms[, Peak Memory: {mem} KB]
```

或 pretty 模式（`explain_perf_mode != EXPLAIN_NORMAL`）：

```
{Plan Node ID} --{Node Name}
  {Plan Node ID} --{Node Name}
  ...
Coordinator/Datanode executor start time: {time} ms
Coordinator/Datanode executor run time: {time} ms
Coordinator/Datanode executor end time: {time} ms
Total network: {size}kB
Planner runtime: {time} ms
Plan size: {size} byte
Query Id: {id}
Total runtime: {time} ms
```

### 3.2 Plan Node 行格式

#### 3.2.1 根节点

```
{NodeName}  (cost={startup}..{total} rows={rows} width={width}) [(actual time={s}..{t} rows={r} loops={l})]
```

#### 3.2.2 子节点

```
  ->  {NodeName} [JoinType] [on {Relation}]  (cost=...) [(actual ...)]
```

- 缩进：每层 2 个空格
- `->  ` 前缀（`->` + 2 spaces）标记一个新节点
- 子节点在父节点之后，缩进更深

#### 3.2.3 属性行

属性行紧跟其所属节点，缩进相同或更深一级：

```
        {Property Label}: {Value}
```

### 3.3 Cost 行内信息

```
(cost={startup_cost}..{total_cost} rows={plan_rows} width={plan_width})
```

可选附加字段（仅单机模式，非 `ENABLE_MULTIPLE_NODES`）：

```
p-time={pred_total_time} p-rows={pred_rows}
```

可选附加字段（仅 HashJoin + verbose）：

```
distinct=[{outer_distinct}, {inner_distinct}]
```

### 3.4 Actual 行内信息

```
(actual time={startup_ms}..{total_ms} rows={ntuples} loops={nloops})
```

- time 单位为毫秒（源码中 `* 1000.0` 转换）
- 无 timer 时省略 time，只显示：`(actual rows={r} loops={l})`
- 未执行时：`(Actual time: never executed)` 或 `(Actual time: unknown)`

### 3.5 节点属性（Quals & Properties）

| 属性标签 | 格式 | 适用节点 |
|----------|------|---------|
| `Index Cond` | `Index Cond: {expr}` | IndexScan, IndexOnlyScan, BitmapIndexScan, CStoreIndexScan |
| `Filter` | `Filter: {expr}` | 几乎所有 Scan 节点 |
| `Rows Removed by Filter` | `Rows Removed by Filter: {n}` | 有 Filter 的节点 |
| `Rows Removed by Index Recheck` | `Rows Removed by Index Recheck: {n}` | 有 Index Cond 的节点 |
| `Join Filter` | `Join Filter: {expr}` | NestLoop, HashJoin, MergeJoin |
| `Rows Removed by Join Filter` | `Rows Removed by Join Filter: {n}` | Join 节点 |
| `Hash Cond` | `Hash Cond: {expr}` | HashJoin |
| `Merge Cond` | `Merge Cond: {expr}` | MergeJoin |
| `Sort Key` | `Sort Key: {col1, col2, ...}` | Sort, MergeJoin, GroupAggregate |
| `Sort Method` | `Sort Method: {method}  {spaceType}: {spaceUsed}kB` | Sort（ANALYZE） |
| `Sort Method (range)` | `Sort Method: {method}  {spaceType}: {min}kB ~ {max}kB` | Sort（分布式多 DN） |
| `Group Key` | `Group Key: {col1, col2, ...}` | Group, GroupAggregate |
| `Hash Buckets/Batches` | `Buckets: {n} (originally {orig}) Batches: {n} (originally {orig})  Memory Usage: {mem}kB` | Hash（ANALYZE） |
| `Hash Buckets/Batches (simple)` | `Buckets: {n}  Batches: {n}  Memory Usage: {mem}kB` | Hash（无 original 信息时） |
| `Output` | `Output: {col1, col2, ...}` | 所有节点（verbose） |
| `Inner Unique` | `Inner Unique: true/false` | Join 节点 |
| `Heap Fetches` | `Heap Fetches: {n}` | IndexOnlyScan（ANALYZE） |
| `One-Time Filter` | `One-Time Filter: {expr}` | SubqueryScan |
| `CTE Name` | `CTE Name: {name}` | CteScan |
| `Function Call` | `Function Call: {expr}` | FunctionScan |
| `Schema` | `Schema: {name}` | FunctionScan, ForeignScan |
| `Merge Inserted/Updated/Deleted` | `Merge Inserted: {n}` / `Merge Updated: {n}` / `Merge Deleted: {n}` | ModifyTable (MERGE) |

### 3.6 Buffer 信息

格式（ANALYZE + BUFFERS）：

```
Buffers: shared hit={n} read={n} dirtied={n} written={n}, local hit={n} read={n} dirtied={n} written={n}, temp read={n} written={n}
```

- 仅显示值 > 0 的字段
- I/O 时间（可选）：`I/O Timings: read={ms} write={ms}`

分布式多 DN 模式：

```
Max Buffers: shared hit={n} read={n} ..., Min Buffers: shared hit={n} read={n} ...
Max I/O Timings: read={ms} write={ms}, Min I/O Timings: read={ms} write={ms}
```

### 3.7 Streaming 类型

| 流类型 | 说明 |
|--------|------|
| `Streaming(type: GATHER)` | 汇聚到 Coordinator |
| `Streaming(type: REDISTRIBUTE)` | 按 hash 重分布 |
| `Streaming(type: BROADCAST)` | 广播到所有 DN |
| `Streaming(type: LOCAL REDISTRIBUTE)` | SMP 本地重分布 |
| `Streaming(type: LOCAL BROADCAST)` | SMP 本地广播 |
| `Streaming(type: LOCAL GATHER)` | SMP 本地汇聚 |
| `Streaming(type: LOCAL ROUNDROBIN)` | SMP 本地轮询 |
| `Streaming(type: SPLIT REDISTRIBUTE)` | 分裂重分布 |
| `Streaming(type: SPLIT BROADCAST)` | 分裂广播 |
| `Streaming(type: RANGE REDISTRIBUTE)` | 范围重分布 |
| `Streaming(type: LIST REDISTRIBUTE)` | 列表重分布 |
| `Streaming(type: PART REDISTRIBUTE PART BROADCAST)` | 混合重分布+广播 |
| `Streaming(type: PART REDISTRIBUTE PART ROUNDROBIN)` | 混合重分布+轮询 |
| `Streaming(type: PART REDISTRIBUTE PART LOCAL)` | 混合重分布+本地 |
| `Streaming(type: PART LOCAL PART BROADCAST)` | 混合本地+广播 |
| `Streaming(type: HYBRID)` | 混合流 |
| `Vector Streaming(type: ...)` | 向量化版本（前缀 `Vector `） |

DOP 信息（并行度）：`dop: {consumer}/{producer}`
NodeGroup 信息：`ng: {producer_group}->{consumer_group}`

### 3.8 完整节点类型清单

#### 扫描类

```
Seq Scan, Partitioned Seq Scan, Sample Scan, Partitioned Sample Scan
Index Scan, Partitioned Index Scan
Index Only Scan, Partitioned Index Only Scan
Bitmap Index Scan, Partitioned Bitmap Index Scan
Bitmap Heap Scan, Partitioned Bitmap Heap Scan
Tid Scan, Partitioned Tid Scan, Tid Range Scan
Subquery Scan, Vector Subquery Scan
Function Scan, Values Scan, CTE Scan, WorkTable Scan
Foreign Scan, Partitioned Foreign Scan, Vector Foreign Scan, Partitioned Vector Foreign Scan
CStore Scan, Partitioned CStore Scan
TsStore Scan, Partitioned TsStore Scan
Ann Index Scan, Partitioned Ann Index Scan
CStore Index Scan, Partitioned CStore Index Scan, CStore Index Only Scan
CStore Index Ctid Scan, Partitioned CStore Index Ctid Scan
CStore Index Heap Scan, Partitioned CStore Index Heap Scan
IMCStore Scan, Partitioned IMCStore Scan
```

#### 连接类

```
Nested Loop, Vector Nest Loop
Hash Join, Vector Hash Join, Vector Sonic Hash Join
Merge Join, Vector Merge Join
Vector Asof Join
```

Join 子类型（拼接在名称后）：
```
Inner, Left, Full, Right, Semi, Anti, Right Semi, Right Anti,
Left Anti Full, Right Anti Full, Left Anti Semi (Not-In)
```

#### 聚合/排序类

```
Aggregate (Plain), GroupAggregate (Sorted), HashAggregate (Hashed)
Vector Aggregate, Vector Hash Aggregate, Vector Sonic Hash Aggregate, Vector Sort Aggregate
Sort, Vector Sort, Group Sort
Group, Vector Group
WindowAgg, Vector WindowAgg
Unique, Vector Unique
```

#### 集合操作类

```
SetOp (Sorted), HashSetOp (Hashed)
Vector SetOp, Vector HashSetOp
Append, Vector Append, Merge Append
Recursive Union
BitmapAnd, BitmapOr
CStore Index And, CStore Index Or
```

#### DML 类

```
Insert, Update, Delete, Merge, Replace
Vector Insert, Vector Update, Vector Delete, Vector Merge
```

#### 辅助节点

```
Result, Vector Result, ProjectSet
Hash
Materialize, Vector Materialize
Limit, Vector Limit
LockRows
Partition Iterator, Vector Partition Iterator
Row Adapter, Vector Adapter
StartWith Operator
```

#### 分布式节点

```
Streaming(type: ...), Vector Streaming(type: ...)
Data Node Scan
```

---

## 4. 系统架构

### 4.1 整体架构

```
┌─────────────────────────────────────────────────────────┐
│                      CLI / Library                       │
├─────────┬──────────────┬───────────────┬────────────────┤
│  Input  │   Parser     │   Analyzer    │   Reporter     │
│  Layer  │   Layer      │   Layer       │   Layer        │
│         │              │               │                │
│ Text /  │ Lexer →      │ Diagnostics   │ Text / JSON /  │
│ JSON /  │ Parser →     │ Engine:       │ Markdown /     │
│ XML /   │ PlanTree     │ - Rule eval   │ HTML output    │
│ YAML    │ (AST)        │ - Scoring     │                │
│         │              │               │ Suggestion     │
│         │              │ Suggestion    │ formatter      │
│         │              │ Engine        │                │
└─────────┴──────────────┴───────────────┴────────────────┘
```

### 4.2 数据流

```
EXPLAIN text
    │
    ▼
┌────────┐     ┌─────────────┐     ┌──────────────┐     ┌────────────┐
│ Input  │────▶│   Parser    │────▶│  PlanTree    │────▶│ Analyzer   │
│ Reader │     │  (TEXT/JSON)│     │  (AST)       │     │ (Rules)    │
└────────┘     └─────────────┘     └──────────────┘     └─────┬──────┘
                                                               │
                                          ┌────────────────────┤
                                          ▼                    ▼
                                   ┌────────────┐      ┌────────────┐
                                   │ Diagnostic │      │ Suggestion │
                                   │ Report     │      │ Set        │
                                   └──────┬─────┘      └──────┬─────┘
                                          │                    │
                                          ▼                    ▼
                                   ┌──────────────────────────────┐
                                   │         Reporter             │
                                   │   (Text/JSON/Markdown/HTML)  │
                                   └──────────────────────────────┘
```

---

## 5. 解析器设计

### 5.1 技术方案：两阶段解析

采用 **词法分析 + 递归下降** 两阶段方案：

1. **Phase 1 — Line Classifier（行分类器）**：逐行扫描，将每行分类为 `NodeLine`、`PropertyLine`、`SummaryLine`、`SeparatorLine`、`BlankLine`
2. **Phase 2 — Tree Builder（树构建器）**：根据缩进层级和行类型，递归构建 Plan Tree

### 5.2 行分类规则

```
LineType = 
  | NodeLine(indent, prefix "->" | none, node_name, cost_info?, actual_info?)
  | PropertyLine(indent, label, value)
  | SummaryLine(key, value)
  | SeparatorLine
  | BlankLine
  | HeaderLine(text)    // "QUERY PLAN" 等
```

#### NodeLine 识别正则

```regex
^(?<indent>\s*)(?:->\s+)?(?<name>[A-Z][A-Za-z\s]*?)(?:\s+on\s+(?<relation>\S+))?\s*(?:\((?<metrics>[^)]+)\))?\s*(?:\((?<actual>actual[^)]+)\))?\s*$
```

#### Cost Info 识别正则

```regex
cost=(?<startup>[\d.]+)\.\.(?<total>[\d.]+)\s+rows=(?<rows>[\d.]+)\s+width=(?<width>\d+)
```

#### Actual Info 识别正则

```regex
actual\s+time=(?<startup>[\d.]+)\.\.(?<total>[\d.]+)\s+rows=(?<rows>[\d.]+)\s+loops=(?<loops>[\d.]+)
```

或无 timer 版本：

```regex
actual\s+rows=(?<rows>[\d.]+)\s+loops=(?<loops>[\d.]+)
```

#### PropertyLine 识别正则

```regex
^(?<indent>\s+)(?<label>[A-Z][A-Za-z :]+?):\s+(?<value>.+)$
```

#### SummaryLine 识别正则

```regex
^(Total runtime|Peak Memory|Planner runtime|Plan size|Query Id|executor (start|run|end) time|Total network):\s+(.+)$
```

### 5.3 树构建算法

```python
def build_tree(classified_lines):
    """
    基于缩进层级构建树。
    核心思路：使用栈维护当前路径。
    """
    root = PlanNode(name="ROOT", children=[])
    stack = [(root, -1)]  # (node, indent_level)

    for line in classified_lines:
        if line.type == NodeLine:
            # 弹出栈直到找到缩进更浅的父节点
            while len(stack) > 1 and stack[-1][1] >= line.indent:
                stack.pop()
            parent = stack[-1][0]
            node = PlanNode(
                name=line.node_name,
                relation=line.relation,
                cost=line.cost_info,
                actual=line.actual_info,
                properties=[],
            )
            parent.children.append(node)
            stack.append((node, line.indent))

        elif line.type == PropertyLine:
            # 属性属于栈顶节点
            stack[-1][0].properties.append(
                Property(label=line.label, value=line.value)
            )

        elif line.type == SummaryLine:
            # 全局摘要信息
            root.summary[line.key] = line.value

    return root
```

### 5.4 AST 数据模型

```rust
// === 核心数据结构 ===

/// 解析后的完整执行计划
struct ExplainPlan {
    /// 计划树的根节点列表（通常只有一个根）
    nodes: Vec<PlanNode>,
    /// 全局摘要信息
    summary: PlanSummary,
    /// 原始文本（用于错误定位）
    source: String,
}

/// 单个计划节点
struct PlanNode {
    /// 节点类型（如 "Seq Scan", "Hash Join"）
    node_type: NodeType,
    /// 操作的关系对象（表名、索引名等）
    relation: Option<String>,
    /// Join 子类型
    join_type: Option<JoinType>,
    /// 优化器估算信息
    estimated: Option<EstimatedCost>,
    /// 实际执行统计（仅 EXPLAIN ANALYZE）
    actual: Option<ActualStats>,
    /// 节点属性列表（Filter, Hash Cond 等）
    properties: Vec<NodeProperty>,
    /// Buffer 统计
    buffers: Option<BufferStats>,
    /// 子节点
    children: Vec<PlanNode>,
    /// 缩进层级（原始文本中的深度）
    indent_level: usize,
    /// 在原始文本中的行号
    line_number: usize,
}

/// 节点类型分类
enum NodeTypeCategory {
    Scan,
    Join,
    Aggregate,
    Sort,
    Dml,
    SetOp,
    Auxiliary,
    Streaming,
    Materialize,
    Limit,
    Other,
}

/// 节点类型（枚举化所有已知类型）
enum NodeType {
    // 扫描
    SeqScan,
    IndexScan,
    IndexOnlyScan,
    BitmapIndexScan,
    BitmapHeapScan,
    TidScan,
    TidRangeScan,
    SubqueryScan,
    FunctionScan,
    ValuesScan,
    CteScan,
    WorkTableScan,
    ForeignScan,
    CStoreScan,
    TsStoreScan,
    AnnIndexScan,
    CStoreIndexScan,
    CStoreIndexCtidScan,
    CStoreIndexHeapScan,
    ImCStoreScan,
    // 向量化扫描变体
    VectorSubqueryScan,
    VectorForeignScan,
    // 连接
    NestedLoop,
    HashJoin,
    MergeJoin,
    // 向量化连接变体
    VectorNestLoop,
    VectorHashJoin,
    VectorSonicHashJoin,
    VectorMergeJoin,
    VectorAsofJoin,
    // 聚合
    Aggregate,       // Plain
    GroupAggregate,  // Sorted
    HashAggregate,   // Hashed
    VectorAggregate,
    VectorHashAggregate,
    VectorSonicHashAggregate,
    // 排序
    Sort,
    GroupSort,
    VectorSort,
    // 分组
    Group,
    VectorGroup,
    // 窗口
    WindowAgg,
    VectorWindowAgg,
    // 唯一
    Unique,
    VectorUnique,
    // 集合操作
    SetOp,
    HashSetOp,
    VectorSetOp,
    VectorHashSetOp,
    Append,
    VectorAppend,
    MergeAppend,
    RecursiveUnion,
    BitmapAnd,
    BitmapOr,
    CStoreIndexAnd,
    CStoreIndexOr,
    // DML
    Insert, Update, Delete, Merge, Replace,
    VectorInsert, VectorUpdate, VectorDelete, VectorMerge,
    // 辅助
    Result, VectorResult, ProjectSet,
    Hash,
    Materialize, VectorMaterialize,
    Limit, VectorLimit,
    LockRows,
    PartitionIterator, VectorPartitionIterator,
    RowAdapter, VectorAdapter,
    StartWithOp,
    // 分布式
    Streaming(StreamingType),
    VectorStreaming(StreamingType),
    DataNodeScan,
    // 分区变体（带 Partitioned 前缀）
    Partitioned(NodeType),
    // 未知
    Unknown(String),
}

/// Join 类型
enum JoinType {
    Inner,
    Left,
    Full,
    Right,
    Semi,
    Anti,
    RightSemi,
    RightAnti,
    LeftAntiFull,
    RightAntiFull,
    LeftAntiSemiNotIn,
}

/// Streaming 子类型
enum StreamingType {
    Gather,
    Redistribute,
    Broadcast,
    LocalRedistribute,
    LocalBroadcast,
    LocalGather,
    LocalRoundrobin,
    SplitRedistribute,
    SplitBroadcast,
    RangeRedistribute,
    ListRedistribute,
    Hybrid,
    PartRedistributePartBroadcast,
    PartRedistributePartRoundrobin,
    PartRedistributePartLocal,
    PartLocalPartBroadcast,
    Unknown(String),
}

/// 优化器估算
struct EstimatedCost {
    startup_cost: f64,
    total_cost: f64,
    plan_rows: f64,
    plan_width: i32,
    /// 预测时间（仅单机模式）
    pred_time: Option<f64>,
    /// 预测行数（仅单机模式）
    pred_rows: Option<f64>,
    /// HashJoin 去重估算（仅 verbose）
    distinct: Option<(f64, f64)>,
}

/// 实际执行统计
struct ActualStats {
    startup_time_ms: f64,
    total_time_ms: f64,
    rows: f64,
    loops: f64,
    /// 是否实际执行过
    executed: bool,
}

/// Buffer 统计
struct BufferStats {
    shared_hit: i64,
    shared_read: i64,
    shared_dirtied: i64,
    shared_written: i64,
    local_hit: i64,
    local_read: i64,
    local_dirtied: i64,
    local_written: i64,
    temp_read: i64,
    temp_written: i64,
    io_read_time_ms: Option<f64>,
    io_write_time_ms: Option<f64>,
}

/// 节点属性
struct NodeProperty {
    label: String,
    value: String,
}

/// 全局摘要
struct PlanSummary {
    total_runtime_ms: Option<f64>,
    peak_memory_kb: Option<i64>,
    planner_runtime_ms: Option<f64>,
    plan_size_bytes: Option<i64>,
    query_id: Option<String>,
    executor_start_ms: Option<f64>,
    executor_run_ms: Option<f64>,
    executor_end_ms: Option<f64>,
    total_network_kb: Option<i64>,
}
```

### 5.5 行内解析细节

#### 5.5.1 Sort Method 解析

格式：`Sort Method: {method}  {spaceType}: {spaceUsed}kB`

已知 Sort Method 值：
- `quicksort` / `top-n heapsort` / `external merge` / `external sort`
- 向量化版本：`vec quicksort` / `vec top-n heapsort`

Space Type：`Memory` / `Disk`

#### 5.5.2 Hash Info 解析

格式（标准）：
```
Buckets: {n}  Batches: {n}  Memory Usage: {mem}kB
```

格式（有 original 信息）：
```
Buckets: {n} (originally {orig}) Batches: {n} (originally {orig})  Memory Usage: {mem}kB
```

分布式多 DN 范围格式：
```
Max Buckets: {n}  Max Batches: {n} [(max originally {n})]  Max Memory Usage: {n}kB
Min Buckets: {n}  Min Batches: {n} [(min originally {n})]  Min Memory Usage: {n}kB
```

#### 5.5.3 Streaming 解析

格式：`Streaming(type: {TYPE} [dop: {c}/{p}] [ng: {g1}->{g2}])`

向量化：`Vector Streaming(type: ...)`

#### 5.5.4 节点名称解析优先级

```
输入行 → 尝试匹配 Streaming 模式
       → 尝试匹配已知 NodeType（含 Partitioned 前缀）
       → 尝试提取 Join 子类型
       → 尝试提取 on {relation}
       → 标记为 Unknown
```

---

## 6. 性能诊断引擎设计

### 6.1 规则模型

```rust
/// 诊断规则
trait DiagnosticRule {
    /// 规则唯一标识
    fn id(&self) -> &str;
    /// 规则名称
    fn name(&self) -> &str;
    /// 严重程度
    fn severity(&self) -> Severity;
    /// 规则分类
    fn category(&self) -> DiagnosticCategory;
    /// 规则描述
    fn description(&self) -> &str;
    /// 执行检查
    fn check(&self, node: &PlanNode, context: &PlanContext) -> Option<Finding>;
}

enum Severity {
    Critical,   // 严重性能问题，必须修复
    Warning,    // 潜在风险，建议关注
    Info,       // 信息性提示
}

enum DiagnosticCategory {
    ScanEfficiency,     // 扫描效率
    JoinStrategy,       // 连接策略
    MemoryUsage,        // 内存使用
    DataSkew,           // 数据倾斜
    SortEfficiency,     // 排序效率
    NetworkOverhead,    // 网络开销（分布式）
    CostMisestimation,  // 估算偏差
    CpuEfficiency,      // CPU 效率
    Concurrency,        // 并发问题
    PushdownFailure,    // 下推失败（OG 特有）
    TypeMismatch,       // 隐式类型转换（OG 特有）
    Vectorization,      // 向量化效率（OG 特有）
    SubqueryStructure,  // 子查询结构
    DistributionIssue,  // 分布策略（OG 特有）
    StorageOptimization, // 存储优化（OG 特有）
}
```

### 6.2 诊断上下文

```rust
/// 分析上下文（传递给每条规则）
struct PlanContext {
    /// 完整计划树
    plan: &ExplainPlan,
    /// 当前节点的祖先路径
    ancestors: Vec<&PlanNode>,
    /// 当前节点在父节点中的位置（left/right/outer/inner）
    position: Option<ChildPosition>,
    /// 全局统计（用于相对比较）
    global_stats: GlobalStats,
}

struct GlobalStats {
    /// 最高的 actual total_time
    max_node_time_ms: f64,
    /// 最多的 actual rows
    max_node_rows: f64,
    /// 最大的 actual loops
    max_node_loops: f64,
    /// 计划树总节点数
    total_nodes: usize,
    /// 计划树最大深度
    max_depth: usize,
}
```

### 6.3 规则清单

#### 6.3.1 扫描效率类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `SCAN-001` | 大表全表扫描 | Warning | Seq Scan 节点，`actual rows` > 阈值（默认 10000），无 Filter 或 Filter 选择率 < 10% |
| `SCAN-002` | 低效索引扫描 | Warning | Index Scan 的 `Rows Removed by Index Recheck` 占比较高（> 50%） |
| `SCAN-003` | Bitmap 堆扫描高过滤率 | Info | Bitmap Heap Scan 的 `Rows Removed by Filter` / actual rows > 2.0 |
| `SCAN-004` | 未使用索引的 Filter | Warning | Seq Scan + Filter 且 estimated rows >> actual rows，暗示缺少合适索引 |
| `SCAN-005` | CStore 扫描未下推过滤 | Info | CStore Scan 有 Filter 但 `Rows Removed by Filter` 占比高 |

#### 6.3.2 连接策略类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `JOIN-001` | Nested Loop 大数据集 | Critical | Nested Loop 且内侧 actual rows * loops > 阈值（默认 10000） |
| `JOIN-002` | Hash Join 溢出到磁盘 | Critical | Hash 节点 `Batches > 1`，表示哈希表超出 work_mem |
| `JOIN-003` | Hash Join 无等值条件 | Warning | Hash Join 但 Hash Cond 为空或不含等值比较 |
| `JOIN-004` | Join 倾斜（分布式） | Warning | 分布式场景下，DN 之间 actual rows 差异 > 3x |
| `JOIN-005` | 低选择性 Join | Info | Join 后 actual rows / 输入 rows < 0.01，可能因 Join 条件不当 |

#### 6.3.3 内存使用类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `MEM-001` | Sort 溢出到磁盘 | Critical | Sort Method 包含 `external`，Space Type = `Disk` |
| `MEM-002` | Hash 表过大 | Warning | `Memory Usage` > 阈值（默认 100MB） |
| `MEM-003` | 内存使用与估算偏差大 | Warning | `estimated memory` 存在时，actual / estimated > 2.0 或 < 0.3 |
| `MEM-004` | 峰值内存过高 | Warning | 全局 `Peak Memory` > 阈值（默认 1GB） |

#### 6.3.4 排序效率类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `SORT-001` | 大量数据排序 | Warning | Sort 节点 actual rows > 阈值（默认 50000） |
| `SORT-002` | 排序耗时占比过高 | Warning | Sort 节点 actual total_time > 总运行时间 * 比例阈值（默认 30%） |
| `SORT-003` | 重复排序 | Warning | 同一子树中出现多层 Sort（如 Sort → Sort） |

#### 6.3.5 网络开销类（分布式）

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `NET-001` | 广播大表 | Critical | Streaming(type: BROADCAST) 且 actual rows > 阈值（默认 10000） |
| `NET-002` | 重分布倾斜 | Warning | Streaming(type: REDISTRIBUTE) 后 DN 间 rows 差异 > 3x |
| `NET-003` | 不必要的网络传输 | Info | Streaming 后紧跟另一个 Streaming，中间无计算 |
| `NET-004` | 大量数据 Gather | Warning | Streaming(type: GATHER) 且 actual rows > 阈值（默认 100000） |

#### 6.3.6 估算偏差类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `EST-001` | 行数严重低估 | Critical | `actual rows / estimated rows` > 100（有实际数据时） |
| `EST-002` | 行数严重高估 | Warning | `estimated rows / actual rows` > 100 |
| `EST-003` | 代价估算偏差 | Warning | 节点耗时占比与 cost 占比差异 > 3x |
| `EST-004` | Nested Loop 因低估选中 | Critical | Nested Loop + EST-001，说明优化器因统计信息不足选择了 Nested Loop |

#### 6.3.7 其他

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `GEN-001` | 执行计划过深 | Info | 计划树深度 > 阈值（默认 10） |
| `GEN-002` | 未执行节点 | Info | 某节点 `(Actual time: never executed)` |
| `GEN-003` | CTE 扫描效率低 | Warning | CteScan + actual rows > 阈值 且 loops > 1 |
| `GEN-004` | 子查询物化开销 | Warning | SubqueryScan + Materialize 且数据量大 |

#### 6.3.8 下推失败检测类（FQS / Pushdown）

> **背景**：OpenGauss 分布式架构支持 Fast Query Shipping (FQS)，将整个查询下发到 DN 执行。当下推失败时，查询会在 CN 端拆分为多个 Stream 算子，带来大量网络开销。FQS 失败是分布式场景下最常见的性能杀手。

**EXPLAIN 中的下推判断依据（源码逆向分析）：**

| EXPLAIN 模式 | 含义 | FQS 状态 |
|---|---|---|
| **无** Streaming / Data Node Scan 节点 | Light Proxy 路径，直接下发 | ✅ 完全下推 |
| `Data Node Scan` + `Remote query: <SQL>` | FQS 成功，整条 SQL 下发到 DN | ✅ 完全下推 |
| `Streaming(type: GATHER)` (is_simple=true) | 仅收集结果，不含 SQL 下发 | ⚠️ 部分下推 |
| `Streaming(type: REDISTRIBUTE)` | DN 间重分布数据 | ❌ 未下推 |
| `Streaming(type: BROADCAST)` | DN 间广播数据 | ❌ 未下推 |
| `Coordinator quals` 属性 | CN 端过滤条件 | ⚠️ 部分下推 |
| 多层 Streaming 嵌套 | 复杂的分布式拆分 | ❌ 严重未下推 |

**源码关键点**：
- `explain.cpp` 第 1249-1259 行：`is_simple=false` 时显示为 `Data Node Scan`（FQS），`is_simple=true` 时显示为 `Streaming (type: GATHER)`
- `pgxcship.cpp`：`pgxc_is_query_shippable()` 返回值决定下推能力
- `plananalyzer.cpp` 第 313-333 行：`"SQL is not plan-shipping, reason : \"%s\""` 提供未下推原因
- `pgxcship.cpp` 第 113-135 行：`ShippabilityStat` 枚举定义所有未下推原因码

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `PUSH-001` | 查询未下推（含 Stream 算子） | Critical | 计划树中出现 `Streaming(type: REDISTRIBUTE)` 或 `Streaming(type: BROADCAST)` 节点，且查询涉及的数据量 > 阈值 |
| `PUSH-002` | 多层 Stream 嵌套 | Critical | 计划树中存在连续 2+ 层 Streaming 节点（如 REDISTRIBUTE → GATHER → REDISTRIBUTE） |
| `PUSH-003` | Coordinator 端过滤 | Warning | RemoteQuery 节点包含 `Coordinator quals` 属性，说明部分条件未下推到 DN |
| `PUSH-004` | 大表广播 | Critical | `Streaming(type: BROADCAST)` 且 actual rows > 阈值（默认 10000） |
| `PUSH-005` | 重分布倾斜 | Warning | `Streaming(type: REDISTRIBUTE)` 后多个 DN 之间 actual rows 差异 > 3x |
| `PUSH-006` | 大量数据 Gather | Warning | `Streaming(type: GATHER)` 且 actual rows > 阈值（默认 100000），大量数据汇聚到 CN |

**PUSH-001 诊断建议**：
```
查询未完全下推到 Datanode，使用了 Stream 算子进行数据重分布/广播。
可能原因：
  - 查询包含不可下推的函数（如 nextval(), random()）
  - 查询包含不可下推的数据类型
  - 查询包含窗口函数等需要单节点计算的表达式
  - 分布键不匹配导致需要重分布
排查建议：
  1. 检查 GUC enable_fast_query_shipping 是否为 on
  2. 设置 enable_unshipping_log = on 查看服务器日志中的未下推原因
  3. 使用 EXPLAIN VERBOSE 查看 Remote query 属性中的下发 SQL
```

#### 6.3.9 隐式类型转换检测类

> **背景**：OpenGauss 的 `deparse_expression()` 在 EXPLAIN 输出中**始终隐藏隐式类型转换**（`showimplicit=false`，源码 `explain.cpp` 第 3719 行）。这意味着 `WHERE varchar_col = 123` 中的隐式转换在 EXPLAIN 中不可见。但我们可以通过间接模式推断。

**类型转换导致索引失效的识别原理（源码 `indxpath.cpp` 逆向）：**

当 Parser 插入隐式类型转换（如 `RelabelType`、`FuncExpr(COERCE_IMPLICIT_CAST)`）包裹索引列时：
1. `match_clause_to_indexcol()` (indxpath.cpp:2380) 要求子句形式必须是 `(indexkey op const)`
2. 隐式转换使实际形式变为 `(CAST(indexkey) op const)`，索引键被埋在 Cast 节点中
3. 匹配失败 → 条件退化为 `plan->qual` → 成为 Seq Scan 的 `Filter`
4. EXPLAIN 中只显示 `Filter: (varchar_col = 123)`，看不到 CAST

**可检测的间接模式：**

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `TYPE-001` | 疑似隐式类型转换导致 Seq Scan | Critical | `Seq Scan` + `Filter` 包含等值条件 + `Rows Removed by Filter` 很高 + 该列有索引（需元数据，或通过估算偏差间接判断） |
| `TYPE-002` | Filter 表达式中的类型不匹配特征 | Warning | Filter/Index Cond 中的字面量类型与常见列类型不符，如 `varchar_col = 数字字面量`、`int_col = '字符串'`、`date_col = 字符串` |
| `TYPE-003` | Index Cond 部分生效（部分条件下推） | Warning | `Index Scan` 同时有 `Index Cond` 和 `Filter`，且 `Filter` 中的条件本应可以使用同一索引 |
| `TYPE-004` | LIKE 前缀通配符导致索引失效 | Warning | `Seq Scan` + `Filter` 包含 `LIKE '%xxx'` 模式（前缀通配符无法使用 B-tree 索引） |
| `TYPE-005` | 函数包裹索引列 | Warning | `Seq Scan` + `Filter` 包含 `func(col) = value` 模式（函数包裹阻止索引使用） |

**TYPE-001 诊断建议**：
```
检测到可能的隐式类型转换：
  节点: Seq Scan on {table}
  Filter: {expression}
  被过滤行数: {N}
  
  列 "{col}" 可能存在隐式类型转换，导致索引无法使用。
  隐式转换在 EXPLAIN 输出中不可见（showimplicit=false），但以下间接证据支持此判断：
    - 使用了 Seq Scan 而非 Index Scan
    - 等值条件未走索引
    - 大量行被过滤掉
  
  修复建议：
    1. 检查 WHERE 条件中列与常量的数据类型是否一致
    2. 使用显式类型转换: WHERE varchar_col = '123' (而非 123)
    3. 创建表达式索引: CREATE INDEX ON table((col::target_type))
    4. 使用 EXPLAIN VERBOSE + 手动检查原始 SQL 确认
```

**TYPE-002 实现方式（纯文本模式匹配，无需元数据）**：

```python
# 通过正则识别 Filter 中的潜在类型不匹配
TYPE_MISMATCH_PATTERNS = [
    # 数字字面量与常见字符串列名
    (r"(\w+)\s*=\s*(\d+(?:\.\d+)?)\b(?!.*::)", "possible_numeric_to_string"),
    # 字符串字面量与常见数字列名
    (r"(\w+)\s*=\s*'([^']*)'\b(?!.*::)", "possible_string_to_number"),
    # LIKE 模式
    (r"(\w+)\s+LIKE\s+'%[^']*'", "leading_wildcard_like"),
    # 函数包裹列
    (r"(?:to_char|to_date|to_number|substr|trim|lower|upper|cast)\s*\(\s*(\w+)", "function_wrapped_column"),
]
```

#### 6.3.10 向量化效率检测类

> **背景**：OpenGauss 向量化引擎（Vector * 节点）对分析型查询有数量级性能提升。当查询未能利用向量化时，执行效率会大幅下降。`Row Adapter` / `Vector Adapter` 节点标志着行引擎与向量化引擎之间的边界。

**EXPLAIN 中的向量化指标（源码 `optcommon.cpp` 和 `instrument.cpp`）：**

- 向量化节点：以 `Vector` 或 `Vec` 为前缀（如 `Vector Hash Join`、`Vec Sort`）
- `Vector Adapter`（T_RowToVec）：行引擎 → 向量化引擎转换，出现在非向量化 Scan 上层
- `Row Adapter`（T_VecToRow）：向量化引擎 → 行引擎转换，出现在需要行格式输出时
- `CStore Scan`：列存扫描天然向量化
- `Sonic Hash`：大内存优化的向量化 Hash

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `VEC-001` | 混合向量化/行引擎（频繁 Adapter） | Warning | 计划树中出现 2+ 个 `Vector Adapter` 或 `Row Adapter` 节点 |
| `VEC-002` | 行存表大扫描未向量化 | Warning | `Seq Scan`（非 CStore）+ actual rows > 阈值（默认 50000）+ 无上层 Vector Adapter |
| `VEC-003` | CStore 表应使用列存扫描 | Info | 普通表名在 `CStore Scan` 之外的扫描中出现，且存在同名 CStore 表 |
| `VEC-004` | Sonic Hash 溢出（大内存场景） | Warning | `Vector Sonic Hash Join` 或 `Vector Sonic Hash Aggregate` + 有 Partition Spill / Temp File 信息 |
| `VEC-005` | LLVM 代码生成未生效 | Info | 高代价节点（Sort, Hash Join, Aggregate）+ 无 `(LLVM Optimized)` 标记 + `es->detail` 模式 |

#### 6.3.11 子查询与计划结构类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `SUBQ-001` | 关联子查询（SubPlan） | Warning | 计划中出现 `SubPlan N` 节点（非 `InitPlan`），SubPlan 内含完整计划树，表示每行都重新执行子查询 |
| `SUBQ-002` | IN 子查询未转换为 Semi Join | Critical | `Filter` 中包含 `(col IN (SubPlan N))` 模式，优化器未将 IN 子查询提升为 Semi Join |
| `SUBQ-003` | InitPlan 数量过多 | Info | 计划中 `InitPlan N` 数量 > 阈值（默认 5） |
| `SUBQ-004` | CTE 物化后多次扫描 | Warning | `CTE Scan` 的 loops > 1 且 actual rows > 阈值，CTE 结果被反复扫描 |
| `SUBQ-005` | 递归查询效率 | Warning | `Recursive Union` + 迭代次数（Iteration times）> 阈值（默认 100） |

**SUBQ-001 / SUBQ-002 诊断建议**：
```
关联子查询 (SubPlan) 在每行数据上重新执行，导致 O(N*M) 复杂度。
  SubPlan: {name}
  外层行数: {outer_rows}
  SubPlan 行数: {subplan_rows}
  估算总比较次数: {outer_rows * subplan_rows}

  优化建议：
    1. 将子查询改写为 JOIN:
       -- 原: SELECT * FROM t1 WHERE col IN (SELECT col FROM t2 WHERE ...)
       -- 改: SELECT t1.* FROM t1 JOIN t2 ON t1.col = t2.col WHERE ...
    2. 如果使用 EXISTS，确保关联条件有索引
    3. 考虑使用 LATERAL JOIN 替代关联子查询
```

#### 6.3.12 分布式特定问题类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `DIST-001` | 分布键不匹配导致重分布 | Warning | `Streaming(type: REDISTRIBUTE)` 出现在 Join 下方，且 Hash Cond 中的列不包含分布键 |
| `DIST-002` | 复制表不适合大表 Join | Warning | `Streaming(type: BROADCAST)` + 实际广播行数 > 阈值，考虑改为 Hash 分布 |
| `DIST-003` | DOP 并行度不足 | Info | Streaming 节点 `dop: 1/1` 且数据量大（actual rows > 100000） |
| `DIST-004` | 跨 NodeGroup 数据迁移 | Warning | Streaming 节点包含 `ng: group1->group2` 标记，数据跨节点组迁移 |
| `DIST-005` | 数据倾斜（Skew 未优化） | Critical | Join/Aggregate 节点 + DN 间行数差异 > 5x + 无 `Skew Join/Agg Optimized` 标记 |

**DIST-005 诊断建议**：
```
检测到数据倾斜且未启用 Skew 优化：
  节点: {node_type} (行号 {line})
  DN 行数分布: {dn1}: {rows1}, {dn2}: {rows2}, ...
  最大/最小比: {ratio}x

  Skew 优化来源:
    - Hint: 使用 skew(@table col) 提示
    - Rule: 优化器规则自动检测
    - Statistic: 基于统计信息检测

  修复建议：
    1. 收集列统计信息: ANALYZE {table};
    2. 添加 Hint: /*+ skew(table skew_col) */
    3. 考虑修改分布键以均匀分布数据
    4. 对于已知倾斜值，考虑使用 Replication 表
```

#### 6.3.13 Bloom Filter 与存储优化类

| Rule ID | 名称 | 严重度 | 触发条件 |
|---------|------|--------|---------|
| `STORE-001` | Bloom Filter 跳过效率低 | Info | `(skip N rows by bloom filter)` 中 N 占总行数比例 < 10%，Bloom Filter 效果不佳 |
| `STORE-002` | Min/Max 过滤效率低 | Info | CStore/Foreign Scan 节点 `(min max skip: ...)` 中 skip 比例低 |
| `STORE-003` | DFS 文件未静态裁剪 | Warning | `(pruned files: static 0, dynamic N)` — 无静态裁剪，所有文件需动态读取 |
| `STORE-004` | Partition Iterator 全分区扫描 | Warning | `Partition Iterator` + 扫描的分区数量 = 总分区数（未做分区裁剪） |

### 6.4 规则执行引擎

```rust
struct DiagnosticEngine {
    rules: Vec<Box<dyn DiagnosticRule>>,
    config: DiagnosticConfig,
}

struct DiagnosticConfig {
    /// 行数阈值（大表判定）
    large_table_threshold: f64,        // 默认 10000
    /// 内存阈值（KB）
    memory_threshold_kb: f64,          // 默认 102400 (100MB)
    /// 估算偏差倍数
    estimation_skew_factor: f64,       // 默认 100
    /// Nested Loop 内侧行数阈值
    nested_loop_inner_threshold: f64,  // 默认 10000
    /// 排序耗时占比阈值
    sort_time_ratio: f64,              // 默认 0.3
    /// 启用的规则 ID（空 = 全部启用）
    enabled_rules: Vec<String>,
    /// 禁用的规则 ID
    disabled_rules: Vec<String>,
}

impl DiagnosticEngine {
    fn analyze(&self, plan: &ExplainPlan) -> DiagnosticReport {
        let global_stats = self.compute_global_stats(plan);
        let mut findings = Vec::new();

        // 深度优先遍历所有节点
        self.walk_plan(plan, &global_stats, &mut findings);

        // 按严重度排序
        findings.sort_by(|a, b| b.severity.cmp(&a.severity));

        DiagnosticReport {
            findings,
            global_stats,
            summary: self.generate_summary(&findings),
        }
    }
}
```

---

## 7. 优化建议引擎设计

### 7.1 建议-诊断关联模型

```rust
struct Suggestion {
    /// 关联的诊断 Rule ID
    related_rules: Vec<String>,
    /// 建议类型
    category: SuggestionCategory,
    /// 建议内容
    message: String,
    /// 具体操作步骤
    actions: Vec<Action>,
    /// 置信度（0.0-1.0）
    confidence: f64,
}

enum SuggestionCategory {
    IndexOptimization,
    StatisticsUpdate,
    QueryRewrite,
    ConfigurationTuning,
    SchemaChange,
    DistributionOptimization,
}

enum Action {
    Sql { command: String, explanation: String },
    Config { parameter: String, suggested_value: String, explanation: String },
    Hint { hint_text: String, explanation: String },
    Generic { explanation: String },
}
```

### 7.2 建议映射表

| 触发规则 | 建议类型 | 建议内容 |
|---------|---------|---------|
| `SCAN-001` (大表全表扫描) | IndexOptimization | 在 `{relation}` 的 `{filter_columns}` 列上创建索引。`CREATE INDEX ON table(col);` |
| `SCAN-001` (大表全表扫描) | StatisticsUpdate | 对表 `{relation}` 执行 `ANALYZE` 更新统计信息 |
| `JOIN-001` (Nested Loop 大数据集) | QueryRewrite | 考虑使用 `SET enable_nestloop = off;` 或添加连接条件索引来避免 Nested Loop |
| `JOIN-002` (Hash 溢出) | ConfigurationTuning | 增大 `work_mem` 参数（当前 Hash 使用 `{mem}kB`，建议 >= `{suggested}kB`） |
| `JOIN-004` (Join 倾斜) | DistributionOptimization | 考虑使用 Replication 表或修改分布键以减少数据倾斜 |
| `MEM-001` (Sort 溢出磁盘) | ConfigurationTuning | 增大 `work_mem` 或 `sort_mem` 参数。当前 Sort 使用磁盘 `{space}kB` |
| `NET-001` (广播大表) | DistributionOptimization | 广播了 `{rows}` 行数据。考虑修改分布键或使用 Hash 重分布替代 |
| `EST-001` (行数低估) | StatisticsUpdate | 对相关表执行 `ANALYZE`。实际行数 `{actual}` vs 估算 `{estimated}`，偏差 `{ratio}x` |
| `EST-004` (Nested Loop 因低估) | StatisticsUpdate + IndexOptimization | 优化器因统计信息不足低估行数并选择 Nested Loop。执行 `ANALYZE {table}` 后重新规划 |
| `PUSH-001` (查询未下推) | DistributionOptimization | 检查 `enable_fast_query_shipping = on`，设置 `enable_unshipping_log = on` 查看未下推原因，检查是否使用了不可下推的函数 |
| `PUSH-002` (多层 Stream 嵌套) | QueryRewrite | 简化查询结构，检查分布键是否匹配 Join 条件，避免使用不可下推的语法 |
| `PUSH-004` (大表广播) | DistributionOptimization | 广播 `{rows}` 行，考虑修改分布键使 Join 无需广播，或将小表改为 Replication 表 |
| `TYPE-001` (疑似隐式类型转换) | QueryRewrite | 检查 Filter 中的列与常量类型是否一致，使用显式类型转换 `WHERE col = 'value'`，或创建表达式索引 |
| `TYPE-004` (LIKE 前缀通配符) | IndexOptimization | `LIKE '%xxx'` 无法使用 B-tree 索引，考虑使用全文搜索或 pg_trgm 扩展 |
| `TYPE-005` (函数包裹索引列) | IndexOptimization | `func(col) = value` 无法使用索引，改为 `col = reverse_func(value)` 或创建表达式索引 `CREATE INDEX ON table(func(col))` |
| `VEC-001` (混合引擎) | ConfigurationTuning | 检查 `enable_vector_engine` 是否开启，考虑对大表使用列存（CStore）以获得更好的向量化支持 |
| `VEC-005` (LLVM 未生效) | ConfigurationTuning | 检查 `enable_codegen` 是否开启，LLVM 代码生成对复杂表达式和大量数据处理有明显加速 |
| `SUBQ-001` (关联子查询) | QueryRewrite | 将子查询改写为 JOIN，或使用 LATERAL JOIN 替代 |
| `SUBQ-002` (IN 子查询未转换) | QueryRewrite | `IN (SubPlan)` 应被优化器转为 Semi Join，检查统计信息或使用 `= ANY (SELECT ...)` 语法 |
| `DIST-001` (分布键不匹配) | DistributionOptimization | Join 条件列不包含分布键导致重分布。`ALTER TABLE ... DISTRIBUTE BY HASH(join_col)` |
| `DIST-005` (数据倾斜未优化) | DistributionOptimization | 使用 `/*+ skew(table col) */` Hint，或执行 `ANALYZE` 让优化器基于统计信息检测倾斜 |
| `STORE-003` (DFS 文件未静态裁剪) | StatisticsUpdate | 对外表执行 `ANALYZE` 收集统计信息，使优化器能进行静态文件裁剪 |

### 7.3 综合推理（跨规则关联）

当多个诊断同时触发时，建议引擎进行综合推理：

```rust
fn synthesize_suggestions(findings: &[Finding]) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();

    // 模式 1：统计信息过时 → 多处估算偏差
    let estimation_issues = findings.iter()
        .filter(|f| f.rule_id.starts_with("EST-"))
        .count();
    if estimation_issues >= 2 {
        suggestions.push(Suggestion {
            category: StatisticsUpdate,
            message: "多处估算偏差表明统计信息可能过时，建议对所有相关表执行 ANALYZE".into(),
            confidence: 0.85,
            actions: vec![Action::Sql {
                command: "ANALYZE; -- 对所有涉及的表更新统计信息".into(),
                explanation: "过时的统计信息导致优化器做出错误的执行计划选择".into(),
            }],
            ..
        });
    }

    // 模式 2：全局内存压力 → 多处溢出
    let spill_issues = findings.iter()
        .filter(|f| f.rule_id == "MEM-001" || f.rule_id == "JOIN-002")
        .count();
    if spill_issues >= 2 {
        suggestions.push(Suggestion {
            category: ConfigurationTuning,
            message: format!("{}处内存溢出，建议增大 work_mem（当前多处 Hash/Sort 溢出到磁盘）", spill_issues),
            confidence: 0.9,
            ..
        });
    }

    // ... 更多模式
    suggestions
}
```

**新增的综合推理模式：**

```rust
// 模式 3：下推失败 + 网络开销（分布式特有）
let pushdown_issues = findings.iter()
    .filter(|f| f.rule_id.starts_with("PUSH-"))
    .count();
let network_issues = findings.iter()
    .filter(|f| f.rule_id.starts_with("NET-"))
    .count();
if pushdown_issues >= 1 && network_issues >= 1 {
    suggestions.push(Suggestion {
        category: DistributionOptimization,
        message: format!("查询未完全下推且存在多处网络传输({}处下推问题+{}处网络开销)，\
                          整体方案：1)启用enable_unshipping_log定位原因 2)调整分布键 3)简化不可下推的语法", 
                         pushdown_issues, network_issues),
        confidence: 0.85,
        ..
    });
}

// 模式 4：统计信息过时 + 下推失败（根因分析）
let has_est_issue = findings.iter().any(|f| f.rule_id.starts_with("EST-"));
let has_push_issue = findings.iter().any(|f| f.rule_id == "PUSH-001");
if has_est_issue && has_push_issue {
    suggestions.push(Suggestion {
        category: StatisticsUpdate,
        message: "统计信息过时可能同时导致估算偏差和下推失败（优化器无法确定最优分布策略）。\
                  执行 ANALYZE 后重新规划可能同时解决两类问题",
        confidence: 0.75,
        ..
    });
}

// 模式 5：隐式类型转换 + 索引未使用（根因关联）
let has_type_issue = findings.iter().any(|f| f.rule_id.starts_with("TYPE-"));
let has_scan_issue = findings.iter().any(|f| f.rule_id == "SCAN-001" || f.rule_id == "SCAN-004");
if has_type_issue && has_scan_issue {
    suggestions.push(Suggestion {
        category: QueryRewrite,
        message: "检测到隐式类型转换导致索引失效。修改 WHERE 条件中的常量类型与列类型一致，\
                  可能同时解决索引未使用和估算偏差问题",
        confidence: 0.8,
        ..
    });
}

// 模式 6：向量化不足 + 大数据量（列存建议）
let has_vec_issue = findings.iter().any(|f| f.rule_id.starts_with("VEC-"));
let large_data = /* global_stats.max_node_rows > 100000 */;
if has_vec_issue && large_data {
    suggestions.push(Suggestion {
        category: ConfigurationTuning,
        message: "分析型查询处理大量数据但未充分利用向量化引擎。\
                  建议：1)对分析表使用列存(CStore) 2)确认enable_vector_engine=on 3)确认enable_codegen=on",
        confidence: 0.75,
        ..
    });
}
```

---

## 8. 输出格式设计

### 8.1 CLI 输出

#### Text 格式（默认）

```
═══════════════════════════════════════════════════════════
  OpenGauss Execution Plan Analysis Report
═══════════════════════════════════════════════════════════

📋 Plan Overview
  Total Runtime: 1,523.456 ms
  Peak Memory:  256,784 KB
  Plan Depth:   5 levels, 12 nodes

🔴 Critical Issues (2)
──────────────────────────
  [JOIN-001] Nested Loop with large inner dataset
    Location:  Node #4 "Nested Loop" (line 8)
    Detail:    Inner side processed 500,000 rows × 1,000 loops
    Impact:    Estimated 500M comparisons, significant CPU overhead
    Fix:       SET enable_nestloop = off; or create index on join column

  [MEM-001] Sort spilled to disk
    Location:  Node #7 "Sort" (line 15)
    Detail:    Sort Method: external merge  Disk: 128,000kB
    Impact:    Disk I/O slows sort by 10-100x vs in-memory
    Fix:       Increase work_mem to at least 256MB

🟡 Warnings (3)
──────────────────
  [SCAN-001] Large table full scan on "orders" (line 12)
    Detail:    Seq Scan returned 1,000,000 rows
    Fix:       CREATE INDEX ON orders(status) WHERE status = 'active'

  [EST-001] Severe row underestimation at Node #4 (line 8)
    Detail:    Actual: 500,000 rows vs Estimated: 100 rows (5000x off)
    Fix:       ANALYZE orders;

  ...

💡 Optimization Suggestions
──────────────────────────
  1. [High Confidence] Run ANALYZE on tables: orders, lineitem
     → Updates statistics, likely fixes EST-001 and JOIN-001
  2. [High Confidence] Increase work_mem from 64MB to ≥256MB
     → Eliminates MEM-001 and JOIN-002 disk spills
  3. [Medium] Create index: CREATE INDEX ON orders(status)
     → Eliminates SCAN-001 full table scan
```

#### JSON 格式

```json
{
  "version": "1.0",
  "plan_summary": {
    "total_runtime_ms": 1523.456,
    "peak_memory_kb": 256784,
    "total_nodes": 12,
    "max_depth": 5
  },
  "findings": [
    {
      "rule_id": "JOIN-001",
      "severity": "critical",
      "category": "JoinStrategy",
      "node": { "line": 8, "type": "Nested Loop", "node_id": 4 },
      "title": "Nested Loop with large inner dataset",
      "detail": "Inner side processed 500000 rows × 1000 loops",
      "suggestion": {
        "category": "QueryRewrite",
        "actions": [
          {
            "type": "config",
            "parameter": "enable_nestloop",
            "value": "off",
            "explanation": "Disable nested loop to force hash/merge join"
          }
        ]
      }
    }
  ],
  "suggestions": [...]
}
```

### 8.2 CLI 接口设计

```
ogexplain-analyzer [OPTIONS] [FILE]

Arguments:
  [FILE]  EXPLAIN output file (use - for stdin)

Options:
  -f, --format <FORMAT>          Input format [default: text]
                                 [possible: text, json, xml, yaml]
  -o, --output <FORMAT>          Output report format [default: text]
                                 [possible: text, json, markdown, html]
  --threshold <LEVEL>            Minimum severity to report [default: info]
                                 [possible: critical, warning, info]
  --enable-rules <RULES>         Only enable specific rules (comma-separated)
  --disable-rules <RULES>        Disable specific rules (comma-separated)
  --config <FILE>                Configuration file path
  --large-table-threshold <N>    Large table row threshold [default: 10000]
  --memory-threshold-kb <N>      Memory alert threshold in KB [default: 102400]
  --estimation-skew <N>          Estimation skew factor [default: 100]
  --no-suggestions               Disable optimization suggestions
  --show-plan                    Include parsed plan tree in output
  --quiet                        Only show critical findings
  -v, --verbose                  Verbose output with full details
  -h, --help                     Print help
  -V, --version                  Print version

Examples:
  # Analyze from file
  ogexplain-analyzer explain_output.txt

  # Pipe from database
  gsql -c "EXPLAIN ANALYZE SELECT ..." | ogexplain-analyzer -

  # JSON output for programmatic use
  ogexplain-analyzer -o json explain_output.txt

  # Only critical warnings
  ogexplain-analyzer --threshold critical explain_output.txt

  # Custom thresholds
  ogexplain-analyzer --large-table-threshold 100000 --memory-threshold-kb 512000
```

---

## 9. 技术选型建议

### 9.1 推荐语言：Rust

| 考量维度 | Rust | Go | Python | TypeScript |
|---------|------|----|--------|------------|
| 单二进制分发 | ✅ cargo build | ✅ go build | ❌ 需运行时 | ❌ 需运行时 |
| 解析性能 | ✅ 零拷贝解析 | ✅ 良好 | ⚠️ 一般 | ⚠️ 一般 |
| 类型安全 AST | ✅ enum + pattern match | ⚠️ 无代数类型 | ⚠️ 运行时类型 | ✅ 但运行时 |
| 规则引擎表达力 | ✅ trait 系统 | ✅ interface | ✅ duck typing | ✅ interface |
| 生态成熟度 | ✅ nom/pest/combine | ✅ 良好 | ✅ 最丰富 | ✅ 良好 |
| 学习曲线 | ⚠️ 较高 | ✅ 低 | ✅ 低 | ✅ 低 |

**最终推荐：Rust**

理由：
- 枚举系统完美匹配节点类型和 Join 类型建模
- 模式匹配极大简化节点分类逻辑
- 单二进制分发，用户无需安装运行时
- 零开销抽象，解析性能优异
- trait 系统天然适合规则引擎的插件化设计

**备选方案：Go** — 如果团队对 Rust 不熟悉，Go 的开发效率更高，编译速度快，但缺少代数类型系统需要用 `string` 或 `interface{}` 模拟。

### 9.2 依赖库选型（Rust 方案）

| 用途 | 推荐库 | 说明 |
|------|--------|------|
| CLI 框架 | `clap` v4 | 成熟、功能完整的命令行解析 |
| 正则 | `regex` | 高性能正则引擎 |
| JSON 序列化 | `serde` + `serde_json` | 标准 JSON 处理 |
| 终端颜色输出 | `colored` 或 `console` | 彩色报告输出 |
| 配置文件 | `toml` | TOML 格式配置 |
| 错误处理 | `anyhow` + `thiserror` | 应用级 + 库级错误 |
| 测试 | 内置 `#[test]` + `insta` | 快照测试，适合验证解析结果 |

### 9.3 备选方案：Python

如需快速原型验证，Python 方案：

| 用途 | 推荐库 |
|------|--------|
| CLI | `click` 或 `typer` |
| 解析 | 手写解析器（`re` + 行扫描） |
| 数据模型 | `pydantic` v2 |
| 测试 | `pytest` |
| 打包 | 可用 `pyinstaller` 打包为单文件 |

---

## 10. 项目结构（Rust 方案）

```
ogexplain-analyzer/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── config.example.toml
│
├── src/
│   ├── main.rs                  # CLI 入口
│   ├── lib.rs                   # Library 入口
│   │
│   ├── parser/                  # 解析层
│   │   ├── mod.rs
│   │   ├── text.rs              # TEXT 格式解析器
│   │   ├── json.rs              # JSON 格式解析器（二期）
│   │   ├── xml.rs               # XML 格式解析器（二期）
│   │   ├── yaml.rs              # YAML 格式解析器（二期）
│   │   ├── line_classifier.rs   # 行分类器
│   │   └── tree_builder.rs      # 树构建器
│   │
│   ├── model/                   # 数据模型
│   │   ├── mod.rs
│   │   ├── plan.rs              # ExplainPlan, PlanNode, PlanSummary
│   │   ├── node_type.rs         # NodeType enum, NodeTypeCategory
│   │   ├── join_type.rs         # JoinType enum
│   │   ├── streaming.rs         # StreamingType enum
│   │   ├── cost.rs              # EstimatedCost, ActualStats
│   │   ├── buffer.rs            # BufferStats
│   │   └── property.rs          # NodeProperty
│   │
│   ├── analyzer/                # 分析层
│   │   ├── mod.rs               # DiagnosticEngine
│   │   ├── context.rs           # PlanContext, GlobalStats
│   │   ├── config.rs            # DiagnosticConfig
│   │   ├── report.rs            # DiagnosticReport, Finding
│   │   └── rules/               # 规则实现
│   │       ├── mod.rs           # Rule trait + 注册
│   │       ├── scan_rules.rs    # SCAN-001 ~ SCAN-005
│   │       ├── join_rules.rs    # JOIN-001 ~ JOIN-005
│   │       ├── memory_rules.rs  # MEM-001 ~ MEM-004
│   │       ├── sort_rules.rs    # SORT-001 ~ SORT-003
│   │       ├── network_rules.rs # NET-001 ~ NET-004
│   │       ├── estimation_rules.rs # EST-001 ~ EST-004
│   │       └── general_rules.rs # GEN-001 ~ GEN-004
│   │
│   ├── suggester/               # 建议引擎
│   │   ├── mod.rs               # SuggestionEngine
│   │   ├── suggestion.rs        # Suggestion, Action, SuggestionCategory
│   │   ├── mapper.rs            # 规则-建议映射
│   │   └── synthesizer.rs       # 跨规则综合推理
│   │
│   └── reporter/                # 输出层
│       ├── mod.rs               # Reporter trait
│       ├── text.rs              # 终端文本输出（彩色）
│       ├── json.rs              # JSON 输出
│       ├── markdown.rs          # Markdown 输出
│       └── html.rs              # HTML 输出（可选）
│
├── tests/                       # 集成测试
│   ├── fixtures/                # EXPLAIN 输出样例
│   │   ├── seq_scan.txt
│   │   ├── hash_join.txt
│   │   ├── nested_loop.txt
│   │   ├── sort_overflow.txt
│   │   ├── streaming.txt
│   │   ├── vector_plan.txt
│   │   └── complex_plan.txt
│   ├── parser_tests.rs
│   ├── analyzer_tests.rs
│   └── end_to_end_tests.rs
│
└── benches/                     # 性能基准
    └── parsing_bench.rs
```

---

## 11. 配置文件格式

```toml
# ogexplain-analyzer 配置文件

[thresholds]
# 大表行数阈值
large_table_rows = 10000
# 内存告警阈值 (KB)
memory_kb = 102400
# 峰值内存阈值 (KB)
peak_memory_kb = 1048576
# 估算偏差因子
estimation_skew_factor = 100
# Nested Loop 内侧行数阈值
nested_loop_inner_rows = 10000
# 排序耗时占比阈值
sort_time_ratio = 0.3
# 计划树最大深度
max_plan_depth = 10

[rules]
# 禁用特定规则
disable = []
# 仅启用特定规则（空 = 全部启用）
enable = []

[output]
# 默认输出格式
format = "text"
# 最低报告严重度
min_severity = "info"
# 是否显示建议
show_suggestions = true
# 是否显示解析后的计划树
show_plan = false

[colors]
enabled = true
```

---

## 12. 测试策略

### 12.1 解析器测试

每个测试用例包含：
1. 输入 EXPLAIN 文本
2. 期望的 AST 结构
3. 解析应产生的 Finding 数量

```
tests/fixtures/
├── basic_seq_scan.txt          → 简单 Seq Scan
├── index_scan_with_filter.txt  → Index Scan + Filter
├── hash_join_basic.txt         → 基本 Hash Join
├── hash_join_spill.txt         → Hash 溢出 (Batches > 1)
├── nested_loop_large.txt       → Nested Loop 大数据集
├── sort_external_merge.txt     → Sort 溢出到磁盘
├── multi_level_join.txt        → 多层 Join 嵌套
├── streaming_redistribute.txt  → 分布式流
├── vector_plan.txt             → 向量化计划
├── partitioned_scan.txt        → 分区表扫描
├── cstore_scan.txt             → 列存扫描
├── merge_stmt.txt              → MERGE 语句
├── pretty_mode.txt             → Pretty 模式输出
├── json_format.txt             → JSON 格式输出
└── full_analyze.txt            → 完整 ANALYZE 输出
```

### 12.2 规则测试

每条规则至少 2 个测试：
1. **正面测试**：应触发该规则的 EXPLAIN 输入
2. **负面测试**：不应触发该规则的 EXPLAIN 输入

### 12.3 回归测试

收集真实 OpenGauss EXPLAIN 输出，确保解析不遗漏节点类型。

---

## 13. 实施路线

### Phase 1 — MVP（2-3 周）

1. TEXT 格式解析器（行分类器 + 树构建器）
2. 核心数据模型
3. 20 条最关键的诊断规则（原有 10 条 + PUSH-001/002, TYPE-001/002/004, VEC-001/005, SUBQ-001/002, DIST-001/005, STORE-003）
4. Text + JSON 输出
5. CLI 基本功能
6. 测试用例（至少 10 个 fixture）

### Phase 2 — 完善（2-3 周）

1. 剩余诊断规则（全部 45+ 条覆盖）
2. 优化建议引擎（含 6 种跨规则综合推理模式）
3. 隐式类型转换的启发式检测（无需元数据的纯文本模式匹配）
4. 下推失败的根因分析（对应 `pgxcship.cpp` 的 ShippabilityStat 映射）
5. Markdown 输出
6. 配置文件支持
7. 更多 fixture 和边界情况处理

### Phase 3 — 扩展（可选）

1. JSON/XML/YAML 输入格式解析
2. HTML 可视化报告
3. 计划对比功能（两个 EXPLAIN 的 diff）
4. 历史趋势分析（多次 EXPLAIN 的对比）
5. 自定义规则插件系统

---

## 附录 A：OpenGauss vs PostgreSQL EXPLAIN 差异速查

| 特性 | PostgreSQL | OpenGauss |
|------|-----------|-----------|
| 节点名称 | Result | Result / BaseResult |
| 向量化节点 | 无 | Vector * 系列（30+ 种） |
| 列存节点 | 无 | CStore * 系列 |
| 时序存储 | 无 | TsStore Scan |
| 向量索引 | 无 | Ann Index Scan |
| 分布式流 | 无 | Streaming(type: *) |
| 分布式扫描 | 无 | Data Node Scan, RemoteQuery |
| Sonic Hash | 无 | Vector Sonic Hash Join/Aggregate |
| 分区变体 | 无 | Partitioned * 前缀（20+ 种） |
| 预测信息 | 无 | p-time=, p-rows= |
| 去重估算 | 无 | distinct=[outer, inner] |
| 运行时摘要 | Planning Time + Execution Time | Total runtime（含 Peak Memory） |
| Pretty 模式 | 无 | 节点 ID + 详细运行时统计 |
| Merge 语句 | 无（PG 15+ 有） | Merge / Vector Merge |
| Bloom Filter | 无 | Bloom Filter 信息 |
| LLVM 优化 | 无 | LLVM optimization 标记 |
| DOP 信息 | 无 | dop: consumer/producer |
| NodeGroup | 无 | ng: group1→group2 |
| Bloom Filter 属性 | 无 | `Generate Bloom Filter On Expr/Index`、`Filter By Bloom Filter On Expr/Index` |
| Min/Max 过滤 | 无 | `(min max skip: rows N strides N/M stripes N/M files N/M)` |
| DFS 文件裁剪 | 无 | `(pruned files: static N dynamic N)` |
| LLVM 优化 | 无 | `(LLVM Optimized)` |
| Skew 优化 | 无 | `Skew Join/Agg Optimized by Hint/Rule/Statistic` |
| Sonic Hash | 无 | `Vector Sonic Hash Join/Aggregate` |
| 向量化适配器 | 无 | `Row Adapter` / `Vector Adapter` |
| 子查询类型 | 相同 | `SubPlan N`（关联）/ `InitPlan N`（一次性） |
| 递归查询 | 无 | `<<ruid:[N] ctlid:[N]>>`、`stream_level:N` |
| CPU 详细信息 | 无 | `(CPU: ex c/r=N, ex row=N, ex cyc=N, inc cyc=N)` |
| Dynamic SMP | 无 | `Avail/Max core`、`Final dop` |
| AI 预测信息 | 无 | `p-time=N`、`p-rows=N` |

## 附录 C：OG 特有性能问题速查

| 问题 | EXPLAIN 中的特征 | 源码位置 | 严重度 |
|------|-----------------|---------|--------|
| **查询未下推** | 出现 `Streaming(type: REDISTRIBUTE/BROADCAST)` | `pgxcship.cpp`, `planner.cpp` | 🔴 Critical |
| **隐式类型转换** | `Seq Scan` + `Filter` 在有索引的列上，高 `Rows Removed` | `indxpath.cpp:2380`, `parse_coerce.cpp` | 🔴 Critical |
| **向量化未启用** | 无 `Vector *` 节点 + 大数据量 Scan/Join | `optcommon.cpp` | 🟡 Warning |
| **LLVM 未生效** | 无 `(LLVM Optimized)` 标记 | `explain.cpp:4661` | 🟢 Info |
| **数据倾斜** | DN 间行数差异 > 5x，无 `Skew Optimized` 标记 | `dataskew.h` | 🔴 Critical |
| **关联子查询** | `SubPlan N` 嵌套在 Filter 中 | `explain.cpp:3426` | 🟡 Warning |
| **IN 子查询未优化** | `Filter: (col IN (SubPlan N))` | `explain.cpp:3426` | 🔴 Critical |
| **分布键不匹配** | Join 下方出现 `Streaming(type: REDISTRIBUTE)` | `execStream.cpp:268` | 🟡 Warning |
| **Bloom Filter 无效** | `(skip N rows by bloom filter)` 中 N 占比低 | `explain.cpp:8032` | 🟢 Info |
| **大表广播** | `Streaming(type: BROADCAST)` + 高行数 | `execStream.cpp` | 🔴 Critical |
| **Sonic Hash 溢出** | `Vector Sonic Hash *` + Partition Spill | `explain.cpp:5362` | 🟡 Warning |
| **递归查询迭代多** | `Recursive Union` + `Iteration times: N` (N > 100) | `explain.cpp:5699` | 🟡 Warning |

## 附录 D：下推失败原因码映射

| ShippabilityStat 枚举值 | 含义 | 常见触发场景 |
|------------------------|------|------------|
| `SS_UNSHIPPABLE_EXPR` | 包含不可下推表达式 | 复杂表达式、子链接 |
| `SS_NEED_SINGLENODE` | 需要单节点计算 | 窗口函数、排序聚合 |
| `SS_NEEDS_COORD` | 需要 Coordinator | 系统目录查询、全局临时表 |
| `SS_NO_NODES` | 无可用 DN | 表在无 DN 的 NodeGroup 上 |
| `SS_UNSUPPORTED_EXPR` | 不支持的表达式 | 特殊操作符 |
| `SS_HAS_AGG_EXPR` | 聚合表达式问题 | 多个 COUNT(DISTINCT) |
| `SS_UNSHIPPABLE_TYPE` | 不可下推的数据类型 | 特殊类型如 cursor、record |
| `SS_UNSHIPPABLE_TRIGGER` | 不可下推的触发器 | 语句级触发器 |
| `SS_UNSHIPPABLE_FUNCTION` | 不可下推的函数 | `nextval()`, `random()`, `currval()` |
| `SS_NEED_NO_CSTORE` | 列存阻止下推 | 列存与行存混合查询 |
| `SS_UNSHIPPABLE_UDF` | 不可下推的 UDF | 非 SHIPPING 类型的用户自定义函数 |

**如何获取未下推原因**（不通过本工具，直接在数据库中）：

```sql
-- 方法 1：开启未下推日志
SET enable_unshipping_log = on;
-- 执行查询后查看 gs_log 中的 "SQL can't be shipped, reason: ..."

-- 方法 2：使用 SQL Advisor（plananalyzer.cpp）
-- 通过 gs_wlm_session_info 视图查看
SELECT query, query_plan FROM gs_wlm_session_info WHERE query_id = <id>;
```

```
# 节点行（带 -> 前缀）
^\s{2,}(?:->\s+)([A-Z][^\(]+?)(?:\s+on\s+(\S+))?\s*(?:\(cost=([^\)]+)\))?(?:\s*(?:\(actual\s+([^\)]+)\)))?

# 根节点行（无 -> 前缀）
^([A-Z][^\(]+?)(?:\s+on\s+(\S+))?\s*(?:\(cost=([^\)]+)\))?(?:\s*(?:\(actual\s+([^\)]+)\)))?

# Cost 信息
cost=([\d.]+)\.\.([\d.]+)\s+rows=([\d.]+)\s+width=(\d+)

# Actual 信息（含 time）
actual\s+time=([\d.]+)\.\.([\d.]+)\s+rows=([\d.]+)\s+loops=([\d.]+)

# Actual 信息（仅 rows）
actual\s+rows=([\d.]+)\s+loops=([\d.]+)

# 属性行
^\s+([A-Z][A-Za-z :]+):\s+(.+)$

# Sort Method
Sort Method:\s+(\S+(?:\s+\S+)?)\s{2}(\w+):\s+(\d+)kB

# Hash Buckets（含 original）
Buckets:\s+(\d+)\s+\(originally\s+(\d+)\)\s+Batches:\s+(\d+)\s+\(originally\s+(\d+)\)\s+Memory Usage:\s+(\d+)kB

# Hash Buckets（简单）
Buckets:\s+(\d+)\s+Batches:\s+(\d+)\s+Memory Usage:\s+(\d+)kB

# Buffer 信息
Buffers:(.*)\)

# Streaming 类型
(?:Vector\s+)?Streaming\(type:\s+([^)]+)\)

# Total runtime
Total runtime:\s+([\d.]+)\s+ms

# Peak Memory
Peak Memory:\s+(\d+)\s+KB

# === OG 特有诊断正则 ===

# SubPlan / InitPlan
(?:SubPlan|InitPlan)\s+(\d+)

# Skew 优化标记
Skew\s+(Join|Agg)\s+Optimized\s+by\s+(Hint|Rule|Statistic)

# LLVM 优化标记
\(LLVM\s+Optimized\)

# Bloom Filter 属性
(?:Generate|Filter By)\s+Bloom\s+Filter\s+On\s+(Expr|Index)

# Min/Max 过滤
\(min max skip:.*?\)

# DFS 文件裁剪
pruned files:\s+static\s+(\d+)\s+dynamic\s+(\d+)

# Bloom Filter 跳过行数
\(skip\s+(\d+)\s+rows\s+by\s+bloom\s+filter

# Row/Vector Adapter
^(Row|Vector)\s+Adapter

# 递归查询标记
<<ruid:\[(\d+)\]\s+ctlid:\[(\d+)\]>>
stream_level:(\d+)

# CPU 详细信息
\(CPU:\s+ex\s+c/r=([\d.]+),\s+ex\s+row=(\d+),\s+ex\s+cyc=([\d.]+),\s+inc\s+cyc=([\d.]+)\)

# 隐式类型转换启发式（在 Filter/Index Cond 中匹配）
# 数字字面量与可能字符串列的等值比较
(\w+)\s*=\s*(\d+(?:\.\d+)?)\b(?!.*::)
# LIKE 前缀通配符
(\w+)\s+LIKE\s+'%[^']*'
# 函数包裹列
(?:to_char|to_date|to_number|substr|trim|lower|upper)\s*\(\s*(\w+)

# Remote Query (FQS)
Data\s+Node\s+Scan
Remote\s+query:\s+(.+)
Coordinator\s+quals:\s+(.+)

# Vector Sonic Hash
Vector\s+Sonic\s+Hash\s+(Join|Aggregate)

# 递归查询迭代次数
Iteration\s+times:\s+(\d+)
```
