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
}
