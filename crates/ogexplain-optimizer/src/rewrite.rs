//! SQL↔AST encapsulation for metamorphosis RewriteEngine integration.
//!
//! Pipeline: parse → rewrite → format, bridging between raw SQL strings
//! and metamorphosis's AST-based RewriteEngine API.
//!
//! The main entry point is [`rewrite_sql`], which takes raw SQL, a schema,
//! diagnostic hints, and rule name filters, and returns the rewritten SQL.

use thiserror::Error;

/// Errors that can occur during SQL rewriting.
#[derive(Debug, Error)]
pub enum RewriteError {
    /// SQL could not be parsed by ogsql-parser.
    #[error("SQL parse error(s): {0}")]
    Parse(String),
    /// Rewrite engine produced no change.
    #[error("Rewrite produced no change")]
    NoChange,
    /// Rewrite engine returned an error.
    #[error("Rewrite engine error: {0}")]
    Engine(String),
}

/// Rewrite a SQL string using metamorphosis's library API.
///
/// # Arguments
///
/// * `sql` - Raw SQL string to rewrite.
/// * `schema` - Optional schema map (`HashMap<table, HashMap<column, type>>`)
///   for context-aware rewriting (e.g. SELECT * expansion).
///   Note: this simplified format is compatible with metamorphosis RewriteContext
///   which accepts any HashMap-based schema.
/// * `hints` - Diagnostic hints from ogexplain to guide metamorphosis rewrite
///   rules (e.g. table name, column names, severity). Pass `&[]` if none.
/// * `rules` - Names of metamorphosis rewrite rules to apply (subset of
///   built-in rules returned by [`metamorphosis_rules::builtin_rules`]).
///
/// # Returns
///
/// * `Ok(Some(rewritten_sql))` — rewrite was applied and produced changes.
/// * `Ok(None)` — rewrite ran but produced identical SQL (no change).
/// * `Err(RewriteError)` — parsing or rewrite engine failure.
pub fn rewrite_sql(
    sql: &str,
    schema: Option<&std::collections::HashMap<String, std::collections::HashMap<String, String>>>,
    rules: &[&str],
) -> Result<Option<String>, RewriteError> {
    // 1. Parse SQL
    let (stmts, errors) = ogsql_parser::parser::Parser::parse_sql(sql);
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        return Err(RewriteError::Parse(msgs.join("; ")));
    }

    // Extract Statements from StatementInfo
    let statements: Vec<_> = stmts.into_iter().map(|info| info.statement).collect();

    if statements.is_empty() {
        return Err(RewriteError::Parse("no SQL statements found".into()));
    }

    // 2. Build rule registry with only the requested rules
    let all_rules = metamorphosis_rules::builtin_rules();
    let filtered: Vec<Box<dyn metamorphosis_core::registry::RewriteRule>> = all_rules
        .into_iter()
        .filter(|r| rules.contains(&r.id()))
        .collect();

    if filtered.is_empty() {
        return Err(RewriteError::Engine(format!(
            "no metamorphosis rules matched the requested rule names: {:?}",
            rules
        )));
    }

    let registry = metamorphosis_core::registry::RuleRegistry::new(filtered);
    let engine = metamorphosis_core::engine::RewriteEngine::new(registry);

    // 3. Build rewrite context
    // diagnostic_hints is required in metamorphosis v0.2.1+
    let ctx = metamorphosis_core::context::RewriteContext {
        version: None,
        schema,
        config: &metamorphosis_core::context::RewriteConfig::default(),
        source_file: None,
        known_variables: None,
        diagnostic_hints: None,
    };

    // 4. Execute rewrite (returns RewriteResult directly, not Result)
    let rewrite_result = engine.rewrite(&ctx, statements);

    // Check if changes were made
    if !rewrite_result.changed {
        return Ok(None);
    }

    // 5. Format rewritten statements back to SQL
    let formatter = ogsql_parser::formatter::SqlFormatter::new();
    let rewritten_sql: String = rewrite_result
        .statements
        .iter()
        .map(|stmt| formatter.format_statement(stmt))
        .collect::<Vec<_>>()
        .join("\n");

    if rewritten_sql.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(rewritten_sql))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_error_display_parse() {
        let err = RewriteError::Parse("syntax error at line 1".into());
        assert!(err.to_string().contains("syntax error"));
    }

    #[test]
    fn rewrite_error_display_no_change() {
        let err = RewriteError::NoChange;
        assert_eq!(err.to_string(), "Rewrite produced no change");
    }

    #[test]
    fn rewrite_error_display_engine() {
        let err = RewriteError::Engine("internal error".into());
        assert!(err.to_string().contains("internal error"));
    }

    #[test]
    fn rewrite_sql_parse_error_returns_err() {
        let result = rewrite_sql("SELECT invalid sql syntax $$$", None, &["subquery-to-join"]);
        assert!(
            result.is_err(),
            "invalid SQL should return Err, got {:?}",
            result
        );
    }

    #[test]
    fn rewrite_sql_empty_rules_returns_err() {
        let result = rewrite_sql("SELECT 1", None, &[]);
        assert!(
            result.is_err(),
            "empty rules should return Err, got {:?}",
            result
        );
    }

    #[test]
    fn rewrite_sql_nonexistent_rules_returns_err() {
        let result = rewrite_sql("SELECT 1", None, &["nonexistent-rule"]);
        assert!(
            result.is_err(),
            "nonexistent rules should return Err, got {:?}",
            result
        );
    }

    #[test]
    fn rewrite_error_parse_no_statement() {
        let err = RewriteError::Parse("no SQL statements found".into());
        assert!(err.to_string().contains("no SQL statements"));
    }

    #[test]
    fn rewrite_error_no_change_debug() {
        let err = RewriteError::NoChange;
        assert_eq!(format!("{err:?}"), "NoChange");
    }
}
