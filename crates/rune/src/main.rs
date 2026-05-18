mod commands;
mod compile;
mod error;
mod run_cache;
mod util;

use std::env;
use std::path::Path;
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
    let normalized_args = normalize_dispatch_args(args);
    match normalized_args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("surtr {}", RUNE_VERSION);
            Ok(())
        }
        Some("check") => commands::check::dispatch(&normalized_args[2..]),
        Some("run") => commands::run::dispatch(&normalized_args[2..]),
        Some("repl") => commands::repl::dispatch(&normalized_args[2..]),
        Some("build") => commands::build::dispatch(&normalized_args[2..]),
        Some("test") => commands::test::dispatch(&normalized_args[2..]),
        Some("dump") => {
            let Some(file_path) = normalized_args.get(2) else {
                return Err(crate::error::RuneError::usage(String::new()));
            };
            if file_path.starts_with('-') {
                return Err(crate::error::RuneError::message(
                    1,
                    format!("dump: unknown option '{}'", file_path),
                ));
            }
            commands::dump::dispatch(file_path, &normalized_args[3..])
        }
        Some("tui") => commands::tui::dispatch(&normalized_args[2..]),
        _ => Err(crate::error::RuneError::usage(String::new())),
    }
}

fn normalize_dispatch_args(args: &[String]) -> Vec<String> {
    let Some(command_or_path) = args.get(1) else {
        return args.to_vec();
    };

    let is_known_command = matches!(
        command_or_path.as_str(),
        "--version" | "check" | "run" | "repl" | "build" | "test" | "dump" | "tui"
    );
    if is_known_command || !Path::new(command_or_path).exists() {
        return args.to_vec();
    }

    let mut normalized_args = Vec::with_capacity(args.len() + 1);
    normalized_args.push(args[0].clone());
    normalized_args.push("run".to_string());
    normalized_args.extend(args[1..].iter().cloned());
    normalized_args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_text_mentions_dump_and_test() {
        assert!(crate::error::USAGE_TEXT.contains("surtr dump"));
        assert!(crate::error::USAGE_TEXT.contains("surtr test"));
        assert!(crate::error::USAGE_TEXT.contains("surtr check"));
    }

    #[test]
    fn dispatch_treats_existing_file_as_run_target() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!(
            "surtr-dispatch-{}-{}.srt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock must be after unix epoch")
                .as_nanos()
        ));
        std::fs::write(&file_path, "print(\"Hello world!\")\n")
            .expect("temporary script must be writable");

        let args = vec![
            "surtr".to_string(),
            file_path.to_string_lossy().into_owned(),
        ];
        let result = dispatch(&args);

        std::fs::remove_file(&file_path).expect("temporary script must be removable");
        assert!(
            result.is_ok(),
            "existing script path should be routed through run dispatch"
        );
    }

    #[test]
    fn dump_rejects_option_like_input() {
        let err = dispatch(&[
            "surtr".to_string(),
            "dump".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .expect_err("option-looking dump input must fail before reading input");

        assert_eq!(err.summary(), "dump: unknown option '--format'");
    }
}
