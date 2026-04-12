use crate::model::{NodeType, PlanNode};

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

    fn name(&self) -> &str {
        "聚合策略不当 — 应使用HashAggregate"
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
            format!(
                "GroupAggregate在{}行数据上使用排序聚合(含Sort子节点)",
                actual.rows as u64
            ),
            node,
            Some(
                "增大work_mem可切换为HashAggregate: /*+ set(work_mem '256MB') */; 或使用Hint: /*+ use_hash_agg */".to_string(),
            ),
        ))
    }
}

// AGG-002: HashAggregate spilling to disk (Batches > 1).
pub struct HashAggregateSpillToDisk;

impl DiagnosticRule for HashAggregateSpillToDisk {
    fn id(&self) -> &str {
        "AGG-002"
    }

    fn name(&self) -> &str {
        "HashAggregate磁盘溢出"
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
            format!(
                "HashAggregate溢出到磁盘({}个批次)",
                batches
            ),
            node,
            Some(
                "增大work_mem: /*+ set(work_mem '256MB') */; 若数据已排序, 考虑 /*+ use_sort_agg */".to_string(),
            ),
        ))
    }
}
