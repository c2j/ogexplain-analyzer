use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AntiPatternInfo {
    pub target_table: String,
    pub subquery_table: String,
    pub correlation_columns: Vec<String>,
    pub set_columns: Vec<String>,
    pub uses_row_constructor: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RewriteResult {
    pub strategy: RewriteStrategy,
    pub rewritten_sql: String,
    pub explanation: String,
    pub pattern_info: AntiPatternInfo,
}

/// `UPDATE t SET ... FROM (SELECT ...) sub WHERE t.key = sub.key`
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum RewriteStrategy {
    UpdateFrom,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum RewriteError {
    ParseError(String),
    PatternNotFound,
    UnsupportedSyntax(String),
}
