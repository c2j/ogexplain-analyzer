# MEM-001 — Sort spill to disk on fixture `05_sort_external_merge.txt`

## What it tests

`MEM-001` (`SortSpillToDisk`) fires on a Sort node when:

1. The node type is `Sort` or `VectorSort`
2. The `Sort Method` property value contains `"external"` (indicating external merge sort, i.e., disk spill)
3. The `Sort Key` property, if present, is appended to the detail text
4. The disk size (`Disk: NkB`) is extracted for the suggestion parameter

This case locks in MEM-001's behavior on the simplest possible triggering fixture: a single Sort node with `Sort Method: external merge  Disk: 48kB`.

## Input source

`source = "supplemental"` pointing at `tests/fixtures/05_sort_external_merge.txt`.

### Why supplemental (not ogagila)

All four MEM-001-targeted queries in ogagila benchmark v3 (Q24–Q27) include a `LIMIT` clause, which allows the optimizer to use **top-N heapsort** — an in-memory algorithm that only allocates O(LIMIT) memory. Even with `SET work_mem = '64kB'`, the sort fits in memory because LIMIT 50/100/200 requires only tens of kB. The pre-recorded EXPLAIN material in `lib/ogagila/benchmark/v3/explains/Q{24,25,26,27}.explain` all show `Sort Method: top-N heapsort`, never `external merge`.

For MEM-001 to trigger naturally, a query must sort a large row set **without** a LIMIT (or with a LIMIT large enough to exceed work_mem). No such query exists in the ogagila benchmark.

Fixture `05_sort_external_merge.txt` is the canonical MEM-001 positive test fixture from the existing test suite (used in `tests/analyzer_tests.rs::mem_001_triggers_on_sort_spill`). It is hand-authored EXPLAIN text showing a Sort on `events` with `Sort Method: external merge  Disk: 48kB`.

### Fixture 05 plan tree

```
Sort  (cost=63.85..66.35 rows=1000 width=44) (actual time=5.432..5.876 rows=1000 loops=1)
  Sort Key: created_at
  Sort Method: external merge  Disk: 48kB
  ->  Seq Scan on events  (cost=0.00..20.00 rows=1000 width=44) (actual time=0.012..1.234 rows=1000 loops=1)
```

## Why MEM-001 fires

| Check | Result |
|-------|--------|
| Node is `Sort` (`is_sort_node` returns true) | ✅ |
| `Sort Method` property present | ✅ (`"external merge  Disk: 48kB"`) |
| Value contains `"external"` | ✅ |
| `extract_disk_size` returns `"48kB"` | ✅ (matches "Disk: 48kB") |
| `Sort Key` property present (`"created_at"`) | ✅ → appended to detail |

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.MEM-001.detail:
  en: "Sort Method: %{value}"
finding.MEM-001.suggestion:
  en: "SET work_mem to a higher value; sort spilled to disk (%{disk}), consider creating an index on the sort key to eliminate sorting"
```

Substituting `%{value} = "external merge  Disk: 48kB"`, `%{disk} = "48kB"`:

- **detail base**: `"Sort Method: external merge  Disk: 48kB"`
- **detail with Sort Key**: `"Sort Method: external merge  Disk: 48kB, Sort Key: created_at"`
- **suggestion**: `"SET work_mem to a higher value; sort spilled to disk (48kB), consider creating an index on the sort key to eliminate sorting"`

These are the source of truth for `detail_must_contain` and `suggestion_must_contain` in [`expected.findings.json`](expected.findings.json).

## Why neighbor rules do NOT fire

| Rule | Reason it does not fire on fixture 05 |
|------|----------------------------------------|
| SCAN-001 | Seq Scan on events has estimated rows=1000 (< default `large_table_rows` threshold 10000). Also, `Sort` is in the `has_legitimate_full_scan_ancestor` allowlist (`scan_rules.rs:90`). |
| SCAN-004 | Requires a `Filter` property on the scan node. The Seq Scan has no Filter. |
| SORT-003 | Requires a Sort node to have at least one Sort descendant. Fixture 05 has exactly one Sort node — no nested sorts. |
| GEN-001 | Plan depth = 2 (Sort → Seq Scan). Default `max_plan_depth` = 10. Depth 2 does not exceed 10. |
| MEM-004 | Requires either a `Peak Memory:` summary line or a node with a `Memory Usage` property. The fixture has neither. |

## Why `source = "supplemental"` (not ogagila)

The ogagila benchmark v3 declares four candidates for MEM-001 (Q24–Q27), each with `SET work_mem` and a `LIMIT` clause. On live openGauss, these queries would likely do a **full external sort** (if work_mem is low and there is no index), but the pre-recorded EXPLAIN material shows top-N heapsort for all four:

| Query | SQL pattern | Actual Sort Method | Why not external |
|-------|-------------|-------------------|------------------|
| Q24 | `ORDER BY rental_date LIMIT 100` | `top-N heapsort Memory: 31kB` | LIMIT 100 fits in memory |
| Q25 | `ORDER BY amount DESC LIMIT 50` | `top-N heapsort Memory: 31kB` | LIMIT 50 fits in memory |
| Q26 | `GROUP BY ... ORDER BY total DESC LIMIT 20` | `top-N heapsort Memory: 27kB` | LIMIT 20 fits in memory |
| Q27 | `ORDER BY rental_date, customer_id LIMIT 200` | `top-N heapsort Memory: 34kB` | LIMIT 200 fits in memory |

A supplemental fixture is the only way to get a repeatable positive MEM-001 regression case without hand-authoring a new query and re-recording EXPLAIN material against a live OG database.

## Live-DB notes

- `live_db_verify = false`: fixture 05 references a non-existent `events` table (the fixture is hand-authored EXPLAIN, not captured from a real OpenGauss session). The table does not exist in the ogagila schema, so the underlying SQL cannot be replayed without authoring the original DDL. Static-mode validation (parse → analyze → compare) is fully sufficient.
- `modifies_data = false`: pure SELECT.
- `requires_delete_stats = false`: no schema mutations.
- `skip_live_reason` documents the limitation explicitly.
