use crate::model::PlanNode;

use super::super::config::DiagnosticConfig;
use super::super::context::{GlobalStats, PlanContext};
use super::super::report::{DiagnosticCategory, Finding, Severity};
use super::utils::{get_property_value, is_sort_node};
use super::{make_finding, DiagnosticRule};

pub struct SortSpillToDisk;

impl DiagnosticRule for SortSpillToDisk {
    fn id(&self) -> &str {
        "MEM-001"
    }
    fn name(&self) -> &str {
        "Sort spilled to disk"
    }
    fn severity(&self) -> Severity {
        Severity::Critical
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::MemoryUsage
    }
    fn check(&self, node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        if !is_sort_node(&node.node_type) {
            return None;
        }
        let sort_method_prop = node.properties.iter().find(|p| p.label == "Sort Method")?;
        let value = &sort_method_prop.value;
        if !value.contains("external") {
            return None;
        }
        let disk_used = extract_disk_size(value).unwrap_or_else(|| "unknown".to_string());
        let sort_key = get_property_value(node, "Sort Key").map(|s| s.to_string());

        let mut detail = format!("Sort Method: {}", value);
        if let Some(ref key) = sort_key {
            detail.push_str(&format!(", Sort Key: {}", key));
        }

        let suggestion = format!(
            "SET work_mem = '更高值'; 排序溢出到磁盘({}), 考虑在排序列创建索引以消除排序",
            disk_used
        );

        Some(make_finding(self, detail, node, Some(suggestion)))
    }
}

pub struct HighPeakMemory {
    threshold: f64,
}

impl HighPeakMemory {
    pub fn new(config: DiagnosticConfig) -> Self {
        Self {
            threshold: config.memory_threshold_kb,
        }
    }
}

impl DiagnosticRule for HighPeakMemory {
    fn id(&self) -> &str {
        "MEM-004"
    }
    fn name(&self) -> &str {
        "High peak memory"
    }
    fn severity(&self) -> Severity {
        Severity::Warning
    }
    fn category(&self) -> DiagnosticCategory {
        DiagnosticCategory::MemoryUsage
    }
    fn check(&self, _node: &PlanNode, _ctx: &PlanContext) -> Option<Finding> {
        None
    }
    fn check_global(&self, plan: &crate::model::ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        let summary = match &plan.summary {
            Some(s) => s,
            None => return Vec::new(),
        };
        let peak = match summary.peak_memory_kb {
            Some(v) => v as f64,
            None => return Vec::new(),
        };
        if peak <= self.threshold {
            return Vec::new();
        }

        let top_node = find_highest_memory_node(&plan.root);
        let mut detail = format!("Peak memory: {}kB (threshold: {}kB)", peak, self.threshold);
        if let Some((node_type, mem_kb, relation)) = top_node {
            detail.push_str(&format!(
                ", 最高内存节点: {} on {} ({}kB)",
                node_type,
                relation.as_deref().unwrap_or("unknown"),
                mem_kb
            ));
        }

        let suggestion =
            "分析高内存节点; Sort/Hash → 增加 work_mem; Materialize → 优化查询减少中间结果集"
                .to_string();

        vec![Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail,
            node_line: None,
            node_type: None,
            suggestion: Some(suggestion),
            sql_rewrite: None,
        }]
    }
}

fn find_highest_memory_node(node: &PlanNode) -> Option<(String, i64, Option<String>)> {
    let mut result: Option<(String, i64, Option<String>)> = None;
    find_highest_recursive(node, &mut result);
    result
}

fn find_highest_recursive(node: &PlanNode, best: &mut Option<(String, i64, Option<String>)>) {
    if let Some(mem_str) = get_property_value(node, "Memory Usage") {
        if let Some(mem_kb) = parse_memory_value(mem_str) {
            if best.as_ref().is_none_or(|b| mem_kb > b.1) {
                *best = Some((node.node_type.to_string(), mem_kb, node.relation.clone()));
            }
        }
    }
    for child in &node.children {
        find_highest_recursive(child, best);
    }
}

fn parse_memory_value(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(kb) = s.strip_suffix("kB") {
        return kb.trim().parse::<i64>().ok();
    }
    if let Some(mb) = s.strip_suffix("MB") {
        return mb.trim().parse::<f64>().ok().map(|v| (v * 1024.0) as i64);
    }
    s.parse::<i64>().ok()
}

fn extract_disk_size(value: &str) -> Option<String> {
    for (i, part) in value.split_whitespace().enumerate() {
        if part == "Disk:" {
            return value.split_whitespace().nth(i + 1).map(|s| s.to_string());
        }
    }
    for part in value.split_whitespace() {
        if part.ends_with("kB") || part.ends_with("MB") || part.ends_with("GB") {
            return Some(part.to_string());
        }
    }
    None
}
