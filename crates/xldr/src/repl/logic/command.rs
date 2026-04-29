/// A parsed REPL command (lines beginning with `:`).
pub enum ReplCommand {
    Quit,
    Doc { symbol: String },
    Error { mode: Option<String> },
    ValueRecall { arg: String },
    Save { path: String },
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
        "doc" => ReplCommand::Doc {
            symbol: rest.to_string(),
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
}
