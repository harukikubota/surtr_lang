use std::collections::HashSet;

use diagnostics::TypeDiagnosticReason;

use super::*;
use crate::env::TypeKind;
use crate::typed::{TypedFacetPath, TypedFacetPathKind, TypedFacetSegment};
use diagnostics::DiagnosticData;
use sindr::primitives::int;

fn test_span() -> Span {
    Span { start: 0, end: 0 }
}

fn registered_callable_id(name: &str, unique_id: u32) -> ResolvedId {
    ResolvedId {
        name: name.into(),
        qualified_name: Some(format!("Global::{name}")),
        unique_id,
        compiler_generated: false,
        symbol_info: None,
        span: test_span(),
    }
}

fn pending_trait_call(trait_id: &str, receiver: Ty) -> TypedNode {
    TypedNode {
        ty: receiver.clone(),
        span: test_span(),
        node: TypedInner::TraitCall {
            trait_name: trait_id.rsplit("::").next().unwrap_or(trait_id).into(),
            method_name: "method".into(),
            receiver_ty: receiver.clone(),
            obligation: TraitObligation {
                trait_id: trait_id.into(),
                trait_args: Vec::new(),
                receiver,
            },
            dispatch: TraitDispatch::Pending,
            origin: TraitCallOrigin::Explicit,
            args: Vec::new(),
        },
    }
}

#[test]
fn full_trait_obligations_enters_do_safebind_control() {
    let node = TypedNode {
        ty: Ty::Int,
        span: test_span(),
        node: TypedInner::DoSafeBind(Box::new(TypedDoSafeBind {
            pattern: TypedPattern::Wildcard(Ty::Int),
            rhs: Box::new(pending_trait_call("Global::RhsTrait", Ty::Int)),
            projection: SafeBindRhsProjection::PatternInput {
                pattern_input_ty: Ty::Int,
            },
            failure_target: SafeBindFailureTarget::DoAlternative {
                empty: Box::new(pending_trait_call("Global::EmptyTrait", Ty::Int)),
            },
            continuation: Box::new(pending_trait_call("Global::NextTrait", Ty::Int)),
            origins: DoSafeBindOrigins {
                do_span: test_span(),
                operator_span: test_span(),
                pattern_span: test_span(),
                rhs_span: test_span(),
                result_span: test_span(),
            },
        })),
    };

    let obligations = Checker::full_trait_obligations(&node);
    assert_eq!(
        obligations
            .iter()
            .map(|obligation| obligation.trait_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Global::RhsTrait",
            "Global::EmptyTrait",
            "Global::NextTrait"
        ]
    );
}

#[test]
fn registered_user_callable_without_canonical_signature_fails_closed() {
    let mut checker = Checker::new(TypecheckContext::default());
    let id = registered_callable_id("target", 91_001);
    checker.register_function_id(&id);
    checker.env.bind_var(
        id.unique_id,
        Ty::UserFunc {
            fun_idx: 0,
            type_params: Vec::new(),
            call_substitution: Vec::new(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Int),
        },
    );

    let error = checker
        .check_app(
            &test_span(),
            &Resolved::Var(test_span(), id),
            &[ResolvedRecordLitArg::Positional(Resolved::Lit(
                test_span(),
                Lit::Int(int(1)),
            ))],
        )
        .expect_err("a registered declaration must not use the legacy positional checker");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::CallableSignatureMetadataMismatch)
    );
}

#[test]
fn resolved_do_requires_canonical_intrinsic_contract_inputs() {
    let mut checker = Checker::new(TypecheckContext::default());
    let node = Resolved::Do(
        test_span(),
        sindr::intrinsic::IntrinsicId::Do,
        sigil::resolved::ResolvedDoContract {
            keyword_span: test_span(),
            monad_fail_trait: None,
            monad_trait: None,
            alternative_trait: None,
        },
        Vec::new(),
        vec![sigil::resolved::ResolvedDoStatement::Statement(
            Resolved::Lit(test_span(), Lit::Int(int(1))),
        )],
    );
    let error = checker
        .check_node(&node)
        .expect_err("do lowering must fail closed without canonical trait metadata");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TypecheckInvariantViolation)
    );
    assert!(matches!(
        error.structured.as_ref().map(|diagnostic| &diagnostic.data),
        Some(DiagnosticData::Policy(diagnostics::PolicyData {
            subject: Some(subject),
            ..
        })) if subject == "canonical intrinsic Trait Monad"
    ));
}

#[test]
fn executable_constructor_audit_enters_generic_declaration_bodies() {
    let checker = Checker::new(TypecheckContext::default());
    let witness_variable = 91_050;
    let constructor_application = Ty::SelfApp(vec![Ty::Hole, Ty::Var(witness_variable), Ty::Int]);
    let definition = TypedNode {
        ty: Ty::Unit,
        span: test_span(),
        node: TypedInner::Def(
            0,
            registered_callable_id("generic", 91_051),
            Vec::new(),
            Vec::new(),
            constructor_application.clone(),
            None,
            Box::new(TypedNode {
                ty: constructor_application,
                span: test_span(),
                node: TypedInner::Lit(Lit::Unit),
            }),
            spire::ast::Visibility::Private,
        ),
    };

    assert!(matches!(
        checker.unresolved_executable_constructor_application(&definition),
        Some((ConstructorApplicationOutcome::Deferred { waiting_on }, _))
            if waiting_on == vec![witness_variable]
    ));
}

#[test]
fn executable_constructor_audit_rejects_malformed_self_application() {
    let checker = Checker::new(TypecheckContext::default());
    let node = TypedNode {
        ty: Ty::SelfApp(vec![Ty::Int]),
        span: test_span(),
        node: TypedInner::Lit(Lit::Unit),
    };

    let Some((ConstructorApplicationOutcome::Rejected { failures }, span)) =
        checker.unresolved_executable_constructor_application(&node)
    else {
        panic!("malformed executable SelfApp must be rejected")
    };
    assert_eq!(
        failures,
        vec![ConstructorProjectionFailure::UnsupportedConstructor]
    );
    let error = signatures::constructor_signature_metadata_error(
        "constructor application",
        &span,
        Checker::constructor_projection_failure_detail(&failures),
    );
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::CallableSignatureMetadataMismatch)
    );
    assert!(!error.message.contains("UnsupportedConstructor"));
}

#[test]
fn registered_builtin_without_canonical_signature_fails_closed() {
    let mut checker = Checker::new(TypecheckContext::default());
    let id = registered_callable_id("runtime_target", 91_002);
    checker.register_function_id(&id);
    checker.env.bind_var(
        id.unique_id,
        Ty::BuiltinFunc {
            name: "runtime_target".into(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Int),
        },
    );

    let error = checker
        .check_app(
            &test_span(),
            &Resolved::Var(test_span(), id),
            &[ResolvedRecordLitArg::Positional(Resolved::Lit(
                test_span(),
                Lit::Int(int(1)),
            ))],
        )
        .expect_err("a registered builtin must not use the legacy positional checker");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::CallableSignatureMetadataMismatch)
    );
}

#[test]
fn ordinary_successful_applications_do_not_create_candidate_probe_checkpoints() {
    let mut checker = Checker::new(TypecheckContext::default());
    let plain = registered_callable_id("plain", 91_101);
    let builtin = registered_callable_id("builtin", 91_102);
    let user = registered_callable_id("user", 91_103);
    checker
        .env
        .bind_var(plain.unique_id, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
    checker.env.bind_var(
        builtin.unique_id,
        Ty::BuiltinFunc {
            name: "builtin".into(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Int),
        },
    );
    checker.env.bind_var(
        user.unique_id,
        Ty::UserFunc {
            fun_idx: 0,
            type_params: Vec::new(),
            call_substitution: Vec::new(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Int),
        },
    );

    for callee in [plain, builtin, user] {
        checker
            .check_app(
                &test_span(),
                &Resolved::Var(test_span(), callee),
                &[ResolvedRecordLitArg::Positional(Resolved::Lit(
                    test_span(),
                    Lit::Int(int(1)),
                ))],
            )
            .expect("ordinary function application should succeed");
    }

    assert_eq!(checker.candidate_probe_checkpoint_count.get(), 0);
}

fn setup_type(
    checker: &mut Checker,
    name: &str,
    fields: Vec<(&str, Ty)>,
    readonly_fields: &[&str],
) {
    checker
        .env
        .predeclare_type_def(name.into(), TypeKind::Struct, Vec::new(), Vec::new());
    checker.env.resolve_type_def_signature(
        name,
        fields
            .into_iter()
            .map(|(field, ty)| (field.into(), ty))
            .collect(),
        Vec::new(),
        HashSet::new(),
        readonly_fields
            .iter()
            .map(|field| (*field).into())
            .collect(),
    );
}

fn field_segment(
    container_type_name: &str,
    field_name: &str,
    field_index: u32,
    readonly: bool,
) -> TypedFacetSegment {
    TypedFacetSegment::Field {
        field_name: field_name.into(),
        origin_index: None,
        field_index,
        container_field_count: 1,
        container_type_name: container_type_name.into(),
        private: false,
        readonly,
    }
}

#[test]
fn error_observation_facet_has_no_owner_update_exception() {
    let mut checker = Checker::new(TypecheckContext::default());
    checker.current_impl_struct_target = Some("Error".into());
    let path = TypedFacetPath {
        source_ty: Ty::Error,
        focus_ty: Ty::Str,
        update_source_ty: Ty::Hole,
        update_focus_ty: Ty::Hole,
        path_kind: TypedFacetPathKind::InfallibleStructural,
        may_fail: false,
        segments: vec![TypedFacetSegment::ReadonlyBuiltin {
            field_name: "message".into(),
            builtin_id: sindr::builtin::builtin_id_by_name("message").unwrap(),
        }],
    };
    let error = checker
        .check_mutating_facet_path_permissions("Facet::set", &path, &test_span())
        .expect_err("Error observations remain readonly in the owner scope");
    assert!(error.message.contains("readonly Error.message"));
}

#[test]
fn mutating_facet_rejects_deep_traversal_through_readonly_field_even_for_owner() {
    let mut checker = Checker::new(TypecheckContext::default());
    setup_type(&mut checker, "Profile", vec![("name", Ty::Str)], &[]);
    setup_type(
        &mut checker,
        "User",
        vec![(
            "profile",
            Ty::Struct(
                "Profile".into(),
                NominalType::monomorphic(vec![("name".into(), Ty::Str)]),
            ),
        )],
        &["profile"],
    );
    checker.current_impl_struct_target = Some("User".into());

    let path = TypedFacetPath {
        source_ty: Ty::Struct(
            "User".into(),
            NominalType::monomorphic(vec![(
                "profile".into(),
                Ty::Struct(
                    "Profile".into(),
                    NominalType::monomorphic(vec![("name".into(), Ty::Str)]),
                ),
            )]),
        ),
        focus_ty: Ty::Str,
        update_source_ty: Ty::Hole,
        update_focus_ty: Ty::Hole,
        path_kind: TypedFacetPathKind::InfallibleStructural,
        may_fail: false,
        segments: vec![
            field_segment("User", "profile", 0, true),
            field_segment("Profile", "name", 0, false),
        ],
    };

    let err = checker
        .check_mutating_facet_path_permissions("Facet::set", &path, &test_span())
        .expect_err("deep traversal through readonly field should fail");
    assert!(err.message.contains("readonly field User.profile"));
    assert!(err.message.contains("replace the property itself"));
}

#[test]
fn mutating_facet_allows_owner_to_replace_readonly_field_itself() {
    let mut checker = Checker::new(TypecheckContext::default());
    setup_type(&mut checker, "Profile", vec![("name", Ty::Str)], &[]);
    setup_type(
        &mut checker,
        "User",
        vec![(
            "profile",
            Ty::Struct(
                "Profile".into(),
                NominalType::monomorphic(vec![("name".into(), Ty::Str)]),
            ),
        )],
        &["profile"],
    );
    checker.current_impl_struct_target = Some("User".into());

    let path = TypedFacetPath {
        source_ty: Ty::Struct(
            "User".into(),
            NominalType::monomorphic(vec![(
                "profile".into(),
                Ty::Struct(
                    "Profile".into(),
                    NominalType::monomorphic(vec![("name".into(), Ty::Str)]),
                ),
            )]),
        ),
        focus_ty: Ty::Struct(
            "Profile".into(),
            NominalType::monomorphic(vec![("name".into(), Ty::Str)]),
        ),
        update_source_ty: Ty::Hole,
        update_focus_ty: Ty::Hole,
        path_kind: TypedFacetPathKind::InfallibleStructural,
        may_fail: false,
        segments: vec![field_segment("User", "profile", 0, true)],
    };

    checker
        .check_mutating_facet_path_permissions("Facet::set", &path, &test_span())
        .expect("owner replacement of readonly field should succeed");
}
