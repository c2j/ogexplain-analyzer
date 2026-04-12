use crate::model::{NodeType, PlanNode};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

// SUBQ-001: Correlated subquery not pulled up
pub struct SubqueryNotPulledUp;

impl DiagnosticRule for SubqueryNotPulledUp {
    fn id(&self) -> &str {
        "SUBQ-001"
    }
    fn name(&self) -> &str {
        "关联子查询未提升"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        // Detect SubqueryScan nodes (subqueries that weren't pulled up into joins)
        if node.node_type == NodeType::SubqueryScan
            || node.node_type == NodeType::VectorSubqueryScan
        {
            return Some(make_finding(
                self,
                "检测到未提升的子查询(SubqueryScan)".to_string(),
                node,
                Some("检测到未提升的子查询(SubqueryScan), 可能导致临时表和性能下降; 改写为JOIN: /*+ EXPAND_SUBQUERY */; 若为关联子查询, 使用 /*+ EXPAND_SUBLINK */; 考虑使用 /*+ USE_MAGIC_SET */ 优化关联子查询".to_string()),
            ));
        }

        // Detect Result nodes with SubPlan in properties
        if node.node_type == NodeType::Result || node.node_type == NodeType::VectorResult {
            let has_subplan = node.properties.iter().any(|p| p.value.contains("SubPlan"));
            if has_subplan {
                return Some(make_finding(
                    self,
                    "检测到未提升的子查询(SubPlan)".to_string(),
                    node,
                    Some("检测到未提升的子查询(SubqueryScan/SubPlan), 可能导致临时表和性能下降; 改写为JOIN: /*+ EXPAND_SUBQUERY */; 若为关联子查询, 使用 /*+ EXPAND_SUBLINK */; 考虑使用 /*+ USE_MAGIC_SET */ 优化关联子查询".to_string()),
                ));
            }
        }

        None
    }
}

// REW-001: Large IN list not converted to JOIN
pub struct LargeInListNotConverted;

impl DiagnosticRule for LargeInListNotConverted {
    fn id(&self) -> &str {
        "REW-001"
    }
    fn name(&self) -> &str {
        "大IN列表未转换"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SubqueryStructure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        // Find Filter property with long IN(...) list
        let filter_prop = node
            .properties
            .iter()
            .find(|p| p.label == "Filter" && p.value.contains("IN ("))?;

        let comma_count = filter_prop.value.matches(',').count();
        if comma_count <= 10 {
            return None;
        }

        Some(make_finding(
            self,
            format!("过滤条件含长IN列表({}+个值)", comma_count + 1),
            node,
            Some("过滤条件含长IN列表; 使用 /*+ INLIST_TO_JOIN */ 将IN列表转换为JOIN; 或改写为临时表JOIN: INSERT INTO temp VALUES (...); SELECT * FROM t JOIN temp ON t.col = temp.col".to_string()),
        ))
    }
}
