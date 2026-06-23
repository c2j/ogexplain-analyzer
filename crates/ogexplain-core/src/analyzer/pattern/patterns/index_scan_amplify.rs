//! ANTI-003: Index scan amplification detection.
//!
//! Detects when a Nested Loop drives many iterations through an
//! Index Scan that has both an Index Cond and a Filter — meaning
//! rows pass the index condition but are then filtered again after
//! the heap lookup, amplifying IO.

use std::collections::HashMap;

use crate::analyzer::pattern::engine::AntiPatternDef;
use crate::analyzer::pattern::types::MatchResult;
use crate::analyzer::report::{DiagnosticCategory, Severity};
use crate::analyzer::rules::utils::get_property_value;
use crate::model::{NodeType, PlanNode};

/// ANTI-003: NestedLoop drives many loops + IndexScan has Filter after Index Cond.
///
/// The Index Cond is not selective enough — rows are fetched from the heap
/// only to be discarded by an additional Filter, causing amplified IO.
pub struct IndexScanAmplify {
    threshold: f64,
}

impl Default for IndexScanAmplify {
    fn default() -> Self {
        Self {
            threshold: 10000.0_f64,
        }
    }
}

impl AntiPatternDef for IndexScanAmplify {
    fn id(&self) -> &str {
        "ANTI-003"
    }

    fn name(&self) -> &str {
        "Index Scan Amplification"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::ScanEfficiency
    }

    fn related_classic_rules(&self) -> Vec<String> {
        vec!["JOIN-001".to_string()]
    }

    fn detail_template(&self) -> String {
        "{nl} drives {nl.actual_rows} iterations; inner {idx} on {idx.relation} \
         has both Index Cond and Filter — rows pass the index but are filtered \
         again after heap lookup. Total lookups: {idx.total_work}"
            .to_string()
    }

    fn suggestion_template(&self) -> String {
        "1. Create a covering index to eliminate post-index filtering\n\
         2. Rewrite SQL to include the Filter column(s) in the index\n\
         3. If the driving table is too large, consider Hash Join instead"
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

        let actual = root.actual.as_ref()?;
        if actual.rows < self.threshold {
            return None;
        }

        // Search children for IndexScan + Filter combination
        for child in &root.children {
            let is_index_scan = matches!(
                child.node_type,
                NodeType::IndexScan
                    | NodeType::PartitionedIndexScan
                    | NodeType::IndexOnlyScan
                    | NodeType::PartitionedIndexOnlyScan
            );
            if !is_index_scan {
                continue;
            }

            // Check: has both Index Cond AND Filter → post-index filtering
            let has_index_cond = get_property_value(child, "Index Cond").is_some();
            let has_filter = get_property_value(child, "Filter").is_some();
            if !has_index_cond || !has_filter {
                continue;
            }

            // Check if rows are actually being removed by the filter
            let rows_removed = child
                .structured_props
                .as_ref()
                .and_then(|p| p.rows_removed_by_filter);
            if rows_removed.unwrap_or(0.0_f64) <= 0.0_f64 {
                continue;
            }

            let mut captures = HashMap::new();
            captures.insert("nl".to_string(), root);
            captures.insert("idx".to_string(), child);

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
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 50.0_f64,
                rows: 50000.0_f64,
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

    fn make_index_scan_with_filter() -> PlanNode {
        PlanNode {
            node_type: NodeType::IndexScan,
            relation: Some("order_items".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0_f64,
                total_cost: 10.0_f64,
                plan_rows: 1.0_f64,
                plan_width: 60,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.001_f64,
                total_time_ms: 0.05_f64,
                rows: 1.0_f64,
                loops: 50000.0_f64,
                executed: true,
            }),
            properties: vec![
                NodeProperty {
                    label: "Index Cond".to_string(),
                    value: "(order_id = orders.id)".to_string(),
                },
                NodeProperty {
                    label: "Filter".to_string(),
                    value: "(status = 'pending'::text)".to_string(),
                },
                NodeProperty {
                    label: "Rows Removed by Filter".to_string(),
                    value: "45000".to_string(),
                },
            ],
            structured_props: Some(NodeProperties {
                rows_removed_by_filter: Some(45000.0_f64),
                ..Default::default()
            }),
            buffers: None,
            children: vec![],
            indent_level: 0usize,
            line_number: 2usize,
        }
    }

    #[test]
    fn test_match_index_amplify() {
        let idx = make_index_scan_with_filter();
        let nl = PlanNode {
            node_type: NodeType::NestedLoop,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.1_f64,
                total_time_ms: 5000.0_f64,
                rows: 50000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![make_node(NodeType::SeqScan, vec![]), idx],
            indent_level: 0usize,
            line_number: 1usize,
        };

        let pattern = IndexScanAmplify::default();
        let result = pattern.try_match(&nl, &[]);
        assert!(result.is_some());
        let r = result.expect("should match");
        assert_eq!(r.pattern_id, "ANTI-003");
        assert!(r.captures.contains_key("nl"));
        assert!(r.captures.contains_key("idx"));
    }

    #[test]
    fn test_no_match_below_threshold() {
        let small_nl = PlanNode {
            node_type: NodeType::NestedLoop,
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
            line_number: 1usize,
        };

        let pattern = IndexScanAmplify::default();
        assert!(pattern.try_match(&small_nl, &[]).is_none());
    }

    #[test]
    fn test_no_match_without_index_scan() {
        let nl = make_node(
            NodeType::NestedLoop,
            vec![make_node(NodeType::SeqScan, vec![])],
        );

        let pattern = IndexScanAmplify::default();
        assert!(pattern.try_match(&nl, &[]).is_none());
    }

    #[test]
    fn test_no_match_index_scan_without_filter() {
        // Index scan with Index Cond but no Filter — should not match
        let idx = PlanNode {
            node_type: NodeType::IndexScan,
            relation: Some("t".to_string()),
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 1.0_f64,
                rows: 1.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![NodeProperty {
                label: "Index Cond".to_string(),
                value: "(id = 1)".to_string(),
            }],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0usize,
            line_number: 2usize,
        };

        let nl = PlanNode {
            node_type: NodeType::NestedLoop,
            relation: None,
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0_f64,
                total_time_ms: 100.0_f64,
                rows: 20000.0_f64,
                loops: 1.0_f64,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![make_node(NodeType::SeqScan, vec![]), idx],
            indent_level: 0usize,
            line_number: 1usize,
        };

        let pattern = IndexScanAmplify::default();
        assert!(pattern.try_match(&nl, &[]).is_none());
    }
}
