use regex::Regex;

use crate::model::PlanNode;

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
        if node.node_type != crate::model::NodeType::SeqScan {
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

        let mismatch = detect_type_mismatch(filter_value);

        let detail = format!(
            "Seq Scan 含过滤条件 '{}' ({}), 过滤掉 {} 行 — 疑似隐式类型转换导致无法使用索引",
            filter_value,
            mismatch
                .as_ref()
                .map(|m| m.description())
                .unwrap_or_else(|| "类型不匹配".to_string()),
            rows_removed
        );

        let suggestion = mismatch
            .as_ref()
            .map(|m| m.fix_suggestion())
            .unwrap_or_else(|| {
                format!(
                    "WHERE 条件存在类型不匹配: {}, 隐式转换导致无法使用索引; 添加显式类型转换",
                    filter_value
                )
            });

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

struct TypeMismatch {
    column: String,
    value_type: String,
    expected_type: String,
}

impl TypeMismatch {
    fn description(&self) -> String {
        format!(
            "{}({}) = {}值",
            self.expected_type, self.column, self.value_type
        )
    }

    fn fix_suggestion(&self) -> String {
        match self.value_type.as_str() {
            "int" => format!(
                "WHERE {} = N — 疑似 varchar 列用 int 值比较, 改为 WHERE {} = 'N'",
                self.column, self.column
            ),
            _ => format!(
                "添加显式类型转换: WHERE {} = value::{}",
                self.column, self.expected_type
            ),
        }
    }
}

fn detect_type_mismatch(filter: &str) -> Option<TypeMismatch> {
    let re_int = Regex::new(r"(\w+)\s*=\s*(\d+)\b").ok()?;
    if let Some(cap) = re_int.captures(filter) {
        let col = cap.get(1)?.as_str().to_string();
        return Some(TypeMismatch {
            column: col,
            value_type: "int".to_string(),
            expected_type: "varchar".to_string(),
        });
    }
    None
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
                && (prop.value.contains("LIKE '%")
                    || prop.value.contains("like '%")
                    || prop.value.contains("~~ '%"))
            {
                let pattern =
                    extract_like_pattern(&prop.value).unwrap_or_else(|| prop.value.clone());

                let is_double_sided =
                    pattern.starts_with('%') && pattern.ends_with('%') && pattern.len() > 1;

                let detail = format!(
                    "过滤条件含前导通配符 LIKE '{}', 无法使用 B-tree 索引{}",
                    pattern,
                    if is_double_sided {
                        " (前后均有通配符)"
                    } else {
                        ""
                    }
                );

                let suggestion = if is_double_sided {
                    "前后通配符 LIKE 无法使用任何索引; 建议: (1) pg_trgm 扩展 + GIN 索引: CREATE EXTENSION pg_trgm; CREATE INDEX idx USING gin(col gin_trgm_ops); (2) 全文搜索: to_tsvector + to_tsquery".to_string()
                } else {
                    "前导通配符 LIKE 无法使用 B-tree 索引; 建议: pg_trgm 扩展; 或反向索引(reverse(col))".to_string()
                };

                return Some(make_finding(self, detail, node, Some(suggestion)));
            }
        }
        None
    }
}

fn extract_like_pattern(value: &str) -> Option<String> {
    let re = Regex::new(r#"(?:LIKE|like|~~)\s+'([^']+)'"#).ok()?;
    re.captures(value)
        .map(|cap| cap.get(1).unwrap().as_str().to_string())
}
