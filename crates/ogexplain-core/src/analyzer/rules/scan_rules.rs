use crate::model::{NodeType, PlanNode};
use rust_i18n::t;

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::{
    any_property_contains, effective_scan_size, get_property_value, strip_cast_annotations,
};
use super::{make_finding, DiagnosticRule};

pub struct LargeTableFullScan {
    threshold: f64,
}

impl LargeTableFullScan {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.large_table_rows,
        }
    }
}

impl DiagnosticRule for LargeTableFullScan {
    fn id(&self) -> &str {
        "SCAN-001"
    }
    fn name(&self) -> String {
        t!("finding.SCAN-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::ScanEfficiency
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        // check_with_ancestors overrides this — this is kept for compatibility
        self.check_with_ancestors(node, _ctx, &[])
    }
    fn check_with_ancestors(
        &self,
        node: &PlanNode,
        _ctx: &PlanContext,
        ancestors: &[&PlanNode],
    ) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan && node.node_type != NodeType::PartitionedSeqScan {
            return None;
        }
        let has_filter = node.properties.iter().any(|p| p.label == "Filter");

        // SCAN-001 is for PURE full table scans (no filter). Filtered scans belong to SCAN-004.
        if has_filter {
            return None;
        }

        // Skip unfiltered scans feeding a HashJoin build (legitimate full table dump for hash table).
        if has_legitimate_full_scan_ancestor(ancestors) {
            return None;
        }

        let rows_examined = effective_scan_size(node);
        if rows_examined <= self.threshold {
            return None;
        }

        let relation = node.relation.as_deref().unwrap_or("unknown");
        let filter_cols = extract_filter_columns(node);

        let mut detail = t!(
            "finding.SCAN-001.detail",
            relation = relation,
            rows = rows_examined,
            threshold = self.threshold
        )
        .to_string();
        if let Some(filter) = get_property_value(node, "Filter") {
            detail.push_str(&format!(", Filter: {}", filter));
        }
        if let Some(removed) = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Filter")
        {
            detail.push_str(&format!(", Rows Removed by Filter: {}", removed.value));
        }

        let suggestion = match filter_cols {
            Some(cols) if !cols.is_empty() => t!(
                "finding.SCAN-001.suggestion_with_cols",
                relation = relation,
                cols = cols.join(", ")
            )
            .to_string(),
            _ => t!("finding.SCAN-001.suggestion_no_cols", relation = relation).to_string(),
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn has_legitimate_full_scan_ancestor(ancestors: &[&PlanNode]) -> bool {
    ancestors.iter().any(|n| {
        matches!(
            n.node_type,
            NodeType::HashJoin
                | NodeType::Hash
                | NodeType::VectorHashJoin
                | NodeType::Sort
                | NodeType::VectorSort
                | NodeType::HashAggregate
                | NodeType::GroupAggregate
                | NodeType::Unique
                | NodeType::MergeJoin
                | NodeType::Materialize
        )
    })
}

pub struct FilterWithoutIndex {
    estimation_ratio: f64,
    min_rows_removed: f64,
}

impl FilterWithoutIndex {
    pub fn new(_config: DiagnosticConfig) -> Self {
        Self {
            estimation_ratio: 10.0,
            min_rows_removed: 500.0,
        }
    }
}

impl DiagnosticRule for FilterWithoutIndex {
    fn id(&self) -> &str {
        "SCAN-004"
    }
    fn name(&self) -> String {
        t!("finding.SCAN-004.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::ScanEfficiency
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan
            && node.node_type != NodeType::PartitionedSeqScan
            && node.node_type != NodeType::CStoreScan
            && node.node_type != NodeType::BitmapHeapScan
            && node.node_type != NodeType::PartitionedBitmapHeapScan
        {
            return None;
        }
        if any_property_contains(node, "SubPlan") {
            return None;
        }
        let has_filter = node.properties.iter().any(|p| p.label == "Filter");
        if !has_filter {
            return None;
        }
        let estimated = node.estimated.as_ref()?;
        let actual = node.actual.as_ref()?;
        if estimated.plan_rows <= 0.0 {
            return None;
        }

        let rows_removed: f64 = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Filter")
            .and_then(|p| p.value.trim().parse::<f64>().ok())
            .unwrap_or(0.0);

        let should_fire = if actual.rows > 0.0 {
            let ratio = estimated.plan_rows / actual.rows;
            ratio > self.estimation_ratio || rows_removed > self.min_rows_removed
        } else {
            rows_removed > self.min_rows_removed
        };

        if !should_fire {
            return None;
        }

        let relation = node.relation.as_deref().unwrap_or("unknown");
        let filter_cols = extract_filter_columns(node);

        // EXPLAIN uses "Bitmap Heap Scan" (with space); NodeType enum name doesn't.
        let node_label = match node.node_type {
            NodeType::BitmapHeapScan => "Bitmap Heap Scan",
            NodeType::PartitionedBitmapHeapScan => "Partitioned Bitmap Heap Scan",
            NodeType::CStoreScan => "CStore Scan",
            NodeType::PartitionedSeqScan => "Partitioned Seq Scan",
            _ => "Seq Scan",
        };
        let ratio_str = if actual.rows > 0.0 {
            estimated.plan_rows / actual.rows
        } else {
            f64::INFINITY
        };
        let mut detail = t!(
            "finding.SCAN-004.detail",
            node_label = node_label,
            relation = relation,
            estimated = estimated.plan_rows,
            actual = actual.rows as i64,
            ratio = ratio_str
        )
        .to_string();
        if rows_removed > 0.0 {
            detail.push_str(&format!(
                ", Rows Removed by Filter: {}",
                rows_removed as i64
            ));
        }

        let suggestion = match filter_cols {
            Some(cols) if !cols.is_empty() => t!(
                "finding.SCAN-004.suggestion_with_cols",
                relation = relation,
                cols = cols.join(", ")
            )
            .to_string(),
            _ => t!("finding.SCAN-004.suggestion_no_cols").to_string(),
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

pub struct IndexScanPoorSelectivity {
    threshold: f64,
}

impl IndexScanPoorSelectivity {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.large_table_rows,
        }
    }
}

impl DiagnosticRule for IndexScanPoorSelectivity {
    fn id(&self) -> &str {
        "SCAN-005"
    }
    fn name(&self) -> String {
        t!("finding.SCAN-005.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::ScanEfficiency
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::IndexScan && node.node_type != NodeType::IndexOnlyScan {
            return None;
        }
        let actual = node.actual.as_ref()?;
        let total_rows = actual.rows * actual.loops;
        if total_rows <= self.threshold {
            return None;
        }
        let index_name = get_property_value(node, "Index Name").unwrap_or("unknown");
        let relation = node.relation.as_deref().unwrap_or("unknown");

        let detail = t!(
            "finding.SCAN-005.detail",
            relation = relation,
            index = index_name,
            actual_rows = actual.rows as i64,
            loops = actual.loops as i64,
            total_rows = total_rows as i64,
            threshold = self.threshold as i64
        )
        .to_string();
        let suggestion = t!(
            "finding.SCAN-005.suggestion",
            actual_rows = actual.rows as i64,
            loops = actual.loops as i64
        )
        .to_string();

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn extract_filter_columns(node: &PlanNode) -> Option<Vec<String>> {
    let filter_raw = get_property_value(node, "Filter")?;
    let filter = strip_cast_annotations(filter_raw);
    // Match: col = value, col != value, col > value, col IS NULL, col BETWEEN
    // Also match col = 'value' with optional parentheses around the expression.
    // After strip_cast_annotations, closing parens may remain between column
    // name and operator, so we allow \) *zero or more before \s*.
    let re = regex::Regex::new(
        r"\(?(\w+)\)*\s*(?:=|!=|<>|<|>|<=|>=|~~|!~~)\s*(?:'[^']*'|\d+(?:\.\d+)?)|\(?(\w+)\)*\s+IS\s+NULL|\(?(\w+)\)*\s+BETWEEN"
    ).ok()?;
    let cols: Vec<String> = re
        .captures_iter(&filter)
        .filter_map(|cap| {
            cap.get(1)
                .or_else(|| cap.get(2))
                .or_else(|| cap.get(3))
                .map(|m| m.as_str().to_string())
        })
        .collect();
    if cols.is_empty() {
        None
    } else {
        Some(cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::config::DiagnosticConfig;
    use crate::analyzer::context::{GlobalStats, PlanContext};
    use crate::model::buffer::NodeProperty;
    use crate::model::cost::{ActualStats, EstimatedCost};
    use crate::model::{ExplainPlan, NodeType, PlanNode};

    /// Helper: call FilterWithoutIndex::check with a minimal PlanContext.
    fn test_check_scan004(node: &PlanNode) -> Option<Finding> {
        let rule = FilterWithoutIndex::new(DiagnosticConfig::default());
        let dummy_root = PlanNode {
            node_type: NodeType::Result,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 0,
        };
        let plan = ExplainPlan {
            root: dummy_root,
            summary: None,
        };
        let stats = GlobalStats::compute(&plan);
        let ctx = PlanContext {
            plan: &plan,
            global_stats: &stats,
        };
        rule.check(node, &ctx)
    }

    fn make_seqscan_with_filter(
        node_type: NodeType,
        filter: &str,
        rows_removed: f64,
        actual_rows: f64,
        relation: &str,
    ) -> PlanNode {
        let total = rows_removed + actual_rows;
        PlanNode {
            node_type,
            relation: Some(relation.to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0,
                total_cost: total * 0.1,
                plan_rows: total,
                plan_width: 8,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: 100.0,
                rows: actual_rows,
                loops: 1.0,
                executed: true,
            }),
            properties: vec![
                NodeProperty {
                    label: "Filter".to_string(),
                    value: filter.to_string(),
                },
                NodeProperty {
                    label: "Rows Removed by Filter".to_string(),
                    value: rows_removed.to_string(),
                },
            ],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 1,
        }
    }

    // ── Task 3.1: Column extraction tests ─────────────────────────

    #[test]
    fn test_extract_filter_columns_strips_cast_annotations() {
        // KEY regression: was returning ["text"], should return ["facctcode"]
        let node = make_seqscan_with_filter(
            NodeType::SeqScan,
            "((facctcode)::text = '1002'::text)",
            1_222_944.0,
            44_696.0,
            "dat_zl_accountinfo",
        );
        let cols = extract_filter_columns(&node).unwrap();
        assert!(
            cols.contains(&"facctcode".to_string()),
            "should contain 'facctcode', got: {:?}",
            cols
        );
        assert!(
            !cols.contains(&"text".to_string()),
            "should NOT contain 'text' (cast type name), got: {:?}",
            cols
        );
    }

    #[test]
    fn test_scan004_suggestion_uses_real_column_name() {
        // Integration test at the rule level
        let node = make_seqscan_with_filter(
            NodeType::SeqScan,
            "((facctcode)::text = '1002'::text)",
            1_222_944.0,
            44_696.0,
            "dat_zl_accountinfo",
        );
        let finding = test_check_scan004(&node)
            .expect("SCAN-004 should fire on filter with many rows removed");
        let s = finding.suggestion.unwrap();
        assert!(
            s.contains("facctcode"),
            "suggestion must use real column 'facctcode', got: {}",
            s
        );
        assert!(!s.contains("(text)"), "must NOT contain literal '(text)'");
    }

    #[test]
    fn test_scan004_fires_on_bitmap_heap_scan_with_filter() {
        let node = make_seqscan_with_filter(
            NodeType::BitmapHeapScan,
            "(to_char(now(), 'yyyymmdd'::text) >= (inure_begin_date)::text)",
            1_184_895.0,
            1_184_900.0,
            "par_sys_securities",
        );
        let finding = test_check_scan004(&node);
        assert!(
            finding.is_some(),
            "SCAN-004 MUST fire on BitmapHeapScan with Filter"
        );
    }

    fn make_index_scan_node(
        node_type: NodeType,
        rows: f64,
        loops: f64,
        index_name: &str,
        relation: &str,
    ) -> PlanNode {
        PlanNode {
            node_type,
            relation: Some(relation.to_string()),
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: 100.0,
                rows,
                loops,
                executed: true,
            }),
            properties: vec![
                NodeProperty {
                    label: "Index Name".to_string(),
                    value: index_name.to_string(),
                },
                NodeProperty {
                    label: "Index Cond".to_string(),
                    value: "(accnt_no = '2'::bpchar)".to_string(),
                },
            ],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 1,
        }
    }

    fn test_check_scan005(node: &PlanNode) -> Option<Finding> {
        let rule = IndexScanPoorSelectivity::new(DiagnosticConfig::default());
        let dummy_root = PlanNode {
            node_type: NodeType::Result,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 0,
        };
        let plan = ExplainPlan {
            root: dummy_root,
            summary: None,
        };
        let stats = GlobalStats::compute(&plan);
        let ctx = PlanContext {
            plan: &plan,
            global_stats: &stats,
        };
        rule.check(node, &ctx)
    }

    #[test]
    fn test_scan005_index_scan_high_rows_fires() {
        let node = make_index_scan_node(NodeType::IndexScan, 50000.0, 1.0, "idx_accnt", "orders");
        let finding = test_check_scan005(&node);
        assert!(
            finding.is_some(),
            "SCAN-005 should fire on IndexScan with 50000 rows"
        );
    }

    #[test]
    fn test_scan005_index_scan_many_loops_fires() {
        let node = make_index_scan_node(NodeType::IndexScan, 100.0, 200.0, "idx_accnt", "orders");
        let finding = test_check_scan005(&node);
        assert!(
            finding.is_some(),
            "SCAN-005 should fire on IndexScan with 100 rows × 200 loops = 20000 total"
        );
    }

    #[test]
    fn test_scan005_index_only_scan_fires() {
        let node = make_index_scan_node(
            NodeType::IndexOnlyScan,
            50000.0,
            1.0,
            "idx_accnt_only",
            "orders",
        );
        let finding = test_check_scan005(&node);
        assert!(
            finding.is_some(),
            "SCAN-005 should fire on IndexOnlyScan with 50000 rows"
        );
    }

    #[test]
    fn test_scan005_below_threshold_does_not_fire() {
        let node = make_index_scan_node(NodeType::IndexScan, 100.0, 1.0, "idx_accnt", "orders");
        let finding = test_check_scan005(&node);
        assert!(
            finding.is_none(),
            "SCAN-005 should NOT fire when total rows (100) ≤ threshold (10000)"
        );
    }

    #[test]
    fn test_scan005_seqscan_does_not_fire() {
        let node = make_index_scan_node(NodeType::SeqScan, 50000.0, 1.0, "idx_accnt", "orders");
        let finding = test_check_scan005(&node);
        assert!(
            finding.is_none(),
            "SCAN-005 should NOT fire on SeqScan node"
        );
    }
}
