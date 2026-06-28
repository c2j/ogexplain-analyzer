use crate::model::PlanNode;
use rust_i18n::t;

use super::super::config::DiagnosticConfig;
use super::super::context::{GlobalStats, PlanContext};
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::DiagnosticRule;

pub struct PlanTooDeep {
    max_depth: usize,
}

impl PlanTooDeep {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            max_depth: config.max_plan_depth,
        }
    }
}

impl DiagnosticRule for PlanTooDeep {
    fn id(&self) -> &str {
        "GEN-001"
    }
    fn name(&self) -> String {
        t!("finding.GEN-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::General
    }
    fn check(&self, _node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        None
    }
    fn check_global(&self, _plan: &crate::model::ExplainPlan, stats: &GlobalStats) -> Vec<Finding> {
        if stats.max_depth <= self.max_depth {
            return Vec::new();
        }

        vec![Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail: t!(
                "finding.GEN-001.detail",
                depth = stats.max_depth,
                max_depth = self.max_depth
            )
            .to_string(),
            node_line: None,
            node_type: None,
            suggestion: Some(t!("finding.GEN-001.suggestion").to_string()),
            sql_rewrite: None,
            evidence: None,
        }]
    }
}
