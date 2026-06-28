//! Diagnostic hint passed to metamorphosis's `RewriteContext.diagnostic_hints`.
//!
//! Cross-tool contract: ogexplain populates this from [`Finding`] data;
//! metamorphosis consumes it to direct rewrite rules. Field names/types
//! align with metamorphosis's `DiagnosticHint` (PR #35).

use serde::{Deserialize, Serialize};

use crate::analyzer::report::Finding;

/// Hint describing one diagnostic finding, for consumption by external
/// rewrite tools (currently metamorphosis).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticHint {
    /// Source diagnostic rule ID (e.g. "SUBQ-001", "TYPE-001").
    pub rule_id: String,
    /// Related table name (extracted from the execution plan).
    pub table: Option<String>,
    /// Related column names.
    pub columns: Vec<String>,
    /// Severity as lowercase string ("critical" / "warning" / "info").
    pub severity: String,
    /// Diagnostic detail text.
    pub detail: String,
}

impl DiagnosticHint {
    /// Build a hint from a [`Finding`]'s structured fields.
    pub fn from_finding(f: &Finding) -> Self {
        Self {
            rule_id: f.rule_id.clone(),
            table: f.table.clone(),
            columns: f.columns.clone(),
            severity: f.severity.as_str().to_string(),
            detail: f.detail.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::report::{DiagnosticCategory, Finding, Severity};

    fn make_finding(rule_id: &str, table: Option<&str>, columns: Vec<&str>) -> Finding {
        Finding {
            rule_id: rule_id.into(),
            severity: Severity::Critical,
            category: DiagnosticCategory::SubqueryStructure,
            title: "t".into(),
            detail: "test detail".into(),
            node_line: None,
            node_type: None,
            suggestion: None,
            sql_rewrite: None,
            evidence: None,
            table: table.map(String::from),
            columns: columns.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn from_finding_preserves_all_fields() {
        let f = make_finding("SUBQ-001", Some("items"), vec!["order_id"]);
        let hint = DiagnosticHint::from_finding(&f);
        assert_eq!(hint.rule_id, "SUBQ-001");
        assert_eq!(hint.table.as_deref(), Some("items"));
        assert_eq!(hint.columns, vec!["order_id".to_string()]);
        assert_eq!(hint.severity, "critical");
        assert_eq!(hint.detail, "test detail");
    }

    #[test]
    fn from_finding_with_none_table() {
        let f = make_finding("X", None, vec![]);
        let hint = DiagnosticHint::from_finding(&f);
        assert!(hint.table.is_none());
        assert!(hint.columns.is_empty());
    }

    #[test]
    fn serializes_to_json_cleanly() {
        let f = make_finding("SUBQ-001", Some("items"), vec!["order_id"]);
        let hint = DiagnosticHint::from_finding(&f);
        let json = serde_json::to_string(&hint).unwrap();
        assert!(json.contains(r#""rule_id":"SUBQ-001""#));
        assert!(json.contains(r#""table":"items""#));
    }
}
