# Contributing to tests/regress/

This guide covers everything you need to author a new regression case. Read it once before writing your first case; thereafter, copy an existing case (`scan/scan-001-large-table-full-scan/`) and adapt.

## Mental model

A regression case is a **contract** with four parts:

1. **Input source** — where the EXPLAIN text comes from (ogagila `Q*.explain` file, supplemental, or hybrid).
2. **Side effects** — what session state must be set up before re-running the query (e.g. `SET work_mem = '64kB'`).
3. **Expected findings** — which rules fire, with what severity, mentioning which entities.
4. **Anti-findings** — which rules must *not* fire (false-positive guards).

The contract is **hand-authored**. Do not generate `expected.findings.json` by running ogexplain and dumping the output — that locks in whatever the tool currently does, defeating the purpose of regression testing. Instead:

1. Read the rule implementation in `crates/ogexplain-core/src/analyzer/rules/<rule>.rs`.
2. Read the i18n template in `crates/ogexplain-core/i18n/app.yml` to predict the exact `detail` and `suggestion` format.
3. Cross-check against the rule's `#[cfg(test)]` block and `tests/analyzer_tests.rs` for existing behavior.
4. Write down what *should* fire, with concrete expected substrings pulled from the template's placeholders.

## Directory and naming

```
tests/regress/<category>/<rule-id-lower>-<kebab-case-scenario>/
├── case.toml
├── expected.findings.json
├── supplemental.sql          ← only if ogagila data is insufficient
└── README.md                  ← optional, for non-trivial cases
```

**Rules:**

- `<category>` is one of: `scan`, `join`, `mem`, `sort`, `net`, `est`, `push`, `type`, `vec`, `gen`, `subq`, `agg`, `dist`, `stats`, `part`.
- `<rule-id-lower>` is the rule ID lowercased (e.g. `scan-001`, `subq-006`).
- `<kebab-case-scenario>` is a short human-readable suffix (`large-table-full-scan`, `correlated-self-update-distributed`).
- One directory = one case. Multiple scenarios of the same rule live as sibling directories.

## `case.toml` schema

TOML, top-level fields plus three tables. All fields are **required** unless marked optional.

```toml
# ── Identity ──────────────────────────────────────────────────────────────
rule_id             = "SCAN-001"               # must match DiagnosticRule::id()
case_name           = "scan-001-large-table-full-scan"  # kebab-case, MUST match directory name
expect_fired        = true                     # false for healthy/anti cases
expect_min_severity = "warning"                # one of: info | warning | critical

# ── [dataset] — where the SQL and EXPLAIN material come from ──────────────
[dataset]
# Three modes:
#   ogagila      — use lib/ogagila/benchmark/<ver>/explains/Q*.explain + queries.sql
#   supplemental — use a hand-authored EXPLAIN text file (path relative to CARGO_MANIFEST_DIR)
#   hybrid       — ogagila base + supplemental extras (rare; document why in README.md)
source              = "ogagila"
ogagila_version     = "v3"                              # ogagila benchmark version
ogagila_query_ids   = ["Q01"]                           # references -- @id in queries.sql

# Required only when source = "supplemental":
# Path is relative to CARGO_MANIFEST_DIR (workspace root). Can reference an existing
# tests/fixtures/*.txt file (recommended — proven to parse) or a case-local file.
# supplemental_file = "tests/fixtures/05_sort_external_merge.txt"

# ── [side_effects] — session state required for replay ────────────────────
[side_effects]
# Any GUC that must be SET before EXPLAIN. Driver issues these inside the
# per-case transaction; ROLLBACK restores defaults.
requires_set         = { work_mem = "64kB" }             # optional, default {}
modifies_data        = false                              # UPDATE/INSERT/DELETE present
requires_delete_stats = false                             # OG-unsupported; if true, live-db skips

# ── [verification] — how this case is validated ───────────────────────────
[verification]
live_db_verify  = true        # participate in --features live-db runs
weak_signal     = false       # true for DIST/SKEW/NET on single-node OG
# Skip reasons are documented, not silently dropped:
skip_live_reason = ""         # e.g. "OG does not support DELETE STATISTICS"

# ── [config] — optional DiagnosticConfig overrides ────────────────────────
# Use when the case needs non-default thresholds to trigger (e.g. small fixture
# data with a high default threshold). All fields optional; missing fields
# fall back to DiagnosticConfig::default().
#
# [config]
# max_plan_depth      = 3      # GEN-001: trigger on shallower plans
# memory_threshold_kb = 500    # MEM-004: trigger on sub-MB peaks
# large_table_rows    = 100    # SCAN-001: trigger on smaller tables
# estimation_skew_factor = 10  # EST-001: trigger on smaller ratios
# nested_loop_inner_rows = 100 # JOIN-001: trigger on smaller inner work
# sort_time_ratio     = 0.1    # MEM-001: ratio threshold
# disabled_rules      = []     # additional rule deny-list
# dedup_per_node      = false  # one finding per node_line
```

### Side-effect precedence

When `ogagila_query_ids` references a query whose `-- @scenario` block in `queries.sql` already contains `SET` statements (e.g. Q24–Q30 set `work_mem`), **do not duplicate them** in `[side_effects]`. The driver parses the `queries.sql` block and applies every `SET` / `UPDATE` / `DELETE STATISTICS` between the `-- @id` marker and the next `-- @id` (or EOF).

`[side_effects]` is only for cases that need **additional** GUCs beyond what `queries.sql` declares.

## `expected.findings.json` schema

```json
{
  "_meta": {
    "ogagila_commit":   "d960d8c",                  // git rev-parse of lib/ogagila at authoring time
    "ogagila_version":  "v3",                        // benchmark version
    "ogagila_query_ids": ["Q01"],                    // must match case.toml
    "ogexplain_version": "0.4.5",                    // current crate version (from workspace Cargo.toml)
    "authored_at":      "2026-06-28",
    "author":           "c2j",
    "review_notes":     "Q01 (SELECT * FROM rental LIMIT 10) → Limit→SeqScan on rental, est 16472 rows. SCAN-001 fires on pure full scan; SCAN-004 does NOT (no Filter)."
  },

  "findings": [
    {
      "rule_id":               "SCAN-001",
      "must_fire":             true,
      "min_severity":          "warning",
      "category":              "ScanEfficiency",
      "detail_must_contain":   ["rental", "16472"],
      "detail_must_not_contain": ["Index"],
      "suggestion_must_contain": ["rental"]
    }
  ],

  "anti_findings": [
    {
      "rule_id":       "SCAN-004",
      "must_not_fire": true,
      "reason":        "No Filter on the SeqScan; SCAN-004 requires Filter property."
    },
    {
      "rule_id":       "GEN-001",
      "must_not_fire": true,
      "reason":        "Plan depth = 2 (Limit → SeqScan); default threshold = 10."
    }
  ]
}
```

### Field semantics

| Field | Type | Meaning |
|-------|------|---------|
| `must_fire` | bool | If true: at least one Finding with this `rule_id` MUST appear. If false: NOT EVEN ONE Finding may appear. |
| `min_severity` | string | The Finding's `severity` must be at least this level (info < warning < critical). Use `info` to allow any. |
| `category` | string | The Finding's `category` must match (e.g. `ScanEfficiency`, `JoinStrategy`). See `crates/ogexplain-core/src/analyzer/report.rs::DiagnosticCategory` for the enum. |
| `detail_must_contain` | string[] | Every listed substring must appear in `Finding.detail` (case-sensitive). |
| `detail_must_not_contain` | string[] | None of these substrings may appear in `Finding.detail`. Useful for catching false mentions (e.g. ensure SCAN-001 detail does NOT mention "Index"). |
| `suggestion_must_contain` | string[] | Same as `detail_must_contain` but for `Finding.suggestion`. Empty array = skip suggestion check. |

### Severity ordering

```
info (0) < warning (1) < critical (2)
```

A `min_severity: "warning"` accepts `warning` or `critical` but rejects `info`.

## Authoring workflow

1. **Pick the rule** — check `crates/ogexplain-core/src/analyzer/rules/mod.rs::all_rules()` for the rule's struct name.
2. **Pick the case scenario** — search `lib/ogagila/benchmark/<ver>/queries.sql` for `-- @target: <RULE-ID>`. If a query fits, use it. If none does, you'll need `source = "supplemental"`.
3. **Read the rule** — open the rule file. Note:
   - What `NodeType` triggers it.
   - What conditions short-circuit (`return None`).
   - What i18n keys produce `detail` / `suggestion`.
4. **Read the i18n template** — `crates/ogexplain-core/i18n/app.yml`. Substitute placeholders mentally:
   - `%{relation}` ← `node.relation`
   - `%{rows}`, `%{threshold}`, `%{estimated}`, `%{actual}` ← rule-specific values
5. **Read the EXPLAIN material** — for ogagila cases, `cat lib/ogagila/benchmark/<ver>/explains/Q<id>.explain`. Confirm the trigger conditions hold.
6. **Capture the ogagila commit** — `git -C lib/ogagila rev-parse --short HEAD`. Put it in `_meta.ogagila_commit`.
7. **Write `case.toml`** — declare the data source and side effects.
8. **Write `expected.findings.json`** — fill `_meta` and `findings` based on your manual analysis. Then list every "neighbor" rule that *could plausibly* fire in this scenario and add it to `anti_findings` with a one-line `reason`.
9. **Self-review** — re-read the rule implementation and ask: "If I were the optimizer and saw this EXPLAIN, would the rule fire? Why?" If you can't confidently answer, ask in review.

## What goes in `anti_findings`

Anti-findings are the most valuable part of regression testing — they catch false positives that substring assertions on `detail` cannot.

**Always add** a rule to `anti_findings` when:

- It shares the trigger node type (e.g. for SCAN-001, list SCAN-004 since both fire on SeqScan).
- It shares the semantic domain (e.g. for JOIN-001, list JOIN-002, JOIN-003).
- The case scenario explicitly tests the boundary (e.g. Q01 has `LIMIT 10`, so EST-001 *could* see est=16472 vs actual=10 — list it as anti even if you think it won't fire, so a future regression catches it).

**Don't list** every rule — that's noise. Aim for 2–5 anti-findings per case.

## Drift management (ogagila `main` policy)

Each `expected.findings.json` records the ogagila commit at authoring time in `_meta.ogagila_commit`. The driver, at test time, compares this to the current submodule commit:

| Match | Behavior |
|-------|----------|
| Equal | Silent pass. |
| Different | `eprintln!("WARN: ogagila moved {} → {}; expected for {} may be stale", expected, current, case_dir)`. Test still passes (or fails) based on actual findings vs expected. |
| Different + `--strict` | Hard fail. |

### When drift is detected

1. Run `cd lib/ogagila && git log --oneline <expected_commit>..HEAD -- benchmark/` to see what changed.
2. Re-run the case in `--features live-db` mode. If findings still match expected, the change was benign — update `_meta.ogagila_commit` to current.
3. If findings differ, investigate which side is correct:
   - **ogagila changed EXPLAIN output** (e.g. new OG version, data reshuffled) → update `expected.findings.json` body + commit.
   - **ogexplain rule changed** (your PR) → update `expected.findings.json` body + commit (this is the regression test catching your change, which is the point).

## OG-specific limitations (must be respected)

Mark these explicitly in `case.toml` / `expected.findings.json`:

| Limitation | Affected rules | Action |
|------------|----------------|--------|
| `DELETE STATISTICS` not supported by OG | STATS-001, EST-001, EST-004 (some cases) | `[side_effects] requires_delete_stats = true`, `[verification] skip_live_reason = "OG does not support DELETE STATISTICS"` |
| Single-node centralized OG | DIST-001, SKEW-001, NET-001 | `[verification] weak_signal = true`, document in case README |
| CStore vector nodes absent on row-mode clusters | VEC-001 (some scenarios) | Use `source = "supplemental"` with a hand-crafted plan, or skip live verification |

## Style guide for `review_notes`

`review_notes` is the single most important quality lever. Write it as if explaining to a reviewer who hasn't read the rule:

**Good:**
```
"Q01 (SELECT * FROM rental LIMIT 10) → Limit→SeqScan on rental, est 16472 rows.
SCAN-001 fires on pure full scan (no Filter). SCAN-004 does NOT (no Filter property).
GEN-001 does NOT (depth=2, threshold=10)."
```

**Bad:**
```
"Tests SCAN-001."                          ← says nothing
"See queries.sql Q01."                     ← forces reviewer to context-switch
"Auto-derived from Q01's root_causes."     ← this is exactly what we're avoiding
```

## Checklist before submitting a new case

- [ ] Directory name matches `<rule-id-lower>-<scenario>` kebab-case convention.
- [ ] `case.toml` validates against the schema (no unknown fields, all required fields present).
- [ ] `expected.findings.json` has `_meta.ogagila_commit` matching current submodule.
- [ ] `review_notes` explains *why* each `must_fire` and `must_not_fire` is declared.
- [ ] At least 2 `anti_findings` entries covering neighbor rules.
- [ ] If `live_db_verify = false`, the `skip_live_reason` is filled and meaningful.
- [ ] Re-read the rule's source file one more time — confirm your expected `detail_must_contain` substrings actually appear in the i18n template.

## Future driver API (not yet implemented — informational)

The harness will eventually expose a single test entry:

```rust,ignore
#[test]
fn regress_all() {
    RegressRunner::crawl("tests/regress")
        .static_mode()        // always
        .live_db_if_feature() // only with --features live-db
        .run();
}
```

Per-case `#[test]` functions are generated at runtime from the directory walk — no boilerplate per case. Until the driver lands, treat the schema in this document as the contract.
