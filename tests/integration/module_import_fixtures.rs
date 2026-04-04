use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use xldr::{collect_compile_sources_with_module_stages, CompileSources, ModuleInput};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("failed to resolve repository root")
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

fn surtr_bin() -> String {
    if let Ok(path) = env::var("CARGO_BIN_EXE_surtr") {
        return path;
    }

    let mut path = env::current_exe().expect("failed to locate current test executable");
    path.pop();
    path.pop();
    path.push("surtr");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.exists(),
        "surtr binary not found at {}",
        path.display()
    );
    path.to_string_lossy().into_owned()
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    let dir = env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), nanos));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[derive(Debug)]
struct CompileErrorExpectation {
    phase: Option<String>,
    contains: Vec<String>,
}

fn parse_compile_error_expectation(path: &Path) -> CompileErrorExpectation {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
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

fn extract_phase_tag(message: &str) -> Option<&str> {
    message
        .strip_prefix("phase=")
        .and_then(|rest| rest.split_once(';').map(|(phase, _)| phase))
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", dir.display(), e))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn case_dirs(root: &Path) -> Vec<PathBuf> {
    sorted_entries(root)
        .into_iter()
        .filter(|path| path.is_dir())
        .collect()
}

fn module_path_from_fixture_file(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("module file stem must be valid utf-8: {}", path.display()))
        .replace("__", "::")
}

fn collect_module_input_stages(case_dir: &Path) -> Vec<Vec<ModuleInput>> {
    let explicit_stage_dirs = sorted_entries(case_dir)
        .into_iter()
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("stage"))
        })
        .collect::<Vec<_>>();

    if explicit_stage_dirs.is_empty() {
        let stage = sorted_entries(case_dir)
            .into_iter()
            .filter(|path| {
                path.is_file()
                    && path.extension().and_then(|ext| ext.to_str()) == Some("srt")
                    && path.file_name().and_then(|name| name.to_str()) != Some("entry.srt")
            })
            .map(|path| ModuleInput {
                file_name: path.to_string_lossy().into_owned(),
                source: fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e)),
                module_path: module_path_from_fixture_file(&path),
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
                sorted_entries(&stage_dir)
                    .into_iter()
                    .filter(|path| {
                        path.is_file()
                            && path.extension().and_then(|ext| ext.to_str()) == Some("srt")
                    })
                    .map(|path| ModuleInput {
                        file_name: path.to_string_lossy().into_owned(),
                        source: fs::read_to_string(&path)
                            .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e)),
                        module_path: module_path_from_fixture_file(&path),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

fn parse_program_with_loader(
    compile_sources: &CompileSources,
) -> Result<(Vec<Vec<sigil::StagedModuleAst>>, Vec<spire::ast::Ast>), String> {
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;

    let mut staged_module_asts = Vec::with_capacity(compile_sources.module_stages.len());
    for stage in &compile_sources.module_stages {
        let mut stage_ast = Vec::with_capacity(stage.len());
        for module in stage {
            let module_source = sources.source(module.source_id).unwrap_or("");
            let module_ast = spire::parse_with_context(
                module_source,
                spire::ParserContext::module(module.source_id.0, Some(module.module_path.clone())),
            )
            .map_err(|e| {
                let file_name = sources.file_name(module.source_id).unwrap_or("<unknown>");
                format!("phase=parse; file={}; message={}", file_name, e.message())
            })?;
            stage_ast.push(sigil::StagedModuleAst {
                module_path: module.module_path.clone(),
                ast: module_ast,
            });
        }
        staged_module_asts.push(stage_ast);
    }

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast =
        spire::parse_with_context(user_source, spire::ParserContext::script(user_source_id.0))
            .map_err(|e| {
                let file_name = sources.file_name(user_source_id).unwrap_or("<unknown>");
                format!("phase=parse; file={}; message={}", file_name, e.message())
            })?;

    Ok((staged_module_asts, user_ast))
}

fn compile_multi_source_case(case_dir: &Path) -> Result<forge::bytecode::Bytecode, String> {
    let entry_path = case_dir.join("entry.srt");
    let entry_source = fs::read_to_string(&entry_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", entry_path.display(), e));
    let module_stages = collect_module_input_stages(case_dir);

    let compile_sources = collect_compile_sources_with_module_stages(
        &entry_path.to_string_lossy(),
        &entry_source,
        &module_stages,
    )
    .map_err(|e| format!("phase=load; message={}", e))?;

    let (module_asts, user_ast) = parse_program_with_loader(&compile_sources)?;
    let declaration_index = sigil::precollect_declaration_index(&module_asts)
        .map_err(|e| format!("phase=resolve; message={}", e))?;

    let resolved = sigil::resolve_staged_program(&module_asts, user_ast, &declaration_index)
        .map_err(|e| format!("phase=resolve; message={}", e))?;
    let typed = scar::typecheck(resolved).map_err(|e| format!("phase=typecheck; message={}", e))?;
    forge::codegen(typed).map_err(|e| format!("phase=codegen; message={}", e))
}

fn run_multi_source_case(case_dir: &Path) -> Result<Vec<String>, String> {
    let bytecode = compile_multi_source_case(case_dir)?;
    let mut vm = eldr::VM::new(bytecode).with_output_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok(vm.output.unwrap_or_default())
}

#[test]
fn module_spec_fixtures_match_expected_stdout_via_loader() {
    let spec_root = repo_root().join("tests/spec/modules");
    let cases = case_dirs(&spec_root);
    assert!(
        !cases.is_empty(),
        "no module spec fixture directories found under {}",
        spec_root.display()
    );

    for case_dir in cases {
        let expected_path = case_dir.join("entry.expected");
        assert!(
            expected_path.exists(),
            "missing entry.expected for {}",
            case_dir.display()
        );

        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", expected_path.display(), e));
        let output = run_multi_source_case(&case_dir)
            .unwrap_or_else(|e| panic!("pipeline failed for {}: {}", case_dir.display(), e));
        let actual_stdout = output.join("\n");
        assert_eq!(
            normalize_text(&actual_stdout),
            normalize_text(&expected),
            "stdout mismatch for {}",
            case_dir.display()
        );
    }
}

#[test]
fn module_compile_error_fixtures_match_expectations_via_loader() {
    let error_root = repo_root().join("tests/compile_errors/modules");
    let cases = case_dirs(&error_root);
    assert!(
        !cases.is_empty(),
        "no module compile-error fixture directories found under {}",
        error_root.display()
    );

    for case_dir in cases {
        let error_path = case_dir.join("entry.error");
        assert!(
            error_path.exists(),
            "missing entry.error for {}",
            case_dir.display()
        );

        let expected = parse_compile_error_expectation(&error_path);
        let result = compile_multi_source_case(&case_dir);
        match result {
            Ok(_) => panic!(
                "expected compile failure but succeeded: {}",
                case_dir.display()
            ),
            Err(msg) => {
                if let Some(expected_phase) = expected.phase.as_deref() {
                    let actual_phase = extract_phase_tag(&msg).unwrap_or("unknown");
                    assert_eq!(
                        actual_phase,
                        expected_phase,
                        "phase mismatch for {}",
                        case_dir.display()
                    );
                }
                for needle in &expected.contains {
                    assert!(
                        msg.contains(needle),
                        "expected '{}' in error for {}\nactual: {}",
                        needle,
                        case_dir.display(),
                        msg
                    );
                }
            }
        }
    }
}

#[test]
fn dump_includes_qualified_function_names_for_module_defined_functions() {
    let case_dir = repo_root().join("tests/spec/modules/qualified_name_without_import");
    let bytecode = compile_multi_source_case(&case_dir)
        .unwrap_or_else(|e| panic!("pipeline failed for {}: {}", case_dir.display(), e));

    let temp = unique_temp_dir("surtr_dump_module_qualified_names");
    let eldr_path = temp.join("module_sample.eldr");
    let bytes = bytecode.encode().expect("encode should succeed");
    fs::write(&eldr_path, bytes).expect("failed to write eldr file");

    let dump = Command::new(surtr_bin())
        .args([
            "dump",
            eldr_path.to_str().expect("eldr path must be utf-8"),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run dump command");
    assert!(
        dump.status.success(),
        "dump failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dump.stdout),
        String::from_utf8_lossy(&dump.stderr)
    );

    let json: Value = serde_json::from_slice(&dump.stdout).expect("dump output must be valid json");
    let functions = json["bytecode"]["functions"]
        .as_array()
        .expect("bytecode.functions must be an array");
    assert!(
        functions
            .iter()
            .any(|entry| entry["qualified_name"] == "Kernel::add"),
        "expected dump to include qualified_name=Kernel::add, got:\n{}",
        String::from_utf8_lossy(&dump.stdout)
    );

    let _ = fs::remove_dir_all(temp);
}
