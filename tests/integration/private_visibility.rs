mod common;
mod support;

use common::{
    extract_phase_tag, module_compile_error_fixtures, module_spec_fixtures, normalize_text,
    parse_compile_error_expectation, repo_root, ModuleFixtureCase,
};

fn compile_multi_source_case(
    case: &ModuleFixtureCase,
) -> Result<forge::bytecode::Bytecode, String> {
    let module_sources = support::collect_module_sources(&case.module_stages)?;
    let compile_sources = support::compose_script_sources(
        &case.entry_path.to_string_lossy(),
        case.entry_source,
        module_sources,
    );

    support::compile_script_sources(&compile_sources)
}

fn run_multi_source_case(case: &ModuleFixtureCase) -> Result<Vec<String>, String> {
    let bytecode = compile_multi_source_case(case)?;
    let mut vm = eldr::VM::new(bytecode).with_output_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok(vm.output.unwrap_or_default())
}

#[test]
fn private_visibility_module_spec_fixture_passes() {
    let fixture = module_spec_fixtures()
        .into_iter()
        .find(|fixture| {
            fixture.case.case_dir
                == repo_root().join("tests/spec/modules/private_visibility_basics")
        })
        .expect("private visibility spec fixture should exist");

    let output = run_multi_source_case(&fixture.case).expect("private visibility spec should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn private_visibility_value_access_spec_fixture_passes() {
    let fixture = module_spec_fixtures()
        .into_iter()
        .find(|fixture| {
            fixture.case.case_dir
                == repo_root().join("tests/spec/modules/private_visibility_value_access")
        })
        .expect("private value access spec fixture should exist");

    let output =
        run_multi_source_case(&fixture.case).expect("private value access spec should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn private_visibility_function_return_private_value_spec_fixture_passes() {
    let fixture = module_spec_fixtures()
        .into_iter()
        .find(|fixture| {
            fixture.case.case_dir
                == repo_root()
                    .join("tests/spec/modules/private_visibility_function_return_private_value")
        })
        .expect("private function return spec fixture should exist");

    let output =
        run_multi_source_case(&fixture.case).expect("private function return spec should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn private_visibility_value_capture_safe_spec_fixture_passes() {
    let fixture = module_spec_fixtures()
        .into_iter()
        .find(|fixture| {
            fixture.case.case_dir
                == repo_root().join("tests/spec/modules/private_visibility_value_capture_safe")
        })
        .expect("private value capture safe spec fixture should exist");

    let output =
        run_multi_source_case(&fixture.case).expect("private value capture safe spec should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn private_visibility_compile_error_fixtures_pass() {
    let cases = module_compile_error_fixtures()
        .into_iter()
        .filter(|fixture| {
            let case_dir = &fixture.case.case_dir;
            case_dir
                == &repo_root().join("tests/compile_errors/modules/private_field_access_forbidden")
                || case_dir
                    == &repo_root()
                        .join("tests/compile_errors/modules/private_field_closure_escape_forbidden")
                || case_dir
                    == &repo_root().join(
                        "tests/compile_errors/modules/private_field_param_closure_escape_forbidden",
                    )
                || case_dir
                    == &repo_root()
                        .join("tests/compile_errors/modules/private_field_type_root_bind_forbidden")
                || case_dir
                    == &repo_root()
                        .join("tests/compile_errors/modules/private_def_import_forbidden")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        cases.len(),
        5,
        "private visibility compile fixtures should exist"
    );

    for fixture in cases {
        let expected = parse_compile_error_expectation(&fixture.error_path);
        let err = compile_multi_source_case(&fixture.case)
            .expect_err("private visibility compile fixture should fail");

        if let Some(expected_phase) = expected.phase.as_deref() {
            let actual_phase = extract_phase_tag(&err).unwrap_or("unknown");
            assert_eq!(actual_phase, expected_phase);
        }

        for needle in &expected.contains {
            assert!(
                err.contains(needle),
                "expected '{}' in error for {}\nactual: {}",
                needle,
                fixture.case.case_dir.display(),
                err
            );
        }
    }
}
