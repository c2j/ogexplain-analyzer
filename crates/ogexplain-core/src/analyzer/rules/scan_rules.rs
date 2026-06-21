use crate::model::{NodeType, PlanNode};

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::{any_property_contains, effective_scan_size, get_property_value};
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
    fn name(&self) -> &str {
        "Large table full scan"
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

        let mut detail = format!(
            "Seq Scan on {} scanned ~{:.0} rows (threshold: {:.0})",
            relation, rows_examined, self.threshold
        );
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
            Some(cols) if !cols.is_empty() => {
                format!(
                    "CREATE INDEX ON {} ({}); 全扫描大量行, 过滤列适合建索引",
                    relation,
                    cols.join(", ")
                )
            }
            _ => format!(
                "Consider creating an index on the filtered columns of {}",
                relation
            ),
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
    fn name(&self) -> &str {
        "Filter without index"
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

        let mut detail = format!(
            "Seq Scan on {} with Filter: estimated {} rows but got {} (ratio: {:.1}x)",
            relation, estimated.plan_rows, actual.rows as i64,
            if actual.rows > 0.0 { estimated.plan_rows / actual.rows } else { f64::INFINITY }
        );
        if rows_removed > 0.0 {
            detail.push_str(&format!(", Rows Removed by Filter: {}", rows_removed as i64));
        }

        let suggestion = match filter_cols {
            Some(cols) if !cols.is_empty() => format!(
                "ANALYZE {}; 同时考虑 CREATE INDEX ON {} ({})",
                relation, relation, cols.join(", ")
            ),
            _ => format!(
                "过滤条件移除大量行, 考虑在过滤列上创建索引",
            ),
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn extract_filter_columns(node: &PlanNode) -> Option<Vec<String>> {
    let filter = get_property_value(node, "Filter")?;
    // Match: col = value, col != value, col > value, col IS NULL, col BETWEEN
    // Also match col = 'value' with optional parentheses around the expression
    let re = regex::Regex::new(
        r"\(?(\w+)\s*(?:=|!=|<>|<|>|<=|>=|~~|!~~)\s*(?:'[^']*'|\d+(?:\.\d+)?)|\(?(\w+)\s+IS\s+NULL|\(?(\w+)\s+BETWEEN"
    ).ok()?;
    let cols: Vec<String> = re
        .captures_iter(filter)
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
