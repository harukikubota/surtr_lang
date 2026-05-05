use crate::common::{normalize_text, repo_root, surtr_command};

fn run_process_example(name: &str) -> String {
    let entry = repo_root().join(format!("examples/process/{name}/entry.srt"));
    let output = surtr_command()
        .args(["run", entry.to_str().expect("entry path must be utf-8")])
        .output()
        .unwrap_or_else(|err| panic!("failed to run process example {name}: {err}"));
    assert!(
        output.status.success(),
        "process example {name} should run\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    normalize_text(&String::from_utf8_lossy(&output.stdout))
}

#[test]
fn process_examples_keep_current_agent_and_task_surface() {
    let cases = [
        (
            "state_agent_singleton",
            r#"Ok("count:42")
Ok(())
Ok("count:100")
Err(NoneError("None Value."))
Ok("count:100")"#,
        ),
        (
            "agent_singleton_counter",
            r#"Ok("count=0")
Ok(())
Ok("count=3")
Ok(())
Ok("count=8")
Err(NoneError("None Value."))
Ok("count=8")"#,
        ),
        ("read_only_agent", r#"Ok("HOME=demo-home")"#),
        (
            "agent_worker_multi",
            r#"alpha: PID(Worker#0)
beta: PID(Worker#1)
Ok("jobs=3")
Ok("jobs=7")
Ok(())
Ok(())
Ok("jobs=2")
Ok("jobs=5")
Err(NoneError("None Value."))
Ok("jobs=2")
Err(NoneError("None Value."))"#,
        ),
        ("task_call", r#"Ok("task:42")"#),
    ];

    for (name, expected) in cases {
        assert_eq!(
            run_process_example(name),
            normalize_text(expected),
            "{name}"
        );
    }
}

#[test]
fn process_examples_cover_io_handler_swap_and_memoized_workers() {
    assert_eq!(
        run_process_example("io_handler_switch"),
        normalize_text(
            r#"Ok(())
logger output was suppressed"#
        )
    );
    assert_eq!(
        run_process_example("memoized_fib_workers"),
        normalize_text(
            r#"Ok(("miss-even", 46368))
Ok(("miss-odd", 75025))
Ok(("hit-even", 46368))
Ok(("hit-odd", 75025))"#
        )
    );
}
