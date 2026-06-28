use crate::model::{NodeType, PlanNode, StreamingType};
use rust_i18n::t;

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::{
    any_property_contains, extract_innermost_parens, extract_target_table,
    find_first_scan_descendant, first_identifier, is_scan_node, table_name_match,
};
use super::{make_finding, make_finding_ext, DiagnosticRule};

// SUBQ-001: Correlated subquery not pulled up
pub struct SubqueryNotPulledUp;

impl DiagnosticRule for SubqueryNotPulledUp {
    fn id(&self) -> &str {
        "SUBQ-001"
    }
    fn name(&self) -> String {
        t!("finding.SUBQ-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }
    fn check_with_ancestors(
        &self,
        node: &PlanNode,
        ctx: &PlanContext,
        ancestors: &[&PlanNode],
    ) -> Option<Finding> {
        // SubqueryScan nodes: always fire (root of subquery tree)
        if node.node_type == NodeType::SubqueryScan
            || node.node_type == NodeType::VectorSubqueryScan
        {
            return self.check(node, ctx);
        }

        // SubPlan detection: fire only if NO SubqueryScan ancestor exists
        // (otherwise the SubqueryScan ancestor already covers this subtree)
        if any_property_contains(node, "SubPlan") {
            let has_subquery_scan_ancestor = ancestors.iter().any(|a| {
                a.node_type == NodeType::SubqueryScan || a.node_type == NodeType::VectorSubqueryScan
            });
            if has_subquery_scan_ancestor {
                return None; // Suppress — let the SubqueryScan-level finding represent this subtree
            }
            // Standalone SubPlan — fire normally
            return Some(make_finding(
                self,
                t!("finding.SUBQ-001.detail_subplan", nt = node.node_type).to_string(),
                node,
                Some(t!("finding.SUBQ-001.suggestion_subplan").to_string()),
            ));
        }

        None
    }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type == NodeType::SubqueryScan
            || node.node_type == NodeType::VectorSubqueryScan
        {
            let child_table_opt = find_first_scan_descendant(node).map(|r| first_identifier(&r));
            let child_table_display = child_table_opt
                .clone()
                .unwrap_or_else(|| "unknown".to_string());

            return Some(make_finding_ext(
                self,
                t!("finding.SUBQ-001.detail_subquery_scan", table = child_table_display).to_string(),
                node,
                Some(t!("finding.SUBQ-001.suggestion_subquery_scan").to_string()),
                child_table_opt,
                Vec::new(),
            ));
        }

        if any_property_contains(node, "SubPlan") {
            return Some(make_finding(
                self,
                t!("finding.SUBQ-001.detail_subplan", nt = node.node_type).to_string(),
                node,
                Some(t!("finding.SUBQ-001.suggestion_subplan").to_string()),
            ));
        }

        None
    }
}

// REW-001: Large IN list not converted to JOIN
pub struct LargeInListNotConverted {
    in_list_threshold: usize,
}

impl LargeInListNotConverted {
    pub fn new() -> Self {
        Self {
            in_list_threshold: 10,
        }
    }
}

impl DiagnosticRule for LargeInListNotConverted {
    fn id(&self) -> &str {
        "REW-001"
    }
    fn name(&self) -> String {
        t!("finding.REW-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        let filter_prop = node
            .properties
            .iter()
            .find(|p| p.label == "Filter" && p.value.contains("IN ("))?;

        let comma_count = filter_prop.value.matches(',').count();
        if comma_count <= self.in_list_threshold {
            return None;
        }

        let column =
            extract_in_list_column(&filter_prop.value).unwrap_or_else(|| "col".to_string());
        let relation = node.relation.as_deref().unwrap_or("unknown");

        let detail = t!(
            "finding.REW-001.detail",
            count = comma_count + 1,
            column = column,
            relation = relation
        )
        .to_string();
        let suggestion = t!(
            "finding.REW-001.suggestion",
            relation = relation,
            column = column
        )
        .to_string();

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn extract_in_list_column(filter_value: &str) -> Option<String> {
    let re = regex::Regex::new(r"(\w+)\s+IN\s*\(").ok()?;
    re.captures(filter_value)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
}

pub struct CorrelatedSubquerySelfUpdate;

impl DiagnosticRule for CorrelatedSubquerySelfUpdate {
    fn id(&self) -> &str {
        "SUBQ-006"
    }

    fn name(&self) -> String {
        t!("finding.SUBQ-006.name").to_string()
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }

    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        let is_dml = matches!(
            node.node_type,
            NodeType::Update | NodeType::ModifyTable | NodeType::VectorUpdate
        );
        if !is_dml {
            return None;
        }

        let target_table = extract_target_table(node)?;
        let signals = collect_signals(node, &target_table);

        if !signals.has_subplan || !signals.same_table_scan {
            return None;
        }

        let mut detail = t!(
            "finding.SUBQ-006.detail",
            table = target_table,
            subplan = signals.has_subplan,
            scan = signals.same_table_scan
        )
        .to_string();

        if signals.has_streaming {
            detail.push_str(&t!("finding.SUBQ-006.detail_streaming"));
        }

        let suggestion = build_rewrite_template(&target_table, &signals.correlation_column);

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

struct Signals {
    has_subplan: bool,
    same_table_scan: bool,
    has_streaming: bool,
    correlation_column: Option<String>,
}

fn collect_signals(node: &PlanNode, target_table: &str) -> Signals {
    let mut signals = Signals {
        has_subplan: false,
        same_table_scan: false,
        has_streaming: false,
        correlation_column: None,
    };

    check_node_recursive(node, target_table, &mut signals);
    signals
}

fn check_node_recursive(node: &PlanNode, target_table: &str, signals: &mut Signals) {
    let has_subplan_prop = node.properties.iter().any(|p| p.value.contains("SubPlan"));
    if has_subplan_prop {
        signals.has_subplan = true;
    }

    if is_scan_node(&node.node_type) {
        if let Some(ref rel) = node.relation {
            if table_name_match(rel, target_table) {
                signals.same_table_scan = true;
            }
        }
    }

    if let NodeType::Streaming(stype) = &node.node_type {
        if matches!(
            stype,
            StreamingType::Redistribute
                | StreamingType::Broadcast
                | StreamingType::SplitRedistribute
                | StreamingType::SplitBroadcast
                | StreamingType::PartRedistributePartBroadcast
        ) {
            signals.has_streaming = true;
        }
    }
    if signals.correlation_column.is_none() {
        if let Some(col) = extract_correlation_column(node) {
            signals.correlation_column = Some(col);
        }
    }

    for child in &node.children {
        check_node_recursive(child, target_table, signals);
    }
}

fn extract_correlation_column(node: &PlanNode) -> Option<String> {
    let value = node
        .properties
        .iter()
        .find(|p| p.label == "Index Cond")?
        .value
        .as_str();
    let paren_content = extract_innermost_parens(value)?;
    let parts: Vec<&str> = paren_content.splitn(2, '=').collect();
    if parts.len() == 2 {
        let col_part = parts[0].trim();
        let col = col_part
            .split('.')
            .next_back()
            .unwrap_or(col_part)
            .trim()
            .to_string();
        if !col.is_empty() {
            return Some(col);
        }
    }
    None
}

fn build_rewrite_template(table: &str, correlation_col: &Option<String>) -> String {
    let col = correlation_col.as_deref().unwrap_or("<correlation_column>");
    t!("finding.SUBQ-006.suggestion", table = table, col = col).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::context::{GlobalStats, PlanContext};
    use crate::model::ExplainPlan;

    fn make_node(nt: NodeType, line: usize) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: line,
        }
    }

    fn test_check_subq001(node: &PlanNode) -> Option<Finding> {
        let rule = SubqueryNotPulledUp;
        let dummy_root = make_node(NodeType::Result, 0);
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

    // ── Task 4.1: Recursive table name lookup ──────────────────────

    #[test]
    fn test_subq001_finds_table_name_in_nested_subquery() {
        // SubqueryScan → HashJoin → SeqScan(table="orders")
        // Currently returns "unknown" (direct child is HashJoin, no relation)
        let mut subquery_scan = make_node(NodeType::SubqueryScan, 1);
        let mut join = make_node(NodeType::HashJoin, 2);
        let mut scan = make_node(NodeType::SeqScan, 3);
        scan.relation = Some("orders".to_string());
        join.children.push(scan);
        subquery_scan.children.push(join);

        let finding = test_check_subq001(&subquery_scan).expect("Should fire on SubqueryScan");
        assert!(
            finding.detail.contains("orders"),
            "detail must mention real table 'orders', got: {}",
            finding.detail
        );
    }

    #[test]
    fn test_subq001_unknown_when_no_scan_in_subtree() {
        // SubqueryScan → Sort → Result (no scan)
        // Should still fire but with "unknown"
        let mut subquery_scan = make_node(NodeType::SubqueryScan, 1);
        let mut sort = make_node(NodeType::Sort, 2);
        let result = make_node(NodeType::Result, 3);
        sort.children.push(result);
        subquery_scan.children.push(sort);

        let finding = test_check_subq001(&subquery_scan).expect("Should fire");
        assert!(
            finding.detail.contains("unknown"),
            "should mention 'unknown' when no scan found, got: {}",
            finding.detail
        );
    }

    // ── Task 4.2: Aggregate SubPlan into SubqueryScan tree ─────────

    #[test]
    fn test_subq001_aggregates_subplan_into_subquery_scan() {
        // Tree: SubqueryScan → BitmapHeapScan (has SubPlan) → BitmapIndexScan (has SubPlan)
        // WITHOUT aggregation: 3 findings (SubqueryScan + 2 SubPlan nodes)
        // WITH aggregation: 1 finding (only SubqueryScan level)
        let mut subquery_scan = make_node(NodeType::SubqueryScan, 1);
        let mut bitmap_heap = make_node(NodeType::BitmapHeapScan, 2);
        bitmap_heap
            .properties
            .push(crate::model::buffer::NodeProperty {
                label: "Filter".to_string(),
                value: "SubPlan 1".to_string(),
            });
        let mut bitmap_idx = make_node(NodeType::BitmapIndexScan, 3);
        bitmap_idx
            .properties
            .push(crate::model::buffer::NodeProperty {
                label: "Index Cond".to_string(),
                value: "(col = (SubPlan 2))".to_string(),
            });
        bitmap_heap.children.push(bitmap_idx);
        subquery_scan.children.push(bitmap_heap);

        let plan = ExplainPlan {
            root: subquery_scan,
            summary: None,
        };
        let report = crate::analyze(&plan);
        let subq_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "SUBQ-001")
            .collect();
        assert_eq!(
            subq_findings.len(),
            1,
            "expected 1 aggregated finding (SubqueryScan only), got {}: {:#?}",
            subq_findings.len(),
            subq_findings
        );
    }

    #[test]
    fn test_subq001_fires_on_standalone_subplan() {
        // Standalone SubPlan with NO SubqueryScan ancestor
        // Should still fire (not suppressed)
        let mut result = make_node(NodeType::Result, 1);
        result.properties.push(crate::model::buffer::NodeProperty {
            label: "Filter".to_string(),
            value: "(col = (SubPlan 1))".to_string(),
        });

        let plan = ExplainPlan {
            root: result,
            summary: None,
        };
        let report = crate::analyze(&plan);
        let subq_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "SUBQ-001")
            .collect();
        assert!(
            !subq_findings.is_empty(),
            "standalone SubPlan (no SubqueryScan ancestor) must still fire"
        );
    }
}
