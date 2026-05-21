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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplCommandKind {
    Quit,
    Help,
    Doc,
    Sig,
    Info,
    Type,
    Facet,
    Error,
    ValueRecall,
    Save,
    Vars,
    Imported,
    Defs,
    History,
    Reload,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplCommandArgCompletion {
    None,
    Semantic,
    CommandTopic,
    Fixed(&'static [&'static str]),
    ResultLine,
    HistorySelector,
    SavePath,
    TypeTarget,
    FacetTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplCommandSpec {
    pub kind: ReplCommandKind,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub summary: &'static str,
    pub detail_help: &'static [&'static str],
    pub arg_completion: ReplCommandArgCompletion,
}

impl ReplCommandSpec {
    pub fn help_line(self) -> String {
        format!("{}  {}", self.usage, self.summary)
    }

    pub fn completion_detail(self) -> String {
        self.help_line()
    }
}

const REPL_COMMAND_SPECS: &[ReplCommandSpec] = &[
    ReplCommandSpec {
        kind: ReplCommandKind::Help,
        aliases: &["help", "h"],
        usage: ":help, :h [command]",
        summary: "Show REPL help",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::CommandTopic,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Quit,
        aliases: &["quit", "exit", "q"],
        usage: ":quit, :exit, :q",
        summary: "Exit the REPL",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::None,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Doc,
        aliases: &["doc"],
        usage: ":doc <symbol|query>",
        summary: "Show documentation for visible symbols, including process surfaces",
        detail_help: &[
            "Usage: :doc <symbol|query>",
            "Examples: :doc print, :doc formatter, :doc Kernel::if, :doc GenServer::spawn, :doc MyServer::pid, :doc User(), :doc compare(Int, Int), :doc |*> Option",
        ],
        arg_completion: ReplCommandArgCompletion::Semantic,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Sig,
        aliases: &["sig"],
        usage: ":sig <symbol|query>",
        summary: "Show signatures for visible callable, family, owner, or process surfaces",
        detail_help: &[
            "Usage: :sig <symbol|query>",
            "Examples: :sig compare, :sig Compare, :sig User, :sig GenServer::spawn, :sig MyServer::pid, :sig compare(Int, Int), :sig |*> Option",
        ],
        arg_completion: ReplCommandArgCompletion::Semantic,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Info,
        aliases: &["info"],
        usage: ":info <query>",
        summary: "Show derived information for visible symbols, retained query targets, or process handles",
        detail_help: &[
            "Usage: :info <query>",
            "Accepts: symbol | singleton-owner | typed-call | operator-target",
            "Examples: :info print, :info Counter, :info pid, :info compare(Int, Int), :info |*> Option",
        ],
        arg_completion: ReplCommandArgCompletion::Semantic,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Type,
        aliases: &["type"],
        usage: ":type <binding>",
        summary: "Show the type for a visible binding or singleton process owner",
        detail_help: &[
            "Usage: :type <binding|singleton-owner>",
            "Examples: :type list, :type Counter, :type pid, :type my_closure",
            "Worker processes are queried through PID bindings; singleton processes are queried by owner name.",
        ],
        arg_completion: ReplCommandArgCompletion::TypeTarget,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Facet,
        aliases: &["facet"],
        usage: ":facet <FacetPath|binding>",
        summary: "Inspect a FacetPath and its API boundaries",
        detail_help: &[
            "Usage: :facet <FacetPath|binding>",
            "Examples: :facet path, :facet Tuple._1, :facet BitWidth.Any",
            "Shows canonical path, API availability, segment details, and where the path may stop.",
        ],
        arg_completion: ReplCommandArgCompletion::FacetTarget,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Error,
        aliases: &["error"],
        usage: ":error [full|summary]",
        summary: "Show or change error display mode",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::Fixed(&["full", "summary"]),
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Save,
        aliases: &["save"],
        usage: ":save <path.eldr>",
        summary: "Save the current session as .eldr",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::SavePath,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Vars,
        aliases: &["vars"],
        usage: ":vars",
        summary: "List visible value bindings",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::None,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Imported,
        aliases: &["imported"],
        usage: ":imported",
        summary: "List imports active in the REPL scope",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::None,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Defs,
        aliases: &["defs"],
        usage: ":defs",
        summary: "List visible top-level REPL defs",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::None,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::History,
        aliases: &["history"],
        usage: ":history [selector]",
        summary: "Show committed REPL input history",
        detail_help: &[
            "Usage: :history [selector]",
            "Examples: :history, :history 3, :history 1, 3, 5, :history 2..4",
        ],
        arg_completion: ReplCommandArgCompletion::HistorySelector,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Reload,
        aliases: &["reload"],
        usage: ":reload [all|defs]",
        summary: "Rebuild the REPL session from preload and defs",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::Fixed(&["all", "defs"]),
    },
    ReplCommandSpec {
        kind: ReplCommandKind::Clear,
        aliases: &["clear"],
        usage: ":clear",
        summary: "Clear the screen when the host supports it",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::None,
    },
    ReplCommandSpec {
        kind: ReplCommandKind::ValueRecall,
        aliases: &["v"],
        usage: ":v <line>",
        summary: "Recall a previous result",
        detail_help: &[],
        arg_completion: ReplCommandArgCompletion::ResultLine,
    },
];

pub fn repl_command_specs() -> &'static [ReplCommandSpec] {
    REPL_COMMAND_SPECS
}

pub fn repl_command_spec_for_alias(alias: &str) -> Option<ReplCommandSpec> {
    let alias = alias.strip_prefix(':').unwrap_or(alias);
    REPL_COMMAND_SPECS
        .iter()
        .find(|spec| spec.aliases.contains(&alias))
        .copied()
}

pub fn repl_command_help_lines() -> Vec<String> {
    std::iter::once("REPL commands:".to_string())
        .chain(REPL_COMMAND_SPECS.iter().map(|spec| spec.help_line()))
        .collect()
}

pub fn repl_command_topic_help_lines(topic: &str) -> Option<Vec<String>> {
    let topic = topic.strip_prefix(':').unwrap_or(topic);
    REPL_COMMAND_SPECS
        .iter()
        .find(|spec| spec.aliases.contains(&topic))
        .and_then(|spec| {
            (!spec.detail_help.is_empty()).then(|| {
                spec.detail_help
                    .iter()
                    .map(|line| (*line).to_string())
                    .collect()
            })
        })
}

fn find_repl_command_spec(cmd: &str) -> Option<ReplCommandSpec> {
    REPL_COMMAND_SPECS
        .iter()
        .copied()
        .find(|spec| spec.aliases.contains(&cmd))
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

    let Some(spec) = find_repl_command_spec(cmd) else {
        return Some(ReplCommand::Unknown {
            raw: format!(
                ":{}{}",
                cmd,
                if rest.is_empty() {
                    String::new()
                } else {
                    format!(" {rest}")
                }
            ),
        });
    };

    let command = match spec.kind {
        ReplCommandKind::Quit => ReplCommand::Quit,
        ReplCommandKind::Help => ReplCommand::Help {
            topic: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        ReplCommandKind::Doc => ReplCommand::Doc {
            symbol: rest.to_string(),
        },
        ReplCommandKind::Sig => ReplCommand::Sig {
            symbol: rest.to_string(),
        },
        ReplCommandKind::Info => ReplCommand::Info {
            query: rest.to_string(),
        },
        ReplCommandKind::Type => ReplCommand::Type {
            symbol: rest.to_string(),
        },
        ReplCommandKind::Facet => ReplCommand::Facet {
            query: rest.to_string(),
        },
        ReplCommandKind::Error => ReplCommand::Error {
            mode: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        ReplCommandKind::ValueRecall => ReplCommand::ValueRecall {
            arg: rest.to_string(),
        },
        ReplCommandKind::Save => ReplCommand::Save {
            path: rest.to_string(),
        },
        ReplCommandKind::Vars => ReplCommand::Vars,
        ReplCommandKind::Imported => ReplCommand::Imported,
        ReplCommandKind::Defs => ReplCommand::Defs,
        ReplCommandKind::History => ReplCommand::History {
            selector: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        ReplCommandKind::Reload => ReplCommand::Reload {
            mode: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        },
        ReplCommandKind::Clear => ReplCommand::Clear,
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
