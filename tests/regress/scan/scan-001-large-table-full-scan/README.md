# Pilot case: SCAN-001 — large-table full scan on `rental`

This is the **reference case** for the `tests/regress/` directory. If you're authoring a new case, copy this directory structure and adapt.

## What it tests

`SCAN-001` (`LargeTableFullScan`) fires on:

- `NodeType::SeqScan` or `NodeType::PartitionedSeqScan`
- **No** `Filter` property (pure full scan; filtered scans belong to SCAN-004)
- Not feeding a "legitimate full scan" ancestor (`HashJoin`, `Hash`, `Sort`, `HashAggregate`, `GroupAggregate`, `Unique`, `MergeJoin`, `Materialize`)
- `effective_scan_size(node) > large_table_rows` (default 10000)

## Input source

ogagila benchmark v3, **Q01**:

```sql
SELECT * FROM rental LIMIT 10;
```

The `rental` table has 16,044 rows in ogagila's seed data. The optimizer estimates `rows=16472`, which exceeds the default threshold of 10,000. The `Limit` node truncates actual output to 10 rows, but `effective_scan_size()` correctly falls back to `estimated.plan_rows` when no `Filter` is present (see `crates/ogexplain-core/src/analyzer/rules/utils.rs:87`).

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q01.explain`)

```
Limit  (cost=0.00..0.19 rows=10 width=40) (actual time=0.065..0.067 rows=10 loops=1)
  ->  Seq Scan on rental  (cost=0.00..318.72 rows=16472 width=40) (actual time=0.064..0.064 rows=10 loops=1)
Total runtime: 0.366 ms
```

## Why SCAN-001 fires

| Check | Result |
|-------|--------|
| Node is `SeqScan` | ✅ |
| No `Filter` property | ✅ |
| No legitimate full-scan ancestor (parent is `Limit`, not in the allowlist) | ✅ |
| `effective_scan_size` = `estimated.plan_rows` = `16472` > threshold `10000` | ✅ |

## Why SCAN-004 does NOT fire

SCAN-004 requires a `Filter` property on the scan node. Q01's SeqScan has none — it's a pure full scan truncated by the parent `Limit`. This is the clean separation between SCAN-001 (full scan) and SCAN-004 (filtered scan without index).

## Why GEN-001 does NOT fire

GEN-001 (`PlanTooDeep`) fires when plan depth exceeds `max_plan_depth` (default 10). Q01's plan depth is 2 (`Limit` → `SeqScan`).

## i18n template (for `detail` and `suggestion` predictions)

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.SCAN-001.detail:
  en: "Seq Scan on %{relation} scanned ~%{rows} rows (threshold: %{threshold})"
finding.SCAN-001.suggestion_no_cols:
  en: "Consider creating an index on the filtered columns of %{relation}"
```

Substituting `%{relation}=rental`, `%{rows}=16472`, `%{threshold}=10000`:

- **detail**: `"Seq Scan on rental scanned ~16472 rows (threshold: 10000)"`
- **suggestion**: `"Consider creating an index on the filtered columns of rental"`

These are the source of truth for `detail_must_contain` and `suggestion_must_contain` in [`expected.findings.json`](expected.findings.json).

## Live-DB notes

- `live_db_verify = true`: this case is fully replayable.
- `modifies_data = false`: pure SELECT, no transaction state leakage.
- `requires_delete_stats = false`: no schema mutations.
- ogagila `main` commit at authoring: `d960d8c` (see `expected.findings.json` → `_meta.ogagila_commit`).
