use crate::model::{NodeType, PlanNode};

use super::super::config::DiagnosticConfig;
use super::super::context::{GlobalStats, PlanContext};
use super::super::report::{DiagnosticCategory, Finding, Severity};
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
        if node.node_type != NodeType::Sort {
            return None;
        }
        let sort_method_prop = node.properties.iter().find(|p| p.label == "Sort Method")?;
        let value = &sort_method_prop.value;
        if !value.contains("external") {
            return None;
        }
        let disk_used = extract_disk_size(value).unwrap_or_else(|| "unknown".to_string());
        Some(make_finding(
            self,
            format!("Sort Method: {}", value),
            node,
            Some(format!(
                "Increase work_mem to avoid disk spill ({} on disk)",
                disk_used
            )),
        ))
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
        vec![Finding {
            rule_id: self.id().to_string(),
            severity: self.severity(),
            category: self.category(),
            title: self.name().to_string(),
            detail: format!("Peak memory: {}kB (threshold: {}kB)", peak, self.threshold),
            node_line: None,
            node_type: None,
            suggestion: Some(
                "Consider reducing work_mem or optimizing the query to use less memory".to_string(),
            ),
            sql_rewrite: None,
        }]
    }
}

fn extract_disk_size(value: &str) -> Option<String> {
    for (i, part) in value.split_whitespace().enumerate() {
        if part == "Disk:" {
            let size = value.split_whitespace().nth(i + 1)?;
            return Some(size.to_string());
        }
    }
    for part in value.split_whitespace() {
        if part.ends_with("kB") || part.ends_with("MB") || part.ends_with("GB") {
            return Some(part.to_string());
        }
    }
    None
}
