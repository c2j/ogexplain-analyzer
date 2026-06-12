use crate::model::{NodeType, PlanNode, StreamingType};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::any_property_contains;
use super::{make_finding, DiagnosticRule};

pub struct QueryNotPushedDown;

impl DiagnosticRule for QueryNotPushedDown {
    fn id(&self) -> &str {
        "PUSH-001"
    }
    fn name(&self) -> &str {
        "查询未下推"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::PushdownFailure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        let streaming_type = match &node.node_type {
            NodeType::Streaming(StreamingType::Redistribute) => "Redistribute",
            NodeType::Streaming(StreamingType::Broadcast) => "Broadcast",
            _ => return None,
        };

        let reasons = collect_pushdown_blockers(node);

        let mut detail = format!("查询未完全下推 — 发现 Streaming({}) 节点", streaming_type);
        if !reasons.is_empty() {
            detail.push_str(&format!(", 可能原因: {}", reasons.join(", ")));
        }

        let suggestion = if reasons.iter().any(|r| r.contains("子查询")) {
            "使用 /*+ EXPAND_SUBLINK */ 提升子链接; /*+ EXPAND_SUBQUERY */ 提升子查询".to_string()
        } else if reasons.iter().any(|r| r.contains("易变函数")) {
            "查询含易变函数, 不可下推; 考虑改写为可下推形式或使用 PL/pgSQL".to_string()
        } else {
            "检查不可下推构造; 使用 hint: EXPAND_SUBLINK/EXPAND_SUBQUERY; SET rewrite_rule=partialpush"
                .to_string()
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn collect_pushdown_blockers(node: &PlanNode) -> Vec<String> {
    let mut blockers = Vec::new();
    collect_blockers_recursive(node, &mut blockers);
    blockers
}

fn collect_blockers_recursive(node: &PlanNode, blockers: &mut Vec<String>) {
    if matches!(
        node.node_type,
        NodeType::SubqueryScan | NodeType::VectorSubqueryScan
    ) {
        blockers.push("子查询未提升".to_string());
    }
    if matches!(node.node_type, NodeType::Result | NodeType::VectorResult)
        && any_property_contains(node, "SubPlan")
    {
        blockers.push("关联子链接(SubPlan)".to_string());
    }
    if let Some(filter) = node
        .properties
        .iter()
        .find(|p| p.label == "Filter")
        .map(|p| p.value.as_str())
    {
        if filter.contains("now()") || filter.contains("random()") || filter.contains("nextval") {
            blockers.push("易变函数调用".to_string());
        }
    }
    for child in &node.children {
        collect_blockers_recursive(child, blockers);
    }
}

pub struct MultiLayerStreaming;

impl DiagnosticRule for MultiLayerStreaming {
    fn id(&self) -> &str {
        "PUSH-002"
    }
    fn name(&self) -> &str {
        "多层流式传输"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::PushdownFailure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if !matches!(
            &node.node_type,
            NodeType::Streaming(_) | NodeType::VectorStreaming(_)
        ) {
            return None;
        }

        let mut layers: Vec<String> = Vec::new();
        collect_streaming_layers(&node.children, &mut layers);

        if layers.is_empty() {
            return None;
        }

        let current_type = streaming_type_name(&node.node_type);
        let total_layers = layers.len() + 1;

        let detail = format!(
            "Streaming 节点下存在 {} 层 Streaming — 数据重分布过多: {} → {}",
            total_layers - 1,
            current_type,
            layers.join(" → ")
        );

        let suggestion = if total_layers >= 3 {
            "多层重分布严重影响性能; /*+ redistribute(t1) */ 显式指定; /*+ broadcast(small) */ 广播小表; /*+ leading(t1 t2 t3) */ 调整连接顺序".to_string()
        } else {
            "使用 hint 减少重分布层数: /*+ redistribute(t1) */ 或 /*+ broadcast(small) */"
                .to_string()
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn collect_streaming_layers(children: &[PlanNode], layers: &mut Vec<String>) {
    for child in children {
        if matches!(
            &child.node_type,
            NodeType::Streaming(_) | NodeType::VectorStreaming(_)
        ) {
            layers.push(streaming_type_name(&child.node_type));
        }
        collect_streaming_layers(&child.children, layers);
    }
}

fn streaming_type_name(nt: &NodeType) -> String {
    match nt {
        NodeType::Streaming(st) => format!("Streaming({:?})", st),
        NodeType::VectorStreaming(st) => format!("VectorStreaming({:?})", st),
        _ => nt.to_string(),
    }
}
