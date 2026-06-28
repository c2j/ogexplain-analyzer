# TYPE-004 — LIKE with leading wildcard (double-sided `%Action%`)

## What it tests

`TYPE-004` (`LikeWithLeadingWildcard`) fires on any plan node whose `Filter` or `Index Cond` property contains a LIKE/`~~` operator whose pattern literal begins with `%`. See `crates/ogexplain-core/src/analyzer/rules/type_coercion_rules.rs:196-245`.

The rule has two variants:

| Variant | Detection | Suggestion template |
|---------|-----------|---------------------|
| single-sided (`%foo`) | pattern starts with `%` but does NOT end with `%` | `finding.TYPE-004.suggestion_single` — pg_trgm or reverse index |
| double-sided (`%foo%`) | pattern starts AND ends with `%` (and `len > 1`) | `finding.TYPE-004.suggestion_double` — pg_trgm + GIN, or full text search |

This case exercises the **double-sided** variant.

## Input source

ogagila benchmark v3, **Q48**:

```sql
SELECT film_id, title FROM film WHERE title LIKE '%Action%' LIMIT 10;
```

The `title` column on `film` has no trigram/GIN index, so the optimizer falls back to a Seq Scan over all 1000 film rows. `%Action%` matches no rows in ogagila's seed data, so `actual.rows = 0` and `Rows Removed by Filter = 1000`.

## Expected plan (from `lib/ogagila/benchmark/v3/explains/Q48.explain`)

```
Limit  (cost=0.00..67.50 rows=1 width=19) (actual time=0.514..0.514 rows=0 loops=1)
  ->  Seq Scan on film  (cost=0.00..67.50 rows=1 width=19) (actual time=0.512..0.512 rows=0 loops=1)
        Filter: (title ~~ '%Action%'::text)
        Rows Removed by Filter: 1000
Total runtime: 0.733 ms
```

Note: OG renders `LIKE` as the internal operator `~~` in EXPLAIN output. The rule accepts any of `LIKE '%`, `like '%`, or `~~ '%`.

## Why TYPE-004 fires (double-sided variant)

| Check | Result |
|-------|--------|
| Node has property `Filter` | Yes: `(title ~~ '%Action%'::text)` |
| Filter value contains `~~ '%` substring | Yes |
| `extract_like_pattern` regex `(?:LIKE\|like\|~~)\s+'([^']+)'` captures | `%Action%` |
| `is_double_sided` (`starts_with('%') && ends_with('%') && len > 1`) | true |

Severity is **Warning** (per `LikeWithLeadingWildcard::severity()`); category is **TypeMismatch** (per `category()`).

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.TYPE-004.detail:
  en: "Filter condition contains leading wildcard LIKE '%{pattern}', cannot use B-tree index"
finding.TYPE-004.detail_double_sided:
  en: " (wildcards on both sides)"
finding.TYPE-004.suggestion_double:
  en: "Double-sided wildcard LIKE cannot use any index; suggestions: (1) pg_trgm extension + GIN index: CREATE EXTENSION pg_trgm; CREATE INDEX idx USING gin(col gin_trgm_ops); (2) full text search: to_tsvector + to_tsquery"
```

Substituting `%{pattern}` = `%Action%`:

- **detail**: `"Filter condition contains leading wildcard LIKE '%Action%', cannot use B-tree index (wildcards on both sides)"`
- **suggestion**: `"Double-sided wildcard LIKE cannot use any index; suggestions: (1) pg_trgm extension + GIN index: CREATE EXTENSION pg_trgm; CREATE INDEX idx USING gin(col gin_trgm_ops); (2) full text search: to_tsvector + to_tsquery"`

These produce the substrings asserted in `expected.findings.json`: `LIKE`, `%Action%`, `wildcards on both sides` (detail) and `pg_trgm`, `GIN` (suggestion).

## Why SCAN-004 co-fires on the same node

Q48's SeqScan has both a `Filter` and `Rows Removed by Filter: 1000`. `SCAN-004` (`FilterWithoutIndex`) at `scan_rules.rs:128-211` fires when:

- `has_filter = true` (yes — `Filter` property present)
- `actual.rows > 0` OR `rows_removed > min_rows_removed` (default 500). Here `actual.rows = 0` but `rows_removed = 1000 > 500` → **should_fire = true**.

SCAN-004 is expected and desired here — Q48 genuinely *also* has the "filter without an index" problem on top of the LIKE-with-wildcard problem. This co-firing is captured in `findings` (not `anti_findings`), per the rule's actual behavior.

## Why neighbor rules do NOT fire

| Rule | Why it does not fire |
|------|----------------------|
| SCAN-001 | Bails at `scan_rules.rs:53-55` when `has_filter` is true (filtered scans belong to SCAN-004, not SCAN-001). |
| TYPE-001 | `extract_column_from_filter` regex requires `=` operator; `~~` has none, so column extraction returns `None` at `type_coercion_rules.rs:35`. |
| SCAN-005 | Only matches `IndexScan` / `IndexOnlyScan` / `PartitionedIndexScan` / `CStoreIndexScan`; Q48 is a `SeqScan`. |
| GEN-001 | Plan depth = 2 (`Limit` → `SeqScan`), below default threshold 10. |

## Why other TYPE-004 candidate queries were not picked

| Query | Filter | Why not picked |
|-------|--------|----------------|
| Q49 (`actor.last_name LIKE '%son%'`) | `(last_name ~~ '%son%'::text)` | Also triggers TYPE-004 double-sided, but Q48 is first and the plan shape is identical. |
| Q50 (`customer.email LIKE '%@example.org'`) | `(email ~~ '%@example.org'::text)` | Triggers TYPE-004 **single-sided** variant (`%@example.org` ends with `g`, not `%`). Different suggestion template — kept as a future case for single-sided coverage. |
| Q146 (`title ILIKE '%love%' OR description ILIKE '%love%'`) | `(title ~~* '%love%'::text)` | Does NOT trigger TYPE-004: rule matches `LIKE '%` / `like '%` / `~~ '%` but not `~~* '%` (ILIKE operator has `*` between `~~` and space). Filed as a known rule gap. |

## Live-DB notes

- `live_db_verify = true`: pure SELECT, fully replayable.
- `modifies_data = false`: no transaction state leakage.
- `requires_delete_stats = false`: no schema mutations.
- The `-- @target: TYPE-004` marker in `queries.sql` carries no `SET` GUC; the Q48 block between `-- @id: Q48` and `-- @id: Q49` is a single SELECT. No `[side_effects].requires_set` needed.
- ogagila `main` commit at authoring: `d960d8c` (see `expected.findings.json` → `_meta.ogagila_commit`).
