use ogexplain_core::sql::{segment_input, ExtractedContent};

fn read_fixture(name: &str) -> String {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::read_to_string(base.join(name))
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e))
}

#[test]
fn pure_explain_input_single_block() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4)\nTotal runtime: 0.123 ms";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].sql_text.is_none());
    assert!(blocks[0].explain_text.contains("Seq Scan"));
    assert!(blocks[0].explain_text.contains("(cost="));
}

#[test]
fn sql_plus_explain_paired_input() {
    let input = "SELECT * FROM t1;\n\nQUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4)";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].sql_text.as_deref(), Some("SELECT * FROM t1;"));
    assert!(blocks[0].explain_text.contains("Seq Scan"));
}

#[test]
fn multiple_sql_explain_pairs() {
    let input = "SELECT 1;\nQUERY PLAN\nResult  (cost=0.00..0.01 rows=1 width=4)\n(1 row)\n\nSELECT 2;\nQUERY PLAN\nResult  (cost=0.00..0.01 rows=1 width=4)\n(1 row)";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].sql_text.as_deref(), Some("SELECT 1;"));
    assert!(blocks[0].explain_text.contains("Result"));
    assert_eq!(blocks[1].sql_text.as_deref(), Some("SELECT 2;"));
    assert!(blocks[1].explain_text.contains("Result"));
}

#[test]
fn separator_comment_splits_blocks() {
    let input = "SELECT 1;\nQUERY PLAN\nResult  (cost=0.00..0.01 rows=1 width=4)\n(1 row)\n-- ===\nSELECT 2;\nQUERY PLAN\nResult  (cost=0.00..0.01 rows=1 width=4)\n(1 row)";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].sql_text.as_deref(), Some("SELECT 1;"));
    assert_eq!(blocks[1].sql_text.as_deref(), Some("SELECT 2;"));
}

#[test]
fn rows_footer_ends_block() {
    let input = "QUERY PLAN\n----------------------------------------------------\nSeq Scan on t1  (cost=0.00..12.00 rows=100 width=4)\n(5 rows)";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].sql_text.is_none());
    assert!(blocks[0].explain_text.contains("Seq Scan"));
}

#[test]
fn explain_statement_extraction() {
    let input = "explain select * from t1 where id = 1;\nQUERY PLAN\nSeq Scan on t1  (cost=0.00..12.00 rows=1 width=4)";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].sql_text.as_deref(),
        Some("select * from t1 where id = 1;")
    );
}

#[test]
fn prefix_stripping() {
    let input = "--? Seq Scan on t1  (cost=0.00..12.00 rows=100 width=4)";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].explain_text.contains("Seq Scan"));
    assert!(!blocks[0].explain_text.contains("--?"));
}

#[test]
fn extracted_content_from_text() {
    let input = "SELECT * FROM t1;\nQUERY PLAN\nSeq Scan on t1\n\nINSERT INTO t1 VALUES (1);";
    let extracted = ExtractedContent::from_text(input);
    assert!(extracted.has_sql);
    assert_eq!(extracted.sql_lines.len(), 2);
    assert!(extracted.sql_text.contains("SELECT * FROM t1;"));
    assert!(extracted.sql_text.contains("INSERT INTO t1 VALUES (1);"));
}

#[test]
fn empty_input_no_blocks() {
    let blocks = segment_input("");
    assert!(blocks.is_empty());
}

#[test]
fn only_sql_no_explain() {
    let input = "SELECT * FROM t1;\nINSERT INTO t1 VALUES (1);";
    let blocks = segment_input(input);
    assert_eq!(blocks.len(), 2);
    assert!(blocks[0].sql_text.is_some());
    assert!(blocks[0].explain_text.is_empty());
    assert_eq!(blocks[0].sql_text.as_deref(), Some("SELECT * FROM t1;"));
    assert_eq!(blocks[1].sql_text.as_deref(), Some("INSERT INTO t1 VALUES (1);"));
}

#[test]
fn fixture_22_segmentation() {
    let input = read_fixture("22_mixed_sql_file.txt");
    let blocks = segment_input(&input);
    assert_eq!(blocks.len(), 4);

    assert_eq!(
        blocks[0].sql_text.as_deref(),
        Some("create schema explain_fqs;")
    );
    assert!(blocks[0].explain_text.is_empty());

    assert_eq!(
        blocks[1].sql_text.as_deref(),
        Some("set current_schema=explain_fqs;")
    );
    assert!(blocks[1].explain_text.contains("Row Adapter"));
    assert!(blocks[1].explain_text.contains("CStore Scan"));
    assert!(blocks[1].explain_text.contains("Filter: (t1.a < 10)"));

    assert_eq!(
        blocks[2].sql_text.as_deref(),
        Some("insert into t1 values(1);")
    );
    assert!(blocks[2].explain_text.contains("Vector Sort"));
    assert!(blocks[2].explain_text.contains("Vector Sonic Hash Aggregate"));

    assert_eq!(
        blocks[3].sql_text.as_deref(),
        Some("select * from t1 where t1.a = 10;")
    );
    assert!(blocks[3].explain_text.contains("Row Adapter"));
    assert!(blocks[3].explain_text.contains("Filter: (t1.a = 10)"));
}
