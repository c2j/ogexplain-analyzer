pub mod types;

mod aggregator;
mod bottleneck;
mod fingerprint;

use crate::analyzer::config::DiagnosticConfig;
use crate::model::ExplainPlan;

pub use types::{BottleneckKind, SerialBottleneck, SessionAnalysis, TemplateGroup};

use types::PlanEntry;

/// Analyze a sequence of EXPLAIN plans from one session (e.g. all auto_explain
/// NOTICE entries from a stored procedure execution).
///
/// Returns per-step serial bottleneck rankings and template-grouped loop
/// hotspot analysis. Each plan is independently diagnosed with the full set of
/// diagnostic rules.
///
/// # Example
///
/// ```ignore
/// use ogexplain_core::session::analyze_session;
/// use ogexplain_core::analyzer::config::DiagnosticConfig;
///
/// let plans = vec![
///     ("SELECT * FROM t1".to_string(), plan1),
///     ("SELECT * FROM t1".to_string(), plan2), // repeated — will be grouped
///     ("SELECT COUNT(*) FROM t2".to_string(), plan3),
/// ];
/// let session = analyze_session(&plans, &DiagnosticConfig::default());
///
/// for group in &session.template_groups {
///     println!("Template '{}' executed {} times, cumulative {} ms",
///              group.sample_sql, group.count, group.cum_time_ms);
/// }
/// for b in &session.serial_bottlenecks {
///     if b.bottleneck_kind != BottleneckKind::None {
///         println!("Step {} is a {:?} bottleneck ({:.1}%)",
///                  b.step_index, b.bottleneck_kind, b.contribution_pct);
///     }
/// }
/// ```
pub fn analyze_session(
    entries: &[(String, ExplainPlan)],
    config: &DiagnosticConfig,
) -> SessionAnalysis {
    let engine = crate::analyzer::DiagnosticEngine::new(config.clone());

    let plan_entries: Vec<PlanEntry> = entries
        .iter()
        .map(|(query, plan)| {
            let report = engine.analyze(plan);
            let runtime = extract_runtime(plan);
            let spill = compute_spill_kb(plan);
            let buf_read = compute_buffer_read(plan);

            PlanEntry {
                query_text: query.clone(),
                runtime_ms: runtime,
                spill_kb: spill,
                buffer_read: buf_read,
                plan: plan.clone(),
                report,
            }
        })
        .collect();

    let total_time_ms: f64 = plan_entries.iter().map(|e| e.runtime_ms).sum();
    let serial_bottlenecks = bottleneck::detect_serial_bottlenecks(&plan_entries);
    let template_groups = aggregator::group_by_template(&plan_entries);

    SessionAnalysis {
        total_entries: plan_entries.len(),
        total_time_ms,
        serial_bottlenecks,
        template_groups,
    }
}

fn extract_runtime(plan: &ExplainPlan) -> f64 {
    plan.summary
        .as_ref()
        .and_then(|s| s.total_runtime_ms)
        .unwrap_or_else(|| {
            plan.root
                .actual
                .as_ref()
                .map(|a| a.total_time_ms)
                .unwrap_or(0.0)
        })
}

fn compute_spill_kb(plan: &ExplainPlan) -> f64 {
    let mut total_spill: f64 = 0.0;
    fn walk(node: &crate::model::PlanNode, spill: &mut f64) {
        if let Some(props) = &node.structured_props {
            if let Some(disk) = &props.sort_disk {
                if let Ok(kb) = disk.trim().trim_end_matches("kB").parse::<f64>() {
                    *spill += kb;
                }
            }
        }
        for child in &node.children {
            walk(child, spill);
        }
    }
    walk(&plan.root, &mut total_spill);
    total_spill
}

fn compute_buffer_read(plan: &ExplainPlan) -> i64 {
    fn collect_read(node: &crate::model::PlanNode) -> i64 {
        let self_read = node.buffers.as_ref().map(|b| b.shared_read).unwrap_or(0);
        self_read + node.children.iter().map(collect_read).sum::<i64>()
    }
    collect_read(&plan.root)
}
