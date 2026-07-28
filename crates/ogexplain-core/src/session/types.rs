use crate::analyzer::report::DiagnosticReport;
use serde::Serialize;

/// Result of analyzing a sequence of EXPLAIN plans from one session
/// (e.g. all auto_explain entries from a stored procedure execution).
#[derive(Debug, Clone, Serialize)]
pub struct SessionAnalysis {
    pub total_entries: usize,
    pub total_time_ms: f64,
    pub serial_bottlenecks: Vec<SerialBottleneck>,
    pub template_groups: Vec<TemplateGroup>,
}

/// A single step in a sequential SQL execution pipeline, flagged as a bottleneck.
#[derive(Debug, Clone, Serialize)]
pub struct SerialBottleneck {
    pub step_index: usize,
    pub query_text: String,
    pub runtime_ms: f64,
    pub contribution_pct: f64,
    pub bottleneck_kind: BottleneckKind,
    pub diagnostic: DiagnosticReport,
}

/// A group of plans sharing the same SQL template (repeated queries, e.g. inside a loop).
#[derive(Debug, Clone, Serialize)]
pub struct TemplateGroup {
    pub fingerprint: u64,
    pub sample_sql: String,
    pub count: usize,
    pub cum_time_ms: f64,
    pub avg_time_ms: f64,
    pub min_time_ms: f64,
    pub max_time_ms: f64,
    pub cum_spill_kb: f64,
    pub cum_buffer_read: i64,
    pub degradation_ratio: f64,
    pub root_op: String,
    pub diagnostic: DiagnosticReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BottleneckKind {
    Primary,
    Secondary,
    None,
}

/// Internal entry used during analysis — pairs a query text with its parsed plan
/// and per-plan diagnostic report.
#[derive(Debug, Clone)]
pub(crate) struct PlanEntry {
    pub query_text: String,
    pub runtime_ms: f64,
    pub spill_kb: f64,
    pub buffer_read: i64,
    pub plan: crate::model::ExplainPlan,
    pub report: DiagnosticReport,
}
