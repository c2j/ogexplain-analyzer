# Case eq-002: IN subquery to DISTINCT JOIN

## Trigger
- Diagnostic: REW-001 (large IN list not rewritten)
- Metamorphosis rule: `subquery-to-join`
- Maps via: `crates/ogexplain-cli/src/optimize/mapper.rs` (line 29)

## Schema
- File: `schemas/schema_pk.json`
- Required PK semantics: Yes — same reasoning as eq-001. The IN-to-JOIN rewrite is only sound when the inner table's join column is unique.

## Expected verify result
- **Status**: Equivalent
- **Engine**: qed | verieql
- **Counterexample**: None under PK guarantee.

## SQL semantics explanation

The original query filters `orders` rows where `o.uid` appears in the set of `users.id` values. `IN (subquery)` is semantically equivalent to `= ANY (subquery)` — it returns TRUE if at least one match exists. The result is exactly one row per matching `orders.id`, with no duplicates.

The rewritten query uses a `JOIN` with `SELECT DISTINCT o.id`. The `IN` condition becomes the `JOIN` condition `u.id = o.uid`. Since `users.id` is a primary key under `schema_pk.json`, each `u.id` value is unique, so the JOIN does not multiply `orders` rows. The `DISTINCT` is a safety net.

Equivalence holds: both queries return the same set of `orders.id` where a corresponding `users.id` exists.

## Dependencies
- **Engine**: QED (PK-aware schema required)
- **Status**: ✅ active — fixed by metamorphosis PR #38 (QED soundness + QED decorrelation)
