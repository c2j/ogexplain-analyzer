use crate::model::*;
use crate::visitor;

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
