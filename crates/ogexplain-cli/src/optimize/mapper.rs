//! Maps ogexplain Finding.rule_id to metamorphosis rewrite rules or advisory actions.
//!
//! Used by the `optimize` subcommand to decide what to do with each finding.
//! Mapping table design: see Heptadecagon `docs/closed-loop-optimization-design.md` §5.2.

use ogexplain_core::analyzer::report::Finding;
use ogexplain_core::DiagnosticHint;

/// Action to take for a finding.
#[derive(Debug, Clone, PartialEq)]
pub enum RemediationAction {
    /// Call metamorphosis with the listed rule IDs.
    Rewrite { rules: Vec<&'static str> },
    /// Use ogexplain's built-in sql_rewrite (e.g. SUBQ-006).
    UseBuiltinRewrite,
    /// Output a DDL suggestion (CREATE INDEX, etc.) — no auto-execution.
    DdlAdvice,
    /// Output a configuration suggestion (SET work_mem, etc.).
    ConfigAdvice,
    /// Run ANALYZE then retry (Phase 0).
    RunAnalyze,
    /// Architectural — record warning, requires human.
    Log,
}

/// Map a diagnostic rule_id to its remediation action.
pub fn map_diagnostic(rule_id: &str) -> RemediationAction {
    match rule_id {
        "SUBQ-001" | "REW-001" => RemediationAction::Rewrite {
            rules: vec!["subquery-to-join"],
        },
        "SUBQ-006" => RemediationAction::UseBuiltinRewrite,
        "TYPE-001" => RemediationAction::Rewrite {
            rules: vec!["add-explicit-cast"],
        },
        "TYPE-004" => RemediationAction::Rewrite {
            rules: vec!["suggest-trgm-index"],
        },
        "AGG-001" => RemediationAction::Rewrite {
            rules: vec!["rewrite-group-agg"],
        },
        "SCAN-001" | "SCAN-004" | "JOIN-001" => RemediationAction::DdlAdvice,
        "MEM-001" | "MEM-004" | "JOIN-002" | "AGG-002" => RemediationAction::ConfigAdvice,
        "STATS-001" | "EST-001" | "EST-004" => RemediationAction::RunAnalyze,
        _ => RemediationAction::Log,
    }
}

/// Filter findings to those with a rewrite action.
///
/// Quality gate: SUBQ-001 findings are only included when `table` is present
/// (not None). v4 benchmark shows SUBQ-001 precision = 0.43 (4 FP / 7 total);
/// requiring non-None table filters cases where no scan descendant was located.
pub fn filter_rewritable(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|f| {
            let action = map_diagnostic(&f.rule_id);
            if !matches!(
                action,
                RemediationAction::Rewrite { .. } | RemediationAction::UseBuiltinRewrite
            ) {
                return false;
            }
            if f.rule_id == "SUBQ-001" && f.table.is_none() {
                return false;
            }
            true
        })
        .collect()
}

/// Convert a finding to a DiagnosticHint for metamorphosis consumption.
pub fn finding_to_hint(f: &Finding) -> Option<DiagnosticHint> {
    Some(DiagnosticHint::from_finding(f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogexplain_core::analyzer::report::{DiagnosticCategory, Finding, Severity};

    fn make_finding(rule_id: &str, table: Option<&str>) -> Finding {
        Finding {
            rule_id: rule_id.into(),
            severity: Severity::Warning,
            category: DiagnosticCategory::SubqueryStructure,
            title: "t".into(),
            detail: "d".into(),
            node_line: None,
            node_type: None,
            suggestion: None,
            sql_rewrite: None,
            evidence: None,
            table: table.map(String::from),
            columns: Vec::new(),
        }
    }

    #[test]
    fn map_subq_001_to_subquery_to_join() {
        assert!(matches!(
            map_diagnostic("SUBQ-001"),
            RemediationAction::Rewrite { rules } if rules == vec!["subquery-to-join"]
        ));
    }

    #[test]
    fn map_subq_006_to_builtin_rewrite() {
        assert!(matches!(
            map_diagnostic("SUBQ-006"),
            RemediationAction::UseBuiltinRewrite
        ));
    }

    #[test]
    fn map_scan_001_to_ddl_advice() {
        assert!(matches!(
            map_diagnostic("SCAN-001"),
            RemediationAction::DdlAdvice
        ));
    }

    #[test]
    fn map_unknown_rule_to_log() {
        assert!(matches!(
            map_diagnostic("UNKNOWN-999"),
            RemediationAction::Log
        ));
    }

    #[test]
    fn filter_subq_001_without_table() {
        let f = make_finding("SUBQ-001", None);
        assert!(
            filter_rewritable(&[f]).is_empty(),
            "SUBQ-001 without table must be filtered"
        );
    }

    #[test]
    fn filter_subq_001_with_table_passes() {
        let f = make_finding("SUBQ-001", Some("items"));
        assert_eq!(filter_rewritable(&[f]).len(), 1);
    }

    #[test]
    fn filter_excludes_ddl_only_rules() {
        let f = make_finding("SCAN-001", Some("orders"));
        assert!(
            filter_rewritable(&[f]).is_empty(),
            "SCAN-001 is DdlAdvice, not rewritable"
        );
    }

    #[test]
    fn filter_includes_type_001() {
        let f = make_finding("TYPE-001", Some("accounts"));
        assert_eq!(filter_rewritable(&[f]).len(), 1);
    }

    #[test]
    fn finding_to_hint_preserves_table() {
        let f = make_finding("SUBQ-001", Some("items"));
        let hint = finding_to_hint(&f).unwrap();
        assert_eq!(hint.rule_id, "SUBQ-001");
        assert_eq!(hint.table.as_deref(), Some("items"));
    }
}
