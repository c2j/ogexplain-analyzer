use crate::model::{NodeType, PlanNode};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

const MAX_PARTITION_RANGE: i64 = 10;

fn parse_partition_range(selected: &str) -> Option<i64> {
    let parts: Vec<&str> = selected.split("..").collect();
    if parts.len() == 2 {
        let start: i64 = parts[0].trim().parse().ok()?;
        let end: i64 = parts[1].trim().parse().ok()?;
        return Some(end - start);
    }
    None
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

        let partition_info = match &node.structured_props {
            Some(props) => match &props.selected_partitions {
                Some(sel) => {
                    if let Some(range) = parse_partition_range(sel) {
                        if range <= MAX_PARTITION_RANGE {
                            return None;
                        }
                        format!("扫描分区范围: {}", sel)
                    } else {
                        format!("扫描分区: {}", sel)
                    }
                }
                None => "未指定分区(扫描全部分区)".to_string(),
            },
            None => "未指定分区(扫描全部分区)".to_string(),
        };

        Some(make_finding(
            self,
            format!("分区表扫描了过多分区({})", partition_info),
            node,
            Some("分区表扫描了过多分区; 确保分区键使用常量表达式过滤; 避免在分区键上使用函数(如to_date, to_char); 检查是否缺少分区键的过滤条件".to_string()),
        ))
    }
}
