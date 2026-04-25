use super::*;

impl Resolver {
    pub(super) fn resolve_pattern(
        &mut self,
        pat: AstPattern,
    ) -> Result<ResolvedPattern, ResolveError> {
        let mut seen = HashMap::<String, Span>::new();
        self.resolve_pattern_inner(pat, &mut seen)
    }

    pub(super) fn define_pattern_binding(
        &mut self,
        name: String,
        span: Span,
        seen: &mut HashMap<String, Span>,
    ) -> Result<ResolvedId, ResolveError> {
        if let Some(prev_span) = seen.get(&name) {
            return Err(ResolveError {
                message: format!("Duplicate binding in pattern: {}", name),
                span: Span {
                    start: prev_span.start,
                    end: span.end,
                },
            });
        }
        seen.insert(name.clone(), span.clone());
        let uid = self.scope.define(&name, span.clone());
        Ok(ResolvedId {
            name,
            qualified_name: None,
            unique_id: uid,
            span,
        })
    }

    pub(super) fn resolve_pattern_inner(
        &mut self,
        pat: AstPattern,
        seen: &mut HashMap<String, Span>,
    ) -> Result<ResolvedPattern, ResolveError> {
        match pat {
            AstPattern::Var(span, name) => Ok(ResolvedPattern::Var(
                self.define_pattern_binding(name, span, seen)?,
            )),
            AstPattern::Annotated(span, name, ty) => Ok(ResolvedPattern::Annotated(
                self.define_pattern_binding(name, span, seen)?,
                ty,
            )),
            AstPattern::Wildcard(span) => Ok(ResolvedPattern::Wildcard(span)),
            AstPattern::ListNil(span) => Ok(ResolvedPattern::ListNil(span)),
            AstPattern::ListCons(_, head, tail) => Ok(ResolvedPattern::ListCons(
                Box::new(self.resolve_pattern_inner(*head, seen)?),
                Box::new(self.resolve_pattern_inner(*tail, seen)?),
            )),
            AstPattern::IntLit(span, n) => Ok(ResolvedPattern::IntLit(span, n)),
            AstPattern::StrLit(span, s) => Ok(ResolvedPattern::StrLit(span, s)),
            AstPattern::BoolLit(span, b) => Ok(ResolvedPattern::BoolLit(span, b)),
            AstPattern::Constructor(span, ctor_name, inners) => {
                let ctor_uid = self.scope.lookup(&ctor_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined constructor: {}", ctor_name),
                    span: span.clone(),
                })?;
                Ok(ResolvedPattern::Constructor(
                    ResolvedId {
                        name: ctor_name,
                        qualified_name: None,
                        unique_id: ctor_uid,
                        span,
                    },
                    inners
                        .into_iter()
                        .map(|inner| self.resolve_pattern_inner(inner, seen))
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
            AstPattern::Call(span, head_name, inners) => {
                let head_uid = self.scope.lookup(&head_name).ok_or_else(|| ResolveError {
                    message: if Self::is_constructor_style_head(&head_name) {
                        format!("Undefined constructor: {}", head_name)
                    } else {
                        format!("Undefined MatchBlock head: {}", head_name)
                    },
                    span: span.clone(),
                })?;
                let head_kind = self
                    .declaration_uid_kinds
                    .get(&head_uid)
                    .cloned()
                    .or_else(|| {
                        if matches!(head_name.as_str(), "Ok" | "Err") {
                            Some(DeclarationKind::ResultCtor)
                        } else if head_name.contains("::") {
                            Some(DeclarationKind::EnumVariant)
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| ResolveError {
                        message: format!("Unknown MatchBlock head: {}", head_name),
                        span: span.clone(),
                    })?;
                let resolved_id = ResolvedId {
                    name: head_name.clone(),
                    qualified_name: None,
                    unique_id: head_uid,
                    span: span.clone(),
                };
                let resolved_inners = inners
                    .into_iter()
                    .map(|inner| self.resolve_pattern_inner(inner, seen))
                    .collect::<Result<Vec<_>, _>>()?;
                match head_kind {
                    DeclarationKind::Extractor => {
                        if Self::is_constructor_style_head(&head_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Extractor names must not use constructor-style names like `{}`; implement `impl {} {{ defextractor deconstruct(...) ... }}` instead",
                                    head_name, head_name
                                ),
                                span,
                            });
                        }
                        Ok(ResolvedPattern::Extractor(resolved_id, resolved_inners))
                    }
                    DeclarationKind::EnumVariant | DeclarationKind::ResultCtor => {
                        Ok(ResolvedPattern::Constructor(resolved_id, resolved_inners))
                    }
                    DeclarationKind::Struct => {
                        let Some((extractor_qualified_name, extractor_uid, extractor_kind)) =
                            self.attached_extractor_for_struct(head_uid, &head_name)
                        else {
                            return Err(ResolveError {
                                message: format!(
                                    "MatchBlock head `{}` requires attached extractor `{}::deconstruct`, but it is not defined",
                                    head_name, head_name
                                ),
                                span,
                            });
                        };
                        if !matches!(extractor_kind, DeclarationKind::Extractor) {
                            return Err(ResolveError {
                                message: format!(
                                    "Attached extractor for `{}` must be implemented as `impl {} {{ defextractor deconstruct(...) ... }}`",
                                    head_name, head_name
                                ),
                                span,
                            });
                        }
                        Ok(ResolvedPattern::Extractor(
                            ResolvedId {
                                name: format!("{}::deconstruct", head_name),
                                qualified_name: extractor_qualified_name,
                                unique_id: extractor_uid,
                                span,
                            },
                            resolved_inners,
                        ))
                    }
                    DeclarationKind::Record => {
                        // Records will eventually gain compiler-generated deconstructors.
                        // For now, keep `Record(...)` MatchBlock heads explicitly unsupported.
                        Err(ResolveError {
                            message: format!(
                                "Record MatchBlock heads like `{}` are not supported yet",
                                head_name
                            ),
                            span,
                        })
                    }
                    other => Err(ResolveError {
                        message: format!(
                            "MatchBlock head `{}` is not a constructor or extractor ({:?})",
                            head_name, other
                        ),
                        span,
                    }),
                }
            }
            AstPattern::Tuple(_, items) => Ok(ResolvedPattern::Tuple(
                items
                    .into_iter()
                    .map(|item| self.resolve_pattern_inner(item, seen))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            AstPattern::Or(_, items) => Ok(ResolvedPattern::Or(
                items
                    .into_iter()
                    .map(|item| self.resolve_pattern_inner(item, seen))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            AstPattern::As(span, inner, alias, alias_ty) => {
                let resolved_inner = self.resolve_pattern_inner(*inner, seen)?;
                let alias_id = self.define_pattern_binding(alias, span, seen)?;
                Ok(ResolvedPattern::As(
                    Box::new(resolved_inner),
                    alias_id,
                    alias_ty,
                ))
            }
        }
    }

    pub(super) fn resolve_match_arm(
        &mut self,
        arm: AstMatchArm,
    ) -> Result<ResolvedMatchArm, ResolveError> {
        self.with_child_scope(|child| {
            let resolved_pat = child.resolve_pattern(arm.pattern)?;
            let resolved_guard = match arm.guard {
                Some(guard) => Some(child.resolve_node(guard)?),
                None => None,
            };
            let resolved_body = child.resolve_node(arm.body)?;
            Ok(ResolvedMatchArm {
                pattern: resolved_pat,
                guard: resolved_guard,
                body: resolved_body,
            })
        })
    }
}
