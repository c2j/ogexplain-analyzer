//! Shared utility functions for diagnostic rules.
//!
//! Common helpers extracted from individual rule files to avoid duplication
//! and enable cross-rule reuse (table name extraction, property lookup, etc.).

use crate::model::{NodeType, PlanNode};

/// Returns true if the NodeType is any kind of scan node.
pub fn is_scan_node(nt: &NodeType) -> bool {
    matches!(
        nt,
        NodeType::SeqScan
            | NodeType::IndexScan
            | NodeType::IndexOnlyScan
            | NodeType::BitmapHeapScan
            | NodeType::CStoreScan
            | NodeType::CStoreIndexScan
            | NodeType::PartitionedSeqScan
            | NodeType::PartitionedIndexScan
            | NodeType::PartitionedBitmapHeapScan
    )
}

/// Returns true if the NodeType is any kind of sort node.
pub fn is_sort_node(nt: &NodeType) -> bool {
    matches!(
        nt,
        NodeType::Sort | NodeType::VectorSort | NodeType::GroupSort
    )
}

/// Returns true if the NodeType is any DML node.
pub fn is_dml_node(nt: &NodeType) -> bool {
    matches!(
        nt,
        NodeType::Update
            | NodeType::VectorUpdate
            | NodeType::ModifyTable
            | NodeType::Delete
            | NodeType::VectorDelete
            | NodeType::Insert
            | NodeType::VectorInsert
    )
}

/// Multi-level fallback to extract target table name from a plan node.
///
/// Tries: `node.relation` → `child.relation` → `grandchild.relation`.
pub fn extract_target_table(node: &PlanNode) -> Option<String> {
    if let Some(ref rel) = node.relation {
        return Some(first_identifier(rel));
    }
    if let Some(child) = node.children.first() {
        if let Some(ref rel) = child.relation {
            return Some(first_identifier(rel));
        }
        if let Some(grandchild) = child.children.first() {
            if let Some(ref rel) = grandchild.relation {
                return Some(first_identifier(rel));
            }
        }
    }
    None
}

/// Extract the first identifier from a string, stripping alias.
///
/// `"employees e"` → `"employees"`, `"orders"` → `"orders"`.
pub fn first_identifier(s: &str) -> String {
    s.split_whitespace().next().unwrap_or(s).to_string()
}

/// Check if a relation name matches a target table name (ignoring aliases).
pub fn table_name_match(relation: &str, target: &str) -> bool {
    first_identifier(relation) == target
}

/// Calculate the effective number of rows a scan node EXAMINED (not just output).
///
/// `actual.rows` is the OUTPUT row count, artificially low when:
/// - A parent LIMIT node truncates output
/// - A Filter removes most rows
///
/// This function reconstructs the true scan size:
/// - No Filter: `estimated.plan_rows` is the full table size
/// - With Filter: `(actual.rows × loops) + Rows Removed by Filter`
pub fn effective_scan_size(node: &PlanNode) -> f64 {
    let has_filter = node.properties.iter().any(|p| p.label == "Filter");
    if !has_filter {
        return node.estimated.as_ref().map(|e| e.plan_rows).unwrap_or(0.0);
    }
    let actual = match node.actual.as_ref() {
        Some(a) => a,
        None => return node.estimated.as_ref().map(|e| e.plan_rows).unwrap_or(0.0),
    };
    let rows_removed: f64 = node
        .properties
        .iter()
        .find(|p| p.label == "Rows Removed by Filter")
        .and_then(|p| p.value.trim().parse::<f64>().ok())
        .unwrap_or(0.0);
    (actual.rows * actual.loops) + rows_removed
}

/// Find a property value by label.
pub fn get_property_value<'a>(node: &'a PlanNode, label: &str) -> Option<&'a str> {
    node.properties
        .iter()
        .find(|p| p.label == label)
        .map(|p| p.value.as_str())
}

/// Check if any property value contains the given string.
pub fn any_property_contains(node: &PlanNode, needle: &str) -> bool {
    node.properties.iter().any(|p| p.value.contains(needle))
}

fn strip_cast_annotations(s: &str) -> String {
    let re = regex::Regex::new(r"::[a-zA-Z_][a-zA-Z0-9_]*(\([^)]*\))?")
        .expect("valid strip_cast_annotations regex");
    re.replace_all(s, "").to_string()
}

fn is_reserved_type_name(s: &str) -> bool {
    matches!(
        s.to_lowercase().as_str(),
        "text"
            | "numeric"
            | "int"
            | "int2"
            | "int4"
            | "int8"
            | "bigint"
            | "smallint"
            | "integer"
            | "varchar"
            | "char"
            | "bpchar"
            | "float"
            | "float4"
            | "float8"
            | "double"
            | "precision"
            | "real"
            | "bool"
            | "boolean"
            | "date"
            | "timestamp"
            | "timestamptz"
            | "time"
            | "timetz"
            | "interval"
            | "name"
            | "bytea"
            | "uuid"
            | "json"
            | "jsonb"
            | "clob"
            | "blob"
            | "raw"
    )
}

/// Extract the first column name from a SQL filter expression of the form
/// `column = literal` or `(column)::type = literal::type`.
///
/// Handles OpenGauss `::type` cast annotations by stripping them first,
/// so `((facctcode)::text = '1002'::text)` correctly returns `"facctcode"`
/// instead of `"text"`.
///
/// Returns `None` when no `identifier = value` pattern is found.
pub fn extract_column_from_filter(filter: &str) -> Option<String> {
    let stripped = strip_cast_annotations(filter);
    let cleaned = stripped.trim();
    // After stripping ::cast annotations, closing parens may remain
    // between the column name and `=`, e.g. `(status) = 'ready'`.
    let re = regex::Regex::new(
        r"(?:^|[\s(,])([a-zA-Z_][a-zA-Z0-9_]*)\)*\s*=\s*(?:'[^']*'|\d+(?:\.\d+)?)"
    )
    .expect("valid extract_column_from_filter regex");
    re.captures(cleaned)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .filter(|col| !is_reserved_type_name(col))
}

/// Extract the content of the innermost parentheses.
///
/// `"outer(inner)"` → `Some("inner")`, `"no parens"` → `None`.
pub fn extract_innermost_parens(s: &str) -> Option<String> {
    let start = s.rfind('(')?;
    let end = s.rfind(')')?;
    if end > start {
        Some(s[start + 1..end].to_string())
    } else {
        None
    }
}

/// Recursively finds the first scan node in a subtree using DFS (left-to-right)
/// and returns its `relation` field.
///
/// Used to extract the underlying table name from wrapper nodes like
/// `SubqueryScan` that may be nested arbitrarily deep.
///
/// Returns `None` when no scan node is found in the subtree.
pub fn find_first_scan_descendant(node: &PlanNode) -> Option<String> {
    if is_scan_node(&node.node_type) {
        return node.relation.clone();
    }
    for child in &node.children {
        if let Some(r) = find_first_scan_descendant(child) {
            return Some(r);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_scan_node() {
        assert!(is_scan_node(&NodeType::SeqScan));
        assert!(is_scan_node(&NodeType::CStoreScan));
        assert!(is_scan_node(&NodeType::PartitionedIndexScan));
        assert!(is_scan_node(&NodeType::BitmapHeapScan));
        assert!(!is_scan_node(&NodeType::Sort));
        assert!(!is_scan_node(&NodeType::HashJoin));
        assert!(!is_scan_node(&NodeType::NestedLoop));
    }

    #[test]
    fn test_is_sort_node() {
        assert!(is_sort_node(&NodeType::Sort));
        assert!(is_sort_node(&NodeType::VectorSort));
        assert!(is_sort_node(&NodeType::GroupSort));
        assert!(!is_sort_node(&NodeType::SeqScan));
        assert!(!is_sort_node(&NodeType::HashJoin));
    }

    #[test]
    fn test_is_dml_node() {
        assert!(is_dml_node(&NodeType::Update));
        assert!(is_dml_node(&NodeType::VectorUpdate));
        assert!(is_dml_node(&NodeType::ModifyTable));
        assert!(is_dml_node(&NodeType::Delete));
        assert!(is_dml_node(&NodeType::Insert));
        assert!(!is_dml_node(&NodeType::SeqScan));
        assert!(!is_dml_node(&NodeType::HashJoin));
    }

    #[test]
    fn test_first_identifier() {
        assert_eq!(first_identifier("employees e"), "employees");
        assert_eq!(first_identifier("orders"), "orders");
        assert_eq!(first_identifier("  trimmed  alias"), "trimmed");
        assert_eq!(first_identifier(""), "");
    }

    #[test]
    fn test_table_name_match() {
        assert!(table_name_match("employees e", "employees"));
        assert!(table_name_match("orders", "orders"));
        assert!(!table_name_match("employees e", "orders"));
        assert!(!table_name_match("orders", "employees"));
    }

    #[test]
    fn test_extract_innermost_parens() {
        assert_eq!(
            extract_innermost_parens("outer(inner)"),
            Some("inner".to_string())
        );
        assert_eq!(extract_innermost_parens("no parens"), None);
        assert_eq!(extract_innermost_parens("(a)(b)"), Some("b".to_string()));
        assert_eq!(extract_innermost_parens(")reverse("), None);
    }

    #[test]
    fn test_extract_column_from_filter_basic() {
        // Simple case: col = 'val'
        assert_eq!(
            extract_column_from_filter("(status)::text = 'ready'::text").unwrap(),
            "status"
        );
    }

    #[test]
    fn test_extract_column_from_filter_with_cast() {
        // Key regression: ::cast should not be mistaken for column name
        // (the core bug in TYPE-001 from 38-case analysis)
        assert_eq!(
            extract_column_from_filter("((facctcode)::text = '1002'::text)").unwrap(),
            "facctcode"
        );
    }

    #[test]
    fn test_extract_column_from_filter_nested_parens() {
        // Nested parentheses wrapping
        assert_eq!(
            extract_column_from_filter("(((amount)::numeric = '100'::numeric))").unwrap(),
            "amount"
        );
    }

    #[test]
    fn test_extract_column_from_filter_no_match() {
        // No = comparison — returns None
        assert!(extract_column_from_filter("col ~~ '%foo'").is_none());
    }

    #[test]
    fn test_extract_column_from_filter_complex_or() {
        // OR chain — takes the first = comparison
        assert_eq!(
            extract_column_from_filter("(a = '1' OR b = '2')").unwrap(),
            "a"
        );
    }

    // ── find_first_scan_descendant tests ──────────────────────────

    fn make_node(nt: NodeType, line: usize, relation: Option<&str>) -> PlanNode {
        PlanNode {
            node_type: nt,
            relation: relation.map(|s| s.to_string()),
            join_type: None,
            estimated: None,
            actual: None,
            properties: vec![],
            structured_props: None,
            buffers: None,
            children: vec![],
            indent_level: 0,
            line_number: line,
        }
    }

    #[test]
    fn test_find_first_scan_descendant_direct_child() {
        // SubqueryScan → SeqScan(table=foo)
        // 返回 Some("foo")
        let mut subquery = make_node(NodeType::SubqueryScan, 1, None);
        let child = make_node(NodeType::SeqScan, 2, Some("foo"));
        subquery.children.push(child);
        assert_eq!(
            find_first_scan_descendant(&subquery),
            Some("foo".to_string())
        );
    }

    #[test]
    fn test_find_first_scan_descendant_nested() {
        // SubqueryScan → Limit → HashJoin (left=SeqScan(table=bar), right=IndexScan(table=baz))
        // DFS 先访问左子树，返回 Some("bar")
        let mut subquery = make_node(NodeType::SubqueryScan, 1, None);
        let mut limit = make_node(NodeType::Limit, 2, None);
        let mut join = make_node(NodeType::HashJoin, 3, None);
        let left = make_node(NodeType::SeqScan, 4, Some("bar"));
        let right = make_node(NodeType::IndexScan, 5, Some("baz"));
        join.children.push(left);
        join.children.push(right);
        limit.children.push(join);
        subquery.children.push(limit);
        assert_eq!(
            find_first_scan_descendant(&subquery),
            Some("bar".to_string())
        );
    }

    #[test]
    fn test_find_first_scan_descendant_no_scan() {
        // SubqueryScan → Sort → Result (无 scan)
        let mut subquery = make_node(NodeType::SubqueryScan, 1, None);
        let mut sort = make_node(NodeType::Sort, 2, None);
        let result = make_node(NodeType::Result, 3, None);
        sort.children.push(result);
        subquery.children.push(sort);
        assert_eq!(find_first_scan_descendant(&subquery), None);
    }

    #[test]
    fn test_find_first_scan_descendant_self_is_scan() {
        // 节点本身是 scan 节点
        let scan = make_node(NodeType::SeqScan, 1, Some("orders"));
        assert_eq!(
            find_first_scan_descendant(&scan),
            Some("orders".to_string())
        );
    }
}
