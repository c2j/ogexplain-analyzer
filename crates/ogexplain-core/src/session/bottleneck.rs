use super::types::{BottleneckKind, PlanEntry, SerialBottleneck};

/// Identify serial bottlenecks in a sequence of plan entries.
///
/// A step is flagged as:
/// - `Primary` if it contributes more than 50% of the total session runtime.
/// - `Secondary` if its runtime exceeds mean + 2× standard deviation
///   (only when stddev > 0, so uniform workload produces no Secondary).
///
/// A single-entry session is flagged `Primary` — it is the sole cost driver.
pub(crate) fn detect_serial_bottlenecks(entries: &[PlanEntry]) -> Vec<SerialBottleneck> {
    if entries.is_empty() {
        return vec![];
    }

    let total: f64 = entries.iter().map(|e| e.runtime_ms).sum();
    if total <= 0.0 {
        return entries
            .iter()
            .enumerate()
            .map(|(i, e)| SerialBottleneck {
                step_index: i,
                query_text: e.query_text.clone(),
                runtime_ms: e.runtime_ms,
                contribution_pct: 0.0,
                bottleneck_kind: BottleneckKind::None,
                diagnostic: e.report.clone(),
            })
            .collect();
    }

    let mean = total / entries.len() as f64;
    let variance = entries
        .iter()
        .map(|e| (e.runtime_ms - mean).powi(2))
        .sum::<f64>()
        / entries.len() as f64;
    let stddev = variance.sqrt();

    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let contribution = e.runtime_ms / total * 100.0;
            let kind = if contribution > 50.0 {
                BottleneckKind::Primary
            } else if stddev > 0.0 && e.runtime_ms > mean + 2.0 * stddev {
                BottleneckKind::Secondary
            } else {
                BottleneckKind::None
            };
            SerialBottleneck {
                step_index: i,
                query_text: e.query_text.clone(),
                runtime_ms: e.runtime_ms,
                contribution_pct: contribution,
                bottleneck_kind: kind,
                diagnostic: e.report.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::report::DiagnosticReport;
    use crate::model::{ExplainPlan, NodeType, PlanNode, PlanSummary};

    fn make_entry(query: &str, ms: f64) -> PlanEntry {
        PlanEntry {
            query_text: query.to_string(),
            runtime_ms: ms,
            spill_kb: 0.0,
            buffer_read: 0,
            plan: ExplainPlan {
                root: PlanNode {
                    node_type: NodeType::SeqScan,
                    relation: Some("t".to_string()),
                    join_type: None,
                    estimated: None,
                    actual: None,
                    properties: vec![],
                    structured_props: None,
                    buffers: None,
                    children: vec![],
                    indent_level: 0,
                    line_number: 1,
                },
                summary: Some(PlanSummary {
                    total_runtime_ms: Some(ms),
                    ..Default::default()
                }),
            },
            report: DiagnosticReport::empty(),
        }
    }

    #[test]
    fn single_entry_is_primary() {
        let entries = vec![make_entry("SELECT 1", 5.0)];
        let result = detect_serial_bottlenecks(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].bottleneck_kind, BottleneckKind::Primary);
        assert_eq!(result[0].contribution_pct, 100.0);
    }

    #[test]
    fn dominating_entry_primary_bottleneck() {
        let entries = vec![
            make_entry("step 1", 1.0),
            make_entry("step 2", 1.0),
            make_entry("step 3", 50.0),
            make_entry("step 4", 1.0),
        ];
        let result = detect_serial_bottlenecks(&entries);
        assert_eq!(result[2].bottleneck_kind, BottleneckKind::Primary);
        assert!((result[2].contribution_pct - 94.34).abs() < 0.1);
    }

    #[test]
    fn outlier_secondary_bottleneck() {
        // 10 equal steps of 5ms + one outlier of 15ms
        // outlier = 15/65 ≈ 23% (< 50% so not Primary)
        // mean ≈ 5.9, stddev ≈ 2.9, mean+2σ ≈ 11.7 → 15 > 11.7 → Secondary
        let mut entries: Vec<_> = (0..10)
            .map(|i| make_entry(&format!("step {i}"), 5.0))
            .collect();
        entries.push(make_entry("outlier", 15.0));
        let result = detect_serial_bottlenecks(&entries);
        assert_eq!(result[10].bottleneck_kind, BottleneckKind::Secondary);
        assert!(result[10].contribution_pct < 50.0);
    }

    #[test]
    fn empty_entries_no_bottleneck() {
        let result = detect_serial_bottlenecks(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn zero_runtime_entries() {
        let entries = vec![make_entry("step 1", 0.0), make_entry("step 2", 0.0)];
        let result = detect_serial_bottlenecks(&entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].bottleneck_kind, BottleneckKind::None);
    }
}
