use crate::model::{NodeType, PlanNode};
use rust_i18n::t;

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

// AGG-001: GroupAggregate on large datasets is slower than HashAggregate.
// Triggers when GroupAggregate has a Sort child AND actual.rows > 10000.
pub struct GroupAggregateShouldBeHash;

impl DiagnosticRule for GroupAggregateShouldBeHash {
    fn id(&self) -> &str {
        "AGG-001"
    }

    fn name(&self) -> String {
        t!("finding.AGG-001.name").to_string()
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::MemoryUsage
    }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::GroupAggregate {
            return None;
        }
        let actual = node.actual.as_ref()?;
        if actual.rows <= 10000.0 {
            return None;
        }
        let has_sort_child = node.children.iter().any(|c| c.node_type == NodeType::Sort);
        if !has_sort_child {
            return None;
        }

        Some(make_finding(
            self,
            t!("finding.AGG-001.detail", rows = actual.rows as u64).to_string(),
            node,
            Some(t!("finding.AGG-001.suggestion").to_string()),
        ))
    }
}

// AGG-002: HashAggregate spilling to disk (Batches > 1).
pub struct HashAggregateSpillToDisk;

impl DiagnosticRule for HashAggregateSpillToDisk {
    fn id(&self) -> &str {
        "AGG-002"
    }

    fn name(&self) -> String {
        t!("finding.AGG-002.name").to_string()
    }

    fn severity(&self) -> Severity {
        Severity::Critical
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::MemoryUsage
    }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::HashAggregate {
            return None;
        }
        let sp = node.structured_props.as_ref()?;
        let batches = sp.hash_batches?;
        if batches <= 1 {
            return None;
        }

        Some(make_finding(
            self,
            t!("finding.AGG-002.detail", batches = batches).to_string(),
            node,
            Some(t!("finding.AGG-002.suggestion").to_string()),
        ))
    }
}
