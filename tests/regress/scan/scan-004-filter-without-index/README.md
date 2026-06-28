# SCAN-004 — Filter without index on `rental`

## What it tests

`SCAN-004` (`FilterWithoutIndex`) fires on:

- `SeqScan` / `PartitionedSeqScan` / `CStoreScan` / `BitmapHeapScan` / `PartitionedBitmapHeapScan`
- Has `Filter` property
- Either `estimated.plan_rows / actual.rows > 10` (estimation_ratio) OR `Rows Removed by Filter > 500` (min_rows_removed)

## Input source

ogagila benchmark v3, **Q09**:

```sql
SELECT COUNT(*) FROM rental WHERE return_date IS NULL;
```

The `rental` table has 16,044 rows; only ~183 are estimated to satisfy `return_date IS NULL` and 0 actually do (in the seed data). The optimizer estimates `rows=188`, actual returns 0, with 16,044 rows removed by filter.

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q09.explain`)

```
Aggregate  (cost=319.19..319.20 rows=1 width=8) (actual time=3.360..3.361 rows=1 loops=1)
  ->  Seq Scan on rental  (cost=0.00..318.72 rows=188 width=0) (actual time=3.350..3.350 rows=0 loops=1)
        Filter: (return_date IS NULL)
        Rows Removed by Filter: 16044
Total runtime: 3.487 ms
```

## Why SCAN-004 fires

| Check | Result |
|-------|--------|
| Node is `SeqScan` | ✅ |
| Has `Filter` property | ✅ `Filter: (return_date IS NULL)` |
| `actual.rows > 0`? | ❌ → falls to else branch |
| `rows_removed (16044) > min_rows_removed (500)`? | ✅ |

## Why SCAN-001 does NOT fire

SCAN-001's first check after `SeqScan`/`PartitionedSeqScan` match is `has_filter` — if Filter is present, return None (filtered scans belong to SCAN-004). Q09 has Filter, so SCAN-001 short-circuits.

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.SCAN-004.detail:
  en: "%{node_label} on %{relation} with Filter: estimated %{estimated} rows but got %{actual} (ratio: %{ratio}x)"
finding.SCAN-004.suggestion_with_cols:
  en: "ANALYZE %{relation}; also consider CREATE INDEX ON %{relation} (%{cols})"
```

Substituting `%{node_label}=Seq Scan`, `%{relation}=rental`, `%{estimated}=188`, `%{actual}=0`, `%{ratio}=inf` (since `actual.rows=0` triggers `f64::INFINITY` path), plus the appended `, Rows Removed by Filter: 16044`:

- **detail**: `"Seq Scan on rental with Filter: estimated 188 rows but got 0 (ratio: infx), Rows Removed by Filter: 16044"`
- **suggestion**: `"ANALYZE rental; also consider CREATE INDEX ON rental (return_date)"`

The column name `return_date` appears in **suggestion** (extracted from Filter via `extract_filter_columns` regex), **not** in detail.

## Live-DB notes

- `live_db_verify = true`: fully replayable.
- `modifies_data = false`: pure SELECT.
- `requires_delete_stats = false`: no schema mutations.
