use super::*;

impl Checker {
    pub(super) fn is_total_bind_pattern(pat: &ResolvedPattern) -> bool {
        match pat {
            ResolvedPattern::Var(_)
            | ResolvedPattern::Annotated(_, _)
            | ResolvedPattern::Wildcard(_) => true,
            ResolvedPattern::As(inner, _, _) => Self::is_total_bind_pattern(inner),
            ResolvedPattern::Tuple(items) => items.iter().all(Self::is_total_bind_pattern),
            ResolvedPattern::Or(_) => false,
            ResolvedPattern::ListNil(_)
            | ResolvedPattern::ListCons(_, _)
            | ResolvedPattern::IntLit(_, _)
            | ResolvedPattern::StrLit(_, _)
            | ResolvedPattern::BoolLit(_, _)
            | ResolvedPattern::DurationLit(_, _)
            | ResolvedPattern::Constructor(_, _)
            | ResolvedPattern::Extractor(_, _) => false,
        }
    }

    pub(super) fn check_pattern(
        &mut self,
        pat: &ResolvedPattern,
        rhs_ty: &Ty,
        span: &Span,
    ) -> Result<(TypedPattern, Ty), TypeError> {
        match pat {
            ResolvedPattern::Var(id) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Var(rhs_ty.clone(), id.clone()), rhs_ty))
            }
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected =
                    self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                if !self.types_compatible(&expected, rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "expected {}, got {}",
                            self.ty_name(&expected),
                            self.ty_name(rhs_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let expected = self.resolve_ty(&expected);
                Ok((TypedPattern::Var(expected.clone(), id.clone()), expected))
            }
            ResolvedPattern::As(inner, alias, alias_ty) => {
                let (typed_inner, inner_ty) = self.check_pattern(inner, rhs_ty, span)?;
                let alias_bind_ty = if let Some(ast_ty) = alias_ty {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                    if !self.types_compatible(&expected, &inner_ty) {
                        return Err(TypeError {
                            message: format!(
                                "expected {}, got {}",
                                self.ty_name(&expected),
                                self.ty_name(&inner_ty)
                            ),
                            span: alias.span.clone(),
                            hint: None,
                        });
                    }
                    self.resolve_ty(&expected)
                } else {
                    self.resolve_ty(&inner_ty)
                };

                Ok((
                    TypedPattern::As(alias_bind_ty, Box::new(typed_inner), alias.clone()),
                    inner_ty,
                ))
            }
            ResolvedPattern::Wildcard(_) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Wildcard(rhs_ty.clone()), rhs_ty))
            }
            ResolvedPattern::Tuple(items) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                let Ty::Tuple(item_tys) = &rhs_ty else {
                    return Err(TypeError {
                        message: format!(
                            "tuple pattern requires tuple scrutinee, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: span.clone(),
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
                        span: span.clone(),
                        hint: None,
                    });
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for (item, item_ty) in items.iter().zip(item_tys.iter()) {
                    let (typed_item, _) = self.check_pattern(item, item_ty, span)?;
                    typed_items.push(typed_item);
                }
                Ok((TypedPattern::Tuple(rhs_ty.clone(), typed_items), rhs_ty))
            }
            ResolvedPattern::Or(_) => Err(TypeError {
                message: "Pattern alternatives are only supported in match expressions.".into(),
                span: span.clone(),
                hint: None,
            }),
            ResolvedPattern::ListNil(pspan) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                match rhs_ty {
                    Ty::List(_) => Ok((TypedPattern::ListNil(rhs_ty.clone()), rhs_ty)),
                    Ty::Str => Ok((TypedPattern::StrLit(Ty::Str, String::new()), rhs_ty)),
                    other => Err(TypeError {
                        message: format!(
                            "empty list pattern requires List<...> or String, got {}",
                            self.ty_name(&other)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    }),
                }
            }
            ResolvedPattern::ListCons(head, tail) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                match &rhs_ty {
                    Ty::List(inner) => {
                        let elem_ty = inner.as_ref().clone();
                        let (typed_head, _) = self.check_pattern(head, &elem_ty, span)?;
                        let tail_ty = Ty::List(Box::new(elem_ty.clone()));
                        let (typed_tail, _) = self.check_pattern(tail, &tail_ty, span)?;
                        Ok((
                            TypedPattern::ListCons(
                                rhs_ty.clone(),
                                Box::new(typed_head),
                                Box::new(typed_tail),
                            ),
                            rhs_ty,
                        ))
                    }
                    Ty::Str => {
                        let extractor_id = self.kernel_uncons_id(span)?;
                        let (input_ty, extractor_ty, seq_tys, success_tag, no_match_tag, err_tag) =
                            self.extractor_contract_for_observed_ty(&extractor_id, &rhs_ty, span)?;
                        debug_assert_eq!(seq_tys.len(), 2);
                        let (typed_head, _) = self.check_pattern(head, &seq_tys[0], span)?;
                        let (typed_tail, _) = self.check_pattern(tail, &seq_tys[1], span)?;
                        Ok((
                            TypedPattern::Extractor {
                                input_ty,
                                extractor: extractor_id,
                                extractor_ty,
                                success_tag,
                                no_match_tag,
                                err_tag,
                                seq_tys,
                                items: vec![typed_head, typed_tail],
                            },
                            rhs_ty,
                        ))
                    }
                    other => Err(TypeError {
                        message: format!(
                            "list pattern requires List<...> or String, got {}",
                            self.ty_name(other)
                        ),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
            ResolvedPattern::IntLit(pspan, n) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Int, &rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "integer literal pattern requires Int, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::IntLit(Ty::Int, n.clone()), rhs_ty))
            }
            ResolvedPattern::StrLit(pspan, s) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Str, &rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "string literal pattern requires String, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::StrLit(Ty::Str, s.clone()), rhs_ty))
            }
            ResolvedPattern::BoolLit(pspan, b) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Bool, &rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "boolean literal pattern requires Boolean, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::BoolLit(Ty::Bool, *b), rhs_ty))
            }
            ResolvedPattern::DurationLit(pspan, n) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !Self::is_duration_ty(&rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "duration literal pattern requires Duration, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::DurationLit(rhs_ty.clone(), n.clone()), rhs_ty))
            }
            ResolvedPattern::Constructor(ctor_id, inners) => {
                if ctor_id.name != "Ok" {
                    return Err(TypeError {
                        message: format!(
                            "SafeBind constructor pattern only supports Ok(...), got {}(...)",
                            ctor_id.name
                        ),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }

                let rhs_ty = self.resolve_ty(rhs_ty);
                let ok_ty = match &rhs_ty {
                    Ty::Result(ok, _) => ok.as_ref().clone(),
                    other => {
                        return Err(TypeError {
                            message: format!(
                                "`Ok(...)` pattern requires Result<...>, got {}",
                                self.ty_name(other)
                            ),
                            span: ctor_id.span.clone(),
                            hint: Some(
                                "Use `num =? expr` directly for Result<T>, and only add `Ok(...)` on the left for nested Result values.".into(),
                            ),
                        });
                    }
                };

                if inners.len() != 1 {
                    return Err(TypeError {
                        message: "SafeBind Ok(...) pattern requires exactly one inner pattern"
                            .into(),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }

                let (typed_inner, _) = self.check_pattern(&inners[0], &ok_ty, span)?;
                Ok((
                    TypedPattern::ResultOk(rhs_ty.clone(), Box::new(typed_inner)),
                    rhs_ty,
                ))
            }
            ResolvedPattern::Extractor(extractor_id, items) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                let (input_ty, extractor_ty, seq_tys, success_tag, no_match_tag, err_tag) = self
                    .extractor_contract_for_observed_ty(
                        extractor_id,
                        &rhs_ty,
                        &extractor_id.span,
                    )?;
                if !self.types_compatible(&input_ty, &rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "Extractor {} expects {}, got {}",
                            extractor_id.name,
                            self.ty_name(&input_ty),
                            self.ty_name(&rhs_ty)
                        ),
                        span: extractor_id.span.clone(),
                        hint: Some(format!(
                            "Extractor type signature: {}. RHS type is {}.",
                            self.callable_signature_for_ty(&extractor_ty)
                                .unwrap_or_else(|| self.ty_name(&extractor_ty)),
                            self.ty_name(&rhs_ty)
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
                    let (typed_item, _) = self.check_pattern(item, item_ty, span)?;
                    typed_items.push(typed_item);
                }
                Ok((
                    TypedPattern::Extractor {
                        input_ty: rhs_ty.clone(),
                        extractor: extractor_id.clone(),
                        extractor_ty,
                        success_tag,
                        no_match_tag,
                        err_tag,
                        seq_tys,
                        items: typed_items,
                    },
                    rhs_ty,
                ))
            }
        }
    }

    pub(super) fn bind_typed_pattern(&mut self, pat: &TypedPattern, rhs_ty: &Ty) {
        let rhs_ty = self.resolve_ty(rhs_ty);
        match pat {
            TypedPattern::Var(_, id) => {
                self.env.bind_var(id.unique_id, rhs_ty.clone());
            }
            TypedPattern::As(alias_ty, inner, id) => {
                self.env.bind_var(id.unique_id, self.resolve_ty(alias_ty));
                self.bind_typed_pattern(inner, &rhs_ty);
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
            TypedPattern::Tuple(_, items) => {
                let item_tys = match &rhs_ty {
                    Ty::Tuple(item_tys) => item_tys.clone(),
                    _ => return,
                };
                for (item, item_ty) in items.iter().zip(item_tys.iter()) {
                    self.bind_typed_pattern(item, item_ty);
                }
            }
            TypedPattern::ListCons(_, head, tail) => {
                let elem_ty = match &rhs_ty {
                    Ty::List(inner) => inner.as_ref().clone(),
                    _ => return,
                };
                self.bind_typed_pattern(head, &elem_ty);
                let tail_ty = Ty::List(Box::new(elem_ty));
                self.bind_typed_pattern(tail, &tail_ty);
            }
            TypedPattern::ResultOk(_, inner) => {
                let ok_ty = match &rhs_ty {
                    Ty::Result(ok, _) => ok.as_ref().clone(),
                    _ => return,
                };
                self.bind_typed_pattern(inner, &ok_ty);
            }
            TypedPattern::Extractor { seq_tys, items, .. } => {
                for (item, item_ty) in items.iter().zip(seq_tys.iter()) {
                    self.bind_typed_pattern(item, item_ty);
                }
            }
        }
    }

    pub(super) fn normalize_env_bindings(&mut self) {
        let profile = self.profiler.start();
        let keys = self.env.vars.keys().copied().collect::<Vec<_>>();
        for key in keys {
            if let Some(ty) = self.env.vars.get(&key).cloned() {
                self.env.vars.insert(key, self.resolve_ty(&ty));
            }
        }
        self.profiler
            .finish(ProfileEvent::NormalizeEnvBindings, profile);
    }

    pub(super) fn collect_pattern_result_error_types(&self, pat: &TypedPattern, out: &mut Vec<Ty>) {
        match pat {
            TypedPattern::ResultOk(ty, inner) => {
                if let Ty::Result(_, err) = self.resolve_ty(ty) {
                    out.push(err.as_ref().clone());
                }
                self.collect_pattern_result_error_types(inner, out);
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.collect_pattern_result_error_types(head, out);
                self.collect_pattern_result_error_types(tail, out);
            }
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    self.collect_pattern_result_error_types(item, out);
                }
            }
            TypedPattern::As(_, inner, _) => {
                self.collect_pattern_result_error_types(inner, out);
            }
            TypedPattern::Extractor { items, .. } => {
                out.push(Ty::Error);
                for item in items {
                    self.collect_pattern_result_error_types(item, out);
                }
            }
            TypedPattern::Var(_, _)
            | TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
        }
    }
}
