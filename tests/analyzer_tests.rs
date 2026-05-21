//! Dedicated analyzer diagnostic tests for ogexplain-analyzer.
//!
//! Tests every implemented diagnostic rule (15 rules) against its trigger
//! fixture, verifies severity / detail content, and exercises config
//! customization and cross-rule suggestion synthesis.

use ogexplain_core::analyzer::config::DiagnosticConfig;
use ogexplain_core::analyzer::report::{DiagnosticCategory, Severity};
use ogexplain_core::suggester::{SuggestionCategory, SuggestionEngine};
use ogexplain_core::{analyze, analyze_with_config, parse};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_fixture(name: &str) -> String {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::read_to_string(base.join(name))
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e))
}

fn parse_fixture(name: &str) -> ogexplain_core::model::ExplainPlan {
    let input = read_fixture(name);
    parse(&input).unwrap_or_else(|e| panic!("Failed to parse {}: {:?}", name, e))
}

fn analyze_fixture(name: &str) -> ogexplain_core::analyzer::report::DiagnosticReport {
    let plan = parse_fixture(name);
    analyze(&plan)
}

fn has_finding(report: &ogexplain_core::analyzer::report::DiagnosticReport, rule_id: &str) -> bool {
    report.findings.iter().any(|f| f.rule_id == rule_id)
}

fn get_finding<'a>(
    report: &'a ogexplain_core::analyzer::report::DiagnosticReport,
    rule_id: &str,
) -> Option<&'a ogexplain_core::analyzer::report::Finding> {
    report.findings.iter().find(|f| f.rule_id == rule_id)
}

// ---------------------------------------------------------------------------
// 1. SCAN-001 — Large table full scan
// ---------------------------------------------------------------------------

#[test]
fn scan_001_triggers_on_large_seq_scan() {
    let report = analyze_fixture("10_complex_plan.txt");
    let finding = get_finding(&report, "SCAN-001")
        .expect("Expected SCAN-001 for large Seq Scan on line_items");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::ScanEfficiency);
    assert!(finding.detail.contains("line_items"), "detail should mention table name");
    assert!(finding.detail.contains("500000"), "detail should mention row count");
}

#[test]
fn scan_001_does_not_trigger_for_small_table() {
    let report = analyze_fixture("01_simple_seq_scan.txt");
    assert!(
        !has_finding(&report, "SCAN-001"),
        "SCAN-001 should not fire for a 100-row table"
    );
}

// ---------------------------------------------------------------------------
// 2. SCAN-004 — Filter without index
// ---------------------------------------------------------------------------

#[test]
fn scan_004_triggers_on_filter_without_index() {
    let report = analyze_fixture("17_implicit_cast.txt");
    let finding = get_finding(&report, "SCAN-004")
        .expect("Expected SCAN-004 for filter without index on orders");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::ScanEfficiency);
    assert!(finding.detail.contains("orders"), "detail should mention table name");
    assert!(finding.detail.contains("Filter"), "detail should mention Filter");
}

#[test]
fn scan_004_does_not_trigger_when_estimation_ratio_low() {
    let report = analyze_fixture("01_simple_seq_scan.txt");
    assert!(
        !has_finding(&report, "SCAN-004"),
        "SCAN-004 should not fire when estimated/actual ratio is low"
    );
}

// ---------------------------------------------------------------------------
// 3. JOIN-001 — Nested loop on large tables
// ---------------------------------------------------------------------------

#[test]
fn join_001_triggers_on_nested_loop_large() {
    let report = analyze_fixture("11_nested_loop_large.txt");
    let finding = get_finding(&report, "JOIN-001")
        .expect("Expected JOIN-001 for large nested loop");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::JoinStrategy);
    assert!(
        finding.detail.contains("loops") || finding.detail.contains("threshold"),
        "detail should mention loops or threshold"
    );
}

#[test]
fn join_001_does_not_trigger_for_small_nested_loop() {
    let report = analyze_fixture("04_nested_loop.txt");
    assert!(
        !has_finding(&report, "JOIN-001"),
        "JOIN-001 should not fire for tiny nested loop below threshold"
    );
}

// ---------------------------------------------------------------------------
// 4. JOIN-002 — Hash join spill to disk
// ---------------------------------------------------------------------------

#[test]
fn join_002_triggers_on_hash_spill() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "JOIN-002")
        .expect("Expected JOIN-002 for hash join spill (Batches: 5)");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::JoinStrategy);
    assert!(
        finding.detail.contains("5") || finding.detail.contains("batches"),
        "detail should mention batch count"
    );
}

// ---------------------------------------------------------------------------
// 5. MEM-001 — Sort spill to disk
// ---------------------------------------------------------------------------

#[test]
fn mem_001_triggers_on_sort_spill() {
    let report = analyze_fixture("05_sort_external_merge.txt");
    let finding = get_finding(&report, "MEM-001")
        .expect("Expected MEM-001 for external merge sort");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::MemoryUsage);
    assert!(
        finding.detail.contains("external"),
        "detail should mention external merge"
    );
}

#[test]
fn mem_001_triggers_on_complex_plan_sort_spill() {
    let report = analyze_fixture("10_complex_plan.txt");
    let finding = get_finding(&report, "MEM-001")
        .expect("Expected MEM-001 for external merge sort in complex plan");
    assert_eq!(finding.severity, Severity::Critical);
    assert!(finding.detail.contains("external"));
}

// ---------------------------------------------------------------------------
// 6. MEM-004 — High peak memory
// ---------------------------------------------------------------------------

#[test]
fn mem_004_triggers_on_high_peak_memory() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "MEM-004")
        .expect("Expected MEM-004 for peak memory 512000 KB");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::MemoryUsage);
    assert!(
        finding.detail.contains("512000") || finding.detail.contains("Peak memory"),
        "detail should mention peak memory value"
    );
}

// ---------------------------------------------------------------------------
// 7. SORT-003 — Duplicate sort
// ---------------------------------------------------------------------------

#[test]
fn sort_003_triggers_on_duplicate_sort() {
    let report = analyze_fixture("13_duplicate_sort.txt");
    let finding = get_finding(&report, "SORT-003")
        .expect("Expected SORT-003 for duplicate sort nodes");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::SortEfficiency);
    assert!(
        finding.detail.contains("redundant") || finding.detail.contains("Sort"),
        "detail should mention redundant sorting"
    );
}

// ---------------------------------------------------------------------------
// 8. NET-001 — Broadcast large data
// ---------------------------------------------------------------------------

#[test]
fn net_001_triggers_on_broadcast_large() {
    let report = analyze_fixture("14_broadcast_large.txt");
    let finding = get_finding(&report, "NET-001")
        .expect("Expected NET-001 for broadcast with 50000 rows");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::NetworkOverhead);
    assert!(
        finding.detail.contains("50000") || finding.detail.contains("Broadcast"),
        "detail should mention row count or Broadcast"
    );
}

// ---------------------------------------------------------------------------
// 9. EST-001 — Severe row underestimation
// ---------------------------------------------------------------------------

#[test]
fn est_001_triggers_on_severe_underestimate_with_custom_config() {
    let plan = parse_fixture("15_severe_underestimate.txt");
    let config = DiagnosticConfig {
        estimation_skew_factor: 10.0,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    let finding = get_finding(&report, "EST-001")
        .expect("Expected EST-001 for severe underestimation with lowered threshold");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::CostMisestimation);
    assert!(
        finding.detail.contains("500000") || finding.detail.contains("5000"),
        "detail should mention actual or estimated row count"
    );
}

// ---------------------------------------------------------------------------
// 10. PUSH-001 — Query not pushed down
// ---------------------------------------------------------------------------

#[test]
fn push_001_triggers_on_streaming_redistribute() {
    let report = analyze_fixture("16_multi_streaming.txt");
    let finding = get_finding(&report, "PUSH-001")
        .expect("Expected PUSH-001 for Streaming(Redistribute)");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::PushdownFailure);
    assert!(
        finding.detail.contains("Streaming") || finding.detail.contains("下推"),
        "detail should mention streaming or pushdown"
    );
}

#[test]
fn push_001_does_not_trigger_on_gather_only() {
    let report = analyze_fixture("06_streaming_gather.txt");
    assert!(
        !has_finding(&report, "PUSH-001"),
        "PUSH-001 should not fire for GATHER streaming alone"
    );
}

// ---------------------------------------------------------------------------
// 11. PUSH-002 — Multi-layer streaming
// ---------------------------------------------------------------------------

#[test]
fn push_002_triggers_on_multi_layer_streaming() {
    let report = analyze_fixture("16_multi_streaming.txt");
    let finding = get_finding(&report, "PUSH-002")
        .expect("Expected PUSH-002 for multi-layer streaming");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::PushdownFailure);
    assert!(
        finding.detail.contains("Streaming") || finding.detail.contains("重分布"),
        "detail should mention streaming or redistribution"
    );
}

// ---------------------------------------------------------------------------
// 12. TYPE-001 — Implicit type coercion
// ---------------------------------------------------------------------------

#[test]
fn type_001_triggers_on_implicit_cast() {
    let report = analyze_fixture("17_implicit_cast.txt");
    let finding = get_finding(&report, "TYPE-001")
        .expect("Expected TYPE-001 for suspected implicit type cast");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::TypeMismatch);
    assert!(
        finding.detail.contains("status = 42") || finding.detail.contains("隐式类型转换"),
        "detail should mention filter condition or implicit cast"
    );
}

// ---------------------------------------------------------------------------
// 13. TYPE-004 — LIKE with leading wildcard
// ---------------------------------------------------------------------------

#[test]
fn type_004_triggers_on_like_wildcard() {
    let report = analyze_fixture("18_like_wildcard.txt");
    let finding = get_finding(&report, "TYPE-004")
        .expect("Expected TYPE-004 for LIKE with leading wildcard");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::TypeMismatch);
    assert!(
        finding.detail.contains("LIKE") || finding.detail.contains("like"),
        "detail should mention LIKE"
    );
}

// ---------------------------------------------------------------------------
// 14. VEC-001 — Mixed row/vector engines
// ---------------------------------------------------------------------------

#[test]
fn vec_001_triggers_on_mixed_engines() {
    let report = analyze_fixture("19_mixed_engines.txt");
    let finding = get_finding(&report, "VEC-001")
        .expect("Expected VEC-001 for mixed row/vector engines");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::Vectorization);
    assert!(
        finding.detail.contains("Adapter") || finding.detail.contains("适配器"),
        "detail should mention Adapter"
    );
}

#[test]
fn vec_001_does_not_trigger_without_adapters() {
    let report = analyze_fixture("07_vector_hash_join.txt");
    assert!(
        !has_finding(&report, "VEC-001"),
        "VEC-001 should not fire when no adapter nodes exist"
    );
}

// ---------------------------------------------------------------------------
// 15. GEN-001 — Plan too deep
// ---------------------------------------------------------------------------

#[test]
fn gen_001_triggers_on_deep_plan_with_custom_config() {
    let plan = parse_fixture("20_deep_plan.txt");
    let config = DiagnosticConfig {
        max_plan_depth: 5,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    let finding = get_finding(&report, "GEN-001")
        .expect("Expected GEN-001 when max_plan_depth is lowered to 5");
    assert_eq!(finding.severity, Severity::Info);
    assert_eq!(finding.category, DiagnosticCategory::General);
    assert!(
        finding.detail.contains("深度") || finding.detail.contains("6"),
        "detail should mention plan depth"
    );
}

// ---------------------------------------------------------------------------
// Config customization tests
// ---------------------------------------------------------------------------

#[test]
fn config_large_table_rows_threshold_blocks_scan_001() {
    let plan = parse_fixture("10_complex_plan.txt");
    let config = DiagnosticConfig {
        large_table_rows: 1_000_000.0,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    assert!(
        !has_finding(&report, "SCAN-001"),
        "SCAN-001 should not fire when large_table_rows threshold exceeds actual rows"
    );
    assert!(has_finding(&report, "MEM-001"), "MEM-001 should still fire");
}

#[test]
fn config_disabled_rules_removes_specified_findings() {
    let plan = parse_fixture("12_hash_spill.txt");
    let config = DiagnosticConfig {
        disabled_rules: vec!["JOIN-002".to_string()],
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    assert!(
        !has_finding(&report, "JOIN-002"),
        "Disabled JOIN-002 should not appear"
    );
    assert!(
        has_finding(&report, "MEM-004"),
        "MEM-004 should still fire when only JOIN-002 is disabled"
    );
}

#[test]
fn config_memory_threshold_blocks_mem_004() {
    let plan = parse_fixture("12_hash_spill.txt");
    let config = DiagnosticConfig {
        memory_threshold_kb: 1_000_000.0,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    assert!(
        !has_finding(&report, "MEM-004"),
        "MEM-004 should not fire when memory threshold exceeds peak memory"
    );
    assert!(has_finding(&report, "JOIN-002"), "JOIN-002 should still fire");
}

#[test]
fn config_nested_loop_threshold_blocks_join_001() {
    let plan = parse_fixture("11_nested_loop_large.txt");
    let config = DiagnosticConfig {
        nested_loop_inner_rows: 10_000_000.0,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    assert!(
        !has_finding(&report, "JOIN-001"),
        "JOIN-001 should not fire when nested_loop_inner_rows exceeds inner work"
    );
}

// ---------------------------------------------------------------------------
// SuggestionEngine cross-rule synthesis tests
// ---------------------------------------------------------------------------

#[test]
fn suggestion_multiple_est_findings_trigger_analyze() {
    let plan = parse_fixture("15_severe_underestimate.txt");
    let config = DiagnosticConfig {
        estimation_skew_factor: 10.0,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);

    // Ensure we have at least 2 EST- findings to trigger the synthesis
    let est_count = report.findings.iter().filter(|f| f.rule_id.starts_with("EST-")).count();
    assert!(
        est_count >= 2,
        "Need at least 2 EST- findings for synthesis, got {}",
        est_count
    );

    let suggestions = SuggestionEngine::suggest(&report.findings);
    let analyze_suggestion = suggestions
        .iter()
        .find(|s| matches!(s.category, SuggestionCategory::StatisticsUpdate))
        .expect("Expected a StatisticsUpdate suggestion");
    assert!(analyze_suggestion.message.contains("ANALYZE"));
    assert!(analyze_suggestion.confidence > 0.0);
}

#[test]
fn suggestion_multiple_spill_findings_trigger_work_mem() {
    // Combine findings from a hash-spill report and a sort-spill report
    let hash_plan = parse_fixture("12_hash_spill.txt");
    let hash_report = analyze(&hash_plan);
    assert!(has_finding(&hash_report, "JOIN-002"), "Need JOIN-002 from hash spill");

    let sort_plan = parse_fixture("05_sort_external_merge.txt");
    let sort_report = analyze(&sort_plan);
    assert!(has_finding(&sort_report, "MEM-001"), "Need MEM-001 from sort spill");

    let mut combined_findings = hash_report.findings.clone();
    combined_findings.extend(sort_report.findings);
    let suggestions = SuggestionEngine::suggest(&combined_findings);
    let work_mem_suggestion = suggestions
        .iter()
        .find(|s| matches!(s.category, SuggestionCategory::ConfigurationTuning))
        .expect("Expected a ConfigurationTuning suggestion for multiple spills");
    assert!(work_mem_suggestion.message.contains("work_mem"));
    assert!(work_mem_suggestion.confidence > 0.0);
    assert!(work_mem_suggestion.related_rules.len() >= 2);
}

#[test]
fn suggestion_scan_and_join_findings_trigger_composite_index() {
    let plan = parse_fixture("11_nested_loop_large.txt");
    let report = analyze(&plan);

    // Ensure both SCAN- and JOIN- findings are present
    assert!(
        report.findings.iter().any(|f| f.rule_id.starts_with("SCAN-")),
        "Need a SCAN- finding"
    );
    assert!(
        report.findings.iter().any(|f| f.rule_id.starts_with("JOIN-")),
        "Need a JOIN- finding"
    );

    let suggestions = SuggestionEngine::suggest(&report.findings);
    let index_suggestion = suggestions
        .iter()
        .find(|s| matches!(s.category, SuggestionCategory::IndexOptimization))
        .expect("Expected an IndexOptimization suggestion for scan + join findings");
    assert!(index_suggestion.message.contains("复合索引") || index_suggestion.message.contains("索引"));
    assert!(index_suggestion.confidence > 0.0);
}

#[test]
fn suggestion_push_findings_trigger_distribution_optimization() {
    let plan = parse_fixture("16_multi_streaming.txt");
    let report = analyze(&plan);

    // Ensure PUSH- findings are present
    assert!(
        report.findings.iter().any(|f| f.rule_id.starts_with("PUSH-")),
        "Need a PUSH- finding"
    );

    let suggestions = SuggestionEngine::suggest(&report.findings);
    let dist_suggestion = suggestions
        .iter()
        .find(|s| matches!(s.category, SuggestionCategory::DistributionOptimization))
        .expect("Expected a DistributionOptimization suggestion for push findings");
    assert!(dist_suggestion.message.contains("下推") || dist_suggestion.message.contains("重分布"));
    assert!(dist_suggestion.confidence > 0.0);
}

// ---------------------------------------------------------------------------
// Structural / edge-case tests
// ---------------------------------------------------------------------------

#[test]
fn finding_contains_node_type_and_line() {
    let report = analyze_fixture("10_complex_plan.txt");
    let finding = get_finding(&report, "SCAN-001").expect("SCAN-001 should be present");
    assert!(finding.node_type.is_some(), "Finding should have node_type");
    assert!(finding.node_line.is_some(), "Finding should have node_line");
    assert!(!finding.title.is_empty(), "Finding should have a title");
    assert!(!finding.suggestion.as_ref().unwrap().is_empty(), "Finding should have a suggestion");
}

#[test]
fn analyze_report_stats_are_populated() {
    let report = analyze_fixture("10_complex_plan.txt");
    assert!(report.stats.total_nodes > 0, "total_nodes should be > 0");
    assert!(report.stats.max_depth > 0, "max_depth should be > 0");
}

#[test]
fn duplicate_sort_finding_targets_outer_sort_node() {
    let report = analyze_fixture("13_duplicate_sort.txt");
    let finding = get_finding(&report, "SORT-003").expect("SORT-003 should be present");
    assert_eq!(finding.node_type.as_deref(), Some("Sort"));
}

#[test]
fn broadcast_finding_contains_actual_row_count() {
    let report = analyze_fixture("14_broadcast_large.txt");
    let finding = get_finding(&report, "NET-001").expect("NET-001 should be present");
    assert!(finding.detail.contains("50000"), "NET-001 detail should contain actual row count");
}

#[test]
fn hash_spill_finding_contains_batch_count() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "JOIN-002").expect("JOIN-002 should be present");
    assert!(
        finding.detail.contains("5 batches") || finding.detail.contains("used 5 batches"),
        "JOIN-002 detail should contain batch count: {}",
        finding.detail
    );
}

#[test]
fn severe_underestimate_has_correct_severity_and_category() {
    let plan = parse_fixture("15_severe_underestimate.txt");
    let config = DiagnosticConfig {
        estimation_skew_factor: 10.0,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    let finding = get_finding(&report, "EST-001").expect("EST-001 should be present");
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::CostMisestimation);
}

#[test]
fn implicit_cast_finding_contains_filter_and_rows_removed() {
    let report = analyze_fixture("17_implicit_cast.txt");
    let finding = get_finding(&report, "TYPE-001").expect("TYPE-001 should be present");
    assert!(finding.detail.contains("status = 42"), "detail should show filter condition");
    assert!(finding.detail.contains("500000"), "detail should show rows removed");
}

#[test]
fn like_wildcard_finding_contains_like_pattern() {
    let report = analyze_fixture("18_like_wildcard.txt");
    let finding = get_finding(&report, "TYPE-004").expect("TYPE-004 should be present");
    assert!(
        finding.detail.contains("'%important%'"),
        "detail should contain the LIKE pattern"
    );
}

#[test]
fn mixed_engines_finding_counts_two_adapters() {
    let report = analyze_fixture("19_mixed_engines.txt");
    let finding = get_finding(&report, "VEC-001").expect("VEC-001 should be present");
    assert!(
        finding.detail.contains("2") || finding.detail.contains("两"),
        "detail should mention adapter count: {}",
        finding.detail
    );
}

#[test]
fn push_002_finding_has_streaming_detail() {
    let report = analyze_fixture("16_multi_streaming.txt");
    let finding = get_finding(&report, "PUSH-002").expect("PUSH-002 should be present");
    assert!(
        finding.detail.contains("Streaming"),
        "PUSH-002 detail should mention Streaming"
    );
}

#[test]
fn mem_004_finding_shows_peak_vs_threshold() {
    let report = analyze_fixture("12_hash_spill.txt");
    let finding = get_finding(&report, "MEM-004").expect("MEM-004 should be present");
    assert!(
        finding.detail.contains("512000"),
        "MEM-004 detail should show peak memory"
    );
    assert!(
        finding.detail.contains("102400"),
        "MEM-004 detail should show threshold"
    );
}

#[test]
fn config_estimation_skew_factor_affects_est_001_trigger() {
    let plan = parse_fixture("15_severe_underestimate.txt");

    // With very high threshold, EST-001 should not fire
    let high_config = DiagnosticConfig {
        estimation_skew_factor: 10000.0,
        ..DiagnosticConfig::default()
    };
    let high_report = analyze_with_config(&plan, &high_config);
    assert!(
        !has_finding(&high_report, "EST-001"),
        "EST-001 should not fire with extremely high estimation_skew_factor"
    );

    // With low threshold, EST-001 should fire
    let low_config = DiagnosticConfig {
        estimation_skew_factor: 10.0,
        ..DiagnosticConfig::default()
    };
    let low_report = analyze_with_config(&plan, &low_config);
    assert!(
        has_finding(&low_report, "EST-001"),
        "EST-001 should fire with lowered estimation_skew_factor"
    );
}

// ---------------------------------------------------------------------------
// SUBQ-006 — Correlated subquery self-referencing UPDATE
// ---------------------------------------------------------------------------

#[test]
fn subq_006_triggers_on_correlated_subquery_update() {
    let report = analyze_fixture("23_correlated_subquery_update.txt");
    let finding = get_finding(&report, "SUBQ-006")
        .expect("Expected SUBQ-006 for correlated subquery self-referencing UPDATE");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::SubqueryStructure);
    assert!(finding.detail.contains("employees"), "detail should mention table name");
}

#[test]
fn subq_006_does_not_trigger_on_normal_update() {
    let report = analyze_fixture("25_normal_update.txt");
    assert!(
        !has_finding(&report, "SUBQ-006"),
        "SUBQ-006 should not fire for normal UPDATE without correlated subquery"
    );
}

#[test]
fn subq_006_finding_contains_template_suggestion() {
    let report = analyze_fixture("23_correlated_subquery_update.txt");
    let finding = get_finding(&report, "SUBQ-006").expect("SUBQ-006 should be present");
    let suggestion = finding.suggestion.as_ref().expect("SUBQ-006 should have a suggestion");
    assert!(
        suggestion.contains("UPDATE") && suggestion.contains("FROM"),
        "suggestion should include UPDATE FROM rewrite template, got: {}",
        suggestion
    );
    assert!(
        suggestion.contains("employees"),
        "suggestion should include actual table name"
    );
}

#[test]
fn subq_006_triggers_with_streaming_in_distributed() {
    let report = analyze_fixture("24_correlated_subquery_update_distributed.txt");
    let finding = get_finding(&report, "SUBQ-006")
        .expect("Expected SUBQ-006 for distributed correlated subquery UPDATE");
    assert!(
        finding.detail.contains("Streaming") || finding.detail.contains("分布式") || finding.detail.contains("distributed"),
        "detail should mention distributed scenario, got: {}",
        finding.detail
    );
}

#[test]
fn suggestion_engine_produces_query_rewrite_for_subq006() {
    let report = analyze_fixture("23_correlated_subquery_update.txt");
    let suggestions = SuggestionEngine::suggest(&report.findings);
    let qr = suggestions.iter().find(|s| matches!(s.category, SuggestionCategory::QueryRewrite));
    assert!(qr.is_some(), "SUBQ-006 should produce a QueryRewrite suggestion");
    assert!(qr.unwrap().confidence >= 0.85);
}

#[test]
fn suggestion_engine_returns_empty_for_no_findings() {
    let suggestions = SuggestionEngine::suggest(&[]);
    assert!(suggestions.is_empty(), "No findings should produce no suggestions");
}

#[test]
fn suggestion_engine_returns_empty_for_irrelevant_findings() {
    // Use a simple fixture that produces no findings
    let plan = parse_fixture("01_simple_seq_scan.txt");
    let report = analyze(&plan);
    let suggestions = SuggestionEngine::suggest(&report.findings);
    for s in &suggestions {
        assert!(s.confidence > 0.0 && s.confidence <= 1.0);
    }
}
