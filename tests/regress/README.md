# tests/regress/

> Per-rule regression suite for `ogexplain-analyzer` diagnostic rules, with optional live-database verification via the `ogagila` submodule.

This directory is **the canonical place for rule-level regression tests**. Each rule has its own subdirectory; each case is a self-contained contract declaring *what should fire, with what severity, mentioning what entities* — authored by hand, never auto-generated.

## Why this exists

| Existing layer | Gap this fills |
|----------------|----------------|
| `tests/analyzer_tests.rs` (workspace root) | Loose substring assertions (`detail.contains("16472")`); coverage gaps (8/25 documented rules untested); no Finding-level golden file. |
| `crates/ogexplain-core/tests/analyzer_tests.rs` | Strict subset of the above; duplication smell. |
| `lib/ogagila/benchmark/v3/cases/OGEXP-GT-*.json` | Per-query evaluation view (P/R/F1), not per-rule regression; `_auto_eval` is heuristic, not hand-verified. |

`tests/regress/` is the layer where:
1. **Every rule has explicit positive + negative cases** — gaps become visually obvious from the directory listing.
2. **Findings are golden-filed** — catches `detail` wording regressions, suggestion-template drift, and severity mismatches that substring tests miss.
3. **Live database verification is available** — when `--features live-db` is enabled, cases are replayed against a real OpenGauss instance initialized from `ogagila`, so EXPLAIN outputs are guaranteed ground truth rather than hand-fabricated text.

## Quickstart

```bash
# Static mode: uses ogagila's pre-recorded EXPLAIN material.
# No Docker required. Millisecond-fast.
cargo test --test regress

# Live-DB smoke test: spawns OpenGauss via testcontainers, inits ogagila.
cargo test --test regress_live --features live-db -- --nocapture

# Run only one rule's cases:
cargo test --test regress -- scan_
```

## Layout

```
tests/regress/
├── README.md                          ← this file
├── CONTRIBUTING.md                    ← authoring guide + case.toml + expected.findings.json schemas
├── harness/                           ← (planned) Rust driver
├── scan/
│   ├── scan-001-large-table-full-scan/   ← populated (pilot)
│   │   ├── case.toml                  ← declares data source + side effects + verification mode
│   │   ├── expected.findings.json     ← hand-authored contract
│   │   └── README.md                  ← optional per-case design notes
│   └── scan-004-filter-without-index/...(planned)
├── join/...                           ← (planned)
├── mem/...                            ← (planned)
├── dist/                              ← (planned) distributed rules; cases marked live_db_verify=false
│   └── README.md                      ← explains single-node physical limitation
└── stats/                             ← (planned) rules with OG-unsupported statements; flagged
```

## Status

**11 / 25 rules covered (44%).** The driver (`tests/regress.rs` + `build.rs`) is operational: cases are auto-discovered, per-case `#[test]` functions are generated at build time, and `cargo test --test regress` runs all 11 static cases. Live-DB smoke test (`tests/regress_live.rs`) validates the ogagila container pipeline.

Covered: SCAN-001, SCAN-004, JOIN-001, MEM-004, SORT-003, TYPE-001, TYPE-004, GEN-001, SUBQ-001, SUBQ-006, PART-001.

Planned: remaining 14 rules in subsequent batches; `--features live-db` per-case integration (Phase 3b).

## Design decisions locked

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Path B with ogagila substrate | Reuse 95% of ogagila's DDL/data/SQL/EXPLAIN; per-rule supplemental only when ogagila is insufficient. |
| 2 | `--features live-db` defaults off | `cargo test` stays fast & CI-friendly; live verification opt-in. |
| 3 | Distributed rules (DIST/SKEW/NET) marked `live_db_verify=false` | ogagila Docker is single-node centralized; physical signal for redistribution is weak. |
| 4 | Rust rewrites the ogagila loader | No Python dependency at test time. |
| 5 | Follow ogagila `main` (no commit pin) | Each expected file records the commit it was authored against; CI warns (not fails) on drift. |
| 6 | `expected.findings.json` hand-authored | Avoids the "locking wrong behavior" trap (cf. ogagila v1→v2 fixed 27 auto-derived GT errors). |

## Relationship to other test layers

| Layer | When it runs | What it validates |
|-------|--------------|-------------------|
| `crates/ogexplain-core/src/analyzer/rules/*_rules.rs #[cfg(test)]` | Every `cargo test` | In-crate rule isolation on synthesized `PlanNode`s. Fast, granular, but synthetic. |
| `tests/integration_tests.rs` | Every `cargo test` | End-to-end pipeline (parse → analyze → report) on hand-written EXPLAIN text fixtures. |
| `tests/analyzer_tests.rs` | Every `cargo test` | Per-rule behavioral assertions on `tests/fixtures/*.txt`. **Will be migrated here incrementally.** |
| `tests/db_explain.rs` (`--features db`) | Opt-in | Smoke-tests `ogexplain-cli::db::fetch_explain` against a fresh OG container. |
| **`tests/regress/` (this dir)** | Every `cargo test` (11 cases) + `--features live-db` smoke test | **Per-rule hand-authored contracts** validated against either pre-recorded EXPLAIN material or live DB replay. |
| `tests/regress_live.rs` (`--features live-db`) | Opt-in | **Phase 3a**: Container lifecycle + ogagila schema loading + smoke test. Validates real OG→EXPLAIN→parse→analyze pipeline. |

## Live-DB mode (`--features live-db`)

A companion test file `tests/regress_live.rs` provides real-database verification. It:

1. **Starts** an OpenGauss container via `testcontainers` (`opengauss/opengauss:latest`, ~15s)
2. **Creates** the `pagila` database (`CREATE DATABASE pagila`)
3. **Loads** ogagila's schema + data using a **two-phase strategy**:
   - **DDL + program files** (schema, functions, triggers, views, ~40KB): loaded via the `gaussdb` Rust client's `batch_execute()` (fast, synchronous)
   - **Large data files** (COPY-format seed data + JSONB, ~100MB total): loaded via `docker exec -i gsql` to bypass the Rust client's COPY FROM STDIN limitation
4. **Runs** live EXPLAIN queries against the real database
5. **Accepts predictable drift** between live and static EXPLAIN output (row counts, plan costs) because ogagila's seed data evolves on `main`

The smoke test validates the full pipeline end-to-end — container → load → query → parse:

```bash
cargo test --test regress_live --features live-db -- --nocapture
```

Output shows each phase, the actual EXPLAIN text (for manual verification against ogagila's pre-recorded `Q*.explain` files), and whether the plan contains expected node types.

> **Phase 3b** (planned) will integrate live verification into `tests/regress.rs`'s per-case execution: cases with `live_db_verify = true` will be replayed against the live DB and findings compared to the same `expected.findings.json`.
