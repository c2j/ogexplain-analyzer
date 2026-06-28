use crate::model::{NodeType, PlanNode};
use rust_i18n::t;

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::{extract_innermost_parens, get_property_value};
use super::{make_finding, DiagnosticRule};

pub struct NestedLoopLargeDataset {
    threshold: f64,
}

impl NestedLoopLargeDataset {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.nested_loop_inner_rows,
        }
    }
}

impl DiagnosticRule for NestedLoopLargeDataset {
    fn id(&self) -> &str {
        "JOIN-001"
    }
    fn name(&self) -> String {
        t!("finding.JOIN-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::JoinStrategy
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::NestedLoop {
            return None;
        }
        let mut max_inner_work = 0.0_f64;
        let mut detail_child = String::new();
        let mut inner_has_index = false;
        let mut join_column: Option<String> = None;

        for child in &node.children {
            if let Some(actual) = &child.actual {
                let work = actual.rows * actual.loops;
                if work > max_inner_work {
                    max_inner_work = work;
                    detail_child = t!(
                        "finding.JOIN-001.detail",
                        rows = actual.rows,
                        loops = actual.loops,
                        total = work
                    )
                    .to_string();
                    inner_has_index = matches!(
                        child.node_type,
                        NodeType::IndexScan
                            | NodeType::IndexOnlyScan
                            | NodeType::BitmapHeapScan
                            | NodeType::PartitionedIndexScan
                    );
                    join_column = child
                        .properties
                        .iter()
                        .find(|p| p.label == "Index Cond")
                        .and_then(|p| {
                            let inner = extract_innermost_parens(&p.value)?;
                            let col = inner
                                .split('=')
                                .next()?
                                .trim()
                                .split('.')
                                .next_back()?
                                .trim()
                                .to_string();
                            Some(col)
                        });
                }
            }
        }
        if max_inner_work <= self.threshold {
            return None;
        }

        let mut detail = format!("{} (threshold: {})", detail_child, self.threshold);
        if inner_has_index {
            detail.push_str(&t!("finding.JOIN-001.detail_has_index"));
        }

        let suggestion = if inner_has_index {
            t!("finding.JOIN-001.suggestion_has_index").to_string()
        } else if let Some(ref col) = join_column {
            t!("finding.JOIN-001.suggestion_no_index_with_col", col = col).to_string()
        } else {
            t!("finding.JOIN-001.suggestion_no_index_no_col").to_string()
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

pub struct HashSpillToDisk;

impl DiagnosticRule for HashSpillToDisk {
    fn id(&self) -> &str {
        "JOIN-002"
    }
    fn name(&self) -> String {
        t!("finding.JOIN-002.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::JoinStrategy
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::Hash {
            return None;
        }
        let buckets_prop = node.properties.iter().find(|p| p.label == "Buckets")?;
        let value = &buckets_prop.value;
        let batches = extract_batches(value)?;
        if batches <= 1 {
            return None;
        }
        let mem_usage_str = get_property_value(node, "Memory Usage").unwrap_or("unknown");
        let disk_size = extract_disk_size_from_buckets(value);

        let mut detail = t!(
            "finding.JOIN-002.detail",
            batches = batches,
            mem_usage = mem_usage_str
        )
        .to_string();
        if let Some(ref disk) = disk_size {
            detail.push_str(&t!("finding.JOIN-002.detail_disk", disk = disk));
        }

        let suggestion = if let Some(disk_kb) = disk_size.as_ref().and_then(|s| parse_kb_value(s)) {
            let mem_kb: i64 = parse_kb_value(mem_usage_str).unwrap_or(0);
            let recommended_mb: i64 = ((disk_kb + mem_kb) / 1024 + 1).max(4);
            t!(
                "finding.JOIN-002.suggestion_recommended",
                recommended = recommended_mb,
                mem_usage = mem_usage_str
            )
            .to_string()
        } else {
            t!(
                "finding.JOIN-002.suggestion_default",
                mem_usage = mem_usage_str
            )
            .to_string()
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn extract_batches(value: &str) -> Option<i64> {
    for part in value.split("  ") {
        let part = part.trim();
        if let Some(num_str) = part.strip_prefix("Batches: ") {
            if let Ok(n) = num_str.trim().parse::<i64>() {
                return Some(n);
            }
        }
    }
    for part in value.split_whitespace() {
        if let Some(num_str) = part.strip_prefix("Batches:") {
            if let Ok(n) = num_str.trim().parse::<i64>() {
                return Some(n);
            }
        }
    }
    None
}

fn extract_disk_size_from_buckets(value: &str) -> Option<String> {
    for (i, part) in value.split_whitespace().enumerate() {
        if part == "Disk:" {
            return value.split_whitespace().nth(i + 1).map(|s| s.to_string());
        }
    }
    None
}

fn parse_kb_value(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(kb) = s.strip_suffix("kB") {
        return kb.trim().parse::<i64>().ok();
    }
    if let Some(mb) = s.strip_suffix("MB") {
        return mb.trim().parse::<f64>().ok().map(|v| (v * 1024.0) as i64);
    }
    if let Some(gb) = s.strip_suffix("GB") {
        return gb
            .trim()
            .parse::<f64>()
            .ok()
            .map(|v| (v * 1024.0 * 1024.0) as i64);
    }
    s.parse::<i64>().ok()
}

// ── JOIN-003: Expensive join filter ────────────────────────────────

pub struct HashJoinExpensiveFilter {
    threshold: f64,
}

impl HashJoinExpensiveFilter {
    pub fn new(_config: DiagnosticConfig) -> Self {
        Self { threshold: 10000.0 }
    }
}

impl DiagnosticRule for HashJoinExpensiveFilter {
    fn id(&self) -> &str {
        "JOIN-003"
    }
    fn name(&self) -> String {
        t!("finding.JOIN-003.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::JoinStrategy
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::HashJoin && node.node_type != NodeType::MergeJoin {
            return None;
        }
        let has_join_filter = node.properties.iter().any(|p| p.label == "Join Filter");
        if !has_join_filter {
            return None;
        }
        let rows_removed: f64 = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Join Filter")
            .and_then(|p| p.value.trim().parse::<f64>().ok())
            .unwrap_or(0.0);
        if rows_removed <= self.threshold {
            return None;
        }

        let join_label = match node.node_type {
            NodeType::HashJoin => "Hash Join",
            _ => "Merge Join",
        };
        let detail = t!(
            "finding.JOIN-003.detail",
            join_label = join_label,
            rows = rows_removed as i64,
            threshold = self.threshold as i64
        )
        .to_string();
        let suggestion = t!("finding.JOIN-003.suggestion", rows = rows_removed as i64).to_string();

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::config::DiagnosticConfig;
    use crate::analyzer::context::{GlobalStats, PlanContext};
    use crate::model::buffer::NodeProperty;
    use crate::model::{ExplainPlan, NodeType, PlanNode};

    fn test_check_join003(node: &PlanNode) -> Option<Finding> {
        let rule = HashJoinExpensiveFilter::new(DiagnosticConfig::default());
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

    fn make_join_node(
        node_type: NodeType,
        join_filter: Option<&str>,
        rows_removed: f64,
    ) -> PlanNode {
        let mut props = Vec::new();
        if let Some(filter) = join_filter {
            props.push(NodeProperty {
                label: "Join Filter".to_string(),
                value: filter.to_string(),
            });
        }
        if rows_removed > 0.0 {
            props.push(NodeProperty {
                label: "Rows Removed by Join Filter".to_string(),
                value: rows_removed.to_string(),
            });
        }
        PlanNode {
            node_type,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: None,
            actual: None,
            properties: props,
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 1,
        }
    }

    #[test]
    fn test_join003_hash_join_with_expensive_filter() {
        let node = make_join_node(NodeType::HashJoin, Some("(o.status = 'shipped')"), 50000.0);
        let finding = test_check_join003(&node);
        assert!(
            finding.is_some(),
            "JOIN-003 should fire on HashJoin with 50000 rows removed"
        );
        let f = finding.unwrap();
        assert!(f.detail.contains("Hash Join"));
        assert!(f.detail.contains("50000"));
        assert!(f.suggestion.unwrap().contains("50000"));
    }

    #[test]
    fn test_join003_merge_join_with_expensive_filter() {
        let node = make_join_node(NodeType::MergeJoin, Some("(o.status = 'shipped')"), 50000.0);
        let finding = test_check_join003(&node);
        assert!(
            finding.is_some(),
            "JOIN-003 should fire on MergeJoin with 50000 rows removed"
        );
        let f = finding.unwrap();
        assert!(f.detail.contains("Merge Join"));
    }

    #[test]
    fn test_join003_no_join_filter_does_not_fire() {
        let node = make_join_node(NodeType::HashJoin, None, 50000.0);
        let finding = test_check_join003(&node);
        assert!(
            finding.is_none(),
            "JOIN-003 should NOT fire without Join Filter property"
        );
    }

    #[test]
    fn test_join003_below_threshold_does_not_fire() {
        let node = make_join_node(NodeType::HashJoin, Some("(o.status = 'shipped')"), 100.0);
        let finding = test_check_join003(&node);
        assert!(
            finding.is_none(),
            "JOIN-003 should NOT fire when rows removed (100) ≤ threshold (10000)"
        );
    }

    #[test]
    fn test_join003_seqscan_does_not_fire() {
        let node = make_join_node(NodeType::SeqScan, Some("(o.status = 'shipped')"), 50000.0);
        let finding = test_check_join003(&node);
        assert!(
            finding.is_none(),
            "JOIN-003 should NOT fire on SeqScan node"
        );
    }
}
