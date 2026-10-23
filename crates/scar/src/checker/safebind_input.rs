use super::*;

/// The requirement at the Pattern boundary; no child Pattern participates.
enum SafeBindInputRequirement {
    Exact(Ty),
    Sequence,
    Tuple,
    Record(String),
}

impl Checker {
    /// Classify only the direct Pattern input. Children are checked once by the
    /// ordinary Pattern path, never speculatively against a different input.
    pub(super) fn safebind_missing_extraction_input(
        &mut self,
        pattern: &ResolvedPattern,
        rhs_ty: &Ty,
        payload_ty: &Ty,
    ) -> Result<Option<String>, TypeError> {
        let requirement = match pattern {
            ResolvedPattern::Located(_, inner)
            | ResolvedPattern::Deferred { pattern: inner, .. } => {
                return self.safebind_missing_extraction_input(inner, rhs_ty, payload_ty);
            }
            ResolvedPattern::Var(_) | ResolvedPattern::Wildcard(_) => {
                return Ok(Some(self.diagnostic_ty_name(payload_ty)));
            }
            ResolvedPattern::Annotated(_, annotation)
            | ResolvedPattern::AnnotatedWildcard(_, annotation) => SafeBindInputRequirement::Exact(
                self.resolve_ast_ty_in_context(annotation, self.local_type_syntax_context())?,
            ),
            ResolvedPattern::As(inner, _, annotation) => {
                if let Some(annotation) = annotation {
                    // An explicit whole-value alias is the direct input
                    // contract; mismatching children remain normal errors.
                    SafeBindInputRequirement::Exact(
                        self.resolve_ast_ty_in_context(
                            annotation,
                            self.local_type_syntax_context(),
                        )?,
                    )
                } else {
                    return self.safebind_missing_extraction_input(inner, rhs_ty, payload_ty);
                }
            }
            ResolvedPattern::Pin(id) => {
                let id = self.canonical_pattern_id(id)?;
                let ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(|| {
                    self.typecheck_invariant_error("resolved pinned pattern binding", &id.span)
                })?;
                SafeBindInputRequirement::Exact(self.resolve_ty(&ty))
            }
            ResolvedPattern::IntLit(..) => SafeBindInputRequirement::Exact(Ty::Int),
            ResolvedPattern::StrLit(..) => SafeBindInputRequirement::Exact(Ty::Str),
            ResolvedPattern::BoolLit(..) => SafeBindInputRequirement::Exact(Ty::Bool),
            ResolvedPattern::DurationLit(..) => SafeBindInputRequirement::Exact(Ty::Struct(
                "Duration".into(),
                NominalType::monomorphic(Vec::new()),
            )),
            ResolvedPattern::ListNil(_) | ResolvedPattern::ListCons(..) => {
                SafeBindInputRequirement::Sequence
            }
            ResolvedPattern::Tuple(_) => SafeBindInputRequirement::Tuple,
            ResolvedPattern::HashMap(..) => SafeBindInputRequirement::Exact(Ty::Enum(
                "HashMap".into(),
                vec![self.env.fresh_tyvar()],
            )),
            ResolvedPattern::Record(id, _) => {
                let Some(definition) = self.env.lookup_type_def(&id.name) else {
                    return Ok(None);
                };
                if definition.kind != TypeKind::Record {
                    // Error Pattern and invalid heads retain their own policy.
                    return Ok(None);
                }
                SafeBindInputRequirement::Record(definition.name.clone())
            }
            ResolvedPattern::Constructor(id, _) => {
                let Some(variant) = self.lookup_enum_variant_by_constructor_id(id.unique_id) else {
                    return Ok(None);
                };
                let variant = self.instantiate_enum_variant(&variant);
                SafeBindInputRequirement::Exact(variant.enum_ty)
            }
            ResolvedPattern::ExtractorApplication { head, args } => {
                let selected = self.select_extractor_application(head, args)?;
                return self.safebind_missing_extraction_input(&selected, rhs_ty, payload_ty);
            }
            ResolvedPattern::Extractor(id, pre_args, _) => {
                let callable = self.extractor_callable_ty(id, &id.span)?;
                let (input, ..) = self.extractor_contract(id, &callable, &id.span)?;
                let requirement = if self
                    .function_ids_by_name
                    .get("Kernel::uncons")
                    .is_some_and(|canonical| canonical.unique_id == id.unique_id)
                    && matches!(callable, Ty::BuiltinFunc { .. })
                {
                    SafeBindInputRequirement::Sequence
                } else {
                    SafeBindInputRequirement::Exact(input)
                };
                // Do not infer pre-arguments against payload when the direct
                // input already accepts the whole value (or neither type).
                if self.safebind_direct_input_accepts(&requirement, rhs_ty)?
                    || !self.safebind_direct_input_accepts(&requirement, payload_ty)?
                {
                    return Ok(None);
                }
                // Validate the existing head/pre-argument contract only for a
                // proven payload entry. This does not check child Patterns.
                let (input, ..) =
                    self.extractor_contract_for_observed_ty(id, payload_ty, pre_args, &id.span)?;
                SafeBindInputRequirement::Exact(input)
            }
            ResolvedPattern::Or(_) | ResolvedPattern::Projection { .. } => return Ok(None),
        };
        if self.safebind_direct_input_accepts(&requirement, rhs_ty)?
            || !self.safebind_direct_input_accepts(&requirement, payload_ty)?
        {
            return Ok(None);
        }
        let input = match requirement {
            SafeBindInputRequirement::Exact(ty) => self.diagnostic_ty_name(&ty),
            _ => self.diagnostic_ty_name(payload_ty),
        };
        Ok(Some(input))
    }

    fn safebind_direct_input_accepts(
        &mut self,
        requirement: &SafeBindInputRequirement,
        actual: &Ty,
    ) -> Result<bool, TypeError> {
        let actual = self.resolve_ty(actual);
        match requirement {
            SafeBindInputRequirement::Exact(expected) => self
                .with_type_relation_probe(&[expected, &actual], |checker| {
                    checker.types_compatible(expected, &actual)
                }),
            SafeBindInputRequirement::Sequence => Ok(matches!(actual, Ty::List(_) | Ty::Str)),
            SafeBindInputRequirement::Tuple => Ok(matches!(actual, Ty::Tuple(_))),
            SafeBindInputRequirement::Record(expected) => Ok(match actual {
                Ty::Record(name, _) => {
                    self.env.lookup_type_def(&name).map(|def| def.tag)
                        == self.env.lookup_type_def(expected).map(|def| def.tag)
                }
                _ => false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span { start: 0, end: 1 }
    }

    fn wrapped(payload: Ty) -> Ty {
        Ty::Struct(
            "Identity".into(),
            NominalType::new(vec![payload], Vec::new()),
        )
    }

    fn annotated(name: &str) -> ResolvedPattern {
        ResolvedPattern::AnnotatedWildcard(span(), AstTy::Named(span(), name.into()))
    }

    #[test]
    fn missing_extraction_uses_direct_input_and_preserves_whole_input() {
        let mut checker = Checker::new(TypecheckContext::default());
        let rhs = wrapped(Ty::Int);
        for pattern in [ResolvedPattern::Wildcard(span()), annotated("Int")] {
            assert_eq!(
                checker
                    .safebind_missing_extraction_input(&pattern, &rhs, &Ty::Int)
                    .unwrap(),
                Some("Int".into())
            );
        }
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&annotated("String"), &rhs, &Ty::Int)
                .unwrap(),
            None
        );
        assert_eq!(
            checker
                .safebind_missing_extraction_input(
                    &ResolvedPattern::IntLit(span(), 1.into()),
                    &rhs,
                    &Ty::Int,
                )
                .unwrap(),
            Some("Int".into())
        );
    }

    #[test]
    fn missing_extraction_does_not_inspect_structural_children() {
        let mut checker = Checker::new(TypecheckContext::default());
        let list = Ty::List(Box::new(Ty::Str));
        let pattern = ResolvedPattern::ListCons(
            Box::new(annotated("Int")),
            Box::new(ResolvedPattern::Wildcard(span())),
        );
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&pattern, &wrapped(list.clone()), &list)
                .unwrap(),
            Some("List<String>".into())
        );
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&pattern, &list, &Ty::Str)
                .unwrap(),
            None,
            "once the outer input matches, child errors belong to the ordinary checker"
        );
        let tuple = ResolvedPattern::Tuple(vec![annotated("String")]);
        let payload = Ty::Tuple(vec![Ty::Int, Ty::Int]);
        assert!(
            checker
                .safebind_missing_extraction_input(&tuple, &wrapped(payload.clone()), &payload)
                .unwrap()
                .is_some(),
            "an extraction diagnostic guarantees the entry only, not child types or tuple arity"
        );
    }

    fn id(name: &str, unique_id: u32) -> ResolvedId {
        ResolvedId {
            name: name.into(),
            qualified_name: None,
            unique_id,
            compiler_generated: false,
            symbol_info: None,
            span: span(),
        }
    }

    #[test]
    fn missing_extraction_uses_alias_annotation_and_pin_input() {
        let mut checker = Checker::new(TypecheckContext::default());
        let rhs = Ty::List(Box::new(Ty::Int));
        let alias = ResolvedPattern::As(
            Box::new(ResolvedPattern::Wildcard(span())),
            id("whole", 900_001),
            Some(AstTy::Generic(
                span(),
                "List".into(),
                vec![AstTy::Named(span(), "Int".into())],
            )),
        );
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&alias, &rhs, &Ty::Int)
                .unwrap(),
            None
        );
        let alias = ResolvedPattern::As(
            Box::new(ResolvedPattern::Wildcard(span())),
            id("whole", 900_001),
            None,
        );
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&alias, &rhs, &Ty::Int)
                .unwrap(),
            Some("Int".into())
        );
        let pin = id("pinned", 900_002);
        checker.env.bind_var(pin.unique_id, Ty::Int);
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&ResolvedPattern::Pin(pin), &rhs, &Ty::Int)
                .unwrap(),
            Some("Int".into())
        );
    }

    #[test]
    fn missing_extraction_relation_probe_does_not_constrain_later_inputs() {
        let mut checker = Checker::new(TypecheckContext::default());
        let variable = checker.env.fresh_tyvar();
        let requirement =
            SafeBindInputRequirement::Exact(Ty::Tuple(vec![variable.clone(), Ty::Int]));
        assert!(!checker
            .safebind_direct_input_accepts(&requirement, &Ty::Tuple(vec![Ty::Bool, Ty::Str]))
            .unwrap());
        assert_eq!(checker.resolve_ty(&variable), variable);
    }

    #[test]
    fn missing_extraction_preserves_extractor_contract_and_whole_input() {
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        let head = id("payload_extractor", 900_010);
        checker.env.bind_var(
            head.unique_id,
            Ty::ExtractorClosure(Box::new(Ty::Func(
                vec![Ty::Int],
                Box::new(Ty::MatchResult(Box::new(Ty::Int))),
            ))),
        );
        let extractor = ResolvedPattern::Extractor(head.clone(), vec![], vec![annotated("String")]);
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&extractor, &wrapped(Ty::Int), &Ty::Int)
                .unwrap(),
            Some("Int".into())
        );
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&extractor, &Ty::Int, &Ty::Str)
                .unwrap(),
            None
        );
        checker.env.bind_var(
            head.unique_id,
            Ty::ExtractorClosure(Box::new(Ty::Func(
                vec![Ty::Str, Ty::Int],
                Box::new(Ty::MatchResult(Box::new(Ty::Int))),
            ))),
        );
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&extractor, &Ty::Int, &Ty::Str)
                .unwrap(),
            None,
            "whole-input pre-argument checking belongs to the ordinary path"
        );
        let variable = checker.env.fresh_tyvar();
        let sequence_head = id("sequence_extractor", 900_011);
        checker.env.bind_var(
            sequence_head.unique_id,
            Ty::ExtractorClosure(Box::new(Ty::Func(
                vec![Ty::List(Box::new(variable.clone()))],
                Box::new(Ty::MatchResult(Box::new(Ty::Int))),
            ))),
        );
        let sequence = ResolvedPattern::Extractor(sequence_head, vec![], vec![]);
        assert_eq!(
            checker
                .safebind_missing_extraction_input(
                    &sequence,
                    &Ty::List(Box::new(Ty::Int)),
                    &Ty::Str,
                )
                .unwrap(),
            None
        );
        assert_eq!(checker.resolve_ty(&variable), variable);
        let error = checker
            .safebind_missing_extraction_input(&extractor, &wrapped(Ty::Int), &Ty::Int)
            .unwrap_err();
        assert!(error.message.contains("pre-argument"), "{error:?}");
    }

    #[test]
    fn missing_extraction_preserves_forbidden_pattern_positions() {
        let mut checker = Checker::new(TypecheckContext::default());
        let pattern = ResolvedPattern::Or(vec![ResolvedPattern::Wildcard(span())]);
        assert_eq!(
            checker
                .safebind_missing_extraction_input(&pattern, &wrapped(Ty::Int), &Ty::Int)
                .unwrap(),
            None
        );
    }
}
