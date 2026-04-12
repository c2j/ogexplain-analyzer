use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use ogexplain_core::analyzer::{Finding, Severity};
use ogexplain_core::model::PlanNode;
use ogexplain_core::suggester::Suggestion;

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    node: Option<&PlanNode>,
    findings: &[Finding],
    suggestions: &[Suggestion],
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

    let lines = match node {
        Some(n) => build_detail_lines(n, findings.to_vec(), suggestions.to_vec()),
        None => vec![Line::from(Span::styled(
            " 粘贴 EXPLAIN 输出后按 Ctrl+P 解析",
            Style::default().fg(Color::DarkGray),
        ))],
    };

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
