use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, HighlightSpacing, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::FlatNode;
use ogexplain_core::analyzer::Severity;

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    nodes: &[FlatNode],
    selected: usize,
    focused: bool,
) {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = if nodes.is_empty() {
        " 计划树 ".to_string()
    } else {
        format!(
            " 计划树 ({}/{}) ",
            selected.min(nodes.len() - 1) + 1,
            nodes.len()
        )
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let items: Vec<ListItem> = nodes
        .iter()
        .map(|node| {
            let indent = "  ".repeat(node.depth);
            let expand = if node.has_children {
                if node.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "· "
            };

            let (sev_icon, sev_color) = match node.max_severity {
                Some(Severity::Critical) => (" !!", Color::Red),
                Some(Severity::Warning) => (" !", Color::Yellow),
                Some(Severity::Info) => (" *", Color::Green),
                None => ("", Color::default()),
            };

            let rel = match &node.relation {
                Some(r) if !r.is_empty() => format!(" 表={}", r),
                _ => String::new(),
            };

            let type_color = category_color(node.category);

            let line = Line::from(vec![
                Span::styled(format!("{}{}", indent, expand), Style::default()),
                Span::styled(&node.node_type_name, Style::default().fg(type_color)),
                Span::styled(rel, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    sev_icon,
                    Style::default().fg(sev_color).add_modifier(Modifier::BOLD),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().reversed().add_modifier(Modifier::BOLD))
        .highlight_spacing(HighlightSpacing::Always);

    let mut state = ListState::default();
    if !nodes.is_empty() {
        state.select(Some(selected.min(nodes.len() - 1)));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn category_color(cat: crate::app::NodeCat) -> Color {
    use crate::app::NodeCat as C;
    match cat {
        C::Scan => Color::Blue,
        C::Join => Color::Magenta,
        C::Aggregate => Color::Cyan,
        C::Sort => Color::Yellow,
        C::Dml => Color::Green,
        C::Streaming => Color::Red,
        C::SetOp => Color::LightMagenta,
        _ => Color::White,
    }
}
