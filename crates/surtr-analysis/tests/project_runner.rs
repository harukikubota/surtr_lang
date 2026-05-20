use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use sindr::runtime::{ListHandle, TypeEntry, TypeKind, TypeRegistry, Value};
use surtr_analysis::{
    decode_project_runner_value, extract_project_runner_input, extract_project_runner_result,
    resolve_project_runner, AnalysisSpan, DeclaredProjectPath, ProjectBootSummary,
    ProjectRunnerInput, ProjectRunnerSourceInput, RunnerDiagnosticKind,
};

#[test]
fn project_runner_extracts_vm_result_shaped_profiles_from_project_source() {
    let root = temp_root("runner-result");
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

    let result = extract_project_runner_result(ProjectRunnerSourceInput {
        project_file: root.join("project.srt"),
        selected_profile: "main".to_string(),
        normalized_args: Vec::new(),
        active_file: None,
        source: source.to_string(),
    })
    .expect("project source should extract runner result");

    assert_eq!(result.profiles.len(), 2);
    assert_eq!(result.profiles[0].name, "main");
    assert_eq!(result.profiles[0].entrypoint, "Main::main");
    assert_eq!(
        result.profiles[0]
            .paths
            .iter()
            .map(|path| path.literal_or_glob.as_str())
            .collect::<Vec<_>>(),
        vec!["./main.srt", "./src/*.srt"]
    );
    assert_eq!(result.profiles[1].name, "test");
    assert_eq!(result.profiles[1].entrypoint, "Main::test");
    assert_eq!(
        result.boot_summary.fields,
        vec![
            (
                "profile.main.entrypoint".to_string(),
                "Main::main".to_string()
            ),
            (
                "profile.test.entrypoint".to_string(),
                "Main::test".to_string()
            ),
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_runner_decodes_vm_project_value_into_runner_result() {
    let root = temp_root("runner-value");
    let registry = project_value_registry();
    let value = Value::Tagged {
        tag: 2,
        fields: vec![Value::List(ListHandle::from_items(vec![
            Value::Tagged {
                tag: 3,
                fields: vec![
                    Value::Str("main".to_string()),
                    Value::Str("Main::main".to_string()),
                    Value::List(ListHandle::from_items(vec![
                        Value::Str("./main.srt".to_string()),
                        Value::Str("./src/*.srt".to_string()),
                    ])),
                ],
            },
            Value::Tagged {
                tag: 3,
                fields: vec![
                    Value::Str("test".to_string()),
                    Value::Str("Main::test".to_string()),
                    Value::List(ListHandle::from_items(vec![Value::Str(
                        "./test.srt".to_string(),
                    )])),
                ],
            },
        ]))],
    };

    let result = decode_project_runner_value(&root.join("project.srt"), &value, &registry)
        .expect("VM Project value should decode");

    assert_eq!(result.profiles.len(), 2);
    assert_eq!(result.profiles[0].name, "main");
    assert_eq!(result.profiles[0].entrypoint, "Main::main");
    assert_eq!(
        result.profiles[0]
            .paths
            .iter()
            .map(|path| path.literal_or_glob.as_str())
            .collect::<Vec<_>>(),
        vec!["./main.srt", "./src/*.srt"]
    );
    assert_eq!(
        result.profiles[0].paths[0].declared_by,
        root.join("project.srt")
    );
    assert_eq!(
        result.boot_summary.fields,
        vec![
            (
                "profile.main.entrypoint".to_string(),
                "Main::main".to_string()
            ),
            (
                "profile.test.entrypoint".to_string(),
                "Main::test".to_string()
            ),
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_runner_rejects_vm_value_with_non_string_path() {
    let root = temp_root("runner-value-bad-path");
    let registry = project_value_registry();
    let value = Value::Tagged {
        tag: 2,
        fields: vec![Value::List(ListHandle::from_items(vec![Value::Tagged {
            tag: 3,
            fields: vec![
                Value::Str("main".to_string()),
                Value::Str("Main::main".to_string()),
                Value::List(ListHandle::from_items(vec![Value::Int(1.into())])),
            ],
        }]))],
    };

    let error = decode_project_runner_value(&root.join("project.srt"), &value, &registry)
        .expect_err("non-string path should be rejected");

    assert!(error.message().contains("Config.paths"));

    let _ = fs::remove_dir_all(root);
}

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
        active_file: None,
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
        active_file: None,
        source,
    })
    .expect("repository project example should extract runner input");

    assert_eq!(input.active_file_profiles, vec!["main"]);
    assert_eq!(input.declared_paths.len(), 11);
    assert_eq!(input.declared_paths[0].literal_or_glob, "./main.srt");
    assert_eq!(input.declared_paths[10].literal_or_glob, "./src/6_cli.srt");
}

#[test]
fn project_runner_active_file_profiles_match_expanded_project_paths() {
    let root = temp_root("active-profile");
    fs::create_dir_all(root.join("src")).expect("create src");
    fs::write(root.join("src/shared.srt"), "shared = 1").expect("write shared");
    fs::write(root.join("src/dev_only.srt"), "dev_only = 1").expect("write dev");
    let source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/*.srt")
  })

  Project::entrypoint(config, "test", {|c|
    Config::add_path(c, "./src/shared.srt")
  })
})
"#;

    let input = extract_project_runner_input(ProjectRunnerSourceInput {
        project_file: root.join("project.srt"),
        selected_profile: "dev".to_string(),
        normalized_args: Vec::new(),
        active_file: Some(root.join("src/shared.srt")),
        source: source.to_string(),
    })
    .expect("project source should extract runner input");

    assert_eq!(input.active_file_profiles, vec!["dev", "test"]);

    let input = extract_project_runner_input(ProjectRunnerSourceInput {
        project_file: root.join("project.srt"),
        selected_profile: "dev".to_string(),
        normalized_args: Vec::new(),
        active_file: Some(root.join("src/dev_only.srt")),
        source: source.to_string(),
    })
    .expect("project source should extract runner input");

    assert_eq!(input.active_file_profiles, vec!["dev"]);

    let _ = fs::remove_dir_all(root);
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
        active_file: None,
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
        entrypoint: "Main::main".to_string(),
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
        entrypoint: "Main::main".to_string(),
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
fn project_runner_expands_recursive_globs_in_stable_path_order() {
    let root = temp_root("recursive-glob");
    let src = root.join("src");
    let nested = src.join("nested");
    let deep = nested.join("deep");
    fs::create_dir_all(&deep).expect("create nested src");
    fs::write(src.join("root.srt"), "root = 1").expect("write root");
    fs::write(nested.join("middle.srt"), "middle = 1").expect("write middle");
    fs::write(deep.join("leaf.srt"), "leaf = 1").expect("write leaf");
    fs::write(deep.join("skip.txt"), "skip").expect("write skip");

    let runner = resolve_project_runner(ProjectRunnerInput {
        project_file: root.join("project.srt"),
        selected_profile: "test".to_string(),
        entrypoint: "Main::main".to_string(),
        normalized_args: Vec::new(),
        declared_paths: vec![DeclaredProjectPath {
            declared_by: root.join("project.srt"),
            literal_or_glob: "src/**/*.srt".to_string(),
            declaration_span: None,
        }],
        active_file_profiles: vec!["test".to_string()],
        boot_summary: ProjectBootSummary::default(),
        external_inputs: Vec::new(),
    });

    assert_eq!(
        runner.resolved_paths[0].expanded_files,
        vec![
            deep.join("leaf.srt"),
            nested.join("middle.srt"),
            src.join("root.srt"),
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn project_runner_reports_glob_no_match_as_runner_diagnostic() {
    let root = temp_root("no-match");
    fs::create_dir_all(root.join("src")).expect("create src");

    let runner = resolve_project_runner(ProjectRunnerInput {
        project_file: root.join("project.srt"),
        selected_profile: "test".to_string(),
        entrypoint: "Main::main".to_string(),
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

fn project_value_registry() -> TypeRegistry {
    TypeRegistry::from_entries(vec![
        TypeEntry {
            tag: 2,
            name: "Project".to_string(),
            kind: TypeKind::Struct,
            field_names: vec!["entries".to_string()],
            private_flags: vec![false],
        },
        TypeEntry {
            tag: 3,
            name: "Config".to_string(),
            kind: TypeKind::Struct,
            field_names: vec![
                "name".to_string(),
                "entrypoint".to_string(),
                "paths".to_string(),
            ],
            private_flags: vec![false, false, false],
        },
    ])
}
