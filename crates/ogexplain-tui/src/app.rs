use std::collections::{HashMap, HashSet};

use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use ratatui_textarea::TextArea;
use rust_i18n::t;

use ogexplain_core::analyzer::{DiagnosticReport, Finding, Severity};
use ogexplain_core::model::node_type::NodeTypeCategory as NodeCategory;
use ogexplain_core::model::{ExplainPlan, PlanNode};
use ogexplain_core::suggester::{Suggestion, SuggestionEngine};

use crate::action::Action;
use crate::components;
use crate::event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Input,
    Browse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Tree,
    Detail,
    Input,
}

impl FocusTarget {
    const VALUES: [FocusTarget; 3] = [FocusTarget::Tree, FocusTarget::Detail, FocusTarget::Input];

    fn index(self) -> usize {
        match self {
            FocusTarget::Tree => 0,
            FocusTarget::Detail => 1,
            FocusTarget::Input => 2,
        }
    }

    fn from_index(i: usize) -> Self {
        Self::VALUES[i % Self::VALUES.len()]
    }

    fn next(self) -> Self {
        Self::from_index(self.index() + 1)
    }

    fn prev(self) -> Self {
        let idx = self.index();
        if idx == 0 {
            Self::from_index(Self::VALUES.len() - 1)
        } else {
            Self::from_index(idx - 1)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCat {
    Scan,
    Join,
    Aggregate,
    Sort,
    Dml,
    SetOp,
    Auxiliary,
    Streaming,
    Other,
}

impl From<NodeCategory> for NodeCat {
    fn from(c: NodeCategory) -> Self {
        match c {
            NodeCategory::Scan => NodeCat::Scan,
            NodeCategory::Join => NodeCat::Join,
            NodeCategory::Aggregate => NodeCat::Aggregate,
            NodeCategory::Sort => NodeCat::Sort,
            NodeCategory::Dml => NodeCat::Dml,
            NodeCategory::SetOp => NodeCat::SetOp,
            NodeCategory::Auxiliary => NodeCat::Auxiliary,
            NodeCategory::Streaming => NodeCat::Streaming,
            NodeCategory::Other => NodeCat::Other,
        }
    }
}

#[derive(Clone)]
pub struct FlatNode {
    pub line_number: usize,
    pub node_type_name: String,
    pub relation: Option<String>,
    pub depth: usize,
    pub expanded: bool,
    pub has_children: bool,
    pub max_severity: Option<Severity>,
    pub category: NodeCat,
}

pub struct App {
    plans: Vec<ExplainPlan>,
    plan_index: usize,
    report: Option<DiagnosticReport>,
    suggestions: Vec<Suggestion>,
    complexity_report: Option<ogsql_complexity::ComplexityReport>,
    gauss_complexity_report: Option<ogsql_complexity::GaussDbComplexityReport>,
    extracted_sql: Option<String>,
    show_complexity: bool,

    flattened_nodes: Vec<FlatNode>,
    selected_index: usize,
    expanded_lines: HashSet<usize>,
    severity_map: HashMap<usize, Severity>,

    mode: AppMode,
    focus: FocusTarget,

    textarea: TextArea<'static>,

    detail_scroll: u16,
    detail_line_count: u16,
    show_all_findings: bool,
    show_help: bool,
    show_raw_view: bool,

    error_message: Option<String>,
    pub should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            plans: Vec::new(),
            plan_index: 0,
            report: None,
            suggestions: Vec::new(),
            complexity_report: None,
            gauss_complexity_report: None,
            extracted_sql: None,
            show_complexity: false,
            flattened_nodes: Vec::new(),
            selected_index: 0,
            expanded_lines: HashSet::new(),
            severity_map: HashMap::new(),
            mode: AppMode::Input,
            focus: FocusTarget::Input,
            textarea: TextArea::default(),
            detail_scroll: 0,
            detail_line_count: 0,
            show_all_findings: false,
            show_help: false,
            show_raw_view: false,
            error_message: None,
            should_quit: false,
        }
    }

    pub fn set_error(&mut self, msg: String) {
        self.error_message = Some(msg);
    }

    pub fn load_file(&mut self, path: &str) -> color_eyre::Result<()> {
        let buf = std::fs::read(path)?;
        let content = String::from_utf8_lossy(&buf).into_owned();
        self.set_textarea_content(&content);
        self.do_parse();
        Ok(())
    }

    fn set_textarea_content(&mut self, content: &str) {
        let lines: Vec<String> = content.lines().map(String::from).collect();
        self.textarea = TextArea::new(lines);
    }

    pub fn handle_paste(&mut self, text: &str) {
        if self.show_help {
            return;
        }
        let text = text.replace('\r', "");
        for ch in text.chars() {
            self.textarea.input(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ));
        }
    }

    fn do_parse(&mut self) {
        let raw: String = self.textarea.lines().join("\n");
        let text = raw.replace('\r', "");
        self.error_message = None;

        let extracted = ogexplain_core::sql::ExtractedContent::from_text(&text);
        if extracted.has_sql {
            self.extracted_sql = Some(extracted.sql_text.clone());
            match ogsql_complexity::analyze(&extracted.sql_text) {
                Ok(report) => {
                    self.complexity_report = Some(report);
                    self.show_complexity = true;
                }
                Err(_) => {
                    self.complexity_report = None;
                }
            }
            if let Ok(report) = ogsql_complexity::gauss_analyze(
                &extracted.sql_text,
                &ogsql_complexity::ComplexityConfig::default(),
            ) {
                self.gauss_complexity_report = Some(report);
            } else {
                self.gauss_complexity_report = None;
            }
        } else {
            self.extracted_sql = None;
            self.complexity_report = None;
            self.gauss_complexity_report = None;
            self.show_complexity = false;
        }

        match ogexplain_core::parse_multi(&text) {
            Ok(plans) if !plans.is_empty() => {
                self.plans = plans;
                self.plan_index = 0;
                self.activate_plan(0);
            }
            Ok(_) => {
                if self.complexity_report.is_some() {
                    self.mode = AppMode::Browse;
                    self.focus = FocusTarget::Tree;
                } else {
                    self.error_message = Some(t!("tui.input.no_nodes").to_string());
                }
            }
            Err(e) => {
                if self.complexity_report.is_some() {
                    self.mode = AppMode::Browse;
                    self.focus = FocusTarget::Tree;
                } else {
                    let preview: String = text
                        .lines()
                        .take(3)
                        .map(|l| {
                            if l.len() > 60 {
                                format!("{}...", &l[..60])
                            } else {
                                l.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    self.error_message = Some(
                        t!(
                            "tui.input.parse_error",
                            error = e.to_string(),
                            lines = text.lines().count(),
                            preview = preview
                        )
                        .to_string(),
                    );
                }
            }
        }
    }

    fn activate_plan(&mut self, index: usize) {
        if index >= self.plans.len() {
            return;
        }
        self.plan_index = index;
        let plan = &self.plans[index];
        let report = ogexplain_core::analyze(plan);
        let suggestions = SuggestionEngine::suggest(&report.findings);

        self.report = Some(report);
        self.suggestions = suggestions;
        self.compute_severity_map();
        self.expand_all();
        self.flatten_tree();
        self.selected_index = 0;
        self.detail_scroll = 0;
        self.mode = AppMode::Browse;
        self.focus = FocusTarget::Tree;
    }

    fn current_plan(&self) -> Option<&ExplainPlan> {
        self.plans.get(self.plan_index)
    }

    fn compute_severity_map(&mut self) {
        self.severity_map.clear();
        let direct: HashMap<usize, Severity> = self
            .report
            .as_ref()
            .map(|r| {
                let mut m = HashMap::new();
                for f in &r.findings {
                    if let Some(line) = f.node_line {
                        let entry = m.entry(line).or_insert(Severity::Info);
                        if f.severity < *entry {
                            *entry = f.severity.clone();
                        }
                    }
                }
                m
            })
            .unwrap_or_default();

        if let Some(plan) = &self.plans.get(self.plan_index) {
            Self::propagate_severity(&plan.root, &direct, &mut self.severity_map);
        }
    }

    fn propagate_severity(
        node: &PlanNode,
        direct: &HashMap<usize, Severity>,
        result: &mut HashMap<usize, Severity>,
    ) -> Option<Severity> {
        let mut max_sev: Option<Severity> = direct.get(&node.line_number).cloned();
        for child in &node.children {
            if let Some(child_sev) = Self::propagate_severity(child, direct, result) {
                max_sev = Some(match max_sev {
                    Some(s) => std::cmp::min(s, child_sev),
                    None => child_sev,
                });
            }
        }
        if let Some(sev) = &max_sev {
            result.insert(node.line_number, sev.clone());
        }
        max_sev
    }

    fn expand_all(&mut self) {
        self.expanded_lines.clear();
        if let Some(plan) = &self.plans.get(self.plan_index) {
            Self::collect_expandable(&plan.root, &mut self.expanded_lines);
        }
    }

    fn collect_expandable(node: &PlanNode, set: &mut HashSet<usize>) {
        if !node.children.is_empty() {
            set.insert(node.line_number);
        }
        for child in &node.children {
            Self::collect_expandable(child, set);
        }
    }

    fn collapse_all(&mut self) {
        self.expanded_lines.clear();
        if let Some(plan) = &self.plans.get(self.plan_index) {
            self.expanded_lines.insert(plan.root.line_number);
        }
    }

    fn flatten_tree(&mut self) {
        self.flattened_nodes.clear();
        if let Some(plan) = &self.plans.get(self.plan_index) {
            let root = plan.root.clone();
            self.flatten_node(&root, 0);
        }
    }

    fn flatten_node(&mut self, node: &PlanNode, depth: usize) {
        let has_children = !node.children.is_empty();
        let is_expanded = self.expanded_lines.contains(&node.line_number);
        let max_severity = self.severity_map.get(&node.line_number).cloned();

        self.flattened_nodes.push(FlatNode {
            line_number: node.line_number,
            node_type_name: node.node_type.to_string(),
            relation: node.relation.clone(),
            depth,
            expanded: is_expanded && has_children,
            has_children,
            max_severity,
            category: node.node_type.category().into(),
        });

        if is_expanded {
            for child in &node.children {
                self.flatten_node(child, depth + 1);
            }
        }
    }

    fn selected_flat_node(&self) -> Option<&FlatNode> {
        self.flattened_nodes.get(self.selected_index)
    }

    fn find_node(&self, line: usize) -> Option<&PlanNode> {
        self.current_plan()
            .and_then(|plan| Self::find_node_rec(&plan.root, line))
    }

    fn find_node_rec(node: &PlanNode, line: usize) -> Option<&PlanNode> {
        if node.line_number == line {
            return Some(node);
        }
        for child in &node.children {
            if let Some(found) = Self::find_node_rec(child, line) {
                return Some(found);
            }
        }
        None
    }

    fn findings_for_line(&self, line: usize) -> Vec<&Finding> {
        self.report
            .as_ref()
            .map(|r| {
                r.findings
                    .iter()
                    .filter(|f| f.node_line == Some(line))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Help overlay blocks everything except closing help
        if self.show_help {
            let action = event::handle_key(key, self.mode, self.focus);
            if matches!(action, Some(Action::ToggleHelp)) {
                self.show_help = false;
                return action;
            }
            return None; // Block all other input while help is open
        }

        let action = event::handle_key(key, self.mode, self.focus);

        if event::should_passthrough(self.mode, self.focus) {
            if let Some(ref a) = action {
                self.update(a.clone());
                return action;
            }
            self.textarea.input(key);
            return None;
        }

        if let Some(ref a) = action {
            self.update(a.clone());
        }
        action
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.should_quit = true;
            }
            Action::ParseExplain => {
                let lines = self.textarea.lines();
                if let Some(first) = lines.first() {
                    let trimmed = first.trim();
                    if let Some(path) = trimmed.strip_prefix(":load ") {
                        let path = path.trim().to_string();
                        match std::fs::read(&path) {
                            Ok(buf) => {
                                self.set_textarea_content(&String::from_utf8_lossy(&buf));
                                self.do_parse();
                            }
                            Err(e) => {
                                self.error_message = Some(
                                    t!("tui.input.load_failed", path = path, error = e.to_string())
                                        .to_string(),
                                );
                            }
                        }
                        return;
                    }
                    if trimmed == ":quit" || trimmed == ":q" {
                        self.should_quit = true;
                        return;
                    }
                }
                self.do_parse();
            }
            Action::LoadFile(path) => match std::fs::read(&path) {
                Ok(buf) => {
                    self.set_textarea_content(&String::from_utf8_lossy(&buf));
                    self.do_parse();
                }
                Err(e) => {
                    self.error_message = Some(
                        t!("tui.input.load_failed", path = path, error = e.to_string()).to_string(),
                    );
                }
            },
            Action::TreeUp => {
                if !self.flattened_nodes.is_empty() && self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.detail_scroll = 0;
                }
            }
            Action::TreeDown => {
                if !self.flattened_nodes.is_empty()
                    && self.selected_index < self.flattened_nodes.len() - 1
                {
                    self.selected_index += 1;
                    self.detail_scroll = 0;
                }
            }
            Action::TreeToggle => {
                if let Some(node) = self.selected_flat_node() {
                    let line = node.line_number;
                    if node.has_children {
                        if self.expanded_lines.contains(&line) {
                            self.expanded_lines.remove(&line);
                        } else {
                            self.expanded_lines.insert(line);
                        }
                        self.flatten_tree();
                    }
                }
            }
            Action::TreeExpandAll => {
                self.expand_all();
                self.flatten_tree();
            }
            Action::TreeCollapseAll => {
                self.collapse_all();
                self.flatten_tree();
            }
            Action::DetailUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
            Action::DetailDown => {
                if self.detail_scroll < self.detail_line_count {
                    self.detail_scroll += 1;
                }
            }
            Action::NextPanel => {
                self.focus = self.focus.next();
            }
            Action::PrevPanel => {
                self.focus = self.focus.prev();
            }
            Action::ToggleFindings => {
                self.show_all_findings = !self.show_all_findings;
                self.detail_scroll = 0;
                if self.show_all_findings && self.focus == FocusTarget::Input {
                    self.focus = FocusTarget::Detail;
                }
            }
            Action::Resize(_, _) => {}
            Action::ClearInput => {
                self.textarea = TextArea::new(vec![String::new()]);
                self.plans.clear();
                self.report = None;
                self.suggestions.clear();
                self.complexity_report = None;
                self.gauss_complexity_report = None;
                self.extracted_sql = None;
                self.show_complexity = false;
                self.flattened_nodes.clear();
                self.severity_map.clear();
                self.error_message = None;
                self.mode = AppMode::Input;
                self.focus = FocusTarget::Input;
                self.show_raw_view = false;
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::ToggleRawView => {
                self.show_raw_view = !self.show_raw_view;
                self.detail_scroll = 0;
            }
            Action::ToggleComplexity => {
                if self.complexity_report.is_some() {
                    self.show_complexity = !self.show_complexity;
                    self.detail_scroll = 0;
                }
            }
            Action::DetailPageUp => {
                let page_size: u16 = 20;
                self.detail_scroll = self.detail_scroll.saturating_sub(page_size);
            }
            Action::DetailPageDown => {
                let page_size: u16 = 20;
                self.detail_scroll = self
                    .detail_scroll
                    .saturating_add(page_size)
                    .min(self.detail_line_count);
            }
            Action::DetailHome => {
                self.detail_scroll = 0;
            }
            Action::DetailEnd => {
                self.detail_scroll = self.detail_line_count;
            }
            Action::TreeTop => {
                if !self.flattened_nodes.is_empty() {
                    self.selected_index = 0;
                    self.detail_scroll = 0;
                }
            }
            Action::TreeBottom => {
                if !self.flattened_nodes.is_empty() {
                    self.selected_index = self.flattened_nodes.len() - 1;
                    self.detail_scroll = 0;
                }
            }
            Action::NextPlan => {
                if !self.plans.is_empty() && self.plan_index + 1 < self.plans.len() {
                    self.activate_plan(self.plan_index + 1);
                }
            }
            Action::PrevPlan => {
                if self.plan_index > 0 {
                    self.activate_plan(self.plan_index - 1);
                }
            }
        }
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(7),
                Constraint::Length(1),
            ])
            .split(area);

        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(chunks[2]);

        self.render_title(frame, chunks[0]);
        components::render_summary(
            frame,
            chunks[1],
            self.report.as_ref(),
            self.current_plan().and_then(|p| p.summary.as_ref()),
            self.plans.len(),
            self.plan_index,
        );
        self.render_main(frame, main_chunks[0], main_chunks[1]);
        self.render_input(frame, chunks[3]);
        components::render_status(frame, chunks[4], self.mode, self.focus, self.plans.len());

        if self.show_help {
            components::render_help(frame, chunks[2]);
        }
    }

    fn render_title(&self, frame: &mut Frame<'_>, area: Rect) {
        let w = area.width as usize;
        let title_text = " ogexplain-analyzer";
        let title_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let mode_style = Style::default().fg(Color::Yellow);
        let hint_style = Style::default().fg(Color::DarkGray);
        let key_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);

        let right_spans: Vec<Span<'_>> = match self.mode {
            AppMode::Input => vec![
                Span::styled(t!("tui.title.hint_paste"), hint_style),
                Span::styled("Ctrl+P", key_style),
                Span::styled(t!("tui.title.hint_parse"), hint_style),
            ],
            AppMode::Browse => match self.focus {
                FocusTarget::Input => vec![
                    Span::styled(t!("tui.title.edit_area"), mode_style),
                    Span::styled("│ ", hint_style),
                    Span::styled("Ctrl+P", key_style),
                    Span::styled(t!("tui.title.reparse"), hint_style),
                ],
                _ => vec![
                    Span::styled(t!("tui.title.browse_mode"), mode_style),
                    Span::styled("│ ", hint_style),
                    Span::styled("?", key_style),
                    Span::styled(t!("tui.title.all_keys"), hint_style),
                ],
            },
        };
        let right_width: usize = right_spans.iter().map(|s| s.content.len()).sum();
        let padding = w.saturating_sub(title_text.len() + right_width);

        let mut spans = vec![
            Span::styled(title_text, title_style),
            Span::raw(format!("{:padding$}", "", padding = padding)),
        ];
        spans.extend(right_spans);

        let line = Line::from(spans);
        frame.render_widget(
            Paragraph::new(line).style(Style::default().bg(Color::Black)),
            area,
        );
    }

    fn render_main(&self, frame: &mut Frame<'_>, tree_area: Rect, detail_area: Rect) {
        components::render_tree(
            frame,
            tree_area,
            &self.flattened_nodes,
            self.selected_index,
            self.focus == FocusTarget::Tree,
        );

        if self.show_raw_view && self.current_plan().is_some() {
            let raw_lines: Vec<Line<'_>> = self
                .textarea
                .lines()
                .iter()
                .map(|l| Line::from(Span::raw(l.clone())))
                .collect();
            let _total = raw_lines.len() as u16;

            let border_style = if self.focus == FocusTarget::Detail {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let block = Block::default()
                .title(t!("tui.raw.title"))
                .borders(Borders::ALL)
                .border_style(border_style);

            let paragraph = Paragraph::new(raw_lines)
                .block(block)
                .scroll((self.detail_scroll, 0));
            frame.render_widget(paragraph, detail_area);
            return;
        }

        if self.show_all_findings {
            let all_findings: Vec<Finding> = self
                .report
                .as_ref()
                .map(|r| r.findings.clone())
                .unwrap_or_default();
            components::render_detail(
                frame,
                detail_area,
                None,
                &all_findings,
                &self.suggestions,
                self.complexity_report.as_ref(),
                self.show_complexity,
                self.gauss_complexity_report.as_ref(),
                self.detail_scroll,
                self.focus == FocusTarget::Detail,
                self.detail_line_count,
                self.current_plan(),
            );
        } else {
            let selected_node = self.selected_flat_node();
            let node = selected_node.and_then(|n| self.find_node(n.line_number));
            let line_num = selected_node.map(|n| n.line_number).unwrap_or(0);
            let findings: Vec<Finding> = self
                .findings_for_line(line_num)
                .into_iter()
                .cloned()
                .collect();
            let related_suggestions: Vec<Suggestion> = if !findings.is_empty() {
                let rule_ids: Vec<&str> = findings.iter().map(|f| f.rule_id.as_str()).collect();
                self.suggestions
                    .iter()
                    .filter(|s| {
                        s.related_rules
                            .iter()
                            .any(|r| rule_ids.contains(&r.as_str()))
                    })
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };

            components::render_detail(
                frame,
                detail_area,
                node,
                &findings,
                &related_suggestions,
                self.complexity_report.as_ref(),
                self.show_complexity,
                self.gauss_complexity_report.as_ref(),
                self.detail_scroll,
                self.focus == FocusTarget::Detail,
                self.detail_line_count,
                self.current_plan(),
            );
        }
    }

    fn render_input(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let focused = self.focus == FocusTarget::Input;

        let title = if self.error_message.is_some() {
            t!("tui.input.error")
        } else if self.mode == AppMode::Input {
            t!("tui.input.parse_hint")
        } else {
            t!("tui.input.title")
        };

        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        self.textarea.set_block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style),
        );

        let text_style = if focused {
            Style::default()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        self.textarea.set_style(text_style);

        frame.render_widget(&self.textarea, area);

        if let Some(err) = &self.error_message {
            let err_line = Line::from(Span::styled(
                t!("tui.input.error_prefix", msg = err),
                Style::default().fg(Color::Red),
            ));
            let inner = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red))
                .inner(area);
            let err_area = Rect {
                height: 2,
                y: area.y + area.height.saturating_sub(2),
                ..inner
            };
            frame.render_widget(
                Paragraph::new(err_line).style(Style::default().bg(Color::Black)),
                err_area,
            );
        }
    }
}
