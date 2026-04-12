use crate::model::{NodeType, PlanNode, StreamingType};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
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
        match &node.node_type {
            NodeType::Streaming(StreamingType::Redistribute)
            | NodeType::Streaming(StreamingType::Broadcast) => {}
            _ => return None,
        }
        let streaming_type = match &node.node_type {
            NodeType::Streaming(st) => st.to_string(),
            _ => unreachable!(),
        };
        Some(make_finding(
            self,
            format!("查询未完全下推 — 发现 Streaming({}) 节点", streaming_type),
            node,
            Some("检查不可下推构造(易变函数/特殊语法); 使用 /*+ EXPAND_SUBLINK */ 提升子链接; /*+ EXPAND_SUBQUERY */ 提升子查询; SET rewrite_rule=partialpush 尝试部分下推".to_string()),
        ))
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
        if !has_streaming_descendant(&node.children) {
            return None;
        }
        Some(make_finding(
            self,
            "Streaming 节点下存在另一个 Streaming 子节点 — 数据重分布过多".to_string(),
            node,
            Some("使用 /*+ redistribute(t1) */ 显式指定重分布减少层数; /*+ broadcast(small_dim) */ 广播小表避免重分布; /*+ leading(t1 t2 t3) */ 调整连接顺序优化流策略".to_string()),
        ))
    }
}

fn has_streaming_descendant(children: &[PlanNode]) -> bool {
    for child in children {
        if matches!(
            &child.node_type,
            NodeType::Streaming(_) | NodeType::VectorStreaming(_)
        ) {
            return true;
        }
        if has_streaming_descendant(&child.children) {
            return true;
        }
    }
    false
}
