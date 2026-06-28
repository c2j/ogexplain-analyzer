use crate::model::PlanNode;
use rust_i18n::t;

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::{get_property_value, is_sort_node};
use super::{make_finding, DiagnosticRule};

pub struct DuplicateSort;

impl DiagnosticRule for DuplicateSort {
    fn id(&self) -> &str {
        "SORT-003"
    }
    fn name(&self) -> String {
        t!("finding.SORT-003.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SortEfficiency
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if !is_sort_node(&node.node_type) {
            return None;
        }

        let current_key = get_property_value(node, "Sort Key")
            .unwrap_or("")
            .to_string();

        let mut child_sorts: Vec<(String, String)> = Vec::new();
        collect_child_sort_keys(node, &mut child_sorts);

        let has_direct_sort_child = node.children.iter().any(|c| is_sort_node(&c.node_type));

        if child_sorts.is_empty() && !has_direct_sort_child {
            return None;
        }

        let duplicates: Vec<&str> = child_sorts
            .iter()
            .filter(|(_, key)| !key.is_empty() && key == &current_key)
            .map(|(nt, _)| nt.as_str())
            .collect();

        let detail = if !duplicates.is_empty() && !current_key.is_empty() {
            t!(
                "finding.SORT-003.detail_duplicate",
                key = current_key,
                duplicates = duplicates.join(", ")
            )
            .to_string()
        } else {
            t!("finding.SORT-003.detail_redundant").to_string()
        };

        let suggestion = if !current_key.is_empty() {
            t!("finding.SORT-003.suggestion_with_key", key = current_key).to_string()
        } else {
            t!("finding.SORT-003.suggestion_no_key").to_string()
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn collect_child_sort_keys(node: &PlanNode, result: &mut Vec<(String, String)>) {
    for child in &node.children {
        if is_sort_node(&child.node_type) {
            let key = get_property_value(child, "Sort Key")
                .unwrap_or("")
                .to_string();
            result.push((child.node_type.to_string(), key));
        }
        collect_child_sort_keys(child, result);
    }
}
