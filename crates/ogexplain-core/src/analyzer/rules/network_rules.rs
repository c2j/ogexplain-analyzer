use crate::model::{NodeType, PlanNode, StreamingType};

use super::super::config::DiagnosticConfig;
use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

pub struct BroadcastLargeTable {
    threshold: f64,
}

impl BroadcastLargeTable {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.large_table_rows,
        }
    }
}

impl DiagnosticRule for BroadcastLargeTable {
    fn id(&self) -> &str {
        "NET-001"
    }
    fn name(&self) -> &str {
        "广播大表数据"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::NetworkOverhead
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        match &node.node_type {
            NodeType::Streaming(StreamingType::Broadcast) => {}
            _ => return None,
        }
        let actual = node.actual.as_ref()?;
        if actual.rows <= self.threshold {
            return None;
        }
        Some(make_finding(
            self,
            format!(
                "Streaming(Broadcast) 传输 {} 行（阈值: {}）",
                actual.rows, self.threshold
            ),
            node,
            Some("使用 /*+ redistribute(t1) */ 替代广播; 或 /*+ no broadcast(t1) */ 禁止广播; 调整分布列使数据本地化以避免重分布".to_string()),
        ))
    }
}
