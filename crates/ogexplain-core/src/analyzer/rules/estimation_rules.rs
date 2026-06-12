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

        let direction = if actual.rows > estimated.plan_rows {
            "低估"
        } else {
            "高估"
        };

        let detail = format!(
            "{}: actual {} rows vs estimated {} rows ({:.1}x {})",
            relation, actual.rows, estimated.plan_rows, ratio, direction
        );

        let suggestion = format!(
            "ANALYZE {}; 估算偏差 {:.1}x ({}), 更新统计信息以改善查询计划选择",
            relation, ratio, direction
        );

        Some(make_finding(self, detail, node, Some(suggestion)))
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

        let inner_work: f64 = node
            .children
            .iter()
            .filter_map(|c| c.actual.as_ref().map(|a| a.rows * a.loops))
            .sum();

        let detail = format!(
            "Nested Loop 因严重低估而选择: actual {} vs estimated {} ({:.1}x), 内表总工作量: {} rows",
            actual.rows, estimated.plan_rows, ratio, inner_work
        );

        let suggestion = format!(
            "ANALYZE 更新统计信息; 考虑 SET enable_nestloop = off; 内表工作量: {} rows",
            inner_work
        );

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}
