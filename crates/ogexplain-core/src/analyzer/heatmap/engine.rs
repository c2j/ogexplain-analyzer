//! Heatmap computation engine.
//!
//! Two-phase algorithm:
//!   Phase 1 (post-order): Compute per-node deviation + subtree geometric mean Q-Error.
//!   Phase 2 (pre-order):  Compute path cumulative Q-Error from root to each node.
//!
//! All computation is read-only on `PlanNode` — no mutation required.
//! The critical path (maximum-deviation root-to-leaf path) is found greedily
//! using `path_cumulative_qerror` (not `subtree_geo_qerror`) to capture the
//! true worst-accumulation path through the plan tree.

use std::collections::HashMap;

use super::types::*;
use crate::model::{ExplainPlan, PlanNode};

/// Heatmap computation engine.
///
/// Usage:
/// ```rust,ignore
/// let heatmap = HeatmapEngine::generate(&plan);
/// ```
pub struct HeatmapEngine;

impl HeatmapEngine {
    /// Main entry point: generate a deviation heatmap from an execution plan.
    ///
    /// Returns `None` when the plan has no EXPLAIN ANALYZE statistics
    /// (no node has both estimated and actual row counts > 0).
    pub fn generate(plan: &ExplainPlan) -> Option<PlanHeatmap> {
        // Phase 1: Post-order — per-node deviation + subtree_geo_qerror
        let mut node_data: HashMap<usize, (HeatmapEntry, f64)> = HashMap::new();
        let _ = Self::post_order(&plan.root, &mut node_data);

        if node_data.is_empty() {
            return None;
        }

        // Phase 2: Pre-order — path_cumulative_qerror
        let mut entries: Vec<HeatmapEntry> = node_data.into_values().map(|(e, _)| e).collect();
        let mut line_to_idx: HashMap<usize, usize> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            line_to_idx.insert(e.deviation.line_number, i);
        }
        Self::pre_order(&plan.root, 1.0_f64, &line_to_idx, &mut entries);

        // Phase 3: Find critical path using path_cumulative_qerror (greedy)
        let critical_path = Self::find_critical_path(&plan.root, &entries, &line_to_idx);

        // Phase 4: Mark critical path nodes + sort hotspots
        let critical_set: std::collections::HashSet<usize> =
            critical_path.iter().copied().collect();
        for entry in &mut entries {
            entry.on_critical_path = critical_set.contains(&entry.deviation.line_number);
        }

        let mut hotspots: Vec<usize> = entries
            .iter()
            .filter(|e| e.deviation.severity >= DeviationSeverity::Moderate)
            .map(|e| e.deviation.line_number)
            .collect();
        hotspots.sort_by(|a, b| {
            let qa = line_to_idx
                .get(a)
                .and_then(|&i| entries.get(i))
                .map(|e| e.deviation.row_qerror)
                .unwrap_or(1.0_f64);
            let qb = line_to_idx
                .get(b)
                .and_then(|&i| entries.get(i))
                .map(|e| e.deviation.row_qerror)
                .unwrap_or(1.0_f64);
            qb.partial_cmp(&qa).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Phase 5: Summary
        let max_entry = entries.iter().max_by(|a, b| {
            a.deviation
                .row_qerror
                .partial_cmp(&b.deviation.row_qerror)
                .expect("row_qerror values are valid finite f64s")
        });
        let summary = HeatmapSummary {
            max_qerror: max_entry.map(|e| e.deviation.row_qerror).unwrap_or(1.0_f64),
            max_qerror_line: max_entry
                .map(|e| e.deviation.line_number)
                .unwrap_or(0_usize),
            severe_count: entries
                .iter()
                .filter(|e| e.deviation.severity >= DeviationSeverity::Severe)
                .count(),
            total_nodes: entries.len(),
            critical_path_length: critical_path.len(),
            deviated_count: entries
                .iter()
                .filter(|e| e.deviation.row_qerror >= 2.0_f64)
                .count(),
        };

        Some(PlanHeatmap {
            entries,
            critical_path,
            hotspots,
            summary,
        })
    }

    // ---- Phase 1: Post-order traversal ----

    /// Recursive post-order traversal.
    ///
    /// Returns `(self_qerror, subtree_geo_qerror)` for the node.
    /// Nodes without valid EXPLAIN ANALYZE statistics are skipped
    /// (not inserted into the data map).
    fn post_order(node: &PlanNode, data: &mut HashMap<usize, (HeatmapEntry, f64)>) -> (f64, f64) {
        let mut child_geo_qerrors: Vec<f64> = Vec::new();
        for child in &node.children {
            let (_, child_geo) = Self::post_order(child, data);
            child_geo_qerrors.push(child_geo);
        }

        let self_qerror = Self::qerror(node);
        let all_q: Vec<f64> = std::iter::once(self_qerror)
            .chain(child_geo_qerrors)
            .collect();
        let subtree_geo = Self::geometric_mean(&all_q);

        // Only record nodes with valid statistics
        if let Some(deviation) = Self::make_deviation(node) {
            data.insert(
                node.line_number,
                (
                    HeatmapEntry {
                        deviation,
                        subtree_geo_qerror: subtree_geo,
                        path_cumulative_qerror: 1.0_f64, // filled in Phase 2
                        on_critical_path: false,
                    },
                    subtree_geo,
                ),
            );
        }

        (self_qerror, subtree_geo)
    }

    // ---- Phase 2: Pre-order traversal ----

    /// Recursive pre-order traversal to compute path cumulative Q-Error.
    fn pre_order(
        node: &PlanNode,
        parent_cumulative: f64,
        line_to_idx: &HashMap<usize, usize>,
        entries: &mut [HeatmapEntry],
    ) {
        let self_q = Self::qerror(node);
        let cumulative = parent_cumulative * self_q;

        if let Some(&idx) = line_to_idx.get(&node.line_number) {
            entries[idx].path_cumulative_qerror = cumulative;
        }

        for child in &node.children {
            Self::pre_order(child, cumulative, line_to_idx, entries);
        }
    }

    // ---- Phase 3: Critical path ----

    /// Find the root-to-leaf path with the highest cumulative Q-Error.
    ///
    /// Uses greedy DFS: at each branch point, selects the child with the largest
    /// `path_cumulative_qerror` — this captures the true worst-accumulation path.
    fn find_critical_path(
        root: &PlanNode,
        entries: &[HeatmapEntry],
        line_to_idx: &HashMap<usize, usize>,
    ) -> Vec<usize> {
        let mut path: Vec<usize> = Vec::new();
        Self::greedy_critical(root, entries, line_to_idx, &mut path);
        path
    }

    /// Greedy DFS that builds the critical path.
    ///
    /// At each node, appends its line number, then descends into the child
    /// with the highest `path_cumulative_qerror`. This is a greedy approximation
    /// that works correctly for tree-structured execution plans (no cycles).
    fn greedy_critical(
        node: &PlanNode,
        entries: &[HeatmapEntry],
        line_to_idx: &HashMap<usize, usize>,
        path: &mut Vec<usize>,
    ) {
        path.push(node.line_number);

        if node.children.is_empty() {
            return;
        }

        // Select child with highest path_cumulative_qerror (the CRITICAL FIX)
        let best_child = node.children.iter().max_by(|a, b| {
            let qa = line_to_idx
                .get(&a.line_number)
                .and_then(|&i| entries.get(i))
                .map(|e| e.path_cumulative_qerror)
                .unwrap_or(1.0_f64);
            let qb = line_to_idx
                .get(&b.line_number)
                .and_then(|&i| entries.get(i))
                .map(|e| e.path_cumulative_qerror)
                .unwrap_or(1.0_f64);
            qa.partial_cmp(&qb).unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(child) = best_child {
            Self::greedy_critical(child, entries, line_to_idx, path);
        }
    }

    // ---- Helper methods ----

    /// Compute Q-Error for a single node.
    ///
    /// Q-Error = `max(actual_rows, estimated_rows) / min(actual_rows, estimated_rows)`.
    /// Returns 1.0 (no deviation) when statistics are missing or zero.
    fn qerror(node: &PlanNode) -> f64 {
        match (&node.estimated, &node.actual) {
            (Some(est), Some(act)) if est.plan_rows > 0.0_f64 && act.rows > 0.0_f64 => {
                let a = act.rows;
                let e = est.plan_rows;
                a.max(e) / a.min(e)
            }
            _ => 1.0_f64,
        }
    }

    /// Build a `NodeDeviation` from a plan node's statistics.
    ///
    /// Returns `None` if the node lacks estimated or actual stats,
    /// or if either row count is zero or negative.
    fn make_deviation(node: &PlanNode) -> Option<NodeDeviation> {
        let est = node.estimated.as_ref()?;
        let act = node.actual.as_ref()?;
        if est.plan_rows <= 0.0_f64 || act.rows <= 0.0_f64 {
            return None;
        }

        let a = act.rows;
        let e = est.plan_rows;
        let row_qerror = a.max(e) / a.min(e);
        let row_ratio = a / e;

        let direction = if row_ratio > 1.5_f64 {
            DeviationDirection::Underestimate
        } else if row_ratio < 0.67_f64 {
            DeviationDirection::Overestimate
        } else {
            DeviationDirection::Accurate
        };

        let severity = DeviationSeverity::from_qerror(row_qerror);

        Some(NodeDeviation {
            line_number: node.line_number,
            node_type: node.node_type.to_string(),
            relation: node.relation.clone(),
            estimated_rows: e,
            actual_rows: a,
            row_qerror,
            row_ratio,
            direction,
            severity,
        })
    }

    /// Compute the geometric mean of a slice of f64 values.
    ///
    /// Returns 1.0 for empty or non-positive inputs.
    /// The geometric mean is more robust than the arithmetic mean for
    /// deviation metrics, as it is not dominated by extreme values.
    fn geometric_mean(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 1.0_f64;
        }
        let product: f64 = values.iter().product();
        if product <= 0.0_f64 {
            return 1.0_f64;
        }
        product.powf(1.0_f64 / values.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_node_with_stats(
        line: usize,
        est_rows: f64,
        actual_rows: f64,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type: NodeType::SeqScan,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0_f64,
                total_cost: 100.0_f64,
                plan_rows: est_rows,
                plan_width: 100_i32,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 10.0_f64,
                rows: actual_rows,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children,
            indent_level: 0_usize,
            line_number: line,
        }
    }

    #[test]
    fn test_qerror_accurate() {
        let node = make_node_with_stats(1, 100.0_f64, 100.0_f64, vec![]);
        let q = HeatmapEngine::qerror(&node);
        assert!((q - 1.0_f64).abs() < 0.001_f64);
    }

    #[test]
    fn test_qerror_underestimate() {
        let node = make_node_with_stats(1, 100.0_f64, 10_000.0_f64, vec![]);
        let q = HeatmapEngine::qerror(&node);
        assert!((q - 100.0_f64).abs() < 0.001_f64);
    }

    #[test]
    fn test_qerror_overestimate() {
        let node = make_node_with_stats(1, 10_000.0_f64, 100.0_f64, vec![]);
        let q = HeatmapEngine::qerror(&node);
        assert!((q - 100.0_f64).abs() < 0.001_f64);
    }

    #[test]
    fn test_qerror_symmetry() {
        let a = make_node_with_stats(1, 10.0_f64, 1000.0_f64, vec![]);
        let b = make_node_with_stats(2, 1000.0_f64, 10.0_f64, vec![]);
        let qa = HeatmapEngine::qerror(&a);
        let qb = HeatmapEngine::qerror(&b);
        assert!((qa - qb).abs() < 0.001_f64);
    }

    #[test]
    fn test_no_stats_returns_none() {
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
        assert!(HeatmapEngine::generate(&plan).is_none());
    }

    #[test]
    fn test_critical_path_picks_worst_branch() {
        // Root (accurate) -> [Child A (10x), Child B (100x)]
        // Critical path should go through Child B (line 3)
        let child_a = make_node_with_stats(2, 100.0_f64, 1000.0_f64, vec![]);
        let child_b = make_node_with_stats(3, 100.0_f64, 10_000.0_f64, vec![]);
        let root = make_node_with_stats(1, 100.0_f64, 100.0_f64, vec![child_a, child_b]);
        let plan = ExplainPlan {
            root,
            summary: None,
        };

        let heatmap = HeatmapEngine::generate(&plan).expect("should generate heatmap");
        assert!(
            heatmap.critical_path.contains(&3),
            "Child B (100x) should be on critical path, not just Child A (10x)"
        );
    }

    #[test]
    fn test_cumulative_path_multiplication() {
        // Root(10x) -> Child(10x): cumulative = 1.0 * 10 * 10 = 100
        let child = make_node_with_stats(2, 100.0_f64, 1000.0_f64, vec![]);
        let root = make_node_with_stats(1, 100.0_f64, 1000.0_f64, vec![child]);
        let plan = ExplainPlan {
            root,
            summary: None,
        };

        let heatmap = HeatmapEngine::generate(&plan).expect("should generate heatmap");
        let leaf_entry = heatmap
            .entries
            .iter()
            .find(|e| e.deviation.line_number == 2)
            .expect("leaf entry should exist");
        assert!(
            leaf_entry.path_cumulative_qerror > 90.0_f64,
            "cumulative qerror should be ~100, got {}",
            leaf_entry.path_cumulative_qerror
        );
    }

    #[test]
    fn test_geometric_mean() {
        let values = vec![10.0_f64, 10.0_f64, 10.0_f64];
        let mean = HeatmapEngine::geometric_mean(&values);
        assert!((mean - 10.0_f64).abs() < 0.001_f64);

        // 1000, 1, 1 -> geo_mean ~= 10 (vs arithmetic mean ~= 334)
        let skewed = vec![1000.0_f64, 1.0_f64, 1.0_f64];
        let mean = HeatmapEngine::geometric_mean(&skewed);
        assert!(
            mean < 20.0_f64,
            "geometric mean should not be dominated by extreme values"
        );
    }

    #[test]
    fn test_severity_classification() {
        assert_eq!(
            DeviationSeverity::from_qerror(1.5_f64),
            DeviationSeverity::Negligible
        );
        assert_eq!(
            DeviationSeverity::from_qerror(3.0_f64),
            DeviationSeverity::Mild
        );
        assert_eq!(
            DeviationSeverity::from_qerror(7.0_f64),
            DeviationSeverity::Moderate
        );
        assert_eq!(
            DeviationSeverity::from_qerror(25.0_f64),
            DeviationSeverity::Severe
        );
        assert_eq!(
            DeviationSeverity::from_qerror(100.0_f64),
            DeviationSeverity::Extreme
        );
    }

    #[test]
    fn test_directory_classification() {
        // Underestimate: actual > estimated by >1.5x
        let d = make_node_with_stats(1, 100.0_f64, 200.0_f64, vec![]);
        let dev = HeatmapEngine::make_deviation(&d).expect("should have deviation");
        assert_eq!(dev.direction, DeviationDirection::Underestimate);

        // Overestimate: actual < estimated by <0.67x
        let d2 = make_node_with_stats(2, 100.0_f64, 50.0_f64, vec![]);
        let dev2 = HeatmapEngine::make_deviation(&d2).expect("should have deviation");
        assert_eq!(dev2.direction, DeviationDirection::Overestimate);

        // Accurate: ratio within tolerance
        let d3 = make_node_with_stats(3, 100.0_f64, 110.0_f64, vec![]);
        let dev3 = HeatmapEngine::make_deviation(&d3).expect("should have deviation");
        assert_eq!(dev3.direction, DeviationDirection::Accurate);
    }

    #[test]
    fn test_zero_stats_skipped() {
        // Zero estimated rows -> should not create deviation
        let node = PlanNode {
            node_type: NodeType::SeqScan,
            relation: Some("empty".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0_f64,
                total_cost: 0.0_f64,
                plan_rows: 0.0_f64,
                plan_width: 0_i32,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 0.0_f64,
                rows: 100.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0_usize,
            line_number: 42_usize,
        };
        assert!(HeatmapEngine::make_deviation(&node).is_none());

        // Zero actual rows -> should not create deviation
        let node2 = PlanNode {
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 0.0_f64,
                rows: 0.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            ..node
        };
        assert!(HeatmapEngine::make_deviation(&node2).is_none());
    }

    #[test]
    fn test_geometric_mean_empty() {
        let mean = HeatmapEngine::geometric_mean(&[]);
        assert!((mean - 1.0_f64).abs() < 0.001_f64);
    }

    #[test]
    fn test_hotspots_sorted_by_qerror() {
        // Create root (1x) with two children: one 5x, one 100x
        let child_a = make_node_with_stats(2, 100.0_f64, 500.0_f64, vec![]); // 5x
        let child_b = make_node_with_stats(3, 100.0_f64, 10_000.0_f64, vec![]); // 100x
        let root = make_node_with_stats(1, 100.0_f64, 100.0_f64, vec![child_a, child_b]);
        let plan = ExplainPlan {
            root,
            summary: None,
        };

        let heatmap = HeatmapEngine::generate(&plan).expect("should generate heatmap");
        // Both children have severity >= Moderate (5x, 100x)
        assert_eq!(heatmap.hotspots.len(), 2);
        // First hotspot should be the 100x node (higher qerror)
        let first_hotspot = heatmap.hotspots.first().copied().unwrap_or(0_usize);
        assert_eq!(first_hotspot, 3_usize);
    }

    #[test]
    fn test_summary_computed_correctly() {
        let child = make_node_with_stats(2, 10.0_f64, 500.0_f64, vec![]); // 50x, Extreme
        let root = make_node_with_stats(1, 100.0_f64, 200.0_f64, vec![child]); // 2x, Mild
        let plan = ExplainPlan {
            root,
            summary: None,
        };

        let heatmap = HeatmapEngine::generate(&plan).expect("should generate heatmap");
        assert_eq!(heatmap.summary.total_nodes, 2_usize);
        assert!((heatmap.summary.max_qerror - 50.0_f64).abs() < 0.001_f64);
        assert_eq!(heatmap.summary.max_qerror_line, 2_usize);
        assert_eq!(heatmap.summary.severe_count, 1_usize);
        assert_eq!(heatmap.summary.deviated_count, 2_usize); // both >= 2.0
    }
}
