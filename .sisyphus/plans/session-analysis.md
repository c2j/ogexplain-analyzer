# Session-Level Cross-Plan Analysis

## Status: Implemented (P0 complete)

## Why

Current `ogexplain-analyzer` operates exclusively per-plan: `parse()` → `analyze()` → `suggest()`. For stored procedures generating multiple auto_explain NOTICE entries, each entry is analyzed independently. The tool cannot answer:

- "Which SQL in this procedure is the bottleneck?"
- "This cursor loop runs a FTS — what's the cumulative cost across 1000 iterations?"
- "Why did step 4 suddenly take 50x longer than step 3?"

The spec's Phase 3 (`.sisyphus/plans/ogexplain-analyzer-spec.md:1642-1648`) lists "计划对比" and "历史趋势分析" as optional — this plan defines the minimum viable first step.

## Scope (P0 — Minimum Viable)

Three new analysis primitives, all operating on `Vec<(String, ExplainPlan)>` from `parse_multi()`:

1. **SQL template fingerprinting + grouping** — normalize SQL/plan structure, group repeated queries, count occurrences
2. **Serial bottleneck ranking** — compute per-step time contribution, flag outliers (Z-score or >50% threshold)
3. **Loop hotspot detection** — per-template cumulative cost, degradation detection (max/avg ratio), aggregate spill/buffer stats

### Out of Scope (P1+)

- Historical baseline comparison (Phase 3.4 — needs persistence)
- Lock-wait inference (needs pg_stat_activity cross-reference)
- Auto-explain parameter self-tuning
- PL vs SQL cost separation
- Causal chain detection

## Architecture

```
crates/ogexplain-core/src/session/          ← NEW module
├── mod.rs                                   ← pub fn analyze_session()
├── types.rs                                 ← SessionAnalysis, TemplateGroup, SerialBottleneck
├── fingerprint.rs                           ← SQL normalization + plan-structure hash
├── aggregator.rs                            ← template grouping + cumulative stats
└── bottleneck.rs                            ← ranking + hotspot detection
```

### Integration point

```rust
// New public API (no breaking changes)
pub fn analyze_session(
    entries: &[(String, ExplainPlan)],
    config: &DiagnosticConfig,
) -> SessionAnalysis;

// Typical call chain for auto_explain log:
//   let blocks = parse_multi(&log_text)?;
//   let entries: Vec<_> = blocks.iter().map(|p| (extract_query_text(p), p.clone())).collect();
//   let session = analyze_session(&entries, &DiagnosticConfig::default());
//   // session.template_groups → loop hotspots
//   // session.serial_bottlenecks → serial bottlenecks
```

### Relationship to existing code

- **Reuses**: `parse_multi()` for input extraction, `analyze()` for per-plan diagnostics, `SummaryRow` for per-plan metrics
- **Extends**: `converge::MetricsSnapshot` pattern (snapshot + comparison) is reused for degradation detection
- **Does NOT modify**: `DiagnosticEngine`, `SuggestionEngine`, parser, model types
- **New rule prefix**: `SESS-*` (session-level rules) — distinct from existing per-plan rules

## Data Model

### SessionAnalysis

```rust
pub struct SessionAnalysis {
    pub total_entries: usize,
    pub total_time_ms: f64,
    pub serial_bottlenecks: Vec<SerialBottleneck>,
    pub template_groups: Vec<TemplateGroup>,   // sorted by cum_time desc
}

pub struct SerialBottleneck {
    pub step_index: usize,                     // sequential position
    pub query_text: String,
    pub runtime_ms: f64,
    pub contribution_pct: f64,
    pub bottleneck_kind: BottleneckKind,       // Primary (>50%) | Secondary (>2σ) | None
    pub diagnostic: DiagnosticReport,
}

pub struct TemplateGroup {
    pub fingerprint: u64,
    pub normalized_sql: String,
    pub count: usize,                          // ≈ loop iterations
    pub cum_time_ms: f64,
    pub avg_time_ms: f64,
    pub min_time_ms: f64,
    pub max_time_ms: f64,
    pub cum_spill_kb: f64,
    pub cum_buffer_read: i64,
    pub degradation_ratio: f64,                // max/avg; >3 suggests degradation
    pub root_op: String,
    pub diagnostic: DiagnosticReport,          // analyzed once, applied to all
}

pub enum BottleneckKind {
    Primary,
    Secondary,
    None,
}
```

## Algorithms

### 1. Fingerprint

Two approaches, chosen by configuration:

- **Plan-structure hash** (default): `hash(root.node_type + children[].node_type + root.relation + filter_columns)` — fast, no external parser needed
- **SQL normalization** (opt-in): use `ogsql-parser` to replace literals with `?` — higher accuracy, more deps

```rust
fn plan_fingerprint(plan: &ExplainPlan) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    plan.root.node_type.to_string().hash(&mut hasher);
    for child in &plan.root.children {
        child.node_type.to_string().hash(&mut hasher);
    }
    plan.root.relation.hash(&mut hasher);
    // Hash filter/sort key columns from properties
    for prop in &plan.root.properties {
        if matches!(prop.label.as_str(), "Filter" | "Sort Key" | "Index Cond") {
            prop.value.hash(&mut hasher);
        }
    }
    hasher.finish()
}
```

### 2. Aggregation

```rust
fn group_by_template(entries: &[(String, ExplainPlan, DiagnosticReport)]) -> Vec<TemplateGroup> {
    let mut groups: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, (_, plan, _)) in entries.iter().enumerate() {
        let fp = plan_fingerprint(plan);
        groups.entry(fp).or_default().push(i);
    }
    
    groups.into_iter()
        .filter(|(_, indices)| indices.len() > 1) // only repeated queries
        .map(|(fp, indices)| {
            let runtimes: Vec<f64> = indices.iter()
                .map(|&i| extract_runtime(&entries[i]))
                .collect();
            TemplateGroup {
                fingerprint: fp,
                count: indices.len(),
                cum_time_ms: runtimes.iter().sum(),
                avg_time_ms: runtimes.iter().sum::<f64>() / indices.len() as f64,
                min_time_ms: runtimes.iter().cloned().fold(f64::INFINITY, f64::min),
                max_time_ms: runtimes.iter().cloned().fold(0.0, f64::max),
                degradation_ratio: if runtimes.len() > 1 {
                    runtimes.iter().cloned().fold(0.0, f64::max)
                        / (runtimes.iter().sum::<f64>() / runtimes.len() as f64)
                } else { 1.0 },
                // ... cum_spill, cum_buffer from SummaryRow aggregation
                diagnostic: entries[indices[0]].2.clone(),
                // ...
            }
        })
        .sorted_by(|a, b| b.cum_time_ms.partial_cmp(&a.cum_time_ms).unwrap())
        .collect()
}
```

### 3. Bottleneck Detection

```rust
fn detect_serial_bottlenecks(entries: &[Entry]) -> Vec<SerialBottleneck> {
    let total = entries.iter().map(|e| e.runtime_ms).sum::<f64>();
    let mean = total / entries.len() as f64;
    let variance = entries.iter()
        .map(|e| (e.runtime_ms - mean).powi(2))
        .sum::<f64>() / entries.len() as f64;
    let stddev = variance.sqrt();

    entries.iter().enumerate().map(|(i, e)| {
        let contribution = e.runtime_ms / total * 100.0;
        let kind = if contribution > 50.0 {
            BottleneckKind::Primary
        } else if e.runtime_ms > mean + 2.0 * stddev {
            BottleneckKind::Secondary
        } else {
            BottleneckKind::None
        };
        SerialBottleneck {
            step_index: i,
            query_text: e.query.clone(),
            runtime_ms: e.runtime_ms,
            contribution_pct: contribution,
            bottleneck_kind: kind,
            diagnostic: e.report.clone(),
        }
    }).collect()
}
```

## Test Strategy

### Unit tests (fingerprint.rs)

- Same plan → same fingerprint
- Different filter columns → different fingerprints
- Different child node types → different fingerprints
- Function Scan vs Seq Scan → different fingerprints

### Integration tests (using existing parser_tests fixtures)

- Single-entry session → 0 template groups, 0 bottlenecks
- Two identical plans → 1 template group, count=2
- `auto_explain_proc_internal_sql_leaked` fixture (parser_tests.rs:582) → 2 entries, mixed template groups
- Synthetic 5-entry session with one dominating entry → Primary bottleneck detected
- Synthetic loop scenario (same SQL × 100) → correct cumulative cost, degradation detection triggers

### Regression

- All existing 578+ tests pass unchanged (no modifications to existing modules)

## CLI Integration (P1)

```bash
ogexplain analyze --multi --session auto_explain_log.txt
```

Output format: grouped by template with cumulative stats, then serial bottleneck ranking.

## Implementation Order

| Step | Module | Est. lines | Depends on |
|------|--------|-----------|------------|
| 1 | `session/types.rs` | ~80 | nothing |
| 2 | `session/fingerprint.rs` | ~60 | types |
| 3 | `session/aggregator.rs` | ~100 | fingerprint, types |
| 4 | `session/bottleneck.rs` | ~80 | types, aggregator |
| 5 | `session/mod.rs` | ~50 | all above |
| 6 | Unit tests | ~150 | all above |
| 7 | Integration tests | ~200 | all above + existing fixtures |

Total: ~720 lines of production + test code. No external dependencies beyond existing `std::collections::HashMap` and `itertools` (already in workspace).

## Open Questions

1. Should `analyze_session()` eagerly call `analyze()` on EVERY plan, or lazily on first access per template group? (Eager is simpler, lazy saves work for high-count groups.)
2. SQL normalization via `ogsql-parser` vs plan-structure hash as default? (Plan-structure hash is simpler and already proven in optimizer's `sql_history` HashSet.)
3. Should the serial bottleneck algorithm use absolute time or relative contribution? (Both — contribution% for ranking, Z-score for flagging.)
