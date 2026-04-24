use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use rust_i18n::t;

use crate::app::{AppMode, FocusTarget};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    mode: AppMode,
    focus: FocusTarget,
    total_plans: usize,
) {
    let k = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let d = Style::default().fg(Color::White);
    let sep = Style::default().fg(Color::DarkGray);

    let multi = total_plans > 1;

    let spans = match mode {
        AppMode::Input => vec![
            span(&t!("tui.status.paste_hint"), sep),
            span("Ctrl+P", k),
            span(&t!("tui.status.parse"), d),
            span("Ctrl+L", k),
            span(&t!("tui.status.clear"), d),
            span(":load", k),
            span(&t!("tui.status.file"), d),
            span("│ ", sep),
            span("?", k),
            span(&t!("tui.status.help"), d),
        ],
        AppMode::Browse => match focus {
            FocusTarget::Tree => {
                let mut v = Vec::new();
                if multi {
                    v.extend_from_slice(&[span("N/P", k), span(&t!("tui.status.switch_plan"), d), span("│ ", sep)]);
                }
                v.extend_from_slice(&[
                    span("↑↓", k),
                    span(&t!("tui.status.select"), d),
                    span("Enter", k),
                    span(&t!("tui.status.expand_collapse"), d),
                    span("E/W", k),
                    span(&t!("tui.status.all_expand_collapse"), d),
                    span("g/G", k),
                    span(&t!("tui.status.jump_top_bottom"), d),
                    span("│ ", sep),
                    span("r", k),
                    span(&t!("tui.status.raw_plan"), d),
                    span("c", k),
                    span(&t!("tui.status.complexity"), d),
                    span("F", k),
                    span(&t!("tui.status.all_diag"), d),
                    span("?", k),
                    span(&t!("tui.status.help"), d),
                ]);
                v
            }
            FocusTarget::Detail => {
                let mut v = Vec::new();
                if multi {
                    v.extend_from_slice(&[span("N/P", k), span(&t!("tui.status.switch_plan"), d), span("│ ", sep)]);
                }
                v.extend_from_slice(&[
                    span("↑↓", k),
                    span(&t!("tui.status.scroll"), d),
                    span("PgUp/PgDn", k),
                    span(&t!("tui.status.page_up_down"), d),
                    span("Home/End", k),
                    span(&t!("tui.status.jump_top_bottom"), d),
                    span("│ ", sep),
                    span("r", k),
                    span(&t!("tui.status.raw_plan"), d),
                    span("c", k),
                    span(&t!("tui.status.complexity"), d),
                    span("F", k),
                    span(&t!("tui.status.back_to_node"), d),
                    span("Tab", k),
                    span(&t!("tui.status.switch_panel"), d),
                    span("?", k),
                    span(&t!("tui.status.help"), d),
                ]);
                v
            }
            FocusTarget::Input => vec![
                span(&t!("tui.status.paste_new"), sep),
                span("Ctrl+P", k),
                span(&t!("tui.status.reparse"), d),
                span("Ctrl+L", k),
                span(&t!("tui.status.clear"), d),
                span("│ ", sep),
                span("Tab", k),
                span(&t!("tui.status.back_to_tree"), d),
                span("?", k),
                span(&t!("tui.status.help"), d),
            ],
        },
    };

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn span(text: &str, style: Style) -> Span<'static> {
    Span::styled(text.to_string(), style)
}
