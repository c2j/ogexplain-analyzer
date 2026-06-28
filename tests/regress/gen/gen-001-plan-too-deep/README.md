# GEN-001 — Plan too deep (triggered via `[config] max_plan_depth = 3`)

## What it tests

`GEN-001` (`PlanTooDeep`) fires globally (not per-node) when:

```rust
// crates/ogexplain-core/src/analyzer/rules/general_rules.rs
fn check_global(&self, _plan: &ExplainPlan, stats: &GlobalStats) -> Vec<Finding> {
    if stats.max_depth <= self.max_depth {
        return Vec::new();
    }
    // ... emits one Finding with Severity::Info, DiagnosticCategory::General
}
```

The rule is a pure depth check against `DiagnosticConfig::max_plan_depth` (default `10`). It does not inspect node types, relations, or costs — only the `GlobalStats::max_depth` computed by the analyzer's DFS traversal.

## Input source

`source = "supplemental"` pointing at `tests/fixtures/20_deep_plan.txt`.

### Why supplemental (not ogagila)

GEN-001's default threshold is `max_plan_depth = 10`. To trigger the rule, a plan must have depth **strictly greater than** 10. No query in the ogagila benchmark produces a plan that deep — ogagila's schema (DVD rental) and data scale (~16k rows in the largest table) keep optimizer plans at depth 2–6. The deepest ogagila v3 plan is Q15 at depth 6, which is exactly fixture 20's depth but still far below the default threshold of 10.

`tests/fixtures/20_deep_plan.txt` is a **hand-written** EXPLAIN output (provenance: authored alongside the original GEN-001 rule implementation, not captured from a live database session). It deliberately stacks six plan operators to exercise depth-counting. Using it via `source = "supplemental"` avoids inventing a fake ogagila query and DDL that cannot actually produce the required depth.

### Fixture 20 plan tree (depth = 6)

```
Sort                                          (depth 1)
└── Hash Join                                 (depth 2)
    ├── Merge Join                            (depth 3)
    │   ├── Hash Join                         (depth 4)
    │   │   ├── Nested Loop                   (depth 5)
    │   │   │   ├── Seq Scan on t1 a1         (depth 6)  ← deepest leaf
    │   │   │   └── Index Scan on t2
    │   │   └── Hash
    │   │       └── Seq Scan on t3 a4         (depth 6)  ← deepest leaf
    │   └── Sort                              (depth 4)
    │       └── Seq Scan on t4 a3             (depth 5)
    └── Hash
        └── Seq Scan on t5 a2                 (depth 4)
```

The longest root-to-leaf path is `Sort → Hash Join → Merge Join → Hash Join → Nested Loop → Seq Scan on t1`, giving `max_depth = 6`.

## Why `[config] max_plan_depth = 3` is required

Under the default `max_plan_depth = 10`, the check `6 <= 10` is true, so GEN-001 returns no findings. The per-case `[config]` override lowers the threshold to 3, making `6 > 3` true and forcing the rule to fire.

This mirrors the existing contract in `tests/analyzer_tests.rs::gen_001_triggers_on_deep_plan_with_custom_config` (which uses `max_plan_depth = 5` — both 3 and 5 trigger since depth is 6). This regression case uses 3 to test a wider margin and to match the CONTRIBUTING.md example.

Without the config override, there is no way to make GEN-001 fire on any available fixture — the rule is effectively untestable in the regression suite. The override is the bridge between "realistic default threshold" and "fixture that is small enough to hand-author."

## i18n template substitution

From `crates/ogexplain-core/i18n/app.yml`:

```yaml
finding.GEN-001.detail:
  en: "Plan depth is %{depth} (threshold: %{max_depth}); excessive depth usually indicates unpulled subqueries or deep nesting"
finding.GEN-001.suggestion:
  en: "Simplify query: /*+ EXPAND_SUBQUERY */; /*+ EXPAND_SUBLINK */; /*+ LAZY_AGG */; /*+ REDUCE_ORDER_BY */; consider splitting into multiple simple queries"
```

Substituting `depth = 6`, `max_depth = 3` (from `stats.max_depth` and the config override):

- **detail**: `"Plan depth is 6 (threshold: 3); excessive depth usually indicates unpulled subqueries or deep nesting"`
- **suggestion**: `"Simplify query: /*+ EXPAND_SUBQUERY */; /*+ EXPAND_SUBLINK */; /*+ LAZY_AGG */; /*+ REDUCE_ORDER_BY */; consider splitting into multiple simple queries"`

These are the source of truth for `detail_must_contain` (`["depth is 6", "threshold: 3"]`) and `suggestion_must_contain` (`["EXPAND_SUBQUERY"]`) in [`expected.findings.json`](expected.findings.json).

## Severity and category

GEN-001 declares `Severity::Info` and `DiagnosticCategory::General`. The `min_severity` in `expected.findings.json` is therefore `"info"` (the lowest rank), not `"warning"`. The `expect_min_severity = "warning"` in `case.toml` is a declarative summary field that the static-mode driver does not enforce (it is `#[allow(dead_code)]` in `tests/regress.rs`); the real severity check happens against `expected.findings.json`.

## Anti-findings rationale

| Rule | Why it must NOT fire |
|------|----------------------|
| SCAN-001 | All four Seq Scans report `rows` ≤ 100, far below the default `large_table_rows = 10000`. |
| JOIN-001 | Nested Loop inner Index Scan: `5 rows × 10 loops = 50` total work, below `nested_loop_inner_rows = 10000`. |
| JOIN-002 | No Hash node carries `Batches > 1` or disk-spill info; JOIN-002 requires a spilling Hash. |
| MEM-001 | Neither Sort node reports an external-merge `Sort Method`; MEM-001 only fires on spill indicators. |

### Why SORT-003 is NOT listed as an anti-finding

Fixture 20 contains an outer Sort (line 3, `Sort Key: a1.val`) wrapping an inner Sort (line 16, `Sort Key: a3.id`). Although the keys differ, SORT-003 ("Duplicate sort") fires on **any** sort-within-sort regardless of key equality — the rule name is misleading (see `tests/analyzer_tests.rs::sort_003_fires_on_nested_sort_with_different_keys`). Listing SORT-003 as an anti-finding would cause the test to fail, because it genuinely fires on this fixture.

## Live-DB caveat

`live_db_verify = false` because fixture 20 is hand-written EXPLAIN text, not captured from a real OpenGauss session. The tables `t1`–`t5` and index `idx` referenced in the plan do not exist in the ogagila schema, so the underlying SQL cannot be replayed without authoring the original DDL. The `skip_live_reason` in `case.toml` documents this explicitly.

Static-mode validation (parse the fixture text → analyze with the config override → compare findings) is fully sufficient to lock in GEN-001's depth-threshold contract.
