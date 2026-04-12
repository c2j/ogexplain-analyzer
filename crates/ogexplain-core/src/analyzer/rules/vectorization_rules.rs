use crate::model::{NodeType, PlanNode};

use super::super::context::{GlobalStats, PlanContext};
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::DiagnosticRule;

pub struct MixedVectorRowEngines;

impl DiagnosticRule for MixedVectorRowEngines {
    fn id(&self) -> &str {
        "VEC-001"
    }
    fn name(&self) -> &str {
        "混合向量化/行存引擎"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::Vectorization
    }
    fn check(&self, _node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        None
    }
    fn check_global(&self, plan: &crate::model::ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        let adapter_count = count_adapters(&plan.root);
        if adapter_count < 2 {
            return Vec::new();
        }
        vec![Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail: format!(
                "执行计划含 {} 个 Row/Vector Adapter 节点 — 向量引擎与行引擎之间发生切换",
                adapter_count
            ),
            node_line: None,
            node_type: None,
            suggestion: Some("统一使用同一引擎以消除适配器开销; 分析场景: SET try_vector_engine_strategy=force 尝试全向量化; 行存点查场景: SET enable_vector_engine=off 使用纯行引擎; 列存表应确保使用向量化扫描".to_string()),
        }]
    }
}

fn count_adapters(node: &PlanNode) -> usize {
    let mut count = 0;
    if node.node_type == NodeType::RowAdapter || node.node_type == NodeType::VectorAdapter {
        count += 1;
    }
    for child in &node.children {
        count += count_adapters(child);
    }
    count
}
