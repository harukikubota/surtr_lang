use super::*;

impl Checker {
    pub(super) fn check_match(
        &mut self,
        span: &Span,
        scrutinee: &Resolved,
        arms: &[ResolvedMatchArm],
    ) -> Result<TypedNode, TypeError> {
        let typed_scrut = self.check_node(scrutinee)?;
        let mut typed_arms = Vec::new();
        let mut result_ty: Option<Ty> = None;

        for arm in arms {
            let mut typed_arm = self.check_match_arm(arm, &typed_scrut.ty, span)?;
            if let Some(ref rt) = result_ty {
                if !self.types_compatible(rt, &typed_arm.body.ty)
                    && self.can_coerce_err_only_result_self_arm(
                        &typed_scrut,
                        &typed_arms,
                        &typed_arm,
                        rt,
                    )
                {
                    typed_arm.body.ty = self.resolve_ty(rt);
                }
            }
            let body_node = &typed_arm.body;
            if let Some(ref rt) = result_ty {
                if !self.types_compatible(rt, &body_node.ty) {
                    return Err(TypeError {
                        message: format!(
                            "Match arm type mismatch: expected {}, got {}",
                            self.ty_name(rt),
                            self.ty_name(&body_node.ty)
                        ),
                        span: body_node.span.clone(),
                        hint: None,
                    });
                }
            } else {
                result_ty = Some(body_node.ty.clone());
            }
            typed_arms.push(typed_arm);
        }

        // Arm-local bindings are rolled back by each arm scope. Keep typed arm
        // subtrees unresolved here and let check_program do one final pass.
        self.check_match_exhaustive(span, &typed_scrut.ty, &typed_arms)?;

        let ty = result_ty.unwrap_or(Ty::Unit);
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Match(Box::new(typed_scrut), typed_arms),
        })
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
                && matches!(
                    prev_arm.pattern,
                    TypedMatchPattern::Constructor { tag: 0, .. }
                )
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
            Ty::Bool => self.check_enum_like_match_exhaustive(span, "Boolean", arms),
            Ty::Result(_, _) => {
                let has_ok = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && matches!(&arm.pattern, TypedMatchPattern::Constructor { tag: 0, .. })
                });
                let has_err = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && matches!(&arm.pattern, TypedMatchPattern::Constructor { tag: 1, .. })
                });

                if has_ok && has_err {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_ok {
                        missing.push("Ok");
                    }
                    if !has_err {
                        missing.push("Err");
                    }
                    Err(TypeError {
                        message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                        span: span.clone(),
                        hint: None,
                    })
                }
            }
            Ty::Enum(enum_name, _) => self.check_enum_like_match_exhaustive(span, enum_name, arms),
            Ty::List(_) => {
                let has_nil = arms.iter().any(|arm| {
                    arm.guard.is_none() && matches!(&arm.pattern, TypedMatchPattern::ListNil)
                });
                let has_cons = arms.iter().any(|arm| {
                    arm.guard.is_none() && matches!(&arm.pattern, TypedMatchPattern::ListCons(_, _))
                });
                if has_nil && has_cons {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_nil {
                        missing.push("[]");
                    }
                    if !has_cons {
                        missing.push("[head, ..tail]");
                    }
                    Err(TypeError {
                        message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                        span: span.clone(),
                        hint: None,
                    })
                }
            }
            Ty::Str => {
                let has_empty = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && matches!(&arm.pattern, TypedMatchPattern::StrLit(value) if value.is_empty())
                });
                let has_cons = arms.iter().any(|arm| {
                    arm.guard.is_none()
                        && matches!(
                            &arm.pattern,
                            TypedMatchPattern::Extractor {
                                input_ty,
                                extractor,
                                ..
                            } if extractor.name == "uncons"
                                && matches!(self.resolve_ty(input_ty), Ty::Str)
                        )
                });
                if has_empty && has_cons {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_empty {
                        missing.push("[]");
                    }
                    if !has_cons {
                        missing.push("[head, ..tail]");
                    }
                    Err(TypeError {
                        message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                        span: span.clone(),
                        hint: None,
                    })
                }
            }
            _ => Err(TypeError {
                message: "Non-exhaustive match. Missing: _".into(),
                span: span.clone(),
                hint: None,
            }),
        };
        self.profiler.finish(ProfileEvent::MatchExhaustive, profile);
        result
    }

    fn check_enum_like_match_exhaustive(
        &self,
        span: &Span,
        enum_name: &str,
        arms: &[TypedMatchArm],
    ) -> Result<(), TypeError> {
        let variants = self
            .lookup_enum_variants_of(enum_name)
            .ok_or_else(|| TypeError {
                message: format!("Unknown enum metadata for match: {}", enum_name),
                span: span.clone(),
                hint: None,
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
            Err(TypeError {
                message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                span: span.clone(),
                hint: None,
            })
        }
    }

    fn match_arm_covers_enum_variant(
        enum_name: &str,
        pattern: &TypedMatchPattern,
        variant: &crate::env::EnumVariantInfo,
    ) -> bool {
        match pattern {
            TypedMatchPattern::Constructor { tag, .. } => *tag == variant.tag,
            TypedMatchPattern::BoolLit(value) if Self::surface_name(enum_name) == "Boolean" => {
                variant.short_name == if *value { "True" } else { "False" }
            }
            _ => false,
        }
    }

    pub(super) fn check_match_arm(
        &mut self,
        arm: &ResolvedMatchArm,
        scrut_ty: &Ty,
        _span: &Span,
    ) -> Result<TypedMatchArm, TypeError> {
        let profile = self.profiler.start();
        self.env.push_var_scope();
        let result = (|| {
            let typed_pat = self.check_match_subpattern(&arm.pattern, scrut_ty)?;
            let typed_guard = if let Some(guard) = &arm.guard {
                let typed_guard = self.check_node(guard)?;
                if !self.types_compatible(&Ty::Bool, &typed_guard.ty) {
                    return Err(TypeError {
                        message: format!(
                            "match guard must be Boolean, got {}",
                            self.ty_name(&typed_guard.ty)
                        ),
                        span: typed_guard.span.clone(),
                        hint: None,
                    });
                }
                Some(typed_guard)
            } else {
                None
            };
            let typed_body = self.check_node(&arm.body)?;
            // Do not normalize env bindings or typed guard/body subtrees in this
            // scoped arm. The env frame is discarded below, and the containing
            // TypedInner::Match is normalized once at the program boundary.
            Ok(TypedMatchArm {
                pattern: typed_pat,
                guard: typed_guard,
                body: typed_body,
            })
        })();
        self.env.pop_var_scope();
        self.profiler.finish(ProfileEvent::MatchArm, profile);
        result
    }

    pub(super) fn check_match_subpattern(
        &mut self,
        pat: &ResolvedPattern,
        expected_ty: &Ty,
    ) -> Result<TypedMatchPattern, TypeError> {
        match pat {
            ResolvedPattern::Var(id) => {
                self.env
                    .bind_var(id.unique_id, self.resolve_ty(expected_ty));
                Ok(TypedMatchPattern::Binding(id.clone()))
            }
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected =
                    self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                if !self.types_compatible(&expected, expected_ty) {
                    return Err(TypeError {
                        message: format!(
                            "expected {}, got {}",
                            self.ty_name(&expected),
                            self.ty_name(expected_ty)
                        ),
                        span: id.span.clone(),
                        hint: None,
                    });
                }
                let bind_ty = self.resolve_ty(&expected);
                self.env.bind_var(id.unique_id, bind_ty);
                Ok(TypedMatchPattern::Binding(id.clone()))
            }
            ResolvedPattern::Pin(id) => {
                let pinned_ty =
                    self.env
                        .lookup_var(id.unique_id)
                        .cloned()
                        .ok_or_else(|| TypeError {
                            message: format!(
                                "Pinned pattern requires an existing value `{}`",
                                id.name
                            ),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                let expected_ty = self.resolve_ty(expected_ty);
                let pinned_ty = self.resolve_ty(&pinned_ty);
                if !self.types_compatible(&pinned_ty, &expected_ty) {
                    return Err(TypeError {
                        message: format!(
                            "Pinned pattern type mismatch: expected {}, got {}",
                            self.ty_name(&pinned_ty),
                            self.ty_name(&expected_ty)
                        ),
                        span: id.span.clone(),
                        hint: None,
                    });
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
                        return Err(TypeError {
                            message: format!(
                                "expected {}, got {}",
                                self.ty_name(&expected),
                                self.ty_name(expected_ty)
                            ),
                            span: alias.span.clone(),
                            hint: None,
                        });
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
                let Ty::Tuple(item_tys) = &expected_ty else {
                    return Err(TypeError {
                        message: format!(
                            "tuple pattern requires tuple scrutinee, got {}",
                            self.ty_name(&expected_ty)
                        ),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    });
                };
                if items.len() != item_tys.len() {
                    return Err(TypeError {
                        message: format!(
                            "tuple pattern expects {} value(s), got {}",
                            item_tys.len(),
                            items.len()
                        ),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    });
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for (item, item_ty) in items.iter().zip(item_tys.iter()) {
                    typed_items.push(self.check_match_subpattern(item, item_ty)?);
                }
                Ok(TypedMatchPattern::Tuple(typed_items))
            }
            ResolvedPattern::BoolLit(span, b) => {
                if !self.types_compatible(&Ty::Bool, expected_ty) {
                    return Err(TypeError {
                        message: "Boolean pattern on non-Boolean scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::BoolLit(*b))
            }
            ResolvedPattern::IntLit(span, n) => {
                if !self.types_compatible(&Ty::Int, expected_ty) {
                    return Err(TypeError {
                        message: "Int pattern on non-Int scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::IntLit(n.clone()))
            }
            ResolvedPattern::StrLit(span, s) => {
                if !self.types_compatible(&Ty::Str, expected_ty) {
                    return Err(TypeError {
                        message: "String pattern on non-String scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::StrLit(s.clone()))
            }
            ResolvedPattern::DurationLit(span, n) => {
                let expected_ty = self.resolve_ty(expected_ty);
                if !Self::is_duration_ty(&expected_ty) {
                    return Err(TypeError {
                        message: format!(
                            "duration literal pattern requires Duration, got {}",
                            self.ty_name(&expected_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::DurationLit(n.clone()))
            }
            ResolvedPattern::Or(items) => {
                if items.is_empty() {
                    return Err(TypeError {
                        message: "empty pattern alternative".into(),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    });
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for item in items {
                    let typed_item = self.check_match_subpattern(item, expected_ty)?;
                    if self.match_pattern_has_bindings(&typed_item) {
                        return Err(TypeError {
                            message: "Pattern alternatives cannot bind names directly.".into(),
                            span: Span { start: 0, end: 0 },
                            hint: Some(
                                "Use an outer as-pattern such as `A | B @ err: Error`.".into(),
                            ),
                        });
                    }
                    typed_items.push(typed_item);
                }
                Ok(TypedMatchPattern::Or(typed_items))
            }
            ResolvedPattern::Constructor(ctor_id, inner_pats) => {
                if matches!(self.resolve_ty(expected_ty), Ty::Error)
                    && matches!(ctor_id.name.as_str(), "Err" | "Result::Err")
                {
                    return Err(TypeError {
                        message: "Nested Result errors are not allowed in match patterns: use Err(error) for the outer failure, or Ok(Err(error)) for an inner failure.".into(),
                        span: ctor_id.span.clone(),
                        hint: Some(
                            "Err matches the Result layer being inspected; do not write Err(Err(...)).".into(),
                        ),
                    });
                }
                if matches!(expected_ty, Ty::Error)
                    && self.env.is_error_constructor(ctor_id.unique_id)
                {
                    if !inner_pats.is_empty() {
                        return Err(TypeError {
                            message: "Error kind patterns do not destructure payloads yet.".into(),
                            span: ctor_id.span.clone(),
                            hint: Some(
                                "Use `Kind @ err: Error` and inspect the Error value.".into(),
                            ),
                        });
                    }
                    return Ok(TypedMatchPattern::ErrorKind(ctor_id.name.clone()));
                }
                if matches!(expected_ty, Ty::Bool) {
                    let variant = self
                        .lookup_enum_variant_by_constructor_id(ctor_id.unique_id)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown constructor: {}", ctor_id.name),
                            span: ctor_id.span.clone(),
                            hint: None,
                        })?
                        .clone();
                    let variant = self.instantiate_enum_variant(&variant);
                    if Self::surface_name(&variant.enum_name) != "Boolean" {
                        return Err(TypeError {
                            message: format!(
                                "Constructor {} does not belong to enum Boolean",
                                ctor_id.name
                            ),
                            span: ctor_id.span.clone(),
                            hint: None,
                        });
                    }
                    if !inner_pats.is_empty() {
                        return Err(TypeError {
                            message: format!(
                                "{} pattern expects 0 argument(s), got {}",
                                ctor_id.name,
                                inner_pats.len()
                            ),
                            span: ctor_id.span.clone(),
                            hint: None,
                        });
                    }
                    return match variant.short_name.as_str() {
                        "True" => Ok(TypedMatchPattern::BoolLit(true)),
                        "False" => Ok(TypedMatchPattern::BoolLit(false)),
                        _ => Err(TypeError {
                            message: format!("Unknown Boolean constructor: {}", ctor_id.name),
                            span: ctor_id.span.clone(),
                            hint: None,
                        }),
                    };
                }
                if let Ty::Result(ok_ty, err_ty) = expected_ty {
                    let tag = match ctor_id.name.as_str() {
                        "Ok" => 0u32,
                        "Err" => 1u32,
                        _ => {
                            return Err(TypeError {
                                message: format!("Unknown constructor: {}", ctor_id.name),
                                span: ctor_id.span.clone(),
                                hint: None,
                            });
                        }
                    };
                    if inner_pats.len() != 1 {
                        return Err(TypeError {
                            message: format!(
                                "{}(...) match pattern requires exactly one argument",
                                ctor_id.name
                            ),
                            span: ctor_id.span.clone(),
                            hint: None,
                        });
                    }
                    let inner_ty = match tag {
                        0 => ok_ty.as_ref().clone(),
                        1 => err_ty.as_ref().clone(),
                        _ => {
                            return Err(TypeError {
                                message: format!("Unknown constructor: {}", ctor_id.name),
                                span: ctor_id.span.clone(),
                                hint: None,
                            });
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
                    return Err(TypeError {
                        message: "Constructor pattern on non-enum/non-Result scrutinee".into(),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                };
                let variant = self
                    .lookup_enum_variant_by_constructor_id(ctor_id.unique_id)
                    .ok_or_else(|| TypeError {
                        message: format!("Unknown constructor: {}", ctor_id.name),
                        span: ctor_id.span.clone(),
                        hint: None,
                    })?
                    .clone();
                let variant = self.instantiate_enum_variant(&variant);
                if &variant.enum_name != expected_enum_name {
                    return Err(TypeError {
                        message: format!(
                            "Constructor {} does not belong to enum {}",
                            ctor_id.name, expected_enum_name
                        ),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }
                if !self.types_compatible(&variant.enum_ty, expected_ty) {
                    return Err(TypeError {
                        message: format!(
                            "Constructor {} does not match expected type {}",
                            ctor_id.name,
                            self.ty_name(expected_ty)
                        ),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }
                if inner_pats.len() != variant.payload.len() {
                    return Err(TypeError {
                        message: format!(
                            "{} pattern expects {} argument(s), got {}",
                            ctor_id.name,
                            variant.payload.len(),
                            inner_pats.len()
                        ),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
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
                _ => Err(TypeError {
                    message: "empty list pattern on non-List/String scrutinee".into(),
                    span: span.clone(),
                    hint: None,
                }),
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
                    let extractor_id = self.kernel_uncons_id(&Span { start: 0, end: 0 })?;
                    let (input_ty, extractor_ty, seq_tys, success_tag, no_match_tag, err_tag) =
                        self.extractor_contract_for_observed_ty(
                            &extractor_id,
                            &Ty::Str,
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
                        success_tag,
                        no_match_tag,
                        err_tag,
                        seq_tys,
                        items: typed_items,
                    })
                }
                other => Err(TypeError {
                    message: format!(
                        "list pattern requires List<...> or String, got {}",
                        self.ty_name(&other)
                    ),
                    span: Span { start: 0, end: 0 },
                    hint: None,
                }),
            },
            ResolvedPattern::Extractor(extractor_id, items) => {
                let expected_ty = self.resolve_ty(expected_ty);
                let (input_ty, extractor_ty, seq_tys, success_tag, no_match_tag, err_tag) = self
                    .extractor_contract_for_observed_ty(
                        extractor_id,
                        &expected_ty,
                        &extractor_id.span,
                    )?;
                if !self.types_compatible(&input_ty, &expected_ty) {
                    return Err(TypeError {
                        message: format!(
                            "Extractor {} expects {}, got {}",
                            extractor_id.name,
                            self.ty_name(&input_ty),
                            self.ty_name(&expected_ty)
                        ),
                        span: extractor_id.span.clone(),
                        hint: Some(format!(
                            "Extractor type signature: {}. Match scrutinee type is {}.",
                            self.callable_signature_for_ty(&extractor_ty)
                                .unwrap_or_else(|| self.ty_name(&extractor_ty)),
                            self.ty_name(&expected_ty)
                        )),
                    });
                }
                if items.len() != seq_tys.len() {
                    return Err(TypeError {
                        message: format!(
                            "Extractor {} returns {} value(s), but pattern expects {}",
                            extractor_id.name,
                            seq_tys.len(),
                            items.len()
                        ),
                        span: extractor_id.span.clone(),
                        hint: Some(format!(
                            "Extractor success value(s): {}.",
                            seq_tys
                                .iter()
                                .map(|ty| self.ty_name(ty))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    });
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for (item, item_ty) in items.iter().zip(seq_tys.iter()) {
                    typed_items.push(self.check_match_subpattern(item, item_ty)?);
                }
                Ok(TypedMatchPattern::Extractor {
                    input_ty: expected_ty,
                    extractor: extractor_id.clone(),
                    extractor_ty,
                    success_tag,
                    no_match_tag,
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

    fn match_pattern_has_bindings(&self, pat: &TypedMatchPattern) -> bool {
        match pat {
            TypedMatchPattern::Binding(_) => true,
            TypedMatchPattern::As(_, _) => true,
            TypedMatchPattern::Tuple(items) | TypedMatchPattern::Or(items) => items
                .iter()
                .any(|item| self.match_pattern_has_bindings(item)),
            TypedMatchPattern::Constructor { fields, .. } => fields
                .iter()
                .any(|item| self.match_pattern_has_bindings(item)),
            TypedMatchPattern::ListCons(head, tail) => {
                self.match_pattern_has_bindings(head) || self.match_pattern_has_bindings(tail)
            }
            TypedMatchPattern::Extractor { items, .. } => items
                .iter()
                .any(|item| self.match_pattern_has_bindings(item)),
            TypedMatchPattern::Wildcard
            | TypedMatchPattern::Pin { .. }
            | TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::DurationLit(_)
            | TypedMatchPattern::ErrorKind(_)
            | TypedMatchPattern::ListNil => false,
        }
    }
}
