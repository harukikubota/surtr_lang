use std::path::PathBuf;

use surtr_analysis::ProjectRunnerSourceInput;

#[test]
fn execute_project_runner_source_decodes_vm_computed_profile() {
    let project_file = PathBuf::from("/repo/project.srt");
    let result = xldr::execute_project_runner_source(ProjectRunnerSourceInput {
        project_file: project_file.clone(),
        selected_profile: "dev".to_string(),
        normalized_args: Vec::new(),
        active_file: None,
        source: r#"
def profile_name() -> String { "dev" }

Project::config({|project|
  Project::entrypoint(project, profile_name(), {|config|
    Config::entry_fun(config, "Main::main")
    |> Config::add_path("./src/main.srt")
  })
})
"#
        .to_string(),
    })
    .expect("project runner should execute through the VM");

    assert_eq!(result.profiles.len(), 1);
    assert_eq!(result.profiles[0].name, "dev");
    assert_eq!(result.profiles[0].entrypoint, "Main::main");
    assert_eq!(result.profiles[0].paths.len(), 1);
    assert_eq!(result.profiles[0].paths[0].declared_by, project_file);
    assert_eq!(
        result.profiles[0].paths[0].literal_or_glob,
        "./src/main.srt"
    );
}
