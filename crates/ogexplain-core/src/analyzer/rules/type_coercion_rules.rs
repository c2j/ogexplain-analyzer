use regex::Regex;

use crate::model::{NodeType, PlanNode};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

pub struct SuspectedImplicitTypeCast;

impl DiagnosticRule for SuspectedImplicitTypeCast {
    fn id(&self) -> &str {
        "TYPE-001"
    }
    fn name(&self) -> &str {
        "疑似隐式类型转换"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::TypeMismatch
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::SeqScan {
            return None;
        }
        let filter_prop = node.properties.iter().find(|p| p.label == "Filter")?;
        let filter_value = &filter_prop.value;

        let re = Regex::new(r"\w+\s*=\s*\d+(\.\d+)?\b").ok()?;
        if !re.is_match(filter_value) {
            return None;
        }

        let rows_removed = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Filter")
            .and_then(|p| p.value.trim().parse::<f64>().ok())?;
        if rows_removed <= 1000.0 {
            return None;
        }

        Some(make_finding(
            self,
            format!(
                "顺序扫描含过滤条件 '{}' 且被过滤掉 {} 行 — 疑似隐式类型转换",
                filter_value, rows_removed
            ),
            node,
            Some(format!(
                "WHERE 条件存在类型不匹配: {}, 隐式转换导致无法使用索引且无法DN裁剪; 添加显式类型转换: WHERE col = value::correct_type; 如 varchar列=int值, 改为 WHERE col = 'value'",
                filter_value
            )),
        ))
    }
}

pub struct LikeWithLeadingWildcard;

impl DiagnosticRule for LikeWithLeadingWildcard {
    fn id(&self) -> &str {
        "TYPE-004"
    }
    fn name(&self) -> &str {
        "LIKE 使用前导通配符"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::TypeMismatch
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        for prop in &node.properties {
            if (prop.label == "Filter" || prop.label == "Index Cond")
                && (prop.value.contains("LIKE '%") || prop.value.contains("like '%"))
            {
                return Some(make_finding(
                    self,
                    format!("过滤条件含前导通配符的 LIKE: {}", prop.value),
                    node,
                    Some(
                        "LIKE 前导通配符无法使用索引，建议使用全文搜索或 pg_trgm 扩展".to_string(),
                    ),
                ));
            }
        }
        None
    }
}
