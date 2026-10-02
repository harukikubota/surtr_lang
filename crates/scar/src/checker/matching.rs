use super::expr::ExpectedTypeRelation;
use super::*;
use diagnostics::{DiagnosticOrigin, PatternKind, SourceRole, TypeDiagnosticReason};

impl Checker {
    pub(super) fn resolved_pattern_span(pattern: &ResolvedPattern) -> Span {
        match pattern {
            ResolvedPattern::Deferred { pattern, .. } => Self::resolved_pattern_span(pattern),
            ResolvedPattern::ExtractorApplication { head, .. }
            | ResolvedPattern::Projection { id: head, .. } => head.span.clone(),
            ResolvedPattern::Var(id)
            | ResolvedPattern::Annotated(id, _)
            | ResolvedPattern::Pin(id) => id.span.clone(),
            ResolvedPattern::Wildcard(span)
            | ResolvedPattern::AnnotatedWildcard(span, _)
            | ResolvedPattern::ListNil(span)
            | ResolvedPattern::IntLit(span, _)
            | ResolvedPattern::StrLit(span, _)
            | ResolvedPattern::BoolLit(span, _)
            | ResolvedPattern::DurationLit(span, _) => span.clone(),
            ResolvedPattern::ListCons(head, _) | ResolvedPattern::As(head, _, _) => {
                Self::resolved_pattern_span(head)
            }
            ResolvedPattern::Constructor(id, _)
            | ResolvedPattern::Extractor(id, _, _)
            | ResolvedPattern::Record(id, _) => id.span.clone(),
            ResolvedPattern::Tuple(items) | ResolvedPattern::Or(items) => items
                .first()
                .map(Self::resolved_pattern_span)
                .unwrap_or(Span { start: 0, end: 0 }),
        }
    }

    pub(super) fn check_match(
        &mut self,
        span: &Span,
        scrutinee: &Resolved,
        arms: &[ResolvedMatchArm],
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        // A polymorphic constructor used as the scrutinee (for example
        // `Err(NoneError)`) cannot be inferred in isolation: without an
        // expected type its payload type defaults to `Unit`.  Match patterns
        // are an equally valid source of constraints, so seed the scrutinee
        // from the first informative pattern and let arm bindings/body
        // expressions refine the fresh variables it contains.
        let pattern_hint = arms
            .iter()
            .find_map(|arm| self.infer_match_pattern_ty(&arm.pattern));
        let mut typed_scrut = match pattern_hint.as_ref() {
            Some(expected) => self.check_node_with_expected(scrutinee, Some(expected))?,
            None => self.check_node(scrutinee)?,
        };
        if Self::is_lazy_placeholder(scrutinee) {
            if let Some(hint) = pattern_hint.as_ref() {
                self.assert_type_relation(
                    hint,
                    &typed_scrut.ty,
                    self.type_fact(SourceRole::Expected, span, hint),
                    self.type_fact(SourceRole::Value, &typed_scrut.span, &typed_scrut.ty),
                    TypeDiagnosticReason::ArgumentTypeMismatch,
                    DiagnosticOrigin::Call,
                    "Pattern",
                    0,
                )?;
                typed_scrut.ty = self.resolve_ty(&typed_scrut.ty);
            }
        }
        self.ensure_no_match_result_value(&typed_scrut.ty, &typed_scrut.span)?;
        let callable_expected = self.contextual_callable_expected(expected);
        let mut typed_arms = Vec::new();
        let mut result_ty: Option<Ty> = None;
        let mut failure = None;

        let scrutinee_provenance = self.constructor_capability_for_node(&typed_scrut);
        for (ordinal, arm) in arms.iter().enumerate() {
            let mut typed_arm = self.check_match_arm(
                arm,
                &typed_scrut.ty,
                &scrutinee_provenance,
                span,
                expected,
                expected_relation,
            )?;
            if let Some(ref rt) = result_ty {
                let coerce = self.with_type_relation_probe(
                    &[rt, &typed_arm.body.ty, &typed_scrut.ty],
                    |checker| {
                        !checker.types_compatible(rt, &typed_arm.body.ty)
                            && checker.can_coerce_err_only_result_self_arm(
                                &typed_scrut,
                                &typed_arms,
                                &typed_arm,
                                rt,
                            )
                    },
                );
                if coerce {
                    typed_arm.body.ty = self.resolve_ty(rt);
                }
            }
            let body_node = &typed_arm.body;
            if let Some(first) = typed_arms.first().filter(|_| callable_expected.is_none()) {
                let first: &TypedMatchArm = first;
                let relation = self.assert_type_relation(
                    &first.body.ty,
                    &body_node.ty,
                    self.branch_fact(SourceRole::Branch, &first.body, 0),
                    self.branch_fact(SourceRole::Branch, body_node, ordinal),
                    TypeDiagnosticReason::MatchArmTypeMismatch,
                    DiagnosticOrigin::Branch {
                        form: diagnostics::BranchForm::Match,
                        ordinal: ordinal as u32,
                    },
                    "match",
                    ordinal as u32,
                );
                if failure.is_none() {
                    failure = relation.err();
                }
            }
            if let Some(expected) = expected {
                let relation = self.assert_value_type_relation(
                    expected,
                    &body_node.ty,
                    self.type_fact(SourceRole::Expected, span, expected),
                    self.branch_fact(SourceRole::Branch, body_node, ordinal),
                    TypeDiagnosticReason::MatchArmTypeMismatch,
                    DiagnosticOrigin::Branch {
                        form: diagnostics::BranchForm::Match,
                        ordinal: ordinal as u32,
                    },
                    "match",
                    ordinal as u32,
                );
                if failure.is_none() {
                    failure = relation.err();
                }
            }
            if result_ty.is_none() {
                result_ty = Some(body_node.ty.clone());
            }
            typed_arms.push(typed_arm);
        }

        if let Some(error) = failure {
            return Err(self.complete_branch_error(
                error,
                &typed_arms.iter().map(|arm| &arm.body).collect::<Vec<_>>(),
                &typed_arms
                    .iter()
                    .map(|arm| arm.guard.as_ref())
                    .collect::<Vec<_>>(),
            ));
        }
        // Arm-local bindings are rolled back by each arm scope. Keep typed arm
        // subtrees unresolved here and let check_program do one final pass.
        self.check_match_exhaustive(span, &typed_scrut.ty, &typed_arms)?;

        let ty = callable_expected.or(result_ty).unwrap_or(Ty::Unit);
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Match(Box::new(typed_scrut), typed_arms),
        })
    }

    /// Infer a type skeleton from a pattern before the scrutinee is checked.
    /// Binding variables deliberately become fresh inference variables.  The
    /// arm body can then constrain them (e.g. `print(name)` constrains `name`
    /// to `String`) and that constraint flows back into the scrutinee type.
    pub(super) fn infer_match_pattern_ty(&mut self, pat: &ResolvedPattern) -> Option<Ty> {
        match pat {
            ResolvedPattern::Deferred { pattern, .. } => self.infer_match_pattern_ty(pattern),
            ResolvedPattern::Projection {
                inner, annotation, ..
            } => annotation
                .as_ref()
                .and_then(|ty| {
                    self.resolve_ast_ty_in_context(ty, self.local_type_syntax_context())
                        .ok()
                })
                .or_else(|| self.infer_match_pattern_ty(inner)),
            ResolvedPattern::ExtractorApplication { .. } => None,
            ResolvedPattern::Var(_) | ResolvedPattern::Wildcard(_) | ResolvedPattern::Pin(_) => {
                None
            }
            ResolvedPattern::Annotated(_, ast_ty)
            | ResolvedPattern::AnnotatedWildcard(_, ast_ty) => self
                .resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())
                .ok(),
            ResolvedPattern::As(inner, _, alias_ty) => alias_ty
                .as_ref()
                .and_then(|ast_ty| {
                    self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())
                        .ok()
                })
                .or_else(|| self.infer_match_pattern_ty(inner)),
            ResolvedPattern::BoolLit(_, _) => Some(Ty::Bool),
            ResolvedPattern::IntLit(_, _) => Some(Ty::Int),
            ResolvedPattern::StrLit(_, _) => Some(Ty::Str),
            ResolvedPattern::DurationLit(_, _) => Some(Ty::Struct(
                "Duration".into(),
                NominalType::monomorphic(Vec::new()),
            )),
            ResolvedPattern::Tuple(items) => Some(Ty::Tuple(
                items
                    .iter()
                    .map(|item| {
                        self.infer_match_pattern_ty(item)
                            .unwrap_or_else(|| self.env.fresh_tyvar())
                    })
                    .collect(),
            )),
            ResolvedPattern::Or(items) => items
                .iter()
                .find_map(|item| self.infer_match_pattern_ty(item)),
            // These patterns are also valid string patterns (`[]` is the
            // empty string and cons is string decomposition), so they are
            // ambiguous without a scrutinee hint.
            ResolvedPattern::ListNil(_) | ResolvedPattern::ListCons(_, _) => None,
            ResolvedPattern::Constructor(id, _) => match id.name.as_str() {
                "Ok" | "Result::Ok" => Some(Ty::Result(
                    Box::new(self.env.fresh_tyvar()),
                    Box::new(Ty::Error),
                )),
                "Err" | "Result::Err" => Some(Ty::Result(
                    Box::new(self.env.fresh_tyvar()),
                    Box::new(Ty::Error),
                )),
                _ => self
                    .lookup_enum_variant_by_constructor_id(id.unique_id)
                    .map(|variant| self.instantiate_enum_variant(&variant).enum_ty),
            },
            ResolvedPattern::Record(id, _) => self.env.lookup_type_def(&id.name).map(|def| {
                Ty::Record(
                    def.name.clone(),
                    crate::types::NominalType::monomorphic(def.fields.clone()),
                )
            }),
            // Extractor patterns need the scrutinee type supplied by the
            // extractor contract; they remain checked by the normal path.
            ResolvedPattern::Extractor(_, _, _) => None,
        }
    }

    fn can_coerce_err_only_result_self_arm(
        &mut self,
        scrutinee: &TypedNode,
        previous_arms: &[TypedMatchArm],
        arm: &TypedMatchArm,
        expected_ty: &Ty,
    ) -> bool {
        if arm.guard.is_some() || !matches!(arm.pattern, TypedMatchPattern::Wildcard) {
            return false;
        }

        let (scrut_ok, scrut_err) = match self.resolve_ty(&scrutinee.ty) {
            Ty::Result(ok, err) => (ok, err),
            _ => return false,
        };
        let (expected_ok, expected_err) = match self.resolve_ty(expected_ty) {
            Ty::Result(ok, err) => (ok, err),
            _ => return false,
        };

        if !self.types_compatible(scrut_err.as_ref(), expected_err.as_ref()) {
            return false;
        }

        if self.types_compatible(scrut_ok.as_ref(), expected_ok.as_ref()) {
            return false;
        }

        let (scrut_id, body_id) = match (&scrutinee.node, &arm.body.node) {
            (TypedInner::Var(scrut_id), TypedInner::Var(body_id)) => {
                (scrut_id.unique_id, body_id.unique_id)
            }
            _ => return false,
        };
        if scrut_id != body_id {
            return false;
        }

        previous_arms.iter().any(|prev_arm| {
            prev_arm.guard.is_none()
                && Self::match_or_alternative_matches(&prev_arm.pattern, &|pattern| {
                    matches!(pattern, TypedMatchPattern::Constructor { tag: 0, .. })
                })
        })
    }

    pub(super) fn check_match_exhaustive(
        &self,
        span: &Span,
        scrut_ty: &Ty,
        arms: &[TypedMatchArm],
    ) -> Result<(), TypeError> {
        let profile = self.profiler.start();
        if arms
            .iter()
            .any(|arm| arm.guard.is_none() && self.is_match_catch_all(&arm.pattern))
        {
            self.profiler.finish(ProfileEvent::MatchExhaustive, profile);
            return Ok(());
        }

        let result = match scrut_ty {
            Ty::Bool => self.check_enum_like_match_exhaustive(span, "Boolean", scrut_ty, arms),
            Ty::Result(_, _) => {
                let has_ok = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && Self::match_or_alternative_matches(&arm.pattern, &|pattern| {
                            matches!(pattern, TypedMatchPattern::Constructor { tag: 0, .. })
                        })
                });
                let has_err = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && Self::match_or_alternative_matches(&arm.pattern, &|pattern| {
                            matches!(pattern, TypedMatchPattern::Constructor { tag: 1, .. })
                        })
                });

                if has_ok && has_err {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_ok {
                        missing.push("Ok".into());
                    }
                    if !has_err {
                        missing.push("Err".into());
                    }
                    Err(self.pattern_error(
                        TypeDiagnosticReason::NonExhaustiveMatch,
                        PatternKind::Match,
                        None,
                        None,
                        Some(scrut_ty),
                        None,
                        None,
                        missing.into_iter().map(str::to_string).collect(),
                        span,
                    ))
                }
            }
            Ty::Enum(enum_name, _) => {
                self.check_enum_like_match_exhaustive(span, enum_name, scrut_ty, arms)
            }
            Ty::List(_) => {
                let has_nil = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && Self::match_or_alternative_matches(&arm.pattern, &|pattern| {
                            matches!(pattern, TypedMatchPattern::ListNil)
                        })
                });
                let has_cons = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && Self::match_or_alternative_matches(&arm.pattern, &|pattern| {
                            matches!(pattern, TypedMatchPattern::ListCons(_, _))
                        })
                });
                if has_nil && has_cons {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_nil {
                        missing.push("[]".into());
                    }
                    if !has_cons {
                        missing.push("[head, ..tail]".into());
                    }
                    Err(self.pattern_error(
                        TypeDiagnosticReason::NonExhaustiveMatch,
                        PatternKind::Match,
                        None,
                        None,
                        Some(scrut_ty),
                        None,
                        None,
                        missing,
                        span,
                    ))
                }
            }
            Ty::Str => {
                let has_empty = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && Self::match_or_alternative_matches(&arm.pattern, &|pattern| {
                            matches!(pattern, TypedMatchPattern::StrLit(value) if value.is_empty())
                        })
                });
                let has_cons = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && Self::match_or_alternative_matches(&arm.pattern, &|pattern| {
                            matches!(
                                pattern,
                                TypedMatchPattern::Extractor {
                                    input_ty,
                                    extractor,
                                    ..
                                } if extractor.name == "uncons"
                                    && matches!(self.resolve_ty(input_ty), Ty::Str)
                            )
                        })
                });
                if has_empty && has_cons {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_empty {
                        missing.push("[]".into());
                    }
                    if !has_cons {
                        missing.push("[head, ..tail]".into());
                    }
                    Err(self.pattern_error(
                        TypeDiagnosticReason::NonExhaustiveMatch,
                        PatternKind::Match,
                        None,
                        None,
                        Some(scrut_ty),
                        None,
                        None,
                        missing,
                        span,
                    ))
                }
            }
            _ => Err(self.pattern_error(
                TypeDiagnosticReason::NonExhaustiveMatch,
                PatternKind::Match,
                None,
                None,
                Some(scrut_ty),
                None,
                None,
                vec!["_".into()],
                span,
            )),
        };
        self.profiler.finish(ProfileEvent::MatchExhaustive, profile);
        result
    }

    fn check_enum_like_match_exhaustive(
        &self,
        span: &Span,
        enum_name: &str,
        scrut_ty: &Ty,
        arms: &[TypedMatchArm],
    ) -> Result<(), TypeError> {
        let variants = self.lookup_enum_variants_of(enum_name).ok_or_else(|| {
            self.typecheck_invariant_error(
                "enum metadata required for exhaustiveness checking",
                span,
            )
        })?;
        let mut missing = Vec::new();
        for variant in variants {
            let covered = arms.iter().any(|arm| {
                arm.guard.is_none()
                    && Self::match_arm_covers_enum_variant(enum_name, &arm.pattern, variant)
            });
            if !covered {
                missing.push(variant.short_name.clone());
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(self.pattern_error(
                TypeDiagnosticReason::NonExhaustiveMatch,
                PatternKind::Match,
                Some(enum_name.to_string()),
                None,
                Some(scrut_ty),
                None,
                None,
                missing,
                span,
            ))
        }
    }

    fn match_arm_covers_enum_variant(
        enum_name: &str,
        pattern: &TypedMatchPattern,
        variant: &crate::env::EnumVariantInfo,
    ) -> bool {
        Self::match_or_alternative_matches(pattern, &|pattern| match pattern {
            TypedMatchPattern::Constructor { tag, .. } => *tag == variant.tag,
            TypedMatchPattern::BoolLit(value) if Self::surface_name(enum_name) == "Boolean" => {
                variant.short_name == if *value { "True" } else { "False" }
            }
            _ => false,
        })
    }

    fn match_or_alternative_matches(
        pattern: &TypedMatchPattern,
        predicate: &impl Fn(&TypedMatchPattern) -> bool,
    ) -> bool {
        match pattern {
            TypedMatchPattern::Or(items) => items
                .iter()
                .any(|item| Self::match_or_alternative_matches(item, predicate)),
            TypedMatchPattern::As(inner, _) => Self::match_or_alternative_matches(inner, predicate),
            other => predicate(other),
        }
    }

    pub(super) fn check_match_arm(
        &mut self,
        arm: &ResolvedMatchArm,
        scrut_ty: &Ty,
        scrutinee_provenance: &ConstructorCapabilityProvenance,
        _span: &Span,
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedMatchArm, TypeError> {
        let profile = self.profiler.start();
        self.env.push_var_scope();
        let saved_provenance = self.constructor_capabilities.clone();
        let result = (|| {
            let typed_pat = self.check_match_subpattern(&arm.pattern, scrut_ty)?;
            self.bind_match_constructor_provenance(
                &typed_pat,
                (scrutinee_provenance.clone(), scrut_ty.clone()),
            );
            let typed_guard = if let Some(guard) = &arm.guard {
                let typed_guard = self.check_node(guard)?;
                if !self.types_compatible(&Ty::Bool, &typed_guard.ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::MatchGuardTypeMismatch,
                        PatternKind::Match,
                        Some("match guard".into()),
                        Some("Boolean".into()),
                        Some(&typed_guard.ty),
                        None,
                        None,
                        Vec::new(),
                        &typed_guard.span,
                    ));
                }
                Some(typed_guard)
            } else {
                None
            };
            let typed_body = match expected {
                Some(expected) if expected_relation.is_some() => self
                    .check_node_with_expected_relation(
                        &arm.body,
                        Some(expected),
                        expected_relation,
                    )?,
                Some(expected) => self.check_branch_node_with_expected(&arm.body, expected)?,
                None => self.check_node(&arm.body)?,
            };
            // Do not normalize env bindings or typed guard/body subtrees in this
            // scoped arm. The env frame is discarded below, and the containing
            // TypedInner::Match is normalized once at the program boundary.
            Ok(TypedMatchArm {
                direct_expression: false,
                pattern: typed_pat,
                guard: typed_guard,
                body: typed_body,
            })
        })();
        self.env.pop_var_scope();
        self.constructor_capabilities = saved_provenance;
        self.profiler.finish(ProfileEvent::MatchArm, profile);
        result
    }

    pub(super) fn check_match_subpattern(
        &mut self,
        pat: &ResolvedPattern,
        expected_ty: &Ty,
    ) -> Result<TypedMatchPattern, TypeError> {
        self.ensure_no_match_result_value(expected_ty, &Self::resolved_pattern_span(pat))?;
        if let ResolvedPattern::Pin(id) = pat {
            let canonical = self.canonical_pattern_id(id)?;
            if canonical.unique_id != id.unique_id {
                return self.check_match_subpattern(&ResolvedPattern::Pin(canonical), expected_ty);
            }
        }
        match pat {
            ResolvedPattern::Projection { id, .. } => Err(self.projection_shape_error(
                "the apply_pattern consumer",
                "a projection outside apply_pattern",
                &id.span,
            )),
            ResolvedPattern::Deferred {
                pattern,
                bindings,
                allow_bindings,
            } => {
                let result = self.check_match_subpattern(pattern, expected_ty)?;
                let mut selected = HashSet::new();
                Self::typed_match_pattern_bindings(&result, &mut selected)?;
                self.finalize_pattern_bindings(bindings, &selected, *allow_bindings)?;
                Ok(result)
            }
            ResolvedPattern::ExtractorApplication { head, args } => {
                let selected = self.select_extractor_application(head, args)?;
                self.check_match_subpattern(&selected, expected_ty)
            }
            ResolvedPattern::Var(id) => {
                self.env
                    .bind_var(id.unique_id, self.resolve_ty(expected_ty));
                Ok(TypedMatchPattern::Binding(id.clone()))
            }
            ResolvedPattern::AnnotatedWildcard(span, _) => {
                self.check_pattern(pat, expected_ty, span)?;
                Ok(TypedMatchPattern::Wildcard)
            }
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected =
                    self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                if !self.types_compatible(&expected, expected_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("annotated".into()),
                        Some(self.ty_name(&expected)),
                        Some(expected_ty),
                        None,
                        None,
                        Vec::new(),
                        &id.span,
                    ));
                }
                let bind_ty = self.resolve_ty(&expected);
                self.env.bind_var(id.unique_id, bind_ty);
                Ok(TypedMatchPattern::Binding(id.clone()))
            }
            ResolvedPattern::Pin(id) => {
                let pinned_ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(|| {
                    self.typecheck_invariant_error(
                        format!("resolved pinned pattern `{}` has no binding", id.name),
                        &id.span,
                    )
                })?;
                let expected_ty = self.resolve_ty(expected_ty);
                let pinned_ty = self.resolve_ty(&pinned_ty);
                if !self.types_compatible(&pinned_ty, &expected_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Pin,
                        Some("pinned value".into()),
                        Some(self.ty_name(&pinned_ty)),
                        Some(&expected_ty),
                        None,
                        None,
                        Vec::new(),
                        &id.span,
                    ));
                }
                let dispatch = self.eq_dispatch_for_pattern_pin(&expected_ty, &id.span)?;
                Ok(TypedMatchPattern::Pin {
                    id: id.clone(),
                    ty: expected_ty,
                    dispatch,
                })
            }
            ResolvedPattern::As(inner, alias, alias_ty) => {
                let typed_inner = self.check_match_subpattern(inner, expected_ty)?;
                let alias_bind_ty = if let Some(ast_ty) = alias_ty {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                    if !self.types_compatible(&expected, expected_ty) {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternTypeMismatch,
                            PatternKind::Other,
                            Some("as-pattern annotation".into()),
                            Some(self.ty_name(&expected)),
                            Some(expected_ty),
                            None,
                            None,
                            Vec::new(),
                            &alias.span,
                        ));
                    }
                    self.resolve_ty(&expected)
                } else {
                    self.resolve_ty(expected_ty)
                };
                self.env.bind_var(alias.unique_id, alias_bind_ty);
                Ok(TypedMatchPattern::As(Box::new(typed_inner), alias.clone()))
            }
            ResolvedPattern::Wildcard(_) => Ok(TypedMatchPattern::Wildcard),
            ResolvedPattern::Tuple(items) => {
                let expected_ty = self.resolve_ty(expected_ty);
                let span = Self::resolved_pattern_span(pat);
                let Ty::Tuple(item_tys) = &expected_ty else {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::Tuple,
                        Some("tuple".into()),
                        Some("tuple".into()),
                        Some(&expected_ty),
                        None,
                        None,
                        Vec::new(),
                        &span,
                    ));
                };
                if items.len() != item_tys.len() {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternArityMismatch,
                        PatternKind::Tuple,
                        Some("tuple".into()),
                        None,
                        Some(&expected_ty),
                        Some(item_tys.len()),
                        Some(items.len()),
                        Vec::new(),
                        &span,
                    ));
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for (item, item_ty) in items.iter().zip(item_tys.iter()) {
                    typed_items.push(self.check_match_subpattern(item, item_ty)?);
                }
                Ok(TypedMatchPattern::Tuple(typed_items))
            }
            ResolvedPattern::Record(id, fields) => {
                let (_, ordered) = self.ordered_record_pattern_fields(id, fields, &expected_ty)?;
                let mut typed = Vec::with_capacity(ordered.len());
                for (item, field_ty) in ordered {
                    typed.push(self.check_match_subpattern(item, &field_ty)?);
                }
                Ok(TypedMatchPattern::Record(typed))
            }
            ResolvedPattern::BoolLit(span, b) => {
                if !self.types_compatible(&Ty::Bool, expected_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("Boolean literal".into()),
                        Some("Boolean".into()),
                        Some(expected_ty),
                        None,
                        None,
                        Vec::new(),
                        span,
                    ));
                }
                Ok(TypedMatchPattern::BoolLit(*b))
            }
            ResolvedPattern::IntLit(span, n) => {
                if !self.types_compatible(&Ty::Int, expected_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("Int literal".into()),
                        Some("Int".into()),
                        Some(expected_ty),
                        None,
                        None,
                        Vec::new(),
                        span,
                    ));
                }
                Ok(TypedMatchPattern::IntLit(n.clone()))
            }
            ResolvedPattern::StrLit(span, s) => {
                if !self.types_compatible(&Ty::Str, expected_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("String literal".into()),
                        Some("String".into()),
                        Some(expected_ty),
                        None,
                        None,
                        Vec::new(),
                        span,
                    ));
                }
                Ok(TypedMatchPattern::StrLit(s.clone()))
            }
            ResolvedPattern::DurationLit(span, n) => {
                let expected_ty = self.resolve_ty(expected_ty);
                if !Self::is_duration_ty(&expected_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("duration literal".into()),
                        Some("Duration".into()),
                        Some(&expected_ty),
                        None,
                        None,
                        Vec::new(),
                        span,
                    ));
                }
                Ok(TypedMatchPattern::DurationLit(n.clone()))
            }
            ResolvedPattern::Or(items) => {
                if items.is_empty() {
                    let span = Self::resolved_pattern_span(pat);
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::Other,
                        Some("pattern alternative".into()),
                        Some("at least one alternative".into()),
                        Some(expected_ty),
                        Some(1),
                        Some(0),
                        Vec::new(),
                        &span,
                    ));
                }
                let mut typed_items = Vec::with_capacity(items.len());
                let mut common_bindings: Option<Vec<(ResolvedId, Ty)>> = None;
                for item in items {
                    self.env.push_var_scope();
                    let checked = (|| {
                        let typed_item = self.check_match_subpattern(item, expected_ty)?;
                        Self::typed_match_pattern_bindings(&typed_item, &mut HashSet::new())?;
                        let mut ids = Vec::new();
                        Self::collect_or_binding_ids(&typed_item, &mut ids);
                        let bindings = ids
                            .into_iter()
                            .map(|id| {
                                let ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(
                                    || {
                                        self.typecheck_invariant_error(
                                            format!(
                                                "OR alternative binding `{}` has no inferred type",
                                                id.name
                                            ),
                                            &id.span,
                                        )
                                    },
                                )?;
                                Ok((id, ty))
                            })
                            .collect::<Result<Vec<_>, TypeError>>()?;
                        Ok((typed_item, bindings))
                    })();
                    self.env.pop_var_scope();
                    let (typed_item, bindings) = checked?;
                    if let Some(common) = &common_bindings {
                        if common.len() != bindings.len() {
                            return Err(Self::deferred_pattern_error(
                                "Pattern alternatives must bind the same variables",
                                &common
                                    .first()
                                    .or(bindings.first())
                                    .expect("different binding counts have a binding")
                                    .0,
                                diagnostics::ResolveDiagnosticReason::Pattern,
                            ));
                        }
                        for ((expected_id, expected_ty), (id, actual_ty)) in
                            common.iter().zip(bindings.iter())
                        {
                            if expected_id.unique_id != id.unique_id || expected_id.name != id.name
                            {
                                return Err(Self::deferred_pattern_error("Pattern alternatives must bind the same variables in the same order", id, diagnostics::ResolveDiagnosticReason::Pattern));
                            }
                            if !self.types_compatible(expected_ty, actual_ty)
                                || self.resolve_ty(expected_ty) != self.resolve_ty(actual_ty)
                            {
                                return Err(self.pattern_error(
                                    TypeDiagnosticReason::PatternTypeMismatch,
                                    PatternKind::Other,
                                    Some(format!("OR pattern binding `{}`", id.name)),
                                    Some(self.ty_name(&self.resolve_ty(expected_ty))),
                                    Some(actual_ty),
                                    None,
                                    None,
                                    Vec::new(),
                                    &id.span,
                                ));
                            }
                        }
                    } else {
                        common_bindings = Some(bindings);
                    }
                    typed_items.push(typed_item);
                }
                for (id, ty) in common_bindings.unwrap_or_default() {
                    self.env.bind_var(id.unique_id, self.resolve_ty(&ty));
                }
                Ok(TypedMatchPattern::Or(typed_items))
            }
            ResolvedPattern::Constructor(ctor_id, inner_pats) => {
                if matches!(self.resolve_ty(expected_ty), Ty::Error)
                    && matches!(ctor_id.name.as_str(), "Err" | "Result::Err")
                {
                    return Err(self
                        .pattern_error(
                            TypeDiagnosticReason::NestedResultErrorPattern,
                            PatternKind::Constructor,
                            Some("nested Err".into()),
                            Some("the current Result layer".into()),
                            Some(expected_ty),
                            None,
                            None,
                            vec!["Use Err(error) for the outer failure or Ok(Err(error)) for an inner failure".into()],
                            &ctor_id.span,
                        )
                        .with_hint(
                            "Err matches the Result layer being inspected; do not write Err(Err(...)).",
                        ));
                }
                if matches!(expected_ty, Ty::Error)
                    && self.env.is_error_constructor(ctor_id.unique_id)
                {
                    if !inner_pats.is_empty() {
                        return Err(self
                            .pattern_error(
                                TypeDiagnosticReason::PatternShapeMismatch,
                                PatternKind::Constructor,
                                Some("Error kind".into()),
                                Some("a payload-free pattern".into()),
                                Some(expected_ty),
                                Some(0),
                                Some(inner_pats.len()),
                                Vec::new(),
                                &ctor_id.span,
                            )
                            .with_hint("Use `Kind @ err: Error` and inspect the Error value."));
                    }
                    return Ok(TypedMatchPattern::ErrorKind(ctor_id.name.clone()));
                }
                if matches!(expected_ty, Ty::Bool) {
                    let variant = self
                        .lookup_enum_variant_by_constructor_id(ctor_id.unique_id)
                        .ok_or_else(|| {
                            self.typecheck_invariant_error(
                                format!(
                                    "resolved constructor `{}` has no variant metadata",
                                    ctor_id.name
                                ),
                                &ctor_id.span,
                            )
                        })?
                        .clone();
                    let variant = self.instantiate_enum_variant(&variant);
                    if Self::surface_name(&variant.enum_name) != "Boolean" {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternShapeMismatch,
                            PatternKind::Constructor,
                            Some(ctor_id.name.clone()),
                            Some("constructor of Boolean".into()),
                            Some(expected_ty),
                            None,
                            None,
                            vec![format!("constructor belongs to {}", variant.enum_name)],
                            &ctor_id.span,
                        ));
                    }
                    if !inner_pats.is_empty() {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternArityMismatch,
                            PatternKind::Constructor,
                            Some(ctor_id.name.clone()),
                            None,
                            Some(expected_ty),
                            Some(0),
                            Some(inner_pats.len()),
                            Vec::new(),
                            &ctor_id.span,
                        ));
                    }
                    return match variant.short_name.as_str() {
                        "True" => Ok(TypedMatchPattern::BoolLit(true)),
                        "False" => Ok(TypedMatchPattern::BoolLit(false)),
                        _ => Err(self.typecheck_invariant_error(
                            format!("unknown Boolean constructor `{}`", ctor_id.name),
                            &ctor_id.span,
                        )),
                    };
                }
                if let Ty::Result(ok_ty, err_ty) = expected_ty {
                    let tag = match ctor_id.name.as_str() {
                        "Ok" => 0u32,
                        "Err" => 1u32,
                        _ => {
                            return Err(self.typecheck_invariant_error(
                                format!(
                                    "resolved Result constructor `{}` is unknown",
                                    ctor_id.name
                                ),
                                &ctor_id.span,
                            ));
                        }
                    };
                    if inner_pats.len() != 1 {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternArityMismatch,
                            PatternKind::Constructor,
                            Some(ctor_id.name.clone()),
                            None,
                            Some(expected_ty),
                            Some(1),
                            Some(inner_pats.len()),
                            Vec::new(),
                            &ctor_id.span,
                        ));
                    }
                    let inner_ty = match tag {
                        0 => ok_ty.as_ref().clone(),
                        1 => err_ty.as_ref().clone(),
                        _ => {
                            return Err(self.typecheck_invariant_error(
                                format!("invalid Result constructor tag {tag}"),
                                &ctor_id.span,
                            ));
                        }
                    };
                    let typed_inner = self.check_match_subpattern(&inner_pats[0], &inner_ty)?;
                    return Ok(TypedMatchPattern::Constructor {
                        tag,
                        fields: vec![typed_inner],
                        field_offset: 0,
                    });
                }

                let Ty::Enum(expected_enum_name, _) = expected_ty else {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        Some("enum or Result scrutinee".into()),
                        Some(expected_ty),
                        None,
                        None,
                        Vec::new(),
                        &ctor_id.span,
                    ));
                };
                let variant = self
                    .lookup_enum_variant_by_constructor_id(ctor_id.unique_id)
                    .ok_or_else(|| {
                        self.typecheck_invariant_error(
                            format!(
                                "resolved constructor `{}` has no variant metadata",
                                ctor_id.name
                            ),
                            &ctor_id.span,
                        )
                    })?
                    .clone();
                let variant = self.instantiate_enum_variant(&variant);
                if &variant.enum_name != expected_enum_name {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        Some(expected_enum_name.clone()),
                        Some(&variant.enum_ty),
                        None,
                        None,
                        vec![format!("constructor belongs to {}", variant.enum_name)],
                        &ctor_id.span,
                    ));
                }
                if !self.types_compatible(&variant.enum_ty, expected_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        Some(self.ty_name(expected_ty)),
                        Some(&variant.enum_ty),
                        None,
                        None,
                        Vec::new(),
                        &ctor_id.span,
                    ));
                }
                if inner_pats.len() != variant.payload.len() {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternArityMismatch,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        None,
                        Some(expected_ty),
                        Some(variant.payload.len()),
                        Some(inner_pats.len()),
                        Vec::new(),
                        &ctor_id.span,
                    ));
                }
                let mut typed_fields = Vec::new();
                for (pat, field_ty) in inner_pats.iter().zip(variant.payload.iter()) {
                    let resolved_field_ty = self.resolve_ty(field_ty);
                    typed_fields.push(self.check_match_subpattern(pat, &resolved_field_ty)?);
                }
                Ok(TypedMatchPattern::Constructor {
                    tag: variant.tag,
                    fields: typed_fields,
                    field_offset: 1,
                })
            }
            ResolvedPattern::ListNil(span) => match self.resolve_ty(expected_ty) {
                Ty::List(_) => Ok(TypedMatchPattern::ListNil),
                Ty::Str => Ok(TypedMatchPattern::StrLit(String::new())),
                other => Err(self.pattern_error(
                    TypeDiagnosticReason::PatternShapeMismatch,
                    PatternKind::List,
                    Some("empty list".into()),
                    Some("List<...> or String".into()),
                    Some(&other),
                    None,
                    None,
                    Vec::new(),
                    span,
                )),
            },
            ResolvedPattern::ListCons(head, tail) => match self.resolve_ty(expected_ty) {
                Ty::List(inner) => {
                    let elem_ty = inner.as_ref().clone();
                    let typed_head = self.check_match_subpattern(head, &elem_ty)?;
                    let tail_ty = Ty::List(Box::new(elem_ty));
                    let typed_tail = self.check_match_subpattern(tail, &tail_ty)?;
                    Ok(TypedMatchPattern::ListCons(
                        Box::new(typed_head),
                        Box::new(typed_tail),
                    ))
                }
                Ty::Str => {
                    let pattern_span = Self::resolved_pattern_span(pat);
                    let extractor_id = self.kernel_uncons_id(&pattern_span)?;
                    let (input_ty, extractor_ty, pre_args, seq_tys, success_tag, err_tag) = self
                        .extractor_contract_for_observed_ty(
                            &extractor_id,
                            &Ty::Str,
                            &[],
                            &extractor_id.span,
                        )?;
                    debug_assert_eq!(seq_tys.len(), 2);
                    let mut typed_items = Vec::with_capacity(2);
                    typed_items.push(self.check_match_subpattern(head, &seq_tys[0])?);
                    typed_items.push(self.check_match_subpattern(tail, &seq_tys[1])?);
                    Ok(TypedMatchPattern::Extractor {
                        input_ty,
                        extractor: extractor_id,
                        extractor_ty,
                        pre_args,
                        success_tag,
                        err_tag,
                        seq_tys,
                        items: typed_items,
                    })
                }
                other => Err(self.pattern_error(
                    TypeDiagnosticReason::PatternShapeMismatch,
                    PatternKind::List,
                    Some("list".into()),
                    Some("List<...> or String".into()),
                    Some(&other),
                    None,
                    None,
                    Vec::new(),
                    &Self::resolved_pattern_span(pat),
                )),
            },
            ResolvedPattern::Extractor(extractor_id, pre_args, items) => {
                let expected_ty = self.resolve_ty(expected_ty);
                let (input_ty, extractor_ty, pre_args, seq_tys, success_tag, err_tag) = self
                    .extractor_contract_for_observed_ty(
                        extractor_id,
                        &expected_ty,
                        pre_args,
                        &extractor_id.span,
                    )?;
                if !self.types_compatible(&input_ty, &expected_ty) {
                    return Err(self
                        .pattern_error(
                            TypeDiagnosticReason::ExtractorInputTypeMismatch,
                            PatternKind::Extractor,
                            Some(extractor_id.name.clone()),
                            Some(self.ty_name(&input_ty)),
                            Some(&expected_ty),
                            None,
                            None,
                            Vec::new(),
                            &extractor_id.span,
                        )
                        .with_hint(format!(
                            "Extractor type signature: {}. Match scrutinee type is {}.",
                            self.callable_signature_for_ty(&extractor_ty)
                                .unwrap_or_else(|| self.ty_name(&extractor_ty)),
                            self.ty_name(&expected_ty)
                        )));
                }
                if items.len() != seq_tys.len() && !(items.is_empty() && seq_tys == [Ty::Unit]) {
                    return Err(self
                        .pattern_error(
                            TypeDiagnosticReason::ExtractorArityMismatch,
                            PatternKind::Extractor,
                            Some(extractor_id.name.clone()),
                            None,
                            Some(&expected_ty),
                            Some(seq_tys.len()),
                            Some(items.len()),
                            seq_tys.iter().map(|ty| self.ty_name(ty)).collect(),
                            &extractor_id.span,
                        )
                        .with_hint(format!(
                            "Extractor success value(s): {}.",
                            seq_tys
                                .iter()
                                .map(|ty| self.ty_name(ty))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )));
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for (item, item_ty) in items.iter().zip(seq_tys.iter()) {
                    typed_items.push(self.check_match_subpattern(item, item_ty)?);
                }
                Ok(TypedMatchPattern::Extractor {
                    input_ty,
                    extractor: extractor_id.clone(),
                    extractor_ty,
                    pre_args,
                    success_tag,
                    err_tag,
                    seq_tys,
                    items: typed_items,
                })
            }
        }
    }

    pub(super) fn is_match_catch_all(&self, pat: &TypedMatchPattern) -> bool {
        match pat {
            TypedMatchPattern::Binding(_) | TypedMatchPattern::Wildcard => true,
            TypedMatchPattern::As(inner, _) => self.is_match_catch_all(inner),
            TypedMatchPattern::Or(items) => items.iter().any(|item| self.is_match_catch_all(item)),
            TypedMatchPattern::Tuple(items) => {
                items.iter().all(|item| self.is_match_catch_all(item))
            }
            TypedMatchPattern::Record(items) => {
                items.iter().all(|item| self.is_match_catch_all(item))
            }
            TypedMatchPattern::Extractor {
                input_ty,
                extractor,
                items,
                ..
            } if extractor.name == "Duration::deconstruct"
                && Self::is_duration_ty(&self.resolve_ty(input_ty)) =>
            {
                items.iter().all(|item| self.is_match_catch_all(item))
            }
            TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::Pin { .. }
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::DurationLit(_)
            | TypedMatchPattern::ErrorKind(_)
            | TypedMatchPattern::Constructor { .. }
            | TypedMatchPattern::ListNil
            | TypedMatchPattern::ListCons(_, _)
            | TypedMatchPattern::Extractor { .. } => false,
        }
    }

    fn collect_or_binding_ids(pat: &TypedMatchPattern, out: &mut Vec<ResolvedId>) {
        match pat {
            TypedMatchPattern::Binding(id) => out.push(id.clone()),
            TypedMatchPattern::As(inner, alias) => {
                Self::collect_or_binding_ids(inner, out);
                out.push(alias.clone());
            }
            TypedMatchPattern::Or(items) => {
                if let Some(first) = items.first() {
                    Self::collect_or_binding_ids(first, out);
                }
            }
            TypedMatchPattern::Tuple(items)
            | TypedMatchPattern::Record(items)
            | TypedMatchPattern::Constructor { fields: items, .. }
            | TypedMatchPattern::Extractor { items, .. } => {
                for item in items {
                    Self::collect_or_binding_ids(item, out);
                }
            }
            TypedMatchPattern::ListCons(head, tail) => {
                Self::collect_or_binding_ids(head, out);
                Self::collect_or_binding_ids(tail, out);
            }
            _ => {}
        }
    }
}

impl Checker {
    fn external_pattern_success_value(node: &TypedNode, bindings: &HashSet<u32>) -> bool {
        match &node.node {
            TypedInner::EagerBoundary(inner) => {
                Self::external_pattern_success_value(inner, bindings)
            }
            TypedInner::Var(id) => !bindings.contains(&id.unique_id),
            TypedInner::Capture(..) => true,
            _ => false,
        }
    }

    pub(super) fn deferred_direct_pattern_variable(
        &self,
        arm: &TypedMatchArm,
    ) -> Result<Option<u32>, TypeError> {
        if !arm.direct_expression {
            return Ok(None);
        }
        let mut bindings = HashSet::new();
        Self::typed_match_pattern_bindings(&arm.pattern, &mut bindings)?;
        if Self::external_pattern_success_value(&arm.body, &bindings) {
            if let Ty::Var(variable) = self.resolve_ty(&arm.body.ty) {
                return Ok(Some(variable));
            }
        }
        Ok(None)
    }

    fn ensure_if_let_direct_expression(
        &self,
        node: &TypedNode,
        bindings: &HashSet<u32>,
    ) -> Result<(), TypeError> {
        let ty = self.resolve_ty(&node.ty);
        if Self::external_pattern_success_value(node, bindings)
            && matches!(ty,
            Ty::Func(ref params, _) | Ty::UserFunc { ref params, .. }
            | Ty::BuiltinFunc { ref params, .. } if params.is_empty())
        {
            return Err(TypeError::new("if_let success branch with Pattern bindings requires a DirectExpression, not an external thunk", node.span.clone()));
        }
        Ok(())
    }

    pub(super) fn validate_direct_pattern_branches(
        &self,
        node: &TypedNode,
    ) -> Result<(), TypeError> {
        let error = self.find_typed_node(node, &|checker, node| {
            if let TypedInner::Match(_, arms) = &node.node {
                for arm in arms.iter().filter(|arm| arm.direct_expression) {
                    let mut bindings = HashSet::new();
                    if let Err(error) =
                        Self::typed_match_pattern_bindings(&arm.pattern, &mut bindings)
                    {
                        return Some(error);
                    }
                    if let Err(error) =
                        checker.ensure_if_let_direct_expression(&arm.body, &bindings)
                    {
                        return Some(error);
                    }
                }
            }
            None
        });
        error.map_or(Ok(()), Err)
    }

    pub(super) fn check_if_let(
        &mut self,
        span: &Span,
        scrutinee: &Resolved,
        arms: &[ResolvedMatchArm],
        expected: Option<&Ty>,
        then_only: bool,
    ) -> Result<TypedNode, TypeError> {
        let callable_expected = self.contextual_callable_expected(expected);
        let [success, failure] = arms else {
            return Err(
                self.typecheck_invariant_error("if_let requires a success and failure arm", span)
            );
        };
        if success.guard.is_some() || failure.guard.is_some() {
            return Err(self.typecheck_invariant_error("if_let arms cannot have guards", span));
        }
        let hint = self.infer_match_pattern_ty(&success.pattern);
        let mut typed_value = self.check_node_with_expected(scrutinee, hint.as_ref())?;
        if let Some(hint) = &hint {
            self.assert_type_relation(
                hint,
                &typed_value.ty,
                self.type_fact(SourceRole::Expected, span, hint),
                self.type_fact(SourceRole::Value, &typed_value.span, &typed_value.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "if_let",
                0,
            )?;
            typed_value.ty = self.resolve_ty(&typed_value.ty);
        }
        self.ensure_no_match_result_value(&typed_value.ty, &typed_value.span)?;
        let provenance = self.constructor_capability_for_node(&typed_value);
        self.env.push_var_scope();
        let saved_provenance = self.constructor_capabilities.clone();
        let checked = (|| {
            let pattern = self.check_match_subpattern(&success.pattern, &typed_value.ty)?;
            self.bind_match_constructor_provenance(&pattern, (provenance, typed_value.ty.clone()));
            let mut bindings = HashSet::new();
            Self::typed_match_pattern_bindings(&pattern, &mut bindings)?;
            let direct = !bindings.is_empty();
            let input = self.check_lazy_input(&success.body, expected, span)?;
            if direct {
                self.ensure_if_let_direct_expression(&input.node, &bindings)?;
            }
            Ok((pattern, input, direct, bindings))
        })();
        self.env.pop_var_scope();
        self.constructor_capabilities = saved_provenance;
        let (pattern, success_input, direct, bindings) = checked?;
        let (then, otherwise) = if then_only {
            let then = if direct {
                success_input.node
            } else {
                self.normalize_lazy_single(success_input, &Ty::Unit, span)?
            };
            (then, self.check_node(&failure.body)?)
        } else if direct {
            let then = success_input.node;
            let otherwise = self.check_lazy_argument_with_expected(
                &failure.body,
                callable_expected.as_ref().unwrap_or(&then.ty),
                span,
            )?;
            (then, otherwise)
        } else {
            let failure_input = self.check_lazy_input(&failure.body, expected, span)?;
            self.normalize_lazy_pair(success_input, failure_input, expected, span)?
        };
        if let Some(expected) = &callable_expected {
            for (ordinal, branch) in [&then, &otherwise].into_iter().enumerate() {
                self.assert_value_type_relation(
                    expected,
                    &branch.ty,
                    self.type_fact(SourceRole::Expected, span, expected),
                    self.branch_fact(SourceRole::Branch, branch, ordinal),
                    TypeDiagnosticReason::IfBranchTypeMismatch,
                    DiagnosticOrigin::Branch {
                        form: diagnostics::BranchForm::IfLet,
                        ordinal: ordinal as u32,
                    },
                    "if_let",
                    ordinal as u32,
                )
                .map_err(|error| {
                    self.complete_branch_error(error, &[&then, &otherwise], &[None, None])
                })?;
            }
        } else {
            self.assert_type_relation(
                &then.ty,
                &otherwise.ty,
                self.branch_fact(SourceRole::Branch, &then, 0),
                self.branch_fact(SourceRole::Branch, &otherwise, 1),
                TypeDiagnosticReason::IfBranchTypeMismatch,
                DiagnosticOrigin::Branch {
                    form: diagnostics::BranchForm::IfLet,
                    ordinal: 1,
                },
                "if_let",
                1,
            )
            .map_err(|error| {
                self.complete_branch_error(error, &[&then, &otherwise], &[None, None])
            })?;
        }
        let ty = self.resolve_ty(callable_expected.as_ref().unwrap_or(&then.ty));
        self.record_lazy_capture_signature(span, &ty);
        let required = if then_only { Some(&Ty::Unit) } else { expected };
        if let Some(required) = required {
            self.assert_value_type_relation(
                required,
                &then.ty,
                self.type_fact(SourceRole::Expected, span, required),
                self.branch_fact(SourceRole::Branch, &then, 0),
                TypeDiagnosticReason::IfBranchTypeMismatch,
                DiagnosticOrigin::Branch {
                    form: diagnostics::BranchForm::IfLet,
                    ordinal: 0,
                },
                "if_let",
                0,
            )?;
        }
        if direct {
            self.ensure_if_let_direct_expression(&then, &bindings)?;
        }
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Match(
                Box::new(typed_value),
                vec![
                    TypedMatchArm {
                        direct_expression: direct,
                        pattern,
                        guard: None,
                        body: then,
                    },
                    TypedMatchArm {
                        direct_expression: false,
                        pattern: TypedMatchPattern::Wildcard,
                        guard: None,
                        body: otherwise,
                    },
                ],
            ),
        })
    }
}
