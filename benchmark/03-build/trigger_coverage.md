# Trigger Coverage Report

**Generated:** 2026-06-20T14:24:32.313373+00:00
**Source:** ogagila repo + queries_meta.json
**Total cases:** 97

## Per-rule trigger coverage

| Rule | Designed | Actually Triggered | Skipped (not really triggered) | Trigger Rate |
|------|----------|--------------------|--------------------------------|--------------|
| AGG-001 | 2 | 2 | 0 | 100% |
| AGG-002 | 2 | 2 | 0 | 100% |
| DIST-001 | 2 | 0 | 2 | 0% |
| EST-001 | 4 | 0 | 4 | 0% |
| EST-004 | 3 | 0 | 3 | 0% |
| GEN-001 | 2 | 0 | 2 | 0% |
| JOIN-001 | 5 | 3 | 2 | 60% |
| JOIN-002 | 4 | 4 | 0 | 100% |
| MEM-001 | 4 | 3 | 1 | 75% |
| MEM-004 | 3 | 0 | 3 | 0% |
| NET-001 | 3 | 2 | 1 | 67% |
| NONE | 15 | 0 | 15 | 0% |
| PART-001 | 4 | 4 | 0 | 100% |
| PUSH-001 | 3 | 0 | 3 | 0% |
| PUSH-002 | 3 | 2 | 1 | 67% |
| REW-001 | 2 | 2 | 0 | 100% |
| SCAN-001 | 8 | 8 | 0 | 100% |
| SCAN-004 | 6 | 6 | 0 | 100% |
| SKEW-001 | 2 | 2 | 0 | 100% |
| SORT-003 | 3 | 0 | 3 | 0% |
| STATS-001 | 2 | 0 | 2 | 0% |
| SUBQ-001 | 3 | 0 | 3 | 0% |
| SUBQ-006 | 2 | 2 | 0 | 100% |
| TYPE-001 | 4 | 4 | 0 | 100% |
| TYPE-004 | 3 | 3 | 0 | 100% |
| VEC-001 | 3 | 2 | 1 | 67% |

## Notes

- **Skipped** = EXPLAIN shows that the rule's intended condition did NOT actually manifest (e.g. optimizer eliminated implicit cast, LIMIT suppressed cost signal, openGauss skipped side-effect statements like DELETE STATISTICS).
- These cases still have ground truth `is_problematic: true` per the original design intent — but the runtime evidence is weaker. Mark as needing manual review.
- For healthy cases (target_rule = NONE), no trigger expected — they are 'true negatives'.
