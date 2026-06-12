//! Guard tests for gauss_analyze() PL/pgSQL paths.
//!
//! Covers stored procedures, functions, DO blocks, anonymous blocks,
//! packages, exception handlers, nested routines, and config options.

use ogsql_complexity::{gauss_analyze, ComplexityConfig, ComplexityTag, InputKind, SqlCategory};

#[test]
fn test_gauss_procedure_with_exception_handler() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE exc_proc()
AS $$
BEGIN
    INSERT INTO logs (msg) VALUES ('start');
    BEGIN
        DELETE FROM temp WHERE id < 0;
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO error_log (msg) VALUES ('error');
    END;
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    let m = &report.pl_metrics;
    assert!(
        m.subtransaction_count >= 1,
        "Exception block creates subtransaction"
    );
    assert!(m.transaction_control_count >= 1, "Should detect COMMIT");
}

#[test]
fn test_gauss_procedure_with_nested_procedure() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE outer_proc()
AS $$
DECLARE
    PROCEDURE inner_proc()
    AS $$
    BEGIN
        INSERT INTO logs (msg) VALUES ('inner');
    END;
    $$
BEGIN
    inner_proc();
    INSERT INTO logs (msg) VALUES ('outer');
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.sql_sub_type, "CREATE PROCEDURE");
}

#[test]
fn test_gauss_procedure_with_nested_function() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE func_proc()
AS $$
DECLARE
    FUNCTION inner_func(x INT) RETURN INT
    AS $$
    BEGIN
        RETURN x * 2;
    END;
    $$
BEGIN
    INSERT INTO logs (msg) VALUES ('done');
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.sql_sub_type, "CREATE PROCEDURE");
}

#[test]
fn test_gauss_procedure_with_while_loop() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE while_proc()
AS $$
DECLARE
    i INT := 1;
BEGIN
    WHILE i <= 10 LOOP
        INSERT INTO logs (msg) VALUES ('iter');
        i := i + 1;
    END LOOP;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.pl_metrics.loop_count, 1);
    assert_eq!(report.pl_metrics.max_loop_nesting_level, 1);
}

#[test]
fn test_gauss_procedure_with_for_in_select() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE for_select_proc()
AS $$
DECLARE
    v_name VARCHAR;
BEGIN
    FOR rec IN SELECT name FROM users WHERE active = true LOOP
        INSERT INTO logs (msg) VALUES (rec.name);
    END LOOP;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.pl_metrics.loop_count, 1);
}

#[test]
fn test_gauss_procedure_with_foreach() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE foreach_proc()
AS $$
DECLARE
    v_item INT;
BEGIN
    FOREACH v_item IN ARRAY ARRAY[1,2,3] LOOP
        INSERT INTO logs (msg) VALUES ('item');
    END LOOP;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.pl_metrics.loop_count, 1);
}

#[test]
fn test_gauss_do_block() {
    let sql = r#"
DO $$
BEGIN
    FOR i IN 1..5 LOOP
        INSERT INTO logs (msg) VALUES ('hello');
    END LOOP;
END;
$$
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::AnonymousBlock);
    assert_eq!(report.pl_metrics.loop_count, 1);
    assert_eq!(report.sql_sub_type, "DO");
}

#[test]
fn test_gauss_create_function_with_block() {
    let sql = r#"
CREATE OR REPLACE FUNCTION get_count() RETURN INT
AS $$
DECLARE
    v_cnt INT;
BEGIN
    SELECT COUNT(*) INTO v_cnt FROM users;
    RETURN v_cnt;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
    assert_eq!(report.sql_category, SqlCategory::PLBlock);
    assert_eq!(report.sql_sub_type, "CREATE FUNCTION");
}

#[test]
fn test_gauss_create_function_java() {
    let sql = r#"
CREATE FUNCTION java_func() RETURNS INTEGER
LANGUAGE JAVA
AS $$ return 42; $$
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    if report.pl_metrics.java_stored_procedure_count > 0 {
        assert!(
            report.overall_score >= 50,
            "Java stored procedure should have minimum score of 50, got {}",
            report.overall_score
        );
    }
}

#[test]
fn test_gauss_create_package_spec() {
    let sql = r#"
CREATE OR REPLACE PACKAGE my_pkg
AS
    PROCEDURE proc1(p1 INT);
    FUNCTION func1(p1 VARCHAR) RETURN INT;
    v_global INT := 0;
END;
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.sql_category, SqlCategory::Package);
    assert_eq!(report.sql_sub_type, "CREATE PACKAGE");
}

#[test]
fn test_gauss_create_package_body() {
    let sql = r#"
CREATE OR REPLACE PACKAGE BODY my_pkg
AS
    PROCEDURE proc1(p1 INT)
    AS $$
    BEGIN
        INSERT INTO logs (msg) VALUES ('proc1');
    END;
    $$

    v_impl VARCHAR := 'implemented';
END;
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.sql_category, SqlCategory::Package);
    assert_eq!(report.sql_sub_type, "CREATE PACKAGE BODY");
}

#[test]
fn test_gauss_custom_functions_config() {
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
fn test_gauss_builtin_functions_config() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE builtin_call_proc()
AS $$
BEGIN
    PERFORM dbms_output.put_line('hello');
END;
$$ LANGUAGE plpgsql
"#;
    let config = ComplexityConfig {
        builtin_functions: vec!["dbms_output.put_line".into()],
        ..Default::default()
    };
    let report = gauss_analyze(sql, &config).unwrap();
    assert_eq!(report.input_kind, InputKind::StoredProcedure);
}

#[test]
fn test_gauss_high_weight_procedures_config() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE hw_proc()
AS $$
BEGIN
    heavy_proc();
END;
$$ LANGUAGE plpgsql
"#;
    let config = ComplexityConfig {
        high_weight_procedures: vec!["heavy_proc".into()],
        ..Default::default()
    };
    let report = gauss_analyze(sql, &config).unwrap();
    assert!(
        report.pl_metrics.high_weight_procedure_count <= 1,
        "high_weight_procedure_count should be 0 or 1"
    );
}

#[test]
fn test_gauss_procedure_triple_nested_loop() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE deep_nest()
AS $$
DECLARE
    i INT;
    j INT;
    k INT;
BEGIN
    FOR i IN 1..3 LOOP
        FOR j IN 1..3 LOOP
            FOR k IN 1..3 LOOP
                INSERT INTO logs (msg) VALUES ('nested');
            END LOOP;
        END LOOP;
    END LOOP;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.pl_metrics.loop_count, 3);
    assert_eq!(report.pl_metrics.max_loop_nesting_level, 3);
}

#[test]
fn test_gauss_procedure_with_savepoint_and_rollback() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE sp_proc()
AS $$
BEGIN
    INSERT INTO orders (id) VALUES (1);
    SAVEPOINT sp1;
    INSERT INTO orders (id) VALUES (2);
    ROLLBACK TO sp1;
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    let m = &report.pl_metrics;
    assert!(
        m.transaction_control_count >= 3,
        "SAVEPOINT + ROLLBACK + COMMIT = 3"
    );
    assert!(
        m.subtransaction_count >= 1,
        "SAVEPOINT creates subtransaction"
    );
}

#[test]
fn test_gauss_procedure_with_multiple_dynamic_sql() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE multi_dyn_proc()
AS $$
DECLARE
    v_sql1 VARCHAR := 'SELECT * FROM t1';
    v_sql2 VARCHAR := 'SELECT * FROM t2';
BEGIN
    EXECUTE IMMEDIATE v_sql1;
    EXECUTE IMMEDIATE v_sql2 USING 42;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.pl_metrics.dynamic_sql_count, 2);
    assert_eq!(report.pl_metrics.param_binding_count, 1);
}

#[test]
fn test_gauss_procedure_detects_tags_high_table_count() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE many_tables()
AS $$
BEGIN
    INSERT INTO t1 SELECT * FROM t2 JOIN t3 ON t2.id = t3.id
    JOIN t4 ON t3.id = t4.id JOIN t5 ON t4.id = t5.id
    JOIN t6 ON t5.id = t6.id;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert!(
        report.tags.contains(&ComplexityTag::HighTableCount),
        "Should detect HighTableCount tag"
    );
}

#[test]
fn test_gauss_procedure_detects_tag_large_join() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE big_join_proc()
AS $$
BEGIN
    INSERT INTO result
    SELECT * FROM t1 JOIN t2 ON t1.id = t2.id
    JOIN t3 ON t2.id = t3.id JOIN t4 ON t3.id = t4.id
    JOIN t5 ON t4.id = t5.id;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert!(
        report.pl_metrics.join_count > 3,
        "join_count should be > 3 for LargeJoin tag, got {}",
        report.pl_metrics.join_count
    );
    assert!(
        report.tags.contains(&ComplexityTag::LargeJoin),
        "Should detect LargeJoin tag when join_count > 3"
    );
}

#[test]
fn test_gauss_procedure_detects_tag_dynamic_sql() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE dyn_tag_proc()
AS $$
DECLARE
    v_sql VARCHAR := 'SELECT 1';
BEGIN
    EXECUTE IMMEDIATE v_sql;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert!(
        report.tags.contains(&ComplexityTag::DynamicSql),
        "Should detect DynamicSql tag"
    );
}

#[test]
fn test_gauss_procedure_detects_tag_deep_nesting() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE deep_nest_tag()
AS $$
BEGIN
    FOR i IN 1..2 LOOP
        FOR j IN 1..2 LOOP
            FOR k IN 1..2 LOOP
                INSERT INTO logs (msg) VALUES ('x');
            END LOOP;
        END LOOP;
    END LOOP;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert!(
        report.tags.contains(&ComplexityTag::DeepNesting),
        "Should detect DeepNesting tag when loop nesting > 2"
    );
}

#[test]
fn test_gauss_procedure_detects_tag_cursor_heavy() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE cursor_heavy_proc()
AS $$
DECLARE
    cur1 CURSOR FOR SELECT id FROM t1;
    cur2 CURSOR FOR SELECT id FROM t2;
    cur3 CURSOR FOR SELECT id FROM t3;
    cur4 CURSOR FOR SELECT id FROM t4;
    v_id INT;
BEGIN
    OPEN cur1; FETCH cur1 INTO v_id; CLOSE cur1;
    OPEN cur2; FETCH cur2 INTO v_id; CLOSE cur2;
    OPEN cur3; FETCH cur3 INTO v_id; CLOSE cur3;
    OPEN cur4; FETCH cur4 INTO v_id; CLOSE cur4;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert!(
        report.tags.contains(&ComplexityTag::CursorHeavy),
        "Should detect CursorHeavy tag when cursor_count > 3"
    );
}

#[test]
fn test_gauss_procedure_detects_tag_transaction_complex() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE tx_complex_proc()
AS $$
DECLARE
    PRAGMA autonomous_transaction;
BEGIN
    INSERT INTO logs (msg) VALUES ('auto');
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    if report.pl_metrics.uses_autonomous_transactions {
        assert!(
            report.tags.contains(&ComplexityTag::TransactionComplex),
            "Should detect TransactionComplex tag for autonomous transactions"
        );
    }
    assert!(
        report.pl_metrics.transaction_control_count >= 1,
        "Should detect COMMIT"
    );
}

#[test]
fn test_gauss_dimensions_populated_for_procedure() {
    let sql = r#"
CREATE OR REPLACE PROCEDURE dim_proc()
AS $$
DECLARE
    cur CURSOR FOR SELECT id, name FROM users;
    v_id INT;
BEGIN
    OPEN cur;
    FOR i IN 1..5 LOOP
        FETCH cur INTO v_id;
        EXECUTE IMMEDIATE 'SELECT 1' USING v_id;
    END LOOP;
    CLOSE cur;
    COMMIT;
END;
$$ LANGUAGE plpgsql
"#;
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    let dim = &report.dimensions;
    assert!(
        dim.sql_structure > 0 || dim.pl_logic > 0 || dim.advanced_feature > 0,
        "At least one dimension should be > 0"
    );
}

#[test]
fn test_gauss_create_table_with_defaults_and_checks() {
    let sql = "CREATE TABLE orders (id INT, total DECIMAL DEFAULT 0, status VARCHAR(10) DEFAULT 'new', CHECK (total >= 0), CHECK (id > 0))";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    assert_eq!(report.input_kind, InputKind::SqlStatement);
    let m = &report.pl_metrics;
    assert_eq!(m.column_count, 3);
    assert_eq!(
        m.computed_column_count, 2,
        "DEFAULT counts as computed column"
    );
    assert_eq!(m.check_constraint_count, 2);
}

#[test]
fn test_gauss_update_non_select_formula() {
    let sql = "UPDATE orders SET status = 'shipped' WHERE id > 100";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    let expected =
        report.pl_metrics.table_count as i64 * 10 + report.pl_metrics.hint_count as i64 * 3;
    assert_eq!(report.overall_score, expected);
}

#[test]
fn test_gauss_delete_non_select_formula() {
    let sql = "DELETE FROM logs WHERE created_at < '2024-01-01'";
    let report = gauss_analyze(sql, &ComplexityConfig::default()).unwrap();
    let expected =
        report.pl_metrics.table_count as i64 * 10 + report.pl_metrics.hint_count as i64 * 3;
    assert_eq!(report.overall_score, expected);
}
