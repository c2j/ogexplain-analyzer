//! Guard tests for analyze() SQL statement paths.
//!
//! Covers SQL statement types and features NOT tested by complexity_tests.rs.

use ogsql_complexity::model::StatementTypeMultiplier;
use ogsql_complexity::{analyze, gauss_analyze, ComplexityConfig, InputKind};

#[test]
fn test_merge_statement() {
    let sql = "MERGE INTO target_t t USING source_t s ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.val = s.val";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, StatementTypeMultiplier::Merge);
    assert!((s.statement_type.multiplier() - 1.5).abs() < 0.001);
    assert!(s.metrics.table_count >= 2, "MERGE should count 2 tables");
    assert!(s.metrics.join_count >= 1, "MERGE should count as 1 join");
}

#[test]
fn test_exists_subquery() {
    let sql =
        "SELECT * FROM orders WHERE EXISTS (SELECT 1 FROM items WHERE items.order_id = orders.id)";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.subquery_count >= 1);
    assert!(s.metrics.subquery_depth >= 1);
}

#[test]
fn test_scalar_subquery_in_select() {
    let sql = "SELECT u.name, (SELECT COUNT(*) FROM orders WHERE user_id = u.id) FROM users u";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.subquery_count >= 1);
    assert!(s.metrics.aggregate_function_count >= 1);
}

#[test]
fn test_multiple_ctes() {
    let sql = r#"
        WITH
            active_users AS (SELECT id FROM users WHERE active = true),
            premium_users AS (SELECT id FROM users WHERE tier = 'premium')
        SELECT * FROM active_users a JOIN premium_users p ON a.id = p.id
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.cte_count >= 2, "Should detect 2 CTEs");
}

#[test]
fn test_group_by_rollup() {
    let sql = "SELECT department, role, COUNT(*) FROM employees GROUP BY ROLLUP(department, role)";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.has_group_by);
    assert!(s.metrics.aggregate_function_count >= 1);
}

#[test]
fn test_group_by_cube() {
    let sql = "SELECT department, role, COUNT(*) FROM employees GROUP BY CUBE(department, role)";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.has_group_by);
}

#[test]
fn test_deeply_nested_subqueries() {
    let sql = r#"
        SELECT * FROM t1 WHERE id IN (
            SELECT t1_id FROM t2 WHERE x IN (
                SELECT t2_id FROM t3 WHERE y > 0
            )
        )
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(
        s.metrics.subquery_depth >= 2,
        "Nested subqueries should have depth >= 2, got {}",
        s.metrics.subquery_depth
    );
    assert!(s.metrics.subquery_count >= 2);
}

#[test]
fn test_multiple_set_operations() {
    let sql = "SELECT id FROM a UNION ALL SELECT id FROM b UNION ALL SELECT id FROM c";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(
        s.metrics.set_operation_count >= 1,
        "Should detect set operations, got {}",
        s.metrics.set_operation_count
    );
}

#[test]
fn test_insert_with_returning() {
    let sql = "INSERT INTO users (name) VALUES ('test') RETURNING id, name";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, StatementTypeMultiplier::Insert);
    assert!(s.metrics.table_count >= 1);
}

#[test]
fn test_delete_with_subquery() {
    let sql = "DELETE FROM logs WHERE user_id IN (SELECT id FROM users WHERE inactive = true)";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, StatementTypeMultiplier::Delete);
    assert!(s.metrics.subquery_count >= 1);
}

#[test]
fn test_window_function_with_partition_and_order() {
    let sql = "SELECT name, department, salary, RANK() OVER (PARTITION BY department ORDER BY salary DESC) FROM employees";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.window_function_count >= 1);
}

#[test]
fn test_multiple_aggregate_functions() {
    let sql = "SELECT COUNT(*), SUM(amount), AVG(amount), MIN(amount), MAX(amount) FROM orders";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(
        s.metrics.aggregate_function_count >= 5,
        "Should detect 5 aggregates, got {}",
        s.metrics.aggregate_function_count
    );
}

#[test]
fn test_multiple_case_expressions() {
    let sql = r#"
        SELECT
            CASE WHEN x > 1 THEN 'a' ELSE 'b' END,
            CASE WHEN y > 2 THEN 'c' ELSE 'd' END
        FROM t
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(
        s.metrics.case_expression_count >= 2,
        "Should detect 2 CASE expressions"
    );
}

#[test]
fn test_explain_wrapping() {
    let sql = "EXPLAIN SELECT * FROM users WHERE id = 1";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(
        s.metrics.table_count >= 1,
        "EXPLAIN should unwrap and count inner table"
    );
    assert!(
        s.metrics.where_condition_count >= 1,
        "EXPLAIN should unwrap and count WHERE"
    );
}

#[test]
fn test_self_join() {
    let sql =
        "SELECT e.name, m.name AS manager FROM employees e JOIN employees m ON e.manager_id = m.id";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.table_count >= 2, "Self-join counts 2 table refs");
    assert!(s.metrics.join_count >= 1);
}

#[test]
fn test_gauss_merge_statement() {
    let sql = "MERGE INTO target_t t USING source_t s ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.val = s.val";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    assert!(report.pl_metrics.table_count >= 2);
    assert!(report.pl_metrics.join_count >= 1);
    let expected =
        report.pl_metrics.table_count as i64 * 10 + report.pl_metrics.hint_count as i64 * 3;
    assert_eq!(report.overall_score, expected);
}

#[test]
fn test_gauss_explain_wrapping() {
    let sql = "EXPLAIN SELECT * FROM users WHERE id = 1";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    let _ = report.pl_metrics.table_count;
}

#[test]
fn test_gauss_multiple_sql_statements() {
    let sql = "SELECT * FROM t1; INSERT INTO t2 (a) VALUES (1)";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    assert!(
        report.sql_statement_scores.len() >= 2,
        "Should have 2 statement scores"
    );
    assert!(report.overall_score > 0, "Overall score should be positive");
}

#[test]
fn test_distinct_flag_in_gauss() {
    let sql = "SELECT DISTINCT department FROM employees";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert!(report.pl_metrics.has_distinct);
}
