//! TUI widget rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Padding, Paragraph, Widget},
    Frame,
};

use crate::repl::logic::PresentedResultKind;

use super::app::{App, FocusPane};

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

fn pane_block(title: String, focused: bool) -> Block<'static> {
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style)
}

// ── Results pane custom widget ────────────────────────────────────────────────

struct ResultsPaneWidget<'a> {
    app: &'a App,
    focused: bool,
}

impl Widget for ResultsPaneWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let outer_block = pane_block(pane_title("Results / History", self.focused), self.focused);
        let inner = outer_block.inner(area);
        outer_block.render(area, buf);

        let mut y = inner.y;
        let max_y = inner.y.saturating_add(inner.height);

        if self.app.results.is_empty() {
            Paragraph::new("(no results)").render(inner, buf);
            return;
        }

        for entry in self.app.results.iter() {
            let entry_lines_len = 1 + entry.rendered_lines.len();
            let entry_height = (entry_lines_len as u16).saturating_add(2);
            if y.saturating_add(entry_height) > max_y {
                break;
            }

            let is_selected = self.app.selected_result == Some(entry.idx);
            let entry_border_style = if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let title_style = if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };

            let mut entry_lines = Vec::with_capacity(entry_lines_len);
            entry_lines.push(Line::raw(entry.source.clone()));

            let rendered_style = match entry.kind {
                PresentedResultKind::EvalError => Style::default().fg(Color::Red),
                PresentedResultKind::Info => Style::default().fg(Color::Yellow),
                PresentedResultKind::CommandOutput => Style::default().fg(Color::Cyan),
                PresentedResultKind::EvalSuccess => Style::default().fg(Color::Green),
            };
            for line in entry.rendered_lines.iter() {
                entry_lines.push(Line::styled(line.clone(), rendered_style));
            }

            let entry_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: entry_height,
            };

            let entry_block = Block::default()
                .title(format!("xldr:{} [{}]", self.app.mode.as_str(), entry.idx))
                .borders(Borders::ALL)
                .border_style(entry_border_style)
                .title_style(title_style)
                .padding(Padding::horizontal(1));
            let entry_inner = entry_block.inner(entry_area);
            entry_block.render(entry_area, buf);

            Paragraph::new(entry_lines).render(entry_inner, buf);

            y = y.saturating_add(entry_height);
            if y < max_y {
                y = y.saturating_add(1);
            }
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
                let summary = d
                    .body
                    .as_deref()
                    .and_then(|body| body.lines().find(|line| !line.trim().is_empty()))
                    .unwrap_or_default()
                    .to_string();
                let focused = app.selected_doc == Some(d.idx);
                let marker = if focused { ">" } else { " " };
                let suffix = if summary.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", summary)
                };
                Line::from(format!(
                    "{marker} [{}] {} {}{}",
                    d.idx, d.symbol, sig, suffix
                ))
            })
            .collect()
    };
    let block = pane_block(title, app.focus == FocusPane::Docs).padding(Padding::horizontal(1));
    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn draw_completion(frame: &mut Frame, app: &App, area: Rect) {
    let completion_focused = app.focus == FocusPane::Input && app.completion.visible;
    let block =
        pane_block("Completion".to_string(), completion_focused).padding(Padding::horizontal(1));
    let lines: Vec<Line> = if app.completion.visible {
        app.completion
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let cursor = if i == app.completion.selected {
                    ">"
                } else {
                    " "
                };
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
    let block = pane_block(title, app.focus == FocusPane::Input).padding(Padding::horizontal(1));
    let inner = block.inner(area);
    let para = Paragraph::new(text).block(block);
    frame.render_widget(para, area);

    let buf = app.active_buf();
    let before = &buf.text[..buf.cursor_byte.min(buf.text.len())];
    let line = before.chars().filter(|&c| c == '\n').count() as u16;
    let col = before
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count() as u16)
        .unwrap_or_else(|| before.chars().count() as u16);

    let cursor_x = inner.x.saturating_add(col);
    let cursor_y = inner.y.saturating_add(line);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(app.status.clone()), area);
}
