pub mod command;
pub mod core;
pub mod output;
pub mod presenter;
pub mod query;
pub mod render;
pub mod styled;

pub use command::{parse_repl_command, ReplCommand};
pub use output::{ReplOutput, ReplResult};
pub use presenter::{
    present_for_cli, present_for_interaction, PresentedDoc, PresentedEvent, PresentedInteraction,
    PresentedResult, PresentedResultKind,
};
pub use query::{
    parse_repl_query, QueryArg, QueryArgKind, ReplQuery, ReplQueryParseError, TypedCallQuery,
    TypedOperatorQuery,
};
