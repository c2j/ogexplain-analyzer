use crate::model::PlanNode;

use super::super::config::DiagnosticConfig;
use super::super::context::{GlobalStats, PlanContext};
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::DiagnosticRule;

pub struct PlanTooDeep {
    max_depth: usize,
}

impl PlanTooDeep {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            max_depth: config.max_plan_depth,
        }
    }
}

impl DiagnosticRule for PlanTooDeep {
    fn id(&self) -> &str {
        "GEN-001"
    }
    fn name(&self) -> &str {
        "执行计划层级过深"
    }
    fn severity(&self) -> Severity {
        Severity::Info
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::General
    }
    fn check(&self, _node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        None
    }
    fn check_global(&self, _plan: &crate::model::ExplainPlan, stats: &GlobalStats) -> Vec<Finding> {
        if stats.max_depth <= self.max_depth {
            return Vec::new();
        }
        vec![Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail: format!(
                "执行计划深度为 {}（阈值: {}）",
                stats.max_depth, self.max_depth
            ),
            node_line: None,
            node_type: None,
            suggestion: Some(format!("计划树过深(>{}, 阈值:{}), 考虑简化查询: /*+ EXPAND_SUBQUERY */ 提升子查询减少深度; /*+ EXPAND_SUBLINK */ 提升子链接; /*+ LAZY_AGG */ 消除冗余聚合; /*+ REDUCE_ORDER_BY */ 消除冗余排序", stats.max_depth, self.max_depth)),
            sql_rewrite: None,
        }]
    }
}
