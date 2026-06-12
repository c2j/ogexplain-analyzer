//! Simple template rendering for anti-pattern diagnostic messages.
//!
//! Supports `{capture_name}` and `{capture_name.property}` placeholders
//! using [`str::replace`]. No external template engine is required for the
//! current set of anti-patterns.
//!
//! Supported properties:
//! - `{name}` — node type string
//! - `{name.relation}` — relation (table) name, or `?` if absent
//! - `{name.actual_rows}` — actual rows (formatted as integer)
//! - `{name.loops}` — loop count (formatted as integer)
//! - `{name.total_work}` — rows × loops (formatted as integer)
//! - `{name.line}` — line number in the original output

use std::collections::HashMap;

use super::engine::AntiPatternDef;
use super::types::MatchResult;
use crate::model::PlanNode;

/// Render the detail message for a matched anti-pattern.
///
/// Delegates to [`render_template`] with the pattern's detail template.
pub fn render_detail(pattern: &dyn AntiPatternDef, result: &MatchResult) -> String {
    let detail_template = pattern.detail_template();
    render_template(&detail_template, &result.captures)
}

/// Render the suggestion message for a matched anti-pattern.
///
/// Delegates to [`render_template`] with the pattern's suggestion template.
pub fn render_suggestion(pattern: &dyn AntiPatternDef, result: &MatchResult) -> String {
    let suggestion_template = pattern.suggestion_template();
    render_template(&suggestion_template, &result.captures)
}

/// Replace `{capture_name.property}` placeholders with node field values.
///
/// For each capture in `captures`:
/// - `{name}` → node type string
/// - `{name.relation}` → relation name or `"?"`
/// - `{name.actual_rows}` → `a.rows` formatted without decimal places
/// - `{name.loops}` → `a.loops` formatted without decimal places
/// - `{name.total_work}` → `a.rows * a.loops` formatted without decimal places
/// - `{name.line}` → line number
///
/// Numeric formatting uses [`format!`] with `{:.0}` to avoid raw `as` casts.
fn render_template(template: &str, captures: &HashMap<String, &PlanNode>) -> String {
    let mut result = template.to_string();
    for (name, node) in captures {
        result = result.replace(&format!("{{{name}}}"), &node.node_type.to_string());
        let rel = node.relation.as_deref().unwrap_or("?");
        result = result.replace(&format!("{{{name}.relation}}"), rel);
        if let Some(actual) = &node.actual {
            result = result.replace(
                &format!("{{{name}.actual_rows}}"),
                &format!("{:.0}", actual.rows),
            );
            result = result.replace(
                &format!("{{{name}.loops}}"),
                &format!("{:.0}", actual.loops),
            );
            result = result.replace(
                &format!("{{{name}.total_work}}"),
                &format!("{:.0}", actual.rows * actual.loops),
            );
        }
        result = result.replace(
            &format!("{{{name}.line}}"),
            &format!("{}", node.line_number),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node_type::NodeType;
    use crate::model::{ActualStats, PlanNode};

    fn make_captured_node(node_type: NodeType, rows: f64, loops: f64) -> PlanNode {
        PlanNode {
            node_type,
            relation: Some("test_table".to_string()),
            join_type: None,
            estimated: None,
            actual: Some(ActualStats {
                startup_time_ms: 0.0,
                total_time_ms: 50.0,
                rows,
                loops,
                executed: true,
            }),
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 3,
        }
    }

    #[test]
    fn test_render_node_type_placeholder() {
        let node = make_captured_node(NodeType::Materialize, 1000.0, 1.0);
        let mut captures = HashMap::new();
        captures.insert("mat1".to_string(), &node);

        let result = render_template("Found {mat1}", &captures);
        assert_eq!(result, "Found Materialize");
    }

    #[test]
    fn test_render_relation_placeholder() {
        let node = make_captured_node(NodeType::SeqScan, 0.0, 0.0);
        let mut captures = HashMap::new();
        captures.insert("scan".to_string(), &node);

        let result = render_template("Scan on {scan.relation}", &captures);
        assert_eq!(result, "Scan on test_table");
    }

    #[test]
    fn test_render_actual_rows() {
        let node = make_captured_node(NodeType::SeqScan, 12345.0, 1.0);
        let mut captures = HashMap::new();
        captures.insert("s".to_string(), &node);

        let result = render_template("{s.actual_rows} rows", &captures);
        assert_eq!(result, "12345 rows");
    }

    #[test]
    fn test_render_loops() {
        let node = make_captured_node(NodeType::NestedLoop, 100.0, 500.0);
        let mut captures = HashMap::new();
        captures.insert("nl".to_string(), &node);

        let result = render_template("Loops: {nl.loops}", &captures);
        assert_eq!(result, "Loops: 500");
    }

    #[test]
    fn test_render_total_work() {
        let node = make_captured_node(NodeType::IndexScan, 50.0, 1000.0);
        let mut captures = HashMap::new();
        captures.insert("idx".to_string(), &node);

        let result = render_template("Total work: {idx.total_work}", &captures);
        assert_eq!(result, "Total work: 50000");
    }

    #[test]
    fn test_render_line_number() {
        let node = make_captured_node(NodeType::Sort, 0.0, 0.0);
        let mut captures = HashMap::new();
        captures.insert("sort".to_string(), &node);

        let result = render_template("At line {sort.line}", &captures);
        assert_eq!(result, "At line 3");
    }

    #[test]
    fn test_multiple_captures() {
        let nl = make_captured_node(NodeType::NestedLoop, 50000.0, 1.0);
        let idx = make_captured_node(NodeType::IndexScan, 1.0, 50000.0);

        let mut captures = HashMap::new();
        captures.insert("nl".to_string(), &nl);
        captures.insert("idx".to_string(), &idx);

        let result = render_template(
            "{nl} drives {nl.actual_rows} loops, {idx} on {idx.relation}: {idx.total_work}",
            &captures,
        );
        assert_eq!(
            result,
            "NestedLoop drives 50000 loops, IndexScan on test_table: 50000"
        );
    }

    #[test]
    fn test_relation_fallback_to_question_mark() {
        let node = PlanNode {
            node_type: NodeType::HashJoin,
            relation: None,
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: 1,
        };
        let mut captures = HashMap::new();
        captures.insert("hj".to_string(), &node);

        let result = render_template("On {hj.relation}", &captures);
        assert_eq!(result, "On ?");
    }
}
