//! TUI key handling and event processing.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::repl::logic::core::{ReplCompletionContext, ReplEngine};
use crate::repl::logic::{present_for_interaction, PresentedEvent, PresentedResultKind};
use crate::repl::ui::completion::{ReplCompletionProvider, ReplCompletionResult};

use super::app::{App, Completion, CompletionItem, FocusPane, InputMode};

// ── Completion helpers ────────────────────────────────────────────────────────

#[cfg(test)]
pub(super) fn current_token_prefix(buf: &super::app::InputBuffer) -> String {
    let before = &buf.text[..buf.cursor_byte];
    before
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '{' | '}' | ','))
        .next_back()
        .unwrap_or("")
        .to_string()
}

pub(super) fn refresh_completion(
    app: &mut App,
    provider: &mut dyn ReplCompletionProvider,
    event_received_at: Option<Instant>,
) {
    match app.input_mode {
        InputMode::Command => {
            let prefix = app.command.text.trim().to_string();
            let items = command_completions(&prefix, app.focus);
            app.completion = Completion {
                visible: !items.is_empty(),
                selected: 0,
                items,
            };
            app.last_completion_telemetry = None;
        }
        InputMode::Insert => {
            if !app.tab_completion_mode {
                app.completion.clear();
                app.last_completion_telemetry = None;
                return;
            }
            if ReplCompletionContext::should_request(&app.input.text, app.input.cursor_byte) {
                app.completion_controller.submit_if_changed(
                    provider,
                    &app.input.text,
                    app.input.cursor_byte,
                    event_received_at,
                );
            } else {
                app.completion_controller.cancel_pending();
                app.completion.clear();
                app.last_completion_telemetry = None;
            }
        }
    }
}

pub(super) fn poll_completion(app: &mut App, provider: &mut dyn ReplCompletionProvider) {
    while let Some(result) = provider.poll_ready() {
        if let Some(result) = app.completion_controller.accept_ready(result) {
            apply_completion_result(app, result);
        }
    }
}

fn apply_completion_result(app: &mut App, result: ReplCompletionResult) {
    let apply_started = Instant::now();
    let mut completion = result.completion;
    let items: Vec<CompletionItem> = completion
        .candidates
        .iter()
        .map(|candidate| CompletionItem {
            label: candidate.replacement.clone(),
            detail: candidate.detail.clone(),
            replace_start: candidate.replace_start,
            replace_end: candidate.replace_end,
        })
        .collect();
    completion
        .telemetry
        .record_completion_apply(apply_started.elapsed());
    completion
        .telemetry
        .record_completion_render(apply_started.elapsed());
    if let Some(event_received_at) = result.event_received_at {
        completion
            .telemetry
            .record_input_event_to_ui_handler(event_received_at.elapsed());
        completion
            .telemetry
            .record_total_key_to_visible_response(event_received_at.elapsed());
    }
    app.last_completion_telemetry = Some(completion.telemetry.clone());
    if items.is_empty() && completion.signature.is_none() {
        app.completion.clear();
    } else {
        app.completion = Completion {
            visible: true,
            selected: 0,
            items,
        };
    }
}

static GLOBAL_COMMANDS: &[(&str, &str)] = &[
    ("q", ":q  — quit"),
    ("help", ":help  — show help"),
    ("doc", ":doc <symbol>  — show docs"),
    ("error", ":error [full|summary]  — set error display mode"),
    ("sig", ":sig <function|query>  — show signature"),
    ("info", ":info <query>  — show derived info"),
    (
        "type",
        ":type <binding>  — lookup binding type (annotate unresolved generics before persistence)",
    ),
    ("facet", ":facet <FacetPath|$binding>  — inspect facet path"),
    ("save", ":save <path>  — save session to .eldr"),
    ("vars", ":vars  — list visible value bindings"),
    ("imported", ":imported  — list active imports"),
    ("defs", ":defs  — list visible top-level defs"),
    ("history", ":history [selector]  — list input history"),
    ("reload", ":reload [all|defs]  — rebuild session"),
    ("clear", ":clear  — clear the screen"),
];

static INPUT_COMMANDS: &[(&str, &str)] = &[("v", ":v <idx>  — recall result")];
static RESULTS_COMMANDS: &[(&str, &str)] = &[("j", ":j <idx>  — jump to result")];
static DOCS_COMMANDS: &[(&str, &str)] = &[
    ("doc-focus", ":doc-focus <idx>"),
    ("doc-drop", ":doc-drop <idx>"),
    ("doc-clear", ":doc-clear"),
];

fn command_completions(prefix: &str, focus: FocusPane) -> Vec<CompletionItem> {
    let pane_cmds = match focus {
        FocusPane::Input => INPUT_COMMANDS,
        FocusPane::Results => RESULTS_COMMANDS,
        FocusPane::Docs => DOCS_COMMANDS,
    };
    GLOBAL_COMMANDS
        .iter()
        .chain(pane_cmds.iter())
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, usage)| CompletionItem {
            label: name.to_string(),
            detail: Some(usage.to_string()),
            replace_start: 0,
            replace_end: prefix.len(),
        })
        .collect()
}

// ── Key handling ──────────────────────────────────────────────────────────────

pub(super) fn handle_key(
    app: &mut App,
    engine: &mut ReplEngine,
    provider: &mut dyn ReplCompletionProvider,
    key: KeyEvent,
    event_received_at: Option<Instant>,
) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match app.focus {
        FocusPane::Input => handle_input_pane(app, engine, provider, key, event_received_at),
        FocusPane::Results => handle_results_pane(app, key),
        FocusPane::Docs => handle_docs_pane(app, key),
    }
}

fn handle_input_pane(
    app: &mut App,
    engine: &mut ReplEngine,
    provider: &mut dyn ReplCompletionProvider,
    key: KeyEvent,
    event_received_at: Option<Instant>,
) {
    match key.code {
        KeyCode::Tab => {
            if app.completion.visible {
                let mut buf = app.active_buf().clone();
                app.completion.apply(&mut buf);
                *app.active_buf_mut() = buf;
                app.completion.clear();
            } else if app.input_mode == InputMode::Insert {
                app.tab_completion_mode = true;
                app.update_status();
                refresh_completion(app, provider, event_received_at);
            } else {
                app.next_focus();
            }
        }
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Left => {
            app.active_buf_mut().move_left();
            refresh_completion(app, provider, event_received_at);
        }
        KeyCode::Right => {
            app.active_buf_mut().move_right();
            refresh_completion(app, provider, event_received_at);
        }
        KeyCode::Up => app.completion.select_prev(),
        KeyCode::Down => app.completion.select_next(),
        KeyCode::Backspace => {
            app.active_buf_mut().backspace();
            refresh_completion(app, provider, event_received_at);
        }
        KeyCode::Esc => {
            if app.input_mode == InputMode::Command {
                app.input_mode = InputMode::Insert;
                app.command.clear();
                app.completion.clear();
                app.completion_controller.cancel_pending();
                app.tab_completion_mode = false;
                app.update_status();
            } else if app.tab_completion_mode || app.completion.visible {
                app.tab_completion_mode = false;
                app.completion.clear();
                app.completion_controller.cancel_pending();
                app.last_completion_telemetry = None;
                app.update_status();
            }
        }
        KeyCode::Enter => match app.input_mode {
            InputMode::Insert => submit_input(app, engine, provider),
            InputMode::Command => submit_command(app, engine, provider),
        },
        KeyCode::Char(':') => {
            if app.input_mode == InputMode::Insert && app.input.cursor_byte == 0 {
                app.input_mode = InputMode::Command;
                app.command.clear();
                app.tab_completion_mode = false;
                app.update_status();
                app.completion_controller.cancel_pending();
                refresh_completion(app, provider, event_received_at);
            } else if app.input_mode == InputMode::Command {
                app.input_mode = InputMode::Insert;
                app.command.clear();
                app.completion.clear();
                app.completion_controller.cancel_pending();
                app.tab_completion_mode = false;
                app.update_status();
            } else {
                app.input.insert_char(':');
                refresh_completion(app, provider, event_received_at);
            }
        }
        KeyCode::Char(ch) => {
            app.active_buf_mut().insert_char(ch);
            refresh_completion(app, provider, event_received_at);
        }
        _ => {}
    }
}

fn handle_results_pane(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => app.next_focus(),
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Up => app.results_scroll = app.results_scroll.saturating_sub(1),
        KeyCode::Down => {
            let max: usize = app
                .results
                .iter()
                .map(|e| 3 + e.stdout_lines.len() + e.rendered_lines.len() + e.stderr_lines.len())
                .sum();
            app.results_scroll = (app.results_scroll + 1).min(max);
        }
        _ => {}
    }
}

fn handle_docs_pane(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => app.next_focus(),
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Up => {
            app.selected_doc = Some(app.selected_doc.unwrap_or(0).saturating_sub(1));
        }
        KeyCode::Down => {
            let max = app.docs.len().saturating_sub(1);
            let next = app.selected_doc.unwrap_or(0).saturating_add(1).min(max);
            app.selected_doc = Some(next);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::repl::ui::completion::{
        ReplCompletionProvider, ReplCompletionRequest, ReplCompletionResult,
    };
    use crate::repl::ui::tui::app::InputBuffer;

    struct ImmediateProvider {
        context: ReplCompletionContext,
        ready: VecDeque<ReplCompletionResult>,
    }

    impl ImmediateProvider {
        fn new(engine: &ReplEngine) -> Self {
            Self {
                context: engine.completion_context(),
                ready: VecDeque::new(),
            }
        }
    }

    impl ReplCompletionProvider for ImmediateProvider {
        fn submit(&mut self, request: ReplCompletionRequest) {
            let mut completion = self.context.completions(&request.input, request.cursor);
            completion
                .telemetry
                .record_completion_queue(std::time::Duration::from_nanos(1));
            self.ready.push_back(ReplCompletionResult {
                input: request.input,
                cursor: request.cursor,
                generation: request.generation,
                completion,
                enqueued_at: request.enqueued_at,
                event_received_at: request.event_received_at,
            });
        }

        fn poll_ready(&mut self) -> Option<ReplCompletionResult> {
            self.ready.pop_front()
        }

        fn replace_context(&mut self, context: ReplCompletionContext) {
            self.context = context;
        }
    }

    #[test]
    fn current_token_prefix_keeps_qualified_type_paths() {
        let mut buf = InputBuffer::default();
        buf.set("String::re".to_string());
        assert_eq!(current_token_prefix(&buf), "String::re");
    }

    #[test]
    fn refresh_completion_uses_engine_candidates_for_qualified_paths() {
        let engine = ReplEngine::new().expect("REPL engine should bootstrap");
        let mut app = App::new();
        let mut provider = ImmediateProvider::new(&engine);
        app.tab_completion_mode = true;
        app.input.set("String::re".to_string());

        refresh_completion(&mut app, &mut provider, Some(Instant::now()));
        poll_completion(&mut app, &mut provider);

        let labels = app
            .completion
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert!(
            labels.contains(&"String::repeat"),
            "qualified TUI completion should use engine candidates: {labels:?}"
        );
        let telemetry = app
            .last_completion_telemetry
            .as_ref()
            .expect("completion telemetry should be recorded");
        assert!(
            telemetry.completion_queue_ns.is_some_and(|value| value > 0),
            "completion telemetry should include queue timing: {telemetry:?}"
        );
        assert!(
            telemetry
                .completion_compute_ns
                .is_some_and(|value| value > 0),
            "completion telemetry should include compute timing: {telemetry:?}"
        );
        assert!(
            telemetry.completion_apply_ns.is_some_and(|value| value > 0),
            "completion telemetry should include apply timing: {telemetry:?}"
        );
        assert!(
            telemetry
                .completion_render_ns
                .is_some_and(|value| value > 0),
            "completion telemetry should include render timing: {telemetry:?}"
        );
        assert!(
            telemetry
                .total_key_to_visible_response_ns
                .is_some_and(|value| value > 0),
            "completion telemetry should include total timing: {telemetry:?}"
        );
    }

    #[test]
    fn completion_apply_replaces_only_engine_reported_range() {
        let engine = ReplEngine::new().expect("REPL engine should bootstrap");
        let mut app = App::new();
        let mut provider = ImmediateProvider::new(&engine);
        app.tab_completion_mode = true;
        app.input.set("foo=Str".to_string());

        refresh_completion(&mut app, &mut provider, None);
        poll_completion(&mut app, &mut provider);
        assert!(app.completion.visible, "completion should be visible");
        app.completion.selected = app
            .completion
            .items
            .iter()
            .position(|item| item.label == "String")
            .expect("String completion should be present");

        let mut buf = app.input.clone();
        app.completion.apply(&mut buf);
        assert_eq!(buf.text, "foo=String");
    }

    #[test]
    fn poll_completion_discards_stale_generation_results() {
        let engine = ReplEngine::new().expect("REPL engine should bootstrap");
        let mut app = App::new();
        let mut provider = ImmediateProvider::new(&engine);
        app.tab_completion_mode = true;

        app.input.set("St".to_string());
        refresh_completion(&mut app, &mut provider, None);
        app.input.set("String::re".to_string());
        refresh_completion(&mut app, &mut provider, None);
        poll_completion(&mut app, &mut provider);

        let labels = app
            .completion
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert!(
            labels.contains(&"String::repeat"),
            "latest generation should win over stale completions: {labels:?}"
        );
        assert!(
            !labels.contains(&"String"),
            "stale short-prefix completions should not overwrite newer input: {labels:?}"
        );
    }

    #[test]
    fn typing_does_not_show_completion_until_tab_mode_is_enabled() {
        let mut engine = ReplEngine::new().expect("REPL engine should bootstrap");
        let mut app = App::new();
        let mut provider = ImmediateProvider::new(&engine);

        handle_key(
            &mut app,
            &mut engine,
            &mut provider,
            KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE),
            None,
        );
        poll_completion(&mut app, &mut provider);

        assert!(
            !app.completion.visible,
            "completion should stay hidden until tab mode is enabled"
        );
        assert!(
            !app.tab_completion_mode,
            "plain typing should not enable tab mode"
        );
    }

    #[test]
    fn tab_enters_completion_mode_and_requests_candidates() {
        let mut engine = ReplEngine::new().expect("REPL engine should bootstrap");
        let mut app = App::new();
        let mut provider = ImmediateProvider::new(&engine);
        app.input.set("String::re".to_string());

        handle_key(
            &mut app,
            &mut engine,
            &mut provider,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            None,
        );
        poll_completion(&mut app, &mut provider);

        assert!(app.tab_completion_mode, "tab should enable completion mode");
        assert!(
            app.completion.visible,
            "tab completion mode should show candidates"
        );
    }
}

// ── Submission ────────────────────────────────────────────────────────────────

pub(super) fn submit_input(
    app: &mut App,
    engine: &mut ReplEngine,
    provider: &mut dyn ReplCompletionProvider,
) {
    let source = app.input.text.trim().to_string();
    if source.is_empty() {
        return;
    }

    app.input.clear();
    app.completion.clear();
    app.completion_controller.cancel_pending();
    app.tab_completion_mode = false;
    app.last_completion_telemetry = None;

    let presented = present_for_interaction(engine.handle_line(&source));
    provider.replace_context(engine.completion_context());
    match presented.event {
        PresentedEvent::None => {}
        PresentedEvent::Result(result) => app.push_result(
            &source,
            result.stdout_lines,
            result.lines,
            result.stderr_lines,
            result.kind,
        ),
        PresentedEvent::Doc(doc) => app.push_doc(doc),
    }
    if presented.should_exit {
        app.should_quit = true;
    }
    app.update_status();
}

pub(super) fn submit_command(
    app: &mut App,
    engine: &mut ReplEngine,
    provider: &mut dyn ReplCompletionProvider,
) {
    let raw = app.command.text.trim().to_string();
    app.command.clear();
    app.input_mode = InputMode::Insert;
    app.completion.clear();
    app.completion_controller.cancel_pending();
    app.tab_completion_mode = false;
    app.update_status();

    if raw.is_empty() {
        return;
    }

    let mut parts = raw.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();

    match cmd {
        "q" | "quit" => app.should_quit = true,
        "help" | "save" | "doc" | "error" | "sig" | "info" | "type" | "facet" => {
            let line = if arg.is_empty() {
                format!(":{cmd}")
            } else {
                format!(":{cmd} {arg}")
            };
            let presented = present_for_interaction(engine.handle_line(&line));
            provider.replace_context(engine.completion_context());
            match presented.event {
                PresentedEvent::Result(result) => {
                    app.push_result(
                        line,
                        result.stdout_lines,
                        result.lines,
                        result.stderr_lines,
                        result.kind,
                    );
                }
                PresentedEvent::Doc(doc) => app.push_doc(doc),
                PresentedEvent::None => {}
            }
        }
        "v" => {
            // Recall source of a previous result into the input buffer.
            match arg.parse::<usize>() {
                Ok(idx) => {
                    if let Some(entry) = app.results.iter().find(|e| e.idx == idx) {
                        let src = entry.source.clone();
                        app.input.set(src);
                    } else {
                        app.push_result(
                            format!(":v {arg}"),
                            Vec::new(),
                            vec![format!("no result with idx {arg}")],
                            Vec::new(),
                            PresentedResultKind::EvalError,
                        );
                    }
                }
                _ => {
                    app.push_result(
                        format!(":v {arg}"),
                        Vec::new(),
                        vec![format!("invalid index: {arg}")],
                        Vec::new(),
                        PresentedResultKind::EvalError,
                    );
                }
            }
        }
        "j" => {
            if let Ok(idx) = arg.parse::<usize>() {
                if let Some(pos) = app.results.iter().position(|e| e.idx == idx) {
                    app.selected_result = Some(idx);
                    let line_offset: usize = app
                        .results
                        .iter()
                        .take(pos)
                        .map(|e| {
                            3 + e.stdout_lines.len() + e.rendered_lines.len() + e.stderr_lines.len()
                        })
                        .sum();
                    app.results_scroll = line_offset;
                } else {
                    app.push_result(
                        format!(":j {arg}"),
                        Vec::new(),
                        vec![format!("no result with idx {arg}")],
                        Vec::new(),
                        PresentedResultKind::EvalError,
                    );
                }
            }
        }
        "doc-focus" => {
            if let Ok(idx) = arg.parse::<usize>() {
                if idx < app.docs.len() {
                    app.selected_doc = Some(idx);
                }
            }
        }
        "doc-drop" => {
            if let Ok(idx) = arg.parse::<usize>() {
                if idx < app.docs.len() {
                    app.docs.remove(idx);
                    for (i, d) in app.docs.iter_mut().enumerate() {
                        d.idx = i;
                    }
                    app.selected_doc = app.selected_doc.filter(|&s| s < app.docs.len());
                }
            }
        }
        "doc-clear" => {
            app.docs.clear();
            app.selected_doc = None;
        }
        other => {
            app.push_result(
                format!(":{other}"),
                Vec::new(),
                vec![format!("unknown command: {other}")],
                Vec::new(),
                PresentedResultKind::EvalError,
            );
        }
    }
}
