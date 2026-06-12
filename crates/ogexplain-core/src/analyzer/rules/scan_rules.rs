use crate::model::{NodeType, PlanNode};

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::{get_property_value, is_scan_node};
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
        if node.node_type != NodeType::SeqScan && node.node_type != NodeType::PartitionedSeqScan {
            return None;
        }
        let actual = node.actual.as_ref()?;
        if actual.rows <= self.threshold {
            return None;
        }

        let relation = node.relation.as_deref().unwrap_or("unknown");
        let filter_cols = extract_filter_columns(node);

        let mut detail = format!(
            "Seq Scan on {} returned {} rows (threshold: {})",
            relation, actual.rows, self.threshold
        );
        if let Some(filter) = get_property_value(node, "Filter") {
            detail.push_str(&format!(", Filter: {}", filter));
        }

        let suggestion = match filter_cols {
            Some(cols) if !cols.is_empty() => {
                format!(
                    "CREATE INDEX ON {} ({}); 全扫描返回大量行, 过滤列适合建索引",
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

pub struct FilterWithoutIndex {
    estimation_ratio: f64,
}

impl FilterWithoutIndex {
    pub fn new(_config: DiagnosticConfig) -> Self {
        Self {
            estimation_ratio: 10.0,
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
        if !is_scan_node(&node.node_type) {
            return None;
        }
        let has_filter = node.properties.iter().any(|p| p.label == "Filter");
        if !has_filter {
            return None;
        }
        let estimated = node.estimated.as_ref()?;
        let actual = node.actual.as_ref()?;
        if estimated.plan_rows <= 0.0 || actual.rows <= 0.0 {
            return None;
        }
        let ratio = estimated.plan_rows / actual.rows;
        if ratio <= self.estimation_ratio {
            return None;
        }

        let relation = node.relation.as_deref().unwrap_or("unknown");
        let filter_cols = extract_filter_columns(node);
        let rows_removed = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Filter")
            .and_then(|p| p.value.trim().parse::<f64>().ok());

        let mut detail = format!(
            "Seq Scan on {} with Filter: estimated {} rows but got {} (ratio: {:.1}x)",
            relation, estimated.plan_rows, actual.rows, ratio
        );
        if let Some(removed) = rows_removed {
            detail.push_str(&format!(", Rows Removed by Filter: {}", removed));
        }

        let suggestion = match filter_cols {
            Some(cols) if !cols.is_empty() => format!(
                "过滤条件高估({:.1}x), 建议: ANALYZE {}; 同时考虑 CREATE INDEX ON {} ({})",
                ratio,
                relation,
                relation,
                cols.join(", ")
            ),
            _ => format!(
                "过滤条件高估({:.1}x), 建议: ANALYZE {}; 考虑在过滤列上创建索引",
                ratio, relation
            ),
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn extract_filter_columns(node: &PlanNode) -> Option<Vec<String>> {
    let filter = get_property_value(node, "Filter")?;
    let re = regex::Regex::new(r"(\w+)\s*=\s*(?:\d+(?:\.\d+)?|'[^']*')").ok()?;
    let cols: Vec<String> = re
        .captures_iter(filter)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();
    if cols.is_empty() {
        None
    } else {
        Some(cols)
    }
}
