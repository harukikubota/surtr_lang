mod commands;
mod compile;
mod error;
mod util;

use std::env;
use std::process;

use crate::error::RuneResult;

const RUNE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();
    let result = dispatch(&args);

    if let Err(error) = result {
        error.emit();
        process::exit(error.exit_code());
    }
}

fn dispatch(args: &[String]) -> RuneResult<()> {
    match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("surtr {}", RUNE_VERSION);
            Ok(())
        }
        Some("run") => commands::run::dispatch(&args[2..]),
        Some("repl") => commands::repl::dispatch(&args[2..]),
        Some("build") => commands::build::dispatch(&args[2..]),
        Some("test") => commands::test::dispatch(&args[2..]),
        Some("dump") => {
            let Some(file_path) = args.get(2) else {
                return Err(crate::error::RuneError::usage(String::new()));
            };
            commands::dump::dispatch(file_path, &args[3..])
        }
        Some("tui") => commands::tui::dispatch(&args[2..]),
        _ => Err(crate::error::RuneError::usage(String::new())),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn usage_text_mentions_dump_and_test() {
        assert!(crate::error::USAGE_TEXT.contains("surtr dump"));
        assert!(crate::error::USAGE_TEXT.contains("surtr test"));
    }
}
