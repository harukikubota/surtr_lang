use super::*;

impl Checker {
    pub(super) fn check_match(
        &mut self,
        span: &Span,
        scrutinee: &Resolved,
        arms: &[(ResolvedPattern, Resolved)],
    ) -> Result<TypedNode, TypeError> {
        let typed_scrut = self.check_node(scrutinee)?;
        let mut typed_arms = Vec::new();
        let mut result_ty: Option<Ty> = None;

        for (pat, body) in arms {
            let (typed_pat, body_node) = self.check_match_arm(pat, body, &typed_scrut.ty, span)?;
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
            typed_arms.push((typed_pat, body_node));
            self.normalize_env_bindings();
        }

        self.check_match_exhaustive(span, &typed_scrut.ty, &typed_arms)?;

        let ty = result_ty.unwrap_or(Ty::Unit);
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Match(Box::new(typed_scrut), typed_arms),
        })
    }

    pub(super) fn check_match_exhaustive(
        &self,
        span: &Span,
        scrut_ty: &Ty,
        arms: &[(TypedMatchPattern, TypedNode)],
    ) -> Result<(), TypeError> {
        if arms.iter().any(|(pat, _)| self.is_match_catch_all(pat)) {
            return Ok(());
        }

        match scrut_ty {
            Ty::Bool => {
                let has_true = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::BoolLit(true)));
                let has_false = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::BoolLit(false)));

                if has_true && has_false {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_true {
                        missing.push("True");
                    }
                    if !has_false {
                        missing.push("False");
                    }
                    Err(TypeError {
                        message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                        span: span.clone(),
                        hint: None,
                    })
                }
            }
            Ty::Result(_, _) => {
                let has_ok = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::Constructor { tag: 0, .. }));
                let has_err = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::Constructor { tag: 1, .. }));

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
            Ty::Enum(enum_name, _) => {
                let variants = self
                    .env
                    .enum_variants_of(enum_name)
                    .cloned()
                    .unwrap_or_default();
                let mut missing = Vec::new();
                for variant in variants {
                    let covered = arms.iter().any(|(pat, _)| {
                        matches!(
                            pat,
                            TypedMatchPattern::Constructor { tag, .. } if *tag == variant.tag
                        )
                    });
                    if !covered {
                        missing.push(variant.short_name);
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
            Ty::List(_) => {
                let has_nil = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::ListNil));
                let has_cons = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::ListCons(_, _)));
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
                let has_empty = arms.iter().any(
                    |(pat, _)| matches!(pat, TypedMatchPattern::StrLit(value) if value.is_empty()),
                );
                let has_cons = arms.iter().any(|(pat, _)| {
                    matches!(
                        pat,
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
        }
    }

    pub(super) fn check_match_arm(
        &mut self,
        pat: &ResolvedPattern,
        body: &Resolved,
        scrut_ty: &Ty,
        _span: &Span,
    ) -> Result<(TypedMatchPattern, TypedNode), TypeError> {
        let mut arm_checker = self.spawn_child_checker(self.env.clone());
        let typed_pat = arm_checker.check_match_subpattern(pat, scrut_ty)?;
        let typed_body = arm_checker.check_node(body)?;
        arm_checker.normalize_env_bindings();
        let typed_body = arm_checker.resolve_typed_node(typed_body);
        self.absorb_child_progress(&arm_checker);
        Ok((typed_pat, typed_body))
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
                    self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
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
            ResolvedPattern::As(inner, alias, alias_ty) => {
                let typed_inner = self.check_match_subpattern(inner, expected_ty)?;
                let alias_bind_ty = if let Some(ast_ty) = alias_ty {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
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
            ResolvedPattern::Constructor(ctor_id, inner_pats) => {
                if matches!(expected_ty, Ty::Result(_, _)) {
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
                    let inner_ty = match (tag, expected_ty) {
                        (0, Ty::Result(ok, _)) => ok.as_ref().clone(),
                        (1, Ty::Result(_, err)) => err.as_ref().clone(),
                        _ => unreachable!(),
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
                    .env
                    .enum_variant_by_constructor_id(ctor_id.unique_id)
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
                        hint: None,
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
                        hint: None,
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
            TypedMatchPattern::Tuple(items) => {
                items.iter().all(|item| self.is_match_catch_all(item))
            }
            TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::Constructor { .. }
            | TypedMatchPattern::ListNil
            | TypedMatchPattern::ListCons(_, _)
            | TypedMatchPattern::Extractor { .. } => false,
        }
    }
}
