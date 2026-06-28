use crate::model::{NodeType, PlanNode};
use rust_i18n::t;

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

// STATS-001: OpenGauss uses plan_rows=10 as default when no ANALYZE has been run.
// Require actual.rows > 1000 to avoid false positives on genuinely small tables.
pub struct StatsNotCollected;

impl DiagnosticRule for StatsNotCollected {
    fn id(&self) -> &str {
        "STATS-001"
    }

    fn name(&self) -> String {
        t!("finding.STATS-001.name").to_string()
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::CostMisestimation
    }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan {
            return None;
        }
        let estimated = node.estimated.as_ref()?;
        let actual = node.actual.as_ref()?;

        if estimated.plan_rows != 10.0 {
            return None;
        }
        if actual.rows <= 1000.0 {
            return None;
        }

        let relation = node.relation.as_deref().unwrap_or("unknown");
        Some(make_finding(
            self,
            t!(
                "finding.STATS-001.detail",
                relation = relation,
                actual = actual.rows
            )
            .to_string(),
            node,
            Some(t!("finding.STATS-001.suggestion", relation = relation).to_string()),
        ))
    }
}
