# ogexplain-analyzer

[English](README.md) | [中文](README.zh-CN.md)

OpenGauss/GaussDB `EXPLAIN` / `EXPLAIN ANALYZE` output parser and performance diagnostics tool. Parses TEXT-format execution plans, runs 25 diagnostic rules (OpenGauss-specific checks for pushdown, vectorization, streaming, implicit type coercion, and more), and outputs actionable findings with parameterized optimization suggestions.

## Features

- **Full EXPLAIN TEXT parsing** — Handles `EXPLAIN` and `EXPLAIN ANALYZE` output including pretty mode (`N --` prefix), vector nodes, CStore scans, streaming operators, and OG-specific properties.
- **25 diagnostic rules** — Covers scan, join, memory, sort, network, estimation, pushdown, type coercion, vectorization, subquery, aggregate, distribution, stats, partition, and general plan health.
- **Parameterized suggestions** — Rules extract table names, column names, and concrete values from plan properties to generate actionable suggestions (e.g., `CREATE INDEX ON orders(status)`). Cross-rule synthesis maps multiple findings to higher-level recommendations.
- **Heatmap visualization** — Cost-actual deviation heatmap shows estimation accuracy per node with Q-error severity (Negligible → Extreme).
- **Resource waterfall** — CPU & memory bottleneck analysis with waterfall charts identifying the slowest/hottest nodes.
- **SQL complexity scoring** — Integrated `ogsql-complexity` crate scores SQL statements on a 0–100 scale with GaussDB-specific dimensions (SQL structure, PL logic, advanced features, extensions).
- **SQL rewrite** — Automatically detects and rewrites correlated-subquery self-update patterns (SUBQ-006) to `UPDATE ... FROM` syntax when original SQL is provided.
- **i18n support** — English and Chinese (`zh-CN`) output via `--lang` flag or auto-detection from system locale.
- **MCP server** — Model Context Protocol server for AI assistant integration (Claude Desktop, Cursor, VS Code) with 5 tools.
- **Multiple interfaces** — CLI for scripting, TUI for interactive exploration, MCP for AI assistants, library crate for embedding.
- **DB-connected EXPLAIN** — `explain` subcommand connects directly to OpenGauss/GaussDB, runs `EXPLAIN [ANALYZE]`, and analyzes the result in one step.
- **Batch processing** — Parse multi-statement files with interleaved SQL and EXPLAIN blocks; export 43-column summary to CSV.

## Quick Start

```bash
# Build all workspace crates
cargo build --workspace

# Analyze an EXPLAIN output file (text report)
cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt

# JSON output
cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt --format json

# Heatmap output (requires EXPLAIN ANALYZE)
cargo run -p ogexplain-cli -- analyze tests/fixtures/10_complex_plan.txt --format heatmap

# Waterfall output (requires EXPLAIN ANALYZE)
cargo run -p ogexplain-cli -- analyze tests/fixtures/10_complex_plan.txt --format waterfall

# Read from stdin
cat tests/fixtures/01_simple_seq_scan.txt | cargo run -p ogexplain-cli -- analyze -

# Launch TUI with a file
cargo run -p ogexplain-tui -- tests/fixtures/10_complex_plan.txt

# Launch TUI in paste mode (Ctrl+P to parse)
cargo run -p ogexplain-tui

# Start MCP server (for AI assistants)
cargo run -p ogexplain-mcp
```

## Installation

From source:

```bash
git clone https://github.com/c2j/ogexplain-analyzer.git
cd ogexplain-analyzer
cargo build --release

# CLI binary
./target/release/ogexplain analyze file.txt

# TUI binary
./target/release/ogexplain-tui file.txt

# MCP server binary
./target/release/ogexplain-mcp
```

## Usage

### CLI

```bash
ogexplain <subcommand> [options]
```

#### Subcommand: `analyze` — Analyze EXPLAIN output file

```bash
ogexplain analyze <file> [options]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--format <format>` | `text` | Output format: `text`, `json`, `heatmap`, `waterfall` |
| `-o, --output <path>` | — | Output file path (for CSV export in txt mode) |
| `--input-format` | `txt` | Input format: `csv` (batch) or `txt` (single EXPLAIN text) |
| `--output-columns` | `minimal` | CSV batch columns: `minimal`, `focused`, `full` |
| `--threshold` | `info` | Minimum severity: `critical`, `warning`, `info` |
| `-q, --quiet` | — | Show findings only, no plan tree |
| `-v, --verbose` | — | Verbose output |
| `--multi` | — | Enable multi-block parsing (mixed SQL+EXPLAIN files) |
| `--lang` | `auto` | Language: `en`, `zh-CN`, or `auto` (system locale) |

#### Subcommand: `explain` — DB-connected EXPLAIN (requires `db` feature)

```bash
# Build with database support (default feature)
cargo build -p ogexplain-cli

# Run EXPLAIN using config file (no plaintext password on CLI)
ogexplain explain -s "SELECT * FROM orders" --config ~/.gaussdb-mcp.toml

# Or rely on the default config path (~/.gaussdb-mcp.toml)
ogexplain explain -s "SELECT * FROM orders"

# Select a named connection from config file
ogexplain explain -s "SELECT ..." --name prod

# With all analysis options
ogexplain explain -s "SELECT ..." --name prod --format json --output results.csv --threshold warning
```

> The `-d/--dsn` flag was removed. Connection info must come from a config file
> (`--config <path>`, default `~/.gaussdb-mcp.toml`) or the `GAUSSDB_URL` /
> `DATABASE_URL` environment variable. Storing credentials in a file or env var
> keeps them out of shell history and `ps` output.

**Options:**

| Option | Description |
|--------|-------------|
| `--config <path>` | Path to TOML config file (default: `~/.gaussdb-mcp.toml`) |
| `--name <name>` | Named connection from `[[connections]]` in config file |
| `-s, --sql <sql>` | SQL statement to explain (inline string) |
| `-f, --sql-file <path>` | File containing SQL statement |
| `--analyze` | Run EXPLAIN ANALYZE (executes the query) |
| `--format <fmt>` | Output format: `text`, `json`, `heatmap`, `waterfall` |
| `-o, --output <path>` | Output file path (for CSV export) |
| `--threshold <level>` | Minimum severity: `info`, `warning`, `critical` |
| `-q, --quiet` | Show findings only |
| `--lang <lang>` | Language: `en`, `zh-CN`, `auto` |

**Config file format** (reuses `~/.gaussdb-mcp.toml` from `gaussdb-mcp`):

```toml
# Flat single-connection config
host = "localhost"
port = 5432
user = "gaussdb"
password = "keyring"       # "keyring" sentinel reads from OS keychain
dbname = "mydb"
sslmode = "disable"
```

```toml
# Multi-connection config with named profiles
default_connection = "prod"

[[connections]]
name = "dev"
host = "dev.example.com"
user = "dev_user"
password = "dev_pass"
dbname = "dev_db"

[[connections]]
name = "prod"
host = "prod.example.com"
user = "prod_user"
password = "keyring"       # uses OS keychain
dbname = "prod_db"
sslmode = "verify-full"
```

**Connection resolution priority:**

1. `GAUSSDB_URL` environment variable
2. `DATABASE_URL` environment variable
3. Config file (`--config <path>` or `~/.gaussdb-mcp.toml`)
5. Error with actionable message

**Keyring support:** When `password = "keyring"` is set in the config file, the tool reads the actual password from the OS keychain using the `gaussdb-mcp` service name. Store passwords with:
```bash
gaussdb-mcp store-password
```

**Note:** `--analyze` will actually execute the query on the database. Use with caution on production systems.

#### Subcommand: `mcp` — Start MCP server (requires `mcp` feature)

```bash
cargo build -p ogexplain-cli --features mcp
ogexplain mcp
```

Starts the MCP server on stdio transport for AI assistant integration.

#### Output Formats

| Format | Description |
|--------|-------------|
| `text` | Human-readable report with plan tree, findings, suggestions, complexity section |
| `json` | Structured JSON with plan tree, findings, suggestions, stats, complexity, heatmap, and waterfall data |
| `heatmap` | Cost-actual deviation heatmap with Q-error severity levels per node (requires EXPLAIN ANALYZE) |
| `waterfall` | Resource waterfall showing CPU/memory bottlenecks with percentage bars (requires EXPLAIN ANALYZE) |

### TUI

```bash
ogexplain-tui [file]
```

**Two launch modes:**
- `ogexplain-tui file.txt` — Loads file and auto-parses on startup
- `ogexplain-tui` — Paste mode: paste EXPLAIN text, press `Ctrl+P` to parse

**Command mode** (type in input area):
- `:load <path>` — Load file from disk
- `:quit` or `:q` — Quit

**Global Shortcuts:**

| Key | Action |
|-----|--------|
| `Ctrl+P` | Parse EXPLAIN text |
| `Ctrl+L` | Clear input and reset |
| `Ctrl+C` | Quit |
| `?` / `F1` | Toggle help overlay |
| `q` | Quit (when not in input mode) |

**Panel Navigation:**

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus: Tree → Detail → Input → Tree |
| `Shift+Tab` | Reverse cycle |

**Tree Navigation** (Tree focus):

| Key | Action |
|-----|--------|
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `Enter` | Expand / collapse node |
| `E` | Expand all nodes |
| `W` | Collapse all nodes |

**Detail Panel** (Detail focus):

| Key | Action |
|-----|--------|
| `↑` / `k` | Scroll up |
| `↓` / `j` | Scroll down |
| `PgUp` | Page up |
| `PgDn` | Page down |
| `Home` | Jump to top |
| `End` | Jump to bottom |

**View Toggles:**

| Key | Action |
|-----|--------|
| `r` | Toggle raw EXPLAIN view |
| `c` | Toggle SQL complexity section |
| `F` | Toggle node diagnostics / all findings view |

**Multi-Plan Navigation** (when file contains multiple EXPLAIN blocks):

| Key | Action |
|-----|--------|
| `N` / `n` | Next plan |
| `P` / `p` | Previous plan |

**Tree Display:**
- Severity icons: `!!` (critical, red), `!` (warning, yellow), `*` (info, green)
- Category colors: Blue (Scan), Magenta (Join), Cyan (Aggregate), Yellow (Sort), Green (DML), Red (Streaming)
- Expand/collapse: `▾` expanded, `▸` collapsed, `·` leaf node

### Library

```rust
use ogexplain_core::{parse, analyze, analyze_with_config, heatmap, waterfall};
use ogexplain_core::analyzer::config::DiagnosticConfig;

// Parse EXPLAIN text
let plan = parse(&explain_text)?;

// Analyze with default config (25 rules)
let report = analyze(&plan);

// Analyze with custom config
let config = DiagnosticConfig {
    large_table_rows: 100000.0,
    disabled_rules: vec!["TYPE-001".to_string()],
    ..Default::default()
};
let report = analyze_with_config(&plan, &config);

// Analyze with SQL rewrite support
let report = analyze_with_rewrite(&plan, Some(&sql_text));

// Access findings
for finding in &report.findings {
    println!("[{}] {} - {}", finding.rule_id, finding.title, finding.detail);
    if let Some(suggestion) = &finding.suggestion {
        println!("  → {}", suggestion);
    }
}

// Generate cost deviation heatmap (requires EXPLAIN ANALYZE)
if let Some(hm) = heatmap(&plan) {
    println!("Max Q-Error: {:.1}", hm.summary.max_qerror);
}

// Generate resource waterfall (requires EXPLAIN ANALYZE)
if let Some(wf) = waterfall(&plan) {
    println!("CPU bottlenecks: {}", wf.bottlenecks.cpu_bottlenecks.len());
}

// Batch parse multi-block files
let plans = parse_multi(&mixed_input)?;
```

## MCP Server

The `ogexplain-mcp` binary exposes 5 tools via the Model Context Protocol (stdio transport) for AI assistants:

| Tool | Description |
|------|-------------|
| `analyze_explain` | Parse + analyze EXPLAIN plan → diagnostic findings (JSON + text summary) |
| `parse_explain` | Parse EXPLAIN text → structured plan tree (JSON) |
| `list_diagnostic_rules` | List all 25 diagnostic rules with IDs, categories, descriptions |
| `get_suggestions` | Cross-rule synthesis suggestions (work_mem, composite index, etc.) with confidence scores |
| `score_sql_complexity` | SQL complexity scoring — standard (0–100) + GaussDB 4-dimension |

**Configuration** (Claude Desktop / Cursor / VS Code):

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

**Integration with `gaussdb-mcp`:** Use `gaussdb-mcp` to run `EXPLAIN` on the database, then pipe the output to `ogexplain-mcp` for analysis — end-to-end SQL performance diagnostics.

**Build:**

```bash
cargo build -p ogexplain-mcp
cargo build -p ogexplain-cli --features mcp   # via unified CLI: ogexplain mcp
```

## Architecture

Rust Cargo workspace with five crates:

| Crate | Type | Purpose |
|-------|------|---------|
| [`ogexplain-core`](crates/ogexplain-core/) | Library | Parser + Model + Analyzer + Suggester + Rewriter (no UI deps) |
| [`ogexplain-cli`](crates/ogexplain-cli/) | Binary (`ogexplain`) | CLI frontend — file/pipe input, text/JSON/heatmap/waterfall/CSV output |
| [`ogexplain-tui`](crates/ogexplain-tui/) | Binary (`ogexplain-tui`) | Interactive TUI — collapsible plan tree, node detail, diagnostics, paste input |
| [`ogexplain-mcp`](crates/ogexplain-mcp/) | Binary (`ogexplain-mcp`) | MCP server — 5 tools for AI assistant integration via stdio |
| [`ogsql-complexity`](crates/ogsql-complexity/) | Library | SQL complexity scoring (standalone, reusable) |

### Core Layers

```
ogexplain-core
├── parser/          Two-phase: line classifier (regex) → tree builder (indent-based stack)
├── model/           ExplainPlan → PlanNode tree, NodeType (80+ variants), cost/stats/buffer types
├── analyzer/        Rule engine with DiagnosticRule trait + DFS traversal + configurable thresholds
│   ├── rules/       25 rules across 17 files with shared utility layer
│   ├── heatmap/     Cost-actual deviation heatmap with Q-error severity analysis
│   └── waterfall/   Resource waterfall — CPU/memory bottleneck identification
├── suggester/       Maps findings → suggestions with cross-rule synthesis (5 categories)
├── rewriter/        SQL rewrite for correlated-subquery self-update (SUBQ-006)
├── summary/         SummaryRow for batch reporting (SQL complexity + plan metrics + diagnostics)
├── sql/             SQL/EXPLAIN block segmentation from mixed input
└── i18n/            rust-i18n based localization (en, zh-CN)
```

## Diagnostic Rules

25 rules implemented across 17 rule files, with shared utility layer (`rules/utils.rs`) for common operations:

| ID | Rule | Category | Description |
|----|------|----------|-------------|
| SCAN-001 | Large table full scan | scan | Detects Seq Scan/PartitionedSeqScan/CStore Scan on tables exceeding row threshold; suggests `CREATE INDEX ON table(col)` |
| SCAN-004 | Filter without index | scan | Filter removing many rows without index support; extracts filter columns for suggestion |
| JOIN-001 | Nested loop on large tables | join | Nested loop join with high row counts; detects inner index presence, extracts join columns |
| JOIN-002 | Hash join spill to disk | join | Hash join exceeding work_mem; calculates recommended work_mem from disk+memory sizes |
| MEM-001 | Sort spill to disk | memory | External merge sort (incl. VectorSort); reports Sort Key in detail |
| MEM-004 | High peak memory | memory | Locates highest-memory node in subtree with node type and relation |
| SORT-003 | Duplicate sort | sort | Recursive subtree Sort Key collection; distinguishes duplicate vs different keys |
| NET-001 | Broadcast large data | network | Broadcasting excessive rows; supports SplitBroadcast/PartRedistributePartBroadcast |
| EST-001 | Severe row estimation error | estimation | Actual rows far exceed/fall below optimizer estimate; reports direction (under/over) |
| EST-004 | Nested loop from underestimation | estimation | Nested Loop caused by row underestimation; reports inner work quantity |
| PUSH-001 | Query not pushed down | pushdown | FQS failure with signal accumulation — identifies specific blockers (SubqueryScan, SubPlan, volatile functions) |
| PUSH-002 | Multi-layer streaming | pushdown | Collects streaming layer chain with `→` notation; layer-count-aware suggestions |
| TYPE-001 | Implicit type coercion | type_coercion | Struct-based `TypeMismatch` detection with specific fix suggestions |
| TYPE-004 | LIKE with leading wildcard | type_coercion | Distinguishes single/double wildcards; suggests `pg_trgm` + GIN index |
| VEC-001 | Mixed row/vector engines | vectorization | Tracks Row↔Vector adapter boundaries with parent→child type tracking |
| GEN-001 | Plan too deep | general | Reports depth with reason (subquery/nesting) |
| SUBQ-001 | Subquery not pulled up | subquery | Detects SubqueryScan nodes; extracts child table name for parameterized suggestions |
| REW-001 | Large IN list not rewritten | subquery | Detects IN lists with many values; extracts column name for `EXISTS` rewrite suggestion |
| SUBQ-006 | Correlated subquery self-update | subquery | Detects self-referencing correlated subqueries in UPDATE/DELETE; supports auto-SQL-rewrite |
| AGG-001 | Group aggregate should be hash | aggregate | Suggests Hash Aggregate for large GROUP BY without sort requirement |
| AGG-002 | Hash aggregate spill to disk | aggregate | Hash Aggregate exceeding work_mem, spilling to disk |
| SKEW-001 | Data skew detected | distribution | Uneven row distribution across datanodes |
| DIST-001 | Distribution column mismatch | distribution | Join columns don't match distribution columns causing redistribution |
| STATS-001 | Stats not collected | stats | Tables with missing or stale statistics |
| PART-001 | Partition pruning failure | partition | Full partition scan when pruning should reduce partitions |

## OpenGauss-Specific Support

This tool targets **OpenGauss/GaussDB** (PostgreSQL fork), not vanilla PostgreSQL. It handles OG-specific EXPLAIN features:

- **Vector nodes**: `Vector Hash Join`, `Vec Sort`, `Vector Sonic Hash Join/Aggregate`, etc.
- **CStore nodes**: `CStore Scan`, `CStore Index Scan` (columnar storage).
- **Streaming nodes**: `Streaming(type: GATHER|REDISTRIBUTE|BROADCAST|...)` with DOP and NodeGroup info.
- **Pushdown detection**: FQS (Fast Query Shipping) via Streaming node presence; `Data Node Scan` + `Remote query` = successful pushdown.
- **Implicit type coercion**: Detects via indirect patterns when `showimplicit=false`.
- **Row/Vector adapters**: `Row Adapter` / `Vector Adapter` engine boundary markers.
- **Pretty mode**: Node IDs with `--` prefix, detailed per-node runtime stats.
- **OG-specific properties**: Bloom Filter, Min/Max skip, DFS pruning, LLVM optimization, Skew optimization, Dynamic SMP, AI prediction (`p-time`, `p-rows`).
- **SQL rewrite**: Detects correlated-subquery self-update patterns and generates `UPDATE ... FROM` rewrites.

## SQL Complexity Scoring

The integrated `ogsql-complexity` crate provides:

- **Standard scoring** (0–100): Based on tables, joins, subqueries, set operations, CTEs, window functions.
- **GaussDB scoring**: Four-dimension model — SQL Structure, PL Logic, Advanced Features, Extensions.
- **SQL classification**: Categories (Query, DML, DDL, PL, Pkg) and sub-types.
- **Tag system**: Identifies specific complexity tags (e.g., `multi-table-join`, `correlated-subquery`, `window-function`).

## Testing

```bash
cargo test --workspace                   # All tests
cargo test -p ogexplain-core            # Core library tests
cargo test -p ogexplain-mcp             # MCP server integration tests
cargo test --test db_explain --features ogexplain-cli/db  # DB integration tests (requires Docker)
cargo insta review                       # Interactive snapshot review
cargo fmt --all && cargo clippy --workspace  # Lint (zero warnings)
```

Test fixtures are in `tests/fixtures/` (31 files) — each is a raw EXPLAIN TEXT output file covering specific scenarios (simple scans, joins, spills, streaming, vectorization, subqueries, aggregates, distribution, etc.).

## License

MIT
