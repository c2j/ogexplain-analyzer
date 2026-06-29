//! E2E regression tests for metamorphosis verify (library API).
//!
//! Each test loads a SQL pair from `tests/fixtures/verify_pairs/`, builds a
//! [`RichSchema`](metamorphosis_qed::schema::RichSchema) from a PK-aware JSON
//! schema file, and calls [`verify_qed`] directly — no subprocess or binary.
//!
//! JSON schemas under `tests/fixtures/verify_pairs/schemas/` use the PK-aware
//! format (`columns` + optional `primary_key`) introduced in metamorphosis
//! PR #38 (commit ed85be8, issue #39).

use std::path::{Path, PathBuf};

use ogexplain_optimizer::verify::VerifyStatus;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Directory containing all verify pair fixtures.
const FIXTURES_ROOT: &str = "tests/fixtures/verify_pairs";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Path to a fixture case directory.
fn case_dir(case_id: &str) -> PathBuf {
    Path::new(FIXTURES_ROOT).join(case_id)
}

/// Path to a schema file under `verify_pairs/schemas/`.
fn schema_path(name: &str) -> PathBuf {
    Path::new(FIXTURES_ROOT)
        .join("schemas")
        .join(format!("{name}.json"))
}

/// Path to a DDL schema directory under verify_pairs/ddl_schemas/.
#[allow(dead_code)]
fn ddl_schema_dir(name: &str) -> PathBuf {
    Path::new(FIXTURES_ROOT).join("ddl_schemas").join(name)
}

/// Read a fixture `.sql` file and strip the leading `-- …` header comment
/// block.  Returns only the SQL body.
fn read_fixture_sql(path: &Path) -> String {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    // Strip lines starting with `--` until we hit the first non-comment,
    // non-empty line.  After that, keep everything (including any inline
    // comments within the SQL body).
    let mut lines = content.lines();
    let mut body_start = 0usize;
    for (i, line) in lines.by_ref().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") || trimmed.is_empty() {
            body_start = i + 1;
        } else {
            break;
        }
    }

    content
        .lines()
        .skip(body_start)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Load a [`RichSchema`](metamorphosis_qed::schema::RichSchema) from a
/// PK-aware JSON schema file.
///
/// The JSON format supports the optional `primary_key` field per table:
///
/// ```json
/// {
///   "users": {
///     "columns": { "id": "INT", "name": "VARCHAR" },
///     "primary_key": ["id"]
///   }
/// }
/// ```
fn load_rich_schema_from_fixture(name: &str) -> metamorphosis_qed::schema::RichSchema {
    use metamorphosis_qed::schema::{ColumnInfo, TableConstraints, TableInfo};
    use std::collections::{HashMap, HashSet};

    #[derive(serde::Deserialize)]
    struct TableSchemaEntry {
        columns: HashMap<String, String>,
        #[serde(default)]
        primary_key: Vec<String>,
    }

    let path = schema_path(name);
    let content =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read schema '{}': {e}", path.display()));

    let entries: HashMap<String, TableSchemaEntry> =
        serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse schema '{}': {e}", path.display()));

    let tables: HashMap<String, TableInfo> = entries
        .into_iter()
        .map(|(table_name, entry)| {
            let pk_set: HashSet<String> = entry.primary_key.iter().map(|s| s.to_lowercase()).collect();

            let columns: Vec<ColumnInfo> = entry
                .columns
                .into_iter()
                .map(|(col_name, data_type)| {
                    let is_pk = pk_set.contains(&col_name.to_lowercase());
                    ColumnInfo {
                        name: col_name.to_lowercase(),
                        data_type,
                        nullable: !is_pk,
                        is_primary_key: is_pk,
                        is_unique: is_pk,
                    }
                })
                .collect();

            (
                table_name,
                TableInfo {
                    columns,
                    constraints: TableConstraints {
                        primary_key: entry.primary_key.iter().map(|s| s.to_lowercase()).collect(),
                        ..Default::default()
                    },
                },
            )
        })
        .collect();

    metamorphosis_qed::schema::RichSchema { tables }
}

// ---------------------------------------------------------------------------
// Active tests (QED — formal proof)
// ---------------------------------------------------------------------------

#[ignore = "QED translation error: column 'o.uid' not found — fixture schema needs update"]
#[test]
fn eq_001_exists_to_distinct_join() {
    let dir = case_dir("eq-001-exists-to-distinct-join");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = load_rich_schema_from_fixture("schema_pk");

    let result = ogexplain_optimizer::verify::verify_qed(&original, &rewritten, &schema, 60)
        .expect("verify should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "EXISTS→DISTINCT JOIN under PK must verify as Equivalent; got status={:?}",
        result.status,
    );
}

#[ignore = "QED translation error: column 'o.uid' not found — fixture schema needs update"]
#[test]
fn eq_002_in_subquery_to_distinct_join() {
    let dir = case_dir("eq-002-in-subquery-to-distinct-join");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = load_rich_schema_from_fixture("schema_pk");

    let result = ogexplain_optimizer::verify::verify_qed(&original, &rewritten, &schema, 60)
        .expect("verify should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "IN→DISTINCT JOIN under PK must verify as Equivalent; got status={:?}",
        result.status,
    );
}

#[test]
fn eq_003_add_explicit_cast() {
    let dir = case_dir("eq-003-add-explicit-cast");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = load_rich_schema_from_fixture("schema_pk");

    let result = ogexplain_optimizer::verify::verify_qed(&original, &rewritten, &schema, 60)
        .expect("verify should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "add-explicit-cast rewrite must verify as Equivalent; got status={:?}",
        result.status,
    );
}

#[test]
fn eq_004_trivial_self_equivalence() {
    let dir = case_dir("eq-004-trivial-self-equivalence");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = load_rich_schema_from_fixture("schema_pk");

    let result = ogexplain_optimizer::verify::verify_qed(&original, &rewritten, &schema, 60)
        .expect("verify should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "self-equivalence must verify as Equivalent; got status={:?}",
        result.status,
    );
}

// ---------------------------------------------------------------------------
// VeriEQL tests (bounded model checking) — temporarily ignored
// TODO: migrate to library API when VeriEQL schema conversion is stable
// ---------------------------------------------------------------------------

#[ignore = "TODO: migrate to library API when VeriEQL schema conversion is stable"]
#[test]
fn ne_001_exists_to_join_no_distinct_no_pk() {
    let dir = case_dir("ne-001-exists-to-join-no-distinct-no-pk");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = load_rich_schema_from_fixture("schema_nopk");

    let tables = ogexplain_optimizer::verify::rich_schema_to_verieql(&schema);
    let constraints = serde_json::json!({});

    let result = ogexplain_optimizer::verify::verify_verieql(
        &original,
        &rewritten,
        &tables,
        &constraints,
        3,
    )
    .expect("verify should complete");

    assert!(
        matches!(result.status, VerifyStatus::NotEquivalent { .. }),
        "EXISTS→JOIN (no DISTINCT, no PK) must be NotEquivalent via VeriEQL; got status={:?}",
        result.status,
    );
}

#[ignore = "TODO: migrate to library API when VeriEQL schema conversion is stable"]
#[test]
fn ne_002_in_subquery_to_join_no_distinct_no_pk() {
    let dir = case_dir("ne-002-in-subquery-to-join-no-distinct-no-pk");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = load_rich_schema_from_fixture("schema_nopk");

    let tables = ogexplain_optimizer::verify::rich_schema_to_verieql(&schema);
    let constraints = serde_json::json!({});

    let result = ogexplain_optimizer::verify::verify_verieql(
        &original,
        &rewritten,
        &tables,
        &constraints,
        3,
    )
    .expect("verify should complete");

    assert!(
        matches!(&result.status, VerifyStatus::NotEquivalent { .. }),
        "IN→JOIN (no DISTINCT, no PK) must be NotEquivalent via VeriEQL; got {:?}",
        result.status,
    );
}

// ---------------------------------------------------------------------------
// Not-equivalent via QED
// ---------------------------------------------------------------------------

#[ignore = "QED translation error: column not found — fixture schema needs update"]
#[test]
fn ne_003_column_count_mismatch() {
    let dir = case_dir("ne-003-exists-to-join-pk-but-columns-differ");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = load_rich_schema_from_fixture("schema_pk");

    let result = ogexplain_optimizer::verify::verify_qed(&original, &rewritten, &schema, 60)
        .expect("verify should complete");

    assert!(
        matches!(result.status, VerifyStatus::NotEquivalent { .. }),
        "column count mismatch must be NotEquivalent via QED; got status={:?}",
        result.status,
    );
}
