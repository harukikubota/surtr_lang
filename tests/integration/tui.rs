use crate::common::{surtr_command, unique_temp_dir};

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

#[test]
fn tui_missing_preload_file_exits_non_zero_and_reports_stderr() {
    let temp = unique_temp_dir("surtr_tui_missing_preload");
    let missing = temp.join("missing.eldr");

    let output = surtr_command()
        .arg("tui")
        .arg(&missing)
        .current_dir(&temp)
        .output()
        .expect("failed to run surtr tui");

    assert!(
        !output.status.success(),
        "tui should fail for missing preload file\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    let exit_code = output.status.code();
    if stderr.contains("disabled in this build") {
        assert_eq!(exit_code, Some(2));
        assert!(stderr.contains("tui: disabled in this build"), "{stderr}");
    } else {
        assert_eq!(exit_code, Some(1));
        assert!(stderr.contains("tui: cannot read"), "{stderr}");
        assert!(stderr.contains("missing.eldr"), "{stderr}");
    }
}
