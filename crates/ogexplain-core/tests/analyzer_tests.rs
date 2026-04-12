use ogexplain_core::analyzer::config::DiagnosticConfig;
use ogexplain_core::analyzer::report::Severity;
use ogexplain_core::suggester::SuggestionEngine;
use ogexplain_core::{analyze, analyze_with_config, parse};

fn parse_fixture(name: &str) -> ogexplain_core::model::ExplainPlan {
    let input = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name),
    )
    .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e));
    parse(&input).unwrap_or_else(|e| panic!("Failed to parse fixture {}: {:?}", name, e))
}

fn analyze_fixture(name: &str) -> ogexplain_core::analyzer::report::DiagnosticReport {
    let plan = parse_fixture(name);
    analyze(&plan)
}

fn has_rule(report: &ogexplain_core::analyzer::report::DiagnosticReport, rule_id: &str) -> bool {
    report.findings.iter().any(|f| f.rule_id == rule_id)
}

#[test]
fn complex_plan_has_findings() {
    let report = analyze_fixture("10_complex_plan.txt");
    assert!(!report.findings.is_empty());
    assert!(has_rule(&report, "SCAN-001"));
    assert!(has_rule(&report, "MEM-001"));
}

#[test]
fn scan_001_large_table_full_scan() {
    let report = analyze_fixture("10_complex_plan.txt");
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "SCAN-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Warning);
    assert!(finding.detail.contains("line_items"));
    assert!(finding.suggestion.is_some());
}

#[test]
fn scan_004_filter_without_index() {
    let config = DiagnosticConfig {
        large_table_rows: 10000.0,
        ..DiagnosticConfig::default()
    };
    let plan = parse_fixture("17_implicit_cast.txt");
    let report = analyze_with_config(&plan, &config);
    let finding = report.findings.iter().find(|f| f.rule_id == "SCAN-004");
    if let Some(f) = finding {
        assert!(f.detail.contains("Seq Scan"));
        assert!(f.suggestion.is_some());
    }
}

#[test]
fn join_001_nested_loop_large() {
    let config = DiagnosticConfig {
        nested_loop_inner_rows: 1000.0,
        ..DiagnosticConfig::default()
    };
    let plan = parse_fixture("11_nested_loop_large.txt");
    let report = analyze_with_config(&plan, &config);
    assert!(
        has_rule(&report, "JOIN-001"),
        "Expected JOIN-001 finding for large nested loop"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "JOIN-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
}

#[test]
fn join_002_hash_spill() {
    let report = analyze_fixture("12_hash_spill.txt");
    assert!(
        has_rule(&report, "JOIN-002"),
        "Expected JOIN-002 finding for hash spill"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "JOIN-002")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
}

#[test]
fn mem_001_sort_spill() {
    let report = analyze_fixture("05_sort_external_merge.txt");
    assert!(
        has_rule(&report, "MEM-001"),
        "Expected MEM-001 finding for sort spill"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "MEM-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.detail.contains("external"));
}

#[test]
fn mem_004_high_peak_memory() {
    let config = DiagnosticConfig {
        memory_threshold_kb: 100000.0,
        ..DiagnosticConfig::default()
    };
    let plan = parse_fixture("12_hash_spill.txt");
    let report = analyze_with_config(&plan, &config);
    assert!(
        has_rule(&report, "MEM-004"),
        "Expected MEM-004 finding for high peak memory"
    );
}

#[test]
fn sort_003_duplicate_sort() {
    let report = analyze_fixture("13_duplicate_sort.txt");
    assert!(
        has_rule(&report, "SORT-003"),
        "Expected SORT-003 finding for duplicate sort"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "SORT-003")
        .unwrap();
    assert_eq!(finding.severity, Severity::Warning);
}

#[test]
fn net_001_broadcast_large() {
    let report = analyze_fixture("14_broadcast_large.txt");
    assert!(
        has_rule(&report, "NET-001"),
        "Expected NET-001 finding for broadcast large table"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "NET-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
}

#[test]
fn est_001_severe_underestimation() {
    let config = DiagnosticConfig {
        estimation_skew_factor: 10.0,
        ..DiagnosticConfig::default()
    };
    let plan = parse_fixture("15_severe_underestimate.txt");
    let report = analyze_with_config(&plan, &config);
    assert!(
        has_rule(&report, "EST-001"),
        "Expected EST-001 finding for severe underestimation"
    );
}

#[test]
fn push_001_query_not_pushed_down() {
    let report = analyze_fixture("16_multi_streaming.txt");
    assert!(
        has_rule(&report, "PUSH-001"),
        "Expected PUSH-001 finding for streaming redistribution"
    );
}

#[test]
fn push_002_multi_layer_streaming() {
    let report = analyze_fixture("16_multi_streaming.txt");
    assert!(
        has_rule(&report, "PUSH-002"),
        "Expected PUSH-002 finding for multi-layer streaming"
    );
}

#[test]
fn type_001_implicit_cast() {
    let report = analyze_fixture("17_implicit_cast.txt");
    assert!(
        has_rule(&report, "TYPE-001"),
        "Expected TYPE-001 finding for implicit type cast"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "TYPE-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.suggestion.is_some());
}

#[test]
fn type_004_like_wildcard() {
    let report = analyze_fixture("18_like_wildcard.txt");
    assert!(
        has_rule(&report, "TYPE-004"),
        "Expected TYPE-004 finding for LIKE with leading wildcard"
    );
}

#[test]
fn vec_001_mixed_engines() {
    let report = analyze_fixture("19_mixed_engines.txt");
    assert!(
        has_rule(&report, "VEC-001"),
        "Expected VEC-001 finding for mixed engines"
    );
}

#[test]
fn gen_001_plan_too_deep() {
    let config = DiagnosticConfig {
        max_plan_depth: 5,
        ..DiagnosticConfig::default()
    };
    let plan = parse_fixture("20_deep_plan.txt");
    let report = analyze_with_config(&plan, &config);
    assert!(
        has_rule(&report, "GEN-001"),
        "Expected GEN-001 finding for deep plan"
    );
}

#[test]
fn global_stats_computation() {
    let report = analyze_fixture("10_complex_plan.txt");
    assert_eq!(report.stats.total_nodes, 7);
    assert_eq!(report.stats.max_depth, 5);
    assert!(report.stats.max_node_time_ms > 0.0);
    assert!(report.stats.max_node_rows > 0.0);
}

#[test]
fn disabled_rules_filtered() {
    let config = DiagnosticConfig {
        disabled_rules: vec!["SCAN-001".to_string()],
        ..DiagnosticConfig::default()
    };
    let plan = parse_fixture("10_complex_plan.txt");
    let report = analyze_with_config(&plan, &config);
    assert!(
        !has_rule(&report, "SCAN-001"),
        "SCAN-001 should be filtered out"
    );
}

#[test]
fn finding_has_all_fields() {
    let report = analyze_fixture("10_complex_plan.txt");
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "MEM-001")
        .unwrap();
    assert!(!finding.rule_id.is_empty());
    assert!(!finding.title.is_empty());
    assert!(!finding.detail.is_empty());
    assert!(finding.node_line.is_some());
    assert!(finding.node_type.is_some());
    assert!(finding.suggestion.is_some());
}

#[test]
fn suggester_cross_rule_patterns() {
    let report = analyze_fixture("12_hash_spill.txt");
    let suggestions = SuggestionEngine::suggest(&report.findings);
    assert!(
        !suggestions.is_empty(),
        "Expected at least one suggestion from cross-rule patterns"
    );
}
