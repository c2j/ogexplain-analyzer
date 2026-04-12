use crate::model::{NodeType, PlanNode, StreamingType};

use super::super::context::{GlobalStats, PlanContext};
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};
use crate::model::ExplainPlan;

const REDISTRIBUTE_LARGE_ROWS: f64 = 10000.0;

// SKEW-001: Data skew detected via Streaming(Redistribute) nodes where
// actual rows >> estimated rows (5x underestimate proxy for uneven distribution).
pub struct DataSkewDetected;

impl DiagnosticRule for DataSkewDetected {
    fn id(&self) -> &str {
        "SKEW-001"
    }

    fn name(&self) -> &str {
        "数据倾斜"
    }

    fn severity(&self) -> Severity {
        Severity::Critical
    }

    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::DistributionIssue
    }

    fn check(&self, _node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        None
    }

    fn check_global(&self, plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        let mut findings = Vec::new();
        Self::walk_node(&plan.root, &mut findings);
        findings
    }
}

impl DataSkewDetected {
    fn walk_node(node: &PlanNode, findings: &mut Vec<Finding>) {
        if let NodeType::Streaming(StreamingType::Redistribute) = node.node_type {
            if let (Some(actual), Some(estimated)) = (&node.actual, &node.estimated) {
                if actual.rows > 100000.0
                    && estimated.plan_rows > 0.0
                    && actual.rows / estimated.plan_rows > 5.0
                {
                    let ratio = actual.rows / estimated.plan_rows;
                    let relation = node.relation.as_deref().unwrap_or("unknown");
                    findings.push(make_finding(
                        &DataSkewDetected,
                        format!(
                            "Streaming(Redistribute)重分布{}行, 实际与估算偏差{:.1}x — 疑似数据倾斜",
                            actual.rows as u64, ratio
                        ),
                        node,
                        Some(format!(
                            "调整分布列: ALTER TABLE {} DISTRIBUTE BY HASH(new_col); 使用 /*+ skew(t1(c1)) */ Hint; 查询倾斜: SELECT * FROM pgxc_get_table_skewness('{}')",
                            relation, relation
                        )),
                    ));
                }
            }
        }
        for child in &node.children {
            Self::walk_node(child, findings);
        }
    }
}

// DIST-001: Distribution column mismatch causing redistribution
pub struct DistributionColumnMismatch;

impl DiagnosticRule for DistributionColumnMismatch {
    fn id(&self) -> &str {
        "DIST-001"
    }
    fn name(&self) -> &str {
        "分布列不当导致重分布"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::DistributionIssue
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if let NodeType::Streaming(StreamingType::Redistribute) = node.node_type {
            if let Some(actual) = &node.actual {
                if actual.rows > REDISTRIBUTE_LARGE_ROWS {
                    let relation = node.relation.as_deref().unwrap_or("unknown");
                    return Some(make_finding(
                        self,
                        format!(
                            "Streaming(Redistribute)重分布{}行, 连接列与分布列不匹配",
                            actual.rows as u64
                        ),
                        node,
                        Some(format!(
                            "Streaming(Redistribute)重分布{}行, 连接列与分布列不匹配; 对齐分布列与JOIN列: ALTER TABLE {} DISTRIBUTE BY HASH(join_col); 使用 /*+ redistribute(t1) */ 显式指定重分布策略",
                            actual.rows as u64, relation
                        )),
                    ));
                }
            }
        }
        None
    }

    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        Vec::new()
    }
}
