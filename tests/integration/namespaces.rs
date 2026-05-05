use crate::common::{
    extract_phase_tag, module_compile_error_fixtures, module_spec_fixtures, normalize_text,
    parse_compile_error_expectation, repo_root,
};
use crate::support;

fn find_module_spec_case(name: &str) -> crate::common::ModuleSpecFixtureCase {
    let case_dir = repo_root().join(format!("tests/spec/modules/{name}"));
    module_spec_fixtures()
        .into_iter()
        .find(|fixture| fixture.case.case_dir == case_dir)
        .expect("module spec fixture must exist")
}

fn find_module_compile_error_case(name: &str) -> crate::common::ModuleCompileErrorFixtureCase {
    let case_dir = repo_root().join(format!("tests/compile_errors/modules/{name}"));
    module_compile_error_fixtures()
        .into_iter()
        .find(|fixture| fixture.case.case_dir == case_dir)
        .expect("module compile-error fixture must exist")
}

fn compile_case_output(case: &crate::common::ModuleFixtureCase) -> Result<Vec<String>, String> {
    let module_sources = support::collect_module_sources(&case.module_stages)?;
    let compile_sources = support::compose_script_sources(
        &case.entry_path.to_string_lossy(),
        case.entry_source,
        module_sources,
    );
    let bytecode = support::compile_script_sources(&compile_sources)?;
    let mut vm = eldr::VM::new(bytecode).with_output_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok(vm.output.unwrap_or_default())
}

fn compile_case(case: &crate::common::ModuleFixtureCase) -> Result<(), String> {
    let module_sources = support::collect_module_sources(&case.module_stages)?;
    let compile_sources = support::compose_script_sources(
        &case.entry_path.to_string_lossy(),
        case.entry_source,
        module_sources,
    );
    support::compile_script_sources(&compile_sources).map(|_| ())
}

#[test]
fn namespaced_type_is_visible_from_root_without_import() {
    let fixture = find_module_spec_case("namespaced_type_root_lookup");
    let output = compile_case_output(&fixture.case).expect("fixture should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn namespaced_same_name_functions_resolve_by_qualified_path() {
    let fixture = find_module_spec_case("namespaced_function_name_collision_qualified_calls");
    let output = compile_case_output(&fixture.case).expect("fixture should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn process_context_out_handler_uses_default_target() {
    let fixture = find_module_spec_case("process_context_out_handler_default");
    let output = compile_case_output(&fixture.case).expect("fixture should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn process_context_out_handler_uses_supervisor_override() {
    let fixture = find_module_spec_case("process_context_out_handler_override");
    let output = compile_case_output(&fixture.case).expect("fixture should run");
    assert_eq!(
        normalize_text(&output.join("\n")),
        normalize_text(fixture.expected),
    );
}

#[test]
fn namespaced_duplicate_type_in_same_namespace_is_rejected() {
    let fixture = find_module_compile_error_case("namespaced_duplicate_type_same_namespace");
    let expected = parse_compile_error_expectation(&fixture.error_path);
    let err = compile_case(&fixture.case).expect_err("fixture should fail");
    assert_eq!(extract_phase_tag(&err), expected.phase.as_deref());
    for needle in &expected.contains {
        assert!(err.contains(needle), "expected '{needle}' in '{err}'");
    }
}

#[test]
fn namespaced_import_collision_keeps_existing_function_import_rules() {
    let fixture = find_module_compile_error_case("namespaced_function_import_collision");
    let expected = parse_compile_error_expectation(&fixture.error_path);
    let err = compile_case(&fixture.case).expect_err("fixture should fail");
    assert_eq!(extract_phase_tag(&err), expected.phase.as_deref());
    for needle in &expected.contains {
        assert!(err.contains(needle), "expected '{needle}' in '{err}'");
    }
}
