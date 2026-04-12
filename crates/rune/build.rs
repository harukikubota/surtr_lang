use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct CompileErrorExpectation {
    phase: Option<String>,
    contains: Vec<String>,
}

#[derive(Debug)]
struct SingleSourceSpecFixture {
    path: String,
    source: String,
    expected: String,
}

#[derive(Debug)]
struct SingleSourceCompileErrorFixture {
    path: String,
    source: String,
    error_path: String,
}

#[derive(Debug)]
struct ModuleFixtureFile {
    file_name: String,
    module_path: String,
    source: String,
}

#[derive(Debug)]
struct ModuleFixtureCase {
    case_dir: String,
    entry_path: String,
    entry_source: String,
    companion_path: String,
    companion_text: String,
    stages: Vec<Vec<ModuleFixtureFile>>,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");
    let spec_root = repo_root.join("tests/spec");
    let compile_errors_root = repo_root.join("tests/compile_errors");

    println!("cargo:rerun-if-changed={}", spec_root.display());
    println!("cargo:rerun-if-changed={}", compile_errors_root.display());

    let compile_error_expectation_files =
        collect_files_with_extension(&compile_errors_root, "error");
    let spec_fixtures = collect_single_source_spec_fixtures(&repo_root, &spec_root);
    let compile_error_fixtures =
        collect_single_source_compile_error_fixtures(&repo_root, &compile_errors_root);
    let module_spec_cases =
        collect_module_fixture_cases(&repo_root, &spec_root.join("modules"), "expected");
    let module_compile_error_cases =
        collect_module_fixture_cases(&repo_root, &compile_errors_root.join("modules"), "error");

    let generated = generate_registry(
        &repo_root,
        &compile_error_expectation_files,
        &spec_fixtures,
        &compile_error_fixtures,
        &module_spec_cases,
        &module_compile_error_cases,
    );
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join("generated_fixture_registry.rs");
    fs::write(&out_path, generated)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", out_path.display(), e));
}

fn collect_files_with_extension(dir: &Path, ext: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        let entries = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("failed to read fixture dir {}: {}", dir.display(), e));

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, ext, out);
            } else if path.extension().and_then(|value| value.to_str()) == Some(ext) {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(dir, ext, &mut files);
    files.sort();
    files
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn relative_path(repo_root: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(repo_root)
        .unwrap_or_else(|_| panic!("{} is not under {}", path.display(), repo_root.display()));
    normalize_path(relative)
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

fn is_module_fixture(path: &Path) -> bool {
    normalize_path(path).contains("/modules/")
}

fn collect_single_source_spec_fixtures(
    repo_root: &Path,
    spec_root: &Path,
) -> Vec<SingleSourceSpecFixture> {
    let mut fixtures = collect_files_with_extension(spec_root, "srt")
        .into_iter()
        .filter(|path| !is_module_fixture(path))
        .filter_map(|path| {
            let expected_path = path.with_extension("expected");
            expected_path.exists().then(|| SingleSourceSpecFixture {
                path: relative_path(repo_root, &path),
                source: read_text(&path),
                expected: read_text(&expected_path),
            })
        })
        .collect::<Vec<_>>();
    fixtures.sort_by(|a, b| a.path.cmp(&b.path));
    fixtures
}

fn collect_single_source_compile_error_fixtures(
    repo_root: &Path,
    compile_errors_root: &Path,
) -> Vec<SingleSourceCompileErrorFixture> {
    let mut fixtures = collect_files_with_extension(compile_errors_root, "srt")
        .into_iter()
        .filter(|path| !is_module_fixture(path))
        .filter_map(|path| {
            let error_path = path.with_extension("error");
            error_path
                .exists()
                .then(|| SingleSourceCompileErrorFixture {
                    path: relative_path(repo_root, &path),
                    source: read_text(&path),
                    error_path: relative_path(repo_root, &error_path),
                })
        })
        .collect::<Vec<_>>();
    fixtures.sort_by(|a, b| a.path.cmp(&b.path));
    fixtures
}

fn sorted_immediate_subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read fixture dir {}: {}", dir.display(), e))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn module_path_from_fixture_file(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("module file stem must be valid utf-8: {}", path.display()))
        .replace("__", "::")
}

fn collect_module_fixture_stages(case_dir: &Path, repo_root: &Path) -> Vec<Vec<ModuleFixtureFile>> {
    let explicit_stage_dirs = sorted_immediate_subdirs(case_dir)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("stage"))
        })
        .collect::<Vec<_>>();

    if explicit_stage_dirs.is_empty() {
        let stage = collect_files_with_extension(case_dir, "srt")
            .into_iter()
            .filter(|path| path.parent() == Some(case_dir))
            .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("entry.srt"))
            .map(|path| ModuleFixtureFile {
                file_name: relative_path(repo_root, &path),
                module_path: module_path_from_fixture_file(&path),
                source: read_text(&path),
            })
            .collect::<Vec<_>>();
        if stage.is_empty() {
            Vec::new()
        } else {
            vec![stage]
        }
    } else {
        explicit_stage_dirs
            .into_iter()
            .map(|stage_dir| {
                collect_files_with_extension(&stage_dir, "srt")
                    .into_iter()
                    .map(|path| ModuleFixtureFile {
                        file_name: relative_path(repo_root, &path),
                        module_path: module_path_from_fixture_file(&path),
                        source: read_text(&path),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

fn collect_module_fixture_cases(
    repo_root: &Path,
    root: &Path,
    companion_ext: &str,
) -> Vec<ModuleFixtureCase> {
    let mut cases = sorted_immediate_subdirs(root)
        .into_iter()
        .filter_map(|case_dir| {
            let entry_path = case_dir.join("entry.srt");
            let companion_path = case_dir.join(format!("entry.{companion_ext}"));
            companion_path.exists().then(|| ModuleFixtureCase {
                case_dir: relative_path(repo_root, &case_dir),
                entry_path: relative_path(repo_root, &entry_path),
                entry_source: read_text(&entry_path),
                companion_path: relative_path(repo_root, &companion_path),
                companion_text: read_text(&companion_path),
                stages: collect_module_fixture_stages(&case_dir, repo_root),
            })
        })
        .collect::<Vec<_>>();
    cases.sort_by(|a, b| a.case_dir.cmp(&b.case_dir));
    cases
}

fn parse_compile_error_expectation(path: &Path) -> CompileErrorExpectation {
    let content = read_text(path);
    let mut phase = None;
    let mut contains = Vec::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("phase:") {
            phase = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("contains:") {
            contains.push(rest.trim().to_string());
            continue;
        }
        panic!(
            "invalid compile error expectation line in {}: {}",
            path.display(),
            line
        );
    }

    CompileErrorExpectation { phase, contains }
}

fn rust_str(value: &str) -> String {
    format!("{value:?}")
}

fn push_struct_header(out: &mut String) {
    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct GeneratedCompileErrorExpectation {\n");
    out.push_str("    pub path: &'static str,\n");
    out.push_str("    pub phase: Option<&'static str>,\n");
    out.push_str("    pub contains: &'static [&'static str],\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct GeneratedSpecFixture {\n");
    out.push_str("    pub path: &'static str,\n");
    out.push_str("    pub source: &'static str,\n");
    out.push_str("    pub expected: &'static str,\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct GeneratedCompileErrorFixture {\n");
    out.push_str("    pub path: &'static str,\n");
    out.push_str("    pub source: &'static str,\n");
    out.push_str("    pub error_path: &'static str,\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct GeneratedModuleFile {\n");
    out.push_str("    pub file_name: &'static str,\n");
    out.push_str("    pub module_path: &'static str,\n");
    out.push_str("    pub source: &'static str,\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct GeneratedModuleStage {\n");
    out.push_str("    pub files: &'static [GeneratedModuleFile],\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct GeneratedModuleSpecCase {\n");
    out.push_str("    pub case_dir: &'static str,\n");
    out.push_str("    pub entry_path: &'static str,\n");
    out.push_str("    pub entry_source: &'static str,\n");
    out.push_str("    pub expected_path: &'static str,\n");
    out.push_str("    pub expected: &'static str,\n");
    out.push_str("    pub stages: &'static [GeneratedModuleStage],\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct GeneratedModuleCompileErrorCase {\n");
    out.push_str("    pub case_dir: &'static str,\n");
    out.push_str("    pub entry_path: &'static str,\n");
    out.push_str("    pub entry_source: &'static str,\n");
    out.push_str("    pub error_path: &'static str,\n");
    out.push_str("    pub stages: &'static [GeneratedModuleStage],\n");
    out.push_str("}\n\n");
}

fn generate_compile_error_expectations(out: &mut String, repo_root: &Path, files: &[PathBuf]) {
    out.push_str(
        "pub static GENERATED_COMPILE_ERROR_EXPECTATIONS: &[GeneratedCompileErrorExpectation] = &[\n",
    );

    for file in files {
        let relative = relative_path(repo_root, file);
        let expectation = parse_compile_error_expectation(file);
        let phase = expectation
            .phase
            .as_deref()
            .map(rust_str)
            .unwrap_or_else(|| "None".to_string());
        let phase = if phase == "None" {
            "None".to_string()
        } else {
            format!("Some({phase})")
        };
        let contains = expectation
            .contains
            .iter()
            .map(|item| rust_str(item))
            .collect::<Vec<_>>()
            .join(", ");

        out.push_str("    GeneratedCompileErrorExpectation {\n");
        out.push_str(&format!("        path: {},\n", rust_str(&relative)));
        out.push_str(&format!("        phase: {},\n", phase));
        out.push_str(&format!("        contains: &[{}],\n", contains));
        out.push_str("    },\n");
    }

    out.push_str("];\n\n");
}

fn generate_spec_fixtures(out: &mut String, fixtures: &[SingleSourceSpecFixture]) {
    out.push_str("pub static GENERATED_SPEC_FIXTURES: &[GeneratedSpecFixture] = &[\n");
    for fixture in fixtures {
        out.push_str("    GeneratedSpecFixture {\n");
        out.push_str(&format!("        path: {},\n", rust_str(&fixture.path)));
        out.push_str(&format!("        source: {},\n", rust_str(&fixture.source)));
        out.push_str(&format!(
            "        expected: {},\n",
            rust_str(&fixture.expected)
        ));
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");
}

fn generate_compile_error_fixtures(out: &mut String, fixtures: &[SingleSourceCompileErrorFixture]) {
    out.push_str(
        "pub static GENERATED_COMPILE_ERROR_FIXTURES: &[GeneratedCompileErrorFixture] = &[\n",
    );
    for fixture in fixtures {
        out.push_str("    GeneratedCompileErrorFixture {\n");
        out.push_str(&format!("        path: {},\n", rust_str(&fixture.path)));
        out.push_str(&format!("        source: {},\n", rust_str(&fixture.source)));
        out.push_str(&format!(
            "        error_path: {},\n",
            rust_str(&fixture.error_path)
        ));
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");
}

fn generate_module_case_consts(out: &mut String, const_prefix: &str, cases: &[ModuleFixtureCase]) {
    for (case_index, case) in cases.iter().enumerate() {
        let case_prefix = format!("{const_prefix}_{case_index}");
        for (stage_index, stage) in case.stages.iter().enumerate() {
            let files_name = format!("{case_prefix}_STAGE_{stage_index}_FILES");
            out.push_str(&format!(
                "const {files_name}: &[GeneratedModuleFile] = &[\n"
            ));
            for file in stage {
                out.push_str("    GeneratedModuleFile {\n");
                out.push_str(&format!(
                    "        file_name: {},\n",
                    rust_str(&file.file_name)
                ));
                out.push_str(&format!(
                    "        module_path: {},\n",
                    rust_str(&file.module_path)
                ));
                out.push_str(&format!("        source: {},\n", rust_str(&file.source)));
                out.push_str("    },\n");
            }
            out.push_str("];\n");
        }

        let stages_name = format!("{case_prefix}_STAGES");
        out.push_str(&format!(
            "const {stages_name}: &[GeneratedModuleStage] = &[\n"
        ));
        for stage_index in 0..case.stages.len() {
            let files_name = format!("{case_prefix}_STAGE_{stage_index}_FILES");
            out.push_str("    GeneratedModuleStage {\n");
            out.push_str(&format!("        files: {files_name},\n"));
            out.push_str("    },\n");
        }
        out.push_str("];\n\n");
    }
}

fn generate_module_spec_cases(out: &mut String, cases: &[ModuleFixtureCase]) {
    generate_module_case_consts(out, "GENERATED_MODULE_SPEC_CASE", cases);
    out.push_str("pub static GENERATED_MODULE_SPEC_CASES: &[GeneratedModuleSpecCase] = &[\n");
    for (case_index, case) in cases.iter().enumerate() {
        let stages_name = format!("GENERATED_MODULE_SPEC_CASE_{case_index}_STAGES");
        out.push_str("    GeneratedModuleSpecCase {\n");
        out.push_str(&format!(
            "        case_dir: {},\n",
            rust_str(&case.case_dir)
        ));
        out.push_str(&format!(
            "        entry_path: {},\n",
            rust_str(&case.entry_path)
        ));
        out.push_str(&format!(
            "        entry_source: {},\n",
            rust_str(&case.entry_source)
        ));
        out.push_str(&format!(
            "        expected_path: {},\n",
            rust_str(&case.companion_path)
        ));
        out.push_str(&format!(
            "        expected: {},\n",
            rust_str(&case.companion_text)
        ));
        out.push_str(&format!("        stages: {stages_name},\n"));
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");
}

fn generate_module_compile_error_cases(out: &mut String, cases: &[ModuleFixtureCase]) {
    generate_module_case_consts(out, "GENERATED_MODULE_COMPILE_ERROR_CASE", cases);
    out.push_str(
        "pub static GENERATED_MODULE_COMPILE_ERROR_CASES: &[GeneratedModuleCompileErrorCase] = &[\n",
    );
    for (case_index, case) in cases.iter().enumerate() {
        let stages_name = format!("GENERATED_MODULE_COMPILE_ERROR_CASE_{case_index}_STAGES");
        out.push_str("    GeneratedModuleCompileErrorCase {\n");
        out.push_str(&format!(
            "        case_dir: {},\n",
            rust_str(&case.case_dir)
        ));
        out.push_str(&format!(
            "        entry_path: {},\n",
            rust_str(&case.entry_path)
        ));
        out.push_str(&format!(
            "        entry_source: {},\n",
            rust_str(&case.entry_source)
        ));
        out.push_str(&format!(
            "        error_path: {},\n",
            rust_str(&case.companion_path)
        ));
        out.push_str(&format!("        stages: {stages_name},\n"));
        out.push_str("    },\n");
    }
    out.push_str("];\n");
}

fn generate_registry(
    repo_root: &Path,
    compile_error_expectation_files: &[PathBuf],
    spec_fixtures: &[SingleSourceSpecFixture],
    compile_error_fixtures: &[SingleSourceCompileErrorFixture],
    module_spec_cases: &[ModuleFixtureCase],
    module_compile_error_cases: &[ModuleFixtureCase],
) -> String {
    let mut out = String::new();
    push_struct_header(&mut out);
    generate_compile_error_expectations(&mut out, repo_root, compile_error_expectation_files);
    generate_spec_fixtures(&mut out, spec_fixtures);
    generate_compile_error_fixtures(&mut out, compile_error_fixtures);
    generate_module_spec_cases(&mut out, module_spec_cases);
    generate_module_compile_error_cases(&mut out, module_compile_error_cases);
    out
}
