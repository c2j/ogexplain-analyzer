use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use ogexplain_core::analyzer::{Finding, Severity};
use ogexplain_core::model::PlanNode;
use ogexplain_core::suggester::Suggestion;
use ogsql_complexity::ComplexityReport;

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    node: Option<&PlanNode>,
    findings: &[Finding],
    suggestions: &[Suggestion],
    complexity: Option<&ComplexityReport>,
    show_complexity: bool,
    scroll: u16,
    focused: bool,
    total_lines: u16,
) {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = if total_lines > 0 {
        format!(" 节点详情 (行 {}/{}) ", scroll, total_lines)
    } else {
        " 节点详情 ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let mut lines = match node {
        Some(n) => build_detail_lines(n, findings.to_vec(), suggestions.to_vec()),
        None => vec![Line::from(Span::styled(
            " 粘贴 EXPLAIN 输出后按 Ctrl+P 解析",
            Style::default().fg(Color::DarkGray),
        ))],
    };

    if show_complexity {
        if let Some(report) = complexity {
            lines.push(Line::from(Span::raw("")));
            lines.extend(build_complexity_lines(report));
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
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "── 节点 ──",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    lines.push(Line::from(vec![
        Span::styled("类型: ", Style::default().fg(Color::Gray)),
        Span::raw(node.node_type.to_string()),
    ]));

    if let Some(rel) = &node.relation {
        lines.push(Line::from(vec![
            Span::styled("表: ", Style::default().fg(Color::Gray)),
            Span::raw(rel.clone()),
        ]));
    }

    if let Some(est) = &node.estimated {
        lines.push(Line::from(vec![
            Span::styled("代价: ", Style::default().fg(Color::Gray)),
            Span::raw(format!(
                "{:.2}..{:.2} (rows={:.0}, width={})",
                est.startup_cost, est.total_cost, est.plan_rows, est.plan_width
            )),
        ]));
    }

    if let Some(act) = &node.actual {
        lines.push(Line::from(vec![
            Span::styled("实际: ", Style::default().fg(Color::Gray)),
            Span::raw(format!(
                "startup={:.3}ms total={:.3}ms rows={:.0} loops={:.0}",
                act.startup_time_ms, act.total_time_ms, act.rows, act.loops
            )),
        ]));
        if !act.executed {
            lines.push(Line::from(Span::styled(
                "  (未执行)",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    if let Some(buffers) = &node.buffers {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "── 缓冲区 ──",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        if buffers.shared_hit > 0 || buffers.shared_read > 0 {
            lines.push(Line::from(format!(
                "  共享: 命中={} 读取={} 写脏={} 写入={}",
                buffers.shared_hit,
                buffers.shared_read,
                buffers.shared_dirtied,
                buffers.shared_written
            )));
        }
        if buffers.temp_read > 0 || buffers.temp_written > 0 {
            lines.push(Line::from(format!(
                "  临时: 读取={} 写入={}",
                buffers.temp_read, buffers.temp_written
            )));
        }
    }

    if !node.properties.is_empty() {
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "── 属性 ──",
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
            "── 诊断 ──",
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
            "── 建议 ──",
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

    lines
}

fn severity_style(sev: &Severity) -> (&'static str, Color) {
    match sev {
        Severity::Critical => ("✖", Color::Red),
        Severity::Warning => ("⚠", Color::Yellow),
        Severity::Info => ("ℹ", Color::Green),
    }
}

fn build_complexity_lines(report: &ComplexityReport) -> Vec<Line<'static>> {
    use ogsql_complexity::ComplexityLevel;

    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "── SQL 复杂度分析 ──",
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
        Span::styled("  总分: ", Style::default().fg(Color::Gray)),
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
                    format!("  语句 #{} ", i + 1),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:.1} 分", stmt.adjusted_score),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        let m = &stmt.metrics;
        let b = &stmt.weighted_breakdown;
        let mut parts: Vec<String> = Vec::new();

        if m.table_count > 0 {
            parts.push(format!("{}表(={:.1})", m.table_count, b.tables));
        }
        if m.join_count > 0 {
            parts.push(format!("{}连接(={:.1})", m.join_count, b.joins));
        }
        if m.where_condition_count > 0 {
            parts.push(format!(
                "{}条件(={:.1})",
                m.where_condition_count, b.where_conditions
            ));
        }
        if m.subquery_count > 0 {
            parts.push(format!("{}子查询(={:.1})", m.subquery_count, b.subqueries));
        }
        if m.aggregate_function_count > 0 {
            parts.push(format!(
                "{}聚合(={:.1})",
                m.aggregate_function_count, b.aggregate_functions
            ));
        }
        if m.case_expression_count > 0 {
            parts.push(format!(
                "{}CASE(={:.1})",
                m.case_expression_count, b.case_expressions
            ));
        }
        if m.set_operation_count > 0 {
            parts.push(format!(
                "{}集合操作(={:.1})",
                m.set_operation_count, b.set_operations
            ));
        }
        if m.has_group_by {
            parts.push(format!("GROUP BY({:.1})", b.group_by));
        }
        if m.has_order_by {
            parts.push(format!("ORDER BY({:.1})", b.order_by));
        }
        if m.window_function_count > 0 {
            parts.push(format!(
                "{}窗口(={:.1})",
                m.window_function_count, b.window_functions
            ));
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
                format!("    嵌套深度: {}", m.subquery_depth),
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

    lines.push(Line::from(Span::styled(
        "    [c] 切换复杂度视图",
        Style::default().fg(Color::DarkGray),
    )));

    lines
}
