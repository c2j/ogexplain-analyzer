//! ANTI-008: NestedLoop with inner Sort detection.
//!
//! Detects when a NestedLoop has a Sort/VectorSort in its children with many
//! rows, producing O(n²·log n) complexity because the sort is repeated per
//! outer iteration.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode};

/// ANTI-008: NestedLoop child contains Sort with large row count.
///
/// The sort is executed once per outer iteration, leading to
/// O(n²·log n) complexity — should use HashJoin or index instead.
pub struct NestedLoopSort {
    threshold: f64,
}

impl Default for NestedLoopSort {
    fn default() -> Self {
        Self {
            threshold: 100000.0_f64,
        }
    }
}

impl AntiPatternDef for NestedLoopSort {
    fn id(&self) -> &str {
        "ANTI-008"
    }

    fn name(&self) -> &str {
        "NestedLoop + inner Sort"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SortEfficiency
    }

    fn related_classic_rules(&self) -> Vec<String> {
        vec![]
    }

    fn detail_template(&self) -> String {
        "NestedLoop inner side contains Sort ({sort.actual_rows} rows × \
         {sort.loops} loops). Sorting is repeated per outer iteration — \
         O(n²·log n)."
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "Replace NestedLoop with HashJoin or MergeJoin; add index on join \
         column to eliminate the inner Sort."
            .to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        if root.node_type != NodeType::NestedLoop && root.node_type != NodeType::VectorNestLoop {
            return None;
        }

        let nl_actual = root.actual.as_ref()?;
        if nl_actual.rows < self.threshold {
            return None;
        }

        // Search children for Sort / VectorSort with large row count
        for child in &root.children {
            let is_sort = matches!(child.node_type, NodeType::Sort | NodeType::VectorSort);
            if !is_sort {
                continue;
            }

            let sort_actual = child.actual.as_ref()?;
            if sort_actual.rows < self.threshold {
                continue;
            }

            let mut captures = HashMap::new();
            captures.insert("nl".to_string(), root);
            captures.insert("sort".to_string(), child);

            return Some(MatchResult {
                pattern_id: self.id().to_string(),
                captures,
                ancestors: ancestors.to_vec(),
                matched_node: root,
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_node(nt: NodeType, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 50.0_f64,
                rows: 500000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children,
            indent_level: 0usize,
            line_number: 1usize,
        }
    }

    fn make_sort(nt: NodeType, rows: f64, loops: f64) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 100.0_f64,
                rows,
                loops,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0usize,
            line_number: 2usize,
        }
    }

    #[test]
    fn test_match_nested_loop_plus_sort() {
        let sort = make_sort(NodeType::Sort, 200000.0_f64, 1.0_f64);
        let seq = make_node(NodeType::SeqScan, vec![]);
        let nl = make_node(NodeType::NestedLoop, vec![seq, sort]);

        let pattern = NestedLoopSort::default();
        let result = pattern.try_match(&nl, &[]);
        assert!(result.is_some());
        let r = result.expect("should match");
        assert_eq!(r.pattern_id, "ANTI-008");
        assert!(r.captures.contains_key("nl"));
        assert!(r.captures.contains_key("sort"));
    }

    #[test]
    fn test_no_match_nl_without_sort() {
        let seq1 = make_node(NodeType::SeqScan, vec![]);
        let seq2 = make_node(NodeType::SeqScan, vec![]);
        let nl = make_node(NodeType::NestedLoop, vec![seq1, seq2]);

        let pattern = NestedLoopSort::default();
        assert!(pattern.try_match(&nl, &[]).is_none());
    }

    #[test]
    fn test_no_match_sort_below_threshold() {
        let small_sort = make_sort(NodeType::Sort, 100.0_f64, 1.0_f64);
        let seq = make_node(NodeType::SeqScan, vec![]);
        let nl = make_node(NodeType::NestedLoop, vec![seq, small_sort]);

        let pattern = NestedLoopSort::default();
        assert!(pattern.try_match(&nl, &[]).is_none());
    }

    #[test]
    fn test_no_match_hash_join_with_sort() {
        let sort = make_sort(NodeType::Sort, 200000.0_f64, 1.0_f64);
        let seq = make_node(NodeType::SeqScan, vec![]);
        let hj = make_node(NodeType::HashJoin, vec![seq, sort]);

        let pattern = NestedLoopSort::default();
        assert!(pattern.try_match(&hj, &[]).is_none());
    }

    #[test]
    fn test_match_vector_variants() {
        let sort = make_sort(NodeType::VectorSort, 200000.0_f64, 1.0_f64);
        let seq = make_node(NodeType::SeqScan, vec![]);
        let nl = make_node(NodeType::VectorNestLoop, vec![seq, sort]);

        let pattern = NestedLoopSort::default();
        let result = pattern.try_match(&nl, &[]);
        assert!(result.is_some());
    }
}
