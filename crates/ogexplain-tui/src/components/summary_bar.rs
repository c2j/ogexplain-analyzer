use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use rust_i18n::t;

use ogexplain_core::analyzer::report::DiagnosticReport;
use ogexplain_core::analyzer::Severity;
use ogexplain_core::model::PlanSummary;

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    report: Option<&DiagnosticReport>,
    plan_summary: Option<&PlanSummary>,
    total_plans: usize,
    plan_index: usize,
) {
    let mut parts: Vec<Span<'static>> = Vec::new();

    if total_plans > 1 {
        parts.push(Span::styled(
            t!(
                "tui.summary.plan",
                current = plan_index + 1,
                total = total_plans
            ),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        parts.push(Span::styled(
            t!("tui.summary.switch"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    match report {
        Some(r) if !r.findings.is_empty() => {
            let criticals = r
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Critical)
                .count();
            let warnings = r
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Warning)
                .count();
            let infos = r
                .findings
                .iter()
                .filter(|f| f.severity == Severity::Info)
                .count();

            if !parts.is_empty() {
                parts.push(Span::raw(" "));
            }
            parts.push(Span::styled(
                t!("tui.summary.findings"),
                Style::default().fg(Color::White),
            ));

            if criticals > 0 {
                parts.push(Span::styled(
                    t!("tui.summary.critical", count = criticals),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            }
            if warnings > 0 {
                if criticals > 0 {
                    parts.push(Span::raw("  "));
                }
                parts.push(Span::styled(
                    t!("tui.summary.warnings", count = warnings),
                    Style::default().fg(Color::Yellow),
                ));
            }
            if infos > 0 {
                if criticals > 0 || warnings > 0 {
                    parts.push(Span::raw("  "));
                }
                parts.push(Span::styled(
                    t!("tui.summary.info", count = infos),
                    Style::default().fg(Color::Green),
                ));
            }

            parts.push(Span::raw("  "));
            parts.push(Span::styled(
                t!("tui.summary.total", count = r.findings.len()),
                Style::default().fg(Color::DarkGray),
            ));
            parts.push(Span::styled(
                t!("tui.summary.view_all"),
                Style::default().fg(Color::DarkGray),
            ));

            append_summary_stats(&mut parts, plan_summary);
        }
        Some(_) => {
            if !parts.is_empty() {
                parts.push(Span::raw(" "));
            }
            parts.push(Span::styled(
                t!("tui.summary.findings"),
                Style::default().fg(Color::White),
            ));
            parts.push(Span::styled(
                t!("tui.summary.no_issues"),
                Style::default().fg(Color::Green),
            ));
            append_summary_stats(&mut parts, plan_summary);
        }
        None => {}
    };

    let line = Line::from(parts);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::Black));
    frame.render_widget(paragraph, area);
}

fn append_summary_stats(parts: &mut Vec<Span<'static>>, summary: Option<&PlanSummary>) {
    if let Some(s) = summary {
        if let Some(ms) = s.total_runtime_ms {
            parts.push(Span::styled(
                t!("tui.summary.time", time = format!("{:.1}", ms)),
                Style::default().fg(Color::DarkGray),
            ));
        }
        if let Some(kb) = s.peak_memory_kb {
            parts.push(Span::styled(
                t!("tui.summary.memory", mem = kb),
                Style::default().fg(Color::DarkGray),
            ));
        }
        if let Some(n) = s.plan_size_bytes {
            parts.push(Span::styled(
                t!("tui.summary.nodes", count = n),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }
}
