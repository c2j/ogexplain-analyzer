# ogexplain-analyzer Developer Guide

This guide is for developers extending, integrating, or contributing to the ogexplain-analyzer project. It covers the internal architecture, public API, extension patterns, and build/test workflows at a level of detail that source code alone cannot provide.

For user-facing usage, see the [README](../README.md). For contribution workflow and coding standards, see [CONTRIBUTING](../CONTRIBUTING.md).

---

## Table of Contents

1. [Architecture Deep Dive](#1-architecture-deep-dive)
2. [Core API Reference](#2-core-api-reference)
3. [MCP Server Development](#3-mcp-server-development)
4. [Adding New Features](#4-adding-new-features)
5. [Integration Examples](#5-integration-examples)
6. [Build and Test](#6-build-and-test)

---

## 1. Architecture Deep Dive

### 1.1 Workspace Structure

ogexplain-analyzer is a Cargo workspace with five crates, all sharing version `0.2.0` and edition `2021`:

```
ogexplain-analyzer/                 # Workspace root (virtual manifest)
├── crates/
│   ├── ogexplain-core/             # Pure library — zero IO/UI deps
│   ├── ogexplain-cli/              # Binary: ogexplain (clap v4 subcommands)
│   ├── ogexplain-tui/              # Binary: ogexplain-tui (ratatui + Elm/TEA)
│   ├── ogexplain-mcp/              # Binary: ogexplain-mcp (rmcp 1.7, stdio)
│   └── ogsql-complexity/           # Standalone library — SQL complexity scoring
├── tests/
│   ├── fixtures/                   # 31 EXPLAIN TEXT fixture files
│   ├── integration_tests.rs        # Parser insta snapshot tests
│   └── analyzer_tests.rs           # Diagnostic rule tests
└── docs/
```

The workspace root's `Cargo.toml` defines feature flags that gate the binary crates:

```toml
[features]
default = []
full = ["cli", "tui", "mcp"]
cli = ["dep:ogexplain-cli", "dep:anyhow"]
tui = ["dep:ogexplain-tui", "dep:color-eyre"]
mcp = ["dep:ogexplain-mcp", "ogexplain-cli?/mcp"]
```

The CLI crate has its own `db` feature (default-enabled) for the `explain` subcommand that connects directly to OpenGauss:

```toml
# crates/ogexplain-cli/Cargo.toml
[features]
default = ["db"]
db = ["dep:tokio-postgres", "dep:tokio"]
mcp = ["dep:ogexplain-mcp"]
```

### 1.2 Data Flow

The primary processing pipeline transforms raw EXPLAIN TEXT into structured diagnostics:

```
EXPLAIN TEXT
    │
    ▼
 parse() / parse_multi()           ← parser::parse()
    │
    ▼
 ExplainPlan { root: PlanNode, summary }
    │
    ├──► analyze()                  ← DiagnosticEngine::analyze()
    │       │
    │       ▼
    │   DiagnosticReport { findings, stats }
    │       │
    │       ├──► SuggestionEngine::suggest()  ← Cross-rule synthesis
    │       │       ▼
    │       │   Vec<Suggestion>
    │       │
    │       └──► SummaryRow::compute()        ← 43-field CSV export
    │
    ├──► heatmap()                  ← HeatmapEngine::generate()
    │       ▼
    │   PlanHeatmap { entries, critical_path, hotspots, summary }
    │
    └──► waterfall()                ← WaterfallEngine::generate()
            ▼
        PlanWaterfall { entries, bottlenecks, total_nodes }
```

The SQL rewrite path extends the analyze path when original SQL is available:

```
ExplainPlan + SQL Text
    │
    ▼
 analyze_with_rewrite(plan, Some(sql))
    │
    ├── Standard analyze → DiagnosticReport
    │
    └── Parse SQL → detect_correlated_subquery_update()
                      │
                      ▼ (if pattern found)
                   rewrite_update_from() → RewriteResult
                      │
                      ▼
                   Inject into SUBQ-006 findings
```

### 1.3 Core Crate Design Principles

**ogexplain-core is a pure library.** It has zero IO or UI dependencies — no `ratatui`, no `crossterm`, no `clap`, no `tokio`. This makes it safe to embed in web services, MCP servers, WASM, or any Rust project without pulling in a terminal UI framework.

**All model types derive `Serialize`.** Every struct in `model/`, `analyzer/report.rs`, `heatmap/types.rs`, `waterfall/types.rs`, `suggester/suggestion.rs`, and `rewriter/types.rs` derives `serde::Serialize`. This enables JSON output across all frontends (CLI, MCP, programmatic) with a single `serde_json::to_string()` call.

**Parser never panics.** The parser returns `Result<ExplainPlan, ParseError>` and handles malformed input gracefully. Even partially parseable input produces a tree with `NodeType::Unknown` variants for unrecognized nodes.

**Diagnostic rules are independently testable.** Each rule implements `DiagnosticRule::check()` which takes immutable references to a single node and a plan context. Rules have no side effects and can be unit-tested in isolation.

### 1.4 Parser Architecture: Two-Phase Design

The parser (`parser/`) works in two phases:

1. **Line classifier** (`line_classifier.rs`): Each line of EXPLAIN output is classified via regex into one of several categories: node header (with cost/actual stats), property line (Filter, Sort Key, Hash Cond, etc.), summary line (Total runtime, Peak Memory), or noise (blank, SQL echo, server messages).

2. **Tree builder** (`tree_builder.rs`): Classified lines are assembled into a `PlanNode` tree using indent-level tracking. An internal stack tracks the current parent at each indent level. Properties are accumulated onto the most recently created node.

This two-phase approach separates "what is this line?" from "how do these lines relate?" — making each phase testable in isolation and simplifying support for new EXPLAIN output formats.

### 1.5 Analyzer Architecture: Rule Engine with DFS Traversal

The analyzer uses a rule engine pattern:

```rust
// Simplified from analyzer/config.rs
pub struct DiagnosticEngine {
    config: DiagnosticConfig,
    rules: Vec<Box<dyn DiagnosticRule>>,
}
```

`DiagnosticEngine::analyze()` performs a depth-first traversal of the `PlanNode` tree. At each node, every registered rule's `check()` method is called. After the traversal, each rule's `check_global()` is called with the full plan and computed global statistics. This two-level approach lets rules operate on individual nodes (e.g., "is this sort spilling?") or on the plan as a whole (e.g., "is the plan too deep?").

The `DiagnosticConfig` controls rule behavior via thresholds and can disable specific rules by ID. Rules are constructed with config values at engine creation time.

### 1.6 TUI Architecture: Elm/TEA Pattern

The TUI uses the Elm Architecture (also called TEA — The Elm Architecture), implemented with `ratatui`:

```
Event (keyboard input)
    │
    ▼
 Event::handle() → Action       ← event.rs
    │
    ▼
 App::update(action)             ← app.rs (state mutation)
    │
    ▼
 App::view() → Vec<Pane>        ← app.rs (pure rendering)
    │
    ▼
 Terminal::draw()                ← main loop
```

Key design choices:
- **State and rendering are strictly separated.** `App` holds mutable state; `view()` is a pure function of state.
- **Actions are explicit.** Keyboard events map to `Action` enum variants (not direct state mutation), making the flow traceable.
- **Components** are in `components/` — each panel (TreePanel, DetailPanel, InputPanel, StatusBar) manages its own rendering logic.

### 1.7 MCP Architecture: rmcp 1.7 SDK

The MCP server uses the official Rust MCP SDK (`rmcp` 1.7). The `OgexplainServer` struct uses the `#[tool_router]` macro to auto-generate the tool routing table, and each tool method is annotated with `#[tool(name = "...", description = "...")]`.

See [Section 3](#3-mcp-server-development) for full details on the MCP architecture.

---

## 2. Core API Reference

This section documents the public API of `ogexplain-core`. All types are re-exported from the crate root or their respective submodules.

### 2.1 Top-Level Functions

The crate root (`lib.rs`) provides six public functions:

```rust
// Parse single EXPLAIN block
pub fn parse(text: &str) -> Result<ExplainPlan, ParseError>;

// Parse multiple EXPLAIN blocks from mixed SQL+EXPLAIN text
pub fn parse_multi(text: &str) -> Result<Vec<ExplainPlan>, ParseError>;

// Analyze with default config (all 25 rules, default thresholds)
pub fn analyze(plan: &ExplainPlan) -> DiagnosticReport;

// Analyze with custom thresholds and disabled rules
pub fn analyze_with_config(
    plan: &ExplainPlan,
    config: &DiagnosticConfig,
) -> DiagnosticReport;

// Analyze with SQL rewrite support (injects RewriteResult into SUBQ-006 findings)
pub fn analyze_with_rewrite(
    plan: &ExplainPlan,
    sql_text: Option<&str>,
) -> DiagnosticReport;

// Generate cost-actual deviation heatmap (requires EXPLAIN ANALYZE data)
pub fn heatmap(plan: &ExplainPlan) -> Option<PlanHeatmap>;

// Generate resource waterfall for CPU/memory bottleneck analysis
pub fn waterfall(plan: &ExplainPlan) -> Option<PlanWaterfall>;
```

**Why `heatmap()` and `waterfall()` return `Option`:** These functions require `EXPLAIN ANALYZE` output with actual timing and memory data. Plans parsed from plain `EXPLAIN` (without `ANALYZE`) have no `ActualStats` on their nodes, so these functions return `None`.

### 2.2 ParseError

```rust
pub enum ParseError {
    LineParse { line: usize, message: String },
    EmptyInput,
    NoPlanNodes,
}
```

- `LineParse` — A specific line could not be classified or parsed.
- `EmptyInput` — The input string is empty or whitespace-only.
- `NoPlanNodes` — No recognizable plan node headers were found (may be pure SQL text or unrelated content).

### 2.3 ExplainPlan and PlanSummary

```rust
pub struct ExplainPlan {
    pub root: PlanNode,
    pub summary: Option<PlanSummary>,
}
```

The `root` is the top-level plan node (e.g., `Hash Join`, `Streaming(type: GATHER)`). The tree is recursive — each `PlanNode` contains a `children: Vec<PlanNode>`.

```rust
pub struct PlanSummary {
    pub total_runtime_ms: Option<f64>,
    pub peak_memory_kb: Option<i64>,
    pub planner_runtime_ms: Option<f64>,
    pub plan_size_bytes: Option<i64>,
    pub query_id: Option<String>,
    pub executor_start_ms: Option<f64>,
    pub executor_run_ms: Option<f64>,
    pub executor_end_ms: Option<f64>,
    pub total_network_kb: Option<i64>,
}
```

`PlanSummary` captures EXPLAIN ANALYZE footer data (total runtime, peak memory, etc.). It is `None` for plain `EXPLAIN` without execution.

### 2.4 PlanNode

```rust
pub struct PlanNode {
    pub node_type: NodeType,
    pub relation: Option<String>,
    pub join_type: Option<JoinType>,
    pub estimated: Option<EstimatedCost>,
    pub actual: Option<ActualStats>,
    pub properties: Vec<NodeProperty>,
    pub structured_props: Option<NodeProperties>,
    pub buffers: Option<BufferStats>,
    pub children: Vec<PlanNode>,
    pub indent_level: usize,
    pub line_number: usize,
}
```

**Fields:**

| Field | Purpose |
|-------|---------|
| `node_type` | The plan operation (e.g., `SeqScan`, `HashJoin`, `Streaming(Gather)`). 80+ variants. |
| `relation` | Table/relation name, present on scan and DML nodes (e.g., `"orders o"`). |
| `join_type` | Join direction (`Inner`, `Left`, `Semi`, etc.) — present on join nodes. |
| `estimated` | Optimizer cost estimates (`plan_rows`, `startup_cost`, `total_cost`). |
| `actual` | Runtime statistics from `EXPLAIN ANALYZE` (`rows`, `total_time_ms`, `loops`). |
| `properties` | Raw key-value properties (`Filter: ...`, `Sort Key: ...`, `Hash Cond: ...`). |
| `structured_props` | Extracted typed properties (sort method, hash buckets, peak memory, rows removed). |
| `buffers` | Buffer statistics (`shared_hit`, `shared_read`, `temp_read`, `temp_written`). |
| `children` | Child nodes (the subtree this node operates on). |
| `indent_level` | Indentation depth in the original EXPLAIN output. |
| `line_number` | 1-based line number in the original input — used for pinpointing findings. |

### 2.5 NodeType Enum

`NodeType` has 80+ variants covering the full OpenGauss plan node taxonomy. Each variant falls into a `NodeTypeCategory`:

```rust
pub enum NodeTypeCategory {
    Scan,       // SeqScan, IndexScan, CStoreScan, BitmapHeapScan, ...
    Join,       // NestedLoop, HashJoin, MergeJoin, VectorHashJoin, ...
    Aggregate,  // Aggregate, GroupAggregate, HashAggregate, VectorHashAggregate, ...
    Sort,       // Sort, GroupSort, VectorSort
    Dml,        // Insert, Update, Delete, Merge, VectorInsert, ...
    SetOp,      // Append, MergeAppend, RecursiveUnion, BitmapAnd, ...
    Streaming,  // Streaming(StreamingType), VectorStreaming(StreamingType)
    Auxiliary,  // Limit, Result, Hash, Materialize, RowAdapter, ...
    Other,      // Everything else
}
```

Access via `node.node_type.category()`. This is used throughout the rule engine to quickly filter nodes by category.

Notable parametric variants:
- `NodeType::Streaming(StreamingType)` — carries the streaming type (`Gather`, `Redistribute`, `Broadcast`, etc.).
- `NodeType::Unknown(String)` — fallback for unrecognized node types; preserves the raw name.

### 2.6 EstimatedCost and ActualStats

```rust
pub struct EstimatedCost {
    pub startup_cost: f64,
    pub total_cost: f64,
    pub plan_rows: f64,
    pub plan_width: i32,
    pub pred_time: Option<f64>,     // AI-predicted time (p-time)
    pub pred_rows: Option<f64>,     // AI-predicted rows (p-rows)
    pub distinct: Option<(f64, f64)>, // Distinct estimate
}

pub struct ActualStats {
    pub startup_time_ms: f64,
    pub total_time_ms: f64,
    pub rows: f64,
    pub loops: f64,
    pub executed: bool,   // false when "(Actual time: never executed)"
}
```

### 2.7 BufferStats and NodeProperty

```rust
pub struct BufferStats {
    pub shared_hit: i64,
    pub shared_read: i64,
    pub shared_dirtied: i64,
    pub shared_written: i64,
    pub local_hit: i64,
    pub local_read: i64,
    pub local_dirtied: i64,
    pub local_written: i64,
    pub temp_read: i64,
    pub temp_written: i64,
    pub io_read_time_ms: Option<f64>,
    pub io_write_time_ms: Option<f64>,
}

pub struct NodeProperty {
    pub label: String,   // "Filter", "Sort Key", "Hash Cond", etc.
    pub value: String,   // Raw value text
}
```

### 2.8 DiagnosticConfig

```rust
#[derive(Debug, Clone)]
pub struct DiagnosticConfig {
    pub large_table_rows: f64,          // Default: 10,000
    pub memory_threshold_kb: f64,       // Default: 102,400 (100 MB)
    pub estimation_skew_factor: f64,    // Default: 100.0
    pub nested_loop_inner_rows: f64,    // Default: 10,000
    pub sort_time_ratio: f64,           // Default: 0.3
    pub max_plan_depth: usize,          // Default: 10
    pub disabled_rules: Vec<String>,    // Default: empty
}
```

**Threshold semantics:**

| Field | Used by | Meaning |
|-------|---------|---------|
| `large_table_rows` | SCAN-001 | Tables with `plan_rows` exceeding this are "large". |
| `memory_threshold_kb` | MEM-004 | Nodes with peak memory above this trigger warnings. |
| `estimation_skew_factor` | EST-001 | Q-Error ratio threshold for severe estimation errors. |
| `nested_loop_inner_rows` | JOIN-001 | Inner side row count that makes Nested Loop concerning. |
| `sort_time_ratio` | SORT-003 | Sort time as fraction of total plan time to flag duplicates. |
| `max_plan_depth` | GEN-001 | Plans deeper than this are flagged as "too deep". |
| `disabled_rules` | Engine | Rules with matching IDs are excluded from analysis. |

### 2.9 Finding and DiagnosticReport

```rust
pub enum Severity { Critical, Warning, Info }

pub enum DiagnosticCategory {
    ScanEfficiency, JoinStrategy, MemoryUsage, SortEfficiency,
    NetworkOverhead, CostMisestimation, PushdownFailure, TypeMismatch,
    Vectorization, SubqueryStructure, DistributionIssue, General,
}

pub struct Finding {
    pub rule_id: String,           // "SCAN-001", "JOIN-002", etc.
    pub severity: Severity,
    pub category: DiagnosticCategory,
    pub title: String,             // Human-readable rule name
    pub detail: String,            // Specific details (table name, row counts)
    pub node_line: Option<usize>,  // Line number in EXPLAIN output
    pub node_type: Option<String>, // Node type that triggered the finding
    pub suggestion: Option<String>, // Parameterized fix suggestion
    pub sql_rewrite: Option<RewriteResult>, // Filled by analyze_with_rewrite() for SUBQ-006
    pub evidence: Option<Evidence>, // Populated by anti-pattern rules
}

pub struct DiagnosticReport {
    pub findings: Vec<Finding>,
    pub stats: GlobalStats,
}
```

### 2.10 PlanHeatmap (Cost-Actual Deviation)

The heatmap analyzes estimation accuracy per node using Q-Error, the standard VLDB metric:

```
Q-Error = max(actual, estimated) / min(actual, estimated)
```

```rust
pub enum DeviationSeverity {
    Negligible,  // Q-Error < 2x
    Mild,        // 2x ≤ Q-Error < 5x
    Moderate,    // 5x ≤ Q-Error < 10x
    Severe,      // 10x ≤ Q-Error < 50x
    Extreme,     // Q-Error ≥ 50x
}

pub struct PlanHeatmap {
    pub entries: Vec<HeatmapEntry>,
    pub critical_path: Vec<usize>,    // Line numbers of max-deviation path
    pub hotspots: Vec<usize>,         // Line numbers of severe nodes, sorted by Q-Error
    pub summary: HeatmapSummary,
}

pub struct HeatmapEntry {
    pub deviation: NodeDeviation,
    pub subtree_geo_qerror: f64,      // Geometric mean Q-Error of subtree
    pub path_cumulative_qerror: f64,  // Product of ancestor Q-Errors
    pub on_critical_path: bool,
}

pub struct HeatmapSummary {
    pub max_qerror: f64,
    pub max_qerror_line: usize,
    pub severe_count: usize,
    pub total_nodes: usize,
    pub critical_path_length: usize,
    pub deviated_count: usize,
}
```

### 2.11 PlanWaterfall (Resource Bottleneck Analysis)

The waterfall identifies CPU and memory bottlenecks:

```rust
pub struct PlanWaterfall {
    pub entries: Vec<WaterfallEntry>,
    pub bottlenecks: BottleneckSummary,
    pub total_nodes: usize,
    pub nodes_with_stats: usize,
}

pub struct WaterfallEntry {
    pub metrics: NodeResourceMetrics,
    pub dimensions: Vec<ResourceDimension>,  // CpuTime, Memory
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub is_bottleneck: bool,
    pub bottleneck_dimensions: Vec<ResourceDimension>,
    pub depth: usize,
}

pub struct BottleneckSummary {
    pub cpu_bottlenecks: Vec<usize>,      // Top 5 line numbers by CPU time
    pub memory_bottlenecks: Vec<usize>,   // Top 5 line numbers by peak memory
    pub total_cpu_time_ms: f64,
    pub max_peak_memory_kb: f64,
    pub spill_node_count: usize,
}
```

### 2.12 Suggestion and SuggestionEngine

The suggester provides cross-rule synthesis — analyzing multiple findings together to generate higher-level recommendations:

```rust
pub enum SuggestionCategory {
    IndexOptimization,
    StatisticsUpdate,
    QueryRewrite,
    ConfigurationTuning,
    DistributionOptimization,
}

pub struct Suggestion {
    pub related_rules: Vec<String>,  // Rule IDs that contributed
    pub category: SuggestionCategory,
    pub message: String,
    pub confidence: f64,             // 0.0 to 1.0
}
```

Usage:

```rust
use ogexplain_core::suggester::SuggestionEngine;

let suggestions = SuggestionEngine::suggest(&report.findings);
for s in &suggestions {
    println!("[{:.0}%] ({:?}) {}", s.confidence * 100.0, s.category, s.message);
}
```

The engine detects five cross-rule patterns:

| Pattern | Trigger | Example Suggestion |
|---------|---------|-------------------|
| Multi-spill | ≥2 of MEM-001, JOIN-002 | Increase `work_mem` to avoid disk I/O |
| Multi-estimation | ≥2 EST-* rules | Run `ANALYZE` on involved tables |
| Scan + Join | Any SCAN-* + any JOIN-* | Create composite index on join + filter columns |
| Pushdown failure | Any PUSH-* rules | Review non-pushdown constructs, consider redistribution |
| Engine mixing | VEC-001 | Unify row or vector engine to eliminate adapter overhead |

### 2.13 SummaryRow (Batch CSV Export)

`SummaryRow` is a 43-field struct for batch reporting across multiple EXPLAIN plans:

```rust
pub struct SummaryRow {
    // SQL complexity fields
    pub sql_preview: Option<String>,
    pub tables: usize,
    pub joins: usize,
    pub subqueries: usize,
    // ... (all ComplexityInput fields)

    // Plan metrics
    pub root_op: String,
    pub total_cost: f64,
    pub total_time_ms: f64,
    pub actual_rows: Option<f64>,
    pub plan_depth: usize,
    pub node_count: usize,

    // Performance indicators
    pub worst_est_ratio: Option<f64>,
    pub spill_kb: Option<f64>,
    pub peak_memory_kb: Option<f64>,
    pub pushdown: PushdownStatus,
    pub buffer_hit_rate: Option<f64>,

    // Diagnostic counts
    pub critical_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
}
```

It is computed from a triple of inputs:

```rust
let row = SummaryRow::compute(&plan, &report, complexity_input.as_ref());
```

Where `complexity_input` is `Option<&ComplexityInput>` — it can be `None` if SQL complexity scoring is not needed.

### 2.14 Rewriter Types

The rewriter handles correlated subquery self-update patterns (SUBQ-006):

```rust
pub struct RewriteResult {
    pub strategy: RewriteStrategy,    // Always UpdateFrom currently
    pub rewritten_sql: String,        // The rewritten SQL
    pub explanation: String,          // Why the rewrite is better
    pub pattern_info: AntiPatternInfo,
}

pub struct AntiPatternInfo {
    pub target_table: String,
    pub subquery_table: String,
    pub correlation_columns: Vec<String>,
    pub set_columns: Vec<String>,
    pub uses_row_constructor: bool,
}
```

The rewriter is invoked via `analyze_with_rewrite()` at the top level, not directly. It parses the SQL text with `ogsql_parser`, detects the correlated subquery self-update anti-pattern, and generates an `UPDATE ... FROM` rewrite.

---

## 3. MCP Server Development

### 3.1 Server Architecture

The MCP server is implemented in `crates/ogexplain-mcp/` using `rmcp` 1.7 (the official Rust MCP SDK). It communicates over stdio transport and exposes 5 tools.

```rust
// crates/ogexplain-mcp/src/server.rs
#[derive(Debug, Clone, Default)]
pub struct OgexplainServer;

#[tool_router(server_handler)]
impl OgexplainServer {
    #[tool(name = "analyze_explain", description = "...")]
    async fn analyze_explain(
        &self,
        Parameters(params): Parameters<AnalyzeExplainParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // ...
    }
    // ... more tools
}
```

The `#[tool_router(server_handler)]` macro auto-generates the `ServerHandler` trait implementation, including the `info()` method that lists all tools and the `call_tool()` dispatch method.

### 3.2 Tool Parameter Structs

Each tool has a parameter struct that derives `Deserialize` and `JsonSchema`:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeExplainParams {
    /// The EXPLAIN or EXPLAIN ANALYZE output text to analyze
    pub explain_text: String,
    /// Optional original SQL text (enables SQL rewrite suggestions)
    #[serde(default)]
    pub sql_text: Option<String>,
}
```

`JsonSchema` is required because the MCP protocol needs JSON Schema descriptions of tool parameters for tool discovery. The `schemars` crate auto-derives these from the Rust struct definition and doc comments.

### 3.3 Response Construction

Tools return `CallToolResult` with multiple `Content` items — typically both structured JSON and human-readable text:

```rust
let mut contents = Vec::new();

// Structured JSON output (for programmatic consumption)
contents.push(Content::json(&report)?);

// Human-readable text summary (for AI assistant consumption)
contents.push(Content::text(text_summary));

Ok(CallToolResult::success(contents))
```

This dual-output pattern lets AI assistants use either format: JSON for precise programmatic access, or text for natural-language reasoning.

### 3.4 Error Handling

Parse errors and invalid inputs use `ErrorData::invalid_params`:

```rust
let plan = ogexplain_core::parse(&params.explain_text)
    .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
```

This returns a proper MCP error response that the client can display to the user.

### 3.5 Transport and Startup

The server uses stdio transport:

```rust
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        let server = OgexplainServer;
        let transport = rmcp::transport::io::stdio();
        if let Err(e) = rmcp::serve_server(server, transport).await {
            eprintln!("MCP server error: {e}");
            std::process::exit(1);
        }
    });
}
```

### 3.6 Adding a New MCP Tool

To add a new tool:

1. **Define a parameter struct** with `Deserialize` + `JsonSchema`:
   ```rust
   #[derive(Debug, Deserialize, JsonSchema)]
   pub struct MyNewToolParams {
       /// Description appears in the MCP tool schema
       pub input: String,
   }
   ```

2. **Add a method to `OgexplainServer`** annotated with `#[tool]`:
   ```rust
   #[tool(
       name = "my_new_tool",
       description = "What this tool does for the AI assistant"
   )]
   async fn my_new_tool(
       &self,
       Parameters(params): Parameters<MyNewToolParams>,
   ) -> Result<CallToolResult, ErrorData> {
       // Implementation
       let result = /* ... */;
       Ok(CallToolResult::success(vec![Content::json(&result)?]))
   }
   ```

3. **The `#[tool_router]` macro handles the rest** — tool registration, routing, and schema generation happen automatically at compile time.

### 3.7 Integration Testing

Test MCP tools without spawning a real server using in-memory transport:

```rust
#[cfg(test)]
mod tests {
    use rmcp::handler::server::wrapper::Parameters;
    use super::*;

    #[tokio::test]
    async fn test_analyze_explain_tool() {
        let server = OgexplainServer;
        let params = AnalyzeExplainParams {
            explain_text: "Seq Scan on t  (cost=0.00..10.00 rows=100 width=4)".to_string(),
            sql_text: None,
        };
        let result = server.analyze_explain(Parameters(params)).await.unwrap();
        assert!(!result.content.is_empty());
    }
}
```

For end-to-end transport testing, use `tokio::io::duplex` to create in-memory read/write pairs:

```rust
#[tokio::test]
async fn test_full_mcp_roundtrip() {
    let (client_read, server_write) = tokio::io::duplex(4096);
    let (server_read, client_write) = tokio::io::duplex(4096);

    // Spawn server task
    tokio::spawn(async move {
        let server = OgexplainServer;
        let transport = (server_read, server_write);
        let _ = rmcp::serve_server(server, transport).await;
    });

    // Client-side testing via client_read/client_write
    // ...
}
```

---

## 4. Adding New Features

### 4.1 Adding a Diagnostic Rule

This is the most common extension point. Follow these steps:

#### Step 1: Create the Rule File

Create a new file in `crates/ogexplain-core/src/analyzer/rules/`. If your rule is thematically related to an existing file (e.g., a new scan rule), add to that file. Otherwise, create a new file.

#### Step 2: Implement the `DiagnosticRule` Trait

```rust
use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{DiagnosticRule, make_finding};
use crate::model::{NodeType, PlanNode};

pub struct MyNewRule {
    threshold: f64,
}

impl MyNewRule {
    pub fn new(config: &DiagnosticConfig) -> Self {
        Self {
            threshold: config.large_table_rows,
        }
    }
}

impl DiagnosticRule for MyNewRule {
    fn id(&self) -> &str { "SCAN-005" }
    fn name(&self) -> &str { "Bitmap scan on large table" }
    fn severity(&self) -> Severity { Severity::Warning }
    fn category(&self) -> DiagnosticCategory { DiagnosticCategory::ScanEfficiency }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        // Only check Bitmap Heap Scan nodes
        if !matches!(node.node_type, NodeType::BitmapHeapScan) {
            return None;
        }

        let estimated = node.estimated.as_ref()?;
        if estimated.plan_rows < self.threshold {
            return None;
        }

        let table = node.relation.as_deref().unwrap_or("unknown");
        let detail = format!(
            "Bitmap Heap Scan on {} with {:.0} estimated rows (threshold: {:.0})",
            table, estimated.plan_rows, self.threshold
        );

        Some(make_finding(self, detail, node, Some(
            format!("Consider a plain Seq Scan for large scans on {}", table)
        )))
    }

    fn check_global(&self, _plan: &crate::model::ExplainPlan, _stats: &super::super::context::GlobalStats) -> Vec<Finding> {
        Vec::new()
    }
}
```

#### Step 3: Use Shared Utilities

The `rules/utils.rs` module provides common helpers:

```rust
use super::utils;

// Check if a node is a scan/sort/DML type
utils::is_scan_node(&node.node_type)

// Extract table name with fallback (node → child → grandchild)
utils::extract_target_table(node)

// Get a property value by label
utils::get_property_value(node, "Filter")

// Check if any property contains a string
utils::any_property_contains(node, "unknown")

// Extract innermost parenthesized content
utils::extract_innermost_parens(value_str)

// Strip alias from relation name ("employees e" → "employees")
utils::first_identifier(relation)
```

#### Step 4: Register in `all_rules()`

In `crates/ogexplain-core/src/analyzer/rules/mod.rs`:

```rust
pub fn all_rules(config: &DiagnosticConfig) -> Vec<Box<dyn DiagnosticRule>> {
    vec![
        // ... existing rules ...
        Box::new(my_rules::MyNewRule::new(config)),
    ]
}
```

Rules requiring config thresholds take `config.clone()` or specific fields. Rules that are purely structural (no thresholds) can be unit structs.

#### Step 5: Add the Module Declaration

In `rules/mod.rs`, add:

```rust
mod my_rules;  // at the top with other module declarations
```

#### Step 6: Write Tests

In `tests/analyzer_tests.rs`:

```rust
#[test]
fn test_scan_005_bitmap_large_table_triggers() {
    let plan = parse(include_str!("fixtures/99_bitmap_large.txt")).unwrap();
    let report = analyze(&plan);
    assert!(report.findings.iter().any(|f| f.rule_id == "SCAN-005"));
}

#[test]
fn test_scan_005_no_false_positive_small_table() {
    let plan = parse(include_str!("fixtures/01_simple_seq_scan.txt")).unwrap();
    let report = analyze(&plan);
    assert!(!report.findings.iter().any(|f| f.rule_id == "SCAN-005"));
}
```

#### Step 7: Update MCP Metadata

In `crates/ogexplain-mcp/src/server.rs`, add a `RuleInfo` entry to the `list_diagnostic_rules` tool:

```rust
RuleInfo {
    id: "SCAN-005".into(),
    name: "Bitmap scan on large table".into(),
    category: "scan".into(),
    description: "Bitmap scan on tables where Seq Scan may be faster".into(),
},
```

### 4.2 Adding an Output Format

The CLI dispatches output format in its main processing function. To add a new format (e.g., Markdown):

1. **Add the variant** to the CLI's output format matching (in `crates/ogexplain-cli/src/lib.rs`):
   ```rust
   match output.as_str() {
       "text" => { /* ... */ },
       "json" => { /* ... */ },
       "heatmap" => { /* ... */ },
       "waterfall" => { /* ... */ },
       "markdown" => render_markdown(&plan, &report),  // NEW
       _ => anyhow::bail!("Unknown output format: {}", output),
   }
   ```

2. **Implement the renderer** — either inline or as a separate module. The renderer takes the same inputs as other formats: `&ExplainPlan`, `&DiagnosticReport`, and optionally heatmap/waterfall data.

3. **The CLI does not constrain the format enum** — it accepts any string via `-o <format>`. Document new formats in the `--help` output and README.

### 4.3 Adding a CLI Subcommand

The CLI uses `clap` derive macros:

```rust
#[derive(Subcommand)]
enum Commands {
    Analyze { /* ... */ },
    Explain { /* ... */ },
    MyCommand {
        /// Description shown in --help
        #[arg(short, long)]
        input: String,
    },
}
```

Add a match arm in the `run()` function:

```rust
match cli.command {
    Some(Commands::Analyze { .. }) => { /* ... */ },
    Some(Commands::Explain { .. }) => { /* ... */ },
    Some(Commands::MyCommand { input }) => { /* ... */ },
    None => { /* default behavior */ },
}
```

For feature-gated subcommands (e.g., requiring database access), use `#[cfg(feature = "db")]` on both the enum variant and the match arm.

### 4.4 Adding Anti-Pattern Rules

Beyond classic diagnostic rules, the analyzer supports anti-pattern rules via the `pattern/` module. Anti-pattern rules use a structural pattern-matching approach against the plan tree.

The `AntiPatternRule` is already registered in `all_rules()`:

```rust
Box::new(super::pattern::AntiPatternRule::new()),
```

To add new anti-patterns, extend the pattern definitions in `analyzer/pattern/patterns/` and update the engine in `analyzer/pattern/engine.rs`. Anti-pattern rules produce findings with `Evidence` attached, which includes matched node details and confidence scores.

---

## 5. Integration Examples

### 5.1 Using ogexplain-core as a Library Dependency

Add to your `Cargo.toml`:

```toml
[dependencies]
ogexplain-core = { path = "../ogexplain-analyzer/crates/ogexplain-core" }
# Or from git:
# ogexplain-core = { git = "https://github.com/c2j/ogexplain-analyzer", branch = "main" }
```

Then use the public API:

```rust
use ogexplain_core::{parse, analyze, analyze_with_config, heatmap, waterfall};
use ogexplain_core::analyzer::config::DiagnosticConfig;
use ogexplain_core::suggester::SuggestionEngine;
use ogexplain_core::summary::SummaryRow;

fn process_explain(explain_text: &str) -> anyhow::Result<()> {
    let plan = parse(explain_text)?;

    // Default analysis
    let report = analyze(&plan);

    // Custom analysis
    let config = DiagnosticConfig {
        large_table_rows: 50_000.0,
        estimation_skew_factor: 50.0,
        ..Default::default()
    };
    let custom_report = analyze_with_config(&plan, &config);

    // Cross-rule suggestions
    let suggestions = SuggestionEngine::suggest(&report.findings);

    // Heatmap (if EXPLAIN ANALYZE)
    if let Some(hm) = heatmap(&plan) {
        println!("Max Q-Error: {:.1} on line {}", hm.summary.max_qerror, hm.summary.max_qerror_line);
        for entry in &hm.entries {
            if entry.deviation.severity >= ogexplain_core::analyzer::heatmap::DeviationSeverity::Severe {
                println!("  {:?} at line {}: {:.1}x", entry.deviation.severity, entry.deviation.line_number, entry.deviation.row_qerror);
            }
        }
    }

    // Batch summary
    let row = SummaryRow::compute(&plan, &report, None);
    println!("Nodes: {}, Findings: {}, Depth: {}",
        row.node_count, report.findings.len(), row.plan_depth);

    Ok(())
}
```

### 5.2 Embedding in a Web Service (axum)

```rust
use axum::{Json, extract::State, http::StatusCode};
use ogexplain_core::{parse, analyze_with_rewrite};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct ExplainRequest {
    explain_text: String,
    sql_text: Option<String>,
}

#[derive(Serialize)]
struct ExplainResponse {
    findings: Vec<ogexplain_core::analyzer::report::Finding>,
    suggestion_count: usize,
}

async fn analyze_handler(
    State(_state): State<()>,
    Json(req): Json<ExplainRequest>,
) -> Result<Json<ExplainResponse>, StatusCode> {
    let plan = parse(&req.explain_text).map_err(|_| StatusCode::BAD_REQUEST)?;
    let report = analyze_with_rewrite(&plan, req.sql_text.as_deref());
    let suggestions = ogexplain_core::suggester::SuggestionEngine::suggest(&report.findings);

    Ok(Json(ExplainResponse {
        findings: report.findings,
        suggestion_count: suggestions.len(),
    }))
}
```

Since `ogexplain-core` has no async runtime dependency, it works with any async framework (tokio, async-std, smol) without conflict.

### 5.3 Batch Processing Pipeline

Process a file with interleaved SQL and EXPLAIN blocks:

```rust
use ogexplain_core::{parse_multi, analyze};
use ogexplain_core::summary::SummaryRow;
use std::fs;

fn batch_process(file_path: &str) -> anyhow::Result<Vec<String>> {
    let content = fs::read_to_string(file_path)?;
    let plans = parse_multi(&content)?;

    let mut csv_rows = Vec::new();
    for plan in &plans {
        let report = analyze(plan);
        let row = SummaryRow::compute(plan, &report, None);
        // Convert to CSV line
        csv_rows.push(format!(
            "{},{},{},{},{},{}",
            row.root_op, row.total_cost, row.total_time_ms,
            row.critical_count, row.warning_count, row.info_count
        ));
    }

    Ok(csv_rows)
}
```

### 5.4 Combining with gaussdb-mcp for End-to-End Diagnostics

The `gaussdb-mcp` server can execute `EXPLAIN` on a live database and pass the output to `ogexplain-mcp` for analysis. In a two-MCP-server setup:

```json
{
  "mcpServers": {
    "gaussdb": {
      "command": "gaussdb-mcp",
      "args": ["--connection", "host=db port=5432 dbname=mydb"]
    },
    "ogexplain": {
      "command": "ogexplain-mcp"
    }
  }
}
```

An AI assistant workflow:
1. Use `gaussdb-mcp` to run `EXPLAIN ANALYZE` on a query.
2. Pass the EXPLAIN output text to `ogexplain-mcp`'s `analyze_explain` tool.
3. Receive structured findings with severity, suggestions, and parameterized fix recommendations.

### 5.5 Building a Custom Frontend

The core crate produces `Serialize`-compatible types, making it straightforward to serve over HTTP or embed in any presentation layer:

```rust
use ogexplain_core::{parse, analyze, heatmap, waterfall};
use serde_json;

fn full_analysis_json(explain_text: &str) -> anyhow::Result<String> {
    let plan = parse(explain_text)?;
    let report = analyze(&plan);

    #[derive(serde::Serialize)]
    struct FullOutput {
        plan: ogexplain_core::model::ExplainPlan,
        report: ogexplain_core::analyzer::report::DiagnosticReport,
        heatmap: Option<ogexplain_core::analyzer::heatmap::PlanHeatmap>,
        waterfall: Option<ogexplain_core::analyzer::waterfall::PlanWaterfall>,
    }

    let output = FullOutput {
        plan,
        report,
        heatmap: heatmap(&explain_text.parse()?),
        waterfall: waterfall(&explain_text.parse()?),
    };

    Ok(serde_json::to_string_pretty(&output)?)
}
```

---

## 6. Build and Test

### 6.1 Feature Flags

The workspace uses feature flags to gate optional components:

| Flag | Scope | Effect |
|------|-------|--------|
| `default` | Workspace root | Empty (no binaries by default) |
| `full` | Workspace root | Enables `cli` + `tui` + `mcp` |
| `cli` | Workspace root | Builds `ogexplain` binary |
| `tui` | Workspace root | Builds `ogexplain-tui` binary |
| `mcp` | Workspace root | Builds `ogexplain-mcp` binary + CLI `mcp` subcommand |
| `db` | ogexplain-cli | Enables `explain` subcommand (tokio-postgres, default on) |
| `mcp` | ogexplain-cli | Enables `mcp` subcommand via CLI |

### 6.2 Build Commands

```bash
# Build everything
cargo build --workspace

# Build specific crates
cargo build -p ogexplain-core          # Core library only
cargo build -p ogexplain-cli           # CLI binary
cargo build -p ogexplain-tui           # TUI binary
cargo build -p ogexplain-mcp           # MCP server

# Build with all features (full workspace including MCP via CLI)
cargo build --workspace --features full

# Build CLI with database support (default)
cargo build -p ogexplain-cli

# Build CLI without database support
cargo build -p ogexplain-cli --no-default-features

# Release build
cargo build --workspace --release
```

### 6.3 Test Types

**Unit tests** — per-module `#[cfg(test)]` blocks testing individual functions:
```bash
cargo test -p ogexplain-core
```

**Integration tests** — parser snapshot tests and analyzer rule tests:
```bash
cargo test --test integration_tests    # insta snapshot tests for all 31 fixtures
cargo test --test analyzer_tests       # positive/negative tests for diagnostic rules
```

**Insta snapshot tests** — the parser uses `insta::assert_yaml_snapshot!` for regression testing. When you add a new fixture, the first run creates a pending snapshot. Review and accept:
```bash
cargo insta test                        # Run tests, creating pending snapshots
cargo insta review                      # Interactive review of pending snapshots
cargo insta accept                      # Accept all pending snapshots
```

**MCP server tests** — unit tests against `OgexplainServer` methods:
```bash
cargo test -p ogexplain-mcp
```

**Database integration tests** — require a running OpenGauss instance (Docker):
```bash
cargo test --test db_explain --features ogexplain-cli/db
```

### 6.4 Linting

The project enforces zero clippy warnings:

```bash
cargo fmt --all -- --check             # Format check
cargo fmt --all                        # Auto-format
cargo clippy --workspace               # Lint (must be zero warnings)
cargo clippy --workspace -- -D warnings # Treat warnings as errors
```

### 6.5 CI Expectations

A typical CI pipeline for this project should:

1. **Format check**: `cargo fmt --all -- --check`
2. **Clippy**: `cargo clippy --workspace -- -D warnings` (zero warnings required)
3. **Test**: `cargo test --workspace` (all tests pass)
4. **Snapshot check**: `cargo insta test && cargo insta check` (no pending snapshots)
5. **Optional**: `cargo-deny check` for dependency auditing

### 6.6 Adding Test Fixtures

Test fixtures are raw EXPLAIN TEXT files in `tests/fixtures/`. They are named with a numeric prefix for ordering:

```
tests/fixtures/01_simple_seq_scan.txt
tests/fixtures/03_hash_join.txt
tests/fixtures/10_complex_plan.txt
...
tests/fixtures/31_my_new_scenario.txt
```

Each fixture should exercise a specific parsing or diagnostic scenario. Add a corresponding test in `tests/integration_tests.rs`:

```rust
#[test]
fn test_fixture_31() {
    let plan = parse(include_str!("fixtures/31_my_new_scenario.txt")).unwrap();
    insta::assert_yaml_snapshot!(plan);
}
```

### 6.7 Documentation

Build rustdoc for all crates:

```bash
cargo doc --workspace --no-deps --open
```

The `ogexplain-core` crate's public items should all have doc comments. New public API additions must include `///` doc comments to keep `cargo doc` warning-free.

---

## Appendix: Key Module Map

Quick reference for finding specific functionality:

| What you want | Where to find it |
|---------------|-----------------|
| Parse EXPLAIN text | `ogexplain_core::parse()` / `parser/mod.rs` |
| Line classification regex | `parser/line_classifier.rs` |
| Tree building logic | `parser/tree_builder.rs` |
| Plan node types (80+ variants) | `model/node_type.rs` |
| Streaming type enum | `model/streaming.rs` |
| Join type enum | `model/join_type.rs` |
| Cost/stats structs | `model/cost.rs` |
| Buffer stats | `model/buffer.rs` |
| ExplainPlan, PlanNode, PlanSummary | `model/plan.rs` |
| DiagnosticConfig, DiagnosticEngine | `analyzer/config.rs` |
| DiagnosticRule trait | `analyzer/rules/mod.rs` |
| All 25 rule implementations | `analyzer/rules/*.rs` (17 files) |
| Shared rule utilities | `analyzer/rules/utils.rs` |
| Finding, Severity, DiagnosticCategory | `analyzer/report.rs` |
| PlanContext, GlobalStats | `analyzer/context.rs` |
| Heatmap types and engine | `analyzer/heatmap/` |
| Waterfall types and engine | `analyzer/waterfall/` |
| Anti-pattern engine | `analyzer/pattern/` |
| Cross-rule suggestion engine | `suggester/mapper.rs` |
| Suggestion types | `suggester/suggestion.rs` |
| SQL rewrite (SUBQ-006) | `rewriter/detector.rs`, `rewriter/transform.rs` |
| Batch summary (43-field CSV) | `summary.rs` |
| SQL block segmentation | `sql/` |
| i18n strings | `i18n/` (en.yml, zh-CN.yml) |
| MCP server tools | `ogexplain-mcp/src/server.rs` |
| CLI subcommand dispatch | `ogexplain-cli/src/lib.rs` |
| TUI Elm architecture | `ogexplain-tui/src/app.rs`, `action.rs`, `event.rs` |
| TUI components | `ogexplain-tui/src/components/` |
