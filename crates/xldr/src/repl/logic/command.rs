/// A parsed REPL command (lines beginning with `:`).
pub enum ReplCommand {
    Quit,
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
        "quit" | "q" => ReplCommand::Quit,
        "v" => ReplCommand::ValueRecall { arg: rest.to_string() },
        "save" => ReplCommand::Save { path: rest.to_string() },
        other => ReplCommand::Unknown { raw: format!(":{}{}", other, if rest.is_empty() { String::new() } else { format!(" {rest}") }) },
    };
    Some(command)
}
