# SUBQ-001 — Subquery not pulled up (INTERSECT branches as SubqueryScan)

## What it tests

`SUBQ-001` (`SubqueryNotPulledUp`) has two trigger variants in
`crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs`:

1. **SubqueryScan variant** (canonical): the engine visits a node whose
   `NodeType` is `SubqueryScan` (or `VectorSubqueryScan`). `check_with_ancestors`
   delegates straight to `check()` (`subquery_rules.rs:35-39`), which always
   fires — no threshold, no severity gate. It extracts the underlying table via
   `find_first_scan_descendant` (recursive DFS for the first scan node in the
   subtree) and emits the `detail_subquery_scan` template with that table.
2. **SubPlan variant** (secondary): any node carrying a property whose value
   contains `SubPlan`, but only when the node has **no** `SubqueryScan` ancestor
   (otherwise the SubqueryScan-level finding already represents the subtree).

This case locks in variant 1 — the canonical, table-name-extracting path.

## Input source

ogagila benchmark v3, **Q118** (`-- @target: SUBQ-001`):

```sql
SELECT customer_id FROM customer WHERE active = 1
INTERSECT
SELECT customer_id FROM payment WHERE amount > 5;
```

INTERSECT/EXCEPT set operations are exactly the kind of construct the OG
optimizer leaves as `SubqueryScan` wrappers around each branch (it cannot pull
set-operation arms into a regular join tree), so the plan contains genuine
`SubqueryScan` nodes rather than a fully-pulled-up join shape.

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q118.explain`)

```
HashSetOp Intersect  (cost=0.00..419.65 rows=148 width=4) (actual time=12.997..13.024 rows=584 loops=1)
  ->  Append  (cost=0.00..406.84 rows=5125 width=4) (actual time=0.078..11.941 rows=4541 loops=1)
        ->  Subquery Scan on "*SELECT* 2"  (cost=0.00..362.18 rows=3957 width=4) (actual time=0.077..11.316 rows=3957 loops=1)
              ->  Partition Iterator  (cost=0.00..322.61 rows=3957 width=4) (actual time=0.073..10.798 rows=3957 loops=1)
                    Iterations: 7
                    ->  Partitioned Seq Scan on payment  (cost=0.00..322.61 rows=3957 width=4) (actual time=0.165..10.242 rows=3957 loops=7)
                          Filter: (amount > 5::numeric)
                          Rows Removed by Filter: 12092
                          Selected Partitions:  1..7
        ->  Subquery Scan on "*SELECT* 1"  (cost=0.00..44.66 rows=1168 width=4) (actual time=0.028..0.264 rows=584 loops=1)
              ->  Seq Scan on customer  (cost=0.00..32.98 rows=1168 width=4) (actual time=0.025..0.189 rows=584 loops=1)
                    Filter: (active = 1)
                    Rows Removed by Filter: 15
Total runtime: 13.720 ms
```

## Why SUBQ-001 fires (twice — once per SubqueryScan)

The engine performs a DFS over every node. For each `SubqueryScan` node it fires
unconditionally:

| SubqueryScan node | `find_first_scan_descendant` result | `first_identifier` | detail table |
|-------------------|-------------------------------------|--------------------|--------------|
| `*SELECT* 2` | DFS: Partition Iterator (not scan) -> Partitioned Seq Scan on **payment** | `payment` | `payment` |
| `*SELECT* 1` | DFS: Seq Scan on **customer** | `customer` | `customer` |

So two `SUBQ-001` findings are produced. Both are `warning` severity,
`SubqueryStructure` category (per `SubqueryNotPulledUp::severity()` /
`category()`).

The third kind of node the rule watches for — a property containing `SubPlan` —
does **not** occur in this plan, so the `detail_subplan` variant is not
exercised here. `detail_must_not_contain: ["SubPlan"]` locks that in.

### Note on the two findings vs. `detail_must_contain`

The regress driver validates every `detail_must_contain` needle against
**every** finding matching the rule id (`tests/regress.rs:376-383` loops over all
`matching`). Because the two findings carry *different* table names (`payment`
vs `customer`), the only stable substrings are the common template fragments.
Hence `detail_must_contain` uses `["SubqueryScan", "involving table"]` — both
present in every `detail_subquery_scan` finding — rather than pinning a specific
table. Asserting `SubqueryScan` plus `detail_must_not_contain: ["SubPlan"]` is
sufficient to guarantee the SubqueryScan code path fired (and the SubPlan path
did not).

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.SUBQ-001.detail_subquery_scan:
  en: "Detected unpulled subquery (SubqueryScan), involving table: %{table}"
finding.SUBQ-001.suggestion_subquery_scan:
  en: "Rewrite as JOIN: /*+ EXPAND_SUBQUERY */; if correlated subquery: /*+ EXPAND_SUBLINK */; consider /*+ USE_MAGIC_SET */ optimization"
```

Substituting `%{table}=payment` and `%{table}=customer` respectively:

- **detail (finding 1)**: `"Detected unpulled subquery (SubqueryScan), involving table: payment"`
- **detail (finding 2)**: `"Detected unpulled subquery (SubqueryScan), involving table: customer"`
- **suggestion (both)**: `"Rewrite as JOIN: /*+ EXPAND_SUBQUERY */; if correlated subquery: /*+ EXPAND_SUBLINK */; consider /*+ USE_MAGIC_SET */ optimization"`

These are the source of truth for `detail_must_contain`, `detail_must_not_contain`
and `suggestion_must_contain` in `expected.findings.json`.

## Why the neighbor rules behave as declared

| Rule | Fires? | Why |
|------|--------|-----|
| **SCAN-001** (LargeTableFullScan) | No (anti) | Only fires on *unfiltered* `SeqScan`/`PartitionedSeqScan`. Both Q118 scans carry a `Filter`, short-circuited at `scan_rules.rs:53`. |
| **SCAN-004** (FilterWithoutIndex) | **Yes — co-fires on payment** | `Rows Removed by Filter=12092 > min_rows_removed=500` at `scan_rules.rs:159`. This is why SCAN-004 is deliberately **not** listed in `anti_findings`; it is a legitimate co-finding intentionally left unstated in `findings[]`. |
| **GEN-001** (PlanTooDeep) | No (anti) | Plan depth = 5; default `max_plan_depth = 10`. |
| **JOIN-001** (NestedLoopLargeDataset) | No (anti) | Requires a `NestedLoop` node; Q118 uses `HashSetOp Intersect -> Append`. |
| **SUBQ-006** (CorrelatedSubquerySelfUpdate) | No (anti) | Requires a DML root (`Update`/`ModifyTable`/`VectorUpdate`) at `subquery_rules.rs:176-182`; Q118 is a `SELECT`. |

## Why other ogagila `SUBQ-001 target` queries were not chosen

| Query | Trigger path | Why not chosen |
|-------|--------------|----------------|
| Q61 / Q62 / Q63 / Q121 | `SubPlan` property on the outer scan | Fires the *secondary* `detail_subplan` variant, not the canonical `detail_subquery_scan` path — this case prefers the SubqueryScan variant per the canonical-first policy. |
| Q111 (CTE + IN + EXISTS) | none | OG fully pulled the query up to `Hash Join`s — no `SubqueryScan` node and no `SubPlan` property. Does not trigger. |
| Q115 (LATERAL with InitPlan) | none | OG emits `InitPlan` (not `SubPlan`) properties; `any_property_contains(node, "SubPlan")` does not match `InitPlan`. Does not trigger. |
| Q119 (EXCEPT) | `SubqueryScan` x2 | Same canonical variant, but the plan is deeper (`Limit -> Sort -> HashSetOp -> Append -> Subquery Scan`); Q118 is the simplest SubqueryScan plan. |

## Live-DB notes

- `live_db_verify = true`: Q118 is a pure `SELECT ... INTERSECT SELECT ...` — fully replayable, no transaction state leakage.
- `modifies_data = false`.
- `requires_delete_stats = false`.
- No `SET` GUCs in Q118's `queries.sql` block, so `[side_effects]` is empty.
- ogagila `main` commit at authoring: `d960d8c` (see `expected.findings.json` -> `_meta.ogagila_commit`).
