use crate::model::{NodeType, PlanNode};

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

pub struct SevereRowUnderestimation {
    factor: f64,
}

impl SevereRowUnderestimation {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            factor: config.estimation_skew_factor,
        }
    }
}

impl DiagnosticRule for SevereRowUnderestimation {
    fn id(&self) -> &str {
        "EST-001"
    }
    fn name(&self) -> &str {
        "Severe row underestimation"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::CostMisestimation
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        let estimated = node.estimated.as_ref()?;
        let actual = node.actual.as_ref()?;
        if estimated.plan_rows <= 0.0 || actual.rows <= 0.0 {
            return None;
        }
        let ratio = actual.rows / estimated.plan_rows;
        if ratio <= self.factor {
            return None;
        }
        let type_str = node.node_type.to_string();
        let relation = node.relation.as_deref().unwrap_or(&type_str);
        Some(make_finding(
            self,
            format!(
                "{}: actual {} rows vs estimated {} rows ({:.1}x off)",
                relation, actual.rows, estimated.plan_rows, ratio
            ),
            node,
            Some(format!(
                "ANALYZE {}; actual {} vs estimated {} ({:.1}x off)",
                relation, actual.rows, estimated.plan_rows, ratio
            )),
        ))
    }
}

pub struct NestedLoopFromUnderestimation {
    factor: f64,
}

impl NestedLoopFromUnderestimation {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            factor: config.estimation_skew_factor,
        }
    }
}

impl DiagnosticRule for NestedLoopFromUnderestimation {
    fn id(&self) -> &str {
        "EST-004"
    }
    fn name(&self) -> &str {
        "Nested Loop from underestimation"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::CostMisestimation
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::NestedLoop {
            return None;
        }
        let estimated = node.estimated.as_ref()?;
        let actual = node.actual.as_ref()?;
        if estimated.plan_rows <= 0.0 || actual.rows <= 0.0 {
            return None;
        }
        let ratio = actual.rows / estimated.plan_rows;
        if ratio <= self.factor {
            return None;
        }
        Some(make_finding(
            self,
            format!(
                "Nested Loop chosen due to severe underestimation: actual {} rows vs estimated {} ({:.1}x off)",
                actual.rows, estimated.plan_rows, ratio
            ),
            node,
            Some("ANALYZE tables involved in join; consider SET enable_nestloop = off".to_string()),
        ))
    }
}
