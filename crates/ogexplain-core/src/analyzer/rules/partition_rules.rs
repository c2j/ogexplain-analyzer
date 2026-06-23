use crate::model::{NodeType, PlanNode};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

const MAX_PARTITION_RANGE: i64 = 10;

fn parse_partition_range(selected: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = selected.split("..").collect();
    if parts.len() == 2 {
        let start: i64 = parts[0].trim().parse().ok()?;
        let end: i64 = parts[1].trim().parse().ok()?;
        Some((start, end))
    } else {
        None
    }
}

pub struct PartitionPruningFailure;

impl DiagnosticRule for PartitionPruningFailure {
    fn id(&self) -> &str {
        "PART-001"
    }
    fn name(&self) -> &str {
        "分区剪枝失效"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::DistributionIssue
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::PartitionedSeqScan
            && node.node_type != NodeType::PartitionedCStoreScan
        {
            return None;
        }

        let has_filter = node.properties.iter().any(|p| p.label == "Filter");
        let filter_has_function = node
            .properties
            .iter()
            .find(|p| p.label == "Filter")
            .map(|p| {
                p.value.contains("date_part")
                    || p.value.contains("EXTRACT")
                    || p.value.contains("to_char")
                    || p.value.contains("to_date")
                    || p.value.contains("to_timestamp")
            })
            .unwrap_or(false);
        let sel = match &node.structured_props {
            Some(props) => match &props.selected_partitions {
                Some(s) => s.as_str(),
                None => return None,
            },
            None => return None,
        };

        let mut pruning_failed = false;
        let mut reason = String::new();

        if let Some((start, end)) = parse_partition_range(sel) {
            let n_scanned = end - start + 1;

            // Case 1: Has filter AND all partitions scanned (start=1, n_scanned >= end)
            // — pruning should have reduced this to fewer partitions
            if has_filter && start <= 1 && n_scanned >= end && filter_has_function {
                pruning_failed = true;
                reason = format!(
                    "分区表有过滤条件但扫描了全部 {} 个分区 ({}), 分区键可能被函数/表达式包装导致无法裁剪",
                    n_scanned, sel
                );
            // Case 2: Very large partition range (existing logic, keep as fallback)
            } else if n_scanned > MAX_PARTITION_RANGE {
                pruning_failed = true;
                reason = format!("扫描分区范围过大: {} ({} 个分区)", sel, n_scanned);
            }
        } else if has_filter && filter_has_function {
            // Non-range format with function-based filter
            pruning_failed = true;
            reason = format!("分区表有函数过滤条件但扫描分区: {}", sel);
        }

        if !pruning_failed {
            return None;
        }

        Some(make_finding(
            self,
            reason,
            node,
            Some("分区表扫描了过多分区; 确保分区键使用常量表达式过滤; 避免在分区键上使用函数(如 EXTRACT, to_date, to_char); 检查是否缺少分区键的过滤条件".to_string()),
        ))
    }
}
