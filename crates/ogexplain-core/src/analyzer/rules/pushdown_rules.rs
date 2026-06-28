use crate::model::{NodeType, PlanNode, StreamingType};
use rust_i18n::t;

use super::super::context::PlanContext;
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::any_property_contains;
use super::{make_finding, DiagnosticRule};

pub struct QueryNotPushedDown;

impl DiagnosticRule for QueryNotPushedDown {
    fn id(&self) -> &str {
        "PUSH-001"
    }
    fn name(&self) -> String {
        t!("finding.PUSH-001.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::PushdownFailure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        let streaming_type = match &node.node_type {
            NodeType::Streaming(StreamingType::Redistribute) => "Redistribute",
            NodeType::Streaming(StreamingType::Broadcast) => "Broadcast",
            _ => return None,
        };

        let (reasons, has_subquery, has_volatile) = collect_pushdown_blockers(node);

        let mut detail = t!("finding.PUSH-001.detail", streaming_type = streaming_type).to_string();
        if !reasons.is_empty() {
            detail.push_str(&t!(
                "finding.PUSH-001.detail_reasons",
                reasons = reasons.join(", ")
            ));
        }

        let suggestion = if has_subquery {
            t!("finding.PUSH-001.suggestion_subquery").to_string()
        } else if has_volatile {
            t!("finding.PUSH-001.suggestion_volatile").to_string()
        } else {
            t!("finding.PUSH-001.suggestion_default").to_string()
        };

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

fn collect_pushdown_blockers(node: &PlanNode) -> (Vec<String>, bool, bool) {
    let mut blockers = Vec::new();
    let mut has_subquery = false;
    let mut has_volatile = false;
    collect_blockers_recursive(node, &mut blockers, &mut has_subquery, &mut has_volatile);
    (blockers, has_subquery, has_volatile)
}

fn collect_blockers_recursive(
    node: &PlanNode,
    blockers: &mut Vec<String>,
    has_subquery: &mut bool,
    has_volatile: &mut bool,
) {
    if matches!(
        node.node_type,
        NodeType::SubqueryScan | NodeType::VectorSubqueryScan
    ) {
        blockers.push(t!("finding.PUSH-001.blocker_subquery").to_string());
        *has_subquery = true;
    }
    if matches!(node.node_type, NodeType::Result | NodeType::VectorResult)
        && any_property_contains(node, "SubPlan")
    {
        blockers.push(t!("finding.PUSH-001.blocker_subplan").to_string());
    }
    if let Some(filter) = node
        .properties
        .iter()
        .find(|p| p.label == "Filter")
        .map(|p| p.value.as_str())
    {
        if filter.contains("now()") || filter.contains("random()") || filter.contains("nextval") {
            blockers.push(t!("finding.PUSH-001.blocker_volatile").to_string());
            *has_volatile = true;
        }
    }
    for child in &node.children {
        collect_blockers_recursive(child, blockers, has_subquery, has_volatile);
    }
}

pub struct MultiLayerStreaming;

impl DiagnosticRule for MultiLayerStreaming {
    fn id(&self) -> &str {
        "PUSH-002"
    }
    fn name(&self) -> String {
        t!("finding.PUSH-002.name").to_string()
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::PushdownFailure
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if !matches!(
            &node.node_type,
            NodeType::Streaming(_) | NodeType::VectorStreaming(_)
        ) {
            return None;
        }

        let mut layers: Vec<String> = Vec::new();
        collect_streaming_layers(&node.children, &mut layers);

        if layers.is_empty() {
            return None;
        }

        let current_type = streaming_type_name(&node.node_type);
        let total_layers = layers.len() + 1;

        let detail = t!(
            "finding.PUSH-002.detail",
            layers = total_layers - 1,
            current = current_type,
            chain = layers.join(" → ")
        )
        .to_string();

        let suggestion = if total_layers >= 3 {
            t!("finding.PUSH-002.suggestion_deep")
        } else {
            t!("finding.PUSH-002.suggestion_shallow")
        };

        Some(make_finding(
            self,
            detail,
            node,
            Some(suggestion.to_string()),
        ))
    }
}

fn collect_streaming_layers(children: &[PlanNode], layers: &mut Vec<String>) {
    for child in children {
        if matches!(
            &child.node_type,
            NodeType::Streaming(_) | NodeType::VectorStreaming(_)
        ) {
            layers.push(streaming_type_name(&child.node_type));
        }
        collect_streaming_layers(&child.children, layers);
    }
}

fn streaming_type_name(nt: &NodeType) -> String {
    match nt {
        NodeType::Streaming(st) => format!("Streaming({:?})", st),
        NodeType::VectorStreaming(st) => format!("VectorStreaming({:?})", st),
        _ => nt.to_string(),
    }
}
