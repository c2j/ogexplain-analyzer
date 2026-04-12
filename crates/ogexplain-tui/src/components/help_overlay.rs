use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

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
            " 快捷键帮助 (?) 关闭 ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" 导航", header_style)),
        Line::from(vec![
            Span::styled("  ↑/k      ", key_style),
            Span::styled("上移", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ↓/j      ", key_style),
            Span::styled("下移", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g        ", key_style),
            Span::styled("跳到树顶", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  G        ", key_style),
            Span::styled("跳到树底", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Tab      ", key_style),
            Span::styled("切换面板", desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" 计划树", header_style)),
        Line::from(vec![
            Span::styled("  Enter    ", key_style),
            Span::styled("展开/折叠", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  E        ", key_style),
            Span::styled("全部展开", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  W        ", key_style),
            Span::styled("全部折叠", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  F        ", key_style),
            Span::styled("切换全部诊断", desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" 详情面板", header_style)),
        Line::from(vec![
            Span::styled("  ↑/k      ", key_style),
            Span::styled("滚动上", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ↓/j      ", key_style),
            Span::styled("滚动下", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  PgUp     ", key_style),
            Span::styled("上翻页", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  PgDn     ", key_style),
            Span::styled("下翻页", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Home     ", key_style),
            Span::styled("跳到顶部", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  End      ", key_style),
            Span::styled("跳到底部", desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" 输入", header_style)),
        Line::from(vec![
            Span::styled("  Ctrl+P   ", key_style),
            Span::styled("解析执行计划", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+L   ", key_style),
            Span::styled("清空输入", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  :load    ", key_style),
            Span::styled("加载文件", desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" 其他", header_style)),
        Line::from(vec![
            Span::styled("  r        ", key_style),
            Span::styled("原始执行计划视图", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ?/F1     ", key_style),
            Span::styled("帮助", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  q        ", key_style),
            Span::styled("退出", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+C   ", key_style),
            Span::styled("退出", desc_style),
        ]),
        Line::from(Span::raw("")),
        Line::from(Span::styled(" 节点颜色", header_style)),
        Line::from(vec![
            Span::styled("  蓝色=Scan ", key_style),
            Span::styled("洋红=Join", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  青色=Aggregate", key_style),
            Span::styled("", empty),
        ]),
        Line::from(vec![
            Span::styled("  黄色=Sort ", key_style),
            Span::styled("绿色=DML", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  红色=Streaming", key_style),
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
