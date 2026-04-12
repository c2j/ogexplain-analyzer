use crate::model::{NodeType, PlanNode};

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

    fn name(&self) -> &str {
        "统计信息未收集"
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
            format!(
                "表 {} 估算行数为10(系统默认值), 实际返回 {} 行 — 疑似统计信息未收集",
                relation, actual.rows
            ),
            node,
            Some(format!(
                "表 {} 的统计信息可能未收集(估算行数=10为系统默认值); 执行 ANALYZE {} 收集统计信息; 大表(>160万行): SET default_statistics_target=-2 (2%采样)",
                relation, relation
            )),
        ))
    }
}
