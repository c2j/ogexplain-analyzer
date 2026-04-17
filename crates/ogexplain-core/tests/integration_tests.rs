use insta::assert_yaml_snapshot;
use ogexplain_core::parse;

fn parse_fixture(name: &str) -> ogexplain_core::model::ExplainPlan {
    let input = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
    .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e));
    parse(&input).unwrap_or_else(|e| panic!("Failed to parse fixture {}: {:?}", name, e))
}

#[test]
fn simple_seq_scan() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn index_scan_filter() {
    let plan = parse_fixture("02_index_scan_filter.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn hash_join() {
    let plan = parse_fixture("03_hash_join.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn nested_loop() {
    let plan = parse_fixture("04_nested_loop.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn sort_external_merge() {
    let plan = parse_fixture("05_sort_external_merge.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn streaming_gather() {
    let plan = parse_fixture("06_streaming_gather.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn vector_hash_join() {
    let plan = parse_fixture("07_vector_hash_join.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn cstore_scan() {
    let plan = parse_fixture("08_cstore_scan.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn pretty_mode() {
    let plan = parse_fixture("09_pretty_mode.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn complex_plan() {
    let plan = parse_fixture("10_complex_plan.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn regression_test_raw() {
    let plan = parse_fixture("21_regression_test_raw.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn mixed_sql_file() {
    let plan = parse_fixture("22_mixed_sql_file.txt");
    insta::with_settings!({ sort_maps => true }, {
        assert_yaml_snapshot!(plan);
    });
}

#[test]
fn parse_multi_extracts_multiple_blocks() {
    let input = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/22_mixed_sql_file.txt"),
    )
    .unwrap();
    let plans = ogexplain_core::parse_multi(&input).unwrap();
    assert!(
        plans.len() >= 2,
        "Expected at least 2 EXPLAIN blocks, got {}",
        plans.len()
    );
}

#[test]
fn sql_extraction_from_mixed_input() {
    let input = "\
SELECT /*+ indexscan(diskann_t2 idx_vectors_10d_100) */ id, description
FROM diskann_t2
ORDER BY embedding <-> '[0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5]' LIMIT 3;
                                      QUERY PLAN                                       
---------------------------------------------------------------------------------------
 [Bypass]
 Limit
   ->  Ann Index Scan using idx_vectors_10d_100 on diskann_t2
         Order By: (embedding <-> '[0.5,0.5,0.5,0.5,0.5,0.5,0.5,0.5,0.5,0.5]'::vector)
(4 rows)";

    let extracted = ogexplain_core::sql::ExtractedContent::from_text(input);
    assert!(extracted.has_sql, "Should detect SQL in mixed input");
    assert!(
        extracted.sql_text.contains("SELECT"),
        "SQL text should contain SELECT: {:?}",
        extracted.sql_text
    );
    assert!(
        extracted.sql_text.contains("FROM diskann_t2"),
        "SQL text should contain FROM clause: {:?}",
        extracted.sql_text
    );
    assert!(
        extracted.sql_text.contains("ORDER BY"),
        "SQL text should contain ORDER BY: {:?}",
        extracted.sql_text
    );
    assert!(
        !extracted.sql_text.contains("QUERY PLAN"),
        "SQL text should not contain EXPLAIN output"
    );
}
