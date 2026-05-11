/// A parsed REPL command (lines beginning with `:`).
pub enum ReplCommand {
    Quit,
    Help { topic: Option<String> },
    Doc { symbol: String },
    Sig { symbol: String },
    Info { query: String },
    Type { symbol: String },
    Facet { query: String },
    Error { mode: Option<String> },
    ValueRecall { arg: String },
    Save { path: String },
    Vars,
    Imported,
    Defs,
    History { selector: Option<String> },
    Reload { mode: Option<String> },
    Clear,
    Unknown { raw: String },
}

/// Parse a line that starts with `:` into a `ReplCommand`.
///
/// The `trimmed` argument must already have leading/trailing whitespace removed.
/// Returns `None` if the line does not start with `:`.
pub fn parse_repl_command(trimmed: &str) -> Option<ReplCommand> {
    if !trimmed.starts_with(':') {
        return None;
    }
    let body = &trimmed[1..]; // drop the leading ':'
    let (cmd, rest) = body
        .split_once(char::is_whitespace)
        .map(|(c, r)| (c, r.trim()))
        .unwrap_or((body, ""));

    let command = match cmd {
        "quit" | "exit" | "q" => ReplCommand::Quit,
        "help" | "h" => ReplCommand::Help {
            topic: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        "doc" => ReplCommand::Doc {
            symbol: rest.to_string(),
        },
        "sig" => ReplCommand::Sig {
            symbol: rest.to_string(),
        },
        "info" => ReplCommand::Info {
            query: rest.to_string(),
        },
        "type" => ReplCommand::Type {
            symbol: rest.to_string(),
        },
        "facet" => ReplCommand::Facet {
            query: rest.to_string(),
        },
        "error" => ReplCommand::Error {
            mode: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        "v" => ReplCommand::ValueRecall {
            arg: rest.to_string(),
        },
        "save" => ReplCommand::Save {
            path: rest.to_string(),
        },
        "vars" => ReplCommand::Vars,
        "imported" => ReplCommand::Imported,
        "defs" => ReplCommand::Defs,
        "history" => ReplCommand::History {
            selector: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        "reload" => ReplCommand::Reload {
            mode: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        "clear" => ReplCommand::Clear,
        other => ReplCommand::Unknown {
            raw: format!(
                ":{}{}",
                other,
                if rest.is_empty() {
                    String::new()
                } else {
                    format!(" {rest}")
                }
            ),
        },
    };
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::{parse_repl_command, ReplCommand};

    #[test]
    fn parse_error_command_without_mode() {
        let parsed = parse_repl_command(":error").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Error { mode: None }));
    }

    #[test]
    fn parse_error_command_with_mode() {
        let parsed = parse_repl_command(":error summary").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Error { mode: Some(mode) } if mode == "summary"
        ));
    }

    #[test]
    fn parse_exit_command_as_quit_alias() {
        let parsed = parse_repl_command(":exit").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Quit));
    }

    #[test]
    fn parse_help_commands_without_topic() {
        let parsed = parse_repl_command(":help").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Help { topic: None }));

        let parsed = parse_repl_command(":h").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Help { topic: None }));
    }

    #[test]
    fn parse_help_doc_topic() {
        let parsed = parse_repl_command(":h doc").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Help { topic: Some(topic) } if topic == "doc"
        ));

        let parsed = parse_repl_command(":h :doc").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Help { topic: Some(topic) } if topic == ":doc"
        ));
    }

    #[test]
    fn parse_sig_command_with_symbol() {
        let parsed = parse_repl_command(":sig add").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Sig { symbol } if symbol == "add"
        ));
    }

    #[test]
    fn parse_type_command_with_symbol() {
        let parsed = parse_repl_command(":type list").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Type { symbol } if symbol == "list"
        ));
    }

    #[test]
    fn parse_info_command_with_query() {
        let parsed = parse_repl_command(":info ret |>= up").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Info { query } if query == "ret |>= up"
        ));
    }

    #[test]
    fn parse_facet_command_with_query() {
        let parsed = parse_repl_command(":facet path").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Facet { query } if query == "path"
        ));
    }

    #[test]
    fn parse_vars_command() {
        let parsed = parse_repl_command(":vars").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Vars));
    }

    #[test]
    fn parse_imported_command() {
        let parsed = parse_repl_command(":imported").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Imported));
    }

    #[test]
    fn parse_defs_command() {
        let parsed = parse_repl_command(":defs").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Defs));
    }

    #[test]
    fn parse_history_command_without_selector() {
        let parsed = parse_repl_command(":history").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::History { selector: None }));
    }

    #[test]
    fn parse_history_command_with_selector() {
        let parsed = parse_repl_command(":history 1..3").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::History {
                selector: Some(selector)
            } if selector == "1..3"
        ));
    }

    #[test]
    fn parse_reload_command_without_mode() {
        let parsed = parse_repl_command(":reload").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Reload { mode: None }));
    }

    #[test]
    fn parse_reload_command_with_mode() {
        let parsed = parse_repl_command(":reload defs").expect("command should parse");
        assert!(matches!(
            parsed,
            ReplCommand::Reload { mode: Some(mode) } if mode == "defs"
        ));
    }

    #[test]
    fn parse_clear_command() {
        let parsed = parse_repl_command(":clear").expect("command should parse");
        assert!(matches!(parsed, ReplCommand::Clear));
    }
}
