use crate::model::{NodeType, PlanNode};

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::{make_finding, DiagnosticRule};

pub struct DuplicateSort;

impl DiagnosticRule for DuplicateSort {
    fn id(&self) -> &str {
        "SORT-003"
    }
    fn name(&self) -> &str {
        "Duplicate sort"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::SortEfficiency
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if node.node_type != NodeType::Sort {
            return None;
        }
        let has_sort_child = node.children.iter().any(|child| {
            child.node_type == NodeType::Sort
                || child.node_type == NodeType::VectorSort
                || child.node_type == NodeType::GroupSort
        });
        if !has_sort_child {
            return None;
        }
        Some(make_finding(
            self,
            "Sort node has a Sort child — redundant sorting detected".to_string(),
            node,
            Some(
                "Remove the inner Sort by adjusting ORDER BY or adding appropriate indexes"
                    .to_string(),
            ),
        ))
    }
}
