use serde::Serialize;

use super::buffer::BufferStats;
use super::buffer::NodeProperty;
use super::cost::{ActualStats, EstimatedCost};
use super::join_type::JoinType;
use super::node_type::NodeType;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct NodeProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_removed_by_filter: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_disk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_buckets: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_batches: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_memory_usage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_partitions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterations: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_memory_kb: Option<f64>,
}

impl NodeProperties {
    pub fn extract(properties: &[NodeProperty]) -> Option<Self> {
        let mut props = Self::default();
        let mut any_found = false;

        for p in properties {
            match p.label.as_str() {
                "Rows Removed by Filter" => {
                    props.rows_removed_by_filter = p.value.trim().parse().ok();
                    any_found = true;
                }
                "Sort Method" => {
                    if let Some(disk_pos) = p.value.find("Disk:") {
                        props.sort_method = Some(p.value[..disk_pos].trim().to_string());
                        let disk_part = &p.value[disk_pos + 5..];
                        props.sort_disk = Some(disk_part.trim().to_string());
                    } else if let Some(mem_pos) = p.value.find("Memory:") {
                        props.sort_method = Some(p.value[..mem_pos].trim().to_string());
                    } else {
                        props.sort_method = Some(p.value.clone());
                    }
                    any_found = true;
                }
                "Buckets" => {
                    for part in p.value.split("  ") {
                        let part = part.trim();
                        if let Some(v) = part.strip_prefix("Batches: ") {
                            props.hash_batches = v.trim().parse().ok();
                        } else if let Some(v) = part.strip_prefix("Memory Usage: ") {
                            props.hash_memory_usage = Some(v.to_string());
                        } else {
                            props.hash_buckets = part.parse().ok();
                        }
                    }
                    any_found = true;
                }
                "Selected Partitions" => {
                    props.selected_partitions = Some(p.value.clone());
                    any_found = true;
                }
                "Iterations" => {
                    props.iterations = p.value.trim().parse().ok();
                    any_found = true;
                }
                "Peak Memory" => {
                    let num_part = p.value.trim().trim_end_matches("kB");
                    props.peak_memory_kb = num_part.parse().ok();
                    any_found = true;
                }
                _ => {}
            }
        }

        if any_found {
            Some(props)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanNode {
    pub node_type: NodeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_type: Option<JoinType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated: Option<EstimatedCost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<ActualStats>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<NodeProperty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_props: Option<NodeProperties>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffers: Option<BufferStats>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<PlanNode>,
    #[serde(skip_serializing_if = "is_zero_usize")]
    pub indent_level: usize,
    pub line_number: usize,
}

fn is_zero_usize(v: &usize) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExplainPlan {
    pub root: PlanNode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<PlanSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct PlanSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_runtime_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_memory_kb: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_runtime_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor_start_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor_run_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor_end_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_network_kb: Option<i64>,
}
