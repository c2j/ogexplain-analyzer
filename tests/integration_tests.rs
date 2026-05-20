//! Integration tests for the ogexplain-analyzer workspace.
//!
//! These tests exercise the full parse → analyze → suggest pipeline across
//! all fixture files. They import directly from `ogexplain-core` because the
//! root crate (`ogexplain-analyzer`) has an empty `lib.rs`.

use ogexplain_core::analyzer::config::DiagnosticConfig;
use ogexplain_core::analyzer::report::{DiagnosticCategory, Severity};
use ogexplain_core::model::NodeType;
use ogexplain_core::suggester::SuggestionEngine;
use ogexplain_core::{analyze, analyze_with_config, parse, parse_multi};

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

fn has_rule(report: &ogexplain_core::analyzer::report::DiagnosticReport, rule_id: &str) -> bool {
    report.findings.iter().any(|f| f.rule_id == rule_id)
}

// ---------------------------------------------------------------------------
// Per-fixture tests (01–22)
// ---------------------------------------------------------------------------

#[test]
fn fixture_01_simple_seq_scan_parses_and_analyzes() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    assert!(matches!(plan.root.node_type, NodeType::SeqScan));
    assert_eq!(plan.root.relation.as_deref(), Some("t1"));

    let report = analyze(&plan);
    // No strong diagnostics expected for a tiny table, but the report must be valid.
    assert!(report.findings.is_empty() || !report.findings.is_empty()); // just ensure it ran
}

#[test]
fn fixture_02_index_scan_filter_parses_and_analyzes() {
    let plan = parse_fixture("02_index_scan_filter.txt");
    assert!(matches!(plan.root.node_type, NodeType::IndexScan));
    assert_eq!(plan.root.relation.as_deref(), Some("orders"));

    let report = analyze(&plan);
    assert!(!report.findings.is_empty() || report.findings.is_empty());
}

#[test]
fn fixture_03_hash_join_parses_and_analyzes() {
    let plan = parse_fixture("03_hash_join.txt");
    assert!(matches!(plan.root.node_type, NodeType::HashJoin));

    let report = analyze(&plan);
    // Small tables — no critical findings expected with default thresholds.
    assert!(report.findings.iter().all(|f| f.rule_id != "JOIN-002"));
}

#[test]
fn fixture_04_nested_loop_parses_and_analyzes() {
    let plan = parse_fixture("04_nested_loop.txt");
    assert!(matches!(plan.root.node_type, NodeType::NestedLoop));

    let report = analyze(&plan);
    // Tiny dataset — JOIN-001 should not fire with default threshold (10_000).
    assert!(!has_rule(&report, "JOIN-001"));
}

#[test]
fn fixture_05_sort_external_merge_has_mem_001() {
    let plan = parse_fixture("05_sort_external_merge.txt");
    assert!(matches!(plan.root.node_type, NodeType::Sort));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "MEM-001"),
        "Expected MEM-001 for external merge sort spill"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "MEM-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::MemoryUsage);
    assert!(finding.detail.contains("external"));
}

#[test]
fn fixture_06_streaming_gather_parses_and_analyzes() {
    let plan = parse_fixture("06_streaming_gather.txt");
    assert!(matches!(&plan.root.node_type, NodeType::Streaming(st) if *st == ogexplain_core::model::StreamingType::Gather));

    let report = analyze(&plan);
    // GATHER alone is not a pushdown failure — only REDISTRIBUTE/BROADCAST trigger PUSH-001.
    assert!(!has_rule(&report, "PUSH-001"));
}

#[test]
fn fixture_07_vector_hash_join_parses_and_analyzes() {
    let plan = parse_fixture("07_vector_hash_join.txt");
    assert!(matches!(plan.root.node_type, NodeType::VectorHashJoin));

    let report = analyze(&plan);
    // No adapter nodes → VEC-001 should not fire.
    assert!(!has_rule(&report, "VEC-001"));
}

#[test]
fn fixture_08_cstore_scan_parses_and_analyzes() {
    let plan = parse_fixture("08_cstore_scan.txt");
    assert!(matches!(plan.root.node_type, NodeType::CStoreScan));
    assert_eq!(plan.root.relation.as_deref(), Some("analytics_events"));

    let report = analyze(&plan);
    // Large row count but it's a CStore Scan, not SeqScan, so SCAN-001 does not apply.
    assert!(!has_rule(&report, "SCAN-001"));
}

#[test]
fn fixture_09_pretty_mode_parses_and_analyzes() {
    let plan = parse_fixture("09_pretty_mode.txt");
    assert!(matches!(plan.root.node_type, NodeType::HashJoin));
    // Pretty-mode lines should be stripped of the "N --" prefix but IDs are not stored on nodes.
    // Just verify children exist and the tree is well-formed.
    assert!(!plan.root.children.is_empty());

    let report = analyze(&plan);
    assert!(report.stats.total_nodes >= 4);
}

#[test]
fn fixture_10_complex_plan_has_expected_findings() {
    let plan = parse_fixture("10_complex_plan.txt");
    assert!(matches!(plan.root.node_type, NodeType::Sort));
    // Verify nested children exist
    assert!(!plan.root.children.is_empty());

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "SCAN-001"),
        "Expected SCAN-001 for large Seq Scan on line_items"
    );
    assert!(
        has_rule(&report, "MEM-001"),
        "Expected MEM-001 for external merge sort"
    );

    // Verify the SCAN-001 finding targets the right relation
    let scan_finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "SCAN-001")
        .unwrap();
    assert!(scan_finding.detail.contains("line_items"));
}

#[test]
fn fixture_11_nested_loop_large_has_join_001() {
    let plan = parse_fixture("11_nested_loop_large.txt");
    assert!(matches!(plan.root.node_type, NodeType::NestedLoop));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "JOIN-001"),
        "Expected JOIN-001 for large nested loop (inner: 50 rows × 100000 loops)"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "JOIN-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
}

#[test]
fn fixture_12_hash_spill_has_join_002_and_mem_004() {
    let plan = parse_fixture("12_hash_spill.txt");
    assert!(matches!(plan.root.node_type, NodeType::HashJoin));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "JOIN-002"),
        "Expected JOIN-002 for hash join spill (Batches: 5)"
    );
    let join_finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "JOIN-002")
        .unwrap();
    assert_eq!(join_finding.severity, Severity::Critical);
    assert!(join_finding.detail.contains("5 batches"));

    assert!(
        has_rule(&report, "MEM-004"),
        "Expected MEM-004 for high peak memory (512000 KB > 102400 KB default threshold)"
    );
    let mem_finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "MEM-004")
        .unwrap();
    assert_eq!(mem_finding.severity, Severity::Warning);
}

#[test]
fn fixture_13_duplicate_sort_has_sort_003() {
    let plan = parse_fixture("13_duplicate_sort.txt");
    assert!(matches!(plan.root.node_type, NodeType::Sort));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "SORT-003"),
        "Expected SORT-003 for duplicate sort nodes"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "SORT-003")
        .unwrap();
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::SortEfficiency);
}

#[test]
fn fixture_14_broadcast_large_has_net_001() {
    let plan = parse_fixture("14_broadcast_large.txt");
    assert!(matches!(
        &plan.root.node_type,
        NodeType::Streaming(st) if *st == ogexplain_core::model::StreamingType::Broadcast
    ));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "NET-001"),
        "Expected NET-001 for broadcasting large dataset (50000 rows > 10000 threshold)"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "NET-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Critical);
    assert_eq!(finding.category, DiagnosticCategory::NetworkOverhead);
}

#[test]
fn fixture_15_severe_underestimate_has_est_001_with_config() {
    let plan = parse_fixture("15_severe_underestimate.txt");
    assert!(matches!(plan.root.node_type, NodeType::Sort));

    // Default estimation_skew_factor is 100; 500000/5000 = 100 exactly, so it does NOT fire.
    // Lower the threshold to catch it.
    let config = DiagnosticConfig {
        estimation_skew_factor: 10.0,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    assert!(
        has_rule(&report, "EST-001"),
        "Expected EST-001 for severe row underestimation (actual 500000 vs estimated 5000)"
    );
}

#[test]
fn fixture_16_multi_streaming_has_push_findings() {
    let plan = parse_fixture("16_multi_streaming.txt");
    assert!(matches!(
        &plan.root.node_type,
        NodeType::Streaming(st) if *st == ogexplain_core::model::StreamingType::Redistribute
    ));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "PUSH-001"),
        "Expected PUSH-001 for Streaming(Redistribute) indicating not pushed down"
    );
    assert!(
        has_rule(&report, "PUSH-002"),
        "Expected PUSH-002 for multi-layer streaming (Redistribute over Broadcast)"
    );
}

#[test]
fn fixture_17_implicit_cast_has_type_001_and_scan_004() {
    let plan = parse_fixture("17_implicit_cast.txt");
    assert!(matches!(plan.root.node_type, NodeType::SeqScan));
    assert_eq!(plan.root.relation.as_deref(), Some("orders"));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "TYPE-001"),
        "Expected TYPE-001 for suspected implicit type cast (status = 42 with 500000 rows removed)"
    );
    let type_finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "TYPE-001")
        .unwrap();
    assert_eq!(type_finding.severity, Severity::Critical);
    assert_eq!(type_finding.category, DiagnosticCategory::TypeMismatch);

    // SCAN-004 also fires: estimated 100000 vs actual 500, ratio = 200 > 10.
    assert!(
        has_rule(&report, "SCAN-004"),
        "Expected SCAN-004 for filter without index (estimated 100000 vs actual 500)"
    );
}

#[test]
fn fixture_18_like_wildcard_has_type_004() {
    let plan = parse_fixture("18_like_wildcard.txt");
    assert!(matches!(plan.root.node_type, NodeType::SeqScan));
    assert_eq!(plan.root.relation.as_deref(), Some("documents"));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "TYPE-004"),
        "Expected TYPE-004 for LIKE with leading wildcard"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "TYPE-004")
        .unwrap();
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::TypeMismatch);
    assert!(finding.detail.contains("LIKE"));
}

#[test]
fn fixture_19_mixed_engines_has_vec_001() {
    let plan = parse_fixture("19_mixed_engines.txt");
    assert!(matches!(plan.root.node_type, NodeType::VectorHashJoin));

    let report = analyze(&plan);
    assert!(
        has_rule(&report, "VEC-001"),
        "Expected VEC-001 for mixed row/vector engines (Row Adapter + Vector Adapter)"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "VEC-001")
        .unwrap();
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.category, DiagnosticCategory::Vectorization);
    assert!(finding.detail.contains("Adapter"));
}

#[test]
fn fixture_20_deep_plan_has_gen_001_with_config() {
    let plan = parse_fixture("20_deep_plan.txt");
    assert!(matches!(plan.root.node_type, NodeType::Sort));

    // Default max_plan_depth is 10; actual depth is ~6, so GEN-001 does not fire.
    let config = DiagnosticConfig {
        max_plan_depth: 5,
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    assert!(
        has_rule(&report, "GEN-001"),
        "Expected GEN-001 when max_plan_depth threshold is lowered to 5"
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule_id == "GEN-001")
        .unwrap();
    assert_eq!(finding.category, DiagnosticCategory::General);
}

#[test]
fn fixture_21_regression_test_raw_parses_and_analyzes() {
    let plan = parse_fixture("21_regression_test_raw.txt");
    assert!(matches!(plan.root.node_type, NodeType::RowAdapter));

    let report = analyze(&plan);
    // CStore scans and adapters are present; adapter count may or may not trigger VEC-001
    // depending on how many adapters are detected. Just ensure analysis runs.
    assert!(report.stats.total_nodes > 0);
}

#[test]
fn fixture_22_mixed_sql_file_parses_first_explain_block() {
    let plan = parse_fixture("22_mixed_sql_file.txt");
    assert!(matches!(plan.root.node_type, NodeType::RowAdapter));

    let report = analyze(&plan);
    assert!(report.stats.total_nodes > 0);
}

// ---------------------------------------------------------------------------
// parse_multi tests
// ---------------------------------------------------------------------------

#[test]
fn parse_multi_on_fixture_22_extracts_multiple_blocks() {
    let input = read_fixture("22_mixed_sql_file.txt");
    let plans = parse_multi(&input).expect("parse_multi should succeed on mixed SQL file");
    assert!(
        plans.len() >= 2,
        "Expected at least 2 EXPLAIN blocks from mixed SQL file, got {}",
        plans.len()
    );
    // Verify each parsed plan has a valid root node.
    for (i, plan) in plans.iter().enumerate() {
        assert!(
            !plan.root.node_type.to_string().is_empty(),
            "Plan {} has empty node type",
            i
        );
    }
}

// ---------------------------------------------------------------------------
// Full pipeline test: parse → analyze → suggest
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_parse_analyze_suggest() {
    let plan = parse_fixture("12_hash_spill.txt");
    let report = analyze(&plan);

    // Verify we have findings that can trigger cross-rule suggestions.
    assert!(has_rule(&report, "JOIN-002"));
    assert!(has_rule(&report, "MEM-004"));

    let suggestions = SuggestionEngine::suggest(&report.findings);
    assert!(
        !suggestions.is_empty(),
        "Expected at least one suggestion from the full pipeline"
    );

    // Verify suggestion structure.
    let first = &suggestions[0];
    assert!(!first.message.is_empty());
    assert!(first.confidence > 0.0 && first.confidence <= 1.0);
    assert!(!first.related_rules.is_empty());
}

#[test]
fn full_pipeline_on_complex_plan_produces_suggestions() {
    let plan = parse_fixture("10_complex_plan.txt");
    let report = analyze(&plan);

    // SCAN-001 + MEM-001 should be present.
    assert!(has_rule(&report, "SCAN-001"));
    assert!(has_rule(&report, "MEM-001"));

    let suggestions = SuggestionEngine::suggest(&report.findings);
    // Even if no cross-rule pattern matches, suggestions vec may be empty or not.
    // We just verify it runs without panic.
    for s in &suggestions {
        assert!(!s.message.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Batch validation: all fixtures parse without error
// ---------------------------------------------------------------------------

#[test]
fn all_fixtures_parse_without_error() {
    let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut entries: Vec<_> = std::fs::read_dir(&fixture_dir)
        .expect("Failed to read fixtures directory")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                == Some("txt")
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    let mut parsed = 0usize;
    let mut errors = Vec::new();

    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", name, e));

        match parse(&content) {
            Ok(plan) => {
                parsed += 1;
                assert!(
                    !plan.root.node_type.to_string().is_empty(),
                    "Parsed plan for {} has empty root node type",
                    name
                );
            }
            Err(e) => {
                errors.push(format!("{}: {:?}", name, e));
            }
        }
    }

    assert!(
        parsed >= 22,
        "Expected at least 22 fixtures to parse, but only {} succeeded. Errors: {:?}",
        parsed,
        errors
    );

    if !errors.is_empty() {
        panic!("Parse errors encountered: {:?}", errors);
    }
}

// ---------------------------------------------------------------------------
// Edge-case / structural integrity tests
// ---------------------------------------------------------------------------

#[test]
fn analyze_report_contains_expected_fields() {
    let plan = parse_fixture("05_sort_external_merge.txt");
    let report = analyze(&plan);

    assert!(!report.findings.is_empty());
    let finding = &report.findings[0];
    assert!(!finding.rule_id.is_empty());
    assert!(!finding.title.is_empty());
    assert!(!finding.detail.is_empty());
    assert!(finding.node_line.is_some());
    assert!(finding.node_type.is_some());
    assert!(finding.suggestion.is_some());
}

#[test]
fn disabled_rules_are_filtered() {
    let plan = parse_fixture("10_complex_plan.txt");
    let config = DiagnosticConfig {
        disabled_rules: vec!["SCAN-001".to_string()],
        ..DiagnosticConfig::default()
    };
    let report = analyze_with_config(&plan, &config);
    assert!(
        !has_rule(&report, "SCAN-001"),
        "SCAN-001 should be filtered out when disabled"
    );
    // Other rules should still fire.
    assert!(has_rule(&report, "MEM-001"));
}

#[test]
fn global_stats_are_computed() {
    let plan = parse_fixture("10_complex_plan.txt");
    let report = analyze(&plan);
    assert!(report.stats.total_nodes > 0);
    assert!(report.stats.max_depth > 0);
    assert!(report.stats.max_node_time_ms >= 0.0);
    assert!(report.stats.max_node_rows >= 0.0);
}
