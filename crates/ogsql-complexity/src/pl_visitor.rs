use crate::model::{ComplexityConfig, ComplexityMetrics};
use ogsql_parser::ast::plpgsql::{PlBlock, PlDeclaration, PlOpenKind, PlStatement};

pub struct PlComplexityVisitor<'a> {
    metrics: &'a mut ComplexityMetrics,
    custom_functions: &'a [String],
    high_weight_tables: &'a [String],
    high_weight_procedures: &'a [String],
    builtin_functions: &'a [String],
    current_loop_depth: usize,
    current_cursor_depth: usize,
    current_savepoint_depth: usize,
}

impl<'a> PlComplexityVisitor<'a> {
    pub fn new(
        metrics: &'a mut ComplexityMetrics,
        custom_functions: &'a [String],
        high_weight_tables: &'a [String],
        high_weight_procedures: &'a [String],
        builtin_functions: &'a [String],
    ) -> Self {
        Self {
            metrics,
            custom_functions,
            high_weight_tables,
            high_weight_procedures,
            builtin_functions,
            current_loop_depth: 0,
            current_cursor_depth: 0,
            current_savepoint_depth: 0,
        }
    }

    pub fn visit_block(&mut self, block: &PlBlock) {
        self.process_declarations(&block.declarations);
        self.process_statements(&block.body);
        if let Some(exc) = &block.exception_block {
            self.metrics.subtransaction_count += 1;
            for handler in &exc.handlers {
                self.process_statements(&handler.statements);
            }
        }
    }

    fn process_declarations(&mut self, decls: &[PlDeclaration]) {
        for decl in decls {
            match decl {
                PlDeclaration::Cursor(_) => {
                    self.metrics.cursor_count += 1;
                }
                PlDeclaration::Pragma { name, .. } => {
                    if name.to_lowercase().contains("autonomous_transaction") {
                        self.metrics.uses_autonomous_transactions = true;
                    }
                }
                PlDeclaration::NestedProcedure(p) => {
                    if let Some(block) = &p.block {
                        self.visit_block(block);
                    }
                }
                PlDeclaration::NestedFunction(f) => {
                    if let Some(block) = &f.block {
                        self.visit_block(block);
                    }
                }
                _ => {}
            }
        }
    }

    fn process_statements(&mut self, stmts: &[PlStatement]) {
        for stmt in stmts {
            self.process_statement(stmt);
        }
    }

    fn process_statement(&mut self, stmt: &PlStatement) {
        match stmt {
            PlStatement::Loop(l) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&l.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::While(w) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&w.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::For(f) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&f.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::ForEach(f) => {
                self.metrics.loop_count += 1;
                self.current_loop_depth += 1;
                self.update_max_loop_depth();
                self.process_statements(&f.body);
                self.current_loop_depth -= 1;
            }
            PlStatement::ForAll(_) => {
                self.metrics.loop_count += 1;
            }
            PlStatement::Block(b) => {
                self.current_cursor_depth += 1;
                self.update_max_cursor_depth();
                self.visit_block(b);
                self.current_cursor_depth -= 1;
            }
            PlStatement::Open(o) => {
                self.metrics.cursor_operation_count += 1;
                if matches!(o.kind, PlOpenKind::ForQuery { .. }) {
                    if let PlOpenKind::ForQuery { parsed_query, .. } = &o.kind {
                        if parsed_query.is_some() {
                            self.metrics.dynamic_sql_count += 1;
                        }
                    }
                }
            }
            PlStatement::Fetch(_) => {
                self.metrics.cursor_operation_count += 1;
            }
            PlStatement::Close { .. } => {
                self.metrics.cursor_operation_count += 1;
            }
            PlStatement::Move { .. } => {
                self.metrics.cursor_operation_count += 1;
            }
            PlStatement::Execute(e) => {
                self.metrics.dynamic_sql_count += 1;
                self.metrics.param_binding_count += e.using_args.len();
                if e.parsed_query.is_some() {
                    self.metrics.nested_dynamic_sql_count += 1;
                }
            }
            PlStatement::Commit => {
                self.metrics.transaction_control_count += 1;
            }
            PlStatement::Rollback { to_savepoint } => {
                self.metrics.transaction_control_count += 1;
                if to_savepoint.is_some() {
                    self.metrics.subtransaction_count += 1;
                }
            }
            PlStatement::Savepoint { .. } => {
                self.metrics.transaction_control_count += 1;
                self.metrics.subtransaction_count += 1;
                self.current_savepoint_depth += 1;
                self.update_max_savepoint_depth();
                self.current_savepoint_depth -= 1;
            }
            PlStatement::ProcedureCall(call) => {
                let name_str = call
                    .name
                    .last()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                if self
                    .custom_functions
                    .iter()
                    .any(|f| f.to_lowercase() == name_str)
                {
                    self.metrics.custom_function_count += 1;
                } else if !self.is_builtin(&name_str) {
                    self.metrics.nested_procedure_count += 1;
                    if self
                        .high_weight_procedures
                        .iter()
                        .any(|p| p.to_lowercase() == name_str)
                    {
                        self.metrics.high_weight_procedure_count += 1;
                    }
                }
            }
            PlStatement::If(i) => {
                self.process_statements(&i.then_stmts);
                for e in &i.elsifs {
                    self.process_statements(&e.stmts);
                }
                self.process_statements(&i.else_stmts);
            }
            PlStatement::Case(c) => {
                for w in &c.whens {
                    self.process_statements(&w.stmts);
                }
                self.process_statements(&c.else_stmts);
            }
            PlStatement::Perform { query, .. } => {
                self.metrics.line_count += query.lines().count();
            }
            PlStatement::Sql(text) => {
                self.metrics.line_count += text.lines().count();
            }
            PlStatement::SqlStatement { sql_text, .. } => {
                self.metrics.line_count += sql_text.lines().count();
            }
            _ => {}
        }
    }

    fn update_max_loop_depth(&mut self) {
        if self.current_loop_depth > self.metrics.max_loop_nesting_level {
            self.metrics.max_loop_nesting_level = self.current_loop_depth;
        }
    }

    fn update_max_cursor_depth(&mut self) {
        if self.current_cursor_depth > self.metrics.max_cursor_nesting_level {
            self.metrics.max_cursor_nesting_level = self.current_cursor_depth;
        }
    }

    fn update_max_savepoint_depth(&mut self) {
        if self.current_savepoint_depth > self.metrics.max_subtransaction_nesting_level {
            self.metrics.max_subtransaction_nesting_level = self.current_savepoint_depth;
        }
    }

    fn is_builtin(&self, name: &str) -> bool {
        self.builtin_functions
            .iter()
            .any(|f| f.to_lowercase() == name)
    }
}

pub fn analyze_pl_block(block: &PlBlock, config: &ComplexityConfig) -> ComplexityMetrics {
    let mut metrics = ComplexityMetrics::default();
    {
        let mut visitor = PlComplexityVisitor::new(
            &mut metrics,
            &config.custom_functions,
            &config.high_weight_tables,
            &config.high_weight_procedures,
            &config.builtin_functions,
        );
        visitor.visit_block(block);
    }
    metrics
}
