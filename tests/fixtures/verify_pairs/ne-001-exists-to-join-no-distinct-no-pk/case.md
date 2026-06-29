# Case ne-001: EXISTS to JOIN without DISTINCT, no PK on users

## Trigger
- Diagnostic: SUBQ-001 (subquery not pulled up)
- Metamorphosis rule: `subquery-to-join`
- Maps via: `crates/ogexplain-cli/src/optimize/mapper.rs` (line 29)

## Schema
- File: `schemas/schema_nopk.json`
- Required PK semantics: No — the schema deliberately omits `primary_key` for `users`. This means `users.id` can contain duplicate values, which is the core reason the rewrite is unsound.

## Expected verify result
- **Status**: NotEquivalent
- **Engine**: qed | verieql
- **Counterexample**:

  **users** (2 rows, same id — allowed because no PK):
  | id | name  |
  |----|-------|
  | 5  | alice |
  | 5  | bob   |

  **orders** (1 row):
  | id | uid | amount |
  |----|-----|--------|
  | 99 | 5   | 50.00  |

  **Original result** (EXISTS): 1 row — `(99)`.
  EXISTS short-circuits on the first matching `users` row; it does not care that there are two `users` rows with id=5.

  **Rewrite result** (JOIN without DISTINCT): 2 rows — `(99), (99)`.
  The single `orders` row joins to both `users` rows, producing a duplicate.

  **Result**: 1 row ≠ 2 rows — NotEquivalent.

## SQL semantics explanation

The original `EXISTS` subquery returns TRUE if at least one matching row exists in `users`. The outer query outputs exactly one row per matching `orders` row, regardless of how many `users` rows match.

The rewritten `JOIN` without `DISTINCT` outputs one row per `(orders, users)` match pair. If `users.id` is not unique (no PK), a single `orders` row can match multiple `users` rows, producing more output rows than the original.

The fix is either: (1) add `DISTINCT` to deduplicate the join output (see eq-001), or (2) keep `EXISTS`. Since `schema_nopk.json` has no PK guarantee, metamorphosis MUST reject this rewrite as NotEquivalent.

## Dependencies
- **Engine**: VeriEQL (bound=3, no-PK schema required)
- **Status**: ✅ active — fixed by metamorphosis PR #38 (QED soundness guard) + PR #41 (VeriEQL EXISTS encoding)
