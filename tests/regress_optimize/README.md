# tests/regress_optimize/

> Per-rule regression suite for `ogexplain-optimizer` closed-loop optimization pipeline.

Each case validates the full pipeline: original SQL → EXPLAIN → diagnose → map →  
rewrite → verify → converge — against mock EXPLAIN data, no live DB required.

## Quickstart

```bash
cargo test --test optimize_regress
cargo test --test optimize_regress -- subq_001
```

## Layout

```
tests/regress_optimize/
├── README.md
├── subq/
│   ├── subq-001-exists-to-join/
│   │   ├── case.toml              # metadata
│   │   ├── original.sql           # input SQL
│   │   ├── explain_before.txt     # EXPLAIN output before rewrite
│   │   ├── explain_after.txt      # EXPLAIN output after rewrite
│   │   └── expected.json          # expected loop outcome contract
│   └── subq-006-...
├── type/
│   └── type-001-implicit-cast/
│       └── ...
└── agg/
    └── agg-001-hash-aggregate/
        └── ...
```

## `case.toml` schema

```toml
rule_id    = "SUBQ-001"
case_name  = "subq-001-exists-to-join"
description = "EXISTS subquery should be rewritten to DISTINCT JOIN"

[config]
max_iterations = 5
skip_verify    = true
```

## `expected.json` schema

```json
{
  "expect_rewrite": true,
  "expect_stop_reason": "Success",
  "expect_iterations": 1,
  "expect_rule_triggered": "SUBQ-001",
  "rewritten_sql_must_contain": ["JOIN", "DISTINCT"],
  "rewritten_sql_must_not_contain": ["EXISTS"],
  "expect_critical_after_less_than": 1
}
```

| Field | Meaning |
|-------|---------|
| `expect_rewrite` | Whether the loop should produce a rewritten SQL |
| `expect_stop_reason` | Expected `StopReason` (Success, FixedPoint, MaxIterations, NoRewritableFindings, etc.) |
| `expect_iterations` | Exact number of loop iterations expected |
| `expect_rule_triggered` | Which diagnostic rule fired the rewrite |
| `rewritten_sql_must_contain` | Substrings that MUST appear in the final SQL |
| `rewritten_sql_must_not_contain` | Substrings that MUST NOT appear in the final SQL |
| `expect_critical_after_less_than` | Final critical finding count must be < N |

## Design decisions

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Mock EXPLAIN data, no live DB | Tests run in milliseconds, no Docker/OpenGauss required |
| 2 | `verify_engine` defaults to skip | QED requires Z3 solver; skirt that for fast regression |
| 3 | Per-case SQL + EXPLAIN pairs | Each case is fully self-contained |
| 4 | `expected.json` not auto-generated | Hand-authored contracts prevent locking in wrong behavior |
