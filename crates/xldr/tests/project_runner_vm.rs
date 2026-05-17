use std::path::PathBuf;

use surtr_analysis::ProjectRunnerSourceInput;

#[test]
fn project_runner_vm_executes_standard_project_config_surface() {
    let input = ProjectRunnerSourceInput {
        project_file: PathBuf::from("/repo/project.srt"),
        selected_profile: "main".to_string(),
        normalized_args: vec![("profile".to_string(), "main".to_string())],
        active_file: None,
        source: r#"
Project::config({|project|
  Project::entrypoint(project, "main", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./main.srt")
    |> Config::add_path("./src/*.srt")
  })
})
"#
        .to_string(),
    };

    let result = xldr::execute_project_runner_source(input)
        .expect("project runner source should execute through VM");

    assert_eq!(result.profiles.len(), 1);
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
}
