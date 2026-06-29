# Case ne-002: IN subquery to JOIN without DISTINCT, no PK on users

## Trigger
- Diagnostic: REW-001 (large IN list not rewritten)
- Metamorphosis rule: `subquery-to-join`
- Maps via: `crates/ogexplain-cli/src/optimize/mapper.rs` (line 29)

## Schema
- File: `schemas/schema_nopk.json`
- Required PK semantics: No — same reasoning as ne-001. The `users` table has no PK, so `users.id` can have duplicates, making the unsound JOIN rewrite produce extra rows.

## Expected verify result
- **Status**: NotEquivalent
- **Engine**: qed | verieql
- **Counterexample**: Identical to ne-001.

  **users** (2 rows, same id):
  | id | name  |
  |----|-------|
  | 5  | alice |
  | 5  | bob   |

  **orders** (1 row):
  | id | uid | amount |
  |----|-----|--------|
  | 99 | 5   | 50.00  |

  **Original result** (IN subquery): 1 row — `(99)`.
  `IN (SELECT id FROM users)` checks set membership. The set `{5}` has one distinct value, so `o.uid=5` is a member. Only one output row.

  **Rewrite result** (JOIN without DISTINCT): 2 rows — `(99), (99)`.
  Same row-multiplication as ne-001.

## SQL semantics explanation

`IN (subquery)` is semantically equivalent to `= ANY (subquery)` — it evaluates to TRUE if the outer expression matches at least one value returned by the subquery. Like `EXISTS`, it does not multiply rows when multiple inner rows match. The join rewrite without `DISTINCT` breaks this semantic contract when duplicates exist in the inner table.

As with ne-001, the correct sound rewrite requires either a PK guarantee (eq-001/eq-002 path) or an explicit `DISTINCT`.

## Dependencies
- **Engine**: VeriEQL (bound=3, no-PK schema required)
- **Status**: ✅ active — fixed by metamorphosis PR #41 (VeriEQL InSubquery encoding)
