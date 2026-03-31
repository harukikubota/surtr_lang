use std::fs;
use std::path::{Path, PathBuf};

const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../lib/builtin.srt");

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("failed to resolve repository root")
}

fn collect_files_with_extension(root: &Path, ext: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, ext, out);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == ext)
            {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(root, ext, &mut files);
    files.sort();
    files
}

fn parse_with_builtin_prelude(source: &str) -> Result<Vec<spire::ast::Ast>, String> {
    let mut ast = spire::parse(BUILTIN_PRELUDE_SOURCE)
        .map_err(|e| format!("phase=parse; message=Parse (builtin.srt): {}", e))?;
    let mut user_ast =
        spire::parse(source).map_err(|e| format!("phase=parse; message=Parse: {}", e))?;
    ast.append(&mut user_ast);
    Ok(ast)
}

fn compile_surtr(source: &str) -> Result<forge::bytecode::Bytecode, String> {
    let ast = parse_with_builtin_prelude(source)?;
    let resolved = sigil::resolve(ast).map_err(|e| format!("phase=resolve; message={}", e))?;
    let typed = scar::typecheck(resolved).map_err(|e| format!("phase=typecheck; message={}", e))?;
    forge::codegen(typed).map_err(|e| format!("phase=codegen; message={}", e))
}

fn run_surtr(source: &str) -> Result<Vec<String>, String> {
    let bytecode = compile_surtr(source)?;
    let mut vm = eldr::VM::new(bytecode).with_output_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok(vm.output.unwrap_or_default())
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
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

#[test]
fn spec_fixtures_match_expected_stdout() {
    let spec_root = repo_root().join("tests/spec");
    let sources = collect_files_with_extension(&spec_root, "srt");
    assert!(
        !sources.is_empty(),
        "no spec fixtures found under {}",
        spec_root.display()
    );

    for source_path in sources {
        let expected_path = source_path.with_extension("expected");
        assert!(
            expected_path.exists(),
            "missing .expected for {}",
            source_path.display()
        );

        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", source_path.display(), e));
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", expected_path.display(), e));

        let output = run_surtr(&source)
            .unwrap_or_else(|e| panic!("pipeline failed for {}: {}", source_path.display(), e));

        let actual_stdout = output.join("\n");
        assert_eq!(
            normalize_text(&actual_stdout),
            normalize_text(&expected),
            "stdout mismatch for {}",
            source_path.display()
        );
    }
}

#[test]
fn compile_error_fixtures_match_expectations() {
    let error_root = repo_root().join("tests/compile_errors");
    let sources = collect_files_with_extension(&error_root, "srt");
    assert!(
        !sources.is_empty(),
        "no compile error fixtures found under {}",
        error_root.display()
    );

    for source_path in sources {
        let error_path = source_path.with_extension("error");
        assert!(
            error_path.exists(),
            "missing .error for {}",
            source_path.display()
        );

        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", source_path.display(), e));
        let expected = parse_compile_error_expectation(&error_path);

        let result = compile_surtr(&source);
        match result {
            Ok(_) => panic!(
                "expected compile failure but succeeded: {}",
                source_path.display()
            ),
            Err(msg) => {
                if let Some(expected_phase) = expected.phase.as_deref() {
                    let actual_phase = extract_phase_tag(&msg).unwrap_or("unknown");
                    assert_eq!(
                        actual_phase,
                        expected_phase,
                        "phase mismatch for {}",
                        source_path.display()
                    );
                }
                for needle in &expected.contains {
                    assert!(
                        msg.contains(needle),
                        "expected '{}' in error for {}\nactual: {}",
                        needle,
                        source_path.display(),
                        msg
                    );
                }
            }
        }
    }
}
