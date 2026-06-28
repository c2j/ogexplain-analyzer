# TYPE-001 — Suspected implicit type cast (BareColumnStringLiteral variant)

## What it tests

`TYPE-001` (`SuspectedImplicitTypeCast`) fires on a `SeqScan` whose `Filter` property contains an asymmetric comparison where a column is compared against a string literal that is implicitly cast to the column's numeric type. See `crates/ogexplain-core/src/analyzer/rules/type_coercion_rules.rs:10-73`.

The rule has two detection variants:

| Variant | Detection regex | Suggestion template |
|---------|-----------------|---------------------|
| `BareColumnStringLiteral` | `col\)*\s*=\s*'([^']+)'` and the captured literal parses as `f64` | `finding.TYPE-001.suggestion_bare_col` — drop quotes / add explicit cast |
| `ColumnToNumeric` | `\(col\)::numeric\s*=\s*'([^']+)'` (column forced to numeric) | `finding.TYPE-001.suggestion_col_to_numeric` — unify data types |

This case exercises the **BareColumnStringLiteral** variant.

## Input source

`source = "supplemental"` — `tests/fixtures/17_implicit_cast.txt`.

### Why supplemental (not ogagila)

A prior subagent surveyed ogagila Q44–Q47 and Q132 (the cast-related queries) and confirmed that **none** trigger TYPE-001: the ogagila optimizer always selects an IndexScan on the primary key for those queries, but TYPE-001 short-circuits at `type_coercion_rules.rs:26-28` unless `node.node_type == SeqScan`. There is no ogagila query that produces a SeqScan-with-numeric-literal-filter plan shape suitable for this rule, so the hand-authored `tests/fixtures/17_implicit_cast.txt` is used instead. The fixture is already proven to parse correctly (it is exercised by `tests/analyzer_tests.rs::scan_004_triggers_on_filter_without_index` and `type_001_triggers_on_implicit_cast`).

## Expected plan

```
Seq Scan on orders  (cost=0.00..25000.00 rows=100000 width=68) (actual time=0.034..234.567 rows=500 loops=1)
  Filter: (status = '42')
  Rows Removed by Filter: 500000
Total runtime: 235.089 ms
```

Single-node plan: `SeqScan` is the root (depth = 1).

## Why TYPE-001 fires (BareColumnStringLiteral)

Trace through `SuspectedImplicitTypeCast::check` (`type_coercion_rules.rs:25-72`):

| Check | Result |
|-------|--------|
| `node.node_type == SeqScan` | Yes |
| `properties.iter().find(label == "Filter")` | `Some((status = '42'))` |
| `extract_column_from_filter("(status = '42')")` (`utils.rs:172-184`) | strips parens, regex matches `status = '42'`, returns `Some("status")` |
| `has_symmetric_cast(filter)` | false — no `::type` annotation on either side of `=` |
| Pattern 1 regex `status\)*\s*=\s*'([^']+)'` captures | `'42'` |
| `"42".parse::<f64>().is_ok()` | true → returns `MismatchPattern::BareColumnStringLiteral` |
| `rows_removed = 500000.0` > 10.0 | Yes |
| `total_scanned = 500000 + 500 = 500500.0` | — |
| `rows_removed / total_scanned ≈ 0.999` > 0.5 | Yes → fires |

Severity is **Critical** (per `SuspectedImplicitTypeCast::severity()`); category is **TypeMismatch**.

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.TYPE-001.detail:
  en: "Seq Scan with filter '%{filter}' (%{desc}), filtered out %{removed} rows (total %{total}) — suspected implicit type cast preventing index usage"
finding.TYPE-001.desc_bare_col:
  en: "%{col} = '%{val}' (column vs string value)"
finding.TYPE-001.suggestion_bare_col:
  en: "WHERE %{col} = %{val} — suspected numeric column compared with string literal, consider removing quotes or adding explicit cast"
```

Substituting `%{filter} = (status = '42')`, `%{col} = status`, `%{val} = 42`, `%{removed} = 500000`, `%{total} = 500500`:

- **detail**: `"Seq Scan with filter '(status = '42')' (status = '42' (column vs string value)), filtered out 500000 rows (total 500500) — suspected implicit type cast preventing index usage"`
- **suggestion**: `"WHERE status = 42 — suspected numeric column compared with string literal, consider removing quotes or adding explicit cast"`

These produce the substrings asserted in `expected.findings.json`: `status = '42'`, `500000`, `suspected implicit type cast` (detail); `status`, `removing quotes` (suggestion).

## Why SCAN-004 co-fires on the same node

Fixture 17's SeqScan has both a `Filter` and `Rows Removed by Filter: 500000`. `SCAN-004` (`FilterWithoutIndex`) at `scan_rules.rs:128-211` fires when:

- `has_filter = true` (yes — `Filter` property present)
- `actual.rows > 0` AND `estimated.plan_rows / actual.rows > estimation_ratio` (default 10) OR `rows_removed > min_rows_removed` (default 500). Here `actual.rows = 500 > 0` and `ratio = 100000 / 500 = 200 > 10` → **should_fire = true**.

SCAN-004 is expected and desired here — the fixture genuinely has both the "implicit cast" problem (TYPE-001) and the broader "filter removed most rows without an index" problem (SCAN-004). This co-firing is captured in `findings` (not `anti_findings`) per the rule's actual behavior.

SCAN-004 detail: `"Seq Scan on orders with Filter: estimated 100000 rows but got 500 (ratio: 200x), Rows Removed by Filter: 500000"`. Suggestion: `"ANALYZE orders; also consider CREATE INDEX ON orders (status)"` (column `status` extracted by `extract_filter_columns`).

## Why neighbor rules do NOT fire

| Rule | Why it does not fire |
|------|----------------------|
| SCAN-001 | Bails at `scan_rules.rs:53-55` when `has_filter` is true (filtered scans belong to SCAN-004, not SCAN-001). |
| TYPE-004 | Requires LIKE/`~~` operator in the filter; fixture 17 uses `=` (no LIKE), so the substring check at `type_coercion_rules.rs:213-216` fails. |
| GEN-001 | `PlanTooDeep` fires when plan depth exceeds `max_plan_depth` (default 10); fixture 17 is a single SeqScan (depth = 1). |
| JOIN-001 | Requires a `NestedLoop` node with high row counts; fixture 17 has no join at all. |

## Live-DB notes

- `live_db_verify = false`: this case uses a hand-authored EXPLAIN (`tests/fixtures/17_implicit_cast.txt`), not a replayable ogagila query, so there is nothing to re-run against a live OG instance.
- `modifies_data = false`, `requires_delete_stats = false`: pure read-only static contract.
- `skip_live_reason = "supplemental EXPLAIN is hand-written; no live OG instance needed for static contract"`.

## What this case locks in

1. **Trigger shape**: SeqScan + Filter containing `col = '<numeric literal>'` + high `Rows Removed by Filter` → fires TYPE-001 with the `BareColumnStringLiteral` variant.
2. **Critical severity**: any future change that downgrades TYPE-001 to `Warning` or `Info` will fail `min_severity: critical`.
3. **Detail wording stability**: the i18n template's `%{filter}`, `%{desc}`, `%{removed}`, `%{total}` placeholders must keep producing the asserted substrings.
4. **Suggestion uses real column name**: `status` (not `text` or `numeric`) must appear in the suggestion, locking in the Bug B1 fix from `utils.rs::extract_column_from_filter`.
5. **Co-firing contract**: SCAN-004 fires alongside TYPE-001 on the same node — declared as a positive expectation, not noise.
