# ogexplain-analyzer

## What This Is

OpenGauss `EXPLAIN` / `EXPLAIN ANALYZE` output parser and performance diagnostics tool. Parses TEXT-format explain plans, runs 45+ diagnostic rules (including OG-specific checks for pushdown, vectorization, streaming, implicit type coercion), and outputs findings + optimization suggestions.

**Status: Pre-implementation.** No source code exists yet — the repo contains only the design spec and reference data.

## Project Structure

```
.sisyphus/plans/ogexplain-analyzer-spec.md   # THE spec — read this first (1823 lines)
GaussDB-2.23.07.210/                         # GaussDB product docs reference data
  sql_plan_hints.json                        # 53 SQL plan hints extracted from docs
  term/                                      # 457+ JSON term files from product documentation
lib/openGauss-server/                        # Git submodule (openGauss source, gitignored)
```

- `lib/openGauss-server` is a git submodule pointing to `https://gitee.com/opengauss/openGauss-server`. It is **gitignored** (`/lib/openGauss-server` in `.gitignore`) — it exists only as local reference for source code analysis, not as build dependency.
- The `.gitignore` also ignores `/target` (Rust convention) and `/examples/gauss`.

## Planned Architecture

Rust, single binary. Four layers:

1. **Parser** (`src/parser/`): Two-phase — line classifier (regex per line) → tree builder (indent-based stack). Handles TEXT format; JSON/XML/YAML deferred to Phase 3.
2. **Model** (`src/model/`): `ExplainPlan` → `PlanNode` tree with `NodeType` enum (80+ variants including `Vector*`, `CStore*`, `Streaming`, `Partitioned*`), `EstimatedCost`, `ActualStats`, `BufferStats`.
3. **Analyzer** (`src/analyzer/`): Rule engine via `DiagnosticRule` trait. 45+ rules across 13 categories (scan, join, memory, sort, network, estimation, pushdown, type coercion, vectorization, subquery, distribution, storage, general). Configurable thresholds via TOML config.
4. **Reporter** (`src/reporter/`): Text (colored terminal), JSON, Markdown, HTML output.

Plus a **Suggestion Engine** (`src/suggester/`) that maps diagnostic findings to actionable suggestions with 6 cross-rule synthesis patterns.

## Key Domain Knowledge

This tool targets **OpenGauss** (PostgreSQL-fork), not vanilla PostgreSQL. OG-specific EXPLAIN features that the parser must handle:

- **Vector nodes**: `Vector Hash Join`, `Vec Sort`, `Vector Sonic Hash Join/Aggregate`, etc.
- **CStore nodes**: `CStore Scan`, `CStore Index Scan`, columnar storage scans.
- **Streaming nodes**: `Streaming(type: GATHER|REDISTRIBUTE|BROADCAST|...)` with DOP (`dop: c/p`) and NodeGroup (`ng: g1->g2`) info.
- **Pushdown**: FQS (Fast Query Shipping) detection via absence/presence of Streaming nodes; `Data Node Scan` + `Remote query` = successful pushdown.
- **Implicit type coercion**: OG hides implicit casts in EXPLAIN (`showimplicit=false`); must detect via indirect patterns (Seq Scan + Filter + high Rows Removed).
- **Adapters**: `Row Adapter` / `Vector Adapter` mark row↔vector engine boundaries.
- **Pretty mode**: Node IDs with `--` prefix, detailed per-node runtime stats.
- **OG-specific properties**: Bloom Filter info, Min/Max skip, DFS file pruning, LLVM optimization markers, Skew optimization markers, CPU details, Dynamic SMP, AI prediction (`p-time`, `p-rows`).

## Spec Location

The full design spec is at `.sisyphus/plans/ogexplain-analyzer-spec.md`. It contains:
- EXPLAIN TEXT format specification (reverse-engineered from openGauss source)
- Complete node type catalog (80+ types)
- All 45+ diagnostic rules with trigger conditions
- Regex patterns for parsing
- Data model definitions (Rust structs)
- CLI interface design (`ogexplain-analyzer [OPTIONS] [FILE]`)
- Planned project directory layout
- Configuration file format (TOML)
- Test strategy and fixture list
- Implementation roadmap (3 phases)

## Planned Dependencies

Rust: `clap` v4 (CLI), `regex`, `serde` + `serde_json`, `colored`/`console`, `toml`, `anyhow` + `thiserror`, `insta` (snapshot testing).

## Build & Test (Not Yet Implemented)

When implemented:
```bash
cargo build              # build
cargo test               # run tests
cargo test --test parser_tests    # parser integration tests
cargo test --test analyzer_tests  # analyzer integration tests
cargo test -- --test-threads=1    # serial test run (if needed)
```

Test fixtures go in `tests/fixtures/` — each is a raw EXPLAIN TEXT output file.

## OpenGauss Source Reference

The spec references specific source files in `lib/openGauss-server/` for parsing behavior:
- `src/gausskernel/optimizer/commands/explain.cpp` — EXPLAIN output generation
- `src/gausskernel/optimizer/util/optcommon.cpp` — plan node naming
- `src/gausskernel/optimizer/plan/pgxcship.cpp` — pushdown/shippability logic
- `src/gausskernel/runtime/executor/indxpath.cpp` — index path selection (implicit cast detection)
- `src/gausskernel/optimizer/commands/plananalyzer.cpp` — unpushable query analysis

These paths are relative to `lib/openGauss-server/`.
