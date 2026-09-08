#![deny(dead_code)]

#[allow(dead_code)]
mod support;
use diagnostics::{SourceId, TypeDiagnosticReason as Reason};
use std::collections::HashSet;

fn error(source: &str) -> diagnostics::StructuredDiagnostic {
    support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err(source)
        .structured
        .expect("structured diagnostic")
}

macro_rules! schema_case {
    ($case:ident) => {
        (stringify!($case), $case as fn())
    };
}

const SCHEMA_CASES: &[(&str, fn())] = &[
    schema_case!(source_diagnostic_data_is_independent_of_presentation_facts),
    schema_case!(missing_return_input_has_no_fabricated_actual_type_and_surface_help),
    schema_case!(trait_arity_uses_null_types_and_owned_impl_origin),
    schema_case!(branch_schema_keeps_both_source_ordinals),
    schema_case!(operator_obligation_origin_matches_its_source_operand),
];

#[test]
fn schema_case_inventory_has_unique_names_and_functions() {
    let direct_tests = include_str!("closed_diagnostic_schema.rs")
        .lines()
        .filter(|line| *line == "#[test]")
        .count();
    assert_eq!(
        direct_tests, 2,
        "diagnostic schema cases must be registered instead of using standalone #[test]"
    );

    let mut names = HashSet::new();
    let mut functions = HashSet::new();
    for &(name, case) in SCHEMA_CASES {
        assert!(
            names.insert(name),
            "duplicate diagnostic schema case name: {name}"
        );
        assert!(
            functions.insert(case as usize),
            "duplicate diagnostic schema case function: {name}"
        );
    }
}

#[test]
fn closed_diagnostic_schema_suite() {
    for &(name, case) in SCHEMA_CASES {
        eprintln!("closed diagnostic schema case: {name}");
        case();
    }
}

fn source_diagnostic_data_is_independent_of_presentation_facts() {
    for source in [
        "def take(x: Int) -> Int { x }\ntake(\"bad\")",
        "if(True, 1, \"bad\")",
        "match True { True => 1, False => \"bad\" }",
        "cond { False => 1, True => \"bad\" }",
        "def missing(mapper: ($A -> Int)) -> Option<$B> { 0 }",
    ] {
        let mut diagnostic = error(source);
        let original = diagnostic.data_json();
        diagnostic.related.clear();
        diagnostic.primary.span = spire::ast::Span { start: 0, end: 0 };
        diagnostic.primary.ty = Some("Poison".into());
        diagnostic.primary.role = diagnostics::SourceRole::Other;
        assert_eq!(original, diagnostic.data_json(), "{source}");
        let rebased = diagnostic
            .map_source_locations(|span| {
                (
                    SourceId(7),
                    spire::ast::Span {
                        start: span.start + 10,
                        end: span.end + 10,
                    },
                )
            })
            .data_json();
        let origin_key = if original["kind"] == "ArgumentRelation" {
            "actual_origin"
        } else {
            "right_origin"
        };
        assert_eq!(rebased[origin_key]["source_id"], 7);
        assert_eq!(
            rebased[origin_key]["span"][0].as_u64(),
            original[origin_key]["span"][0].as_u64().map(|x| x + 10)
        );
    }
}
fn missing_return_input_has_no_fabricated_actual_type_and_surface_help() {
    let source = "def missing(mapper: ($A -> Int)) -> Option<$B> { 0 }";
    let diagnostic = error(source);
    assert_eq!(diagnostic.reason, Reason::MissingReturnTypeArgument);
    let data = diagnostic.data_json();
    assert_eq!(data["expected_type"], "$B");
    assert!(data["actual_type"].is_null());
    assert!(data["declared_origin"].is_null());
    assert_eq!(data["return_origin"]["kind"], "expected");
    let mut sources = diagnostics::SourceRegistry::new();
    let source_id = sources.register("missing.srt", source);
    let spec = diagnostics::structured_type_error_spec(&diagnostic);
    let rendered = diagnostics::serializable_report_by_id(&sources, source_id, "typecheck", &spec)
        .errors
        .remove(0);
    assert_eq!(rendered.expected.as_deref(), Some("$B"));
    assert_eq!(rendered.got, None);

    let help = diagnostic.remediation_text().unwrap();
    assert!(help.contains("def missing::<$B>(...)"), "{help}");
    assert!(!help.contains("__Script"));
    let fixed = source.replace("def missing(", "def missing::<$B>(");
    spire::parse_with_context(&fixed, spire::ParserContext::project(0))
        .expect("suggested declaration syntax parses");
}
fn trait_arity_uses_null_types_and_owned_impl_origin() {
    let diagnostic = error("deftrait Copy { def copy(self: Self, value: Int) -> Int }\nimpl Copy for Int { def copy(self: Self) -> Int { 0 } }");
    assert_eq!(diagnostic.reason, Reason::TraitMethodTypeListArityMismatch);
    let data = diagnostic.data_json();
    assert_eq!(data["kind"], "TraitDispatch");
    assert!(data["expected_type"].is_null());
    assert!(data["actual_type"].is_null());
    assert_eq!(data["impl_declaration"]["kind"], "impl");
}

fn branch_schema_keeps_both_source_ordinals() {
    for (source, form) in [
        ("if(True, 1, \"bad\")", "If"),
        ("match True { True => 1, False => \"bad\" }", "Match"),
        ("cond { False => 1, True => \"bad\" }", "Cond"),
    ] {
        let data = error(source).data_json();
        assert_eq!(data["form"], form);
        assert_eq!(data["left_ordinal"], 0);
        assert_eq!(data["right_ordinal"], 1);
        assert_eq!(data["left_origin"]["ordinal"], 0);
        assert_eq!(data["right_origin"]["ordinal"], 1);
    }
}

fn operator_obligation_origin_matches_its_source_operand() {
    let diagnostic = error("def broken(a: $A, b: $A) -> $A { a + b }");
    assert_eq!(diagnostic.reason, Reason::MissingGenericBound);
    assert!(
        matches!(diagnostic.origin, diagnostics::DiagnosticOrigin::Operator { ref operator } if operator == "+")
    );
    assert_eq!(diagnostic.primary.role, diagnostics::SourceRole::LeftValue);
    assert_eq!(
        diagnostic.data_json()["obligation_origin"]["kind"],
        "left_value"
    );
}
