use crate::model::{NodeType, PlanNode};
use rust_i18n::t;

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
    fn name(&self) -> String {
        t!("finding.EST-001.name").to_string()
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
            t!("finding.EST-001.direction_under")
        } else {
            t!("finding.EST-001.direction_over")
        };

        let detail = t!(
            "finding.EST-001.detail",
            relation = relation,
            actual = actual.rows,
            estimated = estimated.plan_rows,
            ratio = ratio,
            direction = direction
        )
        .to_string();

        let suggestion = t!(
            "finding.EST-001.suggestion",
            relation = relation,
            ratio = ratio,
            direction = direction
        )
        .to_string();

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
    fn name(&self) -> String {
        t!("finding.EST-004.name").to_string()
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

        let detail = t!(
            "finding.EST-004.detail",
            actual = actual.rows,
            estimated = estimated.plan_rows,
            ratio = ratio,
            inner_work = inner_work
        )
        .to_string();
        let suggestion = t!("finding.EST-004.suggestion", inner_work = inner_work).to_string();

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}
