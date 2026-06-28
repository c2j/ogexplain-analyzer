use crate::model::{NodeType, PlanNode, StreamingType};
use rust_i18n::t;

use super::super::context::{GlobalStats, PlanContext};
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};
use crate::model::ExplainPlan;

const REDISTRIBUTE_LARGE_ROWS: f64 = 10000.0;

// SKEW-001: Data skew detected via Streaming(Redistribute) nodes where
// actual rows >> estimated rows (5x underestimate proxy for uneven distribution).
pub struct DataSkewDetected;

impl DiagnosticRule for DataSkewDetected {
    fn id(&self) -> &str {
        "SKEW-001"
    }

    fn name(&self) -> String {
        t!("finding.SKEW-001.name").to_string()
    }

    fn severity(&self) -> Severity {
        Severity::Critical
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::DistributionIssue
    }

    fn check(&self, _node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        None
    }

    fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        let mut findings = Vec::new();
        Self::walk_node(&plan.root, &mut findings);
        findings
    }
}

impl DataSkewDetected {
    fn walk_node(node: &PlanNode, findings: &mut Vec<Finding>) {
        if let NodeType::Streaming(StreamingType::Redistribute) = node.node_type {
            if let (Some(actual), Some(estimated)) = (&node.actual, &node.estimated) {
                if actual.rows > 100000.0
                    && estimated.plan_rows > 0.0
                    && actual.rows / estimated.plan_rows > 5.0
                {
                    let ratio = actual.rows / estimated.plan_rows;
                    let relation = node.relation.as_deref().unwrap_or("unknown");
                    findings.push(make_finding(
                        &DataSkewDetected,
                        t!(
                            "finding.SKEW-001.detail",
                            rows = actual.rows as u64,
                            ratio = ratio
                        )
                        .to_string(),
                        node,
                        Some(t!("finding.SKEW-001.suggestion", relation = relation).to_string()),
                    ));
                }
            }
        }
        for child in &node.children {
            Self::walk_node(child, findings);
        }
    }
}

// DIST-001: Distribution column mismatch causing redistribution
pub struct DistributionColumnMismatch;

impl DiagnosticRule for DistributionColumnMismatch {
    fn id(&self) -> &str {
        "DIST-001"
    }
    fn name(&self) -> String {
        t!("finding.DIST-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::DistributionIssue
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if let NodeType::Streaming(StreamingType::Redistribute) = node.node_type {
            if let Some(actual) = &node.actual {
                if actual.rows > REDISTRIBUTE_LARGE_ROWS {
                    let relation = node.relation.as_deref().unwrap_or("unknown");
                    return Some(make_finding(
                        self,
                        t!("finding.DIST-001.detail", rows = actual.rows as u64).to_string(),
                        node,
                        Some(
                            t!(
                                "finding.DIST-001.suggestion",
                                rows = actual.rows as u64,
                                relation = relation
                            )
                            .to_string(),
                        ),
                    ));
                }
            }
        }
        None
    }

    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        Vec::new()
    }
}
