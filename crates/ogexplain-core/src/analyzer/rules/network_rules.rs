use crate::model::{NodeType, PlanNode, StreamingType};
use rust_i18n::t;

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::first_identifier;
use super::{make_finding, DiagnosticRule};

pub struct BroadcastLargeTable {
    threshold: f64,
}

impl BroadcastLargeTable {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.large_table_rows,
        }
    }
}

impl DiagnosticRule for BroadcastLargeTable {
    fn id(&self) -> &str {
        "NET-001"
    }
    fn name(&self) -> String {
        t!("finding.NET-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::NetworkOverhead
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        let is_broadcast = match &node.node_type {
            NodeType::Streaming(stype) => matches!(
                stype,
                StreamingType::Broadcast
                    | StreamingType::SplitBroadcast
                    | StreamingType::PartRedistributePartBroadcast
            ),
            _ => false,
        };
        if !is_broadcast {
            return None;
        }
        let actual = node.actual.as_ref()?;
        if actual.rows <= self.threshold {
            return None;
        }

        let table = find_child_table_name(node).unwrap_or_else(|| "unknown".to_string());

        let detail = t!(
            "finding.NET-001.detail",
            table = table,
            rows = actual.rows,
            threshold = self.threshold
        )
        .to_string();
        let suggestion = t!("finding.NET-001.suggestion", table = table).to_string();

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn find_child_table_name(node: &PlanNode) -> Option<String> {
    for child in &node.children {
        if let Some(ref rel) = child.relation {
            return Some(first_identifier(rel));
        }
        if let Some(name) = find_child_table_name(child) {
            return Some(name);
        }
    }
    None
}
