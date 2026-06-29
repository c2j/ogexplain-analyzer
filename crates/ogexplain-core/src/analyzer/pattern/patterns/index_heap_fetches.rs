//! ANTI-011: IndexScan with excessive heap fetches.
//!
//! Detects when an Index Scan (or variant) has a high number of heap fetches,
//! meaning the index is not covering the query — rows pass index lookup but
//! must go to the heap for additional columns.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::analyzer::rules::utils::get_property_value;
use crate::model::{NodeType, PlanNode};

/// ANTI-011: IndexScan with more heap fetches than the threshold.
///
/// High heap fetches indicate the index is not covering — every matching row
/// requires a heap lookup for columns not in the index.
pub struct IndexHeapFetches {
    threshold: f64,
}

impl Default for IndexHeapFetches {
    fn default() -> Self {
        Self {
            threshold: 10000.0_f64,
        }
    }
}

impl AntiPatternDef for IndexHeapFetches {
    fn id(&self) -> &str {
        "ANTI-011"
    }

    fn name(&self) -> &str {
        "IndexScan + excessive heap fetches"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::ScanEfficiency
    }

    fn related_classic_rules(&self) -> Vec<String> {
        vec![]
    }

    fn detail_template(&self) -> String {
        "Index Scan at line {idx.line} triggered {heap_fetches} heap \
         fetches — index is not covering enough columns. Index lookup \
         alone is insufficient."
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "Consider a covering index (INCLUDE columns) to avoid heap \
         fetches; or use Bitmap Index Scan for better heap access pattern."
            .to_string()
    }

    fn try_match<'a>(
        &self,
        root: &'a PlanNode,
        ancestors: &[&'a PlanNode],
    ) -> Option<MatchResult<'a>> {
        let is_index_scan = matches!(
            root.node_type,
            NodeType::IndexScan
                | NodeType::PartitionedIndexScan
                | NodeType::BitmapIndexScan
                | NodeType::CStoreIndexScan
        );
        if !is_index_scan {
            return None;
        }

        // Extract heap fetches from properties (not in structured_props)
        let heap_fetches_str = get_property_value(root, "Heap Fetches")?;
        let heap_fetches: f64 = heap_fetches_str.trim().parse().ok()?;

        if heap_fetches <= self.threshold {
            return None;
        }

        // Store a flag so we can rebuild the number in the template
        let mut captures = HashMap::new();
        captures.insert("idx".to_string(), root);

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

    fn make_index_scan(nt: NodeType, heap_fetches: &str) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: Some("orders".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0_f64,
                total_cost: 100.0_f64,
                plan_rows: 10000.0_f64,
                plan_width: 60,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 500.0_f64,
                rows: 10000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![
                NodeProperty {
                    label: "Index Cond".to_string(),
                    value: "(status = 'active'::text)".to_string(),
                },
                NodeProperty {
                    label: "Heap Fetches".to_string(),
                    value: heap_fetches.to_string(),
                },
            ],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0usize,
            line_number: 2usize,
        }
    }

    #[test]
    fn test_match_index_high_heap_fetches() {
        let idx = make_index_scan(NodeType::IndexScan, "50000");

        let pattern = IndexHeapFetches::default();
        let result = pattern.try_match(&idx, &[]);
        assert!(result.is_some());
        let r = result.expect("should match");
        assert_eq!(r.pattern_id, "ANTI-011");
        assert!(r.captures.contains_key("idx"));
    }

    #[test]
    fn test_no_match_below_threshold() {
        let idx = make_index_scan(NodeType::IndexScan, "1000");

        let pattern = IndexHeapFetches::default();
        assert!(pattern.try_match(&idx, &[]).is_none());
    }

    #[test]
    fn test_no_match_without_heap_fetches_property() {
        let idx = PlanNode {
            node_type: NodeType::IndexScan,
            relation: Some("orders".to_string()),
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
            children: vec![],
            indent_level: 0usize,
            line_number: 2usize,
        };

        let pattern = IndexHeapFetches::default();
        assert!(pattern.try_match(&idx, &[]).is_none());
    }

    #[test]
    fn test_no_match_seq_scan_instead_of_index_scan() {
        let seq = PlanNode {
            node_type: NodeType::SeqScan,
            relation: Some("orders".to_string()),
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 50.0_f64,
                rows: 100000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0usize,
            line_number: 1usize,
        };

        let pattern = IndexHeapFetches::default();
        assert!(pattern.try_match(&seq, &[]).is_none());
    }

    #[test]
    fn test_match_exact_threshold() {
        // Heap fetches == threshold should NOT match (strictly above)
        let idx = make_index_scan(NodeType::IndexScan, "10000");

        let pattern = IndexHeapFetches::default();
        assert!(pattern.try_match(&idx, &[]).is_none());
    }
}
