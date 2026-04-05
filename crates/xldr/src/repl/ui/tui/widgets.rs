//! TUI widget rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};

use super::app::{App, FocusPane, ResultEntryKind};

pub(super) fn draw(frame: &mut Frame, app: &App) {
    let completion_h = if app.completion.visible { 5u16 } else { 1 };
    let input_lines = app.active_buf().text.lines().count().max(1) as u16;
    let input_h = input_lines.clamp(3, 8);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(completion_h),
            Constraint::Length(input_h),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_results(frame, app, chunks[0]);
    draw_docs(frame, app, chunks[1]);
    draw_completion(frame, app, chunks[2]);
    draw_input(frame, app, chunks[3]);
    draw_status(frame, app, chunks[4]);
}

fn pane_title(name: &str, focused: bool) -> String {
    if focused {
        format!("[*] {name}")
    } else {
        format!("[ ] {name}")
    }
}

// ── Results pane custom widget ────────────────────────────────────────────────

struct ResultsPaneWidget<'a> {
    app: &'a App,
    focused: bool,
}

impl Widget for ResultsPaneWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let outer_block = Block::default()
            .title(pane_title("Results / History", self.focused))
            .borders(Borders::ALL)
            .border_style(border_style);
        let inner = outer_block.inner(area);
        outer_block.render(area, buf);

        let mut y = inner.top();
        for entry in self.app.results.iter().skip(self.app.results_scroll) {
            if y >= inner.bottom() {
                break;
            }

            let content_lines = entry.rendered_lines.len().max(1) as u16;
            let entry_height = 2 + content_lines; // 2 for top+bottom borders
            let available = inner.bottom() - y;
            let h = entry_height.min(available);
            let entry_rect = Rect::new(inner.left(), y, inner.width, h);

            let is_selected = self.app.selected_result == Some(entry.idx);
            let entry_border_style = if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let kind_style = match entry.kind {
                ResultEntryKind::EvalError => Style::default().fg(Color::Red),
                ResultEntryKind::Info => Style::default().fg(Color::Yellow),
                ResultEntryKind::CommandOutput => Style::default().fg(Color::Cyan),
                ResultEntryKind::EvalSuccess => Style::default(),
            };
            let result_style = match entry.kind {
                ResultEntryKind::EvalSuccess => Style::default().fg(Color::Green),
                _ => kind_style,
            };

            let title = format!("xldr({})> {}", entry.idx, entry.source);
            let entry_block = Block::default()
                .title(Span::styled(title, kind_style))
                .borders(Borders::ALL)
                .border_style(entry_border_style);
            let content_area = entry_block.inner(entry_rect);
            entry_block.render(entry_rect, buf);

            if content_area.height > 0 && !entry.rendered_lines.is_empty() {
                let lines: Vec<Line> = entry
                    .rendered_lines
                    .iter()
                    .map(|l| Line::styled(l.clone(), result_style))
                    .collect();
                Paragraph::new(lines).render(content_area, buf);
            }

            y += h;
        }
    }
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == FocusPane::Results;
    frame.render_widget(ResultsPaneWidget { app, focused }, area);
}

fn draw_docs(frame: &mut Frame, app: &App, area: Rect) {
    let title = pane_title("Docs Queue", app.focus == FocusPane::Docs);
    let lines: Vec<Line> = if app.docs.is_empty() {
        vec![Line::from("(empty)")]
    } else {
        app.docs
            .iter()
            .map(|d| {
                let sig = d.signature.clone().unwrap_or_default();
                let focused = app.selected_doc == Some(d.idx);
                let marker = if focused { ">" } else { " " };
                Line::from(format!("{marker} [{}] {} {}", d.idx, d.symbol, sig))
            })
            .collect()
    };
    let para = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL).padding(Padding::horizontal(1)));
    frame.render_widget(para, area);
}

fn draw_completion(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().title("Completion").borders(Borders::ALL).padding(Padding::horizontal(1));
    let lines: Vec<Line> = if app.completion.visible {
        app.completion
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let cursor = if i == app.completion.selected { ">" } else { " " };
                let detail = item.detail.clone().unwrap_or_default();
                let style = if i == app.completion.selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                Line::styled(format!("{cursor} {:<16} {}", item.label, detail), style)
            })
            .collect()
    } else {
        vec![Line::from("")]
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    use super::app::InputMode;
    let title = match app.input_mode {
        InputMode::Insert => pane_title("Input", app.focus == FocusPane::Input),
        InputMode::Command => pane_title("Command", app.focus == FocusPane::Input),
    };
    let text = app.active_buf().text.clone();
    let para = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL).padding(Padding::horizontal(1)));
    frame.render_widget(para, area);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(app.status.clone()), area);
}
