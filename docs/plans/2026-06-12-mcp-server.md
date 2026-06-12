# ogexplain MCP Server Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add an MCP server crate (`ogexplain-mcp`) that exposes ogexplain-core's EXPLAIN parsing, diagnostics, and suggestion capabilities as MCP tools, enabling AI assistants to analyze OpenGauss query plans directly.

**Architecture:** New Cargo workspace crate `ogexplain-mcp` using the official `rmcp` Rust SDK. Stdio transport. 5 MCP tools wrapping `ogexplain-core` public API (`parse`, `analyze_with_rewrite`, `SuggestionEngine::suggest`, `SummaryRow::compute`). All model types already derive `Serialize` so JSON output is free. Zero changes to existing crates.

**Tech Stack:** Rust, `rmcp` v1.7 (official MCP SDK), `tokio` (async runtime), `ogexplain-core` (existing library), `serde_json`.

---

## Task 1: Scaffold the `ogexplain-mcp` crate

**Files:**
- Create: `crates/ogexplain-mcp/Cargo.toml`
- Create: `crates/ogexplain-mcp/src/lib.rs` (empty for now)
- Create: `crates/ogexplain-mcp/src/main.rs` (minimal skeleton)
- Modify: `Cargo.toml` (add to workspace members)

**Step 1: Create `crates/ogexplain-mcp/Cargo.toml`**

```toml
[package]
name = "ogexplain-mcp"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "MCP server for OpenGauss EXPLAIN plan analysis"

[dependencies]
ogexplain-core = { path = "../ogexplain-core" }
rmcp = { version = "1.7", features = ["server"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
```

**Step 2: Create `crates/ogexplain-mcp/src/lib.rs`**

```rust
pub mod server;
```

**Step 3: Create `crates/ogexplain-mcp/src/main.rs`**

```rust
use ogexplain_mcp::server::OgexplainServer;

#[tokio::main]
async fn main() {
    let server = OgexplainServer;
    let transport = rmcp::transport::io::stdio();
    let result = rmcp::serve_server(server, transport).await;
    if let Err(e) = result {
        eprintln!("MCP server error: {e}");
        std::process::exit(1);
    }
}
```

**Step 4: Create `crates/ogexplain-mcp/src/server.rs`** (minimal placeholder)

```rust
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, model::*};
use serde::Deserialize;
use schemars::JsonSchema;

#[derive(Debug, Clone, Default)]
pub struct OgexplainServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeExplainParams {
    /// The EXPLAIN or EXPLAIN ANALYZE output text to analyze
    pub explain_text: String,
    /// Optional original SQL text (enables SQL rewrite suggestions for correlated subqueries)
    #[serde(default)]
    pub sql_text: Option<String>,
}

#[tool_router(server_handler)]
impl OgexplainServer {
    #[tool(
        name = "analyze_explain",
        description = "Parse and analyze an OpenGauss EXPLAIN plan. Returns structured diagnostic findings with severity, rule IDs, suggestions, and a summary."
    )]
    async fn analyze_explain(
        &self,
        Parameters(params): Parameters<AnalyzeExplainParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Placeholder — will be implemented in Task 3
        let content = Content::text("not yet implemented");
        Ok(CallToolResult::success(vec![content]))
    }
}
```

**Step 5: Add to workspace root `Cargo.toml`**

Add `"crates/ogexplain-mcp"` to the `workspace.members` array.

**Step 6: Verify it compiles**

Run: `cargo build -p ogexplain-mcp`
Expected: Compiles with zero errors (tool macro generates ServerHandler impl).

**Step 7: Commit**

```
feat(mcp): scaffold ogexplain-mcp crate with placeholder tool
```

---

## Task 2: Define all MCP tool input/output types

**Files:**
- Modify: `crates/ogexplain-mcp/src/server.rs` (add all param structs)

**Context:** Each MCP tool needs a `Deserialize + JsonSchema` input struct. Output goes through `Content::json()` which serializes any `Serialize` type. The `ogexplain-core` types (`DiagnosticReport`, `ExplainPlan`, `Suggestion`, `SummaryRow`) already derive `Serialize`.

**Step 1: Define all parameter structs**

Add to `crates/ogexplain-mcp/src/server.rs`:

```rust
// ---- Tool parameter structs ----

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeExplainParams {
    /// The EXPLAIN or EXPLAIN ANALYZE output text to analyze
    pub explain_text: String,
    /// Optional original SQL text (enables SQL rewrite suggestions for correlated subqueries)
    #[serde(default)]
    pub sql_text: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ParseExplainParams {
    /// The EXPLAIN output text to parse into a structured plan tree
    pub explain_text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSuggestionsParams {
    /// The EXPLAIN output text to analyze for cross-rule optimization suggestions
    pub explain_text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScoreSqlComplexityParams {
    /// SQL statement to score for complexity
    pub sql_text: String,
}

// No params needed for list_diagnostic_rules — takes nothing
```

**Step 2: Verify compilation**

Run: `cargo build -p ogexplain-mcp`
Expected: PASS

**Step 3: Commit**

```
feat(mcp): add all MCP tool parameter type definitions
```

---

## Task 3: Implement `analyze_explain` tool (core tool)

**Files:**
- Modify: `crates/ogexplain-mcp/src/server.rs`

**Context:** This is the primary tool. It calls `ogexplain_core::parse()` then `ogexplain_core::analyze_with_rewrite()`. On parse error, return an `ErrorData`. On success, return the `DiagnosticReport` as JSON content plus a human-readable text summary.

**Step 1: Implement the tool handler**

Replace the placeholder `analyze_explain` method:

```rust
#[tool(
    name = "analyze_explain",
    description = "Parse and analyze an OpenGauss EXPLAIN plan. Returns structured diagnostic findings with severity, rule IDs, suggestions, and a summary."
)]
async fn analyze_explain(
    &self,
    Parameters(params): Parameters<AnalyzeExplainParams>,
) -> Result<CallToolResult, ErrorData> {
    let plan = ogexplain_core::parse(&params.explain_text)
        .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

    let report = ogexplain_core::analyze_with_rewrite(&plan, params.sql_text.as_deref());

    let mut contents = Vec::new();

    // Structured JSON output
    contents.push(
        Content::json(&report)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    );

    // Human-readable text summary
    let text_summary = format_text_summary(&report);
    contents.push(Content::text(text_summary));

    Ok(CallToolResult::success(contents))
}
```

**Step 2: Add the `format_text_summary` helper function**

```rust
fn format_text_summary(report: &ogexplain_core::analyzer::report::DiagnosticReport) -> String {
    let mut lines = Vec::new();
    let findings = &report.findings;

    if findings.is_empty() {
        lines.push("No diagnostic issues found. The execution plan looks healthy.".to_string());
        return lines.join("\n");
    }

    lines.push(format!("Found {} diagnostic issue(s):\n", findings.len()));

    for f in findings {
        let severity_icon = match f.severity {
            ogexplain_core::analyzer::report::Severity::Critical => "CRITICAL",
            ogexplain_core::analyzer::report::Severity::Warning => "WARNING",
            ogexplain_core::analyzer::report::Severity::Info => "INFO",
        };
        lines.push(format!("[{}] {} - {}", severity_icon, f.rule_id, f.title));
        if let Some(ref suggestion) = f.suggestion {
            lines.push(format!("  Suggestion: {}", suggestion));
        }
    }

    lines.join("\n")
}
```

**Step 3: Verify compilation**

Run: `cargo build -p ogexplain-mcp`
Expected: PASS

**Step 4: Commit**

```
feat(mcp): implement analyze_explain tool with structured + text output
```

---

## Task 4: Implement `parse_explain` tool (plan tree inspection)

**Files:**
- Modify: `crates/ogexplain-mcp/src/server.rs`

**Context:** This tool returns the parsed `ExplainPlan` tree (node types, relations, costs, actual stats, properties) as structured JSON. Useful when the AI needs to inspect the plan structure itself rather than just diagnostics.

**Step 1: Add the tool method**

```rust
#[tool(
    name = "parse_explain",
    description = "Parse EXPLAIN text into a structured plan tree with node types, costs, actual stats, and properties. Use this when you need to inspect the plan structure rather than run diagnostics."
)]
async fn parse_explain(
    &self,
    Parameters(params): Parameters<ParseExplainParams>,
) -> Result<CallToolResult, ErrorData> {
    let plan = ogexplain_core::parse(&params.explain_text)
        .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

    let content = Content::json(&plan)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![content]))
}
```

**Step 2: Verify compilation**

Run: `cargo build -p ogexplain-mcp`
Expected: PASS

**Step 3: Commit**

```
feat(mcp): implement parse_explain tool for plan tree inspection
```

---

## Task 5: Implement `list_diagnostic_rules` tool (reference)

**Files:**
- Modify: `crates/ogexplain-mcp/src/server.rs`

**Context:** Returns static metadata about all 25 diagnostic rules. This helps the AI understand what checks are available and ask for specific analyses. No input parameters needed.

**Step 1: Define rule metadata struct and the tool method**

```rust
use serde::Serialize;

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RuleInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
}

#[tool(
    name = "list_diagnostic_rules",
    description = "List all available diagnostic rules with IDs, categories, and descriptions. Use this to understand what checks the analyzer performs."
)]
async fn list_diagnostic_rules(&self) -> Result<CallToolResult, ErrorData> {
    let rules = vec![
        RuleInfo { id: "SCAN-001".into(), name: "Large table full scan".into(), category: "scan".into(), description: "Detects sequential scans on large tables exceeding row threshold".into() },
        RuleInfo { id: "SCAN-004".into(), name: "Filter without index".into(), category: "scan".into(), description: "Filter removing many rows without index support".into() },
        RuleInfo { id: "JOIN-001".into(), name: "Nested loop on large tables".into(), category: "join".into(), description: "Nested loop join with high row counts".into() },
        RuleInfo { id: "JOIN-002".into(), name: "Hash join spill to disk".into(), category: "join".into(), description: "Hash join exceeding work_mem and spilling to disk".into() },
        RuleInfo { id: "MEM-001".into(), name: "Sort spill to disk".into(), category: "memory".into(), description: "External merge sort spilling to disk".into() },
        RuleInfo { id: "MEM-004".into(), name: "High peak memory".into(), category: "memory".into(), description: "Locates highest-memory node in subtree".into() },
        RuleInfo { id: "SORT-003".into(), name: "Duplicate sort".into(), category: "sort".into(), description: "Detects duplicate sort operations in the plan".into() },
        RuleInfo { id: "NET-001".into(), name: "Broadcast large data".into(), category: "network".into(), description: "Broadcasting excessive rows across datanodes".into() },
        RuleInfo { id: "EST-001".into(), name: "Severe row estimation error".into(), category: "estimation".into(), description: "Actual rows far exceed or fall below optimizer estimate".into() },
        RuleInfo { id: "EST-004".into(), name: "Nested loop from underestimation".into(), category: "estimation".into(), description: "Nested Loop caused by row underestimation".into() },
        RuleInfo { id: "PUSH-001".into(), name: "Query not pushed down".into(), category: "pushdown".into(), description: "FQS failure — query not shipped to datanodes".into() },
        RuleInfo { id: "PUSH-002".into(), name: "Multi-layer streaming".into(), category: "pushdown".into(), description: "Excessive streaming layers between datanodes".into() },
        RuleInfo { id: "TYPE-001".into(), name: "Implicit type coercion".into(), category: "type_coercion".into(), description: "Hidden implicit type casts in conditions".into() },
        RuleInfo { id: "TYPE-004".into(), name: "LIKE with leading wildcard".into(), category: "type_coercion".into(), description: "LIKE pattern starting with wildcard prevents index usage".into() },
        RuleInfo { id: "VEC-001".into(), name: "Mixed row/vector engines".into(), category: "vectorization".into(), description: "Row and vector engine boundaries with adapter overhead".into() },
        RuleInfo { id: "GEN-001".into(), name: "Plan too deep".into(), category: "general".into(), description: "Execution plan exceeds depth threshold".into() },
        RuleInfo { id: "SUBQ-001".into(), name: "Subquery not pulled up".into(), category: "subquery".into(), description: "SubqueryScan nodes preventing optimization".into() },
        RuleInfo { id: "REW-001".into(), name: "Large IN list not rewritten".into(), category: "subquery".into(), description: "IN lists with many values that should use EXISTS".into() },
        RuleInfo { id: "SUBQ-006".into(), name: "Correlated subquery self-update".into(), category: "subquery".into(), description: "Self-referencing correlated subqueries in UPDATE/DELETE".into() },
        RuleInfo { id: "AGG-001".into(), name: "Group aggregate should be hash".into(), category: "aggregate".into(), description: "Group Aggregate should use Hash Aggregate for large data".into() },
        RuleInfo { id: "AGG-002".into(), name: "Hash aggregate spill to disk".into(), category: "aggregate".into(), description: "Hash Aggregate exceeding work_mem".into() },
        RuleInfo { id: "SKEW-001".into(), name: "Data skew detected".into(), category: "distribution".into(), description: "Uneven row distribution across datanodes".into() },
        RuleInfo { id: "DIST-001".into(), name: "Distribution column mismatch".into(), category: "distribution".into(), description: "Join columns don't match distribution columns".into() },
        RuleInfo { id: "STATS-001".into(), name: "Stats not collected".into(), category: "stats".into(), description: "Tables with missing or stale statistics".into() },
        RuleInfo { id: "PART-001".into(), name: "Partition pruning failure".into(), category: "partition".into(), description: "Full partition scan when pruning should reduce partitions".into() },
    ];

    let content = Content::json(&rules)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![content]))
}
```

**Step 2: Verify compilation**

Run: `cargo build -p ogexplain-mcp`
Expected: PASS

**Step 3: Commit**

```
feat(mcp): implement list_diagnostic_rules tool
```

---

## Task 6: Implement `get_suggestions` tool (cross-rule synthesis)

**Files:**
- Modify: `crates/ogexplain-mcp/src/server.rs`

**Context:** Uses `SuggestionEngine::suggest()` which takes `&[Finding]` and returns cross-rule synthesis suggestions (multi-spill → work_mem, multi-estimation → stale stats, scan+join → composite index, etc.).

**Step 1: Add the tool method**

```rust
#[tool(
    name = "get_suggestions",
    description = "Analyze EXPLAIN plan and return cross-rule optimization suggestions. Synthesizes multiple diagnostic findings into higher-level recommendations (e.g., multiple spills → increase work_mem, scan+join → composite index)."
)]
async fn get_suggestions(
    &self,
    Parameters(params): Parameters<GetSuggestionsParams>,
) -> Result<CallToolResult, ErrorData> {
    let plan = ogexplain_core::parse(&params.explain_text)
        .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

    let report = ogexplain_core::analyze(&plan);
    let suggestions = ogexplain_core::suggester::SuggestionEngine::suggest(&report.findings);

    let mut contents = Vec::new();

    contents.push(
        Content::json(&suggestions)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
    );

    if suggestions.is_empty() {
        contents.push(Content::text(
            "No cross-rule synthesis suggestions. Individual diagnostic findings may still have per-rule suggestions.",
        ));
    }

    Ok(CallToolResult::success(contents))
}
```

**Step 2: Verify compilation**

Run: `cargo build -p ogexplain-mcp`
Expected: PASS

**Step 3: Commit**

```
feat(mcp): implement get_suggestions tool with cross-rule synthesis
```

---

## Task 7: Implement `score_sql_complexity` tool

**Files:**
- Modify: `crates/ogexplain-mcp/src/server.rs`

**Context:** Uses `ogsql_complexity` crate to score SQL statements. Need to check the crate's public API first, then wrap it. If the crate API is complex or requires async, simplify to a basic wrapper.

**Step 1: Check `ogsql-complexity` public API**

Read `crates/ogsql-complexity/src/lib.rs` to find the scoring function signature. Based on the project README, it provides:
- Standard scoring (0-100)
- GaussDB scoring (4 dimensions)
- SQL classification and tags

The exact function signature will determine the implementation. Expected pattern:

```rust
#[tool(
    name = "score_sql_complexity",
    description = "Score SQL statement complexity (0-100) with GaussDB-specific dimensions: SQL structure, PL logic, advanced features, extensions."
)]
async fn score_sql_complexity(
    &self,
    Parameters(params): Parameters<ScoreSqlComplexityParams>,
) -> Result<CallToolResult, ErrorData> {
    // Call ogsql-complexity scoring API
    // Return structured result with score, level, dimensions, tags
}
```

**Step 2: Implement based on actual API**

The implementer must read `crates/ogsql-complexity/src/lib.rs` and `crates/ogexplain-core/src/summary.rs` (which references `ComplexityInput`) to understand the interface, then implement the tool.

**Step 3: Verify compilation**

Run: `cargo build -p ogexplain-mcp`
Expected: PASS

**Step 4: Commit**

```
feat(mcp): implement score_sql_complexity tool
```

---

## Task 8: Add workspace binary entry and feature gate

**Files:**
- Modify: `Cargo.toml` (root) — add `mcp` feature and binary entry

**Step 1: Add feature and binary to root `Cargo.toml`**

```toml
[features]
default = []
full = ["cli", "tui", "mcp"]
cli = ["dep:ogexplain-cli", "dep:anyhow"]
tui = ["dep:ogexplain-tui", "dep:color-eyre"]
mcp = ["dep:ogexplain-mcp"]  # NEW

# Add to [dependencies]:
ogexplain-mcp = { path = "crates/ogexplain-mcp", optional = true }

# Add to [[bin]]:
[[bin]]
name = "ogexplain-mcp"
path = "src/bin/ogexplain-mcp.rs"
required-features = ["mcp"]
```

**Step 2: Create `src/bin/ogexplain-mcp.rs`**

```rust
fn main() {
    ogexplain_mcp::main() // or directly use the main from the crate
}
```

Actually, since `ogexplain-mcp` already has its own binary in `crates/ogexplain-mcp/src/main.rs`, we just need the workspace binary to forward to it. But wait — the root workspace can just reference the binary from the crate directly. Let's keep it simple: the root `Cargo.toml` binary entry should point to `crates/ogexplain-mcp/src/main.rs` directly, OR we add it as an optional dependency and binary.

The simplest approach: add `ogexplain-mcp` as optional dep + binary entry in root `Cargo.toml`, with the bin path pointing to a thin wrapper that re-exports.

**Step 3: Verify full workspace builds**

Run: `cargo build --workspace`
Expected: All crates compile. `ogexplain-mcp` binary is built when `--features mcp` is used.

**Step 4: Commit**

```
feat(mcp): add workspace feature gate and binary entry
```

---

## Task 9: Integration test — MCP tool invocation via in-process transport

**Files:**
- Create: `crates/ogexplain-mcp/tests/integration.rs`

**Context:** Test the MCP server by creating an in-process client-server pair. The `rmcp` crate provides `serve_client`/`serve_server` with in-memory transports.

**Step 1: Write integration test**

```rust
use rmcp::ServiceExt;

#[tokio::test]
async fn test_analyze_explain_tool() {
    let server = ogexplain_mcp::server::OgexplainServer;

    // Create in-memory transport
    let (client_io, server_io) = tokio::io::duplex(4096);
    let (client_sink, client_stream) = client_io.into_split();
    let (server_sink, server_stream) = server_io.into_split();

    // Start server
    let server_handle = tokio::spawn(async move {
        rmcp::serve_server(server, (server_stream, server_sink))
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap()
    });

    // Start client
    let client = rmcp::handler::client::ClientHandler::default();
    let running_client = rmcp::serve_client(client, (client_sink, client_stream))
        .await
        .unwrap();

    // List tools
    let tools = running_client
        .peer()
        .list_tools(Default::default())
        .await
        .unwrap();
    assert!(tools.tools.len() >= 5, "Should have at least 5 tools");

    // Call analyze_explain with a simple plan
    let explain_text = "Seq Scan on t  (cost=0.00..12.00 rows=200 width=4)";
    let result = running_client
        .peer()
        .call_tool(rmcp::model::CallToolRequestParam {
            name: "analyze_explain".into(),
            arguments: serde_json::json!({ "explain_text": explain_text })
                .as_object()
                .cloned(),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(!result.is_error.unwrap_or(false), "Tool should not return error");
    assert!(!result.content.is_empty(), "Tool should return content");

    // Cleanup
    server_handle.abort();
}
```

Note: The exact `rmcp` client API for in-process testing may vary. The implementer should check `rmcp` examples for the correct pattern. The test above is the expected shape but may need adjustment based on actual `rmcp` API.

**Step 2: Run the test**

Run: `cargo test -p ogexplain-mcp`
Expected: PASS

**Step 3: Commit**

```
test(mcp): add integration test for analyze_explain tool
```

---

## Task 10: Update AGENTS.md and workspace documentation

**Files:**
- Modify: `AGENTS.md` — add ogexplain-mcp section
- Modify: `Cargo.toml` comments if needed

**Step 1: Add MCP crate to AGENTS.md**

In the project structure table, add:

```
| `ogexplain-mcp` | library + binary | MCP server — exposes core analysis as MCP tools for AI assistants |
```

In the build commands, add:

```bash
cargo build -p ogexplain-mcp                        # MCP server binary
cargo run -p ogexplain-mcp --bin ogexplain-mcp       # Run MCP server (stdio)
cargo build --features mcp                           # Build with MCP support
```

Add MCP configuration example for Claude Desktop / Cursor:

```json
{
  "mcpServers": {
    "ogexplain": {
      "command": "ogexplain-mcp",
      "args": []
    }
  }
}
```

**Step 2: Commit**

```
docs: add ogexplain-mcp crate to project documentation
```

---

## Task 11: Final verification — full workspace build + clippy + test

**Step 1: Full workspace build**

Run: `cargo build --workspace`
Expected: All 5 crates compile.

**Step 2: Clippy**

Run: `cargo clippy --workspace`
Expected: Zero warnings.

**Step 3: All tests**

Run: `cargo test --workspace`
Expected: All existing 317 tests + new MCP tests pass.

**Step 4: Format**

Run: `cargo fmt --all -- --check`
Expected: No formatting issues.

**Step 5: Final commit (if any fixes needed)**

```
chore: fix clippy/format issues in ogexplain-mcp
```

---

## Summary

| Task | Description | Dependencies |
|------|-------------|-------------|
| 1 | Scaffold crate + Cargo.toml + workspace | None |
| 2 | Define all tool param structs | Task 1 |
| 3 | Implement `analyze_explain` (core tool) | Task 2 |
| 4 | Implement `parse_explain` | Task 2 |
| 5 | Implement `list_diagnostic_rules` | Task 2 |
| 6 | Implement `get_suggestions` | Task 2 |
| 7 | Implement `score_sql_complexity` | Task 2 |
| 8 | Workspace feature gate + binary entry | Task 1 |
| 9 | Integration test | Tasks 3-7 |
| 10 | Update docs | Task 8 |
| 11 | Final verification | All |

**Parallelizable:** Tasks 3-7 can all run in parallel after Task 2. Task 8 can run after Task 1. Task 9 needs all tools implemented.
