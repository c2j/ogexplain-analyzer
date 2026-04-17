use ogsql_complexity::model::StatementTypeMultiplier;
use ogsql_complexity::{analyze, ComplexityLevel};

#[test]
fn test_simple_select() {
    let sql = "SELECT * FROM users WHERE id = 1";
    let report = analyze(sql).unwrap();
    assert_eq!(report.statements.len(), 1);
    let s = &report.statements[0];
    // 1 table (users), 1 where condition (id = 1)
    assert!(s.metrics.table_count >= 1, "Should have at least 1 table");
    assert!(s.raw_score > 0.0, "Score should be positive");
}

#[test]
fn test_join_query() {
    let sql =
        "SELECT u.name, o.total FROM users u JOIN orders o ON u.id = o.user_id WHERE o.total > 100";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    // 2 tables, 1 join, 1 where condition
    // raw_score = 2*1.0 + 1*2.0 + 1*1.0 = 5.0
    assert!(s.metrics.table_count >= 2);
    assert!(s.metrics.join_count >= 1);
    assert!(s.raw_score > 3.0);
}

#[test]
fn test_multi_join() {
    let sql = r#"
        SELECT u.name, o.total, p.name
        FROM users u
        JOIN orders o ON u.id = o.user_id
        JOIN products p ON o.product_id = p.id
        WHERE o.total > 100 AND u.active = true
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    // 3 tables, 2 joins, 3 where conditions (AND + 2 leaf conditions)
    assert!(s.metrics.table_count >= 3);
    assert!(s.metrics.join_count >= 2);
}

#[test]
fn test_subquery() {
    let sql = "SELECT * FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > 1000)";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    // 2 tables (users + orders from subquery), 1 subquery, 2 where conditions (outer IN + inner comparison)
    // raw_score = 2*1.0 + 2*1.0 + 1*3.0 = 7.0
    assert!(s.metrics.subquery_count >= 1);
    assert!(s.raw_score > 5.0);
}

#[test]
fn test_aggregate_and_group_by() {
    let sql = "SELECT department, COUNT(*), AVG(salary) FROM employees GROUP BY department ORDER BY department";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    // 2 aggregate functions (COUNT, AVG), has_group_by=true, has_order_by=true
    assert!(s.metrics.aggregate_function_count >= 2);
    assert!(s.metrics.has_group_by);
    assert!(s.metrics.has_order_by);
}

#[test]
fn test_case_expression() {
    let sql = r#"
        SELECT name, CASE WHEN score >= 90 THEN 'A' WHEN score >= 80 THEN 'B' ELSE 'C' END AS grade
        FROM students
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.case_expression_count >= 1);
}

#[test]
fn test_union() {
    let sql = "SELECT id, name FROM customers UNION ALL SELECT id, name FROM suppliers";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.set_operation_count >= 1);
}

#[test]
fn test_insert_statement() {
    let sql = "INSERT INTO logs (user_id, action) SELECT user_id, 'login' FROM sessions WHERE active = true";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, StatementTypeMultiplier::Insert);
    assert!(s.adjusted_score > 0.0);
}

#[test]
fn test_update_with_multiplier() {
    let sql = "UPDATE orders SET status = 'shipped' WHERE created_at > '2024-01-01'";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, StatementTypeMultiplier::Update);
    assert!((s.statement_type.multiplier() - 1.2).abs() < 0.001);
}

#[test]
fn test_empty_input() {
    let result = analyze("");
    assert!(result.is_err());
}

#[test]
fn test_weighted_breakdown_sums_correctly() {
    let sql = "SELECT * FROM users u JOIN orders o ON u.id = o.user_id WHERE u.active = true";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    let b = &s.weighted_breakdown;
    let sum = b.tables
        + b.joins
        + b.where_conditions
        + b.subqueries
        + b.aggregate_functions
        + b.case_expressions
        + b.set_operations
        + b.group_by
        + b.order_by
        + b.window_functions
        + b.ctes;
    assert!(
        (sum - s.raw_score).abs() < 0.001,
        "Breakdown sum {} != raw_score {}",
        sum,
        s.raw_score
    );
}

#[test]
fn test_complexity_levels() {
    let report = analyze("SELECT 1").unwrap();
    assert!(
        matches!(report.statements[0].level, ComplexityLevel::Trivial),
        "SELECT 1 should be Trivial, got {:?}",
        report.statements[0].level
    );

    let sql = "SELECT u.name, COUNT(*) FROM users u JOIN orders o ON u.id = o.user_id GROUP BY u.name ORDER BY COUNT(*) DESC";
    let report = analyze(sql).unwrap();
    let level = report.statements[0].level;
    assert!(
        !matches!(level, ComplexityLevel::Trivial),
        "Complex query should not be Trivial, got {:?}",
        level
    );
}

#[test]
fn test_delete_statement() {
    let sql = "DELETE FROM sessions WHERE last_active < '2024-01-01'";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, StatementTypeMultiplier::Delete);
    assert!((s.statement_type.multiplier() - 1.1).abs() < 0.001);
}

#[test]
fn test_multiple_statements() {
    let sql = "SELECT 1; SELECT * FROM users";
    let report = analyze(sql).unwrap();
    assert!(
        report.statements.len() >= 2,
        "Should parse multiple statements, got {}",
        report.statements.len()
    );
}

#[test]
fn test_cte() {
    let sql = r#"
        WITH active_users AS (
            SELECT id, name FROM users WHERE active = true
        )
        SELECT u.name, o.total
        FROM active_users u
        JOIN orders o ON u.id = o.user_id
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.cte_count >= 1, "Should detect at least 1 CTE");
    // CTE name "active_users" should not be counted as a real table
    // (visitor checks cte_names set)
    assert!(
        s.metrics.table_count >= 1,
        "Should count orders table, got {}",
        s.metrics.table_count
    );
}

#[test]
fn test_window_function() {
    let sql = "SELECT name, salary, ROW_NUMBER() OVER (ORDER BY salary DESC) FROM employees";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(
        s.metrics.window_function_count >= 1,
        "Should detect window function"
    );
}

#[test]
fn test_subquery_depth() {
    let sql = r#"
        SELECT * FROM users WHERE id IN (
            SELECT user_id FROM orders WHERE product_id IN (
                SELECT id FROM products WHERE category = 'electronics'
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
}

#[test]
fn test_overall_score_is_max_adjusted() {
    let sql = "SELECT 1; SELECT * FROM users u JOIN orders o ON u.id = o.user_id";
    let report = analyze(sql).unwrap();
    let max_adjusted = report
        .statements
        .iter()
        .map(|s| s.adjusted_score)
        .fold(0.0_f64, f64::max);
    assert!(
        (report.overall_score - max_adjusted).abs() < 0.001,
        "overall_score {} should equal max adjusted {}",
        report.overall_score,
        max_adjusted
    );
}

#[test]
fn test_insert_from_values() {
    let sql = "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert_eq!(s.statement_type, StatementTypeMultiplier::Insert);
    // Insert handler adds +1 for the target table
    assert!(s.metrics.table_count >= 1);
}

#[test]
fn test_complex_query_reaches_moderate() {
    let sql = r#"
        SELECT u.name, d.name AS department, COUNT(*) AS order_count, AVG(o.total) AS avg_total
        FROM users u
        JOIN orders o ON u.id = o.user_id
        JOIN departments d ON u.dept_id = d.id
        WHERE u.active = true AND o.created_at > '2024-01-01'
        GROUP BY u.name, d.name
        ORDER BY order_count DESC
    "#;
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    // 3 tables, 2 joins, 3 where conditions (AND + 2 leaves), 2 agg, group_by, order_by
    // raw = 3*1.0 + 2*2.0 + 3*1.0 + 2*1.5 + 1.5 + 1.0 = 3+4+3+3+1.5+1.0 = 15.5
    assert!(
        !matches!(s.level, ComplexityLevel::Trivial | ComplexityLevel::Simple),
        "Should be at least Moderate, got {:?} (score={})",
        s.level,
        s.raw_score
    );
}

#[test]
fn test_distinct_flag() {
    let sql = "SELECT DISTINCT department FROM employees";
    let report = analyze(sql).unwrap();
    let s = &report.statements[0];
    assert!(s.metrics.has_distinct, "Should detect DISTINCT");
}
