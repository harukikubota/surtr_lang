use super::*;

impl Checker {
    fn specialize_pattern_pin_dispatch(
        &mut self,
        ty: &Ty,
        dispatch: TraitDispatch,
        span: &Span,
        context: &mut SpecializationContext<'_>,
    ) -> Result<TraitDispatch, TypeError> {
        if let TraitDispatch::Selected(instantiation) = dispatch {
            return self.materialize_trait_method_instantiation(*instantiation, span, context);
        }
        if !matches!(dispatch, TraitDispatch::Pending)
            && !matches!(&dispatch, TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. })
                if context.needs_specialization.contains(fun_idx))
        {
            return Ok(dispatch);
        }
        let receiver = self.resolve_ty(ty);
        let eq_trait = self
            .trait_key_by_short_name("Eq")
            .ok_or_else(|| TypeError::new("Unknown trait: Eq", span.clone()))?;
        let trait_info = self
            .traits
            .get(&eq_trait)
            .cloned()
            .ok_or_else(|| TypeError::new("Missing Eq trait contract", span.clone()))?;
        let method = trait_info
            .methods
            .get("eq")
            .ok_or_else(|| TypeError::new("Missing Eq::eq method contract", span.clone()))?;
        let (_, result_ty, trait_args, _, _) =
            self.resolve_trait_method_signature(&trait_info, method, &receiver)?;
        match self.select_trait_method_instantiation(
            &eq_trait,
            "eq",
            &receiver,
            &trait_args,
            &[receiver.clone(), receiver.clone()],
            &result_ty,
        )? {
            CandidateApplicability::Applicable(instantiation) => {
                self.materialize_trait_method_instantiation(instantiation, span, context)
            }
            CandidateApplicability::Deferred(pending) => {
                let mut error = self.ambiguous_constructor_result("Eq", "eq", span);
                error.hint = Some(format!(
                    "Add enough type information to choose among {} remaining candidate(s) and resolve {} constructor input(s).",
                    pending.candidates.len(),
                    pending.waiting_on.len()
                ));
                Err(error)
            }
            CandidateApplicability::Rejected(_) => Err(self.trait_dispatch_failure(
                TypeDiagnosticReason::NoApplicableTraitImplementation,
                &eq_trait,
                "eq",
                Some(&receiver),
                span,
            )),
        }
    }

    pub(super) fn specialize_extractor_head(
        &mut self,
        extractor: &ResolvedId,
        extractor_ty: Ty,
        context: &mut SpecializationContext<'_>,
    ) -> Result<Ty, TypeError> {
        let mut reference = TypedNode {
            ty: self.resolve_ty(&extractor_ty),
            span: extractor.span.clone(),
            node: TypedInner::Var(extractor.clone()),
        };
        self.specialize_callable_reference(&mut reference, context)?;
        Ok(reference.ty)
    }

    fn specialize_extractor_pre_args(
        &mut self,
        args: Vec<TypedNode>,
        context: &mut SpecializationContext<'_>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        args.into_iter()
            .map(|arg| {
                self.rewrite_specializations_in_node(
                    arg,
                    context.defs_by_fun_idx,
                    context.bound_tyvars_by_fun_idx,
                    context.needs_specialization,
                    context.specialization_fun_idxs,
                    context.generated_defs,
                )
                .map(|node| *node)
                .map_err(|error| *error)
            })
            .collect()
    }

    fn validate_extractor_payload_shape(
        &self,
        extractor_ty: &Ty,
        seq_tys: &[Ty],
        children: usize,
        span: &Span,
    ) -> Result<(), TypeError> {
        let resolved = self.resolve_ty(extractor_ty);
        let ret = match Self::extractor_signature_ty(&resolved) {
            Ty::UserFunc { ret, .. } | Ty::BuiltinFunc { ret, .. } | Ty::Func(_, ret) => ret,
            _ => {
                return Err(TypeError::new(
                    "Extractor callable contract is unresolved",
                    span.clone(),
                ))
            }
        };
        let Ty::MatchResult(payload) = ret.as_ref() else {
            return Err(TypeError::new(
                "Extractor payload requires canonical MatchResult",
                span.clone(),
            ));
        };
        // Only the outer payload constructor determines child arity. Generic
        // types inside a known tuple or single-value container do not change
        // the runtime layout and remain valid under ordinary type erasure.
        if matches!(payload.as_ref(), Ty::Var(_) | Ty::SelfApp(_)) {
            return Err(TypeError::new(
                "Extractor payload shape must be resolved before execution",
                span.clone(),
            ));
        }
        let shape = match payload.as_ref() {
            Ty::Tuple(items) => items.clone(),
            other => vec![other.clone()],
        };
        let resolved_seq = seq_tys
            .iter()
            .map(|ty| self.resolve_ty(ty))
            .collect::<Vec<_>>();
        if shape != resolved_seq
            || (children != shape.len() && !(children == 0 && shape == [Ty::Unit]))
        {
            return Err(TypeError::new(
                "Extractor payload shape does not match its child patterns",
                span.clone(),
            ));
        }
        Ok(())
    }

    pub(super) fn concretize_specialized_typed_pattern(
        &mut self,
        pattern: TypedPattern,
        expected_ty: Option<&Ty>,
        span: &Span,
        context: &mut SpecializationContext<'_>,
    ) -> Result<TypedPattern, TypeError> {
        let normalized_ty = |checker: &Checker, ty: &Ty, expected: Option<&Ty>| {
            checker.resolve_ty(expected.unwrap_or(ty))
        };
        Ok(match pattern {
            TypedPattern::Located(source, inner) => TypedPattern::Located(
                source,
                Box::new(self.concretize_specialized_typed_pattern(
                    *inner,
                    expected_ty,
                    span,
                    context,
                )?),
            ),
            TypedPattern::Pin(ty, id, dispatch) => {
                let ty = normalized_ty(self, &ty, expected_ty);
                let dispatch =
                    self.specialize_pattern_pin_dispatch(&ty, dispatch, span, context)?;
                TypedPattern::Pin(ty, id, dispatch)
            }
            TypedPattern::HashMap(ty, entries) => {
                let ty = normalized_ty(self, &ty, expected_ty);
                let value_ty = self.hash_map_pattern_value_ty(&ty, span)?;
                let entries = entries
                    .into_iter()
                    .map(|entry| {
                        Ok(TypedHashMapPatternEntry {
                            key: self
                                .specialize_extractor_pre_args(vec![entry.key], context)?
                                .remove(0),
                            pattern: self.concretize_specialized_typed_pattern(
                                entry.pattern,
                                Some(&value_ty),
                                span,
                                context,
                            )?,
                            key_span: entry.key_span,
                        })
                    })
                    .collect::<Result<_, TypeError>>()?;
                TypedPattern::HashMap(ty, entries)
            }
            TypedPattern::Var(ty, id) => {
                TypedPattern::Var(normalized_ty(self, &ty, expected_ty), id)
            }
            TypedPattern::As(ty, inner, id) => {
                let ty = normalized_ty(self, &ty, expected_ty);
                TypedPattern::As(
                    ty.clone(),
                    Box::new(self.concretize_specialized_typed_pattern(
                        *inner,
                        Some(&ty),
                        span,
                        context,
                    )?),
                    id,
                )
            }
            TypedPattern::Wildcard(ty) => {
                TypedPattern::Wildcard(normalized_ty(self, &ty, expected_ty))
            }
            TypedPattern::ListNil(ty) => {
                TypedPattern::ListNil(normalized_ty(self, &ty, expected_ty))
            }
            TypedPattern::ListCons(ty, head, tail) => {
                let ty = normalized_ty(self, &ty, expected_ty);
                let item_ty = match &ty {
                    Ty::List(inner) => Some(inner.as_ref()),
                    _ => None,
                };
                let head =
                    self.concretize_specialized_typed_pattern(*head, item_ty, span, context)?;
                let tail =
                    self.concretize_specialized_typed_pattern(*tail, Some(&ty), span, context)?;
                TypedPattern::ListCons(ty, Box::new(head), Box::new(tail))
            }
            TypedPattern::IntLit(ty, value) => {
                TypedPattern::IntLit(normalized_ty(self, &ty, expected_ty), value)
            }
            TypedPattern::StrLit(ty, value) => {
                TypedPattern::StrLit(normalized_ty(self, &ty, expected_ty), value)
            }
            TypedPattern::BoolLit(ty, value) => {
                TypedPattern::BoolLit(normalized_ty(self, &ty, expected_ty), value)
            }
            TypedPattern::DurationLit(ty, value) => {
                TypedPattern::DurationLit(normalized_ty(self, &ty, expected_ty), value)
            }
            TypedPattern::Tuple(ty, items) => {
                let ty = normalized_ty(self, &ty, expected_ty);
                let expected_items = match &ty {
                    Ty::Tuple(items) => Some(items.as_slice()),
                    _ => None,
                };
                TypedPattern::Tuple(
                    ty.clone(),
                    items
                        .into_iter()
                        .enumerate()
                        .map(|(index, item)| {
                            self.concretize_specialized_typed_pattern(
                                item,
                                expected_items.and_then(|items| items.get(index)),
                                span,
                                context,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )
            }
            TypedPattern::Constructor {
                ty,
                tag,
                field_tys,
                fields,
                field_offset,
            } => {
                let ty = normalized_ty(self, &ty, expected_ty);
                let field_tys = field_tys
                    .into_iter()
                    .map(|field_ty| normalized_ty(self, &field_ty, None))
                    .collect::<Vec<_>>();
                let fields = fields
                    .into_iter()
                    .enumerate()
                    .map(|(index, field)| {
                        self.concretize_specialized_typed_pattern(
                            field,
                            field_tys.get(index),
                            span,
                            context,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                TypedPattern::Constructor {
                    ty,
                    tag,
                    field_tys,
                    fields,
                    field_offset,
                }
            }
            TypedPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                pre_args,
                success_tag,
                err_tag,
                seq_tys,
                items,
            } => {
                self.validate_extractor_payload_shape(
                    &extractor_ty,
                    &seq_tys,
                    items.len(),
                    &extractor.span,
                )?;
                let extractor_ty =
                    self.specialize_extractor_head(&extractor, extractor_ty, context)?;
                TypedPattern::Extractor {
                    input_ty: self.resolve_ty(&input_ty),
                    extractor,
                    extractor_ty,
                    pre_args: self.specialize_extractor_pre_args(pre_args, context)?,
                    success_tag,
                    err_tag,
                    seq_tys: seq_tys.iter().map(|ty| self.resolve_ty(ty)).collect(),
                    items: items
                        .into_iter()
                        .map(|item| {
                            self.concretize_specialized_typed_pattern(item, None, span, context)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                }
            }
        })
    }

    pub(super) fn concretize_specialized_match_pattern(
        &mut self,
        pattern: TypedMatchPattern,
        span: &Span,
        context: &mut SpecializationContext<'_>,
    ) -> Result<TypedMatchPattern, TypeError> {
        Ok(match pattern {
            TypedMatchPattern::HashMap(entries) => TypedMatchPattern::HashMap(
                entries
                    .into_iter()
                    .map(|entry| {
                        Ok(TypedHashMapMatchPatternEntry {
                            key: self
                                .specialize_extractor_pre_args(vec![entry.key], context)?
                                .remove(0),
                            pattern: self.concretize_specialized_match_pattern(
                                entry.pattern,
                                span,
                                context,
                            )?,
                            key_span: entry.key_span,
                        })
                    })
                    .collect::<Result<_, TypeError>>()?,
            ),
            TypedMatchPattern::Pin { id, ty, dispatch } => {
                let dispatch =
                    self.specialize_pattern_pin_dispatch(&ty, dispatch, span, context)?;
                TypedMatchPattern::Pin { id, ty, dispatch }
            }
            TypedMatchPattern::As(inner, id) => TypedMatchPattern::As(
                Box::new(self.concretize_specialized_match_pattern(*inner, span, context)?),
                id,
            ),
            TypedMatchPattern::Or(items) => TypedMatchPattern::Or(
                items
                    .into_iter()
                    .map(|item| self.concretize_specialized_match_pattern(item, span, context))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedMatchPattern::ErrorPayload {
                kind,
                fields: items,
            } => TypedMatchPattern::ErrorPayload {
                kind,
                fields: items
                    .into_iter()
                    .map(|item| self.concretize_specialized_match_pattern(item, span, context))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            TypedMatchPattern::Tuple(items) => TypedMatchPattern::Tuple(
                items
                    .into_iter()
                    .map(|item| self.concretize_specialized_match_pattern(item, span, context))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedMatchPattern::Record(items) => TypedMatchPattern::Record(
                items
                    .into_iter()
                    .map(|item| self.concretize_specialized_match_pattern(item, span, context))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedMatchPattern::Constructor {
                tag,
                fields,
                field_offset,
            } => TypedMatchPattern::Constructor {
                tag,
                fields: fields
                    .into_iter()
                    .map(|item| self.concretize_specialized_match_pattern(item, span, context))
                    .collect::<Result<Vec<_>, _>>()?,
                field_offset,
            },
            TypedMatchPattern::ListCons(head, tail) => TypedMatchPattern::ListCons(
                Box::new(self.concretize_specialized_match_pattern(*head, span, context)?),
                Box::new(self.concretize_specialized_match_pattern(*tail, span, context)?),
            ),
            TypedMatchPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                pre_args,
                success_tag,
                err_tag,
                seq_tys,
                items,
            } => {
                self.validate_extractor_payload_shape(
                    &extractor_ty,
                    &seq_tys,
                    items.len(),
                    &extractor.span,
                )?;
                let extractor_ty =
                    self.specialize_extractor_head(&extractor, extractor_ty, context)?;
                TypedMatchPattern::Extractor {
                    input_ty,
                    extractor,
                    extractor_ty,
                    pre_args: self.specialize_extractor_pre_args(pre_args, context)?,
                    success_tag,
                    err_tag,
                    seq_tys,
                    items: items
                        .into_iter()
                        .map(|item| self.concretize_specialized_match_pattern(item, span, context))
                        .collect::<Result<Vec<_>, _>>()?,
                }
            }
            other => other,
        })
    }
}
