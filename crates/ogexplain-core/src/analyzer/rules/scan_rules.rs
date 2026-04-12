use crate::model::{NodeType, PlanNode};

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

pub struct LargeTableFullScan {
    threshold: f64,
}

impl LargeTableFullScan {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.large_table_rows,
        }
    }
}

impl DiagnosticRule for LargeTableFullScan {
    fn id(&self) -> &str {
        "SCAN-001"
    }
    fn name(&self) -> &str {
        "Large table full scan"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::ScanEfficiency
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan {
            return None;
        }
        let actual = node.actual.as_ref()?;
        if actual.rows <= self.threshold {
            return None;
        }
        let relation = node.relation.as_deref().unwrap_or("unknown");
        Some(make_finding(
            self,
            format!(
                "Seq Scan on {} returned {} rows (threshold: {})",
                relation, actual.rows, self.threshold
            ),
            node,
            Some(format!(
                "Consider creating an index on the filtered columns of {}",
                relation
            )),
        ))
    }
}

pub struct FilterWithoutIndex {
    estimation_ratio: f64,
}

impl FilterWithoutIndex {
    pub fn new(_config: DiagnosticConfig) -> Self {
        Self {
            estimation_ratio: 10.0,
        }
    }
}

impl DiagnosticRule for FilterWithoutIndex {
    fn id(&self) -> &str {
        "SCAN-004"
    }
    fn name(&self) -> &str {
        "Filter without index"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::ScanEfficiency
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan {
            return None;
        }
        let has_filter = node.properties.iter().any(|p| p.label == "Filter");
        if !has_filter {
            return None;
        }
        let estimated = node.estimated.as_ref()?;
        let actual = node.actual.as_ref()?;
        if estimated.plan_rows <= 0.0 || actual.rows <= 0.0 {
            return None;
        }
        let ratio = estimated.plan_rows / actual.rows;
        if ratio <= self.estimation_ratio {
            return None;
        }
        let relation = node.relation.as_deref().unwrap_or("unknown");
        Some(make_finding(
            self,
            format!(
                "Seq Scan on {} with Filter: estimated {} rows but got {} (ratio: {:.1}x)",
                relation, estimated.plan_rows, actual.rows, ratio
            ),
            node,
            Some(format!("ANALYZE {}", relation)),
        ))
    }
}
