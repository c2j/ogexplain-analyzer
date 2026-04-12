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
