use serde::Serialize;

use crate::analyzer::report::{DiagnosticReport, Severity};
use crate::model::PlanNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PushdownStatus {
    Pushed,
    NotPushed,
    Local,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ComplexityInput {
    pub sql_preview: Option<String>,
    pub template_id: Option<String>,
    pub tables: usize,
    pub joins: usize,
    pub subqueries: usize,
    pub where_conditions: usize,
    pub aggregates: usize,
    pub cases: usize,
    pub set_ops: usize,
    pub ctes: usize,
    pub windows: usize,
    pub has_group_by: bool,
    pub has_order_by: bool,
    pub has_distinct: bool,
    pub subquery_depth: usize,
    pub hints: usize,
    pub score: Option<f64>,
    pub level: Option<String>,
    pub gauss_score: Option<i64>,
    pub gauss_level: Option<String>,
    pub sql_category: Option<String>,
    pub sql_sub_type: Option<String>,
    pub gauss_sql_structure: Option<i64>,
    pub gauss_pl_logic: Option<i64>,
    pub gauss_advanced_feature: Option<i64>,
    pub gauss_extension: Option<i64>,
    pub gauss_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryRow {
    pub sql_preview: Option<String>,
    pub tables: usize,
    pub joins: usize,
    pub subqueries: usize,
    pub where_conditions: usize,
    pub aggregates: usize,
    pub cases: usize,
    pub set_ops: usize,
    pub ctes: usize,
    pub windows: usize,
    pub has_group_by: bool,
    pub has_order_by: bool,
    pub has_distinct: bool,
    pub subquery_depth: usize,
    pub hints: usize,
    pub score: Option<f64>,
    pub level: Option<String>,
    pub gauss_score: Option<i64>,
    pub gauss_level: Option<String>,
    pub sql_category: Option<String>,
    pub sql_sub_type: Option<String>,
    pub gauss_sql_structure: Option<i64>,
    pub gauss_pl_logic: Option<i64>,
    pub gauss_advanced_feature: Option<i64>,
    pub gauss_extension: Option<i64>,
    pub gauss_tags: Vec<String>,
    pub template_id: Option<String>,

    pub root_op: String,
    pub total_cost: f64,
    pub total_time_ms: f64,
    pub actual_rows: Option<f64>,
    pub plan_depth: usize,
    pub node_count: usize,

    pub worst_est_ratio: Option<f64>,
    pub spill_kb: Option<f64>,
    pub peak_memory_kb: Option<f64>,
    pub pushdown: PushdownStatus,

    pub buffer_hit_rate: Option<f64>,
    pub total_temp_read_kb: Option<f64>,
    pub total_temp_written_kb: Option<f64>,
    pub max_filter_removed: Option<f64>,
    pub estimated_rows: Option<f64>,
    pub total_loops: Option<f64>,
    pub network_kb: Option<f64>,
    pub planner_time_ms: Option<f64>,

    pub critical_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
}

impl SummaryRow {
    pub fn compute(
        plan: &crate::model::ExplainPlan,
        diag: &DiagnosticReport,
        complexity: Option<&ComplexityInput>,
    ) -> Self {
        let root = &plan.root;

        let sql_preview = complexity.and_then(|c| c.sql_preview.clone());
        let template_id = complexity.and_then(|c| c.template_id.clone());
        let tables = complexity.map(|c| c.tables).unwrap_or(0);
        let joins = complexity.map(|c| c.joins).unwrap_or(0);
        let subqueries = complexity.map(|c| c.subqueries).unwrap_or(0);
        let where_conditions = complexity.map(|c| c.where_conditions).unwrap_or(0);
        let aggregates = complexity.map(|c| c.aggregates).unwrap_or(0);
        let cases = complexity.map(|c| c.cases).unwrap_or(0);
        let set_ops = complexity.map(|c| c.set_ops).unwrap_or(0);
        let ctes = complexity.map(|c| c.ctes).unwrap_or(0);
        let windows = complexity.map(|c| c.windows).unwrap_or(0);
        let has_group_by = complexity.map(|c| c.has_group_by).unwrap_or(false);
        let has_order_by = complexity.map(|c| c.has_order_by).unwrap_or(false);
        let has_distinct = complexity.map(|c| c.has_distinct).unwrap_or(false);
        let subquery_depth = complexity.map(|c| c.subquery_depth).unwrap_or(0);
        let hints = complexity.map(|c| c.hints).unwrap_or(0);
        let score = complexity.and_then(|c| c.score);
        let level = complexity.and_then(|c| c.level.clone());
        let gauss_score = complexity.and_then(|c| c.gauss_score);
        let gauss_level = complexity.and_then(|c| c.gauss_level.clone());
        let sql_category = complexity.and_then(|c| c.sql_category.clone());
        let sql_sub_type = complexity.and_then(|c| c.sql_sub_type.clone());
        let gauss_sql_structure = complexity.and_then(|c| c.gauss_sql_structure);
        let gauss_pl_logic = complexity.and_then(|c| c.gauss_pl_logic);
        let gauss_advanced_feature = complexity.and_then(|c| c.gauss_advanced_feature);
        let gauss_extension = complexity.and_then(|c| c.gauss_extension);
        let gauss_tags = complexity.map(|c| c.gauss_tags.clone()).unwrap_or_default();

        let root_op = format!("{}", root.node_type);
        let total_cost = root.estimated.as_ref().map(|e| e.total_cost).unwrap_or(0.0);

        let total_time_ms = plan
            .summary
            .as_ref()
            .and_then(|s| s.total_runtime_ms)
            .unwrap_or_else(|| root.actual.as_ref().map(|a| a.total_time_ms).unwrap_or(0.0));

        let actual_rows = root.actual.as_ref().map(|a| a.rows);

        let (worst_est_ratio, spill_kb) = compute_tree_metrics(root);

        let peak_memory_kb = plan
            .summary
            .as_ref()
            .and_then(|s| s.peak_memory_kb.map(|v| v as f64))
            .or_else(|| find_peak_memory(root));

        let pushdown = compute_pushdown_status(root);

        let (buffer_hit_rate, total_temp_read_kb, total_temp_written_kb, max_filter_removed) =
            compute_buffer_and_filter_metrics(root);

        let estimated_rows = root.estimated.as_ref().map(|e| e.plan_rows);
        let total_loops = root.actual.as_ref().map(|a| a.loops);
        let network_kb = plan
            .summary
            .as_ref()
            .and_then(|s| s.total_network_kb.map(|v| v as f64));
        let planner_time_ms = plan.summary.as_ref().and_then(|s| s.planner_runtime_ms);

        let (critical_count, warning_count, info_count) =
            diag.findings
                .iter()
                .fold((0usize, 0usize, 0usize), |(c, w, i), f| match f.severity {
                    Severity::Critical => (c + 1, w, i),
                    Severity::Warning => (c, w + 1, i),
                    Severity::Info => (c, w, i + 1),
                });

        Self {
            sql_preview,
            tables,
            joins,
            subqueries,
            where_conditions,
            aggregates,
            cases,
            set_ops,
            ctes,
            windows,
            has_group_by,
            has_order_by,
            has_distinct,
            subquery_depth,
            hints,
            score,
            level,
            gauss_score,
            gauss_level,
            sql_category,
            sql_sub_type,
            gauss_sql_structure,
            gauss_pl_logic,
            gauss_advanced_feature,
            gauss_extension,
            gauss_tags,
            template_id,
            root_op,
            total_cost,
            total_time_ms,
            actual_rows,
            plan_depth: diag.stats.max_depth,
            node_count: diag.stats.total_nodes,
            worst_est_ratio,
            spill_kb,
            peak_memory_kb,
            pushdown,
            buffer_hit_rate,
            total_temp_read_kb,
            total_temp_written_kb,
            max_filter_removed,
            estimated_rows,
            total_loops,
            network_kb,
            planner_time_ms,
            critical_count,
            warning_count,
            info_count,
        }
    }
}

fn compute_tree_metrics(node: &PlanNode) -> (Option<f64>, Option<f64>) {
    let mut worst_ratio: Option<f64> = None;
    let mut total_spill: f64 = 0.0;

    fn walk(node: &PlanNode, ratio: &mut Option<f64>, spill: &mut f64) {
        if let (Some(est), Some(act)) = (&node.estimated, &node.actual) {
            if est.plan_rows > 0.0 && act.rows > 0.0 {
                let r = act.rows / est.plan_rows;
                if r >= 1.0 {
                    *ratio = Some(ratio.unwrap_or(0.0).max(r));
                }
            }
        }
        if let Some(props) = &node.structured_props {
            if let Some(disk) = &props.sort_disk {
                if let Ok(kb) = disk.trim().trim_end_matches("kB").parse::<f64>() {
                    *spill += kb;
                }
            }
        }
        for child in &node.children {
            walk(child, ratio, spill);
        }
    }

    walk(node, &mut worst_ratio, &mut total_spill);
    (
        worst_ratio,
        if total_spill > 0.0 {
            Some(total_spill)
        } else {
            None
        },
    )
}

fn find_peak_memory(node: &PlanNode) -> Option<f64> {
    node.structured_props
        .as_ref()
        .and_then(|p| p.peak_memory_kb)
        .or_else(|| node.children.iter().filter_map(find_peak_memory).next())
}

fn compute_buffer_and_filter_metrics(
    node: &PlanNode,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let mut shared_hit: i64 = 0;
    let mut shared_read: i64 = 0;
    let mut temp_read: i64 = 0;
    let mut temp_written: i64 = 0;
    let mut max_filter: f64 = 0.0;

    fn walk(
        node: &PlanNode,
        hit: &mut i64,
        read: &mut i64,
        tr: &mut i64,
        tw: &mut i64,
        mf: &mut f64,
    ) {
        if let Some(b) = &node.buffers {
            *hit += b.shared_hit;
            *read += b.shared_read;
            *tr += b.temp_read;
            *tw += b.temp_written;
        }
        if let Some(p) = &node.structured_props {
            if let Some(rr) = p.rows_removed_by_filter {
                if rr > *mf {
                    *mf = rr;
                }
            }
        }
        for child in &node.children {
            walk(child, hit, read, tr, tw, mf);
        }
    }

    walk(
        node,
        &mut shared_hit,
        &mut shared_read,
        &mut temp_read,
        &mut temp_written,
        &mut max_filter,
    );

    let total = shared_hit + shared_read;
    let hit_rate = if total > 0 {
        Some(shared_hit as f64 / total as f64 * 100.0)
    } else {
        None
    };
    let temp_read_kb = if temp_read > 0 {
        Some(temp_read as f64)
    } else {
        None
    };
    let temp_written_kb = if temp_written > 0 {
        Some(temp_written as f64)
    } else {
        None
    };
    let filter_removed = if max_filter > 0.0 {
        Some(max_filter)
    } else {
        None
    };

    (hit_rate, temp_read_kb, temp_written_kb, filter_removed)
}

fn compute_pushdown_status(root: &PlanNode) -> PushdownStatus {
    fn has_streaming(node: &PlanNode) -> bool {
        matches!(
            node.node_type.category(),
            crate::model::node_type::NodeTypeCategory::Streaming
        ) || node.children.iter().any(has_streaming)
    }
    if has_streaming(root) {
        PushdownStatus::NotPushed
    } else {
        PushdownStatus::Local
    }
}
