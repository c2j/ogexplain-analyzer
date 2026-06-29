#![cfg(feature = "db")]

//! E2E regression tests for `metamorphosis verify` integration.
//!
//! Each test loads a SQL pair from `tests/fixtures/verify_pairs/`, calls
//! [`call_metamorphosis_verify`] with a PK-aware JSON schema via
//! [`SchemaSource::Json`], and asserts the expected [`VerifyStatus`].
//!
//! JSON schemas under `tests/fixtures/verify_pairs/schemas/` use the
//! PK-aware format (`columns` + optional `primary_key`) introduced in
//! metamorphosis PR #38 (commit ed85be8, issue #39).
//!
//! # Skipping when metamorphosis is unavailable
//!
//! Tests gracefully skip (not fail) when the `metamorphosis` binary is
//! not found on `$PATH` or at `$OGEXPLAIN_METAMORPHOSIS`.  This allows
//! the test suite to pass in CI or on machines that don't have
//! metamorphosis installed.

use std::path::{Path, PathBuf};

use ogexplain_cli::optimize::verify::{
    call_metamorphosis_verify, VerifyEngine, VerifyStatus, SchemaSource,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Directory containing all verify pair fixtures.
const FIXTURES_ROOT: &str = "tests/fixtures/verify_pairs";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the path to the metamorphosis binary if available on PATH or via
/// env override.  Returns `None` and prints a skip message if unavailable.
fn metamorphosis_path() -> Option<PathBuf> {
    // Allow override via OGEXPLAIN_METAMORPHOSIS env var
    if let Ok(p) = std::env::var("OGEXPLAIN_METAMORPHOSIS") {
        return Some(PathBuf::from(p));
    }
    // Probe "metamorphosis" on PATH
    let probe = std::process::Command::new("metamorphosis")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match probe {
        Ok(_) => Some(PathBuf::from("metamorphosis")),
        Err(_) => {
            eprintln!(
                "SKIP: metamorphosis binary not found on PATH \
                 (set OGEXPLAIN_METAMORPHOSIS to override)"
            );
            None
        }
    }
}

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

// ---------------------------------------------------------------------------
// Active tests
// ---------------------------------------------------------------------------

#[test]
fn eq_001_exists_to_distinct_join() {
    let Some(metamorphosis) = metamorphosis_path() else { return; };
    let dir = case_dir("eq-001-exists-to-distinct-join");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = schema_path("schema_pk");

    let result = call_metamorphosis_verify(
        &metamorphosis,
        &original,
        &rewritten,
        Some(SchemaSource::Json(&schema)),
        VerifyEngine::Qed,
        2,
        60,
    )
    .expect("verify subprocess should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "EXISTS→DISTINCT JOIN under PK must verify as Equivalent; got status={:?}\nraw={}",
        result.status,
        result.raw_output.as_deref().unwrap_or("(none)")
    );
}

#[test]
fn eq_002_in_subquery_to_distinct_join() {
    let Some(metamorphosis) = metamorphosis_path() else { return; };
    let dir = case_dir("eq-002-in-subquery-to-distinct-join");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = schema_path("schema_pk");

    let result = call_metamorphosis_verify(
        &metamorphosis,
        &original,
        &rewritten,
        Some(SchemaSource::Json(&schema)),
        VerifyEngine::Qed,
        2,
        60,
    )
    .expect("verify subprocess should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "IN→DISTINCT JOIN under PK must verify as Equivalent; got status={:?}\nraw={}",
        result.status,
        result.raw_output.as_deref().unwrap_or("(none)")
    );
}

#[test]
fn eq_003_add_explicit_cast() {
    let Some(metamorphosis) = metamorphosis_path() else { return; };
    let dir = case_dir("eq-003-add-explicit-cast");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = schema_path("schema_pk");

    let result = call_metamorphosis_verify(
        &metamorphosis,
        &original,
        &rewritten,
        Some(SchemaSource::Json(&schema)),
        VerifyEngine::Qed,
        2,
        60,
    )
    .expect("verify subprocess should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "add-explicit-cast rewrite must verify as Equivalent; got status={:?}\nraw={}",
        result.status,
        result.raw_output.as_deref().unwrap_or("(none)")
    );
}

#[test]
fn eq_004_trivial_self_equivalence() {
    let Some(metamorphosis) = metamorphosis_path() else { return; };
    let dir = case_dir("eq-004-trivial-self-equivalence");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = schema_path("schema_pk");

    let result = call_metamorphosis_verify(
        &metamorphosis,
        &original,
        &rewritten,
        Some(SchemaSource::Json(&schema)),
        VerifyEngine::Qed,
        2,
        60,
    )
    .expect("verify subprocess should complete");

    assert_eq!(
        result.status,
        VerifyStatus::Equivalent,
        "self-equivalence must verify as Equivalent; got status={:?}\nraw={}",
        result.status,
        result.raw_output.as_deref().unwrap_or("(none)")
    );
}

#[test]
fn ne_001_exists_to_join_no_distinct_no_pk() {
    let Some(metamorphosis) = metamorphosis_path() else { return; };
    let dir = case_dir("ne-001-exists-to-join-no-distinct-no-pk");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = schema_path("schema_nopk");

    let result = call_metamorphosis_verify(
        &metamorphosis,
        &original,
        &rewritten,
        Some(SchemaSource::Json(&schema)),
        VerifyEngine::VeriEql,
        3,
        60,
    )
    .expect("verify subprocess should complete");

    assert!(
        matches!(result.status, VerifyStatus::NotEquivalent { .. }),
        "EXISTS→JOIN (no DISTINCT, no PK) must be NotEquivalent via VeriEQL; got status={:?}\nraw={}",
        result.status,
        result.raw_output.as_deref().unwrap_or("(none)")
    );
}

#[test]
fn ne_002_in_subquery_to_join_no_distinct_no_pk() {
    let Some(metamorphosis) = metamorphosis_path() else { return; };
    let dir = case_dir("ne-002-in-subquery-to-join-no-distinct-no-pk");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = schema_path("schema_nopk");

    let result = call_metamorphosis_verify(
        &metamorphosis,
        &original,
        &rewritten,
        Some(SchemaSource::Json(&schema)),
        VerifyEngine::VeriEql,
        3,
        60,
    )
    .expect("verify subprocess should complete");

    assert!(
        matches!(&result.status, VerifyStatus::NotEquivalent { .. }),
        "IN→JOIN (no DISTINCT, no PK) must be NotEquivalent via VeriEQL; got {:?}",
        result.status,
    );
}

#[test]
fn ne_003_column_count_mismatch() {
    let Some(metamorphosis) = metamorphosis_path() else { return; };
    let dir = case_dir("ne-003-exists-to-join-pk-but-columns-differ");
    let original = read_fixture_sql(&dir.join("original.sql"));
    let rewritten = read_fixture_sql(&dir.join("rewritten.sql"));
    let schema = schema_path("schema_pk");

    let result = call_metamorphosis_verify(
        &metamorphosis,
        &original,
        &rewritten,
        Some(SchemaSource::Json(&schema)),
        VerifyEngine::Qed,
        2,
        60,
    )
    .expect("verify subprocess should complete");

    assert!(
        matches!(result.status, VerifyStatus::NotEquivalent { .. }),
        "column count mismatch must be NotEquivalent via QED; got status={:?}\nraw={}",
        result.status,
        result.raw_output.as_deref().unwrap_or("(none)")
    );
}
