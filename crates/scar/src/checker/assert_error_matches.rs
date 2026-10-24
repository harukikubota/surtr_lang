use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

impl Checker {
    pub(super) fn ensure_assert_error_direct_type(
        &self,
        ty: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        if matches!(self.resolve_ty(ty),
            Ty::Func(ref parameters, _) | Ty::UserFunc { params: ref parameters, .. }
            | Ty::BuiltinFunc { params: ref parameters, .. } if parameters.is_empty())
        {
            return Err(TypeError::new(
                "assert_error_matches check with Pattern bindings requires a DirectExpression, not an external thunk",
                span.clone(),
            ));
        }
        Ok(())
    }

    pub(super) fn check_assert_error_matches(
        &mut self,
        span: &Span,
        value: &Resolved,
        success: &ResolvedMatchArm,
    ) -> Result<TypedNode, TypeError> {
        let mut value = self.check_node_with_expected(value, Some(&Ty::Error))?;
        self.assert_type_relation(
            &Ty::Error,
            &value.ty,
            self.type_fact(SourceRole::Expected, span, &Ty::Error),
            self.type_fact(SourceRole::Value, &value.span, &value.ty),
            TypeDiagnosticReason::ArgumentTypeMismatch,
            DiagnosticOrigin::Call,
            "assert_error_matches",
            0,
        )?;
        value.ty = self.resolve_ty(&value.ty);
        let result_ty = Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error));
        let provenance = self.constructor_capability_for_node(&value);
        self.env.push_var_scope();
        let saved_provenance = self.constructor_capabilities.clone();
        let saved_aliases = self.pattern_binding_aliases.clone();
        let checked = (|| {
            let saved_locations = self.preserve_match_pattern_locations;
            self.preserve_match_pattern_locations = true;
            let pattern = self.check_match_subpattern(&success.pattern, &Ty::Error);
            self.preserve_match_pattern_locations = saved_locations;
            let pattern = pattern?;
            self.ensure_assert_error_pattern(&pattern, span)?;
            self.bind_match_constructor_provenance(&pattern, (provenance, Ty::Error));
            let mut bindings = HashSet::new();
            Self::typed_match_pattern_bindings(&pattern, &mut bindings)?;
            let input = self.check_required_lazy_input(&success.body, &result_ty, span)?;
            let external_ty = if !bindings.is_empty()
                && Self::external_pattern_success_value(&input.node, &bindings)
            {
                self.ensure_assert_error_direct_type(&input.node.ty, &input.node.span)?;
                Some(input.node.ty.clone())
            } else {
                None
            };
            let check = self.normalize_lazy_single(input, &result_ty, span)?;
            self.assert_type_relation(
                &result_ty,
                &check.ty,
                self.type_fact(SourceRole::Expected, span, &result_ty),
                self.type_fact(SourceRole::Value, &check.span, &check.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "assert_error_matches",
                2,
            )?;
            if let Some(ty) = &external_ty {
                self.ensure_assert_error_direct_type(ty, &check.span)?;
            }
            Ok((pattern, check, external_ty))
        })();
        self.env.pop_var_scope();
        self.constructor_capabilities = saved_provenance;
        self.pattern_binding_aliases = saved_aliases;
        let (pattern, check, external_ty) = checked?;
        self.record_lazy_capture_signature(span, &result_ty);
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::AssertErrorMatches(
                Box::new(value),
                vec![TypedMatchArm {
                    direct_expression: false,
                    pattern,
                    guard: None,
                    body: check,
                }],
                external_ty,
            ),
        })
    }

    fn ensure_assert_error_pattern(
        &self,
        pattern: &TypedMatchPattern,
        span: &Span,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedMatchPattern::Located(source, _, inner) => {
                self.ensure_assert_error_pattern(inner, source)
            }
            TypedMatchPattern::Or(_) => Err(TypeError::new(
                "OR Pattern is not allowed in assert_error_matches",
                span.clone(),
            )),
            TypedMatchPattern::As(inner, _) => self.ensure_assert_error_pattern(inner, span),
            TypedMatchPattern::Tuple(items)
            | TypedMatchPattern::Record(items)
            | TypedMatchPattern::ErrorPayload { fields: items, .. }
            | TypedMatchPattern::Constructor { fields: items, .. }
            | TypedMatchPattern::Extractor { items, .. } => {
                for item in items {
                    self.ensure_assert_error_pattern(item, span)?;
                }
                Ok(())
            }
            TypedMatchPattern::ListCons(head, tail) => {
                self.ensure_assert_error_pattern(head, span)?;
                self.ensure_assert_error_pattern(tail, span)
            }
            TypedMatchPattern::HashMap(entries) => {
                for entry in entries {
                    self.ensure_assert_error_pattern(&entry.pattern, span)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
