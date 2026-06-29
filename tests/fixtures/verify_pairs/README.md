# Verify Pair Fixtures

SQL pair fixtures for `metamorphosis verify` regression testing (Issue #41).

## Directory layout

- `schemas/` — schema JSON files (proposed PK-aware format — see "Schema protocol" below)
- `eq-XXX-*/` — Equivalent pairs (rewrite should be accepted)
- `ne-XXX-*/` — NotEquivalent pairs (rewrite should be rejected, counterexample expected)

Each pair directory contains:
- `original.sql` — source SQL
- `rewritten.sql` — candidate rewrite
- `case.md` — case documentation (trigger, schema, expected result, counterexample)

## Schema protocol (proposed PK extension)

Current metamorphosis schema JSON is flat `{table: {col: type}}`. We propose a
backward-compatible extension that adds `primary_key` info per table:

```json
{
  "users": {
    "columns": {"id": "INT", "name": "VARCHAR(100)"},
    "primary_key": ["id"]
  }
}
```

Tables without the `primary_key` field are assumed to allow duplicates.
This is required for proving equivalence of `EXISTS → DISTINCT JOIN` rewrites
(see eq-001, eq-002).

**Status**: proposed in ogexplain, pending metamorphosis PR (push for #36 fix).

## Fixture naming convention

Each pair directory follows the pattern:

```
<eq|ne>-NNN-<short-description>/
├── original.sql         # Source SQL — the query before rewriting
├── rewritten.sql        # Target SQL — the candidate rewrite to verify
└── case.md              # Case documentation
```

The `eq/ne` prefix encodes the expected metamorphosis verify result:
- `eq-*` — Transformation should preserve semantics (Equivalent)
- `ne-*` — Transformation changes semantics (NotEquivalent)

## SQL file header convention

Every `.sql` file begins with a SQL comment header:

```sql
-- Case: eq-001-exists-to-distinct-join
-- Role: original
-- Trigger diagnostic: SUBQ-001 (subquery not pulled up)
-- Schema required: schemas/schema_pk.json
-- Expected verify result: Equivalent
```

Fields:
- `Case`: directory name, unique identifier
- `Role`: one of `original`, `rewritten`, or a variant note in parentheses
- `Trigger diagnostic`: ogexplain diagnostic rule ID that would produce this rewrite
- `Schema required`: relative path to schema JSON
- `Expected verify result`: Equivalent or NotEquivalent

## Dependency matrix

### Via JSON schemas (schemas/*.json — PK-aware format, metamorphosis PR #38 / issue #39)

| Case | Engine | Schema | Status |
|------|--------|--------|--------|
| eq-001 | QED | schemas/schema_pk.json | ✅ active |
| eq-002 | QED | schemas/schema_pk.json | ✅ active |
| eq-003 | QED | schemas/schema_pk.json | ✅ active |
| eq-004 | QED | schemas/schema_pk.json | ✅ active |
| ne-001 | VeriEQL (bound=3) | schemas/schema_nopk.json | ✅ active |
| ne-002 | VeriEQL (bound=3) | schemas/schema_nopk.json | ✅ active |
| ne-003 | QED | schemas/schema_pk.json | ✅ active |

### Via DDL schemas (ddl_schemas/*/ — alternative for environments without JSON PK support)

| Case | Engine | Schema dir | Status |
|------|--------|------------|--------|
| eq-001 | QED | ddl_schemas/schema_pk | ✅ active |
| eq-002 | QED | ddl_schemas/schema_pk | ✅ active |
| eq-003 | QED | ddl_schemas/schema_pk | ✅ active |
| eq-004 | QED | ddl_schemas/schema_pk | ✅ active |
| ne-001 | VeriEQL (bound=3) | ddl_schemas/schema_nopk | ✅ active |
| ne-002 | VeriEQL | ddl_schemas/schema_nopk | ⏳ ignored |
| ne-003 | QED | ddl_schemas/schema_pk | ✅ active |

## Usage

Tests in `tests/verify_e2e.rs` use JSON schemas via [`SchemaSource::Json`].
DDL schemas are supported via [`SchemaSource::SqlDir`] as an alternative.
Cases marked "ignored" should be tagged `#[ignore = "..."]` until the
dependency merges.

## Creating new fixtures

1. Choose the expected result: `eq-NNN/` or `ne-NNN/`
2. Write `original.sql`, `rewritten.sql`, and `case.md` following the templates
3. Reference an existing `schemas/schema_*.json` or create a new schema if needed
4. Update the dependency matrix in this README
5. Register the new pair in the E2E test scaffold (separate task)
