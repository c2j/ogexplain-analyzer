# Case eq-004: Trivial self-equivalence (smoke test)

## Trigger
- Diagnostic: none — this is not driven by any ogexplain diagnostic rule.
- Metamorphosis rule: none — the rewrite is identical to the original.
- Maps via: N/A (not triggered by mapper.rs)

## Schema
- File: `schemas/schema_pk.json`
- Required PK semantics: No — any schema with a `users` table works. The choice of schema is purely conventional.

## Expected verify result
- **Status**: Equivalent
- **Engine**: qed | verieql
- **Counterexample**: None — any SQL engine must return the same result for identical queries.

## SQL semantics explanation

The original and rewritten SQL are byte-for-byte identical. By definition, any deterministic SQL engine evaluates the same query text to the same result on the same database state. This case serves as a smoke test for the metamorphosis `verify` command infrastructure itself: if this pair fails with NotEquivalent, the tooling is broken, not the equivalence proof.

## Smoke test value

This case is intentionally trivial. It should pass even when all other eq-* cases are blocked by unmet metamorphosis dependencies (distinct encoding, JOIN column resolution). It validates that:
1. The `verify` command accepts the expected file layout
2. The schema JSON parsing (new PK-aware format) works end-to-end
3. The verifier's self-equivalence proof is functional

Use this as the `#[ignore]`-free test case in the E2E test scaffold. All other cases should be `#[ignore = "blocked by metamorphosis#XX"]` until the corresponding fixes land.

## Dependencies
- **Engine**: QED
- **Status**: ✅ active (self-equivalence — always Equivalent regardless of engine soundness)
