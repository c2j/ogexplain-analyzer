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

        // Use the shared utility for column extraction (fixes Bug B1).
        // extract_column_from_filter strips ::cast annotations and handles
        // parenthesized expressions, returning the real column name.
        let column = super::utils::extract_column_from_filter(filter_value)?;

        // Detect asymmetric cast (fixes Bug B2) — only fire on genuine
        // type mismatch suspects, not symmetric ::text = 'val'::text.
        let mismatch = detect_asymmetric_cast(filter_value, &column)?;

        // Row removal thresholds (unchanged from original)
        let rows_removed = node
            .properties
            .iter()
            .find(|p| p.label == "Rows Removed by Filter")
            .and_then(|p| p.value.trim().parse::<f64>().ok())?;

        let actual_rows = node.actual.as_ref().map(|a| a.rows).unwrap_or(0.0);
        let total_scanned = rows_removed + actual_rows;

        // Minimum absolute threshold to avoid noise
        if rows_removed <= 10.0 {
            return None;
        }
        // Removal ratio must exceed 50% to be significant
        if total_scanned > 0.0 && rows_removed / total_scanned <= 0.5 {
            return None;
        }

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

/// Describes the type of asymmetric cast pattern detected.
#[derive(Debug, Clone, PartialEq)]
enum MismatchPattern {
    /// `col = 42` — bare column compared to bare integer (e.g., varchar_col compared to int)
    BareColumnBareInteger,
    /// `col = '1002'` — bare column with numeric-looking string literal
    BareColumnStringLiteral,
    /// `(col)::numeric = '1002'` — column cast to numeric vs uncast string
    ColumnToNumeric,
}

/// Result of asymmetric cast detection.
struct TypeMismatch {
    column: String,
    literal_value: String,
    pattern: MismatchPattern,
}

impl TypeMismatch {
    fn description(&self) -> String {
        match self.pattern {
            MismatchPattern::BareColumnBareInteger => {
                format!("{} = {} (列vs整数值)", self.column, self.literal_value)
            }
            MismatchPattern::BareColumnStringLiteral => {
                format!("{} = '{}' (列vs字符串值)", self.column, self.literal_value)
            }
            MismatchPattern::ColumnToNumeric => {
                format!(
                    "({})::numeric = '{}' (列被转为numeric)",
                    self.column, self.literal_value
                )
            }
        }
    }

    fn fix_suggestion(&self) -> String {
        match self.pattern {
            MismatchPattern::BareColumnBareInteger => {
                format!(
                    "WHERE {} = '{}' — 疑似 varchar 列用 int 值比较, 建议在等号右侧加引号避免隐式转换",
                    self.column, self.literal_value
                )
            }
            MismatchPattern::BareColumnStringLiteral => {
                format!(
                    "WHERE {} = {} — 疑似 numeric 列用 string 值比较, 建议去掉引号或添加显式类型转换",
                    self.column, self.literal_value
                )
            }
            MismatchPattern::ColumnToNumeric => {
                format!(
                    "WHERE {} = '{}' — 列被强制转为 numeric 类型, 建议统一列和值的数据类型",
                    self.column, self.literal_value
                )
            }
        }
    }
}

/// Detect an asymmetric cast pattern. Returns `None` when:
/// - Both sides share the same `::type` cast (symmetric — valid comparison)
/// - No cast mismatch is detectable
fn detect_asymmetric_cast(filter: &str, column: &str) -> Option<TypeMismatch> {
    // If both sides of `=` have the same ::type annotation, it's symmetric — skip.
    if has_symmetric_cast(filter) {
        return None;
    }

    let escaped_col = regex::escape(column);
    // Allow zero or more closing parens between column name and `=`
    // to handle e.g. `(amount) = '1002'` after ::cast stripping.
    let col_paren = format!(r"{}\)*", escaped_col);

    // Pattern 1: bare column compared to bare integer (e.g., `status = 42`)
    let re_bare_int = Regex::new(&format!(r"(?i){}\s*=\s*(\d+)\b", col_paren)).ok()?;
    if let Some(cap) = re_bare_int.captures(filter) {
        return Some(TypeMismatch {
            column: column.to_string(),
            literal_value: cap.get(1)?.as_str().to_string(),
            pattern: MismatchPattern::BareColumnBareInteger,
        });
    }

    // Pattern 2: bare column = 'numeric-looking literal' (e.g., `amount = '1002'`)
    let re_str_lit = Regex::new(&format!(r"(?i){}\s*=\s*'([^']+)'", col_paren)).ok()?;
    if let Some(cap) = re_str_lit.captures(filter) {
        let val = cap.get(1)?.as_str();
        if val.parse::<f64>().is_ok() {
            return Some(TypeMismatch {
                column: column.to_string(),
                literal_value: val.to_string(),
                pattern: MismatchPattern::BareColumnStringLiteral,
            });
        }
    }

    // Pattern 3: `(col)::numeric = 'literal'` — column forced to numeric
    let re_col_numeric = Regex::new(&format!(
        r"(?i)\(\s*{}\s*\)\s*::\s*numeric\s*=\s*'([^']+)'",
        escaped_col
    ))
    .ok()?;
    if let Some(cap) = re_col_numeric.captures(filter) {
        return Some(TypeMismatch {
            column: column.to_string(),
            literal_value: cap.get(1)?.as_str().to_string(),
            pattern: MismatchPattern::ColumnToNumeric,
        });
    }

    None
}

/// Check if both sides of `=` have the same `::type` cast annotation.
/// Symmetric casts like `(col)::text = 'val'::text` are valid comparisons
/// and should not fire TYPE-001.
fn has_symmetric_cast(filter: &str) -> bool {
    if let Some(eq_pos) = filter.find('=') {
        let left = &filter[..eq_pos];
        let right = &filter[eq_pos + 1..];

        if let (Some(lt), Some(rt)) = (extract_cast_type(left), extract_cast_type(right)) {
            return lt == rt;
        }
    }
    false
}

/// Extract the rightmost `::type` from a string, e.g. `(col)::text` → `"text"`.
fn extract_cast_type(s: &str) -> Option<String> {
    let re = Regex::new(r"::([a-zA-Z_]\w*)").ok()?;
    let caps: Vec<String> = re
        .captures_iter(s)
        .map(|c| c.get(1).unwrap().as_str().to_lowercase())
        .collect();
    caps.last().cloned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::context::{GlobalStats, PlanContext};
    use crate::model::buffer::NodeProperty;
    use crate::model::cost::{ActualStats, EstimatedCost};
    use crate::model::{ExplainPlan, NodeType, PlanNode};

    /// Helper: call `SuspectedImplicitTypeCast::check` with a minimal PlanContext.
    fn test_check(node: &PlanNode) -> Option<Finding> {
        let rule = SuspectedImplicitTypeCast;
        let dummy_root = PlanNode {
            node_type: NodeType::Result,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 0,
        };
        let plan = ExplainPlan {
            root: dummy_root,
            summary: None,
        };
        let stats = GlobalStats::compute(&plan);
        let ctx = PlanContext {
            plan: &plan,
            global_stats: &stats,
        };
        rule.check(node, &ctx)
    }

    fn make_seqscan_with_filter(filter: &str, rows_removed: f64, actual_rows: f64) -> PlanNode {
        let total = rows_removed + actual_rows;
        PlanNode {
            node_type: NodeType::SeqScan,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: Some(EstimatedCost {
                startup_cost: 0.0,
                total_cost: total * 0.1,
                plan_rows: total,
                plan_width: 8,
                pred_time: None,
                pred_rows: None,
                distinct: None,
            }),
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: 100.0,
                rows: actual_rows,
                loops: 1.0,
                executed: true,
            }),
            properties: vec![
                NodeProperty {
                    label: "Filter".to_string(),
                    value: filter.to_string(),
                },
                NodeProperty {
                    label: "Rows Removed by Filter".to_string(),
                    value: rows_removed.to_string(),
                },
            ],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 1,
        }
    }

    // ── Positive: should fire ────────────────────────────────────

    #[test]
    fn test_type001_fires_on_bare_column_with_numeric_literal() {
        // amount = '1002' — no cast, numeric-looking string literal
        let node = make_seqscan_with_filter("amount = '1002'", 100_000.0, 1_000.0);
        let finding = test_check(&node);
        assert!(
            finding.is_some(),
            "Should fire on bare col with numeric-looking literal"
        );
        let f = finding.unwrap();
        assert!(
            f.suggestion
                .as_ref()
                .unwrap_or(&"".to_string())
                .contains("amount"),
            "suggestion must contain real column name 'amount', got: {:?}",
            f.suggestion
        );
    }

    #[test]
    fn test_type001_fires_on_column_cast_to_numeric() {
        // (code)::numeric = '1002' — column cast to numeric, literal is string
        let node = make_seqscan_with_filter("(code)::numeric = '1002'", 100_000.0, 1_000.0);
        let finding = test_check(&node);
        assert!(
            finding.is_some(),
            "Should fire on column cast to ::numeric with string literal"
        );
    }

    // ── Guard: must NOT fire ─────────────────────────────────────

    #[test]
    fn test_type001_does_not_fire_on_symmetric_text_cast() {
        // KEY regression: 38-case had 15 false positives of this pattern
        // ((facctcode)::text = '1002'::text) — both sides ::text, valid text=text comparison
        let node =
            make_seqscan_with_filter("((facctcode)::text = '1002'::text)", 1_000_000.0, 50_000.0);
        let finding = test_check(&node);
        assert!(
            finding.is_none(),
            "Must NOT fire on symmetric ::text cast — this was the false positive pattern"
        );
    }

    #[test]
    fn test_type001_does_not_fire_on_non_numeric_string_literal() {
        // status = 'ready' — literal is not numeric-looking, no suspicion
        let node = make_seqscan_with_filter("status = 'ready'", 100.0, 50.0);
        let finding = test_check(&node);
        assert!(
            finding.is_none(),
            "Must NOT fire on non-numeric-looking string literal"
        );
    }

    #[test]
    fn test_type001_does_not_fire_on_low_row_removal() {
        // Only 5 rows removed out of 1005 — below 50% threshold
        let node = make_seqscan_with_filter("amount = '100'", 5.0, 1000.0);
        let finding = test_check(&node);
        assert!(
            finding.is_none(),
            "Must NOT fire when row removal ratio is below 50%"
        );
    }

    #[test]
    fn test_type001_suggestion_uses_real_column_name() {
        // Critical regression: ensure suggestion uses real column name (was 'text')
        let node = make_seqscan_with_filter("amount = '1002'", 1_000_000.0, 50_000.0);
        let finding = test_check(&node).expect("Should fire on bare col with numeric literal");
        let s = finding.suggestion.unwrap();
        assert!(
            s.contains("amount"),
            "suggestion must use real column 'amount', got: {}",
            s
        );
        assert!(
            !s.contains("WHERE text"),
            "suggestion must NOT contain literal 'WHERE text'"
        );
    }
}
