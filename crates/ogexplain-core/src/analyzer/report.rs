use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

impl Ord for Severity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let order = |s: &Self| match s {
            Self::Critical => 0,
            Self::Warning => 1,
            Self::Info => 2,
        };
        order(self).cmp(&order(other))
    }
}

impl PartialOrd for Severity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Severity {}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum DiagnosticCategory {
    ScanEfficiency,
    JoinStrategy,
    MemoryUsage,
    SortEfficiency,
    NetworkOverhead,
    CostMisestimation,
    PushdownFailure,
    TypeMismatch,
    Vectorization,
    SubqueryStructure,
    DistributionIssue,
    General,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub category: DiagnosticCategory,
    pub title: String,
    pub detail: String,
    pub node_line: Option<usize>,
    pub node_type: Option<String>,
    pub suggestion: Option<String>,
    pub sql_rewrite: Option<crate::rewriter::types::RewriteResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<crate::analyzer::pattern::types::Evidence>,
    /// 关联表名（从计划节点提取，用于下游工具定向重写）。
    /// None 表示该规则未提取表名（向后兼容）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// 关联列名（从过滤/连接条件提取）。
    /// 空表示该规则未提取列名（向后兼容）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiagnosticReport {
    pub findings: Vec<Finding>,
    pub stats: super::context::GlobalStats,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sample_finding(table: Option<&str>, columns: Vec<&str>) -> Finding {
        Finding {
            rule_id: "TEST".into(),
            severity: Severity::Info,
            category: DiagnosticCategory::General,
            title: "t".into(),
            detail: "d".into(),
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
    fn finding_table_defaults_to_none() {
        let f = make_sample_finding(None, vec![]);
        assert!(f.table.is_none());
        assert!(f.columns.is_empty());
    }

    #[test]
    fn finding_json_skips_none_table_and_empty_columns() {
        let f = make_sample_finding(None, vec![]);
        let json = serde_json::to_string(&f).unwrap();
        assert!(
            !json.contains("table"),
            "must skip None table, got: {}",
            json
        );
        assert!(
            !json.contains("columns"),
            "must skip empty columns, got: {}",
            json
        );
    }

    #[test]
    fn finding_json_includes_table_when_some() {
        let f = make_sample_finding(Some("orders"), vec!["status"]);
        let json = serde_json::to_string(&f).unwrap();
        assert!(
            json.contains(r#""table":"orders""#),
            "must include table, got: {}",
            json
        );
        assert!(
            json.contains(r#""columns":["status"]"#),
            "must include columns, got: {}",
            json
        );
    }

    #[test]
    fn finding_partial_eq_compares_table_and_columns() {
        let a = make_sample_finding(Some("t"), vec!["c"]);
        let b = make_sample_finding(Some("t"), vec!["c"]);
        let c = make_sample_finding(Some("other"), vec!["c"]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
