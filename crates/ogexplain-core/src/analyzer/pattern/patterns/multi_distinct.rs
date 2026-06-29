//! ANTI-010: Multi-level DISTINCT nodes detection.
//!
//! Detects when multiple Distinct/Unique nodes appear in an ancestor chain,
//! indicating redundant deduplication — the intermediate result is already
//! unique and the second pass is wasteful.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode};

/// ANTI-010: Multiple DISTINCT/Unique operations in ancestor chain.
///
/// A Unique node has another Unique or VectorUnique in its ancestor chain,
/// meaning data is deduplicated more than once — the inner operation already
/// guarantees uniqueness.
pub struct MultiDistinct;

impl AntiPatternDef for MultiDistinct {
    fn id(&self) -> &str {
        "ANTI-010"
    }

    fn name(&self) -> &str {
        "Multi-level DISTINCT nodes"
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
        "Multiple DISTINCT/Unique operations detected at lines \
         {current.line} and {parent.line}. Redundant deduplication — \
         intermediate results are already unique."
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "Remove redundant DISTINCT; push DISTINCT lower in the plan tree \
         to reduce data volume earlier."
            .to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        if root.node_type != NodeType::Unique
            && root.node_type != NodeType::VectorUnique
        {
            return None;
        }

        // Find a Unique/VectorUnique ancestor
        let parent = ancestors.iter().find(|&a| {
            a.node_type == NodeType::Unique || a.node_type == NodeType::VectorUnique
        })?;

        let mut captures = HashMap::new();
        captures.insert("current".to_string(), root);
        captures.insert("parent".to_string(), parent);

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

    fn make_node(nt: NodeType, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 50.0_f64,
                rows: 5000.0_f64,
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
    fn test_match_multi_distinct() {
        let inner_unique = make_node(NodeType::Unique, vec![]);
        let outer_unique = PlanNode {
            line_number: 2usize,
            ..make_node(NodeType::Unique, vec![inner_unique])
        };

        let ancestors = vec![&outer_unique];
        let pattern = MultiDistinct;
        let result = pattern.try_match(&outer_unique.children[0], &ancestors);
        assert!(result.is_some());
        let r = result.expect("should match");
        assert_eq!(r.pattern_id, "ANTI-010");
        assert!(r.captures.contains_key("current"));
        assert!(r.captures.contains_key("parent"));
    }

    #[test]
    fn test_no_match_single_unique() {
        let unique = make_node(NodeType::Unique, vec![]);

        let pattern = MultiDistinct;
        assert!(pattern.try_match(&unique, &[]).is_none());
    }

    #[test]
    fn test_no_match_different_node_types() {
        let seq = make_node(NodeType::SeqScan, vec![]);
        let unique = make_node(NodeType::Unique, vec![seq]);

        let ancestors = vec![] as Vec<&PlanNode>;
        let pattern = MultiDistinct;
        assert!(pattern.try_match(&unique, &ancestors).is_none());
    }

    #[test]
    fn test_match_vector_unique() {
        let inner = make_node(NodeType::VectorUnique, vec![]);
        let outer = PlanNode {
            line_number: 2usize,
            ..make_node(NodeType::VectorUnique, vec![inner])
        };

        let ancestors = vec![&outer];
        let pattern = MultiDistinct;
        let result = pattern.try_match(&outer.children[0], &ancestors);
        assert!(result.is_some());
    }

    #[test]
    fn test_no_match_unique_with_materialize_ancestor() {
        let seq = make_node(NodeType::SeqScan, vec![]);
        let mat = make_node(NodeType::Materialize, vec![seq]);
        let unique = make_node(NodeType::Unique, vec![]);

        // Manually set child and reference — avoid moving mat
        let ancestors: Vec<&PlanNode> = vec![&mat];
        let pattern = MultiDistinct;
        assert!(pattern.try_match(&unique, &ancestors).is_none());
    }
}
