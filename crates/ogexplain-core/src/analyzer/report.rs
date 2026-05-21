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
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiagnosticReport {
    pub findings: Vec<Finding>,
    pub stats: super::context::GlobalStats,
}
