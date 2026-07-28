use std::collections::HashMap;

use super::fingerprint::plan_fingerprint;
use super::types::{PlanEntry, TemplateGroup};

/// Group a sequence of plan entries by SQL template (plan fingerprint).
///
/// Returns groups sorted by cumulative time descending. Groups with only a
/// single occurrence are excluded (they are not "repeated" queries).
pub(crate) fn group_by_template(entries: &[PlanEntry]) -> Vec<TemplateGroup> {
    let mut groups: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let fp = plan_fingerprint(&entry.plan);
        groups.entry(fp).or_default().push(i);
    }

    let mut result: Vec<TemplateGroup> = groups
        .into_iter()
        .filter(|(_, indices)| indices.len() > 1)
        .map(|(fp, indices)| build_template_group(fp, indices, entries))
        .collect();

    result.sort_by(|a, b| f64::total_cmp(&b.cum_time_ms, &a.cum_time_ms));
    result
}

fn build_template_group(fp: u64, indices: Vec<usize>, entries: &[PlanEntry]) -> TemplateGroup {
    let runtimes: Vec<f64> = indices.iter().map(|&i| entries[i].runtime_ms).collect();
    let spills: Vec<f64> = indices.iter().map(|&i| entries[i].spill_kb).collect();
    let reads: Vec<i64> = indices.iter().map(|&i| entries[i].buffer_read).collect();

    let count = indices.len();
    let cum_time_ms: f64 = runtimes.iter().sum();
    let avg_time_ms = cum_time_ms / count as f64;
    let min_time_ms = runtimes.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_time_ms = runtimes.iter().cloned().fold(0.0_f64, f64::max);
    let cum_spill_kb: f64 = spills.iter().sum();
    let cum_buffer_read: i64 = reads.iter().sum();

    let degradation_ratio = if avg_time_ms > 0.0 {
        max_time_ms / avg_time_ms
    } else {
        1.0
    };

    let first = &entries[indices[0]];

    TemplateGroup {
        fingerprint: fp,
        sample_sql: first.query_text.clone(),
        count,
        cum_time_ms,
        avg_time_ms,
        min_time_ms,
        max_time_ms,
        cum_spill_kb,
        cum_buffer_read,
        degradation_ratio,
        root_op: format!("{}", first.plan.root.node_type),
        diagnostic: first.report.clone(),
    }
}
