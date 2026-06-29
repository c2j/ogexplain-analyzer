# Case eq-003: Add explicit cast

## Trigger
- Diagnostic: TYPE-001 (implicit type coercion)
- Metamorphosis rule: `add-explicit-cast`
- Maps via: `crates/ogexplain-cli/src/optimize/mapper.rs` (line 33)

## Schema
- File: `schemas/schema_pk.json`
- Required PK semantics: No — this rewrite does not depend on uniqueness. The schema is included for convention only; any schema with an `orders` table would work.

## Expected verify result
- **Status**: Equivalent
- **Engine**: qed | verieql
- **Counterexample**: None — this is a safe syntactic rewrite.

## SQL semantics explanation

The original query adds `0` to `amount` before comparing to `100`. In many SQL engines this pattern masks an implicit type coercion: `amount` is `NUMERIC(10,2)` and `100` is an integer literal, so the `+ 0` coerces both operands to a common type. The `WHERE amount = 100` in the rewritten query is semantically identical: the database engine implicitly casts `100` to `NUMERIC(10,2)` in the comparison. The `+ 0` is a no-op that adds zero to every row — it does not change the value of `amount`.

Equivalence holds because `amount + 0 = 100` evaluates to TRUE for exactly the same rows as `amount = 100`, for any row in any possible database state. This is a purely algebraic identity.

## Dependencies
- **Engine**: QED
- **Status**: ✅ active (trivial identity rewrite, not affected by soundness bugs)
