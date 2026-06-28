//! Tests for SummaryRow computation.

use ogexplain_core::model::{
    ActualStats, BufferStats, EstimatedCost, ExplainPlan, NodeType, PlanNode,
};
use ogexplain_core::summary::{ComplexityInput, PushdownStatus, SummaryRow};
use ogexplain_core::{analyze, parse};

fn read_fixture(name: &str) -> String {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::read_to_string(base.join(name))
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", name, e))
}

fn parse_fixture(name: &str) -> ogexplain_core::model::ExplainPlan {
    let input = read_fixture(name);
    parse(&input).unwrap_or_else(|e| panic!("Failed to parse {}: {:?}", name, e))
}

#[test]
fn summary_from_simple_fixture_01() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);

    assert_eq!(summary.root_op, "SeqScan");
    assert_eq!(summary.total_cost, 12.0);
    assert_eq!(summary.total_time_ms, 0.123);
    assert_eq!(summary.actual_rows, Some(100.0));
    assert_eq!(summary.plan_depth, 1);
    assert_eq!(summary.node_count, 1);
    assert_eq!(summary.worst_est_ratio, Some(1.0));
    assert_eq!(summary.spill_kb, None);
    assert_eq!(summary.peak_memory_kb, None);
    assert_eq!(summary.pushdown, PushdownStatus::Local);
    assert_eq!(summary.buffer_hit_rate, None);
    assert_eq!(summary.critical_count, 0);
    assert_eq!(summary.warning_count, 0);
    assert_eq!(summary.info_count, 0);
    assert_eq!(summary.estimated_rows, Some(100.0));
    assert_eq!(summary.total_loops, Some(1.0));
}

#[test]
fn summary_from_complex_fixture_10() {
    let plan = parse_fixture("10_complex_plan.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);

    assert_eq!(summary.root_op, "Sort");
    assert_eq!(summary.total_cost, 275.35);
    assert_eq!(summary.total_time_ms, 55.789);
    assert_eq!(summary.actual_rows, Some(50000.0));
    assert_eq!(summary.plan_depth, 5);
    assert_eq!(summary.node_count, 7);
    assert_eq!(summary.worst_est_ratio, Some(100.0));
    assert_eq!(summary.spill_kb, Some(5840.0));
    assert_eq!(summary.peak_memory_kb, Some(8192.0));
    assert_eq!(summary.pushdown, PushdownStatus::Local);
    assert_eq!(summary.network_kb, None);
    assert_eq!(summary.planner_time_ms, None);
}

#[test]
fn summary_with_complexity_input() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    let report = analyze(&plan);
    let complexity = ComplexityInput {
        sql_preview: Some("SELECT * FROM t1".to_string()),
        tables: 1,
        joins: 0,
        subqueries: 0,
        where_conditions: 1,
        aggregates: 0,
        cases: 0,
        set_ops: 0,
        ctes: 0,
        windows: 0,
        has_group_by: false,
        has_order_by: false,
        has_distinct: false,
        subquery_depth: 0,
        hints: 0,
        score: Some(15.5),
        level: Some("Low".to_string()),
        gauss_score: Some(20),
        gauss_level: Some("Simple".to_string()),
        sql_category: Some("Query".to_string()),
        sql_sub_type: Some("SELECT".to_string()),
        gauss_sql_structure: Some(5),
        gauss_pl_logic: Some(0),
        gauss_advanced_feature: Some(2),
        gauss_extension: Some(1),
        gauss_tags: vec!["single-table".to_string()],
        template_id: None,
    };
    let summary = SummaryRow::compute(&plan, &report, Some(&complexity));

    assert_eq!(summary.sql_preview, Some("SELECT * FROM t1".to_string()));
    assert_eq!(summary.tables, 1);
    assert_eq!(summary.joins, 0);
    assert_eq!(summary.subqueries, 0);
    assert_eq!(summary.where_conditions, 1);
    assert_eq!(summary.aggregates, 0);
    assert_eq!(summary.cases, 0);
    assert_eq!(summary.set_ops, 0);
    assert_eq!(summary.ctes, 0);
    assert_eq!(summary.windows, 0);
    assert!(!summary.has_group_by);
    assert!(!summary.has_order_by);
    assert!(!summary.has_distinct);
    assert_eq!(summary.subquery_depth, 0);
    assert_eq!(summary.hints, 0);
    assert_eq!(summary.score, Some(15.5));
    assert_eq!(summary.level, Some("Low".to_string()));
    assert_eq!(summary.gauss_score, Some(20));
    assert_eq!(summary.gauss_level, Some("Simple".to_string()));
    assert_eq!(summary.sql_category, Some("Query".to_string()));
    assert_eq!(summary.sql_sub_type, Some("SELECT".to_string()));
    assert_eq!(summary.gauss_sql_structure, Some(5));
    assert_eq!(summary.gauss_pl_logic, Some(0));
    assert_eq!(summary.gauss_advanced_feature, Some(2));
    assert_eq!(summary.gauss_extension, Some(1));
    assert_eq!(summary.gauss_tags, vec!["single-table"]);
}

#[test]
fn pushdown_local_for_non_streaming_plan() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);
    assert_eq!(summary.pushdown, PushdownStatus::Local);
}

#[test]
fn pushdown_not_pushed_for_streaming_plan() {
    let plan = parse_fixture("06_streaming_gather.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);
    assert_eq!(summary.pushdown, PushdownStatus::NotPushed);
    assert_eq!(summary.node_count, 2);
    assert_eq!(summary.plan_depth, 2);
}

#[test]
fn buffer_hit_rate_computation() {
    let plan = ExplainPlan {
        root: PlanNode {
            node_type: NodeType::SeqScan,
            relation: Some("t1".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0,
                total_cost: 10.0,
                plan_rows: 100.0,
                plan_width: 4,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: 0.5,
                rows: 100.0,
                loops: 1.0,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: Some(BufferStats {
                shared_hit: 90,
                shared_read: 10,
                ..Default::default()
            }),
            children: vec![],
            indent_level: 0,
            line_number: 1,
        },
        summary: None,
    };
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);
    assert_eq!(summary.buffer_hit_rate, Some(90.0));
}

#[test]
fn buffer_hit_rate_none_when_no_buffers() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);
    assert_eq!(summary.buffer_hit_rate, None);
}

#[test]
fn peak_memory_extraction_fixture_10() {
    let plan = parse_fixture("10_complex_plan.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);
    assert_eq!(summary.peak_memory_kb, Some(8192.0));
}

#[test]
fn peak_memory_extraction_fixture_12() {
    let plan = parse_fixture("12_hash_spill.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);
    assert_eq!(summary.peak_memory_kb, Some(512000.0));
}

#[test]
fn severity_counts_match_report_fixture_10() {
    let plan = parse_fixture("10_complex_plan.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);

    assert_eq!(
        summary.critical_count + summary.warning_count + summary.info_count,
        report.findings.len()
    );
    assert_eq!(summary.critical_count, 1);
    assert_eq!(summary.warning_count, 2);
    assert_eq!(summary.info_count, 0);
}

#[test]
fn severity_counts_zero_for_clean_plan() {
    let plan = parse_fixture("01_simple_seq_scan.txt");
    let report = analyze(&plan);
    let summary = SummaryRow::compute(&plan, &report, None);

    assert_eq!(summary.critical_count, 0);
    assert_eq!(summary.warning_count, 0);
    assert_eq!(summary.info_count, 0);
    assert!(report.findings.is_empty());
}
