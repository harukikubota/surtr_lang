use super::*;

mod builtins;
mod functions;
mod nominal;
mod traits;

pub(super) use builtins::special_form_shape_apply_pattern;

#[cfg(test)]
mod contextual_capability_tests {
    use super::*;

    #[test]
    fn declared_method_generic_absent_from_signature_still_tracks_unused_capability() {
        let span = Span { start: 10, end: 16 };
        let mut checker = Checker::new(TypecheckContext::default());
        let declared = vec![ResolvedTypeParam {
            name: "$T".into(),
            bound: None,
            span: span.clone(),
        }];
        let mut method_tyvars = HashMap::new();
        checker.seed_missing_method_type_params(&declared, &mut method_tyvars);
        let marker_id = ResolvedId {
            name: "Marker".into(),
            qualified_name: Some("Marker".into()),
            unique_id: 1,
            compiler_generated: false,
            symbol_info: None,
            span: span.clone(),
        };
        let where_clause = ResolvedWhereClause {
            constraints: vec![ResolvedWhereConstraint {
                subject: AstTy::Named(span.clone(), "$T".into()),
                bounds: vec![ResolvedWhereConstraintRhs::Trait {
                    trait_id: marker_id,
                }],
                span: span.clone(),
            }],
            span: span.clone(),
        };
        let capabilities = checker
            .resolved_capability_uses(Some(&where_clause), &method_tyvars)
            .expect("declared method capability should resolve");

        let err = checker
            .check_body_in_isolated_scope(
                &[],
                &[],
                &capabilities,
                &mut [],
                method_tyvars.clone(),
                Checker::signature_tyvar_ids(&method_tyvars),
                Ty::Unit,
                &span,
                None,
                "unused".into(),
                None,
                None,
                false,
                &Resolved::Lit(span.clone(), Lit::Unit),
            )
            .expect_err("a where-only declared method generic must remain an unused capability");

        assert!(err.message.contains("UnusedTraitConstraint"), "{err:?}");
        assert_eq!(err.span, span);
    }
}
