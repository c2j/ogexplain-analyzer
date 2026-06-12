//! ANTI-007: Gather-then-sort detection.
//!
//! Detects when a Sort operation is performed after Streaming(GATHER),
//! meaning all data is collected on the CN before sorting — losing the
//! ability to sort in parallel across datanodes.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode, StreamingType};

/// ANTI-007: Streaming(GATHER) ancestor + Sort with large row count.
///
/// Data is gathered from all datanodes to the CN, then sorted on a single
/// node. Sorting cannot leverage DN parallelism.
pub struct GatherThenSort {
    threshold: f64,
}

impl Default for GatherThenSort {
    fn default() -> Self {
        Self {
            threshold: 100000.0_f64,
        }
    }
}

impl AntiPatternDef for GatherThenSort {
    fn id(&self) -> &str {
        "ANTI-007"
    }

    fn name(&self) -> &str {
        "CN-side Large Sort"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::DistributionIssue
    }

    fn related_classic_rules(&self) -> Vec<String> {
        vec![]
    }

    fn detail_template(&self) -> String {
        "Data gathered from DNs to CN, then sorted ({sort.actual_rows} rows). \
         All sorting happens on a single node — DN parallelism is not utilized."
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "1. If ORDER BY column matches the distribution key, use local DN sort + CN merge\n\
         2. Adjust the distribution key to match the sort column\n\
         3. If applicable, use LIMIT + subquery to reduce sort data volume"
            .to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        if root.node_type != NodeType::Sort && root.node_type != NodeType::VectorSort {
            return None;
        }

        let actual = root.actual.as_ref()?;
        if actual.rows < self.threshold {
            return None;
        }

        let gather_node = ancestors.iter().find(|&a| {
            matches!(
                &a.node_type,
                NodeType::Streaming(StreamingType::Gather)
                    | NodeType::VectorStreaming(StreamingType::Gather)
            )
        })?;

        let mut captures = HashMap::new();
        captures.insert("sort".to_string(), root);
        captures.insert("gather".to_string(), gather_node);

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

    fn make_gather() -> PlanNode {
        PlanNode {
            node_type: NodeType::Streaming(StreamingType::Gather),
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 100.0_f64,
                rows: 500000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0usize,
            line_number: 1usize,
        }
    }

    #[test]
    fn test_match_gather_then_sort() {
        let seq_scan = make_node(NodeType::SeqScan, vec![]);
        let sort = make_node(NodeType::Sort, vec![seq_scan]);
        let gather = make_gather();
        let gather = PlanNode {
            children: vec![sort],
            ..gather
        };

        let ancestors = vec![&gather];
        let pattern = GatherThenSort::default();
        let result = pattern.try_match(&gather.children[0], &ancestors);
        assert!(result.is_some());
        let r = result.expect("should match");
        assert_eq!(r.pattern_id, "ANTI-007");
        assert!(r.captures.contains_key("sort"));
        assert!(r.captures.contains_key("gather"));
    }

    #[test]
    fn test_no_match_sort_without_gather() {
        let seq_scan = make_node(NodeType::SeqScan, vec![]);
        let sort = make_node(NodeType::Sort, vec![seq_scan]);

        let pattern = GatherThenSort::default();
        assert!(pattern.try_match(&sort, &[]).is_none());
    }

    #[test]
    fn test_no_match_below_threshold() {
        let small_scan = PlanNode {
            node_type: NodeType::SeqScan,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 1.0_f64,
                rows: 100.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0usize,
            line_number: 3usize,
        };
        let small_sort = PlanNode {
            node_type: NodeType::Sort,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 1.0_f64,
                rows: 100.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![small_scan],
            indent_level: 0usize,
            line_number: 2usize,
        };
        let gather = PlanNode {
            node_type: NodeType::Streaming(StreamingType::Gather),
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 1.0_f64,
                rows: 100.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![small_sort],
            indent_level: 0usize,
            line_number: 1usize,
        };

        let ancestors = vec![&gather];
        let pattern = GatherThenSort::default();
        assert!(pattern.try_match(&gather.children[0], &ancestors).is_none());
    }

    #[test]
    fn test_no_match_gather_without_sort() {
        let scan = make_node(NodeType::SeqScan, vec![]);
        let gather = PlanNode {
            node_type: NodeType::Streaming(StreamingType::Gather),
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 100.0_f64,
                rows: 500000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![scan],
            indent_level: 0usize,
            line_number: 1usize,
        };

        let ancestors = vec![&gather];
        let pattern = GatherThenSort::default();
        assert!(pattern.try_match(&gather.children[0], &ancestors).is_none());
    }
}
