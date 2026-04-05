//! TUI widget rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph},
    Frame,
};

use super::app::{App, FocusPane, ResultEntry, ResultEntryKind};

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

fn result_entry_to_list_item(entry: &ResultEntry) -> ListItem<'static> {
    let base_style = match entry.kind {
        ResultEntryKind::EvalError => Style::default().fg(Color::Red),
        ResultEntryKind::Info => Style::default().fg(Color::Yellow),
        ResultEntryKind::EvalSuccess | ResultEntryKind::CommandOutput => Style::default(),
    };
    // Result lines (bindings / values) in green.
    let result_style = match entry.kind {
        ResultEntryKind::EvalSuccess => Style::default().fg(Color::Green),
        _ => base_style,
    };

    let lines: Vec<Line<'static>> = entry
        .rendered_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            // First and last line (separator) use base_style; middle lines that
            // are not the prompt use result_style.
            let is_sep = i == 0 || i == entry.rendered_lines.len() - 1;
            let is_prompt = i == 1 && !entry.rendered_lines.is_empty();
            let style = if is_sep || is_prompt { base_style } else { result_style };
            Line::styled(line.clone(), style)
        })
        .collect();

    ListItem::new(Text::from(lines))
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let title = pane_title("Results / History", app.focus == FocusPane::Results);
    let items: Vec<ListItem> = app
        .results
        .iter()
        .skip(app.results_scroll)
        .map(result_entry_to_list_item)
        .collect();

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL).padding(Padding::horizontal(1)));
    frame.render_widget(list, area);
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
