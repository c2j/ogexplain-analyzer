# ogexplain-analyzer

[English](README.md) | [中文](README.zh-CN.md)

OpenGauss/GaussDB `EXPLAIN` / `EXPLAIN ANALYZE` output parser and performance diagnostics tool. Parses TEXT-format execution plans, runs 15+ diagnostic rules (OpenGauss-specific checks for pushdown, vectorization, streaming, implicit type coercion, and more), and outputs actionable findings with optimization suggestions.

## Features

- **Full EXPLAIN TEXT parsing** — Handles `EXPLAIN` and `EXPLAIN ANALYZE` output including pretty mode (`N --` prefix), vector nodes, CStore scans, streaming operators, and OG-specific properties.
- **15+ diagnostic rules** — Covers scan, join, memory, sort, network, estimation, pushdown, type coercion, vectorization, and general plan health.
- **Optimization suggestions** — Cross-rule synthesis maps diagnostic findings to actionable suggestions (e.g., multi-spill → increase `work_mem`, multi-estimation → run `ANALYZE`).
- **SQL complexity scoring** — Integrated `ogsql-complexity` crate scores SQL statements on a 0–100 scale with GaussDB-specific dimensions (SQL structure, PL logic, advanced features, extensions).
- **i18n support** — English and Chinese (`zh-CN`) output via `--lang` flag or auto-detection from system locale.
- **Multiple interfaces** — CLI for scripting, TUI for interactive exploration, library crate for embedding.
- **DB-connected EXPLAIN** — `explain` subcommand connects directly to OpenGauss/GaussDB, runs `EXPLAIN [ANALYZE]`, and analyzes the result in one step.
- **Batch processing** — Parse multi-statement files with interleaved SQL and EXPLAIN blocks; export summary to CSV.

## Quick Start

```bash
# Build
cargo build --workspace

# Analyze an EXPLAIN output file (text report)
cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt

# JSON output
cargo run -p ogexplain-cli -- analyze tests/fixtures/03_hash_join.txt -o json

# Read from stdin
cat tests/fixtures/01_simple_seq_scan.txt | cargo run -p ogexplain-cli -- analyze -

# Launch TUI with a file
cargo run -p ogexplain-tui -- tests/fixtures/10_complex_plan.txt

# Launch TUI in paste mode (Ctrl+P to parse)
cargo run -p ogexplain-tui
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
```

## Usage

### CLI

```bash
ogexplain analyze <file> [options]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-o, --output` | `text` | Output format: `text` or `json` |
| `--threshold` | `info` | Minimum severity: `critical`, `warning`, `info` |
| `-q, --quiet` | — | Show findings only, no plan tree |
| `-v, --verbose` | — | Verbose output |
| `--multi` | — | Enable multi-block parsing |
| `--csv <path>` | — | Export summary table to CSV (use `-` for stdout) |
| `--lang` | `auto` | Language: `en`, `zh-CN`, or `auto` (system locale) |

#### DB-connected EXPLAIN (requires `db` feature)

```bash
# Build with database support
cargo build -p ogexplain-cli --features db

# Run EXPLAIN on a remote database
ogexplain explain -d "host=... port=5432 dbname=mydb user=gaussdb password=... sslmode=disable" -s "SELECT * FROM orders WHERE status = 'pending'"

# Run EXPLAIN ANALYZE (actually executes the query)
ogexplain explain -d "host=..." -s "SELECT ..." --analyze

# SQL from file
ogexplain explain -d "host=..." -f query.sql
```

### TUI

```bash
ogexplain-tui [file]
```

| Key | Action |
|-----|--------|
| `Ctrl+P` | Parse pasted EXPLAIN text |
| `Tab` | Cycle focus between panels |
| `↑` `↓` | Navigate plan tree |
| `Enter` | Expand / collapse node |
| `q` | Quit |

### Library

```rust
use ogexplain_core::{parse, analyze, analyze_with_config};
use ogexplain_core::analyzer::config::DiagnosticConfig;

// Parse EXPLAIN text
let plan = parse(&explain_text)?;

// Analyze with default config
let report = analyze(&plan);

// Analyze with custom config
let config = DiagnosticConfig::default();
let report = analyze_with_config(&plan, &config);

// Access findings
for finding in &report.findings {
    println!("[{}] {} - {}", finding.rule_id, finding.title, finding.detail);
}
```

## Architecture

Rust Cargo workspace with four crates:

| Crate | Type | Purpose |
|-------|------|---------|
| [`ogexplain-core`](crates/ogexplain-core/) | Library | Parser + Model + Analyzer + Suggester (no UI deps) |
| [`ogexplain-cli`](crates/ogexplain-cli/) | Binary (`ogexplain`) | CLI frontend — file/pipe input, text/JSON/CSV output |
| [`ogexplain-tui`](crates/ogexplain-tui/) | Binary (`ogexplain-tui`) | Interactive TUI — collapsible plan tree, node detail, paste input |
| [`ogsql-complexity`](crates/ogsql-complexity/) | Library | SQL complexity scoring (standalone, reusable) |

### Core Layers

```
ogexplain-core
├── parser/          Two-phase: line classifier (regex) → tree builder (indent-based stack)
├── model/           ExplainPlan → PlanNode tree, NodeType (80+ variants), cost/stats/buffer types
├── analyzer/        Rule engine with DiagnosticRule trait + DFS traversal + configurable thresholds
├── suggester/       Maps findings → suggestions with cross-rule synthesis
├── summary/         SummaryRow for batch reporting (SQL complexity + plan metrics + diagnostics)
├── sql/             SQL/EXPLAIN block segmentation from mixed input
└── i18n/            rust-i18n based localization (en, zh-CN)
```

## Diagnostic Rules

15 rules implemented across 10 rule files:

| ID | Rule | Category | Description |
|----|------|----------|-------------|
| SCAN-001 | Large table full scan | scan | Detects Seq Scan on tables exceeding row threshold |
| SCAN-004 | Filter without index | scan | Filter removing many rows without index support |
| JOIN-001 | Nested loop on large tables | join | Nested loop join with high row counts on both sides |
| JOIN-002 | Hash join spill to disk | join | Hash join exceeding work_mem, spilling to disk |
| MEM-001 | Sort spill to disk | memory | External merge sort due to insufficient work_mem |
| MEM-004 | High peak memory | memory | Plan node exceeding memory threshold |
| SORT-003 | Duplicate sort | sort | Multiple sort operations that could be eliminated |
| NET-001 | Broadcast large data | network | Broadcasting excessive rows across datanodes |
| EST-001 | Severe row underestimation | estimation | Actual rows far exceed optimizer estimate |
| PUSH-001 | Query not pushed down | pushdown | FQS failure — query executed with streaming overhead |
| PUSH-002 | Multi-layer streaming | pushdown | Multiple Streaming layers indicating poor pushdown |
| TYPE-001 | Implicit type coercion | type_coercion | Implicit cast degrading index usage |
| TYPE-004 | LIKE with leading wildcard | type_coercion | `LIKE '%...'` pattern preventing index usage |
| VEC-001 | Mixed row/vector engines | vectorization | Row↔Vector adapter overhead |
| GEN-001 | Plan too deep | general | Excessive plan depth suggesting optimization opportunity |

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
cargo test --test db_explain --features ogexplain-cli/db  # DB integration tests (requires Docker)
cargo insta review                       # Interactive snapshot review
cargo fmt --all && cargo clippy --workspace  # Lint (zero warnings)
```

Test fixtures are in `tests/fixtures/` — each is a raw EXPLAIN TEXT output file covering specific scenarios (simple scans, joins, spills, streaming, vectorization, etc.).

## License

MIT
