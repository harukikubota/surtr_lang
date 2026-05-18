use crate::common::{
    assert_compile_error_matches, module_compile_error_fixtures, module_spec_fixtures,
    normalize_text, parse_compile_error_expectation, repo_root, ModuleFixtureCase,
};
use crate::support;

fn compile_multi_source_case(
    case: &ModuleFixtureCase,
) -> Result<forge::bytecode::Bytecode, String> {
    support::compile_module_fixture_case(case)
}

fn run_multi_source_case(case: &ModuleFixtureCase) -> Result<Vec<String>, String> {
    support::run_module_fixture_case(case)
}

#[test]
fn private_visibility_module_spec_fixture_passes() {
    let fixture = module_spec_fixtures()
        .into_iter()
        .find(|fixture| {
            fixture.case.case_dir
                == repo_root().join("tests/fixtures/modules/pass/private_visibility_basics")
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
                == repo_root().join("tests/fixtures/modules/pass/private_visibility_value_access")
        })
        .expect("private value access spec fixture should exist");

    let output = run_multi_source_case(&fixture.case)
        .expect("private value access inside owner impl spec should run");
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
                == repo_root().join(
                    "tests/fixtures/modules/pass/private_visibility_function_return_private_value",
                )
        })
        .expect("private function return spec fixture should exist");

    let output = run_multi_source_case(&fixture.case)
        .expect("private function return inside owner impl spec should run");
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
                == repo_root()
                    .join("tests/fixtures/modules/pass/private_visibility_value_capture_safe")
        })
        .expect("private value capture safe spec fixture should exist");

    let output = run_multi_source_case(&fixture.case)
        .expect("private value capture inside owner impl spec should run");
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
                == &repo_root().join("tests/fixtures/modules/fail/private_field_access_forbidden")
                || case_dir
                    == &repo_root()
                        .join("tests/fixtures/modules/fail/private_field_type_root_bind_forbidden")
                || case_dir
                    == &repo_root()
                        .join("tests/fixtures/modules/fail/private_field_closure_escape_forbidden")
                || case_dir
                    == &repo_root().join(
                        "tests/fixtures/modules/fail/private_field_param_closure_escape_forbidden",
                    )
                || case_dir
                    == &repo_root()
                        .join("tests/fixtures/modules/fail/private_field_function_return_forbidden")
                || case_dir
                    == &repo_root().join("tests/fixtures/modules/fail/private_def_import_forbidden")
                || case_dir
                    == &repo_root()
                        .join("tests/fixtures/modules/fail/private_def_import_list_grouped")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        cases.len(),
        7,
        "private visibility compile fixtures should exist"
    );

    for fixture in cases {
        let expected = parse_compile_error_expectation(&fixture.error_path);
        let err = compile_multi_source_case(&fixture.case)
            .expect_err("private visibility compile fixture should fail");

        assert_compile_error_matches(&expected, &err, &fixture.case.case_dir);
    }
}
