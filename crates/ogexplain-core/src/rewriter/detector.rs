use ogsql_parser::ast::{Expr, ObjectName, SelectStatement, Statement, TableRef};

use super::types::{AntiPatternInfo, RewriteError};

pub fn detect_correlated_subquery_update(
    stmt: &Statement,
) -> Result<Option<AntiPatternInfo>, RewriteError> {
    let update = match stmt {
        Statement::Update(s) => &s.node,
        _ => return Ok(None),
    };

    let target_table = extract_table_name(&update.tables).ok_or(
        RewriteError::UnsupportedSyntax("cannot extract target table".into()),
    )?;

    for assignment in &update.assignments {
        if let Some(subquery) = extract_subquery_from_value(&assignment.value) {
            let subquery_tables = extract_from_table_names(&subquery.from);
            if subquery_tables.contains(&target_table) {
                let correlation_columns = extract_correlation_columns(
                    &subquery.where_clause,
                    &target_table,
                    &subquery.from,
                );
                let set_columns = extract_set_columns(&assignment.columns);
                let uses_row_constructor = assignment.columns.len() > 1;
                return Ok(Some(AntiPatternInfo {
                    target_table: target_table.clone(),
                    subquery_table: target_table,
                    correlation_columns,
                    set_columns,
                    uses_row_constructor,
                }));
            }
        }
    }

    Ok(None)
}

fn extract_table_name(tables: &[TableRef]) -> Option<String> {
    match tables.first()? {
        TableRef::Table { name, .. } => Some(object_name_to_string(name)),
        _ => None,
    }
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.join(".")
}

fn extract_subquery_from_value(expr: &Expr) -> Option<&SelectStatement> {
    match expr {
        Expr::Subquery(select) => Some(select),
        Expr::Parenthesized(inner) => extract_subquery_from_value(inner),
        _ => None,
    }
}

fn extract_from_table_names(from: &[TableRef]) -> Vec<String> {
    let mut names = Vec::new();
    for table_ref in from {
        match table_ref {
            TableRef::Table { name, .. } => {
                names.push(object_name_to_string(name));
            }
            TableRef::Join { left, right, .. } => {
                names.extend(extract_from_table_names(std::slice::from_ref(left)));
                names.extend(extract_from_table_names(std::slice::from_ref(right)));
            }
            _ => {}
        }
    }
    names
}

fn extract_correlation_columns(
    where_clause: &Option<Expr>,
    target_table: &str,
    from: &[TableRef],
) -> Vec<String> {
    let alias = find_table_alias(from, target_table);
    let mut columns = Vec::new();
    if let Some(expr) = where_clause {
        collect_equality_columns(expr, target_table, alias.as_deref(), &mut columns);
    }
    columns
}

fn find_table_alias(from: &[TableRef], table_name: &str) -> Option<String> {
    for table_ref in from {
        if let TableRef::Table { name, alias, .. } = table_ref {
            if object_name_to_string(name) == table_name {
                return alias.as_ref().map(|a| a.value.clone());
            }
        }
    }
    None
}

fn collect_equality_columns(
    expr: &Expr,
    target_table: &str,
    subquery_alias: Option<&str>,
    columns: &mut Vec<String>,
) {
    match expr {
        Expr::BinaryOp { left, op, right } if op == "=" => {
            if let (Expr::ColumnRef(lhs), Expr::ColumnRef(rhs)) = (left.as_ref(), right.as_ref()) {
                if let Some(col) =
                    identify_correlation_column(lhs, rhs, target_table, subquery_alias)
                {
                    if !columns.contains(&col) {
                        columns.push(col);
                    }
                }
            }
        }
        Expr::BinaryOp { left, op, right } if op == "AND" => {
            collect_equality_columns(left, target_table, subquery_alias, columns);
            collect_equality_columns(right, target_table, subquery_alias, columns);
        }
        Expr::Parenthesized(inner) => {
            collect_equality_columns(inner, target_table, subquery_alias, columns);
        }
        _ => {}
    }
}

fn identify_correlation_column(
    lhs: &ObjectName,
    rhs: &ObjectName,
    target_table: &str,
    subquery_alias: Option<&str>,
) -> Option<String> {
    let lhs_qualified = is_qualified_ref(lhs, target_table, subquery_alias);
    let rhs_qualified = is_qualified_ref(rhs, target_table, subquery_alias);

    match (lhs_qualified, rhs_qualified) {
        (RefRole::Target, RefRole::Alias) => Some(get_column_name(lhs)),
        (RefRole::Alias, RefRole::Target) => Some(get_column_name(rhs)),
        (RefRole::Target, RefRole::Unqualified) => Some(get_column_name(rhs)),
        (RefRole::Unqualified, RefRole::Target) => Some(get_column_name(lhs)),
        _ => None,
    }
}

enum RefRole {
    Target,
    Alias,
    Unqualified,
}

fn is_qualified_ref(name: &ObjectName, target_table: &str, alias: Option<&str>) -> RefRole {
    match name.len() {
        1 => RefRole::Unqualified,
        2 => {
            let qualifier = &name[0];
            if qualifier == target_table {
                RefRole::Target
            } else if alias.is_some_and(|a| a == qualifier.as_str()) {
                RefRole::Alias
            } else {
                RefRole::Unqualified
            }
        }
        _ => RefRole::Unqualified,
    }
}

fn get_column_name(name: &ObjectName) -> String {
    name.last().map(|i| i.value.clone()).unwrap_or_default()
}

fn extract_set_columns(columns: &[ObjectName]) -> Vec<String> {
    columns.iter().map(object_name_to_string).collect()
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
    fn detects_single_column_correlated_update() {
        let sql = "UPDATE employees SET salary = (SELECT salary * 1.15 FROM employees e WHERE e.emp_id = employees.emp_id)";
        let stmt = parse_stmt(sql);
        let result = detect_correlated_subquery_update(&stmt).unwrap();
        let info = result.unwrap();
        assert_eq!(info.target_table, "employees");
        assert_eq!(info.subquery_table, "employees");
        assert!(info.correlation_columns.contains(&"emp_id".to_string()));
        assert!(!info.uses_row_constructor);
    }

    #[test]
    fn no_detection_for_normal_update() {
        let sql = "UPDATE employees SET salary = 50000 WHERE dept = 'engineering'";
        let stmt = parse_stmt(sql);
        let result = detect_correlated_subquery_update(&stmt).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn no_detection_for_different_table_subquery() {
        let sql = "UPDATE employees SET salary = (SELECT max_salary FROM salary_grades sg WHERE sg.grade = employees.grade)";
        let stmt = parse_stmt(sql);
        let result = detect_correlated_subquery_update(&stmt).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn no_detection_for_non_update() {
        let sql = "SELECT * FROM employees";
        let stmt = parse_stmt(sql);
        let result = detect_correlated_subquery_update(&stmt).unwrap();
        assert!(result.is_none());
    }
}
