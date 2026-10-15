use super::*;

fn test_span() -> Span {
    Span { start: 0, end: 0 }
}

fn resolved_id(name: &str, qualified_name: &str, unique_id: u32) -> ResolvedId {
    ResolvedId {
        name: name.to_string(),
        qualified_name: Some(qualified_name.to_string()),
        symbol_info: None,
        unique_id,
        compiler_generated: false,
        span: test_span(),
    }
}

fn user_func_ty(fun_idx: u32) -> Ty {
    Ty::UserFunc {
        fun_idx,
        type_params: Vec::new(),
        call_substitution: Vec::new(),
        params: vec![Ty::Int],
        ret: Box::new(Ty::Int),
    }
}

fn specializable_def(fun_idx: u32, name: &str, uid: u32) -> Arc<TypedNode> {
    let id = resolved_id(name, &format!("Global::{name}"), uid);
    Arc::new(TypedNode {
        ty: user_func_ty(fun_idx),
        span: test_span(),
        node: TypedInner::Def(
            fun_idx,
            id.clone(),
            Vec::new(),
            vec![TypedValueParameter {
                id: resolved_id("value", "", uid + 1000),
                mode: spire::ast::ValueParameterMode::PositionalOrNamed,
                ty: Ty::Int,
                span: test_span(),
            }],
            Ty::Int,
            None,
            Box::new(TypedNode {
                ty: Ty::Int,
                span: test_span(),
                node: TypedInner::Lit(Lit::Int(1.into())),
            }),
            spire::ast::Visibility::Public,
        ),
    })
}

fn retained_fun_idx(definition: &TypedNode) -> Option<u32> {
    match &definition.node {
        TypedInner::Def(index, ..) => Some(*index),
        _ => panic!("retained test definition must be an ordinary function"),
    }
}

fn specialization_key() -> CallableInstantiationKey {
    CallableInstantiationKey {
        callable: MethodDeclarationId(10),
        implementation: None,
        trait_inputs: Vec::new(),
        type_args: vec![CanonicalTyKey::Int],
    }
}

fn session_with_cached_specialization(uid: u32, old_fun_idx: u32) -> ScarSession {
    let mut session = ScarSession::new();
    let id = resolved_id("helper", "Global::helper", uid);
    session
        .state
        .function_ids_by_name
        .insert("Global::helper".to_string(), id);
    session
        .state
        .env
        .vars
        .insert(uid, user_func_ty(old_fun_idx));
    session
        .state
        .specialization_fun_idxs
        .insert(specialization_key(), old_fun_idx);
    session
}

#[test]
fn do_safebind_fun_index_rewrite_covers_metadata_and_pin_dispatch() {
    let callable_ty = user_func_ty(40);
    let mut node = TypedNode {
        ty: callable_ty.clone(),
        span: test_span(),
        node: TypedInner::DoSafeBind(Box::new(TypedDoSafeBind {
            pattern: TypedPattern::Pin(
                callable_ty.clone(),
                resolved_id("expected", "Global::expected", 91_300),
                TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                    name: "Global::Eq::eq".into(),
                    fun_idx: 40,
                }),
            ),
            rhs: Box::new(TypedNode {
                ty: callable_ty.clone(),
                span: test_span(),
                node: TypedInner::Lit(Lit::Unit),
            }),
            projection: SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty: callable_ty.clone(),
                error_ty: callable_ty.clone(),
            },
            failure_target: SafeBindFailureTarget::DoMonadFail(Box::new(MonadFailTarget {
                carrier_ty: callable_ty.clone(),
                error_id: sigil::resolved::ResolvedId {
                    name: "error".into(),
                    qualified_name: None,
                    unique_id: 91999,
                    compiler_generated: true,
                    symbol_info: None,
                    span: spire::ast::Span { start: 0, end: 0 },
                },
                call: Box::new(TypedNode {
                    ty: Ty::Error,
                    span: spire::ast::Span { start: 0, end: 0 },
                    node: TypedInner::ConstructorCall(
                        1,
                        vec![TypedNode {
                            ty: Ty::Error,
                            span: spire::ast::Span { start: 0, end: 0 },
                            node: TypedInner::Var(sigil::resolved::ResolvedId {
                                name: "error".into(),
                                qualified_name: None,
                                unique_id: 91999,
                                compiler_generated: true,
                                symbol_info: None,
                                span: spire::ast::Span { start: 0, end: 0 },
                            }),
                        }],
                    ),
                }),
            })),
            continuation: Box::new(TypedNode {
                ty: callable_ty,
                span: test_span(),
                node: TypedInner::Lit(Lit::Unit),
            }),
            origins: DoSafeBindOrigins {
                do_span: test_span(),
                operator_span: test_span(),
                pattern_span: test_span(),
                rhs_span: test_span(),
                result_span: test_span(),
            },
        })),
    };

    let TypedInner::DoSafeBind(control) = &node.node else {
        unreachable!()
    };
    let mut failure_target = control.failure_target.clone();
    if let SafeBindFailureTarget::DoMonadFail(target) = &mut failure_target {
        target.call.ty = user_func_ty(40);
    }
    let mut ordinary = TypedNode {
        ty: node.ty.clone(),
        span: test_span(),
        node: TypedInner::SafeBind(
            control.pattern.clone(),
            control.rhs.clone(),
            control.projection.clone(),
            failure_target,
        ),
    };
    ScarSession::rewrite_fun_indices_in_node(&mut ordinary, &HashMap::from([(40, 140)]));
    let TypedInner::SafeBind(_, _, projection, failure_target) = &ordinary.node else {
        unreachable!()
    };
    assert!(matches!(
        projection,
        SafeBindRhsProjection::CanonicalResultOnce {
            payload_ty: Ty::UserFunc { fun_idx: 140, .. },
            error_ty: Ty::UserFunc { fun_idx: 140, .. }
        }
    ));
    let call = failure_target.monad_fail_call().expect("failure call");
    assert!(matches!(call.ty, Ty::UserFunc { fun_idx: 140, .. }));

    ScarSession::rewrite_fun_indices_in_node(&mut node, &HashMap::from([(40, 140)]));

    let TypedInner::DoSafeBind(control) = &node.node else {
        panic!("expected DoSafeBind")
    };
    let TypedPattern::Pin(
        Ty::UserFunc {
            fun_idx: pattern_ty,
            ..
        },
        _,
        TraitDispatch::Static(TraitDispatchTarget::UserFunction {
            fun_idx: dispatch, ..
        }),
    ) = &control.pattern
    else {
        panic!("expected rewritten pin pattern")
    };
    assert_eq!((*pattern_ty, *dispatch), (140, 140));
    assert!(matches!(
        &control.projection,
        SafeBindRhsProjection::CanonicalResultOnce {
            payload_ty: Ty::UserFunc { fun_idx: 140, .. },
            error_ty: Ty::UserFunc { fun_idx: 140, .. },
        }
    ));
    assert!(matches!(
        &control.failure_target,
        SafeBindFailureTarget::DoMonadFail(target)
            if matches!(target.as_ref(), MonadFailTarget {
            carrier_ty: Ty::UserFunc { fun_idx: 140, .. }, ..
        })
    ));
}

#[test]
fn reconcile_function_indices_rewrites_specialization_cache_values() {
    let mut session = session_with_cached_specialization(10, 40);

    session.reconcile_function_indices([("Global::helper", 77)]);

    assert_eq!(
        session
            .state
            .specialization_fun_idxs
            .get(&specialization_key())
            .copied(),
        Some(77)
    );
}

#[test]
fn reconcile_function_indices_keeps_generic_origin_when_specialization_shares_name() {
    let mut session = ScarSession::new();
    let id = resolved_id("eq", "Global::eq", 10);
    session
        .state
        .function_ids_by_name
        .insert("Global::eq".to_string(), id.clone());
    session
        .state
        .env
        .vars
        .insert(id.unique_id, user_func_ty(40));
    session
        .state
        .specializable_defs
        .insert(40, specializable_def(40, "eq", id.unique_id));
    session
        .state
        .specializable_defs
        .insert(77, specializable_def(77, "eq", id.unique_id));
    session
        .state
        .specialization_fun_idxs
        .insert(specialization_key(), 77);
    session.ensure_next_fun_idx_at_least(100);

    session.reconcile_function_indices([("Global::eq", 77)]);

    assert!(matches!(
        session.state.env.vars.get(&id.unique_id),
        Some(Ty::UserFunc { fun_idx: 100, .. })
    ));
    assert!(session.state.specializable_defs.contains_key(&100));
    assert!(session.state.specializable_defs.contains_key(&77));
    assert_eq!(
        session
            .state
            .specialization_fun_idxs
            .get(&specialization_key())
            .copied(),
        Some(77)
    );
}

#[test]
fn reconcile_function_indices_does_not_rekey_unrelated_function_with_colliding_index() {
    let mut session = ScarSession::new();
    let unrelated = resolved_id("unrelated", "Global::a_unrelated", 11);
    let generic = resolved_id("eq", "Global::z_eq", 10);
    session
        .state
        .function_ids_by_name
        .insert("Global::a_unrelated".to_string(), unrelated.clone());
    session
        .state
        .function_ids_by_name
        .insert("Global::z_eq".to_string(), generic.clone());
    session
        .state
        .env
        .vars
        .insert(unrelated.unique_id, user_func_ty(40));
    session
        .state
        .env
        .vars
        .insert(generic.unique_id, user_func_ty(40));
    session
        .state
        .specializable_defs
        .insert(40, specializable_def(40, "eq", generic.unique_id));
    session.ensure_next_fun_idx_at_least(100);

    session.reconcile_function_indices([("Global::a_unrelated", 70), ("Global::z_eq", 71)]);

    assert!(matches!(
        session.state.env.vars.get(&unrelated.unique_id),
        Some(Ty::UserFunc { fun_idx: 70, .. })
    ));
    assert!(matches!(
        session.state.env.vars.get(&generic.unique_id),
        Some(Ty::UserFunc { fun_idx: 100, .. })
    ));
    assert!(session.state.specializable_defs.contains_key(&100));
}

#[test]
fn reconcile_function_indices_drops_unmaterialized_specialization_cache_entry() {
    let mut session = ScarSession::new();
    let id = resolved_id("helper", "Global::helper", 10);
    session
        .state
        .function_ids_by_name
        .insert("Global::helper".to_string(), id.clone());
    session
        .state
        .env
        .vars
        .insert(id.unique_id, user_func_ty(40));
    session
        .state
        .specializable_defs
        .insert(40, specializable_def(40, "helper", id.unique_id));
    session
        .state
        .specializable_defs
        .insert(676, specializable_def(676, "helper", id.unique_id));
    session
        .state
        .specialization_fun_idxs
        .insert(specialization_key(), 676);
    session.ensure_next_fun_idx_at_least(700);

    session.reconcile_function_indices([("Global::helper", 77)]);

    assert!(!session
        .state
        .specialization_fun_idxs
        .contains_key(&specialization_key()));
}

#[test]
fn reconcile_visible_function_indices_rewrites_specialization_cache_values() {
    let mut session = session_with_cached_specialization(10, 40);

    session.reconcile_visible_function_indices([(10, 77)]);

    assert_eq!(
        session
            .state
            .specialization_fun_idxs
            .get(&specialization_key())
            .copied(),
        Some(77)
    );
}

#[test]
fn reconcile_function_indices_moves_colliding_specializable_defs_without_overwrite() {
    let mut session = ScarSession::new();
    let eq_id = resolved_id("a_eq", "Global::a_eq", 10);
    let compare_id = resolved_id("b_compare", "Global::b_compare", 11);
    session
        .state
        .function_ids_by_name
        .insert("Global::a_eq".to_string(), eq_id.clone());
    session
        .state
        .function_ids_by_name
        .insert("Global::b_compare".to_string(), compare_id.clone());
    session
        .state
        .env
        .vars
        .insert(eq_id.unique_id, user_func_ty(451));
    session
        .state
        .env
        .vars
        .insert(compare_id.unique_id, user_func_ty(452));
    session
        .state
        .specializable_defs
        .insert(451, specializable_def(451, "a_eq", 10));
    session
        .state
        .specializable_defs
        .insert(452, specializable_def(452, "b_compare", 11));

    session.reconcile_function_indices([("Global::other", 451)]);

    let names_by_index = session
        .state
        .specializable_defs
        .iter()
        .map(|(fun_idx, def)| {
            let TypedInner::Def(_, id, ..) = &def.node else {
                panic!("expected function definition");
            };
            (*fun_idx, id.qualified_name.clone().unwrap())
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(names_by_index.get(&452), Some(&"Global::a_eq".to_string()));
    assert_eq!(
        names_by_index.get(&453),
        Some(&"Global::b_compare".to_string())
    );
}

#[test]
fn reconcile_function_indices_preserves_registration_order_and_alias_identity() {
    let mut session = ScarSession::new();
    for (name, uid, index) in [("z_first", 10, 40), ("a_second", 11, 41)] {
        let id = resolved_id(name, &format!("Global::{name}"), uid);
        session
            .state
            .function_ids_by_name
            .insert(format!("Global::{name}"), id.clone());
        session
            .state
            .function_ids_by_name
            .insert(name.into(), id.clone());
        session.state.env.vars.insert(uid, user_func_ty(index));
        session
            .state
            .specializable_defs
            .insert(index, specializable_def(index, name, uid));
    }
    session.ensure_next_fun_idx_at_least(100);
    session.reconcile_function_indices(std::iter::empty::<(&str, u32)>());
    assert!(matches!(
        session.state.env.vars.get(&10),
        Some(Ty::UserFunc { fun_idx: 100, .. })
    ));
    assert!(matches!(
        session.state.env.vars.get(&11),
        Some(Ty::UserFunc { fun_idx: 101, .. })
    ));
    assert_eq!(session.state.env.next_fun_idx, 102);
}

#[test]
fn reconcile_function_indices_preserves_materialized_function_floor() {
    let mut session = ScarSession::new();
    let helper_id = resolved_id("helper", "Global::helper", 10);
    session
        .state
        .function_ids_by_name
        .insert("Global::helper".to_string(), helper_id.clone());
    session
        .state
        .env
        .vars
        .insert(helper_id.unique_id, user_func_ty(500));
    session
        .state
        .specializable_defs
        .insert(500, specializable_def(500, "helper", 10));

    session.ensure_next_fun_idx_at_least(512);
    session.reconcile_function_indices([("Global::other", 499)]);

    let helper_fun_idx = session
        .state
        .specializable_defs
        .iter()
        .find_map(|(fun_idx, def)| {
            (ScarSession::def_qualified_name(def).as_deref() == Some("Global::helper"))
                .then_some(*fun_idx)
        });
    assert_eq!(helper_fun_idx, Some(512));
}

#[test]
fn retained_definitions_isolate_reconciled_sessions_and_checkpoints() {
    let mut original = session_with_cached_specialization(10, 40);
    let mut definition = specializable_def(40, "helper", 10);
    Arc::make_mut(&mut definition).span = Span {
        start: 700,
        end: 710,
    };
    original.state.specializable_defs.insert(40, definition);
    let checkpoint = original.checkpoint();
    let mut sibling = original.clone();
    let saved_definition = &checkpoint.specializable_defs[&40];
    assert!(Arc::ptr_eq(
        saved_definition,
        &sibling.state.specializable_defs[&40],
    ));

    // Reconciliation still updates the function floor and cache on a no-op,
    // but must retain the shared definition when no index changes.
    sibling.reconcile_visible_function_indices([(10, 40)]);
    assert!(Arc::ptr_eq(
        saved_definition,
        &sibling.state.specializable_defs[&40],
    ));
    assert_eq!(sibling.state.env.next_fun_idx, 41);
    assert_eq!(
        sibling.state.specialization_fun_idxs[&specialization_key()],
        40
    );

    sibling.reconcile_visible_function_indices([(10, 77)]);
    assert!(!Arc::ptr_eq(
        saved_definition,
        &sibling.state.specializable_defs[&77],
    ));
    assert_eq!(retained_fun_idx(saved_definition), Some(40));
    assert_eq!(
        retained_fun_idx(&sibling.state.specializable_defs[&77]),
        Some(77)
    );
    assert_eq!(
        retained_fun_idx(&original.state.specializable_defs[&40]),
        Some(40)
    );
    assert_eq!(
        sibling.state.specializable_defs[&77].span,
        saved_definition.span
    );

    let mut restored = ScarSession::new();
    restored.rollback(checkpoint.clone());
    assert!(Arc::ptr_eq(
        saved_definition,
        &restored.state.specializable_defs[&40],
    ));
    restored.reconcile_function_indices([("Global::helper", 90)]);
    assert_eq!(retained_fun_idx(saved_definition), Some(40));
    assert_eq!(
        retained_fun_idx(&restored.state.specializable_defs[&91]),
        Some(91)
    );

    sibling.rollback(checkpoint);
    assert!(sibling.state.specializable_defs.contains_key(&40));
    assert!(!sibling.state.specializable_defs.contains_key(&77));
}

#[test]
fn retained_definition_sharing_preserves_wire_and_checkpoint_restore() {
    let mut original = session_with_cached_specialization(10, 40);
    let definition = specializable_def(40, "helper", 10);
    assert_eq!(
        bincode::serialize(&definition).unwrap(),
        bincode::serialize(definition.as_ref()).unwrap(),
    );
    original.state.specializable_defs.insert(40, definition);
    let checkpoint = original.checkpoint();
    let bytes = bincode::serialize(&checkpoint).unwrap();
    let decoded: ScarCheckpoint = bincode::deserialize(&bytes).unwrap();
    let mut restored = ScarSession::new();
    restored.rollback(decoded);
    restored.reconcile_visible_function_indices([(10, 77)]);
    assert_eq!(
        retained_fun_idx(&checkpoint.specializable_defs[&40]),
        Some(40)
    );
    assert_eq!(
        retained_fun_idx(&restored.state.specializable_defs[&77]),
        Some(77)
    );
}
