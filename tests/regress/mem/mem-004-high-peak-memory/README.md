# MEM-004 — high peak memory on Q30 (config_overrides case)

## What it tests

`MEM-004` (`HighPeakMemory`) is a **global** rule (`check_global`, not per-node `check`). It fires when the peak memory across the entire plan tree exceeds `DiagnosticConfig.memory_threshold_kb`. Walking the rule (`crates/ogexplain-core/src/analyzer/rules/memory_rules.rs:76`):

1. Read `plan.summary.peak_memory_kb` (from a top-level `Peak Memory:` summary line). If the summary is absent or has no peak, fall back to `find_peak_memory_in_tree` — a DFS that returns the max `structured_props.peak_memory_kb` across all nodes.
2. Short-circuit: if peak ≤ 0 or peak ≤ threshold → no finding.
3. Attempt to identify the highest-memory node via `find_highest_memory_node` (looks for a property with label `Memory Usage`). If found, append a `, highest memory node: ...` suffix to the detail.
4. Emit a single `Severity::Warning` / `DiagnosticCategory::MemoryUsage` finding.

This case locks in the **fallback tree-scan path** (no `Peak Memory:` summary line — Q30 ends with `Total runtime`, not `Peak Memory`) combined with an **artificially lowered threshold** via `[config]`.

## Why `[config]` overrides are needed

`MEM-004` ships with a default `memory_threshold_kb = 102400.0` (100 MB) — a value calibrated for production-scale workloads where a Hash/Sort node consuming >100 MB is a genuine concern.

ogagila's seed data is the Sakila/Pagila sample database:

| Table | Rows |
|-------|------|
| `payment` (7 partitions) | ~16,049 |
| `rental` | ~16,044 |
| `inventory` | 4,581 |
| `film` | 1,000 |

The largest Hash build across all 97 ogagila benchmark queries is **Q30's Hash on `rental`: 883 kB** — roughly **0.86 MB**, two orders of magnitude below the 100 MB default. A prior analysis of Q28/Q29/Q30/Q148 confirmed none trigger MEM-004 at the default threshold.

This case is therefore explicitly an **"ogagila data + artificially low threshold"** scenario, not a natural ogagila trigger. We lower `memory_threshold_kb` to **500 kB** so that Q30's 883 kB peak exceeds the bar and exercises the rule's comparison, detail rendering, and suggestion emission. This locks in behavioral coverage for MEM-004 without waiting for a production-scale fixture.

## Input source

ogagila benchmark v3, **Q30**:

```sql
SELECT f.title, COUNT(r.rental_id)
FROM film f
JOIN inventory i ON f.film_id = i.film_id
JOIN rental r ON i.inventory_id = r.inventory_id
JOIN payment p ON r.rental_id = p.rental_id
GROUP BY f.title
LIMIT 30;
```

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q30.explain`)

```
Limit  (cost=1602.28..1602.58 rows=30 width=27) (actual time=56.357..56.368 rows=30 loops=1)
  ->  HashAggregate  (cost=1602.28..1612.28 rows=1000 width=27) (actual time=56.353..56.360 rows=30 loops=1)
        Group By Key: f.title
        ->  Hash Join  (cost=798.18..1522.02 rows=16053 width=19) (actual time=15.021..46.879 rows=16049 loops=1)
              Hash Cond: (r.inventory_id = i.inventory_id)
              ->  Hash Join  (cost=524.62..1027.78 rows=16049 width=8) (actual time=8.689..28.137 rows=16049 loops=1)
                    Hash Cond: (p.rental_id = r.rental_id)
                    ->  Partition Iterator  ...
                    ->  Hash  (cost=318.72..318.72 rows=16472 width=8) (actual time=8.501..8.501 rows=16044 loops=1)
                           Buckets: 32768  Batches: 1  Memory Usage: 883kB
                          ->  Seq Scan on rental r  ...
              ->  Hash  (cost=216.30..216.30 rows=4581 width=19) (actual time=6.133..6.133 rows=4581 loops=1)
                     Buckets: 32768  Batches: 1  Memory Usage: 492kB
                    ->  Hash Join  ...
                          ->  Seq Scan on inventory i  ...
                          ->  Hash  (cost=65.00..65.00 rows=1000 width=19) (actual time=1.242..1.242 rows=1000 loops=1)
                                 Buckets: 32768  Batches: 1  Memory Usage: 308kB
                                ->  Seq Scan on film f  ...
Total runtime: 56.943 ms
```

Three Hash nodes, three `Memory Usage` values: **883 kB** (rental), 492 kB (inventory/film subjoin), 308 kB (film).

## Why MEM-004 fires (with threshold = 500)

| Check | Result |
|-------|--------|
| `plan.summary` present? | yes, but `summary.peak_memory_kb` = None (Q30 has `Total runtime`, no top-level `Peak Memory:` line) |
| Fallback: `find_peak_memory_in_tree` | walks all nodes; Hash-on-rental has `structured_props.peak_memory_kb = 883.0` → returns **883.0** |
| `peak > 0` | 883.0 > 0 ✓ |
| `peak > threshold` | 883.0 > 500.0 ✓ → fires |

Rule severity: `Severity::Warning`. Rule category: `DiagnosticCategory::MemoryUsage`.

### How `structured_props.peak_memory_kb` is derived

The line `Buckets: 32768  Batches: 1  Memory Usage: 883kB` is parsed by `try_parse_property` (first-colon rule) as a single property: `label = "Buckets"`, `value = "32768  Batches: 1  Memory Usage: 883kB"`. `NodeProperties::extract` (`crates/ogexplain-core/src/model/plan.rs:54`) matches the `"Buckets"` label and splits the value on double-space, extracting `hash_memory_usage = "883kB"`. The fallback at line 86-90 derives `peak_memory_kb = 883.0` from `hash_memory_usage`.

### Why `detail` has no "highest memory node" suffix

`find_highest_memory_node` calls `get_property_value(node, "Memory Usage")` which searches for a property with `label == "Memory Usage"`. Since the property label is `"Buckets"` (the memory info is embedded in its value, not a standalone property), this search returns `None` for every node. `top_node` is `None`, so the `detail_top_node` template is never appended. The detail is the base template only.

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.MEM-004.detail:
  en: "Peak memory: %{peak}kB (threshold: %{threshold}kB)"
finding.MEM-004.detail_top_node:
  en: ", highest memory node: %{node_type} on %{relation} (%{mem_kb}kB)"
finding.MEM-004.suggestion:
  en: "Analyze high memory node; Sort/Hash → increase work_mem; Materialize → optimize query to reduce intermediate result sets"
```

Substituting `%{peak}=883`, `%{threshold}=500` (f64 values render without trailing `.0` for whole numbers, consistent with existing fixture-12 test assertions on "512000"):

- **detail** = `"Peak memory: 883kB (threshold: 500kB)"` (no suffix — see above)
- **suggestion** = `"Analyze high memory node; Sort/Hash → increase work_mem; Materialize → optimize query to reduce intermediate result sets"`

These are the source of truth for `detail_must_contain` (`Peak memory`, `883`, `500`) and `suggestion_must_contain` (`increase work_mem`) in [`expected.findings.json`](expected.findings.json).

## Co-firing rules (declared neither as `must_fire` nor as `must_not_fire`)

The harness permits undeclared co-firings. This case targets the MEM-004 contract only.

## Live-DB notes

- `live_db_verify = false`: the default `DiagnosticConfig` threshold (100 MB) is physically unreachable on ogagila's 16K-row schema. A live-db replay with default config would **not** reproduce MEM-004 — the rule would silently not fire. Live-db verification would require either a custom GUC to inflate memory usage, a larger dataset, or the same `[config]` override applied to the live engine (which the planned live-db harness does not yet support).
- `weak_signal = false`: MEM-004 is not a distributed-only rule; the signal (peak memory) is meaningful on single-node centralized openGauss. The skip is purely a threshold-reachability issue, not a physical limitation.
- ogagila `main` commit at authoring: `d960d8c` (see `expected.findings.json` → `_meta.ogagila_commit`).
