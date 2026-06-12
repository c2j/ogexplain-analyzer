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
                "执行计划深度为 {}（阈值: {}）; 深度过高通常表示子查询未提升或多层嵌套",
                stats.max_depth, self.max_depth
            ),
            node_line: None,
            node_type: None,
            suggestion: Some("简化查询: /*+ EXPAND_SUBQUERY */; /*+ EXPAND_SUBLINK */; /*+ LAZY_AGG */; /*+ REDUCE_ORDER_BY */; 考虑拆分为多个简单查询".to_string()),
            sql_rewrite: None,
        }]
    }
}
