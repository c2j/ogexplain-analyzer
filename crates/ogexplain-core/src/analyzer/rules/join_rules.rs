use crate::model::{NodeType, PlanNode};

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

pub struct NestedLoopLargeDataset {
    threshold: f64,
}

impl NestedLoopLargeDataset {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.nested_loop_inner_rows,
        }
    }
}

impl DiagnosticRule for NestedLoopLargeDataset {
    fn id(&self) -> &str {
        "JOIN-001"
    }
    fn name(&self) -> &str {
        "Nested Loop with large dataset"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::JoinStrategy
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::NestedLoop {
            return None;
        }
        let mut max_inner_work = 0.0_f64;
        let mut detail_child = String::new();
        for child in &node.children {
            if let Some(actual) = &child.actual {
                let work = actual.rows * actual.loops;
                if work > max_inner_work {
                    max_inner_work = work;
                    detail_child = format!(
                        "Inner side processed {} rows × {} loops = {} total rows",
                        actual.rows, actual.loops, work
                    );
                }
            }
        }
        if max_inner_work <= self.threshold {
            return None;
        }
        Some(make_finding(
            self,
            format!("{} (threshold: {})", detail_child, self.threshold),
            node,
            Some("SET enable_nestloop = off; or create index on join column".to_string()),
        ))
    }
}

pub struct HashSpillToDisk;

impl DiagnosticRule for HashSpillToDisk {
    fn id(&self) -> &str {
        "JOIN-002"
    }
    fn name(&self) -> &str {
        "Hash spill to disk"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::JoinStrategy
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::Hash {
            return None;
        }
        let buckets_prop = node.properties.iter().find(|p| p.label == "Buckets")?;
        let value = &buckets_prop.value;
        let batches = extract_batches(value)?;
        if batches <= 1 {
            return None;
        }
        let mem = node
            .properties
            .iter()
            .find(|p| p.label == "Memory Usage")
            .map(|p| p.value.as_str())
            .unwrap_or("unknown");
        Some(make_finding(
            self,
            format!(
                "Hash used {} batches (spilled to disk). Memory Usage: {}",
                batches, mem
            ),
            node,
            Some(format!("Increase work_mem to at least {}", mem)),
        ))
    }
}

fn extract_batches(value: &str) -> Option<i64> {
    for part in value.split("  ") {
        let part = part.trim();
        if let Some(num_str) = part.strip_prefix("Batches: ") {
            if let Ok(n) = num_str.trim().parse::<i64>() {
                return Some(n);
            }
        }
    }
    for part in value.split_whitespace() {
        if let Some(num_str) = part.strip_prefix("Batches:") {
            if let Ok(n) = num_str.trim().parse::<i64>() {
                return Some(n);
            }
        }
    }
    None
}
