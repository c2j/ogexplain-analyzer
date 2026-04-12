# ogexplain-analyzer

## What This Is

OpenGauss `EXPLAIN` / `EXPLAIN ANALYZE` output parser and performance diagnostics tool. Parses TEXT-format explain plans, runs diagnostic rules (OG-specific checks for pushdown, vectorization, streaming, implicit type coercion), and outputs findings + optimization suggestions.

**Status: Phases 1–3 complete, Phase 4 (polish) in progress.** Core parser, diagnostic engine, CLI, and TUI are all functional. 30 tests pass, zero clippy warnings.

## Project Structure

```
.sisyphus/plans/ogexplain-analyzer-spec.md   # Design spec (1823 lines)
.sisyphus/plans/ogexplain-analyzer-impl.md   # Implementation plan (phases, deps, TUI design)
GaussDB-2.23.07.210/                         # GaussDB product docs reference data
  sql_plan_hints.json                        # 53 SQL plan hints extracted from docs
  term/                                      # 457+ JSON term files from product documentation
lib/openGauss-server/                        # Git submodule (openGauss source, gitignored)
Cargo.toml                                   # Workspace root
crates/
  ogexplain-core/                            # Core library (model + parser + analyzer + suggester)
  ogexplain-cli/                             # CLI frontend
  ogexplain-tui/                             # Interactive TUI frontend
tests/
  fixtures/                                  # 20 EXPLAIN TEXT fixture files (01–20 + complex)
  integration_tests.rs                       # 10 parser insta snapshot tests
  analyzer_tests.rs                          # 20 analyzer tests
```

- `lib/openGauss-server` is a git submodule pointing to `https://gitee.com/opengauss/openGauss-server`. It is **gitignored** — it exists only as local reference for source code analysis, not as a build dependency.
- The `.gitignore` also ignores `/target` (Rust convention) and `/examples/gauss`.

## Architecture

Rust Cargo workspace with three crates sharing a core library:

| Crate | Binary | Purpose |
|-------|--------|---------|
| `ogexplain-core` | library | Parser + Model + Analyzer + Suggester (no UI deps) |
| `ogexplain-cli` | `ogexplain` | CLI frontend — file/pipe input, text/JSON output |
| `ogexplain-tui` | `ogexplain-tui` | Interactive TUI — collapsible plan tree, node detail, paste input |

### Core layers:

1. **Parser** (`parser/`): Two-phase — line classifier (regex per line) → tree builder (indent-based stack). Handles pretty mode (`N --` prefix), `using <index>` clause, unknown nodes, missing actual stats, `(Actual time: never executed)`.
2. **Model** (`model/`): `ExplainPlan` → `PlanNode` tree with `NodeType` enum (80+ variants including `Vector*`, `CStore*`, `Streaming`, `Partitioned*`), `EstimatedCost`, `ActualStats`, `BufferStats`, `NodeProperty`. All types `#[derive(Serialize)]`.
3. **Analyzer** (`analyzer/`): Rule engine via `DiagnosticRule` trait + `DiagnosticEngine` with DFS traversal. 15 rules implemented across 10 rule files. `DiagnosticConfig` with configurable thresholds and disabled rules support.
4. **Suggester** (`suggester/`): Maps diagnostic findings to actionable suggestions with cross-rule synthesis patterns (multi-spill → work_mem, multi-estimation → stale stats, scan+join → composite index).

### TUI (ratatui + Elm Architecture):

- Custom tree rendering with severity icons (🔴🟡🔵) and expand/collapse
- `ratatui-textarea` for multi-line EXPLAIN paste input
- TEA pattern: Event → Action → Model mutation → Render
- Panels: TreePanel | DetailPanel / InputPanel | StatusBar
- Input modes: `ogexplain-tui file.txt` (load file) or `ogexplain-tui` (paste mode, Ctrl+P to parse)
- Key bindings: Ctrl+P parse, Tab cycle focus, ↑↓ navigate, Enter expand/collapse, q quit

### Key files:

| File | What it is |
|------|-----------|
| `crates/ogexplain-core/src/lib.rs` | Public API: `parse()`, `analyze()`, `analyze_with_config()` |
| `crates/ogexplain-core/src/model/` | Data model — `plan.rs`, `node_type.rs`, `join_type.rs`, `streaming.rs`, `cost.rs`, `buffer.rs` |
| `crates/ogexplain-core/src/parser/` | `mod.rs`, `line_classifier.rs`, `tree_builder.rs` |
| `crates/ogexplain-core/src/analyzer/` | `mod.rs`, `config.rs`, `context.rs`, `report.rs`, `rules/*.rs` (10 files) |
| `crates/ogexplain-core/src/suggester/` | `mod.rs`, `suggestion.rs`, `mapper.rs` |
| `crates/ogexplain-cli/src/main.rs` | clap CLI with `analyze` subcommand |
| `crates/ogexplain-tui/src/` | `main.rs`, `app.rs` (TEA model), `action.rs`, `event.rs`, `components/` |
| `.sisyphus/plans/ogexplain-analyzer-spec.md` | Design spec (1823 lines) |
| `.sisyphus/plans/ogexplain-analyzer-impl.md` | Implementation plan |

### Implemented Diagnostic Rules (15 of 45+ planned)

| ID | Rule | Category |
|----|------|----------|
| SCAN-001 | Large table full scan | scan |
| SCAN-004 | Filter without index | scan |
| JOIN-001 | Nested loop on large tables | join |
| JOIN-002 | Hash join spill to disk | join |
| MEM-001 | Sort spill to disk | memory |
| MEM-004 | High peak memory | memory |
| SORT-003 | Duplicate sort | sort |
| NET-001 | Broadcast large data | network |
| EST-001 | Severe row underestimation | estimation |
| PUSH-001 | Query not pushed down | pushdown |
| PUSH-002 | Multi-layer streaming | pushdown |
| TYPE-001 | Implicit type coercion | type_coercion |
| TYPE-004 | LIKE with leading wildcard | type_coercion |
| VEC-001 | Mixed row/vector engines | vectorization |
| GEN-001 | Plan too deep | general |

## Key Domain Knowledge

This tool targets **OpenGauss** (PostgreSQL-fork), not vanilla PostgreSQL. OG-specific EXPLAIN features that the parser handles:

- **Vector nodes**: `Vector Hash Join`, `Vec Sort`, `Vector Sonic Hash Join/Aggregate`, etc.
- **CStore nodes**: `CStore Scan`, `CStore Index Scan`, columnar storage scans.
- **Streaming nodes**: `Streaming(type: GATHER|REDISTRIBUTE|BROADCAST|...)` with DOP (`dop: c/p`) and NodeGroup (`ng: g1->g2`) info.
- **Pushdown**: FQS (Fast Query Shipping) detection via absence/presence of Streaming nodes; `Data Node Scan` + `Remote query` = successful pushdown.
- **Implicit type coercion**: OG hides implicit casts in EXPLAIN (`showimplicit=false`); must detect via indirect patterns (Seq Scan + Filter + high Rows Removed).
- **Adapters**: `Row Adapter` / `Vector Adapter` mark row↔vector engine boundaries.
- **Pretty mode**: Node IDs with `--` prefix, detailed per-node runtime stats.
- **OG-specific properties**: Bloom Filter info, Min/Max skip, DFS file pruning, LLVM optimization markers, Skew optimization markers, CPU details, Dynamic SMP, AI prediction (`p-time`, `p-rows`).

## Dependencies

- **core**: `regex`, `serde` + `serde_json`, `thiserror`, `toml`; dev: `insta` (YAML snapshots)
- **cli**: `ogexplain-core`, `clap` v4, `colored`, `anyhow`
- **tui**: `ogexplain-core`, `ratatui` 0.30, `crossterm` 0.29, `ratatui-textarea` 0.8, `tokio`, `color-eyre`, `clap` v4

## Build & Test

```bash
cargo build                              # full workspace
cargo build -p ogexplain-core            # core library only
cargo test --workspace                   # all 30 tests (10 parser + 20 analyzer)
cargo test --test integration_tests      # parser insta snapshot tests only
cargo test --test analyzer_tests         # analyzer diagnostic tests only
cargo insta review                       # interactive snapshot review
cargo run -p ogexplain-cli -- analyze file.txt -o json   # CLI (text or json output)
cargo run -p ogexplain-tui -- file.txt   # TUI with file
cargo run -p ogexplain-tui               # TUI paste mode
cargo fmt --all && cargo clippy --workspace  # lint (zero warnings)
```

Test fixtures go in `tests/fixtures/` — each is a raw EXPLAIN TEXT output file. Each fixture has a corresponding `insta::assert_yaml_snapshot!` test.

## Constraints

- `ogexplain-core` is a pure library — no UI/IO crate dependencies (no ratatui, no crossterm, no clap).
- All model types derive `Serialize`.
- Parser never panics — returns `Result<_, ParseError>`.
- Diagnostic rules are independently testable via `DiagnosticRule::check()`.
- TUI state and rendering are strictly separated (TEA architecture).

## OpenGauss Source Reference

The spec references specific source files in `lib/openGauss-server/` for parsing behavior:
- `src/gausskernel/optimizer/commands/explain.cpp` — EXPLAIN output generation
- `src/gausskernel/optimizer/util/optcommon.cpp` — plan node naming
- `src/gausskernel/optimizer/plan/pgxcship.cpp` — pushdown/shippability logic
- `src/gausskernel/runtime/executor/indxpath.cpp` — index path selection (implicit cast detection)
- `src/gausskernel/optimizer/commands/plananalyzer.cpp` — unpushable query analysis

These paths are relative to `lib/openGauss-server/`.

## Remaining Work (Optional / Incremental)

- Remaining 30+ diagnostic rules (from 15 to 45+) — see spec for full list
- Markdown output for CLI (`-o markdown`)
- TOML config file loading (`--config file.toml`)
- `suggester/synthesizer.rs` full implementation
