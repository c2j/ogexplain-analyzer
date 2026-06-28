# PART-001 — partition pruning failure via function-wrapped partition key

## What it tests

`PART-001` (`PartitionPruningFailure`) fires on:

- `NodeType::PartitionedSeqScan` or `NodeType::PartitionedCStoreScan`
- Node has structured `selected_partitions` property
- One of three trigger patterns:
  1. **`detail_function`** — has Filter, filter value contains a function (`date_part` / `EXTRACT` / `to_char` / `to_date` / `to_timestamp`), and selected range covers all partitions from partition 1 (`start <= 1 && n_scanned >= end`)
  2. **`detail_range_large`** — range spans more than `MAX_PARTITION_RANGE` (10) partitions
  3. **`detail_non_range`** — non-numeric range format with function-wrapped filter

This case locks in **pattern 1** (`detail_function`): the partition key is wrapped in a function in the WHERE clause, defeating partition pruning.

## Input source

ogagila benchmark v3, **Q57**:

```sql
SELECT COUNT(*) FROM payment WHERE EXTRACT(MONTH FROM payment_date) = 3;
```

ogagila's `payment` table is range-partitioned by `payment_date` into **7 monthly partitions** (`payment_p2022_01` … `payment_p2022_07`, covering Jan–Jul 2022), declared with openGauss inline `VALUES LESS THAN` syntax in `lib/ogagila/sqls/ddl/schema.sql`.

Because the predicate is `EXTRACT(MONTH FROM payment_date) = 3` rather than a range comparison on `payment_date` itself, the optimizer cannot prune to a single month partition — it must scan all 7 partitions and apply the function as a post-scan Filter.

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q57.explain`)

```
Aggregate  (cost=362.94..362.94 rows=1 width=8) (actual time=5.099..5.099 rows=1 loops=1)
  ->  Partition Iterator  (cost=0.00..362.74 rows=80 width=0) (actual time=0.944..4.852 rows=2713 loops=1)
        Iterations: 7
        ->  Partitioned Seq Scan on payment  (cost=0.00..362.74 rows=80 width=0) (actual time=3.764..4.584 rows=2713 loops=7)
              Filter: (date_part('month'::text, payment_date) = 3::double precision)
              Rows Removed by Filter: 13336
              Selected Partitions:  1..7
Total runtime: 5.238 ms
```

## Why PART-001 fires (pattern 1: `detail_function`)

Walking the rule (`crates/ogexplain-core/src/analyzer/rules/partition_rules.rs`):

| Check | Result |
|-------|--------|
| Node is `PartitionedSeqScan` | yes |
| `has_filter` (any property labelled `Filter`) | yes — `date_part(...) = 3` |
| `filter_has_function` — value contains one of `date_part` / `EXTRACT` / `to_char` / `to_date` / `to_timestamp` | yes — openGauss lowers `EXTRACT(MONTH FROM ...)` to `date_part('month', ...)` in EXPLAIN output |
| `structured_props.selected_partitions` present | yes — `"1..7"` |
| `parse_partition_range("1..7")` | `Some((1, 7))` |
| `n_scanned = 7 - 1 + 1 = 7` | 7 |
| Pattern 1 guard: `has_filter && start <= 1 && n_scanned >= end && filter_has_function` | `true && 1 <= 1 && 7 >= 7 && true` -> fires |

Rule severity: `Severity::Warning`. Rule category: `DiagnosticCategory::DistributionIssue`.

## Candidate queries triaged

| Query | Plan shape | PART-001 outcome |
|-------|-----------|------------------|
| **Q57** — `EXTRACT(MONTH FROM payment_date) = 3` | Partitioned Seq Scan with function Filter, `Selected Partitions: 1..7` | **FIRES** (pattern 1 — chosen) |
| Q58 — `payment_date IS NULL` | `Result` with `One-Time Filter: false` (constant-false folded) | does not fire — no PartitionedSeqScan node in plan |
| Q59 — `payment_date >= '...' OR customer_id = 100` | Partitioned Seq Scan, Filter lacks any of the five function markers | does not fire — `filter_has_function` is false; `n_scanned` (7) is also below `MAX_PARTITION_RANGE` (10) |
| Q60 — `SELECT COUNT(*) FROM payment` (no WHERE) | Partitioned Seq Scan, no Filter | does not fire — `has_filter` false, range count below threshold |
| Q160 — `payment_date BETWEEN '2022-06-01' AND '2022-06-30'` (after `CREATE INDEX ... LOCAL`) | `Partitioned Index Scan using idx_payment_date`, `Selected Partitions: 6` | does not fire — node type is `PartitionedIndexScan`, not matched by rule |

The ogagila `-- @target: PART-001` tags are aspirational; only Q57 actually triggers under the current rule logic. This matches the pilot's experience with JOIN-001 (aspirational targets need rule-level verification).

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.PART-001.detail_function:
  en: "Partition table has filter condition but scanned all %{count} partitions (%{range}), partition key may be wrapped in function/expression preventing pruning"
finding.PART-001.suggestion:
  en: "Partition table scanned too many partitions; ensure partition key uses constant expression filter; avoid functions on partition key (e.g. EXTRACT, to_date, to_char); check if partition key filter condition is missing"
```

Substituting `%{count}=7`, `%{range}="1..7"`:

- **detail** = `"Partition table has filter condition but scanned all 7 partitions (1..7), partition key may be wrapped in function/expression preventing pruning"`
- **suggestion** = `"Partition table scanned too many partitions; ensure partition key uses constant expression filter; avoid functions on partition key (e.g. EXTRACT, to_date, to_char); check if partition key filter condition is missing"`

These are the source of truth for `detail_must_contain` (`7 partitions`, `1..7`, `wrapped in function`) and `suggestion_must_contain` (`EXTRACT`, `constant expression filter`) in [`expected.findings.json`](expected.findings.json).

## Co-firing rules (declared neither as `must_fire` nor as `must_not_fire`)

- **SCAN-004** also fires on the same node. `FilterWithoutIndex` triggers because `Rows Removed by Filter: 13336` exceeds `min_rows_removed = 500.0`. This is expected and orthogonal to PART-001 (the harness permits undeclared co-firings); the case deliberately targets the PART-001 contract only.

## Live-DB notes

- `live_db_verify = true`: this case is fully replayable against a fresh ogagila container.
- `modifies_data = false`: pure `SELECT COUNT(*)`, no transaction state leakage.
- `requires_delete_stats = false`: no `DELETE STATISTICS` or schema mutation.
- `requires_set = {}`: no GUC overrides; Q57's `queries.sql` block declares no `SET` statements.
- `weak_signal = false`: PART-001 is not a distributed-only rule; it observes per-partition pruning behavior which is meaningful even on single-node centralized openGauss.
- ogagila `main` commit at authoring: `d960d8c` (see `expected.findings.json` -> `_meta.ogagila_commit`).
