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

        // Match both bare numeric AND quoted string literal comparisons
        // Pattern 1: col = 123 (bare int, possible varchar col compared to int)
        // Pattern 2: col = '123' (quoted string that looks numeric, possible int col compared to string)
        let re_bare = Regex::new(r"\w+\s*=\s*\d+(\.\d+)?\b").ok()?;
        let re_quoted = Regex::new(r"\w+\s*=\s*'[^']*'").ok()?;

        if !re_bare.is_match(filter_value) && !re_quoted.is_match(filter_value) {
            return None;
        }

        // Use RELATIVE threshold instead of absolute: filter must remove >50% of scanned rows
        let rows_removed = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Filter")
            .and_then(|p| p.value.trim().parse::<f64>().ok());

        let actual_rows = node.actual.as_ref().map(|a| a.rows).unwrap_or(0.0);
        let total_scanned = rows_removed.unwrap_or(0.0) + actual_rows;

        let rows_removed = rows_removed?;
        // Minimum absolute threshold to avoid noise
        if rows_removed <= 10.0 {
            return None;
        }
        // Removal ratio must exceed 50% to be significant
        if total_scanned > 0.0 && rows_removed / total_scanned <= 0.5 {
            return None;
        }

        // Require a detected type mismatch — harmless string comparisons (e.g., status = 'pending')
        // should not fire. Only flag when actual type coercion is suspected.
        let mismatch = detect_type_mismatch(filter_value)?;

        let detail = format!(
            "Seq Scan 含过滤条件 '{}' ({}), 过滤掉 {} 行 (共 {} 行) — 疑似隐式类型转换导致无法使用索引",
            filter_value,
            mismatch.description(),
            rows_removed as i64,
            total_scanned as i64
        );

        let suggestion = mismatch.fix_suggestion();

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

struct TypeMismatch {
    column: String,
    value_type: String,
    expected_type: String,
    literal_value: String,
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
                "WHERE {} = {} — 疑似 varchar 列用 int 值比较, 改为 WHERE {} = '{}'",
                self.column, self.literal_value, self.column, self.literal_value
            ),
            "string_literal" => format!(
                "WHERE {} = '{}' — 疑似 numeric 列用 string 值比较, 改为 WHERE {} = {}",
                self.column, self.literal_value, self.column, self.literal_value
            ),
            _ => format!("添加显式类型转换: WHERE {} = value::{}", self.column, self.expected_type),
        }
    }
}

fn detect_type_mismatch(filter: &str) -> Option<TypeMismatch> {
    // Direction 1: string literal compared to column (e.g., int_col = '42')
    let re_str = Regex::new(r"(\w+)\s*=\s*'([^']*)'").ok()?;
    if let Some(cap) = re_str.captures(filter) {
        let col = cap.get(1)?.as_str().to_string();
        let val = cap.get(2)?.as_str().to_string();
        // If the string literal is a numeric string, likely an int column compared to string
        if val.parse::<f64>().is_ok() {
            return Some(TypeMismatch {
                column: col,
                value_type: "string_literal".to_string(),
                expected_type: "numeric".to_string(),
                literal_value: val,
            });
        }
    }

    // Direction 2: bare numeric compared to column (e.g., varchar_col = 42)
    let re_int = Regex::new(r"(\w+)\s*=\s*(\d+)\b").ok()?;
    if let Some(cap) = re_int.captures(filter) {
        let col = cap.get(1)?.as_str().to_string();
        let val = cap.get(2)?.as_str().to_string();
        return Some(TypeMismatch {
            column: col,
            value_type: "int".to_string(),
            expected_type: "varchar".to_string(),
            literal_value: val,
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
