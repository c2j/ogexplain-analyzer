//! ANTI-009: HashJoin skewed build/probe detection.
//!
//! Detects when a HashJoin's build side (second child) is far larger than
//! its probe side (first child). The build side is hashed into memory and
//! should be the smaller table for optimal performance.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode};

/// ANTI-009: HashJoin build side is significantly larger than probe side.
///
/// The build side is hashed into memory — should be the smaller table.
/// Large skew increases memory pressure and reduces probe efficiency.
pub struct HashJoinSkewed {
    threshold: f64,
}

impl Default for HashJoinSkewed {
    fn default() -> Self {
        Self {
            threshold: 5.0_f64,
        }
    }
}

impl AntiPatternDef for HashJoinSkewed {
    fn id(&self) -> &str {
        "ANTI-009"
    }

    fn name(&self) -> &str {
        "HashJoin skewed build/probe"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::JoinStrategy
    }

    fn related_classic_rules(&self) -> Vec<String> {
        vec![]
    }

    fn detail_template(&self) -> String {
        "HashJoin build side ({build.actual_rows} rows) is \
         significantly larger than probe side ({probe.actual_rows} rows). \
         Build is the side hashed into memory — should be the SMALLER table."
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "Swap join order so the smaller table is the build (inner) side; \
         or use MergeJoin if both sides are pre-sorted."
            .to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        let is_hash_join = matches!(
            root.node_type,
            NodeType::HashJoin
                | NodeType::VectorHashJoin
                | NodeType::VectorSonicHashJoin
        );
        if !is_hash_join {
            return None;
        }

        // Need at least 2 children: [probe, build]
        if root.children.len() < 2 {
            return None;
        }

        let probe = &root.children[0];
        let build = &root.children[1];

        let probe_actual = probe.actual.as_ref()?;
        let build_actual = build.actual.as_ref()?;

        let probe_rows = probe_actual.rows;
        let build_rows = build_actual.rows;

        // Ratio: build_rows / max(probe_rows, 1)
        let ratio = build_rows / probe_rows.max(1.0_f64);
        if ratio <= self.threshold {
            return None;
        }

        let mut captures = HashMap::new();
        captures.insert("hj".to_string(), root);
        captures.insert("probe".to_string(), probe);
        captures.insert("build".to_string(), build);

        Some(MatchResult {
            pattern_id: self.id().to_string(),
            captures,
            ancestors: ancestors.to_vec(),
            matched_node: root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn make_node(nt: NodeType, rows: f64, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 50.0_f64,
                rows,
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

    #[test]
    fn test_match_hash_join_skewed() {
        // build 100k / probe 1k = ratio 100 > 5 → match
        let probe = make_node(NodeType::SeqScan, 1000.0_f64, vec![]);
        let build = make_node(NodeType::SeqScan, 100000.0_f64, vec![]);
        let hj = make_node(NodeType::HashJoin, 1000.0_f64, vec![probe, build]);

        let pattern = HashJoinSkewed::default();
        let result = pattern.try_match(&hj, &[]);
        assert!(result.is_some());
        let r = result.expect("should match");
        assert_eq!(r.pattern_id, "ANTI-009");
        assert!(r.captures.contains_key("hj"));
        assert!(r.captures.contains_key("probe"));
        assert!(r.captures.contains_key("build"));
    }

    #[test]
    fn test_no_match_balanced_join() {
        // build 5k / probe 5k = ratio 1 ≤ 5 → no match
        let probe = make_node(NodeType::SeqScan, 5000.0_f64, vec![]);
        let build = make_node(NodeType::SeqScan, 5000.0_f64, vec![]);
        let hj = make_node(NodeType::HashJoin, 5000.0_f64, vec![probe, build]);

        let pattern = HashJoinSkewed::default();
        assert!(pattern.try_match(&hj, &[]).is_none());
    }

    #[test]
    fn test_no_match_below_ratio_threshold() {
        // build 10k / probe 3k = ratio 3.33 ≤ 5 → no match
        let probe = make_node(NodeType::SeqScan, 3000.0_f64, vec![]);
        let build = make_node(NodeType::SeqScan, 10000.0_f64, vec![]);
        let hj = make_node(NodeType::HashJoin, 3000.0_f64, vec![probe, build]);

        let pattern = HashJoinSkewed::default();
        assert!(pattern.try_match(&hj, &[]).is_none());
    }

    #[test]
    fn test_no_match_nested_loop_instead_of_hash_join() {
        // Only HashJoin family should match
        let probe = make_node(NodeType::SeqScan, 100.0_f64, vec![]);
        let build = make_node(NodeType::SeqScan, 10000.0_f64, vec![]);
        let nl = make_node(NodeType::NestedLoop, 100.0_f64, vec![probe, build]);

        let pattern = HashJoinSkewed::default();
        assert!(pattern.try_match(&nl, &[]).is_none());
    }

    #[test]
    fn test_match_vector_hash_join() {
        let probe = make_node(NodeType::SeqScan, 500.0_f64, vec![]);
        let build = make_node(NodeType::SeqScan, 50000.0_f64, vec![]);
        let hj = make_node(NodeType::VectorHashJoin, 500.0_f64, vec![probe, build]);

        let pattern = HashJoinSkewed::default();
        let result = pattern.try_match(&hj, &[]);
        assert!(result.is_some());
    }
}
