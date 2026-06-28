# SORT-003 — Duplicate/redundant sort on stacked `ORDER BY` subqueries

## What it tests

`SORT-003` (`DuplicateSort`) fires on a Sort node that has at least one Sort descendant. The rule walks the whole subtree via `collect_child_sort_keys`, then picks one of two i18n variants:

- `detail_duplicate` — at least one descendant Sort has the **exact same** `Sort Key` string as the current node.
- `detail_redundant` — descendant Sort(s) exist but **none** share the current node's `Sort Key` (fallback).

## Input source

ogagila benchmark v3, **Q150**:

```sql
SELECT t1.customer_id, t1.amount, t2.amount
FROM (
    SELECT customer_id, amount FROM payment ORDER BY amount DESC LIMIT 10
) t1
JOIN (
    SELECT customer_id, amount FROM payment ORDER BY amount DESC LIMIT 10
) t2 USING (customer_id)
ORDER BY t1.amount DESC, t2.amount DESC;
```

Both branches use the same pattern (`ORDER BY amount DESC LIMIT 10` inside a subquery, then re-sorted by the outer `ORDER BY`). Q31, Q32, Q33 were the other SORT-003 candidates in `queries.sql` but none of them actually trigger the rule (see "Why Q150 is the only candidate that fires" below).

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q150.explain`)

Three Sort nodes — annotated `[A]`, `[B]`, `[C]`:

```
Sort [A]  (cost=1259.04..1259.04 rows=1 width=16) (actual time=17.592..17.592 rows=10 loops=1)
  Sort Key: public.payment.amount DESC, t2.amount DESC
  Sort Method: quicksort  Memory: 25kB
  ->  Hash Join  (cost=1258.86..1259.03 rows=1 width=16) (actual time=17.443..17.459 rows=10 loops=1)
        Hash Cond: (public.payment.customer_id = t2.customer_id)
        ->  Limit  (cost=629.30..629.33 rows=10 width=10) (actual time=9.585..9.590 rows=10 loops=1)
              ->  Sort [B]  (cost=629.30..669.43 rows=16049 width=10) (actual time=9.582..9.586 rows=10 loops=1)
                    Sort Key: public.payment.amount DESC
                    Sort Method: top-N heapsort  Memory: 25kB
                    ->  Partition Iterator  (cost=0.00..282.49 rows=16049 width=10)
                          Iterations: 7
                          ->  Partitioned Seq Scan on payment  (...)
                                Selected Partitions:  1..7
        ->  Hash  (cost=629.43..629.43 rows=10 width=10) (...)
              Buckets: 32768  Batches: 1  Memory Usage: 257kB
              ->  Subquery Scan on t2  (...)
                    ->  Limit  (...)
                          ->  Sort [C]  (...)
                                Sort Key: public.payment.amount DESC
                                Sort Method: top-N heapsort  Memory: 25kB
                                ->  Partition Iterator  (...)
                                      ->  Partitioned Seq Scan on payment  (...)
Total runtime: 17.762 ms
```

## Why SORT-003 fires on Sort [A] (the redundant variant)

The engine's DFS visits each Sort node and calls `DuplicateSort::check`:

| Sort node | `current_key` | `collect_child_sort_keys` result | `duplicates` (child key == current_key) | Variant |
|-----------|---------------|----------------------------------|------------------------------------------|---------|
| `[A]` outer | `public.payment.amount DESC, t2.amount DESC` | `[("Sort", "public.payment.amount DESC"), ("Sort", "public.payment.amount DESC")]` | `[]` (neither child key matches the two-column outer key) | **`detail_redundant`** |
| `[B]` inner-left | `public.payment.amount DESC` | `[]` (descendants are Partition Iterator + Partitioned Seq Scan) | `[]` | does not fire |
| `[C]` inner-right | `public.payment.amount DESC` | `[]` | `[]` | does not fire |

The decisive detail: `Sort [A]` sorts on **two** columns (the outer `ORDER BY t1.amount DESC, t2.amount DESC`), while the inner sorts sort on **one** (`payment.amount DESC`). The rule compares keys by exact string equality, so the two-column key is *not* equal to the one-column key — this falls through to `detail_redundant` rather than `detail_duplicate`.

Produced detail text (from `finding.SORT-003.detail_redundant`):

```
Sort node has a Sort child — redundant sorting detected
```

Produced suggestion text (from `finding.SORT-003.suggestion_with_key`, since `current_key` is non-empty):

```
Eliminate duplicate sort: create an index on column(s) public.payment.amount DESC, t2.amount DESC, or use /*+ REDUCE_ORDER_BY */ to remove redundant sorting
```

These are the source of truth for `detail_must_contain` (`["Sort", "redundant"]`) and `suggestion_must_contain` (`["public.payment.amount DESC", "REDUCE_ORDER_BY"]`) in [`expected.findings.json`](expected.findings.json). `detail_must_not_contain: ["identical"]` locks in that this is the *redundant* variant — if a future change makes the rule pick `detail_duplicate` ("identical Sort Key"), this case will fail and signal the regression.

## Why Q150 is the only SORT-003 candidate that fires

The other three `-- @target: SORT-003` queries in `queries.sql` only ever produce a single Sort node, so `collect_child_sort_keys` returns empty and SORT-003 short-circuits at `sort_rules.rs:38`:

| Query | Plan shape (root → leaves) | Sort count | Why SORT-003 does not fire |
|-------|----------------------------|------------|---------------------------|
| Q31 | `Sort → Limit → Index Scan` | 1 | No Sort descendant |
| Q32 | `Sort → Result → Append → [SeqScan, SeqScan]` | 1 | No Sort descendant |
| Q33 | `Unique → Sort → SeqScan` | 1 | No Sort descendant |
| **Q150** | `Sort [A] → HashJoin → [Limit → Sort [B] → ...], [Hash → SubqueryScan → Limit → Sort [C] → ...]` | **3** | **Outer Sort [A] has two Sort descendants → fires** |

Q150 is selected per the task rule "pick the FIRST Q that actually triggers" — it is the only candidate that triggers.

## Why co-firing SUBQ-001 is expected (not an anti-finding)

Q150's plan contains a `Subquery Scan on t2` node, on which SUBQ-001 unconditionally fires (see `subquery_rules.rs:35`). This is a **genuine co-finding**, not a false positive — the SQL really does wrap a derived table that the optimizer did not pull up. SUBQ-001 is therefore deliberately omitted from `anti_findings`. The case's `findings` array focuses on SORT-003 only; the driver does not enforce "all fired rules must be listed", so SUBQ-001 firing alongside SORT-003 does not affect the test outcome.

## Why the neighbor rules do NOT fire

| Rule | Reason it does not fire on Q150 |
|------|----------------------------------|
| SCAN-001 | Both `Partitioned Seq Scan on payment` nodes have a `Sort` ancestor, which is in the `has_legitimate_full_scan_ancestor` allowlist (`scan_rules.rs:83`). SCAN-001 treats the scan as a legitimate full-table dump feeding the sort and suppresses the finding, even though est rows=16049 > threshold 10000. |
| SCAN-004 | Requires a `Filter` property on the scan. The Partitioned Seq Scans have only `Selected Partitions: 1..7`, no Filter. |
| MEM-001 | Requires `Sort Method` containing `"external"`. All three Sorts use `quicksort` or `top-N heapsort` (in-memory). |
| GEN-001 | `max_depth` = 8 (longest path: Sort → Hash Join → Hash → Subquery Scan → Limit → Sort → Partition Iterator → Partitioned Seq Scan) ≤ default threshold 10. |
| JOIN-001 | Requires `NodeType::NestedLoop`. Q150 uses `HashJoin` on `customer_id`; no nested loop in the plan. |

## i18n templates (for reference)

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.SORT-003.detail_duplicate:
  en: "Sort node has child Sort with identical Sort Key: %{key} (duplicate nodes: %{duplicates})"
finding.SORT-003.detail_redundant:
  en: "Sort node has a Sort child — redundant sorting detected"
finding.SORT-003.suggestion_with_key:
  en: "Eliminate duplicate sort: create an index on column(s) %{key}, or use /*+ REDUCE_ORDER_BY */ to remove redundant sorting"
finding.SORT-003.suggestion_no_key:
  en: "Remove the inner Sort by adjusting ORDER BY or adding appropriate indexes"
```

## Live-DB notes

- `live_db_verify = true`: this case is fully replayable against a fresh ogagila container.
- `modifies_data = false`: pure SELECT, no transaction-state leakage.
- `requires_delete_stats = false`: no schema mutations.
- No `SET` GUCs in `queries.sql` Q150 block — `[side_effects] requires_set = {}`.
- ogagila `main` commit at authoring: `d960d8c` (see `expected.findings.json` → `_meta.ogagila_commit`).
