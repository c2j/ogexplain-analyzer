//! Unit tests for ogsql-complexity model types and engine scoring functions.
//!
//! These are regression guard tests covering pure functions and model types
//! that are NOT tested by the existing complexity_tests.rs integration tests.

use ogsql_complexity::model::gauss_weights::*;
use ogsql_complexity::model::{ComplexityLevel, ComplexityMetrics, SqlCategory, WeightProfile};
use ogsql_complexity::{analyze, gauss_analyze, ComplexityConfig};

// ============================================================================
// ComplexityLevel::from_score() boundary tests
// ============================================================================

#[test]
fn test_from_score_trivial_boundaries() {
    assert_eq!(ComplexityLevel::from_score(0.0), ComplexityLevel::Trivial);
    assert_eq!(ComplexityLevel::from_score(4.99), ComplexityLevel::Trivial);
    assert_eq!(
        ComplexityLevel::from_score(5.0),
        ComplexityLevel::Simple,
        "5.0 should be Simple, not Trivial"
    );
}

#[test]
fn test_from_score_simple_boundaries() {
    assert_eq!(ComplexityLevel::from_score(5.0), ComplexityLevel::Simple);
    assert_eq!(ComplexityLevel::from_score(14.99), ComplexityLevel::Simple);
    assert_eq!(
        ComplexityLevel::from_score(15.0),
        ComplexityLevel::Moderate,
        "15.0 should be Moderate, not Simple"
    );
}

#[test]
fn test_from_score_moderate_boundaries() {
    assert_eq!(ComplexityLevel::from_score(15.0), ComplexityLevel::Moderate);
    assert_eq!(
        ComplexityLevel::from_score(29.99),
        ComplexityLevel::Moderate
    );
    assert_eq!(
        ComplexityLevel::from_score(30.0),
        ComplexityLevel::Complex,
        "30.0 should be Complex, not Moderate"
    );
}

#[test]
fn test_from_score_complex_boundaries() {
    assert_eq!(ComplexityLevel::from_score(30.0), ComplexityLevel::Complex);
    assert_eq!(ComplexityLevel::from_score(49.99), ComplexityLevel::Complex);
    assert_eq!(
        ComplexityLevel::from_score(50.0),
        ComplexityLevel::VeryComplex,
        "50.0 should be VeryComplex, not Complex"
    );
}

#[test]
fn test_from_score_very_complex() {
    assert_eq!(
        ComplexityLevel::from_score(50.0),
        ComplexityLevel::VeryComplex
    );
    assert_eq!(
        ComplexityLevel::from_score(100.0),
        ComplexityLevel::VeryComplex
    );
    assert_eq!(
        ComplexityLevel::from_score(9999.0),
        ComplexityLevel::VeryComplex
    );
}

// ============================================================================
// ComplexityLevel::label()
// ============================================================================

#[test]
fn test_complexity_level_labels() {
    assert!(!ComplexityLevel::Trivial.label().is_empty());
    assert!(!ComplexityLevel::Simple.label().is_empty());
    assert!(!ComplexityLevel::Moderate.label().is_empty());
    assert!(!ComplexityLevel::Complex.label().is_empty());
    assert!(!ComplexityLevel::VeryComplex.label().is_empty());

    // Labels should be distinct
    let labels = [
        ComplexityLevel::Trivial.label(),
        ComplexityLevel::Simple.label(),
        ComplexityLevel::Moderate.label(),
        ComplexityLevel::Complex.label(),
        ComplexityLevel::VeryComplex.label(),
    ];
    for i in 0..labels.len() {
        for j in (i + 1)..labels.len() {
            assert_ne!(labels[i], labels[j], "Labels should be distinct");
        }
    }
}

// ============================================================================
// SqlCategory::label()
// ============================================================================

#[test]
fn test_sql_category_labels() {
    assert!(!SqlCategory::Query.label().is_empty());
    assert!(!SqlCategory::DML.label().is_empty());
    assert!(!SqlCategory::DDL.label().is_empty());
    assert!(!SqlCategory::DCL.label().is_empty());
    assert!(!SqlCategory::PLBlock.label().is_empty());
    assert!(!SqlCategory::Package.label().is_empty());
}

// ============================================================================
// WeightProfile
// ============================================================================

#[test]
fn test_weight_profile_default_is_gauss() {
    let default = WeightProfile::default();
    let gauss = WeightProfile::gauss();
    assert_eq!(default.name, gauss.name);
    assert!((default.table - gauss.table).abs() < f64::EPSILON);
    assert!((default.join - gauss.join).abs() < f64::EPSILON);
    assert!((default.where_condition - gauss.where_condition).abs() < f64::EPSILON);
    assert!((default.subquery - gauss.subquery).abs() < f64::EPSILON);
    assert!((default.aggregate_function - gauss.aggregate_function).abs() < f64::EPSILON);
    assert!((default.case_expression - gauss.case_expression).abs() < f64::EPSILON);
    assert!((default.set_operation - gauss.set_operation).abs() < f64::EPSILON);
    assert!((default.group_by - gauss.group_by).abs() < f64::EPSILON);
    assert!((default.order_by - gauss.order_by).abs() < f64::EPSILON);
    assert!((default.window_function - gauss.window_function).abs() < f64::EPSILON);
    assert!((default.cte - gauss.cte).abs() < f64::EPSILON);
}

#[test]
fn test_weight_profile_oracle_constructs() {
    let oracle = WeightProfile::oracle();
    assert_eq!(oracle.name, "oracle");
    assert!(oracle.table > 0.0);
    assert!(oracle.join > 0.0);
}

#[test]
fn test_weight_profile_hive_constructs() {
    let hive = WeightProfile::hive();
    assert_eq!(hive.name, "hive");
    assert!(hive.table > 0.0);
    assert!(hive.join > 0.0);
}

// ============================================================================
// gauss_score_statement() with crafted metrics
// ============================================================================

#[test]
fn test_gauss_score_statement_all_zeros() {
    let metrics = ComplexityMetrics::default();
    let score = ogsql_complexity::engine::gauss_score_statement(&metrics);
    assert_eq!(score, 0);
}

#[test]
fn test_gauss_score_statement_basic() {
    let mut metrics = ComplexityMetrics::default();
    metrics.table_count = 1;
    metrics.where_condition_count = 1;
    let score = ogsql_complexity::engine::gauss_score_statement(&metrics);
    assert_eq!(score, 1 * TABLE + 1 * WHERE_CONDITION);
    assert_eq!(score, 15); // 10 + 5
}

#[test]
fn test_gauss_score_statement_with_joins() {
    let mut metrics = ComplexityMetrics::default();
    metrics.join_count = 2;
    metrics.subquery_count = 1;
    let score = ogsql_complexity::engine::gauss_score_statement(&metrics);
    assert_eq!(score, 2 * JOIN + 1 * SUBQUERY);
    assert_eq!(score, 50); // 30 + 20
}

#[test]
fn test_gauss_score_statement_with_hints() {
    let mut metrics = ComplexityMetrics::default();
    metrics.hint_count = 3;
    let score = ogsql_complexity::engine::gauss_score_statement(&metrics);
    assert_eq!(score, 3 * HINT);
    assert_eq!(score, 9); // 3 * 3
}

#[test]
fn test_gauss_score_statement_with_group_order() {
    let mut metrics = ComplexityMetrics::default();
    metrics.has_group_by = true;
    metrics.has_order_by = true;
    let score = ogsql_complexity::engine::gauss_score_statement(&metrics);
    assert_eq!(score, GROUP_BY + ORDER_BY);
    assert_eq!(score, 10); // 5 + 5
}

#[test]
fn test_gauss_score_statement_full_formula() {
    let mut metrics = ComplexityMetrics::default();
    metrics.table_count = 3;
    metrics.join_count = 2;
    metrics.where_condition_count = 4;
    metrics.subquery_count = 1;
    metrics.aggregate_function_count = 2;
    metrics.case_expression_count = 1;
    metrics.set_operation_count = 1;
    metrics.has_group_by = true;
    metrics.has_order_by = true;
    metrics.hint_count = 2;

    let score = ogsql_complexity::engine::gauss_score_statement(&metrics);
    let expected = 3 * TABLE
        + 2 * JOIN
        + 4 * WHERE_CONDITION
        + 1 * SUBQUERY
        + 2 * AGGREGATE_FUNCTION
        + 1 * CASE_EXPRESSION
        + 1 * SET_OPERATION
        + 1 * GROUP_BY
        + 1 * ORDER_BY
        + 2 * HINT;
    assert_eq!(score, expected);
}

// ============================================================================
// gauss_score_non_select()
// ============================================================================

#[test]
fn test_gauss_score_non_select_basic() {
    let mut metrics = ComplexityMetrics::default();
    metrics.table_count = 2;
    let score = ogsql_complexity::engine::gauss_score_non_select(&metrics);
    assert_eq!(score, 20); // 2 * 10
}

#[test]
fn test_gauss_score_non_select_with_hints() {
    let mut metrics = ComplexityMetrics::default();
    metrics.table_count = 1;
    metrics.hint_count = 2;
    let score = ogsql_complexity::engine::gauss_score_non_select(&metrics);
    assert_eq!(score, 1 * TABLE + 2 * HINT);
    assert_eq!(score, 16); // 10 + 6
}

#[test]
fn test_gauss_score_non_select_zeros() {
    let metrics = ComplexityMetrics::default();
    let score = ogsql_complexity::engine::gauss_score_non_select(&metrics);
    assert_eq!(score, 0);
}

// ============================================================================
// gauss_score_create_table()
// ============================================================================

#[test]
fn test_gauss_score_create_table_base_only() {
    let metrics = ComplexityMetrics::default();
    let score = ogsql_complexity::engine::gauss_score_create_table(&metrics);
    assert_eq!(score, TABLE_WEIGHT);
    assert_eq!(score, 10);
}

#[test]
fn test_gauss_score_create_table_with_columns() {
    let mut metrics = ComplexityMetrics::default();
    metrics.column_count = 5;
    let score = ogsql_complexity::engine::gauss_score_create_table(&metrics);
    assert_eq!(score, TABLE_WEIGHT + 5 * COLUMN);
    assert_eq!(score, 20); // 10 + 10
}

#[test]
fn test_gauss_score_create_table_with_computed_columns() {
    let mut metrics = ComplexityMetrics::default();
    metrics.computed_column_count = 2;
    let score = ogsql_complexity::engine::gauss_score_create_table(&metrics);
    assert_eq!(score, TABLE_WEIGHT + 2 * COMPUTED_COLUMN);
    assert_eq!(score, 40); // 10 + 30
}

#[test]
fn test_gauss_score_create_table_with_check_constraint() {
    let mut metrics = ComplexityMetrics::default();
    metrics.check_constraint_count = 1;
    let score = ogsql_complexity::engine::gauss_score_create_table(&metrics);
    assert_eq!(score, TABLE_WEIGHT + CHECK_CONSTRAINT);
    assert_eq!(score, 20); // 10 + 10
}

#[test]
fn test_gauss_score_create_table_full() {
    let mut metrics = ComplexityMetrics::default();
    metrics.column_count = 5;
    metrics.computed_column_count = 1;
    metrics.check_constraint_count = 2;
    let score = ogsql_complexity::engine::gauss_score_create_table(&metrics);
    let expected = TABLE_WEIGHT + 5 * COLUMN + 1 * COMPUTED_COLUMN + 2 * CHECK_CONSTRAINT;
    assert_eq!(score, expected);
    assert_eq!(score, 10 + 10 + 15 + 20); // = 55
}

// ============================================================================
// gauss_score_dynamic_sql()
// ============================================================================

#[test]
fn test_gauss_score_dynamic_sql_basic() {
    // log10(100) * 5 = 2 * 5 = 10, adjusted by (1 + 0) = 10
    let score = ogsql_complexity::engine::gauss_score_dynamic_sql(100, 0, 0);
    assert_eq!(score, 10);
}

#[test]
fn test_gauss_score_dynamic_sql_with_tables() {
    // log10(10000) * 5 = 4 * 5 = 20, adjusted by (1 + 0.1*5) = 20 * 1.5 = 30
    let score = ogsql_complexity::engine::gauss_score_dynamic_sql(10000, 5, 0);
    assert_eq!(score, 30);
}

#[test]
fn test_gauss_score_dynamic_sql_with_hints() {
    // log10(100) * 5 = 10, adjusted by (1 + 0.1*3) = 10 * 1.3 = 13, + 2*3 = 6 = 19
    let score = ogsql_complexity::engine::gauss_score_dynamic_sql(100, 3, 2);
    let base = (100_f64.log10() * 5.0) * (1.0 + 0.1 * 3.0);
    let expected = base as i64 + 2 * HINT;
    assert_eq!(score, expected);
}

// ============================================================================
// gauss_score_procedure() - via gauss_analyze() entry point
// Tests for the full procedure scoring formula indirectly
// ============================================================================

#[test]
fn test_gauss_complexity_level_boundaries() {
    // These test the internal gauss_complexity_level function indirectly
    // by using gauss_analyze and checking the resulting level

    // Trivial: score < 5
    let sql = "SELECT 1";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    // SELECT 1: table_count=0, no WHERE → score = 0 → Trivial
    assert_eq!(report.level, ComplexityLevel::Trivial);

    // Simple: 5 <= score < 15
    let sql = "SELECT * FROM t";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    // table_count=1 → score=10 → Simple
    assert_eq!(report.level, ComplexityLevel::Simple);
    assert_eq!(report.overall_score, 10);
}

// ============================================================================
// ComplexityConfig default
// ============================================================================

#[test]
fn test_complexity_config_default() {
    let config = ComplexityConfig::default();
    assert!(config.custom_functions.is_empty());
    assert!(config.high_weight_tables.is_empty());
    assert!(config.high_weight_procedures.is_empty());
    assert!(config.builtin_functions.is_empty());
}

// ============================================================================
// GaussDB score breakdown consistency
// ============================================================================

#[test]
fn test_gauss_procedure_breakdown_sums_match() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE breakdown_test()
AS $$
DECLARE
    cur CURSOR FOR SELECT id FROM users;
    v_sql VARCHAR := 'SELECT 1';
BEGIN
    OPEN cur;
    FETCH cur INTO v_id;
    CLOSE cur;

    FOR i IN 1..3 LOOP
        EXECUTE IMMEDIATE v_sql USING i;
    END LOOP;

    SAVEPOINT sp1;
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    let bd = &report.score_breakdown;

    // Each breakdown component should be weight × metric count
    assert_eq!(
        bd.loop_complexity,
        report.pl_metrics.loop_count as i64 * LOOP
            + report.pl_metrics.max_loop_nesting_level as i64 * NESTED_LOOP
    );
    assert_eq!(
        bd.dynamic_sql_complexity,
        report.pl_metrics.dynamic_sql_count as i64 * DYNAMIC_SQL
    );
    assert_eq!(
        bd.param_binding_complexity,
        report.pl_metrics.param_binding_count as i64 * PARAMETER_BINDING
    );
    assert!(bd.cursor_complexity > 0, "Should have cursor complexity");
    assert!(
        bd.transaction_complexity > 0,
        "Should have transaction complexity"
    );
}

// ============================================================================
// Error cases
// ============================================================================

#[test]
fn test_analyze_whitespace_only() {
    let result = analyze("   \n\t  ");
    assert!(result.is_err());
}

#[test]
fn test_gauss_analyze_whitespace_only() {
    let result = gauss_analyze("   \n\t  ", &ComplexityConfig::default());
    assert!(result.is_err());
}

#[test]
fn test_analyze_invalid_sql() {
    let result = analyze("NOT VALID SQL AT ALL {{{}}");
    // Parser may still parse something or return error — either way should not panic
    // Just verify it doesn't panic
    let _ = result;
}

// ============================================================================
// GaussDB weights are non-zero
// ============================================================================

#[test]
fn test_gauss_weights_non_zero() {
    assert!(TABLE > 0);
    assert!(JOIN > 0);
    assert!(WHERE_CONDITION > 0);
    assert!(SUBQUERY > 0);
    assert!(AGGREGATE_FUNCTION > 0);
    assert!(CASE_EXPRESSION > 0);
    assert!(SET_OPERATION > 0);
    assert!(GROUP_BY > 0);
    assert!(ORDER_BY > 0);
    assert!(LOOP > 0);
    assert!(NESTED_LOOP > 0);
    assert!(CUSTOM_FUNCTION > 0);
    assert!(HIGH_WEIGHT_TABLE > 0);
    assert!(HIGH_WEIGHT_PROCEDURE > 0);
    assert!(NESTED_PROCEDURE > 0);
    assert!(HINT > 0);
    assert!(CURSOR_DECLARATION > 0);
    assert!(CURSOR_OPERATION > 0);
    assert!(DYNAMIC_SQL > 0);
    assert!(PARAMETER_BINDING > 0);
    assert!(TRANSACTION_CONTROL > 0);
    assert!(AUTONOMOUS_TRANSACTION > 0);
    assert!(JAVA_PROCEDURE > 0);
}
