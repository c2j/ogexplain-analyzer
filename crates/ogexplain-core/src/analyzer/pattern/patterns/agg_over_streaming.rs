//! ANTI-012: Aggregate over multi-layer Streaming detection.
//!
//! Detects when an Aggregate node has multiple Streaming ancestors,
//! meaning data is redistributed multiple times before aggregation —
//! excessive network overhead.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::model::{NodeType, PlanNode};

/// ANTI-012: Aggregate with multiple Streaming redistribution ancestors.
///
/// Data is shuffled multiple times across datanodes before reaching the
/// aggregate — typically caused by GROUP BY columns that do not match
/// the distribution key.
pub struct AggOverStreaming {
    threshold: f64,
}

impl Default for AggOverStreaming {
    fn default() -> Self {
        Self { threshold: 1.0_f64 }
    }
}

impl AntiPatternDef for AggOverStreaming {
    fn id(&self) -> &str {
        "ANTI-012"
    }

    fn name(&self) -> &str {
        "Aggregate over multi-layer Streaming"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::NetworkOverhead
    }

    fn related_classic_rules(&self) -> Vec<String> {
        vec![]
    }

    fn detail_template(&self) -> String {
        "Aggregate operation at line {agg.line} has {layers} Streaming \
         redistributions above it. Data is shuffled multiple times before \
         aggregation — excessive network overhead."
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "Ensure the GROUP BY column matches the distribution key; consider \
         co-located aggregation to minimize redistribution layers."
            .to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        let is_agg = matches!(
            root.node_type,
            NodeType::Aggregate
                | NodeType::GroupAggregate
                | NodeType::HashAggregate
                | NodeType::VectorAggregate
                | NodeType::VectorHashAggregate
                | NodeType::VectorSonicHashAggregate
                | NodeType::VectorSortAggregate
        );
        if !is_agg {
            return None;
        }

        // Count all streaming ancestors (any type)
        let streaming_ancestors: Vec<&'a PlanNode> = ancestors
            .iter()
            .filter(|a| {
                matches!(
                    a.node_type,
                    NodeType::Streaming(_) | NodeType::VectorStreaming(_)
                )
            })
            .copied()
            .collect();

        let layers = streaming_ancestors.len() as f64;
        if layers <= self.threshold {
            return None;
        }

        let first_streaming = streaming_ancestors.first().copied()?;

        let mut captures = HashMap::new();
        captures.insert("agg".to_string(), root);
        captures.insert("gather".to_string(), first_streaming);

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
                rows: 10000.0_f64,
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

    fn make_streaming(stype: StreamingType, children: Vec<PlanNode>, line: usize) -> PlanNode {
        PlanNode {
            node_type: NodeType::Streaming(stype),
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 100.0_f64,
                rows: 50000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children,
            indent_level: 0usize,
            line_number: line,
        }
    }

    #[test]
    fn test_match_agg_over_two_streaming_layers() {
        // Stream(Gather) → Stream(Redistribute) → Aggregate
        let agg = make_node(NodeType::HashAggregate, vec![]);
        let inner_stream = make_streaming(StreamingType::Redistribute, vec![agg.clone()], 2);
        let outer_stream = make_streaming(StreamingType::Gather, vec![inner_stream], 1);

        let ancestors = vec![&outer_stream, &outer_stream.children[0]];
        let pattern = AggOverStreaming::default();
        let result = pattern.try_match(&agg, &ancestors);
        assert!(result.is_some());
        let r = result.expect("should match");
        assert_eq!(r.pattern_id, "ANTI-012");
        assert!(r.captures.contains_key("agg"));
        assert!(r.captures.contains_key("gather"));
    }

    #[test]
    fn test_no_match_single_streaming_layer() {
        // Stream(Gather) → Aggregate (1 layer, threshold is 1, so count must be > 1)
        let agg = make_node(NodeType::HashAggregate, vec![]);
        let stream = make_streaming(StreamingType::Gather, vec![agg.clone()], 1);

        let ancestors = vec![&stream];
        let pattern = AggOverStreaming::default();
        assert!(pattern.try_match(&agg, &ancestors).is_none());
    }

    #[test]
    fn test_no_match_without_streaming() {
        let agg = make_node(NodeType::HashAggregate, vec![]);

        let pattern = AggOverStreaming::default();
        assert!(pattern.try_match(&agg, &[]).is_none());
    }

    #[test]
    fn test_no_match_non_aggregate_node() {
        let seq = make_node(NodeType::SeqScan, vec![]);

        let pattern = AggOverStreaming::default();
        assert!(pattern.try_match(&seq, &[]).is_none());
    }

    #[test]
    fn test_match_three_streaming_layers() {
        // Three streaming layers → should fire.
        // Build tree: s1(Stream) → s2(Stream) → s3(Stream) → agg
        let agg = make_node(NodeType::VectorHashAggregate, vec![]);
        let s3 = make_streaming(StreamingType::Redistribute, vec![agg.clone()], 3);
        let s2 = make_streaming(StreamingType::Broadcast, vec![s3.clone()], 2);
        let s1 = make_streaming(StreamingType::Gather, vec![s2.clone()], 1);

        // Use references into the tree: s1 → s1.children[0] → s1.children[0].children[0]
        let ancestors = vec![&s1, &s1.children[0], &s1.children[0].children[0]];
        let pattern = AggOverStreaming::default();
        let result = pattern.try_match(&agg, &ancestors);
        assert!(result.is_some());
    }
}
