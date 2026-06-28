//! Static end-to-end tests for the optimize pipeline.
//!
//! These tests validate the decision chain (parse → analyze → filter → map →
//! converge) without requiring a database or metamorphosis binary. Live-DB
//! validation is in `tests/optimize_live_e2e.sh`.

use ogexplain_cli::optimize::mapper::{filter_rewritable, map_diagnostic, RemediationAction};
use ogexplain_core::convergence::{
    should_continue, LoopConfig, LoopDecision, MetricsSnapshot, StopReason,
};
use ogexplain_core::{analyze, parse};

/// Fixture 21 triggers SUBQ-001 via SubPlan path (table=None).
/// Verify: the finding fires but filter_rewritable correctly filters it
/// (quality gate: SUBQ-001 without table is excluded).
#[test]
fn fixture_21_subq_001_filtered_by_quality_gate() {
    let explain_text = include_str!("fixtures/21_regression_test_raw.txt");
    let plan = parse(explain_text).expect("parse fixture 21");
    let report = analyze(&plan);

    let subq_findings: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.rule_id == "SUBQ-001")
        .collect();
    assert!(
        !subq_findings.is_empty(),
        "fixture 21 must trigger SUBQ-001"
    );

    let rewritable = filter_rewritable(&report.findings);
    let subq_in_rewritable: Vec<_> = rewritable
        .iter()
        .filter(|f| f.rule_id == "SUBQ-001")
        .collect();
    assert!(
        subq_in_rewritable.is_empty(),
        "SUBQ-001 with table=None must be filtered by quality gate, got {:?}",
        subq_in_rewritable
    );
}

/// Verify all Track A rule_ids map to Rewrite actions (or UseBuiltinRewrite).
#[test]
fn track_a_rules_map_to_rewrite_actions() {
    let track_a = [
        ("SUBQ-001", vec!["subquery-to-join"]),
        ("REW-001", vec!["subquery-to-join"]),
        ("TYPE-001", vec!["add-explicit-cast"]),
        ("TYPE-004", vec!["suggest-trgm-index"]),
        ("AGG-001", vec!["rewrite-group-agg"]),
    ];
    for (rule_id, expected_rules) in track_a {
        let action = map_diagnostic(rule_id);
        match action {
            RemediationAction::Rewrite { rules } => {
                assert_eq!(
                    rules, expected_rules,
                    "rule_id {} mapped to wrong rules",
                    rule_id
                );
            }
            _ => panic!(
                "rule_id {} should map to Rewrite, got {:?}",
                rule_id, action
            ),
        }
    }
    assert!(matches!(
        map_diagnostic("SUBQ-006"),
        RemediationAction::UseBuiltinRewrite
    ));
}

/// Verify Track B/C/D rules map to advisory actions, not Rewrite.
#[test]
fn track_bcd_rules_map_to_advisory_actions() {
    let ddl_rules = ["SCAN-001", "SCAN-004", "JOIN-001"];
    for rule_id in ddl_rules {
        assert!(
            matches!(map_diagnostic(rule_id), RemediationAction::DdlAdvice),
            "{} should be DdlAdvice",
            rule_id
        );
    }
    let config_rules = ["MEM-001", "MEM-004", "JOIN-002", "AGG-002"];
    for rule_id in config_rules {
        assert!(
            matches!(map_diagnostic(rule_id), RemediationAction::ConfigAdvice),
            "{} should be ConfigAdvice",
            rule_id
        );
    }
    let analyze_rules = ["STATS-001", "EST-001", "EST-004"];
    for rule_id in analyze_rules {
        assert!(
            matches!(map_diagnostic(rule_id), RemediationAction::RunAnalyze),
            "{} should be RunAnalyze",
            rule_id
        );
    }
    assert!(matches!(map_diagnostic("PUSH-001"), RemediationAction::Log));
}

/// Convergence: cost reduction + critical reduction → Continue.
#[test]
fn convergence_continues_on_improvement() {
    let prev = MetricsSnapshot {
        total_cost: Some(100_000.0),
        critical_count: 3,
        ..Default::default()
    };
    let curr = MetricsSnapshot {
        total_cost: Some(50_000.0),
        critical_count: 2,
        ..Default::default()
    };
    let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, false);
    assert!(matches!(decision, LoopDecision::Continue));
}

/// Convergence: critical=0 → Success stop.
#[test]
fn convergence_stops_on_success() {
    let prev = MetricsSnapshot {
        total_cost: Some(100_000.0),
        critical_count: 1,
        ..Default::default()
    };
    let curr = MetricsSnapshot {
        total_cost: Some(50_000.0),
        critical_count: 0,
        ..Default::default()
    };
    let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, false, false);
    assert!(matches!(decision, LoopDecision::Stop(StopReason::Success)));
}

/// Convergence: SQL unchanged (fixed-point) → FixedPoint stop.
#[test]
fn convergence_stops_on_fixed_point() {
    let prev = MetricsSnapshot {
        total_cost: Some(100_000.0),
        critical_count: 2,
        ..Default::default()
    };
    let curr = MetricsSnapshot {
        total_cost: Some(95_000.0),
        critical_count: 2,
        ..Default::default()
    };
    let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, true);
    assert!(matches!(
        decision,
        LoopDecision::Stop(StopReason::FixedPoint)
    ));
}

/// Convergence: cost regression > 10% → Regression stop.
#[test]
fn convergence_stops_on_regression() {
    let prev = MetricsSnapshot {
        total_cost: Some(100_000.0),
        critical_count: 2,
        ..Default::default()
    };
    let curr = MetricsSnapshot {
        total_cost: Some(120_000.0),
        critical_count: 2,
        ..Default::default()
    };
    let decision = should_continue(&prev, &curr, &LoopConfig::default(), 1, 0, true, false);
    assert!(matches!(
        decision,
        LoopDecision::Stop(StopReason::Regression)
    ));
}

/// Full pipeline on fixture 10 (complex plan): parse → analyze produces
/// findings; filter_rewritable returns only Track A findings with structured
/// metadata.
#[test]
fn full_pipeline_fixture_10_filters_correctly() {
    let explain_text = include_str!("fixtures/10_complex_plan.txt");
    let plan = parse(explain_text).expect("parse fixture 10");
    let report = analyze(&plan);

    assert!(
        !report.findings.is_empty(),
        "fixture 10 must produce findings"
    );

    let rewritable = filter_rewritable(&report.findings);
    for f in &rewritable {
        let action = map_diagnostic(&f.rule_id);
        assert!(
            matches!(
                action,
                RemediationAction::Rewrite { .. } | RemediationAction::UseBuiltinRewrite
            ),
            "filter_rewritable must only return Rewrite/UseBuiltinRewrite findings, got {:?} for {}",
            action,
            f.rule_id
        );
    }
}
