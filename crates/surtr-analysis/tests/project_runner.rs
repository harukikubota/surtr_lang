use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use surtr_analysis::{
    extract_project_runner_input, resolve_project_runner, AnalysisSpan, DeclaredProjectPath,
    ProjectBootSummary, ProjectRunnerInput, ProjectRunnerSourceInput, RunnerDiagnosticKind,
};

#[test]
fn project_runner_extracts_selected_profile_paths_from_project_source() {
    let root = temp_root("source");
    let source = r#"
Project::config({|config|
  Project::entrypoint(config, "main", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./main.srt")
    |> Config::add_path("./src/*.srt")
  })

  Project::entrypoint(config, "test", {|c|
    Config::entry_fun(c, "Main::test")
    |> Config::add_path("./test.srt")
  })
})
"#;

    let input = extract_project_runner_input(ProjectRunnerSourceInput {
        project_file: root.join("project.srt"),
        selected_profile: "main".to_string(),
        normalized_args: vec![("profile".to_string(), "main".to_string())],
        source: source.to_string(),
    })
    .expect("project source should extract runner input");

    assert_eq!(input.selected_profile, "main");
    assert_eq!(
        input
            .declared_paths
            .iter()
            .map(|path| path.literal_or_glob.as_str())
            .collect::<Vec<_>>(),
        vec!["./main.srt", "./src/*.srt"]
    );
    assert_eq!(input.active_file_profiles, vec!["main", "test"]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_runner_extracts_paths_from_repository_project_example() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/mahjong");
    let source = fs::read_to_string(root.join("project.srt")).expect("read example project");

    let input = extract_project_runner_input(ProjectRunnerSourceInput {
        project_file: root.join("project.srt"),
        selected_profile: "main".to_string(),
        normalized_args: Vec::new(),
        source,
    })
    .expect("repository project example should extract runner input");

    assert_eq!(input.active_file_profiles, vec!["main"]);
    assert_eq!(input.declared_paths.len(), 11);
    assert_eq!(input.declared_paths[0].literal_or_glob, "./main.srt");
    assert_eq!(input.declared_paths[10].literal_or_glob, "./src/6_cli.srt");
}

#[test]
fn project_runner_reports_unknown_selected_profile_from_project_source() {
    let root = temp_root("unknown-profile");
    let source = r#"
Project::config({|config|
  Project::entrypoint(config, "main", {|c|
    Config::add_path(c, "./main.srt")
  })
})
"#;

    let diagnostics = extract_project_runner_input(ProjectRunnerSourceInput {
        project_file: root.join("project.srt"),
        selected_profile: "test".to_string(),
        normalized_args: Vec::new(),
        source: source.to_string(),
    })
    .expect_err("unknown profile should be reported as runner diagnostic");

    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == RunnerDiagnosticKind::ProjectProfileUnknown));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_runner_resolves_literal_paths_into_module_stage() {
    let root = temp_root("literal");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("user.srt"), "def user() -> Int { 1 }").expect("write source");

    let runner = resolve_project_runner(ProjectRunnerInput {
        project_file: root.join("project.srt"),
        selected_profile: "dev".to_string(),
        normalized_args: vec![("profile".to_string(), "dev".to_string())],
        declared_paths: vec![DeclaredProjectPath {
            declared_by: root.join("project.srt"),
            literal_or_glob: "src/user.srt".to_string(),
            declaration_span: Some(AnalysisSpan { start: 4, end: 18 }),
        }],
        active_file_profiles: vec!["dev".to_string()],
        boot_summary: ProjectBootSummary::default(),
        external_inputs: Vec::new(),
    });

    assert_eq!(runner.selected_profile, "dev");
    assert!(runner.diagnostics.is_empty());
    assert_eq!(
        runner.resolved_paths[0].expanded_files,
        vec![src.join("user.srt")]
    );
    assert_eq!(runner.module_stages[0].files.len(), 1);
    assert_eq!(runner.module_stages[0].files[0].path, src.join("user.srt"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_runner_expands_globs_in_stable_path_order() {
    let root = temp_root("glob");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("zeta.srt"), "z = 1").expect("write zeta");
    fs::write(src.join("alpha.srt"), "a = 1").expect("write alpha");
    fs::write(src.join("skip.txt"), "ignored").expect("write txt");

    let runner = resolve_project_runner(ProjectRunnerInput {
        project_file: root.join("project.srt"),
        selected_profile: "test".to_string(),
        normalized_args: Vec::new(),
        declared_paths: vec![DeclaredProjectPath {
            declared_by: root.join("project.srt"),
            literal_or_glob: "src/*.srt".to_string(),
            declaration_span: None,
        }],
        active_file_profiles: vec!["test".to_string()],
        boot_summary: ProjectBootSummary::default(),
        external_inputs: Vec::new(),
    });

    assert_eq!(
        runner.resolved_paths[0].expanded_files,
        vec![src.join("alpha.srt"), src.join("zeta.srt")]
    );
    assert_eq!(runner.module_stages[0].files[0].path, src.join("alpha.srt"));
    assert_eq!(runner.module_stages[0].files[1].path, src.join("zeta.srt"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_runner_reports_glob_no_match_as_runner_diagnostic() {
    let root = temp_root("no-match");
    fs::create_dir_all(root.join("src")).expect("create src");

    let runner = resolve_project_runner(ProjectRunnerInput {
        project_file: root.join("project.srt"),
        selected_profile: "test".to_string(),
        normalized_args: Vec::new(),
        declared_paths: vec![DeclaredProjectPath {
            declared_by: root.join("project.srt"),
            literal_or_glob: "src/*.srt".to_string(),
            declaration_span: Some(AnalysisSpan { start: 10, end: 21 }),
        }],
        active_file_profiles: Vec::new(),
        boot_summary: ProjectBootSummary::default(),
        external_inputs: Vec::new(),
    });

    assert!(runner.resolved_paths[0].expanded_files.is_empty());
    assert!(runner
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == RunnerDiagnosticKind::GlobNoMatch));

    let _ = fs::remove_dir_all(root);
}

fn temp_root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("surtr-analysis-{name}-{nonce}"))
}
