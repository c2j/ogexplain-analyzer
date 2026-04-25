use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use rust_i18n::t;

use ogexplain_core::analyzer::{Finding, Severity};
use ogexplain_core::model::{ExplainPlan, PlanNode, PlanSummary};
use ogexplain_core::suggester::Suggestion;
use ogsql_complexity::{ComplexityLevel, ComplexityReport, GaussDbComplexityReport, SqlCategory};

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    node: Option<&PlanNode>,
    findings: &[Finding],
    suggestions: &[Suggestion],
    complexity: Option<&ComplexityReport>,
    show_complexity: bool,
    gauss_complexity: Option<&GaussDbComplexityReport>,
    scroll: u16,
    focused: bool,
    total_lines: u16,
    plan: Option<&ExplainPlan>,
) {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = if total_lines > 0 {
        t!(
            "tui.detail.title_scrolled",
            current = scroll,
            total = total_lines
        )
    } else {
        t!("tui.detail.title")
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let summary = plan.and_then(|p| p.summary.as_ref());
    let root = plan.map(|p| &p.root);

    let mut lines = match node {
        Some(n) => build_detail_lines(n, findings.to_vec(), suggestions.to_vec(), root, summary),
        None => vec![Line::from(Span::styled(
            t!("tui.detail.empty_hint"),
            Style::default().fg(Color::DarkGray),
        ))],
    };

    if show_complexity {
        if let Some(report) = complexity {
            lines.push(Line::from(Span::raw("")));
            lines.extend(build_complexity_lines(report, gauss_complexity));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

fn build_detail_lines(
    node: &PlanNode,
    findings: Vec<Finding>,
    suggestions: Vec<Suggestion>,
    root: Option<&PlanNode>,
    summary: Option<&PlanSummary>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        t!("tui.detail.section_node"),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    lines.push(Line::from(vec![
        Span::styled(
            t!("tui.detail.label_type"),
            Style::default().fg(Color::Gray),
        ),
        Span::raw(node.node_type.to_string()),
    ]));

    if let Some(rel) = &node.relation {
        lines.push(Line::from(vec![
            Span::styled(
                t!("tui.detail.label_table"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(rel.clone()),
        ]));
    }

    if let Some(est) = &node.estimated {
        lines.push(Line::from(vec![
            Span::styled(
                t!("tui.detail.label_cost"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(format!(
                "{:.2}..{:.2} (rows={:.0}, width={})",
                est.startup_cost, est.total_cost, est.plan_rows, est.plan_width
            )),
        ]));
    }

    if let Some(act) = &node.actual {
        lines.push(Line::from(vec![
            Span::styled(
                t!("tui.detail.label_actual"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(format!(
                "startup={:.3}ms total={:.3}ms rows={:.0} loops={:.0}",
                act.startup_time_ms, act.total_time_ms, act.rows, act.loops
            )),
        ]));
        if !act.executed {
            lines.push(Line::from(Span::styled(
                t!("tui.detail.not_executed"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    if let Some(buffers) = &node.buffers {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            t!("tui.detail.section_buffers"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        if buffers.shared_hit > 0 || buffers.shared_read > 0 {
            lines.push(Line::from(t!(
                "tui.detail.shared_buffers",
                hit = buffers.shared_hit,
                read = buffers.shared_read,
                dirtied = buffers.shared_dirtied,
                written = buffers.shared_written
            )));
        }
        if buffers.temp_read > 0 || buffers.temp_written > 0 {
            lines.push(Line::from(t!(
                "tui.detail.temp_buffers",
                read = buffers.temp_read,
                written = buffers.temp_written
            )));
        }
    }

    if !node.properties.is_empty() {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            t!("tui.detail.section_properties"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for prop in &node.properties {
            lines.push(Line::from(format!("{}: {}", prop.label, prop.value)));
        }
    }

    if !findings.is_empty() {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            t!("tui.detail.section_diagnostics"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for f in findings {
            let (icon, color) = severity_style(&f.severity);
            lines.push(Line::from(vec![
                Span::styled(
                    icon,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" [{}] ", f.rule_id), Style::default().fg(color)),
                Span::raw(f.title.clone()),
            ]));
            lines.push(Line::from(Span::styled(
                format!("   {}", f.detail),
                Style::default().fg(Color::Gray),
            )));
            if let Some(sug) = &f.suggestion {
                lines.push(Line::from(Span::styled(
                    format!("   💡 {}", sug),
                    Style::default().fg(Color::Green),
                )));
            }
        }
    }

    if !suggestions.is_empty() {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            t!("tui.detail.section_suggestions"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for s in suggestions {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[{:.0}%] ", s.confidence * 100.0),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(s.message.clone()),
            ]));
        }
    }

    if let Some(root) = root {
        let metrics = build_exec_metrics(root, summary);
        if !metrics.is_empty() {
            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                t!("tui.detail.section_metrics"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.extend(metrics);
        }
    }

    lines
}

fn severity_style(sev: &Severity) -> (&'static str, Color) {
    match sev {
        Severity::Critical => ("✖", Color::Red),
        Severity::Warning => ("⚠", Color::Yellow),
        Severity::Info => ("ℹ", Color::Green),
    }
}

struct AggMetrics {
    buffer_hit_rate: Option<f64>,
    total_temp_read_kb: f64,
    total_temp_written_kb: f64,
    max_filter_removed: f64,
    estimated_rows: Option<f64>,
    actual_rows: Option<f64>,
    loops: Option<f64>,
    network_kb: Option<f64>,
    planner_time_ms: Option<f64>,
    total_runtime_ms: Option<f64>,
}

fn aggregate_from_tree(node: &PlanNode, summary: Option<&PlanSummary>) -> AggMetrics {
    let mut shared_hit: i64 = 0;
    let mut shared_read: i64 = 0;
    let mut temp_read: i64 = 0;
    let mut temp_written: i64 = 0;
    let mut max_filter: f64 = 0.0;

    fn walk(n: &PlanNode, hit: &mut i64, read: &mut i64, tr: &mut i64, tw: &mut i64, mf: &mut f64) {
        if let Some(b) = &n.buffers {
            *hit += b.shared_hit;
            *read += b.shared_read;
            *tr += b.temp_read;
            *tw += b.temp_written;
        }
        if let Some(p) = &n.structured_props {
            if let Some(rr) = p.rows_removed_by_filter {
                *mf = mf.max(rr);
            }
        }
        for child in &n.children {
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
    let buffer_hit_rate = if total > 0 {
        Some(shared_hit as f64 / total as f64 * 100.0)
    } else {
        None
    };

    AggMetrics {
        buffer_hit_rate,
        total_temp_read_kb: temp_read as f64,
        total_temp_written_kb: temp_written as f64,
        max_filter_removed: max_filter,
        estimated_rows: node.estimated.as_ref().map(|e| e.plan_rows),
        actual_rows: node.actual.as_ref().map(|a| a.rows),
        loops: node.actual.as_ref().map(|a| a.loops),
        network_kb: summary.and_then(|s| s.total_network_kb.map(|v| v as f64)),
        planner_time_ms: summary.and_then(|s| s.planner_runtime_ms),
        total_runtime_ms: summary.and_then(|s| s.total_runtime_ms),
    }
}

fn format_rows(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else {
        format!("{:.0}", v)
    }
}

fn build_exec_metrics(root: &PlanNode, summary: Option<&PlanSummary>) -> Vec<Line<'static>> {
    let m = aggregate_from_tree(root, summary);

    let has_any = m.buffer_hit_rate.is_some()
        || m.total_temp_read_kb > 0.0
        || m.total_temp_written_kb > 0.0
        || m.max_filter_removed > 0.0
        || m.estimated_rows.is_some()
        || m.actual_rows.is_some()
        || m.network_kb.is_some()
        || m.planner_time_ms.is_some()
        || m.total_runtime_ms.is_some();

    if !has_any {
        return Vec::new();
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let label = Style::default().fg(Color::Gray);
    let val = Style::default().fg(Color::White);

    if let Some(rate) = m.buffer_hit_rate {
        let color = if rate >= 99.0 {
            Color::Green
        } else if rate >= 90.0 {
            Color::Yellow
        } else {
            Color::Red
        };
        lines.push(Line::from(vec![
            Span::styled(t!("tui.detail.cache_hit_rate"), label),
            Span::styled(
                format!("{:.1}%", rate),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    if m.total_temp_read_kb > 0.0 || m.total_temp_written_kb > 0.0 {
        let mut parts: Vec<Span<'static>> = vec![Span::styled("  Temp: ", label)];
        if m.total_temp_read_kb > 0.0 {
            parts.push(Span::styled(
                t!(
                    "tui.detail.temp_read",
                    kb = format!("{:.0}", m.total_temp_read_kb)
                ),
                val,
            ));
        }
        if m.total_temp_written_kb > 0.0 {
            if m.total_temp_read_kb > 0.0 {
                parts.push(Span::raw("  "));
            }
            parts.push(Span::styled(
                t!(
                    "tui.detail.temp_written",
                    kb = format!("{:.0}", m.total_temp_written_kb)
                ),
                val,
            ));
        }
        lines.push(Line::from(parts));
    }

    if m.max_filter_removed > 0.0 {
        lines.push(Line::from(vec![
            Span::styled(t!("tui.detail.filter_removed"), label),
            Span::styled(format_rows(m.max_filter_removed), val),
        ]));
    }

    if m.estimated_rows.is_some() || m.actual_rows.is_some() {
        let mut parts: Vec<Span<'static>> = vec![Span::styled("  ", Style::default())];
        if let Some(est) = m.estimated_rows {
            parts.push(Span::styled(t!("tui.detail.estimated_rows"), label));
            parts.push(Span::styled(format_rows(est), val));
            if m.actual_rows.is_some() {
                parts.push(Span::raw("  "));
            }
        }
        if let Some(act) = m.actual_rows {
            parts.push(Span::styled(t!("tui.detail.actual_rows"), label));
            parts.push(Span::styled(format_rows(act), val));
        }
        if let Some(loops) = m.loops {
            if loops > 1.0 {
                parts.push(Span::raw("  "));
                parts.push(Span::styled(t!("tui.detail.loops"), label));
                parts.push(Span::styled(format!("{:.0}", loops), val));
            }
        }
        lines.push(Line::from(parts));
    }

    if let Some(kb) = m.network_kb {
        let display = if kb >= 1024.0 {
            format!("{:.1}MB", kb / 1024.0)
        } else {
            format!("{:.0}kB", kb)
        };
        lines.push(Line::from(vec![
            Span::styled(t!("tui.detail.network"), label),
            Span::styled(display, val),
        ]));
    }

    if let Some(t_val) = m.planner_time_ms {
        lines.push(Line::from(vec![
            Span::styled(t!("tui.detail.planner_time"), label),
            Span::styled(format!("{:.3}ms", t_val), val),
        ]));
    }

    if let Some(t_val) = m.total_runtime_ms {
        let display = if t_val >= 1000.0 {
            format!("{:.2}s", t_val / 1000.0)
        } else {
            format!("{:.3}ms", t_val)
        };
        lines.push(Line::from(vec![
            Span::styled(t!("tui.detail.total_time"), label),
            Span::styled(display, val),
        ]));
    }

    lines
}

fn build_complexity_lines(
    report: &ComplexityReport,
    gauss: Option<&GaussDbComplexityReport>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        t!("tui.detail.section_complexity"),
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )));

    let (level_color, _level_icon) = match report.overall_level {
        ComplexityLevel::Trivial => (Color::Green, "●"),
        ComplexityLevel::Simple => (Color::Green, "◐"),
        ComplexityLevel::Moderate => (Color::Yellow, "◑"),
        ComplexityLevel::Complex => (Color::Red, "◉"),
        ComplexityLevel::VeryComplex => (Color::Magenta, "✖"),
    };

    lines.push(Line::from(vec![
        Span::styled(
            t!("tui.detail.total_score"),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            format!("{:.1}", report.overall_score),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", report.overall_level.label()),
            Style::default()
                .fg(level_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", report.profile),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    for (i, stmt) in report.statements.iter().enumerate() {
        if report.statements.len() > 1 {
            lines.push(Line::from(vec![
                Span::styled(
                    t!("tui.detail.statement_n", n = i + 1),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    t!(
                        "tui.detail.score_pts",
                        score = format!("{:.1}", stmt.adjusted_score)
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        let m = &stmt.metrics;
        let b = &stmt.weighted_breakdown;
        let mut parts: Vec<String> = Vec::new();

        if m.table_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_tables",
                    count = m.table_count,
                    score = format!("{:.1}", b.tables)
                )
                .to_string(),
            );
        }
        if m.join_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_joins",
                    count = m.join_count,
                    score = format!("{:.1}", b.joins)
                )
                .to_string(),
            );
        }
        if m.where_condition_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_conditions",
                    count = m.where_condition_count,
                    score = format!("{:.1}", b.where_conditions)
                )
                .to_string(),
            );
        }
        if m.subquery_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_subqueries",
                    count = m.subquery_count,
                    score = format!("{:.1}", b.subqueries)
                )
                .to_string(),
            );
        }
        if m.aggregate_function_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_aggregates",
                    count = m.aggregate_function_count,
                    score = format!("{:.1}", b.aggregate_functions)
                )
                .to_string(),
            );
        }
        if m.case_expression_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_cases",
                    count = m.case_expression_count,
                    score = format!("{:.1}", b.case_expressions)
                )
                .to_string(),
            );
        }
        if m.set_operation_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_set_ops",
                    count = m.set_operation_count,
                    score = format!("{:.1}", b.set_operations)
                )
                .to_string(),
            );
        }
        if m.has_group_by {
            parts.push(format!("GROUP BY({:.1})", b.group_by));
        }
        if m.has_order_by {
            parts.push(format!("ORDER BY({:.1})", b.order_by));
        }
        if m.window_function_count > 0 {
            parts.push(
                t!(
                    "tui.detail.complexity_stmt_windows",
                    count = m.window_function_count,
                    score = format!("{:.1}", b.window_functions)
                )
                .to_string(),
            );
        }
        if m.cte_count > 0 {
            parts.push(format!("{}CTE(={:.1})", m.cte_count, b.ctes));
        }

        for part in parts {
            lines.push(Line::from(Span::styled(
                format!("    {}", part),
                Style::default().fg(Color::Gray),
            )));
        }

        if m.subquery_depth > 0 {
            lines.push(Line::from(Span::styled(
                t!(
                    "tui.detail.complexity_stmt_nesting",
                    depth = m.subquery_depth
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }

        let sql_preview: String = stmt
            .sql_text
            .lines()
            .take(2)
            .map(|l| {
                if l.len() > 60 {
                    format!("{}...", &l[..60])
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(Line::from(Span::styled(
            format!("    SQL: {}", sql_preview),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // GaussDB complexity section
    if let Some(gauss) = gauss {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            t!("tui.detail.section_gauss"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        let gauss_level_color = match gauss.level {
            ComplexityLevel::Trivial | ComplexityLevel::Simple => Color::Green,
            ComplexityLevel::Moderate => Color::Yellow,
            ComplexityLevel::Complex | ComplexityLevel::VeryComplex => Color::Red,
        };
        lines.push(Line::from(vec![
            Span::styled(
                t!("tui.detail.gauss_score"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{}", gauss.overall_score),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}", gauss.level.label()),
                Style::default()
                    .fg(gauss_level_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        let cat_color = match gauss.sql_category {
            SqlCategory::Query => Color::Green,
            SqlCategory::DML => Color::Yellow,
            SqlCategory::DDL => Color::Cyan,
            SqlCategory::PLBlock => Color::Magenta,
            SqlCategory::Package => Color::Blue,
            SqlCategory::DCL => Color::White,
        };
        lines.push(Line::from(vec![
            Span::styled(
                t!("tui.detail.gauss_type"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{} ({})", gauss.sql_sub_type, gauss.sql_category.label()),
                Style::default().fg(cat_color),
            ),
        ]));

        let d = &gauss.dimensions;
        let mut dim_parts: Vec<String> = Vec::new();
        if d.sql_structure > 0 {
            dim_parts.push(t!("tui.detail.gauss_dim_sql", val = d.sql_structure).to_string());
        }
        if d.pl_logic > 0 {
            dim_parts.push(t!("tui.detail.gauss_dim_pl", val = d.pl_logic).to_string());
        }
        if d.advanced_feature > 0 {
            dim_parts.push(t!("tui.detail.gauss_dim_adv", val = d.advanced_feature).to_string());
        }
        if d.extension > 0 {
            dim_parts.push(t!("tui.detail.gauss_dim_ext", val = d.extension).to_string());
        }
        if !dim_parts.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    t!("tui.detail.gauss_dimensions"),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(dim_parts.join(" "), Style::default().fg(Color::White)),
            ]));
        }

        if !gauss.tags.is_empty() {
            let tag_str: Vec<String> = gauss
                .tags
                .iter()
                .map(|t_tag| format!("{}{}", t_tag.icon(), t_tag.label()))
                .collect();
            lines.push(Line::from(vec![
                Span::styled(
                    t!("tui.detail.gauss_tags"),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(tag_str.join(" "), Style::default().fg(Color::Yellow)),
            ]));
        }

        // SQL-level metrics breakdown
        let m = &gauss.pl_metrics;
        let mut metric_parts: Vec<Span<'static>> = Vec::new();

        fn add_metric(parts: &mut Vec<Span<'static>>, label: &str, count: usize, weight: i64) {
            if count > 0 {
                if !parts.is_empty() {
                    parts.push(Span::raw("  "));
                }
                parts.push(Span::styled(
                    format!("{}:{}(={})", label, count, count as i64 * weight),
                    Style::default().fg(Color::Gray),
                ));
            }
        }

        add_metric(
            &mut metric_parts,
            &t!("tui.detail.metric_tables"),
            m.table_count,
            10,
        );
        add_metric(
            &mut metric_parts,
            &t!("tui.detail.metric_joins"),
            m.join_count,
            15,
        );
        add_metric(&mut metric_parts, "WHERE", m.where_condition_count, 5);
        add_metric(
            &mut metric_parts,
            &t!("tui.detail.metric_subqueries"),
            m.subquery_count,
            20,
        );
        add_metric(
            &mut metric_parts,
            &t!("tui.detail.metric_aggregates"),
            m.aggregate_function_count,
            10,
        );
        add_metric(&mut metric_parts, "CASE", m.case_expression_count, 5);
        add_metric(
            &mut metric_parts,
            &t!("tui.detail.metric_set_ops"),
            m.set_operation_count,
            15,
        );
        add_metric(&mut metric_parts, "Hint", m.hint_count, 3);
        add_metric(&mut metric_parts, "CTE", m.cte_count, 0);
        add_metric(
            &mut metric_parts,
            &t!("tui.detail.metric_windows"),
            m.window_function_count,
            0,
        );

        if m.has_group_by {
            if !metric_parts.is_empty() {
                metric_parts.push(Span::raw("  "));
            }
            metric_parts.push(Span::styled(
                format!("GROUP BY(={})", 5),
                Style::default().fg(Color::Gray),
            ));
        }
        if m.has_order_by {
            if !metric_parts.is_empty() {
                metric_parts.push(Span::raw("  "));
            }
            metric_parts.push(Span::styled(
                format!("ORDER BY(={})", 5),
                Style::default().fg(Color::Gray),
            ));
        }
        if m.has_distinct {
            if !metric_parts.is_empty() {
                metric_parts.push(Span::raw("  "));
            }
            metric_parts.push(Span::styled("DISTINCT", Style::default().fg(Color::Gray)));
        }
        if m.subquery_depth > 0 {
            if !metric_parts.is_empty() {
                metric_parts.push(Span::raw("  "));
            }
            metric_parts.push(Span::styled(
                t!("tui.detail.metric_depth", depth = m.subquery_depth),
                Style::default().fg(Color::Gray),
            ));
        }

        if !metric_parts.is_empty() {
            lines.push(Line::from(Span::styled(
                t!("tui.detail.gauss_sql_metrics"),
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(metric_parts));
        }

        // PL-level metrics
        let has_pl_metrics = m.loop_count > 0
            || m.cursor_count > 0
            || m.dynamic_sql_count > 0
            || m.transaction_control_count > 0
            || m.uses_autonomous_transactions
            || m.java_stored_procedure_count > 0;

        if has_pl_metrics {
            lines.push(Line::from(Span::styled(
                t!("tui.detail.gauss_pl_metrics"),
                Style::default().fg(Color::DarkGray),
            )));

            if m.loop_count > 0 {
                let nesting = if m.max_loop_nesting_level > 1 {
                    t!("tui.detail.pl_nesting", depth = m.max_loop_nesting_level).to_string()
                } else {
                    String::new()
                };
                lines.push(Line::from(Span::styled(
                    t!(
                        "tui.detail.pl_loops",
                        count = m.loop_count,
                        nesting = nesting
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }

            if m.cursor_count > 0 {
                lines.push(Line::from(Span::styled(
                    t!(
                        "tui.detail.pl_cursors",
                        count = m.cursor_count,
                        ops = m.cursor_operation_count
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }

            if m.dynamic_sql_count > 0 {
                lines.push(Line::from(Span::styled(
                    t!(
                        "tui.detail.pl_dynamic_sql",
                        count = m.dynamic_sql_count,
                        params = m.param_binding_count
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }

            if m.transaction_control_count > 0 {
                let auto_str = if m.uses_autonomous_transactions {
                    t!("tui.detail.pl_autonomous").to_string()
                } else {
                    String::new()
                };
                lines.push(Line::from(Span::styled(
                    t!(
                        "tui.detail.pl_tx_control",
                        count = m.transaction_control_count,
                        auto = auto_str
                    ),
                    Style::default().fg(Color::Gray),
                )));
            }

            if m.java_stored_procedure_count > 0 {
                lines.push(Line::from(Span::styled(
                    t!("tui.detail.pl_java", count = m.java_stored_procedure_count),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }

        // Score breakdown
        let bd = &gauss.score_breakdown;
        let mut bd_parts: Vec<String> = Vec::new();
        if bd.enhanced_complexity > 0 {
            bd_parts.push(t!("tui.detail.bd_enhanced", val = bd.enhanced_complexity).to_string());
        }
        if bd.loop_complexity > 0 {
            bd_parts.push(t!("tui.detail.bd_loops", val = bd.loop_complexity).to_string());
        }
        if bd.cursor_complexity > 0 {
            bd_parts.push(t!("tui.detail.bd_cursors", val = bd.cursor_complexity).to_string());
        }
        if bd.dynamic_sql_complexity > 0 {
            bd_parts
                .push(t!("tui.detail.bd_dynamic_sql", val = bd.dynamic_sql_complexity).to_string());
        }
        if bd.transaction_complexity > 0 {
            bd_parts.push(t!("tui.detail.bd_tx", val = bd.transaction_complexity).to_string());
        }
        if bd.autonomous_transaction_bonus > 0 {
            bd_parts.push(
                t!(
                    "tui.detail.bd_autonomous",
                    val = bd.autonomous_transaction_bonus
                )
                .to_string(),
            );
        }
        if bd.hint_complexity > 0 {
            bd_parts.push(t!("tui.detail.bd_hint", val = bd.hint_complexity).to_string());
        }

        if !bd_parts.is_empty() {
            lines.push(Line::from(Span::styled(
                t!("tui.detail.score_breakdown", details = bd_parts.join(" ")),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    lines.push(Line::from(Span::styled(
        t!("tui.detail.toggle_complexity"),
        Style::default().fg(Color::DarkGray),
    )));

    lines
}
