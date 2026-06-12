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
        let mut adapters: Vec<AdapterSignal> = Vec::new();
        collect_adapter_signals(&plan.root, None, &mut adapters);

        if adapters.len() < 2 {
            return Vec::new();
        }

        let switch_points: Vec<String> = adapters
            .iter()
            .map(|a| {
                format!(
                    "{} (line {:?}): {}",
                    a.adapter_type, a.line_number, a.direction
                )
            })
            .collect();

        let detail = format!(
            "执行计划含 {} 处引擎切换 (需要 {} 次适配器转换): {}",
            adapters.len(),
            adapters.len(),
            switch_points.join("; ")
        );

        let suggestion = "统一使用同一引擎以消除适配器开销; SET try_vector_engine_strategy=force 尝试全向量化; 行存点查: SET enable_vector_engine=off".to_string();

        vec![Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail,
            node_line: None,
            node_type: None,
            suggestion: Some(suggestion),
            sql_rewrite: None,
        }]
    }
}

struct AdapterSignal {
    adapter_type: String,
    line_number: Option<usize>,
    direction: String,
}

fn collect_adapter_signals(
    node: &PlanNode,
    parent_type: Option<&NodeType>,
    adapters: &mut Vec<AdapterSignal>,
) {
    let is_adapter =
        node.node_type == NodeType::RowAdapter || node.node_type == NodeType::VectorAdapter;

    if is_adapter {
        let direction = match node.node_type {
            NodeType::RowAdapter => "Row→Vector".to_string(),
            NodeType::VectorAdapter => "Vector→Row".to_string(),
            _ => "Unknown".to_string(),
        };

        let child_type = node.children.first().map(|c| c.node_type.to_string());

        adapters.push(AdapterSignal {
            adapter_type: node.node_type.to_string(),
            line_number: Some(node.line_number),
            direction: format!(
                "{} [{} → {}]",
                direction,
                parent_type
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                child_type.unwrap_or_else(|| "?".to_string())
            ),
        });
    }

    for child in &node.children {
        collect_adapter_signals(child, Some(&node.node_type), adapters);
    }
}
