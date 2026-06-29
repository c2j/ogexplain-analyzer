# Case eq-001: EXISTS to DISTINCT JOIN

## Trigger
- Diagnostic: SUBQ-001 (subquery not pulled up)
- Metamorphosis rule: `subquery-to-join`
- Maps via: `crates/ogexplain-cli/src/optimize/mapper.rs` (line 29)

## Schema
- File: `schemas/schema_pk.json`
- Required PK semantics: Yes — `users.id` must be a primary key to guarantee that each `orders.uid` value matches at most one row in `users`. Without the PK guarantee, the JOIN could multiply rows and the `DISTINCT` step may not be sufficient to restore cardinality (see ne-001 for the counterexample).

## Expected verify result
- **Status**: Equivalent
- **Engine**: qed (with DISTINCT encoding) or verieql (JOIN semantics)
- **Counterexample**: None — the rewrite is sound given the PK guarantee.

## SQL semantics explanation

The original query uses an `EXISTS` subquery to check whether each `orders` row has at least one matching `users` row. It returns one row per matching `orders.id` — never more, never less. The presence of duplicate or additional `users` rows does not affect the result because `EXISTS` short-circuits on the first match.

The rewritten query replaces the subquery with an explicit `JOIN` followed by `SELECT DISTINCT o.id`. The `JOIN` alone would multiply rows if `u.id` had duplicates (see ne-001). However, under `schema_pk.json`, `users.id` is declared as a primary key, meaning every value in `u.id` is unique. Therefore the JOIN preserves the cardinality of `orders`: each `orders` row matches exactly zero or one `users` rows. The `DISTINCT` is technically redundant under the PK guarantee but harmless, and makes the equivalence provable even without PK-aware reasoning in the verifier.

Result: both queries produce exactly the set of `orders.id` values for which a matching `users` row exists.

## Dependencies
- **Engine**: QED (PK-aware schema required)
- **Status**: ✅ active — fixed by metamorphosis PR #38 (QED soundness + JOIN decorrelation)
