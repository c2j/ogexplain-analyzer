use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use rust_i18n::t;

pub fn render(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Clear, area);

    let popup_width = (area.width as usize * 60 / 100).max(40);
    let popup_height = (area.height as usize * 85 / 100).max(20);
    let x = (area.width as usize).saturating_sub(popup_width) / 2;
    let y = (area.height as usize).saturating_sub(popup_height) / 2;

    let popup_area = Rect {
        x: area.x + x as u16,
        y: area.y + y as u16,
        width: popup_width as u16,
        height: popup_height as u16,
    };

    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::Gray);
    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let empty = Style::default();

    let lines = vec![
        Line::from(Span::styled(
            t!("tui.help.title"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(t!("tui.help.section_nav"), header_style)),
        Line::from(vec![
            Span::styled("  ↑/k      ", key_style),
            Span::styled(t!("tui.help.up"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ↓/j      ", key_style),
            Span::styled(t!("tui.help.down"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g        ", key_style),
            Span::styled(t!("tui.help.top"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  G        ", key_style),
            Span::styled(t!("tui.help.bottom"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Tab      ", key_style),
            Span::styled(t!("tui.help.switch_panel"), desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(t!("tui.help.section_tree"), header_style)),
        Line::from(vec![
            Span::styled("  Enter    ", key_style),
            Span::styled(t!("tui.help.expand_collapse"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  E        ", key_style),
            Span::styled(t!("tui.help.expand_all"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  W        ", key_style),
            Span::styled(t!("tui.help.collapse_all"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  F        ", key_style),
            Span::styled(t!("tui.help.toggle_diag"), desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(t!("tui.help.section_detail"), header_style)),
        Line::from(vec![
            Span::styled("  ↑/k      ", key_style),
            Span::styled(t!("tui.help.scroll_up"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ↓/j      ", key_style),
            Span::styled(t!("tui.help.scroll_down"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  PgUp     ", key_style),
            Span::styled(t!("tui.help.page_up"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  PgDn     ", key_style),
            Span::styled(t!("tui.help.page_down"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Home     ", key_style),
            Span::styled(t!("tui.help.jump_top"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  End      ", key_style),
            Span::styled(t!("tui.help.jump_bottom"), desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(t!("tui.help.section_input"), header_style)),
        Line::from(vec![
            Span::styled("  Ctrl+P   ", key_style),
            Span::styled(t!("tui.help.parse"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+L   ", key_style),
            Span::styled(t!("tui.help.clear"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  :load    ", key_style),
            Span::styled(t!("tui.help.load"), desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(t!("tui.help.section_other"), header_style)),
        Line::from(vec![
            Span::styled("  r        ", key_style),
            Span::styled(t!("tui.help.raw_view"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ?/F1     ", key_style),
            Span::styled(t!("tui.help.help"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  q        ", key_style),
            Span::styled(t!("tui.help.quit"), desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+C   ", key_style),
            Span::styled(t!("tui.help.quit"), desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(t!("tui.help.section_colors"), header_style)),
        Line::from(vec![
            Span::styled(t!("tui.help.color_scan"), key_style),
            Span::styled(t!("tui.help.color_join"), desc_style),
        ]),
        Line::from(vec![
            Span::styled(t!("tui.help.color_agg"), key_style),
            Span::styled("", empty),
        ]),
        Line::from(vec![
            Span::styled(t!("tui.help.color_sort"), key_style),
            Span::styled(t!("tui.help.color_dml"), desc_style),
        ]),
        Line::from(vec![
            Span::styled(t!("tui.help.color_stream"), key_style),
            Span::styled("", empty),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, popup_area);
}
