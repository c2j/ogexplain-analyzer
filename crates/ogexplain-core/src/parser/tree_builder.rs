use std::str::FromStr;

use crate::model::buffer::NodeProperty;
use crate::model::cost::{ActualStats, EstimatedCost};
use crate::model::join_type::JoinType;
use crate::model::node_type::NodeType;
use crate::model::plan::{ExplainPlan, NodeProperties, PlanNode, PlanSummary};

use super::line_classifier::{ActualInfo, ClassifiedLine};
use super::ParseError;

pub fn build_tree(classified: &[ClassifiedLine]) -> Result<ExplainPlan, ParseError> {
    let mut nodes: Vec<(usize, PlanNode)> = Vec::new();
    let mut summary = PlanSummary::default();
    let mut has_summary = false;
    let mut last_node_idx: Option<usize> = None;

    for item in classified {
        match item {
            ClassifiedLine::Node(nl) => {
                let (node_type, relation, join_type) = parse_node_text(&nl.raw_node_text);

                let estimated = nl.cost.as_ref().map(|c| EstimatedCost {
                    startup_cost: c.startup_cost,
                    total_cost: c.total_cost,
                    plan_rows: c.plan_rows,
                    plan_width: c.plan_width,
                    pred_time: c.pred_time,
                    pred_rows: c.pred_rows,
                    distinct: None,
                });

                let actual = nl.actual.as_ref().map(|a: &ActualInfo| ActualStats {
                    startup_time_ms: a.startup_time_ms.unwrap_or(0.0),
                    total_time_ms: a.total_time_ms.unwrap_or(0.0),
                    rows: a.rows,
                    loops: a.loops,
                    executed: a.executed,
                });

                let node = PlanNode {
                    node_type,
                    relation,
                    join_type,
                    estimated,
                    actual,
                    properties: Vec::new(),
                    structured_props: None,
                    buffers: None,
                    children: Vec::new(),
                    indent_level: nl.indent_level,
                    line_number: nl.line_number,
                };

                nodes.push((nl.indent, node));
                last_node_idx = Some(nodes.len() - 1);
            }
            ClassifiedLine::Property(pl) => {
                if let Some(idx) = last_node_idx {
                    nodes[idx].1.properties.push(NodeProperty {
                        label: pl.label.clone(),
                        value: pl.value.clone(),
                    });
                    nodes[idx].1.structured_props =
                        NodeProperties::extract(&nodes[idx].1.properties);
                }
            }
            ClassifiedLine::Summary(sl) => {
                has_summary = true;
                for (label, value) in &sl.entries {
                    apply_summary_entry(&mut summary, label, value);
                }
            }
            _ => {}
        }
    }

    if nodes.is_empty() {
        return Err(ParseError::NoPlanNodes);
    }

    let root = assemble_tree(nodes)?;

    Ok(ExplainPlan {
        root,
        summary: if has_summary { Some(summary) } else { None },
    })
}

fn assemble_tree(nodes: Vec<(usize, PlanNode)>) -> Result<PlanNode, ParseError> {
    if nodes.is_empty() {
        return Err(ParseError::NoPlanNodes);
    }

    let root = nodes[0].1.clone();
    let mut stack: Vec<(usize, PlanNode)> = vec![(nodes[0].0, root)];

    for (indent, node) in nodes.iter().skip(1) {
        while stack.len() > 1
            && stack
                .last()
                .map(|(ind, _)| *ind >= *indent)
                .unwrap_or(false)
        {
            let (_, finished) = stack.pop().unwrap();
            if let Some(parent) = stack.last_mut() {
                parent.1.children.push(finished);
            }
        }

        stack.push((*indent, node.clone()));
    }

    while stack.len() > 1 {
        let (_, finished) = stack.pop().unwrap();
        if let Some(parent) = stack.last_mut() {
            parent.1.children.push(finished);
        }
    }

    Ok(stack.pop().unwrap().1)
}

fn parse_node_text(text: &str) -> (NodeType, Option<String>, Option<JoinType>) {
    let text = text.trim();

    let text = strip_pretty_prefix(text);

    if let Some(result) = try_parse_streaming(text) {
        return result;
    }

    let (node_type_name, relation, join_type) = extract_node_parts(text);
    let node_type = NodeType::from_str(&node_type_name)
        .unwrap_or_else(|_| NodeType::Unknown(node_type_name.clone()));
    (node_type, relation, join_type)
}

fn strip_pretty_prefix(text: &str) -> &str {
    let text = text.trim_start();
    let rest = text.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = rest.trim_start_matches(['-', ' ']);
    rest
}

fn try_parse_streaming(text: &str) -> Option<(NodeType, Option<String>, Option<JoinType>)> {
    if text.starts_with("Streaming") {
        let stype = extract_streaming_type_from_text(text)?;
        return Some((NodeType::Streaming(stype), None, None));
    }
    if text.starts_with("Vector Streaming") {
        let rest = text.trim_start_matches("Vector Streaming");
        let stype = extract_streaming_type_from_text(rest)?;
        return Some((NodeType::VectorStreaming(stype), None, None));
    }
    None
}

fn extract_streaming_type_from_text(text: &str) -> Option<crate::model::StreamingType> {
    let start = text.find('(')?;
    let end = text.find(')')?;
    let inner = &text[start + 1..end];
    let type_str = inner
        .strip_prefix("type: ")
        .or_else(|| inner.strip_prefix("type:"))?;
    type_str.trim().parse().ok()
}

fn extract_node_parts(text: &str) -> (String, Option<String>, Option<JoinType>) {
    let text = strip_using_clause(text);

    let join_types = [
        "Left Anti Full",
        "Right Anti Full",
        "Left Anti Semi Not In",
        "Left",
        "Right",
        "Full",
        "Inner",
        "Semi",
        "Anti",
        "Right Semi",
        "Right Anti",
    ];

    for jt in &join_types {
        let pattern = format!(" {} ", jt);
        if let Some(pos) = text.find(&pattern) {
            let node_name = text[..pos].trim();
            let rest = text[pos + pattern.len()..].trim();
            let relation = extract_relation_from_rest(rest);
            let join_type = JoinType::from_str(jt).ok();
            return (node_name.to_string(), relation, join_type);
        }
        let suffix = format!(" {}", jt);
        if text.ends_with(&suffix) {
            let node_name = text[..text.len() - suffix.len()].trim();
            let join_type = JoinType::from_str(jt).ok();
            return (node_name.to_string(), None, join_type);
        }
    }

    if let Some(pos) = text.find(" on ") {
        let node_name = text[..pos].trim();
        let rest = &text[pos + 4..];
        let rel = rest
            .split_whitespace()
            .next()
            .unwrap_or(rest)
            .trim()
            .to_string();
        if !rel.is_empty() {
            return (node_name.to_string(), Some(rel), None);
        }
    }

    (text.to_string(), None, None)
}

fn strip_using_clause(text: &str) -> String {
    if let Some(pos) = text.find(" using ") {
        let node_type_part = &text[..pos];
        let after_using = &text[pos + 7..];
        let idx_name = after_using.split_whitespace().next().unwrap_or("");
        let remaining = &after_using[idx_name.len()..];
        format!("{}{}", node_type_part, remaining)
    } else {
        text.to_string()
    }
}

fn extract_relation_from_rest(rest: &str) -> Option<String> {
    if let Some(pos) = rest.find(" on ") {
        let rel = rest[pos + 4..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !rel.is_empty() {
            return Some(rel);
        }
    }
    None
}

fn apply_summary_entry(summary: &mut PlanSummary, label: &str, value: &str) {
    match label {
        "Total runtime" | "Total Runtime" => {
            summary.total_runtime_ms = parse_f64(value);
        }
        "Peak Memory" => {
            summary.peak_memory_kb = parse_memory(value);
        }
        "Planner runtime" | "Planner Runtime" => {
            summary.planner_runtime_ms = parse_f64(value);
        }
        "Plan size" | "Plan Size" => {
            summary.plan_size_bytes = parse_bytes(value);
        }
        "Query Id" | "Query ID" => {
            summary.query_id = Some(value.to_string());
        }
        "Executor Start" => {
            summary.executor_start_ms = parse_f64(value);
        }
        "Executor Run" => {
            summary.executor_run_ms = parse_f64(value);
        }
        "Executor End" => {
            summary.executor_end_ms = parse_f64(value);
        }
        "Total Network" => {
            summary.total_network_kb = parse_memory(value);
        }
        _ => {}
    }
}

fn parse_f64(value: &str) -> Option<f64> {
    let num_str = value.split_whitespace().next()?;
    num_str.parse().ok()
}

fn parse_memory(value: &str) -> Option<i64> {
    let num_str = value.split_whitespace().next()?;
    num_str.parse().ok()
}

fn parse_bytes(value: &str) -> Option<i64> {
    let num_str = value.split_whitespace().next()?;
    num_str.parse().ok()
}
