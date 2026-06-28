# SUBQ-006 — Correlated subquery self-referencing UPDATE

## What it tests

`SUBQ-006` (`CorrelatedSubquerySelfUpdate`) fires on a DML root node
(`Update`, `ModifyTable`, or `VectorUpdate`) whose subtree contains both a
`SubPlan` property and a scan on the same table being modified. The rule
detects the classic correlated-subquery self-update anti-pattern where each
row of the UPDATE/DELETE triggers a per-row subquery lookup — an O(n²)
execution risk.

The rule implementation lives at
`crates/ogexplain-core/src/analyzer/rules/subquery_rules.rs:156-207`.

### Trigger conditions (all must hold)

| Check | Code location | This fixture |
|-------|---------------|--------------|
| Node type is `Update` / `ModifyTable` / `VectorUpdate` | `subquery_rules.rs:176-182` | `Update on employees` |
| `extract_target_table(node)` returns a table name | `utils.rs::extract_target_table` | `"employees"` |
| `signals.has_subplan` = true | `subquery_rules.rs:229-232` | `SubPlan 1` property on Seq Scan |
| `signals.same_table_scan` = true | `subquery_rules.rs:234-240` | Seq Scan on `employees` + Index Scan on `employees e` both match target |

When `has_streaming` is true the detail gets a
`detail_streaming` suffix; this fixture has no Streaming nodes so the suffix is
absent (`detail_must_not_contain: ["Streaming", "distributed"]` locks that in).

## Input source

Supplemental EXPLAIN from `tests/fixtures/23_correlated_subquery_update.txt`
(also used by `tests/analyzer_tests.rs::subq_006_triggers_on_correlated_subquery_update`).

This case uses `source = "supplemental"` because no ogagila benchmark query
produces this exact correlated-self-update plan shape. The ogagila benchmark is
SELECT-heavy and does not exercise the SubPlan-correlated-to-outer-DML-table
pattern that SUBQ-006 targets. The fixture is hand-written EXPLAIN TEXT that has
been proven to parse correctly by the existing analyzer test suite.

### Fixture plan

```
Update on employees  (cost=0.00..35000.00 rows=1000 width=100)
  ->  Seq Scan on employees  (cost=0.00..15.00 rows=1000 width=50) (actual time=0.010..5.230 rows=1000 loops=1)
        SubPlan 1
          ->  Index Scan using emp_pkey on employees e  (cost=0.00..35.00 rows=1 width=20) (actual time=0.003..0.005 rows=1 loops=1000)
                Index Cond: (emp_id = employees.emp_id)
Total runtime: 5023.456 ms
```

The `loops=1000` on the Index Scan is the visible O(n²) signal: the correlated
subquery runs once per outer row (1000 rows × 1 loop each = 1000 executions).

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.SUBQ-006.detail:
  en: "Detected correlated subquery self-referencing UPDATE (table: %{table}), SubPlan exists: %{subplan}, same table scan: %{scan}"
finding.SUBQ-006.suggestion:
  en: "Correlated subquery self-referencing UPDATE has O(n²) risk from row-by-row execution; suggestions:\n  Method 1 (UPDATE FROM): UPDATE %{table} SET ... = t.new_val FROM (SELECT %{col}, ... FROM %{table}) t WHERE %{table}.%{col} = t.%{col};\n  Method 2 (CTE): WITH new_vals AS (SELECT %{col}, ... FROM %{table}) UPDATE %{table} SET ... = n.new_val FROM new_vals n WHERE %{table}.%{col} = n.%{col};"
```

### Placeholder derivation

| Placeholder | Source | Value |
|-------------|--------|-------|
| `%{table}` | `extract_target_table(Update node)` → relation on Update node | `employees` |
| `%{subplan}` | `signals.has_subplan` (SubPlan 1 property found in subtree) | `true` |
| `%{scan}` | `signals.same_table_scan` (Seq Scan + Index Scan both on employees) | `true` |
| `%{col}` | `signals.correlation_column` extracted from `Index Cond: (emp_id = employees.emp_id)` via `extract_innermost_parens` → split on `=` → last segment after `.` | `emp_id` |

### Substituted strings

- **detail**: `"Detected correlated subquery self-referencing UPDATE (table: employees), SubPlan exists: true, same table scan: true"`
- **suggestion**: `"Correlated subquery self-referencing UPDATE has O(n²) risk from row-by-row execution; suggestions:\n  Method 1 (UPDATE FROM): UPDATE employees SET ... = t.new_val FROM (SELECT emp_id, ... FROM employees) t WHERE employees.emp_id = t.emp_id;\n  Method 2 (CTE): WITH new_vals AS (SELECT emp_id, ... FROM employees) UPDATE employees SET ... = n.new_val FROM new_vals n WHERE employees.emp_id = n.emp_id;"`

These are the source of truth for `detail_must_contain`,
`detail_must_not_contain`, and `suggestion_must_contain` in
`expected.findings.json`.

## Anti-findings rationale

| Rule | Fires? | Why |
|------|--------|-----|
| **SCAN-001** | No | Seq Scan on employees has `rows=1000 < threshold 10000`. Also no Filter (pure scan). |
| **SCAN-004** | No | Requires a `Filter` property on the scan. The Seq Scan driving the UPDATE has none. |
| **GEN-001** | No | Plan depth is ~4 (Update → Seq Scan → SubPlan → Index Scan), well below `max_plan_depth=10`. |
| **JOIN-001** | No | Requires a `NestedLoop` node. This is a DML plan with no join. |

### Note on SUBQ-001 (co-finding, not anti)

SUBQ-001 (`SubqueryNotPulledUp`) may co-fire because the Seq Scan carries a
`SubPlan` property and has no `SubqueryScan` ancestor — triggering the
standalone-SubPlan variant of SUBQ-001 (`subquery_rules.rs:43-57`). This is a
legitimate co-finding representing the same subquery from a different angle
(not-pulled-up vs self-referencing-update), not a false positive. It is
deliberately **not** listed in `anti_findings` for the same reason SCAN-004 is
omitted from the SUBQ-001 sibling case's anti list.

## Live-DB caveat

- `live_db_verify = false`: this case uses `source = "supplemental"` with a
  hand-written EXPLAIN text. There is no corresponding DDL for the `employees`
  table in ogagila's seed data, so the plan cannot be replayed against a live
  OpenGauss instance.
- `modifies_data = true`: the fixture is an UPDATE plan (relevant if live-db
  mode is ever extended to supplemental sources).
- `skip_live_reason` documents why live verification is skipped.
