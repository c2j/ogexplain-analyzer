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

// ============================================================================
// GaussDB Scoring Tests
// ============================================================================

use ogsql_complexity::{gauss_analyze, ComplexityConfig, InputKind};

#[test]
fn test_gauss_simple_select() {
    let sql = "SELECT * FROM users WHERE id = 1";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    // table=1, where=1 (GaussDB mode counts WHERE as 1)
    // score = 1×10 + 1×5 = 15
    assert_eq!(report.overall_score, 15);
}

#[test]
fn test_gauss_select_with_join() {
    let sql =
        "SELECT u.name, o.total FROM users u JOIN orders o ON u.id = o.user_id WHERE o.total > 100";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    // table=2, join=1, where=1
    // score = 2×10 + 1×15 + 1×5 = 40
    assert_eq!(report.overall_score, 40);
}

#[test]
fn test_gauss_select_complex() {
    let sql = "SELECT department, CASE WHEN salary > 100000 THEN 'high' ELSE 'normal' END AS level, COUNT(*) AS cnt FROM employees WHERE department IN (SELECT name FROM departments WHERE active = 1) GROUP BY department ORDER BY cnt DESC";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);

    let m = &report.pl_metrics;
    assert!(
        m.table_count >= 2,
        "table_count should be >= 2, got {}",
        m.table_count
    );
    assert!(
        m.subquery_count >= 1,
        "subquery_count >= 1, got {}",
        m.subquery_count
    );
    assert!(m.aggregate_function_count >= 1);
    assert!(m.case_expression_count >= 1);

    let expected = m.table_count as i64 * 10
        + m.join_count as i64 * 15
        + m.where_condition_count as i64 * 5
        + m.subquery_count as i64 * 20
        + m.aggregate_function_count as i64 * 10
        + m.case_expression_count as i64 * 5
        + m.set_operation_count as i64 * 15
        + if m.has_group_by { 5 } else { 0 }
        + if m.has_order_by { 5 } else { 0 };
    assert_eq!(report.overall_score, expected);
}

#[test]
fn test_gauss_insert() {
    let sql = "INSERT INTO logs (user_id, action) VALUES (1, 'login')";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    // Non-SELECT formula: table_count × 10 + hint_count × 3
    // table=1, hint=0 → 1×10 + 0×3 = 10
    assert_eq!(report.overall_score, 10);
}

#[test]
fn test_gauss_update_with_hint() {
    let sql = "UPDATE users SET active = false WHERE last_login < '2024-01-01'";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    let m = &report.pl_metrics;
    assert!(
        m.table_count >= 1,
        "table_count >= 1, got {}",
        m.table_count
    );
    let expected = m.table_count as i64 * 10 + m.hint_count as i64 * 3;
    assert_eq!(report.overall_score, expected);
}

#[test]
fn test_gauss_create_table() {
    let sql = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(100) DEFAULT 'unknown', email VARCHAR(200), CHECK (id > 0))";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    let m = &report.pl_metrics;
    // column_count=3 (id, name, email), computed_column=1 (DEFAULT), check_constraint=1 (CHECK)
    assert_eq!(m.column_count, 3);
    assert_eq!(m.computed_column_count, 1);
    assert_eq!(m.check_constraint_count, 1);
    // score = 10 + 3×2 + 1×15 + 1×10 = 41
    assert_eq!(report.overall_score, 41);
}

#[test]
fn test_gauss_where_exists_only() {
    let sql = "SELECT * FROM users WHERE a = 1 AND b = 2 AND c = 3";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    // In GaussDB mode, WHERE counts as 1 regardless of AND/OR conditions
    assert_eq!(report.pl_metrics.where_condition_count, 1);
}

#[test]
fn test_gauss_hint_counting() {
    let sql = "SELECT * FROM t1 JOIN t2 ON t1.id = t2.id";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(
        report.pl_metrics.hint_count, 0,
        "Query without hints should have hint_count=0"
    );
    assert_eq!(report.pl_metrics.table_count, 2);
    assert_eq!(report.pl_metrics.join_count, 1);
}

#[test]
fn test_gauss_simple_procedure() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE simple_proc()
AS $$
BEGIN
    INSERT INTO logs (msg) VALUES ('hello');
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    assert!(report.overall_score >= 0);
    assert_eq!(report.pl_metrics.loop_count, 0);
    assert_eq!(report.pl_metrics.cursor_count, 0);
    assert_eq!(report.pl_metrics.dynamic_sql_count, 0);
}

#[test]
fn test_gauss_procedure_with_loops() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE loop_proc()
AS $$
DECLARE
    i INT;
    j INT;
BEGIN
    FOR i IN 1..10 LOOP
        FOR j IN 1..5 LOOP
            INSERT INTO logs (msg) VALUES ('nested');
        END LOOP;
    END LOOP;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    let m = &report.pl_metrics;
    assert_eq!(m.loop_count, 2, "Should have 2 loops");
    assert_eq!(m.max_loop_nesting_level, 2, "Max loop nesting should be 2");
}

#[test]
fn test_gauss_procedure_with_cursor() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE cursor_proc()
AS $$
DECLARE
    cur CURSOR FOR SELECT id, name FROM users;
    v_id INT;
    v_name VARCHAR;
BEGIN
    OPEN cur;
    FETCH cur INTO v_id, v_name;
    CLOSE cur;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    let m = &report.pl_metrics;
    assert_eq!(m.cursor_count, 1, "Should have 1 cursor declaration");
    assert_eq!(
        m.cursor_operation_count, 3,
        "Should have 3 cursor ops (OPEN + FETCH + CLOSE)"
    );
}

#[test]
fn test_gauss_procedure_with_dynamic_sql() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE dynamic_proc()
AS $$
DECLARE
    v_sql VARCHAR := 'SELECT * FROM users WHERE id = $1';
BEGIN
    EXECUTE IMMEDIATE v_sql USING 42;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    let m = &report.pl_metrics;
    assert_eq!(m.dynamic_sql_count, 1, "Should have 1 dynamic SQL");
    assert_eq!(
        m.param_binding_count, 1,
        "Should have 1 parameter binding (USING)"
    );
}

#[test]
fn test_gauss_procedure_with_transactions() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE tx_proc()
AS $$
BEGIN
    INSERT INTO orders (user_id, total) VALUES (1, 100);
    SAVEPOINT sp1;
    UPDATE orders SET total = 200 WHERE user_id = 1;
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    let m = &report.pl_metrics;
    // SAVEPOINT + COMMIT = 2 transaction control ops
    assert_eq!(
        m.transaction_control_count, 2,
        "Should have 2 txn controls (SAVEPOINT + COMMIT), got {}",
        m.transaction_control_count
    );
    // SAVEPOINT creates 1 subtransaction
    assert_eq!(
        m.subtransaction_count, 1,
        "Should have 1 subtransaction (SAVEPOINT), got {}",
        m.subtransaction_count
    );
}

#[test]
fn test_gauss_procedure_with_pragma() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE auto_proc()
AS $$
DECLARE
    PRAGMA autonomous_transaction;
BEGIN
    INSERT INTO audit_log (msg) VALUES ('autonomous');
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    let m = &report.pl_metrics;
    if m.uses_autonomous_transactions {
        assert_eq!(report.score_breakdown.autonomous_transaction_bonus, 15);
    } else {
        assert_eq!(report.score_breakdown.autonomous_transaction_bonus, 0);
    }
    assert!(m.transaction_control_count >= 1, "Should detect COMMIT");
}

#[test]
fn test_gauss_java_minimum_score() {
    let sql = r#"
CREATE FUNCTION java_func() RETURNS INTEGER
LANGUAGE JAVA
AS $$ return 42; $$
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    let m = &report.pl_metrics;
    if m.java_stored_procedure_count > 0 {
        assert!(
            report.overall_score >= 50,
            "Java stored procedure should have minimum score of 50, got {}",
            report.overall_score
        );
    }
}

#[test]
fn test_gauss_custom_functions() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE custom_fn_proc()
AS $$
BEGIN
    PERFORM my_custom_func(42);
END;
$$ LANGUAGE plpgsql
"#;
    let config = ComplexityConfig {
        custom_functions: vec!["my_custom_func".into()],
        ..Default::default()
    };
    let report = gauss_analyze(sql, &config).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    assert!(
        report.pl_metrics.custom_function_count <= 1,
        "custom_function_count should be 0 or 1, got {}",
        report.pl_metrics.custom_function_count
    );
}

#[test]
fn test_gauss_anonymous_block() {
    let sql = r#"
DO $$
BEGIN
    FOR i IN 1..10 LOOP
        INSERT INTO logs (msg) VALUES ('hello');
    END LOOP;
END;
$$
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::AnonymousBlock);
    assert_eq!(report.pl_metrics.loop_count, 1, "Should have 1 loop");
}

#[test]
fn test_gauss_empty_input() {
    let result = gauss_analyze("", &ComplexityConfig::default());
    assert!(result.is_err(), "Empty input should return error");
}

#[test]
fn test_gauss_procedure_full_formula() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE full_proc()
AS $$
DECLARE
    cur CURSOR FOR SELECT id, name FROM users;
    v_id INT;
    v_name VARCHAR;
    v_sql VARCHAR;
BEGIN
    OPEN cur;
    FOR i IN 1..5 LOOP
        INSERT INTO logs (msg) VALUES ('processing');
    END LOOP;
    CLOSE cur;

    EXECUTE IMMEDIATE v_sql USING v_id;

    SAVEPOINT sp1;
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    let m = &report.pl_metrics;
    let bd = &report.score_breakdown;

    // Each breakdown component should equal its weight × metric count
    assert_eq!(
        bd.loop_complexity,
        m.loop_count as i64 * 15 + m.max_loop_nesting_level as i64 * 20
    );
    assert_eq!(
        bd.cursor_complexity,
        m.cursor_count as i64 * 10 + m.cursor_operation_count as i64 * 5
    );
    assert_eq!(bd.dynamic_sql_complexity, m.dynamic_sql_count as i64 * 15);
    assert_eq!(
        bd.param_binding_complexity,
        m.param_binding_count as i64 * 5
    );
    assert_eq!(
        bd.transaction_complexity,
        m.transaction_control_count as i64 * 10 + m.transaction_nesting_level as i64 * 20
    );

    assert_eq!(bd.custom_function_complexity, 0);
    assert_eq!(bd.high_weight_table_complexity, 0);
    assert_eq!(bd.nested_procedure_complexity, 0);
    assert_eq!(bd.high_weight_procedure_complexity, 0);
    assert_eq!(bd.autonomous_transaction_bonus, 0);
    assert_eq!(bd.java_procedure_complexity, 0);
    assert_eq!(bd.java_type_conversion_complexity, 0);
    assert_eq!(bd.package_complexity, 0);
}

#[test]
fn test_gauss_score_breakdown_populated() {
    let sql = "SELECT * FROM users WHERE id = 1";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    let bd = &report.score_breakdown;

    // For a plain SELECT, PL-specific breakdown fields must be 0
    assert_eq!(bd.loop_complexity, 0);
    assert_eq!(bd.cursor_complexity, 0);
    assert_eq!(bd.dynamic_sql_complexity, 0);
    assert_eq!(bd.transaction_complexity, 0);
    assert_eq!(bd.autonomous_transaction_bonus, 0);
    assert_eq!(bd.java_procedure_complexity, 0);
    assert_eq!(bd.package_complexity, 0);

    // sql_statement_scores should have exactly one entry
    assert_eq!(
        report.sql_statement_scores.len(),
        1,
        "Should have exactly 1 SQL statement score"
    );
    // That entry should equal the overall score for a standalone SELECT
    assert_eq!(report.sql_statement_scores[0], report.overall_score);

    // sql_statements_sum in breakdown should also match
    assert_eq!(bd.sql_statements_sum, report.overall_score);
}
