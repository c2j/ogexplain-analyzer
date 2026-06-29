# Case ne-003: EXISTS to JOIN with PK but projection differs

## Trigger
- Diagnostic: SUBQ-001 (subquery not pulled up)
- Metamorphosis rule: `subquery-to-join`
- Maps via: `crates/ogexplain-cli/src/optimize/mapper.rs` (line 29)

## Schema
- File: `schemas/schema_mixed.json`
- Required PK semantics: Yes, but irrelevant for this case. Both `users.id` and `orders.id` have PKs. The NotEquivalent result is driven by the column projection difference, not by PK semantics.

## Expected verify result
- **Status**: NotEquivalent
- **Engine**: qed | verieql
- **Counterexample**:

  **orders** (any non-empty row):
  | id | uid | amount |
  |----|-----|--------|
  | 1  | 5   | 50.00  |

  **users**:
  | id | name |
  |----|------|
  | 5  | bob  |

  **Original result**: 1 row — `(1, 50.00)` (2 columns).
  **Rewrite result**: 1 row — `(1)` (1 column).

  Column count mismatch: 2 ≠ 1. Even though both return the same number of rows, the output schemas differ. The rewrite drops `o.amount` from the projection.

## SQL semantics explanation

This case tests the most basic form of non-equivalence: the SELECT lists differ. The original returns both `o.id` and `o.amount`; the rewritten version returns only `o.id`. Any equivalence verifier MUST detect this structural mismatch regardless of its sophistication.

This case is special because it does NOT depend on metamorphosis #36 or #37. Even the current (soundness-bugged) QED should reject this pair because the column count is a structural property visible without any semantic reasoning. It serves as a positive control for the NotEquivalent path — if this case unexpectedly reports Equivalent, the entire verification toolchain is broken.

## Dependencies
- **Engine**: QED
- **Status**: ✅ active (structural column-count mismatch — caught regardless of engine soundness)
