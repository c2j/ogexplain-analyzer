use crate::model::gauss_weights::*;
use crate::model::*;
use crate::pl_visitor;
use crate::visitor;
use ogsql_parser::ast::Statement;

pub fn compute_score(
    metrics: &ComplexityMetrics,
    profile: &WeightProfile,
) -> (f64, WeightedBreakdown, ComplexityLevel) {
    let breakdown = WeightedBreakdown {
        tables: metrics.table_count as f64 * profile.table,
        joins: metrics.join_count as f64 * profile.join,
        where_conditions: metrics.where_condition_count as f64 * profile.where_condition,
        subqueries: metrics.subquery_count as f64 * profile.subquery,
        aggregate_functions: metrics.aggregate_function_count as f64 * profile.aggregate_function,
        case_expressions: metrics.case_expression_count as f64 * profile.case_expression,
        set_operations: metrics.set_operation_count as f64 * profile.set_operation,
        group_by: if metrics.has_group_by {
            profile.group_by
        } else {
            0.0
        },
        order_by: if metrics.has_order_by {
            profile.order_by
        } else {
            0.0
        },
        window_functions: metrics.window_function_count as f64 * profile.window_function,
        ctes: metrics.cte_count as f64 * profile.cte,
    };

    let raw_score = breakdown.tables
        + breakdown.joins
        + breakdown.where_conditions
        + breakdown.subqueries
        + breakdown.aggregate_functions
        + breakdown.case_expressions
        + breakdown.set_operations
        + breakdown.group_by
        + breakdown.order_by
        + breakdown.window_functions
        + breakdown.ctes;

    let level = ComplexityLevel::from_score(raw_score);
    (raw_score, breakdown, level)
}

pub fn adjust_score(raw_score: f64, stmt_type: StatementTypeMultiplier) -> f64 {
    raw_score * stmt_type.multiplier()
}

pub fn overall_score(statements: &[StatementComplexity]) -> (f64, ComplexityLevel) {
    let max_adjusted = statements
        .iter()
        .map(|s| s.adjusted_score)
        .fold(0.0_f64, f64::max);

    (max_adjusted, ComplexityLevel::from_score(max_adjusted))
}

#[derive(Debug, thiserror::Error)]
pub enum ComplexityError {
    #[error("SQL parse error: {0}")]
    ParseError(String),
    #[error("Empty input")]
    EmptyInput,
}

pub fn analyze(sql: &str) -> Result<ComplexityReport, ComplexityError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(ComplexityError::EmptyInput);
    }

    let (infos, parse_errors) = ogsql_parser::Parser::parse_sql(trimmed);

    if infos.is_empty() {
        if !parse_errors.is_empty() {
            return Err(ComplexityError::ParseError(
                parse_errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        return Err(ComplexityError::EmptyInput);
    }

    let profile = WeightProfile::default();
    let mut statements = Vec::new();

    for info in &infos {
        let stmt_type = visitor::statement_type(&info.statement);
        let metrics = visitor::analyze_statement(&info.statement);
        let (raw_score, weighted_breakdown, _) = compute_score(&metrics, &profile);
        let adjusted_score = adjust_score(raw_score, stmt_type);

        statements.push(StatementComplexity {
            sql_text: info.sql_text.clone(),
            statement_type: stmt_type,
            metrics,
            weighted_breakdown,
            raw_score,
            adjusted_score,
            level: ComplexityLevel::from_score(adjusted_score),
        });
    }

    let (overall_score, overall_level) = overall_score(&statements);

    Ok(ComplexityReport {
        statements,
        overall_score,
        overall_level,
        profile: profile.name,
    })
}

/// GaussDB SELECT statement scoring (step 1 formula).
///
/// ```text
/// score = (tableCount × 10) + (joinCount × 15) + (whereConditionCount × 5)
///       + (subqueryCount × 20) + (aggregateFunctionCount × 10) + (caseExpressionCount × 5)
///       + (setOperationCount × 15) + (groupByCount × 5) + (orderByCount × 5)
///       + (hintCount × 3)
/// ```
pub fn gauss_score_statement(metrics: &ComplexityMetrics) -> i64 {
    let group_by_count = if metrics.has_group_by { 1 } else { 0 };
    let order_by_count = if metrics.has_order_by { 1 } else { 0 };

    (metrics.table_count as i64 * TABLE)
        + (metrics.join_count as i64 * JOIN)
        + (metrics.where_condition_count as i64 * WHERE_CONDITION)
        + (metrics.subquery_count as i64 * SUBQUERY)
        + (metrics.aggregate_function_count as i64 * AGGREGATE_FUNCTION)
        + (metrics.case_expression_count as i64 * CASE_EXPRESSION)
        + (metrics.set_operation_count as i64 * SET_OPERATION)
        + (group_by_count * GROUP_BY)
        + (order_by_count * ORDER_BY)
        + (metrics.hint_count as i64 * HINT)
}

/// GaussDB non-SELECT (INSERT/UPDATE/DELETE/MERGE) scoring.
///
/// ```text
/// score = (tableCount × 10) + (hintCount × 3)
/// ```
pub fn gauss_score_non_select(metrics: &ComplexityMetrics) -> i64 {
    (metrics.table_count as i64 * TABLE) + (metrics.hint_count as i64 * HINT)
}

/// GaussDB CREATE TABLE scoring.
///
/// ```text
/// score = 10 + (columnCount × 2) + (computedColumnCount × 15) + (checkConstraintCount × 10)
/// ```
pub fn gauss_score_create_table(metrics: &ComplexityMetrics) -> i64 {
    TABLE_WEIGHT
        + (metrics.column_count as i64 * COLUMN)
        + (metrics.computed_column_count as i64 * COMPUTED_COLUMN)
        + (metrics.check_constraint_count as i64 * CHECK_CONSTRAINT)
}

/// GaussDB dynamic SQL scoring.
///
/// ```text
/// baseScore = log10(sqlLength) × 5
/// score = baseScore × (1 + 0.1 × tableCount) + (hintCount × 3)
/// ```
pub fn gauss_score_dynamic_sql(sql_length: usize, table_count: usize, hint_count: usize) -> i64 {
    let base = (sql_length as f64).log10() * 5.0;
    let adjusted = base * (1.0 + 0.1 * table_count as f64);
    adjusted as i64 + (hint_count as i64 * HINT)
}

/// GaussDB stored procedure full 11-step scoring formula.
///
/// Returns a `GaussDbComplexityReport` with the full breakdown.
pub fn gauss_score_procedure(
    sql_statement_scores: &[i64],
    metrics: &ComplexityMetrics,
    package_metrics: Option<&PackageMetrics>,
) -> GaussDbComplexityReport {
    let stmt_sum: i64 = sql_statement_scores.iter().sum();
    let mut _score = stmt_sum;

    let loop_complexity =
        (metrics.loop_count as i64 * LOOP) + (metrics.max_loop_nesting_level as i64 * NESTED_LOOP);
    _score += loop_complexity;

    let custom_fn = metrics.custom_function_count as i64 * CUSTOM_FUNCTION;
    _score += custom_fn;

    let hw_table = metrics.high_weight_table_count as i64 * HIGH_WEIGHT_TABLE;
    _score += hw_table;

    let nested_proc = metrics.nested_procedure_count as i64 * NESTED_PROCEDURE;
    _score += nested_proc;

    let hw_proc = if metrics.high_weight_procedure_count > 0 {
        metrics.high_weight_procedure_count as i64 * HIGH_WEIGHT_PROCEDURE
    } else {
        0
    };
    _score += hw_proc;

    let cursor_base_raw = (metrics.cursor_count as i64 * CURSOR_DECLARATION)
        + (metrics.cursor_operation_count as i64 * CURSOR_OPERATION);
    let cursor_complexity = if metrics.max_cursor_nesting_level > 1 {
        let factor = 1 + (metrics.max_cursor_nesting_level - 1) as i64 * NESTED_CURSOR;
        cursor_base_raw * factor
    } else {
        cursor_base_raw
    };
    _score += cursor_complexity;

    // Step 8: Enhanced complexity — replaces accumulated score per spec
    let enhanced_raw = (metrics.table_count as i64 * TABLE)
        + (metrics.join_count as i64 * JOIN)
        + (metrics.where_condition_count as i64 * WHERE_CONDITION)
        + (metrics.subquery_count as i64 * SUBQUERY)
        + (metrics.set_operation_count as i64 * SET_OPERATION)
        + (metrics.loop_count as i64 * LOOP);
    let enhanced = enhanced_raw;
    let mut base_score = enhanced;

    let dyn_sql = metrics.dynamic_sql_count as i64 * DYNAMIC_SQL;
    base_score += dyn_sql;

    let param_bind = metrics.param_binding_count as i64 * PARAMETER_BINDING;
    base_score += param_bind;

    let nested_dyn = metrics.nested_dynamic_sql_count as i64 * NESTED_DYNAMIC_SQL;
    base_score += nested_dyn;

    let txn_ctrl = metrics.transaction_control_count as i64 * TRANSACTION_CONTROL;
    base_score += txn_ctrl;

    let txn_nesting = metrics.transaction_nesting_level as i64 * NESTED_TRANSACTION;
    base_score += txn_nesting;

    let auto_txn = if metrics.uses_autonomous_transactions {
        AUTONOMOUS_TRANSACTION
    } else {
        0
    };
    base_score += auto_txn;

    let java_proc = metrics.java_stored_procedure_count as i64 * JAVA_PROCEDURE;
    base_score += java_proc;

    let java_conv = metrics.java_type_conversion_count as i64 * TYPE_CONVERSION;
    base_score += java_conv;

    let hint = metrics.hint_count as i64 * HINT;
    base_score += hint;

    let pkg_complexity = if let Some(pkg) = package_metrics {
        let mut pc = pkg.total_procedures as i64 * 5 + pkg.package_level_variables as i64 * 2;
        if pkg.contains_java_procedures {
            pc += 25;
        }
        pc
    } else {
        0
    };
    base_score += pkg_complexity;

    let mut overall = base_score;

    if metrics.java_stored_procedure_count > 0 {
        overall = overall.max(50);
    }

    let level = gauss_complexity_level(overall);

    let breakdown = GaussDbScoreBreakdown {
        sql_statements_sum: stmt_sum,
        loop_complexity,
        custom_function_complexity: custom_fn,
        high_weight_table_complexity: hw_table,
        nested_procedure_complexity: nested_proc,
        high_weight_procedure_complexity: hw_proc,
        cursor_complexity,
        enhanced_complexity: enhanced,
        dynamic_sql_complexity: dyn_sql,
        param_binding_complexity: param_bind,
        nested_dynamic_sql_complexity: nested_dyn,
        transaction_complexity: txn_ctrl + txn_nesting,
        autonomous_transaction_bonus: auto_txn,
        java_procedure_complexity: java_proc,
        java_type_conversion_complexity: java_conv,
        hint_complexity: hint,
        package_complexity: pkg_complexity,
    };

    GaussDbComplexityReport {
        input_kind: InputKind::StoredProcedure,
        sql_category: SqlCategory::PLBlock,
        sql_sub_type: "PROCEDURE".into(),
        overall_score: overall,
        level,
        dimensions: compute_dimensions(&breakdown),
        tags: detect_tags(metrics),
        score_breakdown: breakdown,
        sql_statement_scores: sql_statement_scores.to_vec(),
        pl_metrics: metrics.clone(),
    }
}

/// Main entry point for GaussDB complexity analysis.
///
/// Parses the SQL input, classifies each statement, applies the appropriate
/// scoring formula, and returns a `GaussDbComplexityReport`.
pub fn gauss_analyze(
    sql: &str,
    config: &ComplexityConfig,
) -> Result<GaussDbComplexityReport, ComplexityError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(ComplexityError::EmptyInput);
    }

    let (infos, parse_errors) = ogsql_parser::Parser::parse_sql(trimmed);

    if infos.is_empty() {
        if !parse_errors.is_empty() {
            return Err(ComplexityError::ParseError(
                parse_errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        return Err(ComplexityError::EmptyInput);
    }

    if infos.len() == 1 {
        let info = &infos[0];
        if let Some(report) = try_score_pl_statement(&info.statement, config) {
            return Ok(report);
        }
    }

    let mut sql_scores: Vec<i64> = Vec::new();
    let mut all_metrics = ComplexityMetrics::default();
    let input_kind = InputKind::SqlStatement;

    for info in &infos {
        if let Some(report) = try_score_pl_statement(&info.statement, config) {
            return Ok(report);
        }

        match &info.statement {
            Statement::CreateTable(_) => {
                let metrics = visitor::analyze_statement_gauss(&info.statement, &info.sql_text);
                let score = gauss_score_create_table(&metrics);
                sql_scores.push(score);
                merge_metrics(&mut all_metrics, &metrics);
            }
            Statement::Select(_) => {
                let metrics = visitor::analyze_statement_gauss(&info.statement, &info.sql_text);
                let score = gauss_score_statement(&metrics);
                sql_scores.push(score);
                merge_metrics(&mut all_metrics, &metrics);
            }
            Statement::Insert(_)
            | Statement::InsertAll(_)
            | Statement::InsertFirst(_)
            | Statement::Update(_)
            | Statement::Delete(_)
            | Statement::Merge(_) => {
                let metrics = visitor::analyze_statement_gauss(&info.statement, &info.sql_text);
                let score = gauss_score_non_select(&metrics);
                sql_scores.push(score);
                merge_metrics(&mut all_metrics, &metrics);
            }
            _ => {
                sql_scores.push(0);
            }
        }
    }

    let overall: i64 = sql_scores.iter().sum();
    let level = gauss_complexity_level(overall);

    let (sql_category, sql_sub_type) = if let Some(info) = infos.first() {
        classify_statement(&info.statement)
    } else {
        (SqlCategory::Query, "OTHER".into())
    };

    let breakdown = GaussDbScoreBreakdown {
        sql_statements_sum: overall,
        ..Default::default()
    };

    Ok(GaussDbComplexityReport {
        input_kind,
        sql_category,
        sql_sub_type,
        overall_score: overall,
        level,
        dimensions: compute_dimensions(&breakdown),
        tags: detect_tags(&all_metrics),
        score_breakdown: breakdown,
        sql_statement_scores: sql_scores,
        pl_metrics: all_metrics,
    })
}

/// Attempt to score a single PL-bearing statement (function, procedure, DO block,
/// anonymous block, package). Returns `None` if the statement is not PL-bearing.
fn try_score_pl_statement(
    stmt: &Statement,
    config: &ComplexityConfig,
) -> Option<GaussDbComplexityReport> {
    match stmt {
        Statement::CreateFunction(cf) => {
            let is_java = cf.options.language.as_deref() == Some("java")
                || cf.options.language.as_deref() == Some("JAVA");

            if let Some(block) = &cf.block {
                let mut metrics = pl_visitor::analyze_pl_block(block, config);
                let sql_metrics = collect_sql_metrics_from_block(block);
                merge_metrics(&mut metrics, &sql_metrics);
                if is_java {
                    metrics.java_stored_procedure_count += 1;
                }
                let sql_scores = collect_sql_scores_from_block(block, config);
                let mut report = gauss_score_procedure(&sql_scores, &metrics, None);
                report.input_kind = InputKind::StoredProcedure;
                report.sql_category = SqlCategory::PLBlock;
                report.sql_sub_type = "CREATE FUNCTION".into();
                Some(report)
            } else if is_java {
                let metrics = ComplexityMetrics {
                    java_stored_procedure_count: 1,
                    ..Default::default()
                };
                let report = gauss_score_procedure(&[], &metrics, None);
                Some(GaussDbComplexityReport {
                    input_kind: InputKind::StoredProcedure,
                    sql_category: SqlCategory::PLBlock,
                    sql_sub_type: "CREATE FUNCTION".into(),
                    ..report
                })
            } else {
                None
            }
        }
        Statement::CreateProcedure(cp) => {
            let block = cp.block.as_ref()?;
            let mut metrics = pl_visitor::analyze_pl_block(block, config);
            let sql_metrics = collect_sql_metrics_from_block(block);
            merge_metrics(&mut metrics, &sql_metrics);
            let sql_scores = collect_sql_scores_from_block(block, config);
            let mut report = gauss_score_procedure(&sql_scores, &metrics, None);
            report.input_kind = InputKind::StoredProcedure;
            report.sql_category = SqlCategory::PLBlock;
            report.sql_sub_type = "CREATE PROCEDURE".into();
            Some(report)
        }
        Statement::Do(d) => {
            let block = d.block.as_ref()?;
            let mut metrics = pl_visitor::analyze_pl_block(block, config);
            let sql_metrics = collect_sql_metrics_from_block(block);
            merge_metrics(&mut metrics, &sql_metrics);
            let sql_scores = collect_sql_scores_from_block(block, config);
            let mut report = gauss_score_procedure(&sql_scores, &metrics, None);
            report.input_kind = InputKind::AnonymousBlock;
            report.sql_category = SqlCategory::PLBlock;
            report.sql_sub_type = "DO".into();
            Some(report)
        }
        Statement::AnonyBlock(ab) => {
            let mut metrics = pl_visitor::analyze_pl_block(&ab.block, config);
            let sql_metrics = collect_sql_metrics_from_block(&ab.block);
            merge_metrics(&mut metrics, &sql_metrics);
            let sql_scores = collect_sql_scores_from_block(&ab.block, config);
            let mut report = gauss_score_procedure(&sql_scores, &metrics, None);
            report.input_kind = InputKind::AnonymousBlock;
            report.sql_category = SqlCategory::PLBlock;
            report.sql_sub_type = "ANONYMOUS BLOCK".into();
            Some(report)
        }
        Statement::CreatePackage(cp) => {
            let (pkg_metrics, block_metrics, sql_scores) = score_package(&cp.items, None, config);
            let mut report = gauss_score_procedure(&sql_scores, &block_metrics, Some(&pkg_metrics));
            report.input_kind = InputKind::StoredProcedure;
            report.sql_category = SqlCategory::Package;
            report.sql_sub_type = "CREATE PACKAGE".into();
            Some(report)
        }
        Statement::CreatePackageBody(cpb) => {
            let (pkg_metrics, block_metrics, sql_scores) = score_package(&cpb.items, None, config);
            let mut report = gauss_score_procedure(&sql_scores, &block_metrics, Some(&pkg_metrics));
            report.input_kind = InputKind::StoredProcedure;
            report.sql_category = SqlCategory::Package;
            report.sql_sub_type = "CREATE PACKAGE BODY".into();
            Some(report)
        }
        _ => None,
    }
}

fn collect_sql_metrics_from_block(
    block: &ogsql_parser::ast::plpgsql::PlBlock,
) -> ComplexityMetrics {
    let mut metrics = ComplexityMetrics::default();
    collect_sql_metrics_from_stmts(&block.body, &mut metrics);
    if let Some(exc) = &block.exception_block {
        for handler in &exc.handlers {
            collect_sql_metrics_from_stmts(&handler.statements, &mut metrics);
        }
    }
    for decl in &block.declarations {
        match decl {
            ogsql_parser::ast::plpgsql::PlDeclaration::NestedProcedure(p) => {
                if let Some(b) = &p.block {
                    let sub = collect_sql_metrics_from_block(b);
                    merge_metrics(&mut metrics, &sub);
                }
            }
            ogsql_parser::ast::plpgsql::PlDeclaration::NestedFunction(f) => {
                if let Some(b) = &f.block {
                    let sub = collect_sql_metrics_from_block(b);
                    merge_metrics(&mut metrics, &sub);
                }
            }
            _ => {}
        }
    }
    metrics
}

fn collect_sql_metrics_from_stmts(
    stmts: &[ogsql_parser::ast::plpgsql::PlStatement],
    metrics: &mut ComplexityMetrics,
) {
    use ogsql_parser::ast::plpgsql::PlStatement;

    for stmt in stmts {
        match stmt {
            PlStatement::SqlStatement {
                sql_text,
                statement,
            } => {
                let sub = visitor::analyze_statement_gauss(statement, sql_text);
                merge_metrics(metrics, &sub);
            }
            PlStatement::Execute(e) => {
                if let Some(parsed) = &e.parsed_query {
                    let sql_text = format!("{:?}", e.string_expr);
                    let sub = visitor::analyze_statement_gauss(parsed, &sql_text);
                    merge_metrics(metrics, &sub);
                }
            }
            PlStatement::Perform {
                query,
                parsed_query: Some(parsed),
            } => {
                let sub = visitor::analyze_statement_gauss(parsed, query);
                merge_metrics(metrics, &sub);
            }
            PlStatement::Loop(l) => {
                collect_sql_metrics_from_stmts(&l.body, metrics);
            }
            PlStatement::While(w) => {
                collect_sql_metrics_from_stmts(&w.body, metrics);
            }
            PlStatement::For(f) => {
                collect_sql_metrics_from_stmts(&f.body, metrics);
            }
            PlStatement::ForEach(f) => {
                collect_sql_metrics_from_stmts(&f.body, metrics);
            }
            PlStatement::If(i) => {
                collect_sql_metrics_from_stmts(&i.then_stmts, metrics);
                for e in &i.elsifs {
                    collect_sql_metrics_from_stmts(&e.stmts, metrics);
                }
                collect_sql_metrics_from_stmts(&i.else_stmts, metrics);
            }
            PlStatement::Case(c) => {
                for w in &c.whens {
                    collect_sql_metrics_from_stmts(&w.stmts, metrics);
                }
                collect_sql_metrics_from_stmts(&c.else_stmts, metrics);
            }
            PlStatement::Block(b) => {
                let sub = collect_sql_metrics_from_block(b);
                merge_metrics(metrics, &sub);
            }
            _ => {}
        }
    }
}

/// Collect individual SQL statement scores from all SQL statements embedded in a PL block.
fn collect_sql_scores_from_block(
    block: &ogsql_parser::ast::plpgsql::PlBlock,
    config: &ComplexityConfig,
) -> Vec<i64> {
    let mut scores = Vec::new();
    collect_sql_scores_from_stmts(&block.body, config, &mut scores);
    if let Some(exc) = &block.exception_block {
        for handler in &exc.handlers {
            collect_sql_scores_from_stmts(&handler.statements, config, &mut scores);
        }
    }
    for decl in &block.declarations {
        match decl {
            ogsql_parser::ast::plpgsql::PlDeclaration::NestedProcedure(p) => {
                if let Some(b) = &p.block {
                    scores.extend(collect_sql_scores_from_block(b, config));
                }
            }
            ogsql_parser::ast::plpgsql::PlDeclaration::NestedFunction(f) => {
                if let Some(b) = &f.block {
                    scores.extend(collect_sql_scores_from_block(b, config));
                }
            }
            _ => {}
        }
    }
    scores
}

fn collect_sql_scores_from_stmts(
    stmts: &[ogsql_parser::ast::plpgsql::PlStatement],
    config: &ComplexityConfig,
    scores: &mut Vec<i64>,
) {
    use ogsql_parser::ast::plpgsql::PlStatement;

    for stmt in stmts {
        match stmt {
            PlStatement::SqlStatement {
                sql_text,
                statement,
            } => {
                let score = score_single_statement(statement, sql_text);
                scores.push(score);
            }
            PlStatement::Execute(e) => {
                if let Some(parsed) = &e.parsed_query {
                    let sql_text = format!("{:?}", e.string_expr);
                    let score = score_single_statement(parsed, &sql_text);
                    scores.push(score);
                } else {
                    let sql_text = format!("{:?}", e.string_expr);
                    let len = sql_text.len();
                    scores.push(gauss_score_dynamic_sql(len, 0, 0));
                }
            }
            PlStatement::Perform {
                query,
                parsed_query: Some(parsed),
            } => {
                let score = score_single_statement(parsed, query);
                scores.push(score);
            }
            PlStatement::Sql(text) => {
                let len = text.len();
                scores.push(gauss_score_dynamic_sql(len, 0, 0));
            }
            PlStatement::Loop(l) => {
                collect_sql_scores_from_stmts(&l.body, config, scores);
            }
            PlStatement::While(w) => {
                collect_sql_scores_from_stmts(&w.body, config, scores);
            }
            PlStatement::For(f) => {
                collect_sql_scores_from_stmts(&f.body, config, scores);
            }
            PlStatement::ForEach(f) => {
                collect_sql_scores_from_stmts(&f.body, config, scores);
            }
            PlStatement::If(i) => {
                collect_sql_scores_from_stmts(&i.then_stmts, config, scores);
                for e in &i.elsifs {
                    collect_sql_scores_from_stmts(&e.stmts, config, scores);
                }
                collect_sql_scores_from_stmts(&i.else_stmts, config, scores);
            }
            PlStatement::Case(c) => {
                for w in &c.whens {
                    collect_sql_scores_from_stmts(&w.stmts, config, scores);
                }
                collect_sql_scores_from_stmts(&c.else_stmts, config, scores);
            }
            PlStatement::Block(b) => {
                scores.extend(collect_sql_scores_from_block(b, config));
            }
            _ => {}
        }
    }
}

/// Score a single parsed SQL statement using the appropriate GaussDB formula.
fn score_single_statement(stmt: &Statement, sql_text: &str) -> i64 {
    match stmt {
        Statement::Select(_) => {
            let metrics = visitor::analyze_statement_gauss(stmt, sql_text);
            gauss_score_statement(&metrics)
        }
        Statement::CreateTable(_) => {
            let metrics = visitor::analyze_statement_gauss(stmt, sql_text);
            gauss_score_create_table(&metrics)
        }
        Statement::Insert(_)
        | Statement::InsertAll(_)
        | Statement::InsertFirst(_)
        | Statement::Update(_)
        | Statement::Delete(_)
        | Statement::Merge(_) => {
            let metrics = visitor::analyze_statement_gauss(stmt, sql_text);
            gauss_score_non_select(&metrics)
        }
        Statement::Explain(e) => score_single_statement(&e.query, sql_text),
        _ => 0,
    }
}

/// Extract PackageMetrics and aggregate PL metrics from package items.
fn score_package(
    items: &[ogsql_parser::ast::PackageItem],
    _body: Option<&str>,
    config: &ComplexityConfig,
) -> (PackageMetrics, ComplexityMetrics, Vec<i64>) {
    let mut pkg = PackageMetrics::default();
    let mut agg_metrics = ComplexityMetrics::default();
    let mut all_scores: Vec<i64> = Vec::new();
    let mut has_java = false;

    for item in items {
        match item {
            ogsql_parser::ast::PackageItem::Procedure(proc) => {
                pkg.total_procedures += 1;
                if let Some(block) = &proc.block {
                    let m = pl_visitor::analyze_pl_block(block, config);
                    let scores = collect_sql_scores_from_block(block, config);
                    all_scores.extend(scores);
                    merge_metrics(&mut agg_metrics, &m);
                }
            }
            ogsql_parser::ast::PackageItem::Function(func) => {
                pkg.total_procedures += 1;
                if let Some(block) = &func.block {
                    let m = pl_visitor::analyze_pl_block(block, config);
                    let scores = collect_sql_scores_from_block(block, config);
                    all_scores.extend(scores);
                    merge_metrics(&mut agg_metrics, &m);
                    if is_java_function(func) {
                        has_java = true;
                        agg_metrics.java_stored_procedure_count += 1;
                    }
                }
            }
            ogsql_parser::ast::PackageItem::Raw(_) => {
                pkg.package_level_variables += 1;
            }
        }
    }

    pkg.contains_java_procedures = has_java;
    (pkg, agg_metrics, all_scores)
}

fn is_java_function(func: &ogsql_parser::ast::PackageFunction) -> bool {
    let _ = func;
    false
}

/// Merge child metrics into a parent aggregate.
fn merge_metrics(target: &mut ComplexityMetrics, source: &ComplexityMetrics) {
    target.table_count += source.table_count;
    target.join_count += source.join_count;
    target.where_condition_count += source.where_condition_count;
    target.subquery_count += source.subquery_count;
    target.aggregate_function_count += source.aggregate_function_count;
    target.case_expression_count += source.case_expression_count;
    target.set_operation_count += source.set_operation_count;
    target.cte_count += source.cte_count;
    target.window_function_count += source.window_function_count;
    target.has_group_by = target.has_group_by || source.has_group_by;
    target.has_order_by = target.has_order_by || source.has_order_by;
    target.has_distinct = target.has_distinct || source.has_distinct;
    target.subquery_depth = target.subquery_depth.max(source.subquery_depth);
    target.hint_count += source.hint_count;
    target.loop_count += source.loop_count;
    target.max_loop_nesting_level = target
        .max_loop_nesting_level
        .max(source.max_loop_nesting_level);
    target.cursor_count += source.cursor_count;
    target.cursor_operation_count += source.cursor_operation_count;
    target.max_cursor_nesting_level = target
        .max_cursor_nesting_level
        .max(source.max_cursor_nesting_level);
    target.dynamic_sql_count += source.dynamic_sql_count;
    target.param_binding_count += source.param_binding_count;
    target.nested_dynamic_sql_count += source.nested_dynamic_sql_count;
    target.transaction_control_count += source.transaction_control_count;
    target.transaction_nesting_level = target
        .transaction_nesting_level
        .max(source.transaction_nesting_level);
    target.uses_autonomous_transactions =
        target.uses_autonomous_transactions || source.uses_autonomous_transactions;
    target.subtransaction_count += source.subtransaction_count;
    target.max_subtransaction_nesting_level = target
        .max_subtransaction_nesting_level
        .max(source.max_subtransaction_nesting_level);
    target.custom_function_count += source.custom_function_count;
    target.high_weight_table_count += source.high_weight_table_count;
    target.nested_procedure_count += source.nested_procedure_count;
    target.high_weight_procedure_count += source.high_weight_procedure_count;
    target.java_stored_procedure_count += source.java_stored_procedure_count;
    target.java_type_conversion_count += source.java_type_conversion_count;
    target.column_count += source.column_count;
    target.computed_column_count += source.computed_column_count;
    target.check_constraint_count += source.check_constraint_count;
    target.line_count += source.line_count;
    target.package_procedure_count += source.package_procedure_count;
    target.package_variable_count += source.package_variable_count;
    target.package_has_java = target.package_has_java || source.package_has_java;
}

/// Classify a SQL statement into a category and sub-type string.
fn classify_statement(stmt: &Statement) -> (SqlCategory, String) {
    match stmt {
        Statement::Select(_) => (SqlCategory::Query, "SELECT".into()),
        Statement::Insert(_) => (SqlCategory::DML, "INSERT".into()),
        Statement::InsertAll(_) => (SqlCategory::DML, "INSERT ALL".into()),
        Statement::InsertFirst(_) => (SqlCategory::DML, "INSERT FIRST".into()),
        Statement::Update(_) => (SqlCategory::DML, "UPDATE".into()),
        Statement::Delete(_) => (SqlCategory::DML, "DELETE".into()),
        Statement::Merge(_) => (SqlCategory::DML, "MERGE".into()),
        Statement::CreateTable(_) | Statement::CreateTableAs(_) => {
            (SqlCategory::DDL, "CREATE TABLE".into())
        }
        Statement::CreateIndex(_) | Statement::CreateGlobalIndex(_) => {
            (SqlCategory::DDL, "CREATE INDEX".into())
        }
        Statement::CreateFunction(_) => (SqlCategory::PLBlock, "CREATE FUNCTION".into()),
        Statement::CreateProcedure(_) => (SqlCategory::PLBlock, "CREATE PROCEDURE".into()),
        Statement::Do(_) => (SqlCategory::PLBlock, "DO".into()),
        Statement::AnonyBlock(_) => (SqlCategory::PLBlock, "ANONYMOUS BLOCK".into()),
        Statement::CreatePackage(_) => (SqlCategory::Package, "CREATE PACKAGE".into()),
        Statement::CreatePackageBody(_) => (SqlCategory::Package, "CREATE PACKAGE BODY".into()),
        Statement::Explain(e) => classify_statement(&e.query),
        _ => (SqlCategory::Query, "OTHER".into()),
    }
}

/// Compute score dimensions from the breakdown for statistical grouping.
fn compute_dimensions(bd: &GaussDbScoreBreakdown) -> ScoreDimensions {
    ScoreDimensions {
        sql_structure: bd.enhanced_complexity,
        pl_logic: bd.loop_complexity
            + bd.cursor_complexity
            + bd.custom_function_complexity
            + bd.nested_procedure_complexity
            + bd.high_weight_procedure_complexity,
        advanced_feature: bd.dynamic_sql_complexity
            + bd.param_binding_complexity
            + bd.nested_dynamic_sql_complexity
            + bd.transaction_complexity
            + bd.autonomous_transaction_bonus,
        extension: bd.java_procedure_complexity
            + bd.java_type_conversion_complexity
            + bd.hint_complexity
            + bd.package_complexity
            + bd.high_weight_table_complexity,
    }
}

/// Detect risk tags from complexity metrics.
fn detect_tags(m: &ComplexityMetrics) -> Vec<ComplexityTag> {
    let mut tags = Vec::new();
    if m.table_count > 5 {
        tags.push(ComplexityTag::HighTableCount);
    }
    if m.subquery_depth > 3 || m.max_loop_nesting_level > 2 {
        tags.push(ComplexityTag::DeepNesting);
    }
    if m.dynamic_sql_count > 0 {
        tags.push(ComplexityTag::DynamicSql);
    }
    if m.cursor_count > 3 {
        tags.push(ComplexityTag::CursorHeavy);
    }
    if m.uses_autonomous_transactions || m.subtransaction_count > 0 {
        tags.push(ComplexityTag::TransactionComplex);
    }
    if m.java_stored_procedure_count > 0 {
        tags.push(ComplexityTag::JavaProcedure);
    }
    if m.join_count > 3 {
        tags.push(ComplexityTag::LargeJoin);
    }
    tags
}

/// Map an integer score to a GaussDB complexity level.
fn gauss_complexity_level(score: i64) -> ComplexityLevel {
    if score < 5 {
        ComplexityLevel::Trivial
    } else if score < 15 {
        ComplexityLevel::Simple
    } else if score < 30 {
        ComplexityLevel::Moderate
    } else if score < 50 {
        ComplexityLevel::Complex
    } else {
        ComplexityLevel::VeryComplex
    }
}
