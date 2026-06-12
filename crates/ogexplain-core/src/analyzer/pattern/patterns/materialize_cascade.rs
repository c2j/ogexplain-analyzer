//! ANTI-005: Materialize cascade detection.
//!
//! Detects the pattern Materialize → Materialize → NestedLoop,
//! indicating the optimizer has double-materialized the inner side
//! of a Nested Loop join, suggesting the inner table is being
//! repeatedly scanned or the optimizer is overly conservative.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode};

/// ANTI-005: Materialize → Materialize → NestedLoop nesting.
///
/// When the optimizer double-materializes the inner side of a Nested Loop,
/// it typically means the inner table is scanned many times or the optimizer
/// failed to choose Hash Join.
pub struct MaterializeCascade;

impl AntiPatternDef for MaterializeCascade {
    fn id(&self) -> &str {
        "ANTI-005"
    }

    fn name(&self) -> &str {
        "Multi-layer Materialization"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::MemoryUsage
    }

    fn related_classic_rules(&self) -> Vec<String> {
        vec![]
    }

    fn detail_template(&self) -> String {
        "Materialize → Materialize → {nl} triple-layer structure: \
         the optimizer has double-materialized the inner side of a Nested Loop. \
         This typically means the inner table is repeatedly scanned or the \
         optimizer is overly conservative."
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "1. Check if enable_hashjoin is disabled\n\
         2. Try forcing Hash Join: SET enable_nestloop = off;\n\
         3. Verify that the inner table has a suitable index for Index Scan"
            .to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        // Match: Materialize → Materialize → (NestedLoop | VectorNestLoop)
        if root.node_type != NodeType::Materialize && root.node_type != NodeType::VectorMaterialize
        {
            return None;
        }

        let mat2 = root.children.first()?;
        if mat2.node_type != NodeType::Materialize && mat2.node_type != NodeType::VectorMaterialize
        {
            return None;
        }

        let nl = mat2.children.first()?;
        if nl.node_type != NodeType::NestedLoop && nl.node_type != NodeType::VectorNestLoop {
            return None;
        }

        let mut captures = HashMap::new();
        captures.insert("mat1".to_string(), root);
        captures.insert("mat2".to_string(), mat2);
        captures.insert("nl".to_string(), nl);

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
    fn test_match_materialize_cascade() {
        let inner = make_node(NodeType::NestedLoop, vec![]);
        let mat2 = make_node(NodeType::Materialize, vec![inner]);
        let mat1 = make_node(NodeType::Materialize, vec![mat2]);

        let pattern = MaterializeCascade;
        let result = pattern.try_match(&mat1, &[]);
        assert!(result.is_some());
        let r = result.expect("should have match");
        assert_eq!(r.pattern_id, "ANTI-005");
        assert!(r.captures.contains_key("mat1"));
        assert!(r.captures.contains_key("mat2"));
        assert!(r.captures.contains_key("nl"));
    }

    #[test]
    fn test_no_match_single_materialize() {
        let nl = make_node(NodeType::NestedLoop, vec![]);
        let mat = make_node(NodeType::Materialize, vec![nl]);

        let pattern = MaterializeCascade;
        assert!(pattern.try_match(&mat, &[]).is_none());
    }

    #[test]
    fn test_no_match_materialize_sort_nestedloop() {
        let nl = make_node(NodeType::NestedLoop, vec![]);
        let sort = make_node(NodeType::Sort, vec![nl]);
        let mat = make_node(NodeType::Materialize, vec![sort]);

        let pattern = MaterializeCascade;
        assert!(pattern.try_match(&mat, &[]).is_none());
    }

    #[test]
    fn test_match_vector_variants() {
        let inner = make_node(NodeType::VectorNestLoop, vec![]);
        let mat2 = make_node(NodeType::VectorMaterialize, vec![inner]);
        let mat1 = make_node(NodeType::VectorMaterialize, vec![mat2]);

        let pattern = MaterializeCascade;
        let result = pattern.try_match(&mat1, &[]);
        assert!(result.is_some());
    }
}
