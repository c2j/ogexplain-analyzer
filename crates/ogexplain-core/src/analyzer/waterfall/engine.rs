//! Resource waterfall computation engine.
//!
//! Algorithm: DFS post-order traversal (bottom-up reduction).
//! - Phase 1: Post-order — compute per-node metrics + subtree reduction
//! - Phase 2: Compute percentages relative to plan totals
//! - Phase 3: Identify bottlenecks
//!
//! All computation is read-only on PlanNode — no mutation required.

use super::profile::ResourceProfile;
use super::types::*;
use crate::model::{ExplainPlan, PlanNode};

/// CPU percentage threshold for bottleneck detection.
const CPU_BOTTLENECK_THRESHOLD: f64 = 0.20_f64; // 20%
/// Memory percentage threshold for bottleneck detection.
const MEMORY_BOTTLENECK_THRESHOLD: f64 = 0.25_f64; // 25%

/// Resource waterfall computation engine.
///
/// Generates a [`PlanWaterfall`] from an [`ExplainPlan`] by performing
/// a DFS post-order traversal to extract per-node resource consumption
/// metrics and identify bottleneck nodes.
pub struct WaterfallEngine;

impl WaterfallEngine {
    /// Main entry point: generate a resource waterfall from an execution plan.
    ///
    /// Returns `None` when the plan has no EXPLAIN ANALYZE data
    /// (no node has actual execution statistics).
    pub fn generate(plan: &ExplainPlan) -> Option<PlanWaterfall> {
        // Phase 1: DFS post-order — collect all node metrics
        let mut entries: Vec<(WaterfallEntry, usize)> = Vec::new();
        let (total_cpu, max_mem) = Self::post_order(&plan.root, 0_usize, &mut entries);

        if entries.is_empty() {
            return None;
        }

        // If no entry has any stats at all, this is a pure EXPLAIN (no ANALYZE)
        let has_any_stats = entries
            .iter()
            .any(|(e, _)| e.metrics.cpu_time_ms.is_some() || e.metrics.peak_memory_kb.is_some());
        if !has_any_stats {
            return None;
        }

        // Phase 2: Compute percentages
        let nodes_with_stats = entries
            .iter()
            .filter(|(e, _)| e.metrics.cpu_time_ms.is_some() || e.metrics.peak_memory_kb.is_some())
            .count();

        for (entry, _) in &mut entries {
            if total_cpu > 0.0_f64 {
                let cpu = entry.metrics.cpu_time_ms.unwrap_or(0.0_f64);
                entry.cpu_percent = cpu / total_cpu * 100.0_f64;
            }
            if max_mem > 0.0_f64 {
                let mem = entry.metrics.peak_memory_kb.unwrap_or(0.0_f64);
                entry.memory_percent = mem / max_mem * 100.0_f64;
            }
        }

        // Phase 3: Mark bottlenecks
        for (entry, _) in &mut entries {
            let mut bottleneck_dims = Vec::new();
            if entry.cpu_percent >= CPU_BOTTLENECK_THRESHOLD * 100.0_f64 {
                bottleneck_dims.push(ResourceDimension::CpuTime);
            }
            if entry.memory_percent >= MEMORY_BOTTLENECK_THRESHOLD * 100.0_f64 {
                bottleneck_dims.push(ResourceDimension::Memory);
            }
            // Spill nodes are also Memory bottlenecks
            if entry.metrics.has_memory_spill
                && !bottleneck_dims.contains(&ResourceDimension::Memory)
            {
                bottleneck_dims.push(ResourceDimension::Memory);
            }
            entry.is_bottleneck = !bottleneck_dims.is_empty();
            entry.bottleneck_dimensions = bottleneck_dims;
        }

        // Phase 4: Bottleneck summary
        let mut cpu_sorted: Vec<_> = entries
            .iter()
            .filter(|(e, _)| e.metrics.cpu_time_ms.is_some())
            .collect();
        cpu_sorted.sort_by(|a, b| {
            b.0.metrics
                .cpu_time_ms
                .unwrap_or(0.0_f64)
                .partial_cmp(&a.0.metrics.cpu_time_ms.unwrap_or(0.0_f64))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut mem_sorted: Vec<_> = entries
            .iter()
            .filter(|(e, _)| e.metrics.peak_memory_kb.is_some())
            .collect();
        mem_sorted.sort_by(|a, b| {
            b.0.metrics
                .peak_memory_kb
                .unwrap_or(0.0_f64)
                .partial_cmp(&a.0.metrics.peak_memory_kb.unwrap_or(0.0_f64))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let spill_count = entries
            .iter()
            .filter(|(e, _)| e.metrics.has_memory_spill)
            .count();

        let bottlenecks = BottleneckSummary {
            cpu_bottlenecks: cpu_sorted
                .iter()
                .take(5_usize)
                .map(|(e, _)| e.metrics.line_number)
                .collect(),
            memory_bottlenecks: mem_sorted
                .iter()
                .take(5_usize)
                .map(|(e, _)| e.metrics.line_number)
                .collect(),
            total_cpu_time_ms: total_cpu,
            max_peak_memory_kb: max_mem,
            spill_node_count: spill_count,
        };

        let final_entries: Vec<WaterfallEntry> = entries.into_iter().map(|(e, _)| e).collect();
        let total_nodes = final_entries.len();

        Some(PlanWaterfall {
            entries: final_entries,
            bottlenecks,
            total_nodes,
            nodes_with_stats,
        })
    }

    // ---- Phase 1: DFS post-order traversal ----

    /// Recursive post-order traversal.
    ///
    /// Returns `(subtree_total_cpu, subtree_max_memory)` for the node.
    fn post_order(
        node: &PlanNode,
        depth: usize,
        entries: &mut Vec<(WaterfallEntry, usize)>,
    ) -> (f64, f64) {
        let mut child_total_cpu = 0.0_f64;
        let mut child_max_mem = 0.0_f64;

        for child in &node.children {
            let (c_cpu, c_mem) = Self::post_order(child, depth + 1_usize, entries);
            child_total_cpu += c_cpu;
            child_max_mem = child_max_mem.max(c_mem);
        }

        // Extract current node resource metrics
        let cpu_time_ms = Self::extract_cpu_time(node);
        let peak_memory_kb = Self::extract_peak_memory(node);
        let sort_spill_kb = Self::extract_sort_spill(node);
        let hash_spill_batches = Self::extract_hash_batches(node);
        let hash_memory_usage = Self::extract_hash_memory(node);

        let has_memory_spill =
            sort_spill_kb.is_some() || hash_spill_batches.is_some_and(|b| b > 1_i64);

        let self_cpu = cpu_time_ms.unwrap_or(0.0_f64);
        let self_mem = peak_memory_kb.unwrap_or(0.0_f64);

        let subtree_cpu = child_total_cpu + self_cpu;
        let subtree_mem = child_max_mem.max(self_mem);

        // Get resource profile
        let profile = ResourceProfile::for_node(&node.node_type);

        let metrics = NodeResourceMetrics {
            line_number: node.line_number,
            node_type: node.node_type.to_string(),
            relation: node.relation.clone(),
            cpu_time_ms,
            peak_memory_kb,
            sort_spill_kb,
            hash_spill_batches,
            hash_memory_usage,
            has_memory_spill,
            subtree_cpu_time_ms: subtree_cpu,
            subtree_peak_memory_kb: subtree_mem,
        };

        let entry = WaterfallEntry {
            metrics,
            dimensions: profile.primary_dimensions.to_vec(),
            cpu_percent: 0.0_f64,          // Phase 2 fills
            memory_percent: 0.0_f64,       // Phase 2 fills
            is_bottleneck: false,          // Phase 3 fills
            bottleneck_dimensions: vec![], // Phase 3 fills
            depth,
        };

        entries.push((entry, depth));

        (subtree_cpu, subtree_mem)
    }

    // ---- Metric extraction helpers ----

    /// Extract CPU time = actual.total_time_ms × loops.
    /// Returns None when no EXPLAIN ANALYZE data is available
    /// or the node was not executed.
    fn extract_cpu_time(node: &PlanNode) -> Option<f64> {
        let actual = node.actual.as_ref()?;
        if !actual.executed {
            return None;
        }
        Some(actual.total_time_ms * actual.loops)
    }

    /// Extract peak memory (KB) from structured_props.
    fn extract_peak_memory(node: &PlanNode) -> Option<f64> {
        node.structured_props.as_ref()?.peak_memory_kb
    }

    /// Extract sort spill size from structured_props.sort_disk.
    /// Parses strings like "5840kB" → 5840.0.
    fn extract_sort_spill(node: &PlanNode) -> Option<f64> {
        let sp = node.structured_props.as_ref()?;
        let disk_str = sp.sort_disk.as_ref()?;
        let num_str = disk_str.trim_end_matches("kB").trim();
        num_str.parse().ok()
    }

    /// Extract hash spill batches from structured_props.
    /// Batches > 1 indicates spill to disk.
    fn extract_hash_batches(node: &PlanNode) -> Option<i64> {
        node.structured_props.as_ref()?.hash_batches
    }

    /// Extract hash memory usage (raw string, e.g. "28kB").
    fn extract_hash_memory(node: &PlanNode) -> Option<String> {
        node.structured_props.as_ref()?.hash_memory_usage.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_node(
        line: usize,
        node_type: NodeType,
        cpu_time_ms: Option<f64>,
        peak_mem_kb: Option<f64>,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0_f64,
                total_cost: 100.0_f64,
                plan_rows: 1000.0_f64,
                plan_width: 32_i32,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: cpu_time_ms.map(|t| ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: t,
                rows: 1000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: peak_mem_kb.map(|m| NodeProperties {
                peak_memory_kb: Some(m),
                ..Default::default()
            }),
            buffers: None,
            children,
            indent_level: 0_usize,
            line_number: line,
        }
    }

    fn make_node_full(
        line: usize,
        node_type: NodeType,
        cpu_time_ms: f64,
        peak_mem_kb: f64,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0_f64,
                total_cost: 100.0_f64,
                plan_rows: 1000.0_f64,
                plan_width: 32_i32,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: cpu_time_ms,
                rows: 1000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: Some(NodeProperties {
                peak_memory_kb: Some(peak_mem_kb),
                ..Default::default()
            }),
            buffers: None,
            children,
            indent_level: 0_usize,
            line_number: line,
        }
    }

    #[test]
    fn test_cpu_time_extraction() {
        let node = make_node_full(1_usize, NodeType::SeqScan, 50.0_f64, 0.0_f64, vec![]);
        assert_eq!(WaterfallEngine::extract_cpu_time(&node), Some(50.0_f64));
    }

    #[test]
    fn test_cpu_time_with_loops() {
        let mut node = make_node(1_usize, NodeType::SeqScan, Some(10.0_f64), None, vec![]);
        if let Some(ref mut actual) = node.actual {
            actual.loops = 5.0_f64;
        }
        // cpu_time = 10.0 × 5 = 50.0
        assert_eq!(WaterfallEngine::extract_cpu_time(&node), Some(50.0_f64));
    }

    #[test]
    fn test_peak_memory_extraction() {
        let node = make_node_full(1_usize, NodeType::HashJoin, 10.0_f64, 8192.0_f64, vec![]);
        assert_eq!(
            WaterfallEngine::extract_peak_memory(&node),
            Some(8192.0_f64)
        );
    }

    #[test]
    fn test_no_analyze_returns_none() {
        let node = PlanNode {
            node_type: NodeType::SeqScan,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0_usize,
            line_number: 1_usize,
        };
        let plan = ExplainPlan {
            root: node,
            summary: None,
        };
        assert!(WaterfallEngine::generate(&plan).is_none());
    }

    #[test]
    fn test_subtree_cpu_reduction() {
        // Root(10ms) → [Child A(30ms), Child B(20ms)]
        // subtree_cpu for Root = 10 + 30 + 20 = 60
        let child_a = make_node_full(2_usize, NodeType::SeqScan, 30.0_f64, 0.0_f64, vec![]);
        let child_b = make_node_full(3_usize, NodeType::SeqScan, 20.0_f64, 0.0_f64, vec![]);
        let root = make_node_full(
            1_usize,
            NodeType::HashJoin,
            10.0_f64,
            0.0_f64,
            vec![child_a, child_b],
        );
        let plan = ExplainPlan {
            root,
            summary: None,
        };

        let waterfall = WaterfallEngine::generate(&plan).expect("should generate waterfall");
        assert_eq!(
            waterfall.bottlenecks.total_cpu_time_ms, 60.0_f64,
            "total CPU should be 60 (10 + 30 + 20)"
        );
    }

    #[test]
    fn test_bottleneck_detection() {
        // Root(10ms) → [BigChild(80ms), SmallChild(5ms)]
        // BigChild is 80/95 ≈ 84% → bottleneck
        let big = make_node_full(2_usize, NodeType::SeqScan, 80.0_f64, 0.0_f64, vec![]);
        let small = make_node_full(3_usize, NodeType::IndexScan, 5.0_f64, 0.0_f64, vec![]);
        let root = make_node_full(
            1_usize,
            NodeType::HashJoin,
            10.0_f64,
            0.0_f64,
            vec![big, small],
        );
        let plan = ExplainPlan {
            root,
            summary: None,
        };

        let waterfall = WaterfallEngine::generate(&plan).expect("should generate waterfall");
        let big_entry = waterfall
            .entries
            .iter()
            .find(|e| e.metrics.line_number == 2_usize)
            .expect("big child entry should exist");
        assert!(big_entry.is_bottleneck);
        assert!(big_entry
            .bottleneck_dimensions
            .contains(&ResourceDimension::CpuTime));
    }

    #[test]
    fn test_spill_detection() {
        let mut node = make_node_full(1_usize, NodeType::Sort, 10.0_f64, 8192.0_f64, vec![]);
        // Add sort spill
        if let Some(ref mut sp) = node.structured_props {
            sp.sort_method = Some("external merge".to_string());
            sp.sort_disk = Some("5840kB".to_string());
        }

        let spill_kb = WaterfallEngine::extract_sort_spill(&node);
        assert_eq!(spill_kb, Some(5840.0_f64));
    }

    #[test]
    fn test_profile_for_streaming() {
        use crate::model::StreamingType;
        let st = NodeType::Streaming(StreamingType::Gather);
        let profile = ResourceProfile::for_node(&st);
        assert!(profile.description.contains("network"));
    }

    #[test]
    fn test_profile_for_all_scans() {
        let scans = [
            NodeType::SeqScan,
            NodeType::IndexScan,
            NodeType::CStoreScan,
            NodeType::BitmapIndexScan,
            NodeType::ForeignScan,
        ];
        for nt in &scans {
            let profile = ResourceProfile::for_node(nt);
            assert!(
                !profile.primary_dimensions.is_empty(),
                "No profile for {:?}",
                nt
            );
        }
    }
}
