# ogexplain-analyzer User Guide

OpenGauss/GaussDB EXPLAIN plan parser, performance diagnostics, and optimization advisor.

---

## Table of Contents

1. [Introduction](#introduction)
2. [Installation](#installation)
3. [CLI Usage](#cli-usage)
   - [analyze Subcommand](#analyze-subcommand)
   - [explain Subcommand (DB-Connected)](#explain-subcommand-db-connected)
   - [mcp Subcommand](#mcp-subcommand)
   - [Output Formats](#output-formats)
   - [Reading from Pipe/Stdin](#reading-from-pipestdin)
   - [Internationalization (i18n)](#internationalization-i18n)
4. [TUI Usage](#tui-usage)
   - [Launch Modes](#launch-modes)
   - [Command Mode](#command-mode)
   - [Screen Layout](#screen-layout)
   - [Keybindings](#keybindings)
   - [Tree Display](#tree-display)
5. [Interpreting Results](#interpreting-results)
   - [Severity Levels](#severity-levels)
   - [Finding Structure](#finding-structure)
   - [Suggestion Categories](#suggestion-categories)
   - [Cross-Rule Synthesis](#cross-rule-synthesis)
   - [Heatmap: Cost-Actual Deviation](#heatmap-cost-actual-deviation)
   - [Waterfall: CPU and Memory Bottlenecks](#waterfall-cpu-and-memory-bottlenecks)
6. [MCP Integration with AI Assistants](#mcp-integration-with-ai-assistants)
   - [What Is MCP?](#what-is-mcp)
   - [Configuration](#configuration)
   - [MCP Tools Reference](#mcp-tools-reference)
   - [Integration with gaussdb-mcp](#integration-with-gaussdb-mcp)
7. [Common Workflows](#common-workflows)
   - [Quick Health Check](#quick-health-check)
   - [Detailed JSON Export](#detailed-json-export)
   - [Batch CSV Reporting](#batch-csv-reporting)
   - [DB-Connected Analysis](#db-connected-analysis)
   - [AI-Assisted Analysis via TUI](#ai-assisted-analysis-via-tui)
8. [Troubleshooting](#troubleshooting)
9. [Diagnostic Rules Reference](#diagnostic-rules-reference)

---

## Introduction

**ogexplain-analyzer** is a command-line and interactive tool for parsing and diagnosing OpenGauss and GaussDB `EXPLAIN` and `EXPLAIN ANALYZE` output. It helps database administrators (DBAs) and developers identify performance bottlenecks in SQL execution plans and provides actionable optimization suggestions.

### Who Is This For?

- **Database Administrators** monitoring and tuning OpenGauss/GaussDB query performance
- **Application Developers** optimizing SQL queries before deploying to production
- **Data Engineers** running analytical workloads on distributed GaussDB clusters
- **AI/ML Engineers** using AI assistants (Claude, Cursor, VS Code Copilot) for SQL performance debugging

### What It Does

- Parses TEXT-format `EXPLAIN` and `EXPLAIN ANALYZE` output (including OpenGauss-specific features: vector nodes, CStore scans, streaming operators, pretty mode)
- Runs **25 diagnostic rules** across 15 categories (scan, join, memory, sort, network, estimation, pushdown, type coercion, vectorization, subquery, aggregate, distribution, statistics, partition, and general plan health)
- Generates **parameterized suggestions** with concrete values extracted from the plan (e.g., `CREATE INDEX ON orders(status)`)
- Produces **cost deviation heatmaps** showing estimation accuracy per node with Q-error severity levels
- Produces **resource waterfalls** identifying CPU and memory bottlenecks
- Scores **SQL complexity** on a 0-100 scale with GaussDB-specific four-dimension analysis
- Supports **batch processing** of multi-statement files with CSV summary export

---

## Installation

### Prerequisites

- **Rust** 1.70+ (install via [rustup](https://rustup.rs/))
- **C compiler** (gcc or clang) for native dependencies
- For DB-connected mode: OpenGauss/GaussDB client libraries (optional)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/c2j/ogexplain-analyzer.git
cd ogexplain-analyzer

# Build all workspace crates (release mode for production)
cargo build --release

# Verify the build
./target/release/ogexplain --version
./target/release/ogexplain-tui --version
```

### Binary Locations

After a successful build, the binaries are located at:

| Binary | Path | Purpose |
|--------|------|---------|
| `ogexplain` | `./target/release/ogexplain` | CLI frontend |
| `ogexplain-tui` | `./target/release/ogexplain-tui` | Interactive TUI |
| `ogexplain-mcp` | `./target/release/ogexplain-mcp` | MCP server for AI assistants |

### Installing to PATH (Optional)

```bash
# Install all binaries to ~/.cargo/bin/
cargo install --path crates/ogexplain-cli
cargo install --path crates/ogexplain-tui
cargo install --path crates/ogexplain-mcp
```

### Feature Flags

The CLI has optional features that can be enabled at build time:

```bash
# Build with database connectivity (for the explain subcommand)
cargo build -p ogexplain-cli --features db

# Build with MCP server support in the unified CLI
cargo build -p ogexplain-cli --features mcp

# Build everything
cargo build --workspace
```

---

## CLI Usage

The CLI binary `ogexplain` provides three subcommands:

```bash
ogexplain <subcommand> [options]
```

### analyze Subcommand

The `analyze` subcommand parses an EXPLAIN output file and runs all 25 diagnostic rules.

```bash
ogexplain analyze <file> [options]
```

#### Options

| Option | Short | Default | Description |
|--------|-------|---------|-------------|
| `--output <format>` | `-o` | `text` | Output format: `text`, `json`, `heatmap`, `waterfall` |
| `--threshold <level>` | | `info` | Minimum severity to report: `critical`, `warning`, `info` |
| `--quiet` | `-q` | off | Show findings only; omit the plan tree |
| `--verbose` | `-v` | off | Verbose output with additional details |
| `--multi` | | off | Enable multi-block parsing for files with mixed SQL + EXPLAIN blocks |
| `--csv <path>` | | | Export 43-column summary to CSV file; use `-` for stdout |
| `--lang <code>` | | `auto` | Language: `en`, `zh-CN`, or `auto` (detect from system locale) |

#### Examples

```bash
# Basic text report
ogexplain analyze explain_output.txt

# JSON output for programmatic consumption
ogexplain analyze explain_output.txt -o json

# Only show critical and warning findings
ogexplain analyze explain_output.txt --threshold warning

# Heatmap (requires EXPLAIN ANALYZE data)
ogexplain analyze explain_analyze_output.txt -o heatmap

# Waterfall (requires EXPLAIN ANALYZE data)
ogexplain analyze explain_analyze_output.txt -o waterfall

# Findings only, no plan tree
ogexplain analyze explain_output.txt -q

# Multi-block file with CSV export
ogexplain analyze mixed_sql_explain.txt --multi --csv results.csv

# Pipe-friendly: export CSV to stdout
ogexplain analyze explain_output.txt --csv -
```

### explain Subcommand (DB-Connected)

The `explain` subcommand connects directly to an OpenGauss/GaussDB database, runs `EXPLAIN` or `EXPLAIN ANALYZE`, and analyzes the result in one step. Requires the `db` feature flag.

```bash
ogexplain explain -s <sql> [options]
```

Connection info is loaded from a config file (`--config <path>`, default `~/.gaussdb-mcp.toml`) or the `GAUSSDB_URL` / `DATABASE_URL` environment variable. The `-d/--dsn` flag was removed so credentials never appear on the command line.

#### Options

| Option | Short | Description |
|--------|-------|-------------|
| `--config <path>` | | Path to TOML config file (default: `~/.gaussdb-mcp.toml`) |
| `--name <name>` | | Named connection from `[[connections]]` in config file |
| `--sql <statement>` | `-s` | SQL statement to explain (inline) |
| `--sql-file <path>` | `-f` | SQL statement from file |
| `--analyze` | | Run `EXPLAIN ANALYZE` (actually executes the query) |
| `--output <format>` | `-o` | Output format (default: `text`) |
| `--threshold <level>` | | Minimum severity (default: `info`) |
| `--quiet` | `-q` | Findings only |
| `--csv <path>` | | Export CSV summary |
| `--lang <code>` | | Language (default: `auto`) |

#### Examples

```bash
# Build with database support
cargo build -p ogexplain-cli --features db

# Run EXPLAIN using the default config path (~/.gaussdb-mcp.toml)
ogexplain explain -s "SELECT * FROM orders WHERE status = 'pending'"

# Explicit config path
ogexplain explain --config /etc/ogexplain/prod.toml -s "SELECT ..."

# Named connection from multi-connection config
ogexplain explain --name prod -s "SELECT ..."

# Run EXPLAIN ANALYZE (executes the query!)
ogexplain explain -s "SELECT ..." --analyze

# Read SQL from file
ogexplain explain -f query.sql

# JSON output with CSV export
ogexplain explain -s "SELECT ..." --name prod -o json --csv results.csv --threshold warning
```

> **Warning:** The `--analyze` flag causes the query to execute on the database. Use with caution on production systems. Prefer `EXPLAIN` (without `ANALYZE`) for read-only estimation checks.

### mcp Subcommand

Starts the MCP (Model Context Protocol) server for AI assistant integration. Requires the `mcp` feature flag.

```bash
# Build with MCP support
cargo build -p ogexplain-cli --features mcp

# Start the MCP server
ogexplain mcp
```

The MCP server communicates via stdio transport. It is designed to be launched by AI assistants (Claude Desktop, Cursor, VS Code) rather than used directly. See [MCP Integration](#mcp-integration-with-ai-assistants) for configuration details.

### Output Formats

#### text (Default)

Human-readable report with the plan tree, color-coded findings grouped by severity, per-finding suggestions, and a SQL complexity section.

```
══════════════════════════════════════════════
  OpenGauss Execution Plan Analysis Report
══════════════════════════════════════════════

Execution Plan Tree
─────────
Hash Join  (cost=0.00..100.00 rows=1000 width=64  actual=0.123..5.678ms rows=987 loops=1)
   Hash Cond: orders.customer_id = customers.id
  ├── Seq Scan on orders  (cost=0.00..50.00 rows=50000 width=32  ...)
  └── Hash  (...)
        └── Seq Scan on customers  (...)

🔴 Critical (1)
──────────────────
  [SCAN-001] Large table full scan
    Node: "Seq Scan" (line 4)
    Seq Scan on "orders" (50000 rows) exceeds threshold
    Suggestion: CREATE INDEX ON orders(customer_id)

🟡 Warnings (2)
───────────────
  ...

🟢 Info (1)
──────────
  ...

💡 Suggestions
──────────────
  1. [High] Consider creating a composite index on join + filter columns
```

#### json

Structured JSON output containing the full plan tree, all findings, suggestions, diagnostic stats, complexity data, heatmap, and waterfall data. Ideal for programmatic consumption with `jq` or other tools.

```bash
ogexplain analyze explain_output.txt -o json | jq '.findings[] | select(.severity == "Critical")'
```

The JSON output includes:
- `plan` - The parsed plan tree structure
- `findings` - All diagnostic findings with severity, rule ID, detail, suggestion
- `suggestions` - Cross-rule synthesis suggestions
- `stats` - Global diagnostic statistics
- `complexity` - SQL complexity report (if SQL was provided)
- `gauss_complexity` - GaussDB four-dimension complexity (if SQL was provided)
- `heatmap` - Cost deviation heatmap (if EXPLAIN ANALYZE data available)
- `waterfall` - Resource waterfall data (if EXPLAIN ANALYZE data available)

#### heatmap

Cost-actual deviation heatmap showing estimation accuracy per node. **Requires EXPLAIN ANALYZE output** (plans with `actual` statistics).

```
═══════════════════════════════════════════
  Cost-Actual Deviation Heatmap
═══════════════════════════════════════════

Summary: Max Q-Error=45.2x  Severe=3  Deviated=7/12

   Node                              Est     Act    Q-Err  Severity
──────────────────────────────────────────────────────────────────────
🔴  Hash Join on orders            1000    45200   45.2x  Extreme
🟠  Seq Scan on orders               50    50000   100.0x Extreme
🟡  Index Scan on customers         100      987    9.9x  Moderate
🟢  Hash                             50       48    1.0x  Negligible
⚪  Hash Join on items              200      210    1.1x  Negligible

Critical Path: 3 → 4 → 1
Hotspots: [4, 1, 5]
```

#### waterfall

Resource waterfall showing CPU and memory consumption per node with percentage bars. **Requires EXPLAIN ANALYZE output**.

```
═══════════════════════════════════════════
  Resource Waterfall
═══════════════════════════════════════════

Bottlenecks: CPU=2  Memory=1  Spills=1

Node                              CPU%    Mem%    Bottleneck
──────────────────────────────────────────────────────────────
Hash Join on orders              [████████░░] 82.3%  [██░░] 41.2%  CPU,Mem
  ├── Seq Scan on orders         [█████░░░░░] 52.1%  [░░░░]  0.5%
  └── Hash                       [██░░░░░░░░] 18.4%  [████░] 80.3%  Mem,Spill
        └── Seq Scan on cust.    [████░░░░░░] 28.7%  [░░░░]  2.1%
```

### Reading from Pipe/Stdin

Use `-` as the file argument to read from stdin:

```bash
# Pipe from a file
cat explain_output.txt | ogexplain analyze -

# Pipe from a database query tool
gsql -h db.example.com -c "EXPLAIN SELECT * FROM orders" | ogexplain analyze -

# Combine with other tools
curl -s https://example.com/explain.txt | ogexplain analyze - -o json
```

### Internationalization (i18n)

The tool supports English and Chinese output. Set the language with `--lang`:

```bash
# Force English output
ogexplain analyze explain_output.txt --lang en

# Force Chinese output
ogexplain analyze explain_output.txt --lang zh-CN

# Auto-detect from system locale (default)
ogexplain analyze explain_output.txt --lang auto
```

When `--lang auto` is used, the tool detects the system's locale setting and selects the appropriate language. English is the fallback if the locale is not recognized.

---

## TUI Usage

The TUI (Terminal User Interface) provides an interactive plan browser built with ratatui. It offers a collapsible plan tree, node detail view, diagnostic findings, and SQL complexity display.

### Launch Modes

```bash
# File mode: load and parse an EXPLAIN file on startup
ogexplain-tui path/to/explain_output.txt

# Paste mode: start with empty input area, paste EXPLAIN text and parse
ogexplain-tui
```

### Command Mode

When the input area is focused, you can type commands (prefixed with `:`) into the text area and press `Ctrl+P` to execute them:

| Command | Description |
|---------|-------------|
| `:load <path>` | Load an EXPLAIN file from disk and parse it |
| `:quit` or `:q` | Quit the TUI |

Example: Type `:load /path/to/explain.txt` in the input area, then press `Ctrl+P` to load and parse the file.

### Screen Layout

The TUI screen is divided into five panels:

```
┌─────────────────── Title Bar ─────────────────────────────────────┐
│ ogexplain-analyzer          Browse mode │ ? for all keys           │
├─────────────────── Summary Bar ───────────────────────────────────┤
│ Plan: 12 nodes │ Critical:1 Warning:2 Info:3 │ C/W/I: 1/2/3      │
├───────────────────┬─────────────────── Detail ────────────────────┤
│                   │                                               │
│  Tree Panel       │  Detail Panel                                 │
│  (40%)            │  (60%)                                        │
│                   │                                               │
│  ▾ Hash Join !!   │  Node: Hash Join on orders                    │
│    ├── Seq Scan ! │  Estimated: cost=0.00..100.00 rows=1000       │
│    └── Hash       │  Actual: 0.123..5.678ms rows=987 loops=1     │
│          └── ...  │                                               │
│                   │  Findings:                                    │
│                   │  [SCAN-001] Large table full scan              │
│                   │    Suggestion: CREATE INDEX ON orders(...)     │
│                   │                                               │
├─────────────────── Input Area ────────────────────────────────────┤
│ EXPLAIN text (Ctrl+P to parse, :load to load file)               │
│ Hash Join (cost=0.00..100.00 rows=1000 width=64)                 │
│   -> Seq Scan on orders                                          │
├─────────────────── Status Bar ────────────────────────────────────┤
│ Mode: Browse │ Focus: Tree │ Plans: 1/1 │ ?: Help                 │
└───────────────────────────────────────────────────────────────────┘
```

**Panels:**

1. **Title Bar** - Shows the application name, current mode (Input/Browse), and context-sensitive hints
2. **Summary Bar** - Shows plan statistics, finding counts, and summary metrics
3. **Tree Panel** (left, 40%) - Collapsible plan tree with severity icons and category colors
4. **Detail Panel** (right, 60%) - Shows selected node details, findings, suggestions, and complexity
5. **Input Area** - Multi-line text area for pasting or editing EXPLAIN text
6. **Status Bar** - Shows current mode, focused panel, plan count, and help hint

### Keybindings

Keybindings are organized by category. When the input area is focused, most navigation keys are passed through as text input.

#### Global Shortcuts (always active)

| Key | Action |
|-----|--------|
| `Ctrl+P` | Parse the EXPLAIN text in the input area |
| `Ctrl+L` | Clear input and reset to initial state |
| `Ctrl+C` | Quit the application |
| `?` | Toggle help overlay (when not in Input focus) |
| `F1` | Toggle help overlay (always active) |
| `q` | Quit (when not in Input focus) |

#### Panel Navigation

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus forward: Tree → Detail → Input → Tree |
| `Shift+Tab` | Cycle focus backward: Tree → Input → Detail → Tree |

#### Tree Navigation (Tree focus)

| Key | Action |
|-----|--------|
| `↑` or `k` | Move selection up |
| `↓` or `j` | Move selection down |
| `g` | Jump to first node |
| `G` | Jump to last node |
| `Enter` | Expand or collapse the selected node |
| `E` | Expand all nodes in the tree |
| `W` | Collapse all nodes (keep only root expanded) |

#### Detail Panel Navigation (Detail focus)

| Key | Action |
|-----|--------|
| `↑` or `k` | Scroll up one line |
| `↓` or `j` | Scroll down one line |
| `PgUp` | Scroll up one page (20 lines) |
| `PgDn` | Scroll down one page (20 lines) |
| `Home` | Jump to top of detail |
| `End` | Jump to bottom of detail |

#### View Toggles (Tree or Detail focus)

| Key | Action |
|-----|--------|
| `r` | Toggle raw EXPLAIN text view (replaces detail panel with raw input) |
| `c` | Toggle SQL complexity section in detail panel |
| `F` or `f` | Toggle between node-specific findings and all findings view |

#### Multi-Plan Navigation (Tree or Detail focus)

When the input contains multiple EXPLAIN blocks (multi-block mode), use these keys to switch between plans:

| Key | Action |
|-----|--------|
| `n` or `N` | Switch to next plan |
| `p` or `P` | Switch to previous plan |

### Tree Display

The plan tree uses visual indicators to communicate diagnostic information at a glance:

#### Severity Icons

Nodes with diagnostic findings display severity icons to the right of the node type name:

| Icon | Color | Meaning |
|------|-------|---------|
| `!!` | Red | Critical finding on this node or its subtree |
| `!` | Yellow | Warning finding on this node or its subtree |
| `*` | Green | Informational finding on this node or its subtree |

Severity propagates up the tree: if a child has a critical finding, its parent will also display the critical icon.

#### Category Colors

Node type names are color-coded by their functional category:

| Color | Category | Example Node Types |
|-------|----------|--------------------|
| Blue | Scan | Seq Scan, Index Scan, CStore Scan |
| Magenta | Join | Hash Join, Nested Loop, Merge Join |
| Cyan | Aggregate | Hash Aggregate, Group Aggregate |
| Yellow | Sort | Sort, Vec Sort |
| Green | DML | Insert, Update, Delete |
| Red | Streaming | Streaming (GATHER, REDISTRIBUTE, BROADCAST) |

#### Expand/Collapse Indicators

| Symbol | Meaning |
|--------|---------|
| `▾` | Node is expanded (children visible) |
| `▸` | Node is collapsed (children hidden) |
| `·` | Leaf node (no children) |

---

## Interpreting Results

### Severity Levels

Diagnostic findings are classified into three severity levels:

| Level | Color | Icon | Meaning |
|-------|-------|------|---------|
| **Critical** | Red | 🔴 | Severe performance issue requiring immediate attention. Likely causes significant query slowdown. |
| **Warning** | Yellow | 🟡 | Moderate issue that may degrade performance under certain conditions. Should be investigated. |
| **Info** | Green | 🟢 | Informational finding. Not necessarily a problem, but worth being aware of for optimization. |

Use the `--threshold` flag to filter output:

```bash
# Only show critical and warning (suppress informational)
ogexplain analyze file.txt --threshold warning

# Only show critical
ogexplain analyze file.txt --threshold critical
```

### Finding Structure

Each finding contains the following fields:

| Field | Description |
|-------|-------------|
| `rule_id` | Unique rule identifier (e.g., `SCAN-001`, `JOIN-002`) |
| `title` | Short human-readable title (e.g., "Large table full scan") |
| `severity` | Critical, Warning, or Info |
| `category` | Diagnostic category (e.g., ScanEfficiency, JoinStrategy) |
| `detail` | Detailed description of the specific issue found, with concrete values from the plan |
| `node_line` | Line number in the EXPLAIN output where the affected node appears |
| `node_type` | Type of the affected node (e.g., "Seq Scan", "Hash Join") |
| `suggestion` | Parameterized optimization suggestion with table/column names |
| `sql_rewrite` | (Optional) Auto-generated SQL rewrite, available for SUBQ-006 |

Example text output for a single finding:

```
  [SCAN-001] Large table full scan
    Node: "Seq Scan" (line 4)
    Seq Scan on "orders" (50000 rows) exceeds threshold
    Suggestion: CREATE INDEX ON orders(status)
```

### Suggestion Categories

Cross-rule synthesis suggestions are organized into five categories:

| Category | Description | Example |
|----------|-------------|---------|
| **IndexOptimization** | Create new indexes or composite indexes | "Create composite index on join + filter columns" |
| **StatisticsUpdate** | Update table statistics to improve optimizer estimates | "Multiple estimation errors suggest stale statistics; run ANALYZE on all involved tables" |
| **QueryRewrite** | Rewrite the SQL query for better execution | "Rewrite correlated subquery self-update as UPDATE ... FROM" |
| **ConfigurationTuning** | Adjust database configuration parameters | "Multiple memory spills detected; increase work_mem to avoid disk I/O" |
| **DistributionOptimization** | Optimize data distribution in distributed setups | "Pushdown issues detected; review non-shippable constructs and consider data redistribution" |

### Cross-Rule Synthesis

The suggestion engine looks for **patterns across multiple findings** and generates higher-level recommendations:

| Pattern | Synthesized Suggestion |
|---------|----------------------|
| 2+ estimation errors (EST-*) | Statistics may be stale; run ANALYZE on all involved tables |
| 2+ memory spills (MEM-001, JOIN-002) | Increase work_mem to avoid disk I/O |
| Scan + Join findings together | Create composite index on join columns and filter columns |
| Pushdown findings (PUSH-*) | Review non-shippable query constructs; consider data redistribution |
| Subquery self-update (SUBQ-006) | Rewrite as UPDATE ... FROM or CTE form |
| 2+ type coercion issues (TYPE-*) | Audit all WHERE/JOIN conditions for data type consistency |
| Vectorization switching (VEC-001) | Unify engine choice (row or vector) to eliminate adapter overhead |

Each synthesized suggestion includes a **confidence score** (0.0-1.0) indicating how certain the engine is about the recommendation:

| Confidence | Range | Meaning |
|------------|-------|---------|
| High | 0.85+ | Strong evidence from multiple findings |
| Medium | 0.70-0.84 | Moderate evidence, worth investigating |
| Low | < 0.70 | Weaker signal, use judgment |

### Heatmap: Cost-Actual Deviation

The heatmap visualizes how accurately the optimizer estimated row counts versus actual execution results. It is only available for `EXPLAIN ANALYZE` output.

#### Q-Error Severity Levels

Q-Error is the standard VLDB metric for estimation accuracy: `max(actual, estimated) / min(actual, estimated)`. It is always >= 1.0 (1.0 = perfect estimation).

| Severity | Q-Error Range | Icon | Interpretation |
|----------|---------------|------|----------------|
| **Negligible** | < 2x | ⚪ | Estimation is practically accurate |
| **Mild** | 2x - 5x | 🟢 | Minor deviation, worth noting |
| **Moderate** | 5x - 10x | 🟡 | Moderate deviation, may need investigation |
| **Severe** | 10x - 50x | 🟠 | Severe deviation, likely a performance issue |
| **Extreme** | >= 50x | 🔴 | Extreme deviation, critical performance problem |

#### Key Heatmap Concepts

- **Critical Path**: The path through the plan tree (root to leaf) with the maximum cumulative Q-error. Nodes on this path are the primary targets for statistics updates.
- **Hotspots**: Nodes with Moderate or higher severity, sorted by Q-error descending. These are the most impactful nodes to fix.
- **Subtree Q-Error**: Geometric mean of Q-error across the entire subtree rooted at each node. Helps identify problematic subtrees.
- **Deviation Direction**: Whether the optimizer **underestimated** (actual > estimated, more dangerous) or **overestimated** (actual < estimated).

### Waterfall: CPU and Memory Bottlenecks

The waterfall chart shows resource consumption per plan node, helping you identify which nodes consume the most CPU time and memory.

#### Key Waterfall Concepts

- **CPU Time**: `actual.total_time_ms * loops` — the total CPU time spent on each node
- **Peak Memory**: Maximum memory used by each node (from structured properties)
- **Memory Spill**: Whether a sort or hash operation exceeded `work_mem` and spilled to disk
- **Bottleneck**: A node consuming a disproportionate share of resources (CPU or memory exceeds threshold percentage)
- **Subtree Totals**: Aggregated CPU time and peak memory for each subtree

#### Bottleneck Summary

The waterfall includes a bottleneck summary:

| Metric | Description |
|--------|-------------|
| `cpu_bottlenecks` | Top 5 nodes by CPU time (line numbers) |
| `memory_bottlenecks` | Top 5 nodes by peak memory (line numbers) |
| `total_cpu_time_ms` | Total CPU time across all nodes |
| `max_peak_memory_kb` | Maximum single-node peak memory |
| `spill_node_count` | Number of nodes with memory spill |

---

## MCP Integration with AI Assistants

### What Is MCP?

The **Model Context Protocol (MCP)** is a standard protocol for AI assistants to interact with external tools. ogexplain-analyzer provides an MCP server that exposes its analysis capabilities as tools that AI assistants can call directly.

When configured, your AI assistant can:
- Parse EXPLAIN output and identify performance issues
- Get optimization suggestions
- Score SQL complexity
- List available diagnostic rules

### Configuration

Add the following to your AI assistant's MCP server configuration:

#### Claude Desktop

Edit `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "ogexplain": {
      "command": "/path/to/ogexplain-mcp",
      "args": []
    }
  }
}
```

Or use the unified CLI with the MCP feature:

```json
{
  "mcpServers": {
    "ogexplain": {
      "command": "/path/to/ogexplain",
      "args": ["mcp"]
    }
  }
}
```

#### Cursor

Add to your Cursor MCP configuration (Settings → MCP):

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

#### VS Code (with GitHub Copilot or Continue)

Add to your `.vscode/mcp.json` or equivalent settings:

```json
{
  "servers": {
    "ogexplain": {
      "command": "ogexplain-mcp",
      "args": []
    }
  }
}
```

### MCP Tools Reference

The MCP server exposes five tools:

#### analyze_explain

Parse and analyze an EXPLAIN plan. Returns structured diagnostic findings with severity, rule IDs, suggestions, and a summary.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `explain_text` | string | Yes | The EXPLAIN or EXPLAIN ANALYZE output text |
| `sql_text` | string | No | Original SQL text (enables SQL rewrite suggestions) |

**Returns:** JSON diagnostic report + human-readable text summary.

#### parse_explain

Parse EXPLAIN text into a structured plan tree with node types, costs, actual stats, and properties. Use this when you need to inspect plan structure rather than run diagnostics.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `explain_text` | string | Yes | The EXPLAIN output text |

**Returns:** JSON plan tree structure.

#### list_diagnostic_rules

List all available diagnostic rules with IDs, categories, and descriptions. Use this to understand what checks the analyzer performs.

**Parameters:** None

**Returns:** Array of rule objects with `id`, `name`, `category`, and `description` fields.

#### get_suggestions

Analyze an EXPLAIN plan and return cross-rule optimization suggestions. Synthesizes multiple findings into higher-level recommendations (e.g., multiple spills → increase work_mem, scan + join → composite index).

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `explain_text` | string | Yes | The EXPLAIN output text to analyze |

**Returns:** Array of suggestion objects with `related_rules`, `category`, `message`, and `confidence` fields.

#### score_sql_complexity

Score a SQL statement's complexity (0-100) with GaussDB-specific dimensions. Returns both standard and GaussDB complexity scores.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `sql_text` | string | Yes | SQL statement to score |

**Returns:** Standard complexity report + GaussDB four-dimension complexity report.

### Integration with gaussdb-mcp

For end-to-end SQL performance diagnostics, combine ogexplain-mcp with [gaussdb-mcp](https://github.com/opengauss/gaussdb-mcp):

1. **gaussdb-mcp** connects to your GaussDB database and can run `EXPLAIN` / `EXPLAIN ANALYZE`
2. **ogexplain-mcp** analyzes the resulting EXPLAIN output

Workflow:
1. Ask your AI assistant: "Analyze the performance of `SELECT * FROM orders WHERE status = 'pending'`"
2. The assistant uses gaussdb-mcp to run `EXPLAIN ANALYZE` on the database
3. The assistant passes the EXPLAIN output to ogexplain-mcp's `analyze_explain` tool
4. ogexplain-mcp returns diagnostic findings and suggestions
5. The assistant presents the analysis with actionable recommendations

---

## Common Workflows

### Quick Health Check

Get a fast text report for a single EXPLAIN output file:

```bash
# Basic analysis
ogexplain analyze explain_output.txt

# Only show problems (no plan tree)
ogexplain analyze explain_output.txt -q

# Only critical issues
ogexplain analyze explain_output.txt --threshold critical -q
```

### Detailed JSON Export

Export full analysis as JSON for further processing:

```bash
# Full JSON export
ogexplain analyze explain_output.txt -o json > analysis.json

# Extract only critical findings with jq
ogexplain analyze explain_output.txt -o json | jq '.findings[] | select(.severity == "Critical")'

# Extract suggestions
ogexplain analyze explain_output.txt -o json | jq '.suggestions[]'

# Get heatmap data (requires EXPLAIN ANALYZE)
ogexplain analyze explain_analyze.txt -o json | jq '.heatmap.summary'

# Get waterfall bottleneck data
ogexplain analyze explain_analyze.txt -o json | jq '.waterfall.bottlenecks'
```

### Batch CSV Reporting

Analyze a file containing multiple SQL statements and EXPLAIN blocks:

```bash
# Parse all blocks, export 43-column CSV
ogexplain analyze mixed_workload.txt --multi --csv batch_report.csv

# CSV to stdout for piping
ogexplain analyze mixed_workload.txt --multi --csv - | head -5

# Use with threshold filtering
ogexplain analyze mixed_workload.txt --multi --csv report.csv --threshold warning
```

The CSV output contains 43 columns covering:
- SQL metadata (preview, category, sub-type, tags)
- SQL complexity metrics (tables, joins, subqueries, aggregates, CTEs, window functions, score, level)
- GaussDB complexity dimensions (sql_structure, pl_logic, advanced_feature, extension)
- Plan metrics (root operation, cost, time, rows, depth, node count)
- Performance indicators (estimation ratio, spill KB, peak memory, buffer hit rate, temp I/O)
- Diagnostic counts (critical, warning, info)

### DB-Connected Analysis

Connect directly to a database for one-step analysis:

```bash
# Quick check (EXPLAIN only, no query execution).
# Connection info is read from ~/.gaussdb-mcp.toml by default.
ogexplain explain -s "SELECT COUNT(*) FROM orders WHERE created_at > CURRENT_DATE - 7"

# Full analysis with actual execution (EXPLAIN ANALYZE)
ogexplain explain \
    -s "SELECT COUNT(*) FROM orders WHERE created_at > CURRENT_DATE - 7" \
    --analyze -o json
```

### AI-Assisted Analysis via TUI

Use the TUI in combination with AI assistants:

1. Copy the EXPLAIN output from your database client
2. Launch the TUI in paste mode: `ogexplain-tui`
3. Paste the EXPLAIN text into the input area
4. Press `Ctrl+P` to parse
5. Navigate the plan tree with arrow keys
6. Press `F` to toggle all findings view
7. Press `c` to toggle SQL complexity section
8. Press `r` to toggle raw EXPLAIN view
9. Press `?` for the full help overlay

---

## Troubleshooting

### Parse Errors

**Symptom:** `Failed to parse EXPLAIN output` or `Parse error` in the TUI.

**Common causes and fixes:**

| Cause | Fix |
|-------|-----|
| Input is not TEXT-format EXPLAIN | ogexplain-analyzer only parses TEXT format, not XML, JSON, or YAML. Use `SET explain_format = 'text';` before running EXPLAIN. |
| Partial EXPLAIN output | Ensure the entire EXPLAIN output is captured, from the first plan node to the last line. Truncated output cannot be parsed. |
| Mixed encoding (BOM, non-UTF8) | Save the EXPLAIN output as UTF-8 without BOM. Use `iconv` to convert: `iconv -f UTF-16 -t UTF-8 input.txt > output.txt` |
| Extra non-EXPLAIN text before/after | Use `--multi` to handle files with interleaved SQL and EXPLAIN blocks. Remove unrelated content otherwise. |
| PostgreSQL (non-OG) EXPLAIN output | This tool is specifically designed for OpenGauss/GaussDB. Vanilla PostgreSQL EXPLAIN output may parse partially but some OG-specific features will be missed. |

### Missing Database Features

**Symptom:** `Database support not compiled. Rebuild with --features db`

**Fix:** The `explain` subcommand requires the `db` feature:

```bash
cargo build -p ogexplain-cli --features db
```

**Symptom:** `MCP support not compiled. Rebuild with --features mcp`

**Fix:** The `mcp` subcommand requires the `mcp` feature:

```bash
cargo build -p ogexplain-cli --features mcp
```

### Heatmap/Waterfall Shows No Data

**Symptom:** Heatmap or waterfall output is empty or shows "No EXPLAIN ANALYZE data."

**Fix:** Heatmap and waterfall require `EXPLAIN ANALYZE` output (with actual execution statistics). Plain `EXPLAIN` output only contains optimizer estimates, not actual runtime data.

```sql
-- This produces estimated data only (no heatmap/waterfall)
EXPLAIN SELECT * FROM orders;

-- This produces actual execution data (enables heatmap/waterfall)
EXPLAIN ANALYZE SELECT * FROM orders;
```

### Encoding Issues

**Symptom:** Garbled characters, missing text, or unexpected parsing errors.

**Fix:** Ensure your terminal and files use UTF-8 encoding:

```bash
# Check file encoding
file -I explain_output.txt

# Convert to UTF-8 if needed
iconv -f <source-encoding> -t UTF-8 explain_input.txt > explain_utf8.txt

# Set terminal encoding (Linux/macOS)
export LANG=en_US.UTF-8
```

### No Findings Reported

**Symptom:** Analysis completes but reports "No issues found."

**Possible causes:**

1. **The plan is genuinely healthy** — the query is already well-optimized
2. **Threshold too high** — try `--threshold info` (the default) to see all findings
3. **Estimated-only plan** — some rules only fire on `EXPLAIN ANALYZE` data (e.g., EST-001 requires actual row counts)
4. **Small tables** — rules like SCAN-001 have configurable thresholds; small tables won't trigger full-scan warnings

### Connection Errors (explain subcommand)

**Symptom:** Connection refused, timeout, or authentication errors.

**Fix:** Check your config file (`~/.gaussdb-mcp.toml` by default, override with `--config <path>`):

```toml
# Example ~/.gaussdb-mcp.toml — verify these fields
host = "<hostname>"            # Use the correct hostname or IP address
port = 5432                    # Default is 5432 for OpenGauss
user = "<username>"
password = "<password>"        # Or "keyring" to read from OS keychain
dbname = "<database>"
sslmode = "disable"            # disable | require | verify-ca | verify-full
```

Common issues:
- **host**: Use the correct hostname or IP address
- **port**: Default is 5432 for OpenGauss
- **sslmode**: Use `disable` for local dev, `require` for production
- **password**: Special characters are fine inside TOML strings; no shell escaping needed
- **keyring mismatch**: If you see "keyring entry not found", the password was likely stored under service `gaussdb-mcp` (via `gaussdb-mcp store-password`) but the gaussdb library queries service `gaussdb`. Until the upstream fix ([rust-opengauss#35](https://github.com/c2j/rust-opengauss/issues/35)) lands, use a plaintext password in the config — it will be migrated back to keyring automatically later.

---

## Diagnostic Rules Reference

The analyzer implements 25 diagnostic rules across 15 categories:

### Scan Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| SCAN-001 | Large table full scan | Warning | Sequential scan on a table exceeding the row threshold. Suggests creating an appropriate index. |
| SCAN-004 | Filter without index | Warning | A filter condition removing many rows without index support. Extracts filter columns for the suggestion. |

### Join Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| JOIN-001 | Nested loop on large tables | Critical | Nested loop join with high row counts on both sides. Detects inner index presence and extracts join columns. |
| JOIN-002 | Hash join spill to disk | Warning | Hash join exceeding `work_mem` and spilling to disk. Calculates recommended `work_mem` from disk + memory sizes. |

### Memory Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| MEM-001 | Sort spill to disk | Warning | External merge sort spilling to disk (including Vec Sort). Reports the sort key in the detail. |
| MEM-004 | High peak memory | Info | Locates the node with the highest memory usage in the subtree. Reports node type and relation. |

### Sort Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| SORT-003 | Duplicate sort | Info | Identifies duplicate sort operations in the plan subtree. Distinguishes duplicate vs. different sort keys. |

### Network Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| NET-001 | Broadcast large data | Warning | Broadcasting excessive rows across datanodes. Supports SplitBroadcast and PartRedistributePartBroadcast variants. |

### Estimation Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| EST-001 | Severe row estimation error | Warning | Actual rows far exceed or fall below the optimizer's estimate. Reports direction (under/over) and ratio. |
| EST-004 | Nested loop from underestimation | Critical | Nested Loop join caused by severe row underestimation. Reports inner work quantity. |

### Pushdown Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| PUSH-001 | Query not pushed down | Warning | FQS (Fast Query Shipping) failure. Identifies specific blockers: SubqueryScan, SubPlan, volatile functions. |
| PUSH-002 | Multi-layer streaming | Info | Excessive streaming layers between datanodes. Collects the streaming chain with arrow notation. |

### Type Coercion Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| TYPE-001 | Implicit type coercion | Warning | Hidden implicit type casts in conditions (OpenGauss hides these with `showimplicit=false`). Provides specific fix suggestions. |
| TYPE-004 | LIKE with leading wildcard | Info | LIKE pattern starting with `%` prevents index usage. Distinguishes single/double wildcards; suggests `pg_trgm` + GIN index. |

### Vectorization Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| VEC-001 | Mixed row/vector engines | Warning | Row and vector engine boundaries marked by Row Adapter / Vector Adapter nodes. Tracks parent-to-child engine transitions. |

### General Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| GEN-001 | Plan too deep | Info | Execution plan exceeds depth threshold. Reports depth with reason (subquery nesting, etc.). |

### Subquery Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| SUBQ-001 | Subquery not pulled up | Info | SubqueryScan nodes preventing query optimization. Extracts child table name for parameterized suggestions. |
| REW-001 | Large IN list not rewritten | Info | IN lists with many values that should use EXISTS. Extracts column name for rewrite suggestion. |
| SUBQ-006 | Correlated subquery self-update | Critical | Self-referencing correlated subqueries in UPDATE/DELETE statements. Supports automatic SQL rewrite to `UPDATE ... FROM` syntax. |

### Aggregate Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| AGG-001 | Group aggregate should be hash | Info | Group Aggregate that should use Hash Aggregate for large GROUP BY without sort requirement. |
| AGG-002 | Hash aggregate spill to disk | Warning | Hash Aggregate exceeding `work_mem` and spilling to disk. |

### Distribution Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| SKEW-001 | Data skew detected | Warning | Uneven row distribution across datanodes in a distributed cluster. |
| DIST-001 | Distribution column mismatch | Warning | Join columns don't match distribution columns, causing data redistribution. |

### Statistics Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| STATS-001 | Stats not collected | Warning | Tables with missing or stale statistics that lead to poor optimizer estimates. |

### Partition Rules

| ID | Rule | Severity | What It Detects |
|----|------|----------|-----------------|
| PART-001 | Partition pruning failure | Warning | Full partition scan when partition pruning should reduce the number of partitions scanned. |

---

## License

ogexplain-analyzer is released under the MIT License.
