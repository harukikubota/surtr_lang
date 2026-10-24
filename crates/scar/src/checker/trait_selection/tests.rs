use super::*;

#[cfg(test)]
mod applicability_tests {
    use super::*;

    fn checker(source: &str) -> Checker {
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        let resolved = crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap();
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        checker.check_program(resolved).unwrap();
        checker
    }

    #[test]
    fn canonical_trait_identity_visits_definitions_once_for_repeated_proofs() {
        let checker = checker("deftrait IdentityMarker {}\nimpl IdentityMarker for Int {}\n");
        checker.trait_identity_entries_visited.set(0);
        let mut next_variable = 1_000_000;
        for _ in 0..2 {
            for (key, info) in &checker.traits {
                assert_eq!(
                    checker.canonical_trait_by_uid(info.id.unique_id).unwrap().0,
                    key
                );
            }
            assert!(checker.canonical_trait_by_uid(u32::MAX).is_none());
            let marker =
                &checker.traits[&checker.trait_key_by_short_name("IdentityMarker").unwrap()];
            assert!(matches!(
                checker
                    .prove_canonical_capability(
                        marker.id.unique_id,
                        &CanonicalTy::builtin(TypeName::Int, vec![]),
                        &mut HashSet::new(),
                        &mut next_variable,
                    )
                    .unwrap(),
                ApplicabilityProof::Satisfied(_)
            ));
        }
        assert!(
            checker.trait_identity_entries_visited.get() <= checker.traits.len(),
            "identity lookup revisited {} definitions for {} traits",
            checker.trait_identity_entries_visited.get(),
            checker.traits.len(),
        );
    }

    #[test]
    fn canonical_trait_identity_refreshes_after_registration_and_checkpoint_restore() {
        const FIRST: &str = "deftrait IdentityFirst {}";
        const BOTH: &str = "deftrait IdentityFirst {}\ndeftrait IdentityLater {}";
        let mut original = checker(FIRST);
        let first = original.traits[&original.trait_key_by_short_name("IdentityFirst").unwrap()]
            .id
            .unique_id;
        assert!(original.canonical_trait_by_uid(first).is_some());
        let checkpoint = original
            .persistent_state_with_env(original.env.clone())
            .checkpoint(Vec::new());
        let bytes = bincode::serialize(&checkpoint).unwrap();
        let sibling = original.spawn_child_checker(original.env.clone());
        let ast = spire::parse_with_context(BOTH, spire::ParserContext::project(0)).unwrap();
        let suffix = crate::test_support::resolve_ast_with_builtin_prelude(ast)
            .unwrap()
            .into_iter()
            .skip(1)
            .collect::<Vec<_>>();
        let Resolved::TraitDef(_, declaration, ..) = &suffix[0] else {
            panic!("expected a trait declaration");
        };
        let later = declaration.unique_id;
        assert!(original.canonical_trait_by_uid(later).is_none());
        original.check_program(suffix).unwrap();
        let later_key = original.trait_key_by_short_name("IdentityLater").unwrap();
        assert_eq!(original.canonical_trait_by_uid(later).unwrap().0, later_key);
        assert!(original.canonical_trait_by_uid(first).is_some());
        assert!(sibling.canonical_trait_by_uid(later).is_none());
        assert_eq!(bincode::serialize(&checkpoint).unwrap(), bytes);

        let restored: ScarCheckpoint = bincode::deserialize(&bytes).unwrap();
        let restored_checker =
            Checker::with_persistent_state(restored.clone().into(), TypecheckContext::default());
        assert!(restored_checker.canonical_trait_by_uid(first).is_some());
        assert!(restored_checker.canonical_trait_by_uid(later).is_none());
        let mut session = ScarSession::new();
        session.state = original.into_persistent_state();
        session.rollback(restored);
        let rolled_back =
            Checker::with_persistent_state(session.state, TypecheckContext::default());
        assert!(rolled_back.canonical_trait_by_uid(first).is_some());
        assert!(rolled_back.canonical_trait_by_uid(later).is_none());
    }

    #[test]
    fn canonical_identity_lookup_keeps_proof_outcomes_and_missing_identity_error() {
        let checker = checker("deftrait IdentityMarker {}\nimpl IdentityMarker for Int {}\n");
        let marker = checker.traits[&checker.trait_key_by_short_name("IdentityMarker").unwrap()]
            .id
            .unique_id;
        let mut next_variable = 1_000_000;
        let mut prove = |trait_id, subject| {
            checker.prove_canonical_capability(
                trait_id,
                &subject,
                &mut HashSet::new(),
                &mut next_variable,
            )
        };
        assert!(
            matches!(prove(marker, CanonicalTy::variable(999_999)).unwrap(),
            ApplicabilityProof::Deferred(waiting) if waiting == vec![999_999])
        );
        assert!(matches!(
            prove(marker, CanonicalTy::builtin(TypeName::Int, vec![])).unwrap(),
            ApplicabilityProof::Satisfied(_)
        ));
        assert!(matches!(
            prove(marker, CanonicalTy::builtin(TypeName::String, vec![])).unwrap(),
            ApplicabilityProof::Unsatisfied
        ));
        let error = prove(u32::MAX, CanonicalTy::builtin(TypeName::Int, vec![])).unwrap_err();
        assert_eq!(
            error.message,
            "Internal error: canonical Trait identity missing"
        );
        assert_eq!(error.span, Span { start: 0, end: 0 });
    }

    #[test]
    fn checked_method_obligations_detach_only_the_child_implementation() {
        let source = "deftrait Needed { def need(self: Self) -> Int }\nimpl Needed for Int { def need(self: Self) -> Int { self } }\ndeftrait Carrier { def carry(self: Self) -> Int }\nimpl Carrier for Int { def carry(self: Self) -> Int { Needed::need(self) } }";
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        let resolved = crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap();
        let mut parent = checker(source);
        let key = parent.trait_impl_candidate_keys("Carrier").pop().unwrap();
        let initial = &parent.trait_impls[&key];
        assert!(!initial.methods["carry"].body_obligations.is_empty());
        assert_eq!(
            bincode::serialize(initial).unwrap(),
            bincode::serialize(initial.as_ref()).unwrap(),
        );
        // Recheck a registered method whose body obligations have not yet been
        // recorded in this branch, as during declaration/body checking.
        Arc::make_mut(parent.trait_impls.get_mut(&key).unwrap())
            .methods
            .get_mut("carry")
            .unwrap()
            .body_obligations
            .clear();
        let checkpoint = parent
            .persistent_state_with_env(parent.env.clone())
            .checkpoint(Vec::new());
        let sibling = parent.spawn_child_checker(parent.env.clone());
        let mut child = parent.spawn_child_checker(parent.env.clone());
        let untouched_key = parent
            .trait_impls
            .keys()
            .find(|candidate| *candidate != &key)
            .unwrap()
            .clone();
        assert!(Arc::ptr_eq(
            &parent.trait_impls[&key],
            &child.trait_impls[&key]
        ));

        let Resolved::TraitImplDef(span, declaration, trait_id, arguments, target, constraints, methods) = resolved
            .iter()
            .find(|node| matches!(node, Resolved::TraitImplDef(_, _, trait_id, ..) if trait_id.name == "Carrier"))
            .unwrap()
        else {
            unreachable!()
        };
        child
            .check_trait_impl_items(
                span,
                *declaration,
                trait_id,
                arguments,
                target,
                constraints.as_ref(),
                methods,
            )
            .unwrap();
        assert!(!child.trait_impls[&key].methods["carry"]
            .body_obligations
            .is_empty());
        assert!(parent.trait_impls[&key].methods["carry"]
            .body_obligations
            .is_empty());
        assert!(sibling.trait_impls[&key].methods["carry"]
            .body_obligations
            .is_empty());
        assert!(checkpoint.trait_impls[&key].methods["carry"]
            .body_obligations
            .is_empty());
        assert!(!Arc::ptr_eq(
            &parent.trait_impls[&key],
            &child.trait_impls[&key]
        ));
        assert!(Arc::ptr_eq(
            &parent.trait_impls[&untouched_key],
            &child.trait_impls[&untouched_key]
        ));

        let bytes = bincode::serialize(&checkpoint).unwrap();
        let restored: ScarCheckpoint = bincode::deserialize(&bytes).unwrap();
        assert!(restored.trait_impls[&key].methods["carry"]
            .body_obligations
            .is_empty());
    }

    #[test]
    fn required_constructor_cycle_restores_relation_state_and_cannot_be_serialized() {
        let mut checker = checker(
            r#"
deftrait First where Self: Type<$A> { def use(self: Self<Int>) -> Int }
deftrait Second where Self: Type<$A> { def use(self: Self<Int>) -> Int }
impl First for List<$A> where Self: Second {
  def use(self: List<Int>) -> Int { Second::use(self) }
}
impl Second for List<$A> where Self: First {
  def use(self: List<Int>) -> Int { First::use(self) }
}
"#,
        );
        let trait_key = checker.trait_key_by_short_name("First").unwrap();
        let variable = checker.env.fresh_tyvar();
        let Ty::Var(witness) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        checker
            .constructor_witness_traits
            .insert(witness, trait_key.clone());
        let expected = Ty::Tuple(vec![
            variable.clone(),
            Ty::SelfApp(vec![Ty::Hole, Ty::Var(witness), Ty::Int]),
        ]);
        let actual = Ty::Tuple(vec![Ty::Bool, Ty::List(Box::new(Ty::Int))]);
        let span = Span { start: 10, end: 20 };
        let substitutions = checker.substitutions.clone();
        let bounds = checker.tyvar_bounds.clone();
        let obligations = checker.pending_trait_obligations.clone();
        let error = checker
            .assert_type_relation(
                &expected,
                &actual,
                checker.type_fact(diagnostics::SourceRole::Expected, &span, &expected),
                checker.type_fact(diagnostics::SourceRole::Value, &span, &actual),
                diagnostics::TypeDiagnosticReason::ArgumentTypeMismatch,
                diagnostics::DiagnosticOrigin::Call,
                "accept",
                0,
            )
            .expect_err("the second tuple element requires the cyclic proof");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::CyclicTraitObligation)
        );
        assert_eq!(error.span, span);
        assert_eq!(checker.substitutions, substitutions);
        assert_eq!(checker.tyvar_bounds, bounds);
        assert_eq!(checker.pending_trait_obligations, obligations);
        let left = TypedNode {
            ty: expected.clone(),
            span: span.clone(),
            node: TypedInner::Lit(spire::ast::Lit::Unit),
        };
        let right = TypedNode {
            ty: actual.clone(),
            span: span.clone(),
            node: TypedInner::Lit(spire::ast::Lit::Unit),
        };
        for payload in [false, true] {
            let error = if payload {
                checker.assert_operand_relation(
                    &expected,
                    &actual,
                    &left,
                    &right,
                    diagnostics::TypeDiagnosticReason::TypePayloadMismatch,
                    "bind",
                    "|>=",
                    Some(&trait_key),
                    diagnostics::SourceRole::LeftValue,
                )
            } else {
                checker.assert_carrier_relation(
                    &expected,
                    &actual,
                    &left,
                    &right,
                    &trait_key,
                    "|>=",
                    diagnostics::SourceRole::LeftValue,
                )
            }
            .expect_err("operator decoration must retain the proof failure");
            assert_eq!(
                error.reason(),
                Some(diagnostics::TypeDiagnosticReason::CyclicTraitObligation)
            );
            assert!(matches!(
                &error.structured.as_ref().unwrap().data,
                diagnostics::DiagnosticData::TraitDispatch(_)
            ));
            let completed =
                checker.complete_branch_error(error.clone(), &[&left, &right], &[None, None]);
            assert_eq!(completed, error);
            assert_eq!(checker.substitutions, substitutions);
        }
        checker.profiler.enabled = true;
        let before = checker.profiler.snapshot();
        let Ty::Var(bounded) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        checker
            .tyvar_bounds
            .insert(bounded, vec![trait_key.clone()]);
        checker
            .types_compatible(&Ty::Var(bounded), &Ty::List(Box::new(Ty::Int)))
            .expect_err("bounded variable requires the cyclic proof");
        let after = checker.profiler.snapshot();
        assert_eq!(
            after.types_compatible_calls,
            before.types_compatible_calls + 1
        );
        assert_eq!(after.bind_tyvar_calls, before.bind_tyvar_calls + 1);
        let Ty::Var(pending) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        checker.pending_trait_obligations.insert(
            pending,
            vec![PendingTraitObligation {
                trait_id: trait_key.clone(),
                args: vec![],
                receiver: Ty::Var(pending),
            }],
        );
        let error = checker
            .bind_tyvar(pending, &Ty::List(Box::new(Ty::Int)))
            .expect_err("pending required proof must not become false");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::CyclicTraitObligation)
        );
        assert_eq!(checker.resolve_ty(&Ty::Var(pending)), Ty::Var(pending));
        assert!(checker.pending_trait_obligations.contains_key(&pending));
        let ConstructorProjectionOutcome::Rejected { failures } =
            checker.constructor_projection(&trait_key, &Ty::List(Box::new(Ty::Int)))
        else {
            panic!("cyclic proof must be rejected")
        };
        let failed = ConstructorApplicationOutcome::Rejected { failures };
        assert!(
            bincode::serialize(&failed).is_err(),
            "failed proof cannot enter a successful checkpoint"
        );
        let applied = ConstructorApplicationOutcome::Applied(Ty::Int);
        let bytes = bincode::serialize(&applied).unwrap();
        assert_eq!(
            bincode::deserialize::<ConstructorApplicationOutcome>(&bytes).unwrap(),
            applied
        );
    }

    #[test]
    fn derived_method_keeps_a_synthetic_contract_identity() {
        let mut checker = checker(
            r#"
@derive Default
defstruct Wrapped { value: Int }
impl Wrapped { def new(value: Int) -> Wrapped { Wrapped { value: value } } }
Default::default::<Wrapped>()
"#,
        );
        let implementation = checker
            .trait_impls
            .values()
            .find(|info| info.target_name.ends_with("Wrapped"))
            .unwrap()
            .clone();
        let method = &implementation.methods["default"];
        let identity = checker
            .trait_implementation_identity(&implementation, method)
            .unwrap();
        assert!(
            matches!(
                &identity,
                TraitImplementationId::Declared {
                    origin: TraitMethodOrigin::Derived(_),
                    ..
                }
            ),
            "derived identity must retain its site, target and contract: {identity:?}"
        );
        assert_eq!(
            method.return_type_arguments.len(),
            1,
            "Default introduces Self exactly once"
        );
        let receiver = Ty::Struct(
            implementation.target_name.clone(),
            NominalType::monomorphic(),
        );
        for _ in 0..2 {
            let CandidateApplicability::Applicable(instantiation) = checker
                .select_trait_method_instantiation(
                    "Default",
                    "default",
                    &receiver,
                    &[],
                    &[],
                    &receiver,
                )
                .unwrap()
            else {
                panic!("concrete derive candidate")
            };
            assert_eq!(instantiation.implementation, identity);
        }
    }

    #[test]
    fn failed_method_probe_does_not_change_complete_instantiation_identity() {
        let mut checker = checker(
            r#"
deftrait Pick<$T> { def pick(self: Self, value: $T) -> Int }
impl Pick<Int> for Int { def pick(self: Self, value: Int) -> Int { value } }
"#,
        );
        let mut select = |argument: Ty| {
            checker
                .select_trait_method_instantiation(
                    "Pick",
                    "pick",
                    &Ty::Int,
                    &[Ty::Int],
                    &[Ty::Int, argument],
                    &Ty::Int,
                )
                .unwrap()
        };
        let CandidateApplicability::Applicable(before) = select(Ty::Int) else {
            panic!("applicable")
        };
        assert!(matches!(
            select(Ty::Str),
            CandidateApplicability::Rejected(_)
        ));
        let CandidateApplicability::Applicable(after) = select(Ty::Int) else {
            panic!("applicable after retry")
        };
        assert_eq!(before, after);
    }

    #[test]
    fn declared_composite_capability_does_not_alpha_rename_distinct_rigid_slots() {
        let mut checker = checker("deftrait Marker {}\n");
        let Ty::Var(declared) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        let Ty::Var(requested) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        checker.rigid_tyvars.extend([declared, requested]);
        let trait_key = checker.trait_key_by_short_name("Marker").unwrap();
        checker.active_capabilities.push(CapabilityUse {
            subject_ty: Ty::List(Box::new(Ty::Var(declared))),
            subject_name: "Self".into(),
            trait_id: trait_key.clone(),
            span: Span { start: 0, end: 0 },
            consumed: false,
        });

        let proof = checker
            .probe_trait_head(&trait_key, &[], &Ty::List(Box::new(Ty::Var(requested))))
            .unwrap();

        assert!(matches!(proof, ApplicabilityProof::Unsatisfied));
        assert!(!checker.active_capabilities[0].consumed);
        assert!(
            matches!(checker.probe_trait_head(&trait_key,&[],&Ty::List(Box::new(Ty::Var(declared)))).unwrap(),ApplicabilityProof::Satisfied(indices) if indices==vec![0])
        );
    }

    #[test]
    fn concrete_impl_cannot_bind_a_nested_rigid_subject() {
        let mut checker = checker("deftrait Marker {}\nimpl Marker for List<Int> {}\n");
        let Ty::Var(var) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        checker.rigid_tyvars.insert(var);
        let subject = Ty::List(Box::new(Ty::Var(var)));
        let trait_key = checker.trait_key_by_short_name("Marker").unwrap();
        assert!(matches!(
            checker.probe_trait_head(&trait_key, &[], &subject).unwrap(),
            ApplicabilityProof::Unsatisfied
        ));
        assert!(matches!(
            checker
                .prove_trait_capability(&trait_key, &subject)
                .unwrap(),
            ApplicabilityProof::Unsatisfied
        ));
        assert_eq!(checker.resolve_ty(&subject), subject);
        assert!(matches!(
            checker
                .probe_trait_head(&trait_key, &[], &Ty::List(Box::new(Ty::Int)))
                .unwrap(),
            ApplicabilityProof::Satisfied(_)
        ));
        let mut generic_checker =
            self::checker("deftrait Marker {}\nimpl Marker for List<$T> {}\n");
        let Ty::Var(generic_var) = generic_checker.env.fresh_tyvar() else {
            unreachable!()
        };
        generic_checker.rigid_tyvars.insert(generic_var);
        let generic_trait = generic_checker.trait_key_by_short_name("Marker").unwrap();
        assert!(matches!(
            generic_checker
                .probe_trait_head(
                    &generic_trait,
                    &[],
                    &Ty::List(Box::new(Ty::Var(generic_var)))
                )
                .unwrap(),
            ApplicabilityProof::Satisfied(_)
        ));
    }

    #[test]
    fn method_target_uses_the_original_declaration_index() {
        let source = "deftrait Only { def value(self: Self) -> Int }\nimpl Only for List<$A> { def value(self: Self) -> Int { 1 } }\n";
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        let resolved = crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap();
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        let typed = checker.check_program(resolved).unwrap();
        let key = checker.trait_impl_candidate_keys("Only").pop().unwrap();
        let method = checker.trait_impls[&key].methods["value"].clone();
        let Some(Ty::UserFunc {
            fun_idx: original, ..
        }) = checker
            .env
            .lookup_var(method.function_id.unique_id)
            .cloned()
        else {
            unreachable!()
        };
        assert!(
            typed
                .iter()
                .all(|node| !matches!(&node.node, TypedInner::Def(idx, ..) if *idx == original)),
            "generic implementation method must wait for a selected substitution"
        );
        let mut generated = checker
            .specializable_defs
            .get(&original)
            .cloned()
            .expect("original retained as specialization source");
        let generated_idx = checker.env.next_fun_idx + 10;
        let TypedInner::Def(fun_idx, ..) = &mut Arc::make_mut(&mut generated).node else {
            unreachable!()
        };
        *fun_idx = generated_idx;
        checker.specializable_defs.clear();
        checker.specializable_defs.insert(generated_idx, generated);
        assert!(
            matches!(checker.impl_method_dispatch_target(&method),Some(TraitDispatchTarget::UserFunction {fun_idx,..}) if fun_idx==original)
        );
    }

    #[test]
    fn one_visible_impl_does_not_determine_unknown_subject() {
        let mut checker = checker(
            "deftrait Only { def value(self: Self) -> Int }\nimpl Only for Int { def value(self: Self) -> Int { self } }\n",
        );
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(var) = unknown else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Only").unwrap();
        let result = checker
            .select_trait_method_instantiation(
                &trait_key,
                "value",
                &unknown,
                &[],
                &[unknown.clone()],
                &Ty::Int,
            )
            .unwrap();
        assert!(
            matches!(result,CandidateApplicability::Deferred(PendingTraitCandidate { waiting_on, .. }) if waiting_on==vec![var])
        );
        assert_eq!(checker.resolve_ty(&unknown), unknown);
    }

    #[test]
    fn constructor_projection_distinguishes_deferred_from_rejected() {
        let mut checker = checker(
            r#"
deftrait Context where Self: Type<$A> {}
impl Context for List<$A> {}
"#,
        );
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(variable) = unknown else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Context").unwrap();

        assert!(matches!(
            checker.constructor_projection(&trait_key, &unknown),
            ConstructorProjectionOutcome::Deferred { waiting_on } if waiting_on == vec![variable]
        ));
        assert!(matches!(
            checker.constructor_projection(&trait_key, &Ty::Int),
            ConstructorProjectionOutcome::Rejected { failures }
                if failures.contains(&ConstructorProjectionFailure::NoApplicableImplementation)
        ));
    }

    #[test]
    fn constructor_application_retains_deferred_and_rejected_reasons() {
        let mut checker = checker(
            r#"
deftrait Context where Self: Type<$A> {}
impl Context for List<$A> {}
"#,
        );
        let source = checker.env.fresh_tyvar();
        let Ty::Var(source_variable) = source else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Context").unwrap();
        checker
            .constructor_witness_traits
            .insert(source_variable, trait_key);
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(variable) = unknown else {
            unreachable!()
        };

        assert!(matches!(
            checker.apply_constructor_application(&source, &unknown, &[Ty::Int]),
            ConstructorApplicationOutcome::Deferred { waiting_on } if waiting_on == vec![variable]
        ));
        assert!(matches!(
            checker.apply_constructor_application(&source, &Ty::Int, &[Ty::Int]),
            ConstructorApplicationOutcome::Rejected { failures }
                if failures.contains(&ConstructorProjectionFailure::UnsupportedConstructor)
        ));
    }

    #[test]
    fn constructor_projection_reports_each_metadata_contract_failure() {
        fn context_checker() -> (Checker, String) {
            let checker = checker(
                r#"
deftrait Context where Self: Type<$A> {}
defenum Box<$A> { Box($A), }
impl Context for Box<$A> {}
"#,
            );
            let trait_key = checker.trait_key_by_short_name("Context").unwrap();
            (checker, trait_key)
        }

        let (mut checker, trait_key) = context_checker();
        let implementation_key = checker.trait_impl_candidate_keys(&trait_key)[0].clone();
        Arc::make_mut(checker.trait_impls.get_mut(&implementation_key).unwrap())
            .head_type_list
            .entries
            .retain(|entry| entry.role != TypeListRole::ImplTarget);
        assert!(matches!(
            checker.constructor_projection(&trait_key, &Ty::Enum("Global::Box".into(), vec![Ty::Int])),
            ConstructorProjectionOutcome::Rejected { failures }
                if failures == vec![ConstructorProjectionFailure::MissingImplTargetMetadata]
        ));

        let (mut checker, trait_key) = context_checker();
        let implementation_key = checker.trait_impl_candidate_keys(&trait_key)[0].clone();
        Arc::make_mut(checker.trait_impls.get_mut(&implementation_key).unwrap())
            .constructor_slot_vars
            .clear();
        assert!(matches!(
            checker.constructor_projection(&trait_key, &Ty::Enum("Global::Box".into(), vec![Ty::Int])),
            ConstructorProjectionOutcome::Rejected { failures }
                if failures == vec![ConstructorProjectionFailure::SlotCountMismatch {
                    expected: 1,
                    actual: 0,
                }]
        ));

        let (mut checker, trait_key) = context_checker();
        let implementation_key = checker.trait_impl_candidate_keys(&trait_key)[0].clone();
        Arc::make_mut(checker.trait_impls.get_mut(&implementation_key).unwrap())
            .constructor_slot_vars[0] = u32::MAX;
        assert!(matches!(
            checker.constructor_application_slots_for_trait(
                &trait_key,
                &Ty::Enum("Global::Box".into(), vec![Ty::Int]),
            ),
            ConstructorSlotsOutcome::Rejected { failures }
                if failures == vec![ConstructorProjectionFailure::MissingConstructorSlotMapping {
                    variable: u32::MAX,
                }]
        ));

        let (mut checker, trait_key) = context_checker();
        let witness_source = checker.env.fresh_tyvar();
        let Ty::Var(witness_variable) = witness_source else {
            unreachable!()
        };
        checker
            .constructor_witness_traits
            .insert(witness_variable, trait_key.clone());
        let witness = Ty::Enum("Global::Box".into(), vec![Ty::Int]);
        assert!(matches!(
            checker.apply_constructor_application(&witness_source, &witness, &[]),
            ConstructorApplicationOutcome::Rejected { failures }
                if failures == vec![ConstructorProjectionFailure::SlotCountMismatch {
                    expected: 1,
                    actual: 0,
                }]
        ));

        let implementation_key = checker.trait_impl_candidate_keys(&trait_key)[0].clone();
        Arc::make_mut(checker.trait_impls.get_mut(&implementation_key).unwrap())
            .constructor_slot_positions[0] = 4;
        assert!(matches!(
            checker.apply_constructor_application(&witness_source, &witness, &[Ty::Str]),
            ConstructorApplicationOutcome::Rejected { failures }
                if failures == vec![ConstructorProjectionFailure::InvalidSlotPosition {
                    position: 4,
                }]
        ));
    }

    #[test]
    fn one_visible_impl_does_not_determine_unknown_trait_argument() {
        let mut checker = checker(
            "deftrait Only<$A> { def value(self: Self) -> Int }\nimpl Only<Int> for Int { def value(self: Self) -> Int { self } }\n",
        );
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(var) = unknown else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Only").unwrap();
        let result = checker
            .select_trait_method_instantiation(
                &trait_key,
                "value",
                &Ty::Int,
                &[unknown.clone()],
                &[Ty::Int],
                &Ty::Int,
            )
            .unwrap();
        assert!(
            matches!(result, CandidateApplicability::Deferred(PendingTraitCandidate { waiting_on, candidates }) if waiting_on == vec![var] && candidates.len() == 1)
        );
        assert_eq!(checker.resolve_ty(&unknown), unknown);
        let result = checker
            .select_trait_method_instantiation(
                &trait_key,
                "value",
                &Ty::Int,
                &[Ty::Str],
                &[Ty::Int],
                &Ty::Int,
            )
            .unwrap();
        assert!(
            matches!(result, CandidateApplicability::Rejected(rejection) if rejection.failures.iter().any(|failure| failure.kind == CandidateFailureKind::TraitImplHeadMismatch))
        );
    }

    #[test]
    fn pending_alias_preserves_nested_receiver_and_trait_arguments() {
        let mut checker = checker(
            "defstruct Box<$T> { value: $T }\nimpl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }\ndeftrait Rel<$A> {}\nimpl Rel<List<Int>> for Box<Int> {}\n",
        );
        let Ty::Var(source) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        let Ty::Var(alias) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Rel").unwrap();
        let name = checker.env.lookup_type_def("Box").unwrap().name.clone();
        checker.pending_trait_obligations.insert(
            source,
            vec![PendingTraitObligation {
                trait_id: trait_key.clone(),
                args: vec![Ty::List(Box::new(Ty::Var(source)))],
                receiver: Ty::Struct(name.clone(), NominalType::new(vec![Ty::Var(source)])),
            }],
        );
        assert!(checker.bind_tyvar(source, &Ty::Var(alias)).unwrap());
        assert!(!checker.pending_trait_obligations.contains_key(&source));
        assert_eq!(
            checker.pending_trait_obligations[&alias],
            vec![PendingTraitObligation {
                trait_id: trait_key,
                args: vec![Ty::List(Box::new(Ty::Var(alias)))],
                receiver: Ty::Struct(name, NominalType::new(vec![Ty::Var(alias)]),),
            }]
        );
        assert!(
            !checker.bind_tyvar(alias, &Ty::Bool).unwrap(),
            "aliased nested obligation must still reject a mismatched concrete binding"
        );
        assert_eq!(checker.resolve_ty(&Ty::Var(alias)), Ty::Var(alias));
        assert!(checker.bind_tyvar(alias, &Ty::Int).unwrap());
        assert_eq!(checker.resolve_ty(&Ty::Var(source)), Ty::Int);
    }

    #[test]
    fn where_probe_waits_only_on_call_site_variables() {
        let mut checker = checker(
            "deftrait Marker { def mark(self: Self) -> Int }\ndefstruct Box<$T> { value: $T }\nimpl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }\ndeftrait Read { def read(self: Self) -> Int }\nimpl Read for Box<$T> where $T: Marker { def read(self: Self) -> Int { Marker::mark(self.value) } }\n",
        );
        let unknown = checker.env.fresh_tyvar();
        let Ty::Var(var) = unknown else {
            unreachable!()
        };
        let trait_key = checker.trait_key_by_short_name("Read").unwrap();
        let result = checker.probe_trait_head(&trait_key, &[], &unknown).unwrap();
        assert!(matches!(result,ApplicabilityProof::Deferred(waiting_on) if waiting_on==vec![var]));
        assert_eq!(checker.resolve_ty(&unknown), unknown);
    }

    #[test]
    fn rejected_where_does_not_bind_either_requested_argument_for_the_next_candidate() {
        let mut checker = checker(
            r#"
deftrait Marker { def mark(self: Self) -> Int }
defstruct Box<$T> { value: $T }
impl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }
deftrait Rel<$A,$B> { def apply(self: Self, a: $A, b: $B) -> Int }
impl Rel<Int,String> for Box<$T> where $T: Marker { def apply(self: Self, a: Int, b: String) -> Int { Marker::mark(self.value) } }
impl Rel<String,Int> for Box<$T> { def apply(self: Self, a: String, b: Int) -> Int { 1 } }
"#,
        );
        let a = checker.env.fresh_tyvar();
        let b = checker.env.fresh_tyvar();
        let trait_key = checker.trait_key_by_short_name("Rel").unwrap();
        let def = checker.env.lookup_type_def("Box").unwrap();
        let receiver = Ty::Struct(def.name.clone(), NominalType::new(vec![Ty::Bool]));
        let substitutions = checker.substitutions.clone();
        let pending = checker.pending_trait_obligations.clone();
        let witnesses = checker.constructor_witness_traits.clone();
        let bounds = checker.tyvar_bounds.clone();
        let next_variable = checker.env.next_tyvar;
        assert!(matches!(
            checker
                .probe_trait_head(&trait_key, &[a.clone(), b.clone()], &receiver)
                .unwrap(),
            ApplicabilityProof::Deferred(_)
        ));
        assert_eq!(checker.resolve_ty(&a), a);
        assert_eq!(checker.resolve_ty(&b), b);
        assert_eq!(checker.substitutions, substitutions);
        assert_eq!(checker.pending_trait_obligations, pending);
        assert_eq!(checker.constructor_witness_traits, witnesses);
        assert_eq!(checker.tyvar_bounds, bounds);
        assert_eq!(checker.env.next_tyvar, next_variable);
    }

    #[test]
    fn rejected_head_rolls_back_both_trait_argument_bindings() {
        let mut checker = checker("deftrait Rel<$A, $B> {}\nimpl Rel<Int, String> for Int {}\n");
        let a = checker.env.fresh_tyvar();
        let b = checker.env.fresh_tyvar();
        let trait_key = checker.trait_key_by_short_name("Rel").unwrap();
        assert!(!checker.trait_impl_exists_for_args(
            &trait_key,
            &[a.clone(), b.clone()],
            &Ty::Bool
        ));
        assert_eq!(checker.resolve_ty(&a), a);
        assert_eq!(checker.resolve_ty(&b), b);
    }
}

#[cfg(test)]
mod extractor_type_contract_tests {
    use super::*;

    #[test]
    fn canonical_error_kind_types_round_trip() {
        let checker = Checker::new(TypecheckContext::default());
        let kind = Ty::Enum("ErrorKind".into(), Vec::new());
        for ty in [
            kind.clone(),
            Ty::List(Box::new(kind.clone())),
            Ty::Func(vec![kind.clone()], Box::new(kind)),
        ] {
            let canonical = checker
                .canonical_resolved_type(&ty)
                .expect("ErrorKind canonical declaration type");
            assert_eq!(
                checker
                    .canonical_to_ty(&canonical)
                    .expect("ErrorKind inverse type"),
                ty
            );
        }
        assert!(checker
            .canonical_to_ty(&CanonicalTy::builtin(
                TypeName::ErrorKind,
                vec![CanonicalTy::builtin(TypeName::Int, vec![])]
            ))
            .is_err());
    }

    #[test]
    fn canonical_extractor_types_round_trip() {
        let checker = Checker::new(TypecheckContext::default());
        for payload in [Ty::Int, Ty::Unit, Ty::Tuple(vec![Ty::Str, Ty::Var(7)])] {
            let result = Ty::MatchResult(Box::new(payload));
            let closure = Ty::ExtractorClosure(Box::new(Ty::Func(
                vec![Ty::Int, Ty::Var(7)],
                Box::new(result.clone()),
            )));
            for ty in [result, closure.clone(), Ty::List(Box::new(closure))] {
                let canonical = checker
                    .canonical_resolved_type(&ty)
                    .expect("canonical type");
                assert_eq!(
                    checker
                        .canonical_to_ty(&canonical)
                        .expect("dedicated inverse"),
                    ty
                );
            }
        }
        for (name, arguments) in [
            (TypeName::MatchResult, vec![]),
            (TypeName::ExtractorClosure, vec![]),
            (
                TypeName::ExtractorClosure,
                vec![CanonicalTy::builtin(TypeName::Int, vec![])],
            ),
            (
                TypeName::ExtractorClosure,
                vec![CanonicalTy::new(
                    CanonicalTypeHead::Function,
                    vec![CanonicalTy::builtin(TypeName::Int, vec![])],
                )],
            ),
        ] {
            assert!(checker
                .canonical_to_ty(&CanonicalTy::builtin(name, arguments))
                .is_err());
        }
    }
}

#[path = "canonical_unifier_tests.rs"]
mod canonical_unifier_tests;
