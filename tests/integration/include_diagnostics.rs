use crate::common::{repo_root, surtr_command, unique_temp_dir, write_source};
use serde_json::Value;
use std::fs;

fn check_error(temp: &std::path::Path, command: &str) -> Value {
    let output = surtr_command()
        .current_dir(temp)
        .args([command, "entry.srt", "--format", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "expected rejection");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid JSON: {e}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    if command == "test" {
        report["errors"][0]["diagnostic"]["errors"][0].clone()
    } else {
        report["errors"][0].clone()
    }
}

#[test]
fn test_command_include_rejects_user_builtin_with_include_primary_and_related_declaration() {
    let temp = unique_temp_dir("include_builtin_diagnostic");
    write_source(
        &temp.join("allowed.srt"),
        "defmod Allowed { def value() -> Int { 1 } }",
    );
    write_source(
        &temp.join("copy.srt"),
        &fs::read_to_string(repo_root().join("lib/test.srt")).unwrap(),
    );
    let source = "include './allowed.srt'\ninclude './copy.srt'\n()\n";
    write_source(&temp.join("entry.srt"), source);
    for command in ["check", "test"] {
        let error = check_error(&temp, command);
        assert_eq!(
            error["message"],
            "Cannot include './copy.srt': @builtin is not allowed in user-defined source."
        );
        assert_eq!(error["phase"], "parse");
        assert_eq!(error["reason"], "UserBuiltinDeclaration");
        assert_eq!(error["span"], serde_json::json!([24, 44]));
        assert!(error["related"].as_array().unwrap().len() >= 2, "{error}");
    }
    let output = surtr_command()
        .current_dir(&temp)
        .args(["run", "entry.srt"])
        .output()
        .unwrap();
    let human = String::from_utf8_lossy(&output.stderr);
    assert!(human.contains("Cannot include './copy.srt'"), "{human}");
    assert!(human.contains("copy.srt"), "{human}");
    assert!(human.contains("include './copy.srt'"), "{human}");
    let output = surtr_command()
        .current_dir(&temp)
        .args(["repl", "--quiet", "--script", "entry.srt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let human = String::from_utf8_lossy(&output.stderr);
    assert!(human.contains("Cannot include './copy.srt'"), "{human}");
    assert!(human.contains("include './copy.srt'"), "{human}");
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn test_command_include_reports_existing_duration_as_collision() {
    let temp = unique_temp_dir("include_duration_diagnostic");
    let duration = repo_root().join("lib/types/duration.srt");
    let source = format!("include '{}'\n()\n", duration.display());
    write_source(&temp.join("entry.srt"), &source);
    let error = check_error(&temp, "check");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("symbol 'Duration' is already defined"),
        "{error}"
    );
    assert_eq!(error["phase"], "resolve");
    assert_eq!(error["line"], 1);
    assert!(error["related"].as_array().unwrap().len() >= 3, "{error}");
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn test_command_include_only_relabels_structured_policy_restrictions() {
    let temp = unique_temp_dir("include_policy_diagnostic");
    write_source(&temp.join("entry.srt"), "include './module.srt'\n()\n");
    write_source(&temp.join("module.srt"), "@FacetPathKind Type Example\n");
    let error = check_error(&temp, "check");
    assert_eq!(
        error["message"],
        "Cannot include './module.srt': @FacetPathKind Type is not allowed in user-defined source."
    );
    for source in ["def helper() -> Int { 1 }", "1"] {
        write_source(&temp.join("module.srt"), source);
        let error = check_error(&temp, "check");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .starts_with("Cannot include './module.srt':"),
            "{error}"
        );
        assert_eq!(error["reason"], "SourcePolicy");
    }
    write_source(
        &temp.join("module.srt"),
        "defmod Module { def value() -> Int { ) } }",
    );
    let error = check_error(&temp, "check");
    assert!(
        !error["message"]
            .as_str()
            .unwrap()
            .contains("Cannot include"),
        "{error}"
    );
    write_source(&temp.join("module.srt"), "# @builtin and @FacetPathKind Type are text\nType Callback = (Int -> Int)\ndefmod Module { def text() -> String { \"@builtin Type\" } }");
    let output = surtr_command()
        .current_dir(&temp)
        .args(["check", "entry.srt"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn test_command_include_does_not_identify_standard_sources_by_path() {
    let temp = unique_temp_dir("include_path_identity");
    let original = repo_root().join("lib/test.srt");
    fs::copy(&original, temp.join("arbitrary.srt")).unwrap();
    let mut paths = vec![
        "./arbitrary.srt".to_string(),
        original.to_string_lossy().into_owned(),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&original, temp.join("linked.srt")).unwrap();
        paths.push("./linked.srt".into());
    }
    for path in paths {
        write_source(&temp.join("entry.srt"), &format!("include '{path}'\n()\n"));
        for command in ["check", "test"] {
            let error = check_error(&temp, command);
            assert_eq!(
                error["message"],
                format!("Cannot include '{path}': @builtin is not allowed in user-defined source.")
            );
            assert_eq!(error["reason"], "UserBuiltinDeclaration");
        }
    }
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn test_command_include_repeated_file_uses_the_conflicting_occurrence() {
    let temp = unique_temp_dir("include_repeated_occurrence");
    write_source(
        &temp.join("module.srt"),
        "defmod Helper { def value() -> Int { 1 } }",
    );
    write_source(
        &temp.join("entry.srt"),
        "include './module.srt'\ninclude './module.srt'\ninclude './module.srt'\n()\n",
    );
    for command in ["check", "test"] {
        let error = check_error(&temp, command);
        assert_eq!(
            error["message"],
            "Cannot include './module.srt': symbol 'Helper' is already defined."
        );
        assert_eq!(
            error["line"], 2,
            "the second occurrence causes the first collision: {error}"
        );
        assert_eq!(error["span"], serde_json::json!([23, 45]));
    }
    let output = surtr_command()
        .current_dir(&temp)
        .args(["repl", "--quiet", "--script", "entry.srt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let human = String::from_utf8_lossy(&output.stderr);
    assert!(human.contains("entry.srt:2:"), "{human}");
    fs::remove_dir_all(temp).unwrap();
}
