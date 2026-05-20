use scar::typecheck_with_warnings;
use sigil::resolved::{
    Resolved, ResolvedDeclAttrs, ResolvedEnumVariant, ResolvedId, ResolvedTraitMethodSig,
    ResolvedTypeParam,
};
use sindr::primitives::int;
use sindr::warning::WarningKind;
use spire::ast::{AstTy, Lit, Span};

#[allow(dead_code)]
mod support;

fn span(start: usize, end: usize) -> Span {
    Span { start, end }
}

fn id(name: &str, uid: u32) -> ResolvedId {
    ResolvedId {
        name: name.to_string(),
        qualified_name: Some(format!("Global::{name}")),
        symbol_info: None,
        unique_id: uid,
        compiler_generated: false,
        span: span(uid as usize, uid as usize + 1),
    }
}

fn type_param(name: &str, start: usize) -> ResolvedTypeParam {
    ResolvedTypeParam {
        name: name.to_string(),
        bound: None,
        span: span(start, start + name.chars().count()),
    }
}

fn named_ty(name: &str, start: usize) -> AstTy {
    AstTy::Named(span(start, start + name.chars().count()), name.to_string())
}

fn generic_ty(name: &str, args: Vec<AstTy>, start: usize) -> AstTy {
    AstTy::Generic(
        span(start, start + name.chars().count()),
        name.to_string(),
        args,
    )
}

fn empty_new_def(struct_name: &str, uid: u32) -> Resolved {
    let method_name = format!("{struct_name}::new");
    Resolved::Def(
        span(100 + uid as usize, 110 + uid as usize),
        ResolvedId {
            name: method_name.clone(),
            qualified_name: Some(format!("Global::{method_name}")),
            symbol_info: None,
            unique_id: uid,
            compiler_generated: false,
            span: span(uid as usize, uid as usize + 1),
        },
        vec![type_param("$A", 100 + uid as usize)],
        Vec::new(),
        Some(generic_ty(
            struct_name,
            vec![named_ty("$A", 103 + uid as usize)],
            100 + uid as usize,
        )),
        Box::new(Resolved::StructLit(
            span(105 + uid as usize, 107 + uid as usize),
            id(struct_name, uid + 1),
            Vec::new(),
        )),
        ResolvedDeclAttrs::default(),
    )
}

#[test]
fn non_tail_non_unit_block_value_warns() {
    let output = typecheck_with_warnings(vec![Resolved::Block(
        span(0, 8),
        vec![
            Resolved::Lit(span(1, 2), Lit::Int(int(1))),
            Resolved::Lit(span(4, 6), Lit::Unit),
        ],
    )])
    .expect("block should typecheck");

    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].kind, WarningKind::UnusedValue);
    assert_eq!(output.warnings[0].span.start, 1);
}

#[test]
fn unit_and_explicit_semicolon_values_do_not_warn() {
    let output = typecheck_with_warnings(vec![Resolved::Block(
        span(0, 12),
        vec![
            Resolved::Lit(span(1, 3), Lit::Unit),
            Resolved::Semi(
                span(5, 8),
                Box::new(Resolved::Lit(span(5, 6), Lit::Int(int(1)))),
            ),
            Resolved::Lit(span(10, 12), Lit::Unit),
        ],
    )])
    .expect("block should typecheck");

    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}

#[test]
fn result_unit_non_tail_value_warns() {
    let resolved = support::resolve_with_builtin_prelude(
        r#"Ok(())
()"#,
    );
    let mut session = support::session_from_cached_std_prelude();
    let output = session
        .typecheck_with_warnings(resolved)
        .expect("result unit expression should typecheck");

    let unused_values = output
        .warnings
        .iter()
        .filter(|warning| warning.kind == WarningKind::UnusedValue)
        .collect::<Vec<_>>();
    assert_eq!(unused_values.len(), 1, "{:?}", output.warnings);
}

#[test]
fn unused_struct_and_enum_type_parameters_warn() {
    let output = typecheck_with_warnings(vec![
        Resolved::StructDef(
            span(0, 20),
            id("Box", 10),
            vec![type_param("$A", 4)],
            Vec::new(),
            ResolvedDeclAttrs::default(),
        ),
        empty_new_def("Box", 40),
        Resolved::EnumDef(
            span(21, 45),
            id("Choice", 11),
            vec![type_param("$B", 28)],
            vec![ResolvedEnumVariant {
                id: id("Choice::One", 12),
                payload: Vec::new(),
                discriminant: None,
                span: span(34, 37),
            }],
            ResolvedDeclAttrs::default(),
        ),
    ])
    .expect("declarations should typecheck");

    let kinds = output
        .warnings
        .iter()
        .map(|warning| warning.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            WarningKind::UnusedTypeParameter,
            WarningKind::UnusedTypeParameter
        ]
    );
}

#[test]
fn type_parameters_used_in_real_type_positions_do_not_warn() {
    let output = typecheck_with_warnings(vec![Resolved::EnumDef(
        span(0, 30),
        id("Maybe", 20),
        vec![type_param("$A", 4)],
        vec![ResolvedEnumVariant {
            id: id("Maybe::Some", 21),
            payload: vec![named_ty("$A", 18)],
            discriminant: None,
            span: span(12, 20),
        }],
        ResolvedDeclAttrs::default(),
    )])
    .expect("declaration should typecheck");

    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
}

#[test]
fn trait_head_type_parameter_unused_by_methods_warns_even_with_bound() {
    let mut param = type_param("$A", 9);
    param.bound = Some("Show".to_string());

    let output = typecheck_with_warnings(vec![Resolved::TraitDef(
        span(0, 48),
        id("Describe", 30),
        vec![param],
        vec![ResolvedTraitMethodSig {
            id: id("Describe::describe", 31),
            type_params: Vec::new(),
            params: Vec::new(),
            ret_ty: named_ty("String", 35),
            body: None,
            attrs: ResolvedDeclAttrs::default(),
            span: span(18, 42),
        }],
        ResolvedDeclAttrs::default(),
    )])
    .expect("trait should typecheck");

    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].kind, WarningKind::UnusedTypeParameter);
}
