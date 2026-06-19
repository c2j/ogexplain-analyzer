use std::collections::HashSet;

use crate::model::{ComplexityMetrics, StatementTypeMultiplier};
use ogsql_parser::ast::{
    ColumnConstraint, Expr, GroupByItem, InsertSource, SelectStatement, SelectTarget, SetOperation,
    Statement, TableConstraint, TableRef, WithClause,
};
use ogsql_parser::ObjectName;

const AGGREGATE_FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "STDDEV",
    "STDDEV_POP",
    "STDDEV_SAMP",
    "VARIANCE",
    "VAR_POP",
    "VAR_SAMP",
    "ARRAY_AGG",
    "STRING_AGG",
    "LISTAGG",
    "GROUP_CONCAT",
    "BOOL_AND",
    "BOOL_OR",
    "BIT_AND",
    "BIT_OR",
    "BIT_XOR",
    "CORR",
    "COVAR_POP",
    "COVAR_SAMP",
    "REGR_AVGX",
    "REGR_AVGY",
    "REGR_COUNT",
    "REGR_INTERCEPT",
    "REGR_R2",
    "REGR_SLOPE",
    "REGR_SXX",
    "REGR_SXY",
    "REGR_SYY",
    "APPROX_COUNT_DISTINCT",
    "EVERY",
    "JSON_AGG",
    "JSON_OBJECT_AGG",
    "JSONB_AGG",
    "JSONB_OBJECT_AGG",
    "XMLAGG",
    "PERCENTILE_CONT",
    "PERCENTILE_DISC",
    "MODE",
    "RANK",
    "DENSE_RANK",
    "PERCENT_RANK",
    "CUME_DIST",
    "NTILE",
    "LAG",
    "LEAD",
    "FIRST_VALUE",
    "LAST_VALUE",
    "NTH_VALUE",
    "ROW_NUMBER",
];

pub fn statement_type(stmt: &Statement) -> StatementTypeMultiplier {
    match stmt {
        Statement::Select(_) => StatementTypeMultiplier::Select,
        Statement::Insert(_) | Statement::InsertAll(_) | Statement::InsertFirst(_) => {
            StatementTypeMultiplier::Insert
        }
        Statement::Update(_) => StatementTypeMultiplier::Update,
        Statement::Delete(_) => StatementTypeMultiplier::Delete,
        Statement::Merge(_) => StatementTypeMultiplier::Merge,
        Statement::Explain(e) => statement_type(&e.query),
        _ => StatementTypeMultiplier::Other,
    }
}

pub fn analyze_statement(stmt: &Statement) -> ComplexityMetrics {
    let mut visitor = ComplexityVisitor::default();
    visitor.visit_statement(stmt);
    visitor.metrics
}

/// GaussDB-mode analysis: WHERE conditions counted as existence-only (1 per WHERE clause),
/// and `line_count` is populated from the raw SQL text.
pub fn analyze_statement_gauss(stmt: &Statement, sql_text: &str) -> ComplexityMetrics {
    let mut visitor = ComplexityVisitor {
        gaussdb_where_mode: true,
        ..Default::default()
    };
    visitor.visit_statement(stmt);
    visitor.metrics.line_count = sql_text.lines().count();
    visitor.metrics
}

#[derive(Default)]
struct ComplexityVisitor {
    metrics: ComplexityMetrics,
    cte_names: HashSet<String>,
    current_depth: usize,
    gaussdb_where_mode: bool,
}

impl ComplexityVisitor {
    fn visit_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Select(s) => self.visit_select(s),
            Statement::Insert(i) => {
                self.metrics.table_count += 1;
                self.metrics.hint_count += i.hints.len();
                if let InsertSource::Select(sel) = &i.source {
                    self.visit_select(sel);
                }
                for target in &i.returning {
                    if let SelectTarget::Expr(expr, _) = target {
                        self.walk_expr(expr);
                    }
                }
            }
            Statement::InsertAll(ia) => {
                self.metrics.table_count += ia.targets.len();
                self.visit_select(&ia.source);
            }
            Statement::InsertFirst(if_) => {
                self.metrics.table_count += if_.when_clauses.len();
                self.visit_select(&if_.source);
            }
            Statement::Update(u) => {
                self.metrics.hint_count += u.hints.len();
                for t in &u.tables {
                    self.count_table_ref(t);
                }
                for t in &u.from {
                    self.count_table_ref(t);
                }
                for assignment in &u.assignments {
                    self.walk_expr(&assignment.value);
                }
                if let Some(w) = &u.where_clause {
                    self.metrics.where_condition_count += self.count_conditions(w);
                    self.walk_expr(w);
                }
                for target in &u.returning {
                    if let SelectTarget::Expr(expr, _) = target {
                        self.walk_expr(expr);
                    }
                }
            }
            Statement::Delete(d) => {
                self.metrics.hint_count += d.hints.len();
                for t in &d.tables {
                    self.count_table_ref(t);
                }
                for t in &d.using {
                    self.count_table_ref(t);
                }
                if let Some(w) = &d.where_clause {
                    self.metrics.where_condition_count += self.count_conditions(w);
                    self.walk_expr(w);
                }
                for target in &d.returning {
                    if let SelectTarget::Expr(expr, _) = target {
                        self.walk_expr(expr);
                    }
                }
            }
            Statement::Merge(m) => {
                self.metrics.hint_count += m.hints.len();
                self.count_table_ref(&m.target);
                self.count_table_ref(&m.source);
                self.metrics.join_count += 1;
                self.walk_expr(&m.on_condition);
            }
            Statement::Explain(e) => {
                self.visit_statement(&e.query);
            }
            Statement::CreateTable(ct) => {
                self.metrics.table_count += 1;
                self.metrics.column_count = ct.columns.len();
                for col in &ct.columns {
                    for constraint in &col.constraints {
                        match constraint {
                            ColumnConstraint::Default(_) => self.metrics.computed_column_count += 1,
                            ColumnConstraint::Check(_) => self.metrics.check_constraint_count += 1,
                            _ => {}
                        }
                    }
                }
                for constraint in &ct.constraints {
                    if matches!(constraint, TableConstraint::Check(_)) {
                        self.metrics.check_constraint_count += 1;
                    }
                }
            }
            _ => {}
        }
    }

    fn visit_select(&mut self, select: &SelectStatement) {
        self.metrics.hint_count += select.hints.len();

        if let Some(with) = &select.with {
            self.visit_with_clause(with);
        }

        for table_ref in &select.from {
            self.count_table_ref(table_ref);
        }

        self.metrics.set_operation_count += self.count_set_operations(select);

        self.metrics.has_group_by = !select.group_by.is_empty();
        self.metrics.has_order_by = !select.order_by.is_empty();
        self.metrics.has_distinct = select.distinct;

        if let Some(w) = &select.where_clause {
            self.metrics.where_condition_count += self.count_conditions(w);
            self.walk_expr(w);
        }

        for target in &select.targets {
            match target {
                SelectTarget::Expr(expr, _) => self.walk_expr(expr),
                SelectTarget::Star(_) => {}
            }
        }

        if let Some(h) = &select.having {
            self.walk_expr(h);
        }

        for item in &select.order_by {
            self.walk_expr(&item.expr);
        }

        for item in &select.group_by {
            match item {
                GroupByItem::Expr(expr) => self.walk_expr(expr),
                GroupByItem::GroupingSets(sets) => {
                    for set in sets {
                        for expr in set {
                            self.walk_expr(expr);
                        }
                    }
                }
                GroupByItem::Rollup(exprs) | GroupByItem::Cube(exprs) => {
                    for expr in exprs {
                        self.walk_expr(expr);
                    }
                }
            }
        }

        if let Some(l) = &select.limit {
            self.walk_expr(l);
        }
        if let Some(o) = &select.offset {
            self.walk_expr(o);
        }
    }

    fn visit_with_clause(&mut self, with: &WithClause) {
        self.metrics.cte_count += with.ctes.len();
        for cte in &with.ctes {
            self.cte_names.insert(cte.name.to_lowercase());
            self.visit_select(&cte.query);
        }
    }

    fn count_table_ref(&mut self, table_ref: &TableRef) {
        match table_ref {
            TableRef::Table { name, .. } => {
                let is_cte = name
                    .last()
                    .map(|n| self.cte_names.contains(&n.to_lowercase()))
                    .unwrap_or(false);
                if !is_cte {
                    self.metrics.table_count += 1;
                }
            }
            TableRef::FunctionCall { name: _, args, .. } => {
                for arg in args {
                    self.walk_expr(arg);
                }
            }
            TableRef::Subquery { query, .. } => {
                self.metrics.subquery_count += 1;
                self.visit_select(query);
            }
            TableRef::Join {
                left,
                right,
                condition,
                ..
            } => {
                self.metrics.join_count += 1;
                self.count_table_ref(left);
                self.count_table_ref(right);
                if let Some(cond) = condition {
                    self.walk_expr(cond);
                }
            }
            TableRef::Pivot { source, pivot } => {
                self.count_table_ref(source);
                self.walk_expr(&pivot.aggregate);
            }
            TableRef::Unpivot { source, .. } => {
                self.count_table_ref(source);
            }
            TableRef::Values { .. } => {}
        }
    }

    fn count_set_operations(&mut self, select: &SelectStatement) -> usize {
        match &select.set_operation {
            None => 0,
            Some(SetOperation::Union { right, .. })
            | Some(SetOperation::Intersect { right, .. })
            | Some(SetOperation::Except { right, .. }) => 1 + self.count_set_operations(right),
        }
    }

    fn count_conditions(&self, expr: &Expr) -> usize {
        if self.gaussdb_where_mode {
            return 1;
        }
        match expr {
            Expr::BinaryOp { op, left, right } => {
                let op_upper = op.to_uppercase();
                match op_upper.as_str() {
                    "AND" | "OR" => 1 + self.count_conditions(left) + self.count_conditions(right),
                    _ => 1,
                }
            }
            Expr::Parenthesized(inner) => self.count_conditions(inner),
            _ => 1,
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Case {
                operand,
                whens,
                else_expr,
            } => {
                self.metrics.case_expression_count += 1;
                if let Some(op) = operand {
                    self.walk_expr(op);
                }
                for when_clause in whens {
                    self.walk_expr(&when_clause.condition);
                    self.walk_expr(&when_clause.result);
                }
                if let Some(e) = else_expr {
                    self.walk_expr(e);
                }
            }
            Expr::Subquery(query) => {
                self.metrics.subquery_count += 1;
                let depth = self.current_depth + 1;
                if depth > self.metrics.subquery_depth {
                    self.metrics.subquery_depth = depth;
                }
                let saved = self.current_depth;
                self.current_depth = depth;
                self.visit_select(query);
                self.current_depth = saved;
            }
            Expr::Exists(query) => {
                self.metrics.subquery_count += 1;
                let depth = self.current_depth + 1;
                if depth > self.metrics.subquery_depth {
                    self.metrics.subquery_depth = depth;
                }
                let saved = self.current_depth;
                self.current_depth = depth;
                self.visit_select(query);
                self.current_depth = saved;
            }
            Expr::InSubquery { subquery, expr, .. } => {
                self.metrics.subquery_count += 1;
                let depth = self.current_depth + 1;
                if depth > self.metrics.subquery_depth {
                    self.metrics.subquery_depth = depth;
                }
                self.walk_expr(expr);
                let saved = self.current_depth;
                self.current_depth = depth;
                self.visit_select(subquery);
                self.current_depth = saved;
            }
            Expr::FunctionCall {
                name, over, args, ..
            } => {
                if over.is_some() {
                    self.metrics.window_function_count += 1;
                } else if is_aggregate(name) {
                    self.metrics.aggregate_function_count += 1;
                }
                for arg in args {
                    self.walk_expr(arg);
                }
                if let Some(spec) = over {
                    for p in &spec.partition_by {
                        self.walk_expr(p);
                    }
                    for o in &spec.order_by {
                        self.walk_expr(&o.expr);
                    }
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::UnaryOp { expr: inner, .. } => {
                self.walk_expr(inner);
            }
            Expr::Parenthesized(inner) => {
                self.walk_expr(inner);
            }
            Expr::TypeCast { expr: inner, .. } => {
                self.walk_expr(inner);
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                self.walk_expr(expr);
                self.walk_expr(low);
                self.walk_expr(high);
            }
            Expr::InList { expr, list, .. } => {
                self.walk_expr(expr);
                for item in list {
                    self.walk_expr(item);
                }
            }
            Expr::IsNull { expr: inner, .. } => {
                self.walk_expr(inner);
            }
            Expr::Array(exprs) | Expr::RowConstructor(exprs) => {
                for e in exprs {
                    self.walk_expr(e);
                }
            }
            Expr::Subscript {
                object,
                lower,
                upper,
                ..
            } => {
                self.walk_expr(object);
                if let Some(lo) = lower {
                    self.walk_expr(lo);
                }
                if let Some(hi) = upper {
                    self.walk_expr(hi);
                }
            }
            Expr::Prior(inner) => {
                self.walk_expr(inner);
            }
            Expr::SpecialFunction { args, .. } => {
                for arg in args {
                    self.walk_expr(arg);
                }
            }
            Expr::XmlElement { content, .. } => {
                for c in content {
                    self.walk_expr(&c.expr);
                }
            }
            Expr::XmlConcat(exprs) => {
                for e in exprs {
                    self.walk_expr(e);
                }
            }
            Expr::XmlForest(items) => {
                for item in items {
                    self.walk_expr(&item.expr);
                }
            }
            Expr::XmlParse { expr: inner, .. } => {
                self.walk_expr(inner);
            }
            Expr::XmlPi {
                content: Some(c), ..
            } => {
                self.walk_expr(c);
            }
            Expr::XmlRoot {
                expr: inner,
                version,
                ..
            } => {
                self.walk_expr(inner);
                if let Some(v) = version {
                    self.walk_expr(v);
                }
            }
            Expr::XmlSerialize { expr: inner, .. } => {
                self.walk_expr(inner);
            }
            // Leaf nodes: no children to recurse into
            Expr::Literal(_)
            | Expr::ColumnRef(_)
            | Expr::QualifiedStar(_)
            | Expr::Parameter(_)
            | Expr::Default => {}
            // Newer variants from updated ogsql-parser — no special handling needed
            _ => {}
        }
    }
}

fn is_aggregate(name: &ObjectName) -> bool {
    name.last()
        .map(|n| AGGREGATE_FUNCTIONS.contains(&n.to_uppercase().as_str()))
        .unwrap_or(false)
}
