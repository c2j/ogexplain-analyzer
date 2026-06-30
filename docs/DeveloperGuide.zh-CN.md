# ogexplain-analyzer 开发者指南

本文档面向希望**扩展**、**集成**或**深度定制** ogexplain-analyzer 的 Rust 开发者。涵盖架构原理、公共 API 参考、扩展模式、集成示例以及构建测试策略。

> **前置条件：** 熟悉 Rust 2021 edition、Cargo 工作空间、Serde 序列化、trait 系统。不涉及用户操作说明（参见 `UserGuide.zh-CN.md`）和贡献流程（参见 `CONTRIBUTING.zh-CN.md`）。

---

## 目录

1. [架构深入](#1-架构深入)
2. [核心 API 参考](#2-核心-api-参考)
3. [MCP 服务器开发](#3-mcp-服务器开发)
4. [添加新功能](#4-添加新功能)
5. [集成示例](#5-集成示例)
6. [构建与测试](#6-构建与测试)

---

## 1. 架构深入

### 1.1 工作空间结构

6 个 crate 共享 `version = "0.2.0"` 和 `edition = "2021"`：

```
ogexplain-analyzer/          # 工作空间根（虚拟 manifest）
├── crates/
│   ├── ogexplain-core/      # 纯库：解析器 + 模型 + 分析器 + 建议引擎 + 改写器
│   ├── ogexplain-optimizer/ # 闭环优化器：编排 + 收敛 + 与 metamorphosis 的重写/验证集成
│   ├── ogexplain-cli/       # CLI 二进制 (ogexplain)
│   ├── ogexplain-tui/       # TUI 二进制 (ogexplain-tui)
│   ├── ogexplain-mcp/       # MCP 服务器 (ogexplain-mcp)
│   └── ogsql-complexity/    # SQL 复杂度评分（独立可复用库）
├── tests/
│   ├── fixtures/            # EXPLAIN TEXT 测试用例（31 个）
│   ├── integration_tests.rs # 解析器 insta 快照测试
│   ├── analyzer_tests.rs    # 诊断规则测试
│   └── regress_optimize/    # 优化器回归测试用例
└── Cargo.toml               # 工作空间 manifest + feature 门控
```

**为什么 6 个 crate？** 关注点分离 + 依赖隔离。`ogexplain-core` 零 IO/UI 依赖，可嵌入任何 Rust 项目（Web 服务、WASM、CLI）。`ogexplain-optimizer` 持有 metamorphosis 和 Z3 依赖，与 core 解耦。`ogsql-complexity` 独立于 EXPLAIN 解析，可单独用于 SQL 审计。每个前端 crate 只引入自己需要的依赖。

### 1.2 数据流

```
EXPLAIN TEXT ──► parse() ──► ExplainPlan
                    │
                    ├──► analyze() ──► DiagnosticReport
                    ├──► heatmap() ──► Option<PlanHeatmap>
                    ├──► waterfall() ──► Option<PlanWaterfall>
                    └──► analyze_with_rewrite(plan, sql) ──► DiagnosticReport
                                                                  │
                    SuggestionEngine::suggest(findings) ◄────────┘
                              │
                              ▼
                    输出格式：text / json / heatmap / waterfall / csv
```

**关键设计决策：** 每个 `analyze_*` 函数都是纯函数——接收不可变引用，返回新分配的结果。同一 `ExplainPlan` 可以被多次分析（不同配置）而无副作用。

### 1.3 设计原则

| 原则 | 实现方式 | 原因 |
|------|---------|------|
| **core 纯库** | `ogexplain-core` 不依赖 ratatui、clap、crossterm 等 | 任何项目可引入，不拉入不需要的 UI 依赖 |
| **全模型 Serialize** | 所有 model 类型 `#[derive(Serialize)]` | JSON 输出、MCP 响应、快照测试都依赖序列化 |
| **DiagnosticRule trait** | 25 条规则各自独立实现同一个 trait | 独立开发、测试、禁用；新增不影响现有代码 |
| **TUI TEA 架构** | 事件 → Action → Model 变更 → 重绘 | 状态和渲染严格分离 |
| **MCP rmcp SDK** | `#[tool]` 属性宏 + `tool_router` | 声明式定义工具，自动处理 JSON-RPC |
| **`#[non_exhaustive]`** | heatmap/waterfall 类型 | 允许未来添加字段而不破坏下游 |

### 1.4 解析器两阶段设计

**阶段 1 — 行分类器**（`line_classifier.rs`）：逐行匹配正则，分类为 `NodeType`、缩进、成本/实际统计、属性键值对。

**阶段 2 — 树构建器**（`tree_builder.rs`）：基于缩进级别的栈算法。新行入栈时根据缩进差确定父子关系。处理 pretty 模式 `N --` 前缀。解析器返回 `Result<_, ParseError>`，永不 panic。

### 1.5 TUI 架构

采用 Elm/TEA 架构：

```
事件（按键/终端）→ Event::convert() → Action → App::update(action)
                                                       │
                                                App::view(frame)
                                                       │
                                          ┌────────────┼────────────┐
                                          ▼            ▼            ▼
                                     TreePanel   DetailPanel   InputPanel
```

关键类型：`AppMode`（Input/Browse）、`FocusTarget`（Tree/Detail/Input，Tab 循环）、`Action` 枚举（Parse/Quit/MoveUp/Expand/CycleFocus 等）。

### 1.6 依赖图

```
ogexplain-core ← ogexplain-cli
               ← ogexplain-tui
               ← ogexplain-optimizer
               ← ogexplain-mcp ← ogsql-complexity

ogsql-complexity ← ogexplain-cli, ogexplain-mcp
ogexplain-optimizer → metamorphosis-core, metamorphosis-rewrite, z3 (via QED)
```

`ogexplain-core` 依赖极少（`regex`、`serde`、`thiserror`、`toml`、`rust-i18n`、`ogsql-parser`）。

---

## 2. 核心 API 参考

### 2.1 顶层函数

```rust
use ogexplain_core::{parse, parse_multi, analyze, analyze_with_config,
                     analyze_with_rewrite, heatmap, waterfall};
```

| 函数 | 签名 | 说明 |
|------|------|------|
| `parse` | `(text: &str) -> Result<ExplainPlan, ParseError>` | 解析单条 EXPLAIN 文本 |
| `parse_multi` | `(text: &str) -> Result<Vec<ExplainPlan>, ParseError>` | 解析多条 EXPLAIN 的混合文本 |
| `analyze` | `(plan: &ExplainPlan) -> DiagnosticReport` | 默认配置分析（25 条规则） |
| `analyze_with_config` | `(plan: &ExplainPlan, config: &DiagnosticConfig) -> DiagnosticReport` | 自定义配置分析 |
| `analyze_with_rewrite` | `(plan: &ExplainPlan, sql_text: Option<&str>) -> DiagnosticReport` | 分析 + SQL 改写（SUBQ-006） |
| `heatmap` | `(plan: &ExplainPlan) -> Option<PlanHeatmap>` | 成本偏差热力图（需 ANALYZE 数据） |
| `waterfall` | `(plan: &ExplainPlan) -> Option<PlanWaterfall>` | 资源瀑布图（需 ANALYZE 数据） |

`analyze_with_rewrite` 内部：先 `analyze(plan)`，再用 `ogsql_parser` 解析 SQL，检测关联子查询自更新模式，匹配则调用 `rewriter::transform::rewrite_update_from()` 生成改写 SQL 注入 SUBQ-006 的 `sql_rewrite` 字段。

### 2.2 核心模型类型

**`ExplainPlan`** — 解析顶层，`root: PlanNode` + `summary: Option<PlanSummary>`（汇总行可能不存在）。

**`PlanSummary`** — `total_runtime_ms`、`peak_memory_kb`、`planner_runtime_ms`、`query_id` 等，全为 `Option` 字段。

**`PlanNode`** — 不可变树结构，关键字段：

```rust
pub struct PlanNode {
    pub node_type: NodeType,
    pub relation: Option<String>,          // 表名（扫描节点有值）
    pub join_type: Option<JoinType>,       // Inner/Left/Right/Full
    pub estimated: Option<EstimatedCost>,  // 优化器估算（始终有值）
    pub actual: Option<ActualStats>,       // 仅 EXPLAIN ANALYZE
    pub properties: Vec<NodeProperty>,     // 原始键值对
    pub structured_props: Option<NodeProperties>, // 解构后的属性
    pub buffers: Option<BufferStats>,
    pub children: Vec<PlanNode>,
    pub line_number: usize,                // 诊断定位用
}
```

**`NodeProperties`**（从 `properties` 提取的结构化属性）— `rows_removed_by_filter`、`sort_method`、`sort_disk`、`hash_buckets`、`hash_batches`、`peak_memory_kb` 等。规则代码应优先使用此字段而非手动遍历 `properties`。

**`NodeType`** — 80+ 变体枚举，按类别分为扫描（14 种）、连接（8 种）、聚合（10 种）、排序（3 种）、DML（9 种）、Streaming（参数化 `Streaming(StreamingType)`/`VectorStreaming`）、适配器（`RowAdapter`/`VectorAdapter`）、兜底（`Unknown(String)`）。

- `NodeType::category()` → `NodeTypeCategory`（Scan/Join/Aggregate/Sort/Dml/SetOp/Auxiliary/Streaming/Other）
- `Unknown(String)` 保证解析器永不因未知节点类型失败

**`EstimatedCost`** — `startup_cost`、`total_cost`、`plan_rows`、`plan_width`、`pred_time`（AI 预测）、`pred_rows`、`distinct`。

**`ActualStats`** — `startup_time_ms`、`total_time_ms`、`rows`、`loops`、`executed: bool`（false = never executed）。

**`BufferStats`** — `shared_hit/read`、`temp_read/written`、`io_read_time_ms` 等。`temp_read` 非零通常意味着排序/哈希溢出。

规则中的空安全访问模式：

```rust
if let Some(actual) = &node.actual {
    if actual.executed {
        // 安全使用 actual.rows, actual.total_time_ms
    }
}
```

### 2.3 分析器类型

**`DiagnosticConfig`** — 可配置阈值 + 禁用规则：

```rust
pub struct DiagnosticConfig {
    pub large_table_rows: f64,           // 大表行数阈值（默认 10000.0）
    pub memory_threshold_kb: f64,        // 内存阈值 KB（默认 102400.0 = 100MB）
    pub estimation_skew_factor: f64,     // 估算偏差因子（默认 100.0）
    pub nested_loop_inner_rows: f64,     // 嵌套循环内侧行数（默认 10000.0）
    pub sort_time_ratio: f64,            // 排序时间占比（默认 0.3）
    pub max_plan_depth: usize,           // 最大计划深度（默认 10）
    pub disabled_rules: Vec<String>,     // 禁用的规则 ID
}
```

```rust
let config = DiagnosticConfig {
    large_table_rows: 100_000.0,
    disabled_rules: vec!["TYPE-001".into()],
    ..Default::default()
};
let report = analyze_with_config(&plan, &config);
```

**`DiagnosticEngine`** — `new(config)` 构建规则列表（`disabled_rules` 在此过滤），`analyze(plan)` 计算 `GlobalStats` → DFS 遍历每节点执行每条规则的 `check()` → 执行所有规则的 `check_global()`。

**`Finding`** — `rule_id`、`severity`（Critical/Warning/Info，实现 Ord）、`category`（DiagnosticCategory 12 变体）、`title`、`detail`、`node_line`、`node_type`、`suggestion`、`sql_rewrite`、`evidence`。

**`DiagnosticReport`** — `findings: Vec<Finding>` + `stats: GlobalStats`（含 `max_node_time_ms`、`total_nodes`、`max_depth`）。

### 2.4 热力图类型

```rust
pub struct PlanHeatmap {
    pub entries: Vec<HeatmapEntry>,
    pub critical_path: Vec<usize>,    // 最大偏差路径行号
    pub hotspots: Vec<usize>,         // 严重节点行号（按 Q-Error 降序）
    pub summary: HeatmapSummary,
}
```

**Q-Error**：`max(actual, estimated) / min(actual, estimated)`，VLDB 标准估算准确性指标，对称且 >= 1.0。五级 `DeviationSeverity`：Negligible(< 2x)、Mild(2–5x)、Moderate(5–10x)、Severe(10–50x)、Extreme(>= 50x)。

`HeatmapEntry` 含 `subtree_geo_qerror`（子树几何平均）和 `path_cumulative_qerror`（根到当前累计），用于识别偏差传播路径。

### 2.5 瀑布图类型

```rust
pub struct PlanWaterfall {
    pub entries: Vec<WaterfallEntry>,
    pub bottlenecks: BottleneckSummary,  // Top 5 CPU + Top 5 内存
    pub total_nodes: usize,
    pub nodes_with_stats: usize,
}
```

`WaterfallEntry` 含 `cpu_time_ms`（= total_time_ms × loops）、`peak_memory_kb`、`cpu_percent`/`memory_percent`、`is_bottleneck`、`has_memory_spill`。

### 2.6 建议引擎

```rust
pub struct Suggestion {
    pub related_rules: Vec<String>,
    pub category: SuggestionCategory,  // IndexOptimization / StatisticsUpdate / QueryRewrite / ConfigurationTuning / DistributionOptimization
    pub message: String,
    pub confidence: f64,  // 0.0–1.0
}
```

`SuggestionEngine::suggest(&findings)` 跨规则综合：统计信息过期（>=2 EST-*，0.85）、work_mem 不足（>=2 溢出，0.9）、复合索引（SCAN+JOIN，0.8）、下推问题（PUSH-*，0.75）、子查询改写（SUBQ-006，0.9）、类型一致性（>=2 TYPE-*，0.85）、引擎统一（VEC-*，0.8）。

### 2.7 SQL 改写与批量报告

**`RewriteResult`** — `strategy: RewriteStrategy`（目前仅 UpdateFrom）、`rewritten_sql`、`explanation`、`pattern_info: AntiPatternInfo`（含 `target_table`、`correlation_columns`、`set_columns`）。

**`SummaryRow`** — 43 字段扁平记录（SQL 复杂度 + 执行计划 + 诊断统计），通过 `SummaryRow::compute(&plan, &diag, complexity_input)` 计算，用于 CSV 导出和批量汇总表。

---

## 3. MCP 服务器开发

### 3.1 架构

基于 `rmcp` 1.7 SDK，stdio 传输：

```rust
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let server = OgexplainServer;
        let transport = rmcp::transport::io::stdio();
        rmcp::serve_server(server, transport).await
    });
}
```

### 3.2 服务器结构与工具路由

```rust
#[derive(Debug, Clone, Default)]
pub struct OgexplainServer;

#[tool_router(server_handler)]
impl OgexplainServer {
    #[tool(name = "analyze_explain", description = "...")]
    async fn analyze_explain(
        &self,
        Parameters(params): Parameters<AnalyzeExplainParams>,
    ) -> Result<CallToolResult, ErrorData> { ... }
}
```

`#[tool_router(server_handler)]` 自动生成 `ServerHandler` trait 实现，`#[tool]` 处理方法注册。

### 3.3 参数与响应

参数使用 `Deserialize + JsonSchema`，`schemars` 为 MCP 客户端生成 JSON Schema：

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeExplainParams {
    pub explain_text: String,
    #[serde(default)]
    pub sql_text: Option<String>,  // 可选，启用 SQL 改写
}
```

响应使用双格式——`Content::json()` 提供结构化 JSON（机器消费），`Content::text()` 提供人类可读摘要（AI 助手消费）：

```rust
Ok(CallToolResult::success(vec![
    Content::json(&report)?,
    Content::text(text_summary),
]))
```

### 3.4 错误处理

```rust
let plan = ogexplain_core::parse(&params.explain_text)
    .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
```

### 3.5 五个工具

| 工具 | 参数 | 返回 |
|------|------|------|
| `analyze_explain` | `explain_text`, `sql_text?` | JSON DiagnosticReport + 文本摘要 |
| `parse_explain` | `explain_text` | JSON ExplainPlan |
| `list_diagnostic_rules` | 无 | JSON Vec\<RuleInfo\> |
| `get_suggestions` | `explain_text` | JSON Vec\<Suggestion\> |
| `score_sql_complexity` | `sql_text` | JSON 标准评分 + GaussDB 四维评分 |

### 3.6 添加新工具

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MyToolParams { pub input: String; }

#[tool_router(server_handler)]
impl OgexplainServer {
    #[tool(name = "my_tool", description = "描述")]
    async fn my_tool(
        &self, Parameters(params): Parameters<MyToolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = /* ... */;
        Ok(CallToolResult::success(vec![Content::json(&result)?]))
    }
}
```

### 3.7 集成测试

```rust
#[tokio::test]
async fn test_mcp_tool() {
    let server = OgexplainServer;
    let (client_io, server_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        let transport = rmcp::transport::io::stdio_with_io(server_io);
        let _ = rmcp::serve_server(server, transport).await;
    });
    // 客户端发送 tool call 并验证响应...
}
```

---

## 4. 添加新功能

### 4.1 添加诊断规则

**理解 `DiagnosticRule` trait：**

```rust
pub trait DiagnosticRule: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> Severity;
    fn category(&self) -> DiagnosticCategory;
    fn check(&self, node: &PlanNode, ctx: &PlanContext) -> Option<Finding>;
    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        Vec::new()
    }
}
```

- `check()` — DFS 遍历时每个节点调用一次，关心单节点时使用
- `check_global()` — 整棵树遍历后调用一次，需要跨节点信息时使用（如"重复排序"）
- 两者可以同时实现

**完整示例：**

```rust
// crates/ogexplain-core/src/analyzer/rules/my_rules.rs
use super::{DiagnosticRule, make_finding};
use crate::analyzer::context::PlanContext;
use crate::analyzer::report::{DiagnosticCategory, Finding, Severity};
use crate::model::PlanNode;

pub struct MyNewRule { threshold: f64 }

impl MyNewRule {
    pub fn new(config: &super::super::config::DiagnosticConfig) -> Self {
        Self { threshold: config.large_table_rows }
    }
}

impl DiagnosticRule for MyNewRule {
    fn id(&self) -> &str { "MYCAT-001" }
    fn name(&self) -> &str { "我的新诊断规则" }
    fn severity(&self) -> Severity { Severity::Warning }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::General }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if !matches!(node.node_type,
            crate::model::NodeType::SeqScan | crate::model::NodeType::PartitionedSeqScan
        ) { return None; }

        let est = node.estimated.as_ref()?;
        if est.plan_rows < self.threshold { return None; }

        let table = super::utils::extract_target_table(node)
            .unwrap_or_else(|| "unknown".to_string());

        Some(make_finding(self,
            format!("表 {} 估算 {} 行，超过阈值 {}", table, est.plan_rows, self.threshold),
            node,
            Some(format!("考虑在 {} 上创建索引", table)),
        ))
    }
}
```

**`rules/utils.rs` 共享工具：** `is_scan_node`/`is_sort_node`/`is_dml_node`（节点类别判断）、`extract_target_table`（多级回退提取表名）、`first_identifier`（去别名）、`get_property_value`（按标签查属性）、`any_property_contains`、`extract_innermost_parens`。

**注册（`rules/mod.rs`）：**

```rust
mod my_rules;

pub fn all_rules(config: &DiagnosticConfig) -> Vec<Box<dyn DiagnosticRule>> {
    vec![
        // ... 已有规则 ...
        Box::new(my_rules::MyNewRule::new(config)),
    ]
}
```

`config.disabled_rules` 过滤已内置。

**测试：**

```rust
#[test]
fn test_mycat_001_triggers() {
    let plan = parse(include_str!("fixtures/my_scenario.txt")).unwrap();
    assert!(analyze(&plan).findings.iter().any(|f| f.rule_id == "MYCAT-001"));
}

#[test]
fn test_mycat_001_no_false_positive() {
    let plan = parse(include_str!("fixtures/01_simple_seq_scan.txt")).unwrap();
    assert!(!analyze(&plan).findings.iter().any(|f| f.rule_id == "MYCAT-001"));
}

#[test]
fn test_mycat_001_can_be_disabled() {
    let plan = parse(include_str!("fixtures/my_scenario.txt")).unwrap();
    let config = DiagnosticConfig {
        disabled_rules: vec!["MYCAT-001".into()], ..Default::default()
    };
    assert!(!analyze_with_config(&plan, &config).findings.iter().any(|f| f.rule_id == "MYCAT-001"));
}
```

**更新 MCP 元数据：** 在 `crates/ogexplain-mcp/src/server.rs` 的 `list_diagnostic_rules` 中添加 `RuleInfo { id: "MYCAT-001".into(), ... }`。

### 4.2 添加输出格式

在 `crates/ogexplain-cli/src/lib.rs` 的 `match output` 分发处添加新分支：

```rust
match output {
    "json" => output_json(...),
    "heatmap" => output_heatmap(...),
    "waterfall" => output_waterfall(...),
    "markdown" => output_markdown(...),  // 新增
    _ => output_text(...),
}
```

### 4.3 添加 CLI 子命令

CLI 使用 builder 模式 clap（支持 i18n help 文本），feature 门控：

```rust
Some(("my_subcommand", args)) => {
    #[cfg(feature = "my_feature")]
    { /* 实现 */ }
    #[cfg(not(feature = "my_feature"))]
    { anyhow::bail!("此功能未编译。请使用 --features my_feature 重新构建"); }
}
```

### 4.4 添加解析器节点类型

1. 在 `model/node_type.rs` 的 `NodeType` 枚举添加变体
2. 在 `parse_node_type()` 的 match 添加字符串映射
3. 如属已知类别，更新 `category()` 方法
4. 添加 fixture + `insta::assert_yaml_snapshot!` 测试
5. `cargo insta review` 审查快照

---

## 5. 集成示例

### 5.1 作为库依赖引入

```toml
[dependencies]
ogexplain-core = { path = "../ogexplain-analyzer/crates/ogexplain-core" }
```

```rust
use ogexplain_core::{parse, analyze, heatmap, waterfall};

fn analyze_explain(text: &str) -> anyhow::Result<()> {
    let plan = parse(text).map_err(|e| anyhow::anyhow!("解析失败: {}", e))?;
    let report = analyze(&plan);
    for f in &report.findings {
        println!("[{}] {} - {}", f.rule_id, f.title, f.detail);
    }
    if let Some(hm) = heatmap(&plan) {
        println!("最大 Q-Error: {:.1}", hm.summary.max_qerror);
    }
    Ok(())
}
```

### 5.2 嵌入 Web 服务（axum）

```rust
use axum::{Json, http::StatusCode};
use ogexplain_core::{parse, analyze_with_rewrite};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct AnalyzeRequest { explain_text: String, sql_text: Option<String> }

#[derive(Serialize)]
struct AnalyzeResponse { findings_count: usize, has_critical: bool, findings: Vec<SerdeFinding> }

#[derive(Serialize)]
struct SerdeFinding { rule_id: String, severity: String, title: String, detail: String, suggestion: Option<String> }

async fn analyze_handler(Json(req): Json<AnalyzeRequest>) -> Result<Json<AnalyzeResponse>, StatusCode> {
    let plan = parse(&req.explain_text).map_err(|_| StatusCode::BAD_REQUEST)?;
    let report = analyze_with_rewrite(&plan, req.sql_text.as_deref());
    let findings: Vec<_> = report.findings.iter().map(|f| SerdeFinding {
        rule_id: f.rule_id.clone(), severity: f.severity.as_str().into(),
        title: f.title.clone(), detail: f.detail.clone(), suggestion: f.suggestion.clone(),
    }).collect();
    Ok(Json(AnalyzeResponse {
        findings_count: findings.len(),
        has_critical: findings.iter().any(|f| f.severity == "critical"),
        findings,
    }))
}
```

### 5.3 批量处理管道

```rust
use ogexplain_core::{parse, analyze_with_rewrite};
use ogexplain_core::sql::segment_input;
use ogexplain_core::summary::SummaryRow;

fn batch_analyze(input: &str) -> Vec<SummaryRow> {
    segment_input(input).iter().filter_map(|block| {
        parse(&block.explain_text).ok().map(|plan| {
            let report = analyze_with_rewrite(&plan, block.sql_text.as_deref());
            SummaryRow::compute(&plan, &report, None)
        })
    }).collect()
}
```

### 5.4 与 gaussdb-mcp 组合端到端诊断

```
AI 助手 ──► gaussdb-mcp: execute_query("EXPLAIN SELECT ...") ──► EXPLAIN TEXT
    │
    └──► ogexplain-mcp: analyze_explain(explain_text, sql_text) ──► DiagnosticReport
```

```json
{
  "mcpServers": {
    "gaussdb": { "command": "gaussdb-mcp", "args": ["--connection", "host=localhost port=5432 dbname=mydb"] },
    "ogexplain": { "command": "ogexplain-mcp" }
  }
}
```

### 5.5 构建自定义前端

```rust
use ogexplain_core::{parse, analyze, heatmap, waterfall};
use ogexplain_core::suggester::SuggestionEngine;

fn build_dashboard(text: &str) -> DashboardData {
    let plan = parse(text).expect("解析失败");
    let report = analyze(&plan);
    DashboardData {
        tree: serialize_plan_tree(&plan.root),
        critical_count: report.findings.iter().filter(|f| f.severity == Severity::Critical).count(),
        heatmap: heatmap(&plan).map(|hm| hm.entries),
        waterfall: waterfall(&plan).map(|wf| wf.entries),
        suggestions: SuggestionEngine::suggest(&report.findings),
    }
}
```

---

## 6. 构建与测试

### 6.1 Feature Flags

```
ogexplain-analyzer (根): default=[], full=["cli","tui","mcp"]
ogexplain-cli: db(默认启用)=数据库直连, mcp=MCP 子命令
```

### 6.2 构建命令

```bash
cargo build --workspace                              # 完整构建
cargo build -p ogexplain-core                        # 仅核心库
cargo build -p ogexplain-cli                         # CLI（含 db）
cargo build -p ogexplain-cli --no-default-features   # CLI（不含 db）
cargo build -p ogexplain-mcp                         # MCP 服务器
cargo build -p ogexplain-tui                         # TUI
cargo build --features full                          # cli + tui + mcp
```

### 6.3 测试类型

| 类型 | 位置 | 运行命令 |
|------|------|---------|
| 单元测试 | `#[cfg(test)]` 模块 | `cargo test -p ogexplain-core` |
| 解析器快照 | `tests/integration_tests.rs` | `cargo test --test integration_tests` |
| 分析器规则 | `tests/analyzer_tests.rs` | `cargo test --test analyzer_tests` |
| MCP 集成 | `crates/ogexplain-mcp/tests/` | `cargo test -p ogexplain-mcp` |
| 数据库集成 | `tests/db_explain.rs` | `cargo test --test db_explain --features ogexplain-cli/db`（需 Docker） |

### 6.4 insta 快照测试

```rust
#[test]
fn test_parse_fixture() {
    let plan = parse(include_str!("fixtures/03_hash_join.txt")).unwrap();
    insta::assert_yaml_snapshot!(plan);
}
```

工作流：修改 → `cargo test` → `cargo insta review` → 批准 → 提交。

### 6.5 CI 期望

```bash
cargo fmt --all -- --check                          # 格式化
cargo clippy --workspace --all-features -- -D warnings  # 零警告
cargo test --workspace                               # 全量测试
cargo test --test integration_tests                  # 快照一致性
cargo test --test analyzer_tests                     # 规则测试
cargo deny check                                     # 依赖审计（可选）
```

### 6.6 调试技巧

```bash
# JSON 输出查看
cargo run -p ogexplain-cli -- analyze fixtures/10_complex_plan.txt -o json | jq .

# 特定规则
cargo run -p ogexplain-cli -- analyze fixtures/03_hash_join.txt -o json | \
  jq '.findings[] | select(.rule_id == "JOIN-002")'

# 单个测试
cargo test -p ogexplain-core -- test_is_scan_node
cargo test --test analyzer_tests -- test_scan_001
```

### 6.7 依赖版本

| 核心依赖 | 用途 |
|---------|------|
| `regex` | 解析器行分类 |
| `serde` + `serde_json` | 模型序列化 |
| `thiserror` | 错误类型 |
| `rust-i18n` | 国际化 |
| `ogsql-parser` | SQL 解析（改写器） |

| 前端依赖 | crate |
|---------|-------|
| `clap` v4, `colored` | cli |
| `ratatui` 0.30, `crossterm` 0.29, `ratatui-textarea` 0.8 | tui |
| `rmcp` 1.7, `schemars`, `tokio` | mcp |

---

## 附录：扩展检查清单

添加新诊断规则：

- [ ] 在 `analyzer/rules/` 创建规则文件
- [ ] 实现 `DiagnosticRule` trait（`check` 和/或 `check_global`）
- [ ] 使用 `utils.rs` 共享工具函数
- [ ] 在 `rules/mod.rs` 的 `all_rules()` 注册
- [ ] 正面测试 + 负面测试 + 禁用测试
- [ ] 更新 MCP `list_diagnostic_rules` 元数据
- [ ] `cargo fmt` + `cargo clippy` 零警告 + `cargo test` 全通过
- [ ] 新增公开项有文档注释
