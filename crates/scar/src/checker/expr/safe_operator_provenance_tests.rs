use super::*;
use diagnostics::SourceFact;

#[test]
fn candidate_rollback_isolates_generated_capture_aliases_and_warnings_in_either_order() {
    fn candidate_source(rejected: bool) -> Vec<Resolved> {
        let source = if rejected {
            "capture = &or(&1, &2)\nTrue"
        } else {
            "capture = &and(&1, &2)\n1"
        };
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap()
    }

    fn check_candidate(
        checker: &mut Checker,
        source: &[Resolved],
        proxy: &ResolvedId,
        outer: &ResolvedId,
    ) -> Result<Ty, TypeError> {
        let typed = source
            .iter()
            .map(|node| checker.check_node(node))
            .collect::<Result<Vec<_>, _>>()?;
        // Deferred pattern selection produces the alias, rather than
        // populating the rollback map directly in the test.
        checker.check_pattern(
            &ResolvedPattern::Deferred {
                pattern: Box::new(ResolvedPattern::Wildcard(proxy.span.clone())),
                bindings: vec![sigil::resolved::ResolvedPatternBinding {
                    proxy: proxy.clone(),
                    outer: Some(outer.clone()),
                }],
                allow_bindings: true,
            },
            &Ty::Unit,
            &proxy.span,
        )?;
        // A warning produced during a candidate probe must follow the same
        // commit/rollback boundary as the generated capture metadata.
        checker.warnings.push(CompilerWarning::new(
            WarningKind::UnusedTypeParameter,
            WarningPhase::Typecheck,
            WarningSpan { start: 0, end: 1 },
            "unused type parameter `$Probe`",
            None,
        ));
        assert_eq!(checker.lazy_capture_bindings.len(), 1);
        assert_eq!(
            checker.pattern_binding_aliases.get(&proxy.unique_id),
            Some(&Some(outer.clone()))
        );
        assert_eq!(checker.warnings.as_slice().len(), 1);
        let tail = typed.last().unwrap();
        checker.assert_type_relation(
            &Ty::Int,
            &tail.ty,
            checker.type_fact(SourceRole::Expected, &tail.span, &Ty::Int),
            checker.type_fact(SourceRole::Value, &tail.span, &tail.ty),
            TypeDiagnosticReason::ArgumentTypeMismatch,
            DiagnosticOrigin::Call,
            "candidate",
            0,
        )?;
        Ok(tail.ty.clone())
    }

    let rejected = candidate_source(true);
    let accepted = candidate_source(false);
    let span = Span { start: 40, end: 41 };
    let proxy = ResolvedId {
        name: "proxy".into(),
        qualified_name: None,
        unique_id: 99_001,
        compiler_generated: false,
        symbol_info: None,
        span: span.clone(),
    };
    let mut accepted_outer = proxy.clone();
    accepted_outer.unique_id += 1;
    let mut rejected_outer = proxy.clone();
    rejected_outer.unique_id += 2;
    let mut observations = Vec::new();
    for order in [[true, false], [false, true]] {
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        for is_rejected in order {
            let checkpoint = checker.candidate_probe_checkpoint();
            let result = check_candidate(
                &mut checker,
                if is_rejected { &rejected } else { &accepted },
                &proxy,
                if is_rejected {
                    &rejected_outer
                } else {
                    &accepted_outer
                },
            );
            if is_rejected {
                assert_eq!(
                    result.unwrap_err().reason(),
                    Some(TypeDiagnosticReason::ArgumentTypeMismatch)
                );
            } else {
                assert_eq!(result.unwrap(), Ty::Int);
            }
            checker.rollback_candidate_probe(checkpoint);
            assert!(checker.lazy_capture_bindings.is_empty());
            assert!(checker.pattern_binding_aliases.is_empty());
            assert!(checker.warnings.as_slice().is_empty());
            assert!(checker.active_lazy_capture.is_none());
        }
        assert_eq!(
            check_candidate(&mut checker, &accepted, &proxy, &accepted_outer).unwrap(),
            Ty::Int
        );
        let Resolved::Bind(_, ResolvedPattern::Var(capture), _) = &accepted[0] else {
            panic!("capture binding")
        };
        let signature = checker.resolve_ty(checker.env.lookup_var(capture.unique_id).unwrap());
        assert_eq!(
            signature,
            Ty::Func(
                vec![Ty::Bool, Ty::Func(Vec::new(), Box::new(Ty::Bool))],
                Box::new(Ty::Bool),
            )
        );
        let error = checker
            .check_app(
                &span,
                &Resolved::Var(span.clone(), capture.clone()),
                &[
                    ResolvedRecordLitArg::Positional(Resolved::Lit(span.clone(), Lit::Bool(true))),
                    ResolvedRecordLitArg::Positional(Resolved::Lit(span.clone(), Lit::Bool(false))),
                ],
            )
            .expect_err("the second argument requires a lazy function");
        assert!(
            error
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("and captures")),
            "{error:?}"
        );
        observations.push((
            signature,
            error.reason(),
            bincode::serialize(&error.structured).unwrap(),
            error.hint,
            error.span,
            checker.warnings.as_slice().to_vec(),
            checker.pattern_binding_aliases.clone(),
            bincode::serialize(&checker.lazy_capture_bindings).unwrap(),
        ));
    }
    assert_eq!(observations[0], observations[1]);
}

#[test]
fn candidate_rollback_restores_only_committed_operator_provenance() {
    let mut checker = Checker::new(TypecheckContext::default());
    let fact = SourceFact::typed(
        SourceRole::OperatorSignature,
        diagnostics::SourceId(0),
        Span { start: 2, end: 3 },
        "(Int, Int) -> Result<Int>",
    );
    checker.safe_operator_results.insert((0, 5), fact.clone());
    let checkpoint = checker.candidate_probe_checkpoint();
    checker.safe_operator_results.clear();
    checker.safe_operator_results.insert((8, 13), fact.clone());
    checker.rollback_candidate_probe(checkpoint);
    assert_eq!(checker.safe_operator_results.len(), 1);
    assert_eq!(checker.safe_operator_results.get(&(0, 5)), Some(&fact));
}
