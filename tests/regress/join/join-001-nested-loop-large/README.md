# JOIN-001 — Nested Loop with large dataset (forced via `/*+ nestloop */` hint)

## What it tests

`JOIN-001` (`NestedLoopLargeDataset`) fires on:

- `NodeType::NestedLoop`
- After iterating all children, `max(actual.rows × actual.loops)` across children exceeds `nested_loop_inner_rows` threshold (default 10000)

## Input source

ogagila benchmark v3, **Q17**:

```sql
SELECT /*+ nestloop(r p) */ r.rental_id, p.amount
FROM rental r
INNER JOIN payment p ON r.rental_id = p.rental_id
WHERE r.customer_id BETWEEN 1 AND 10;
```

The `/*+ nestloop(r p) */` hint forces the optimizer to choose Nested Loop instead of the Hash Join it would otherwise pick (cf. Q15/Q19 which use Hash/Merge Join for similar queries without the hint).

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q17.explain`)

```
Nested Loop  (cost=0.00..5656.46 rows=267 width=10) (actual time=0.521..76.111 rows=278 loops=1)
  ->  Partition Iterator  (cost=0.00..282.49 rows=16049 width=10) (actual time=0.040..5.277 rows=16049 loops=1)
        Iterations: 7
        ->  Partitioned Seq Scan on payment p  (cost=0.00..282.49 rows=16049 width=10) (actual time=0.075..2.920 rows=16049 loops=7)
              Selected Partitions:  1..7
  ->  Index Scan using rental_pkey on rental r  (cost=0.00..0.32 rows=1 width=4) (actual time=62.955..63.089 rows=278 loops=16049)
        Index Cond: (rental_id = p.rental_id)
        Filter: ((customer_id >= 1) AND (customer_id <= 10))
        Rows Removed by Filter: 15771
Total runtime: 77.211 ms
```

## Why JOIN-001 fires (with Critical severity)

The rule iterates NL children and computes `actual.rows × actual.loops` for each:

| Child | rows × loops = work |
|-------|---------------------|
| Partition Iterator (payment) | 16049 × 1 = 16,049 |
| **Index Scan on rental r** | **278 × 16049 = 4,461,622** |

`max_inner_work = 4,461,622 ≫ threshold (10000)` → fires. Severity is **Critical** (per rule impl at `join_rules.rs:30`), not Warning as ogagila's `@severity` marker claims — the marker is aspirational from a DBA perspective, not reflective of rule behavior.

The inner child is `IndexScan`, so `inner_has_index = true` and `join_column = "rental_id"` (extracted from `Index Cond: (rental_id = p.rental_id)`). This selects the `suggestion_has_index` template variant.

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.JOIN-001.detail:
  en: "Inner side processed %{rows} rows × %{loops} loops = %{total} total rows (threshold: %{threshold})"
finding.JOIN-001.detail_threshold:
  en: "%{child} (threshold: %{threshold})"
finding.JOIN-001.detail_has_index:
  en: ", inner table has index"
finding.JOIN-001.suggestion_has_index:
  en: "Nested Loop inner table already has an index but workload is still high; consider ANALYZE to update statistics or SET enable_nestloop = off"
```

Producing (note the placeholder bug called out below):

- **detail**: `"Inner side processed 278 rows × 16049 loops = 4461622 total rows (threshold: %{threshold}) (threshold: 10000), inner table has index"`
- **suggestion**: `"Nested Loop inner table already has an index but workload is still high; consider ANALYZE to update statistics or SET enable_nestloop = off"`

## Known rule bug captured by this case

**Tracked at:** https://github.com/c2j/ogexplain-analyzer/issues/35

The `finding.JOIN-001.detail` i18n template contains `%{threshold}`, but the rule's `t!()` call at `join_rules.rs:49-55` only passes `rows`, `loops`, `total` — never `threshold`. As a result, `detail_child` contains the literal text `(threshold: %{threshold})` unsubstituted. Then `finding.JOIN-001.detail_threshold` wraps `detail_child` and appends a *correctly substituted* `(threshold: 10000)` — so the final detail has `(threshold: %{threshold}) (threshold: 10000)`.

This case locks in the **current (buggy) behavior**. If the rule is fixed to pass `threshold` to the first `t!()` call (or the template is changed to drop the inner `(threshold: …)`), this case will fail and `expected.findings.json` must be updated. The mismatch is the regression signal.

## Why other ogagila `JOIN-001 target` queries don't fire

| Query | Plan shape | Why JOIN-001 doesn't fire |
|-------|------------|---------------------------|
| Q15 (5-table customer→rental→…) | All Hash Join | No `NestedLoop` node |
| Q16 (rental ↔ inventory) | Merge Join | No `NestedLoop` node |
| Q18 (film→film_actor→actor) | Nested Loop, but inner work = 20×20 = 400 | Below threshold 10000 |
| Q19 (customer → address) | Merge Join | No `NestedLoop` node |

Q17 is the only v3 query that genuinely triggers JOIN-001. This is itself a useful regression coverage signal — if the optimizer behavior changes in future OG releases, this case will catch it.

## Live-DB notes

- `live_db_verify = true`: fully replayable.
- The `/*+ nestloop(r p) */` hint is in the SQL text itself, so no `SET` GUC is required.
- `modifies_data = false`: pure SELECT.
