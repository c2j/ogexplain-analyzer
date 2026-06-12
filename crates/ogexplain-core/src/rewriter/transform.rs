use ogsql_parser::ast::{
    Expr, SelectStatement, SelectTarget, Statement, TableRef, UpdateStatement,
};
use ogsql_parser::formatter::SqlFormatter;

use super::detector;
use super::types::{AntiPatternInfo, RewriteError, RewriteResult, RewriteStrategy};

const SUBQUERY_ALIAS: &str = "sub";

pub fn rewrite_update_from(stmt: &Statement) -> Result<RewriteResult, RewriteError> {
    let info =
        detector::detect_correlated_subquery_update(stmt)?.ok_or(RewriteError::PatternNotFound)?;

    let mut rewritten = stmt.clone();
    let update = match &mut rewritten {
        Statement::Update(s) => &mut s.node,
        _ => return Err(RewriteError::PatternNotFound),
    };

    rewrite_assignments(update, &info)?;

    let formatter = SqlFormatter::new();
    let rewritten_sql = formatter.format_statement(&rewritten);

    Ok(RewriteResult {
        strategy: RewriteStrategy::UpdateFrom,
        rewritten_sql,
        explanation: format!(
            "将关联子查询 UPDATE 改写为 UPDATE ... FROM 形式，避免对 {} 的逐行子查询执行",
            info.target_table
        ),
        pattern_info: info,
    })
}

fn rewrite_assignments(
    update: &mut UpdateStatement,
    info: &AntiPatternInfo,
) -> Result<(), RewriteError> {
    let assignment = update
        .assignments
        .iter_mut()
        .find(|a| detector_has_subquery(&a.value))
        .ok_or(RewriteError::PatternNotFound)?;

    let subquery =
        extract_subquery_owned(&mut assignment.value).ok_or(RewriteError::PatternNotFound)?;

    let mut enriched_subquery = subquery.clone();
    ensure_correlation_in_select(&mut enriched_subquery, &info.correlation_columns);

    if info.uses_row_constructor {
        let column_names = info.set_columns.to_vec();

        let mut new_assignments = Vec::new();
        for col in &column_names {
            new_assignments.push(ogsql_parser::ast::UpdateAssignment {
                columns: vec![vec![col.clone()]],
                value: Expr::ColumnRef(vec![SUBQUERY_ALIAS.to_string(), col.clone()]),
            });
        }
        let idx = update
            .assignments
            .iter()
            .position(|a| detector_has_subquery(&a.value));
        if let Some(i) = idx {
            update.assignments.splice(i..=i, new_assignments);
        }
    } else {
        let col_name = info.set_columns.first().cloned().unwrap_or_default();
        assignment.value = Expr::ColumnRef(vec![SUBQUERY_ALIAS.to_string(), col_name]);
    }

    update.from.push(TableRef::Subquery {
        query: Box::new(enriched_subquery),
        alias: Some(SUBQUERY_ALIAS.to_string()),
    });

    if !info.correlation_columns.is_empty() {
        let mut conditions: Vec<Expr> = Vec::new();
        for col in &info.correlation_columns {
            conditions.push(Expr::BinaryOp {
                left: Box::new(Expr::ColumnRef(vec![
                    info.target_table.clone(),
                    col.clone(),
                ])),
                op: "=".to_string(),
                right: Box::new(Expr::ColumnRef(vec![
                    SUBQUERY_ALIAS.to_string(),
                    col.clone(),
                ])),
            });
        }

        let new_where = conditions.into_iter().reduce(|acc, cond| Expr::BinaryOp {
            left: Box::new(acc),
            op: "AND".to_string(),
            right: Box::new(cond),
        });

        if let Some(existing_where) = &update.where_clause {
            update.where_clause = Some(Expr::BinaryOp {
                left: Box::new(new_where.unwrap()),
                op: "AND".to_string(),
                right: Box::new(existing_where.clone()),
            });
        } else {
            update.where_clause = new_where;
        }
    }

    Ok(())
}

fn detector_has_subquery(expr: &Expr) -> bool {
    match expr {
        Expr::Subquery(_) => true,
        Expr::Parenthesized(inner) => detector_has_subquery(inner),
        _ => false,
    }
}

fn extract_subquery_owned(expr: &mut Expr) -> Option<SelectStatement> {
    match expr {
        Expr::Subquery(select) => Some(*std::mem::replace(
            select,
            Box::new(SelectStatement {
                hints: vec![],
                with: None,
                distinct: false,
                distinct_on: vec![],
                targets: vec![],
                into_targets: None,
                bulk_collect: false,
                into_table: None,
                from: vec![],
                where_clause: None,
                connect_by: None,
                group_by: vec![],
                having: None,
                order_by: vec![],
                order_siblings: false,
                limit: None,
                offset: None,
                fetch: None,
                lock_clause: None,
                window_clause: vec![],
                set_operation: None,
                raw_body: None,
            }),
        )),
        Expr::Parenthesized(inner) => extract_subquery_owned(inner),
        _ => None,
    }
}

fn ensure_correlation_in_select(subquery: &mut SelectStatement, columns: &[String]) {
    let existing_cols = collect_selected_columns(&subquery.targets);

    for col in columns {
        if !existing_cols.contains(col) {
            subquery
                .targets
                .push(SelectTarget::Expr(Expr::ColumnRef(vec![col.clone()]), None));
        }
    }
}

fn collect_selected_columns(targets: &[SelectTarget]) -> Vec<String> {
    let mut cols = Vec::new();
    for target in targets {
        if let SelectTarget::Expr(Expr::ColumnRef(name), _) = target {
            if let Some(col) = name.last() {
                cols.push(col.clone());
            }
        }
    }
    cols
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogsql_parser::parser::Parser;

    fn parse_stmt(sql: &str) -> Statement {
        let (stmts, errors) = Parser::parse_sql(sql);
        assert!(errors.is_empty(), "Parse errors: {:?}", errors);
        stmts.into_iter().next().expect("no statements").statement
    }

    #[test]
    fn rewrite_single_column() {
        let sql = "UPDATE employees SET salary = (SELECT salary * 1.15 FROM employees e WHERE e.emp_id = employees.emp_id)";
        let stmt = parse_stmt(sql);
        let result = rewrite_update_from(&stmt).unwrap();
        assert_eq!(result.strategy, RewriteStrategy::UpdateFrom);
        assert!(
            result.rewritten_sql.contains("FROM"),
            "should contain FROM: {}",
            result.rewritten_sql
        );
        assert!(result.rewritten_sql.contains("employees"));
    }

    #[test]
    fn rewrite_produces_parseable_sql() {
        let sql = "UPDATE employees SET salary = (SELECT salary * 1.15 FROM employees e WHERE e.emp_id = employees.emp_id)";
        let stmt = parse_stmt(sql);
        let result = rewrite_update_from(&stmt).unwrap();
        let (stmts, errors) = Parser::parse_sql(&result.rewritten_sql);
        assert!(
            errors.is_empty(),
            "Parse errors in rewritten SQL: {:?}\nSQL: {}",
            errors,
            result.rewritten_sql
        );
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn rewrite_returns_error_for_non_update() {
        let sql = "SELECT * FROM employees";
        let stmt = parse_stmt(sql);
        let result = rewrite_update_from(&stmt);
        assert!(result.is_err());
    }

    #[test]
    fn rewrite_returns_error_for_normal_update() {
        let sql = "UPDATE employees SET salary = 50000";
        let stmt = parse_stmt(sql);
        let result = rewrite_update_from(&stmt);
        assert!(result.is_err());
    }
}
