use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

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
            span(" 粘贴EXPLAIN文本 → ", sep),
            span("Ctrl+P", k),
            span("解析  ", d),
            span("Ctrl+L", k),
            span("清空  ", d),
            span(":load", k),
            span("文件  ", d),
            span("│ ", sep),
            span("?", k),
            span("帮助 ", d),
        ],
        AppMode::Browse => match focus {
            FocusTarget::Tree => {
                let mut v = Vec::new();
                if multi {
                    v.extend_from_slice(&[span("N/P", k), span("切换计划  ", d), span("│ ", sep)]);
                }
                v.extend_from_slice(&[
                    span("↑↓", k),
                    span("选择  ", d),
                    span("Enter", k),
                    span("展开/折叠  ", d),
                    span("E/W", k),
                    span("全展开/全折叠  ", d),
                    span("g/G", k),
                    span("跳首/跳尾  ", d),
                    span("│ ", sep),
                    span("r", k),
                    span("原始计划  ", d),
                    span("F", k),
                    span("全部诊断  ", d),
                    span("?", k),
                    span("帮助 ", d),
                ]);
                v
            }
            FocusTarget::Detail => {
                let mut v = Vec::new();
                if multi {
                    v.extend_from_slice(&[span("N/P", k), span("切换计划  ", d), span("│ ", sep)]);
                }
                v.extend_from_slice(&[
                    span("↑↓", k),
                    span("滚动  ", d),
                    span("PgUp/PgDn", k),
                    span("翻页  ", d),
                    span("Home/End", k),
                    span("跳首/跳尾  ", d),
                    span("│ ", sep),
                    span("r", k),
                    span("原始计划  ", d),
                    span("F", k),
                    span("回到节点  ", d),
                    span("Tab", k),
                    span("切换面板  ", d),
                    span("?", k),
                    span("帮助 ", d),
                ]);
                v
            }
            FocusTarget::Input => vec![
                span("粘贴新EXPLAIN → ", sep),
                span("Ctrl+P", k),
                span("重新解析  ", d),
                span("Ctrl+L", k),
                span("清空  ", d),
                span("│ ", sep),
                span("Tab", k),
                span("回到计划树  ", d),
                span("?", k),
                span("帮助 ", d),
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
