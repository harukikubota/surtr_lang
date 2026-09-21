use super::*;

const DUPLICATE_PATTERN_LABELS: [&str; 5] = ["first", "second", "third", "fourth", "fifth"];

impl Resolver {
    fn pattern_id(&self, name: String, uid: u32, span: Span) -> ResolvedId {
        ResolvedId {
            symbol_info: self.symbol_info_for_uid(&name, uid),
            name,
            qualified_name: None,
            unique_id: uid,
            compiler_generated: false,
            span,
        }
    }

    pub(super) fn pattern_has_deferred_application(&self, pattern: &AstPattern) -> bool {
        match pattern {
            AstPattern::Call(_, name, args) => {
                let kind = self
                    .scope
                    .lookup(name)
                    .and_then(|uid| self.declaration_uid_kinds.get(&uid));
                !matches!(
                    kind,
                    Some(
                        DeclarationKind::Extractor
                            | DeclarationKind::Enum
                            | DeclarationKind::Struct
                            | DeclarationKind::EnumVariant
                            | DeclarationKind::ResultCtor
                            | DeclarationKind::Record
                    )
                ) || args
                    .iter()
                    .filter_map(|arg| arg.pattern.as_deref())
                    .any(|item| self.pattern_has_deferred_application(item))
            }
            AstPattern::Constructor(_, _, items)
            | AstPattern::Tuple(_, items)
            | AstPattern::Or(_, items) => items
                .iter()
                .any(|item| self.pattern_has_deferred_application(item)),
            AstPattern::ListCons(_, head, tail) => {
                self.pattern_has_deferred_application(head)
                    || self.pattern_has_deferred_application(tail)
            }
            AstPattern::As(_, inner, _, _, _) => self.pattern_has_deferred_application(inner),
            _ => false,
        }
    }
    pub(super) fn select_pattern_argument_roles(
        &self,
        mut pattern: AstPattern,
    ) -> Result<AstPattern, ResolveError> {
        match &mut pattern {
            AstPattern::Call(span, name, args) => {
                let uid = self.scope.lookup(name).ok_or_else(|| {
                    pattern_argument_error(
                        format!("Undefined MatchBlock head: {name}"),
                        span.clone(),
                    )
                })?;
                let kind = self.declaration_uid_kinds.get(&uid);
                if !matches!(
                    kind,
                    Some(
                        DeclarationKind::Extractor
                            | DeclarationKind::Enum
                            | DeclarationKind::Struct
                            | DeclarationKind::EnumVariant
                            | DeclarationKind::ResultCtor
                            | DeclarationKind::Record
                    )
                ) {
                    return Ok(pattern);
                }
                let extractor_uid = match kind {
                    Some(DeclarationKind::Extractor) => Some(uid),
                    Some(DeclarationKind::Struct) => self
                        .attached_extractor_for_struct(uid, name)
                        .map(|(_, id, _)| id),
                    _ => None,
                };
                let pre_count = if let Some(extractor_uid) = extractor_uid {
                    self.declaration_entry_for_uid(extractor_uid)
                        .and_then(|entry| entry.value_parameter_count)
                        .and_then(|count| count.checked_sub(1))
                        .ok_or_else(|| {
                            pattern_argument_error(
                                "Extractor signature must declare at least one input",
                                span.clone(),
                            )
                        })?
                } else {
                    0
                };
                if args.len() < pre_count {
                    return Err(pattern_argument_error(
                        format!(
                            "Extractor expects {pre_count} pre-arguments, got {} total arguments",
                            args.len()
                        ),
                        span.clone(),
                    ));
                }
                for (index, argument) in args.iter_mut().enumerate() {
                    if index < pre_count {
                        if argument.expression.is_none() {
                            return Err(deferred_pattern_parse_error(
                                argument.expression_error.clone(),
                                "Extractor pre-argument must be an expression",
                                argument.span.clone(),
                            ));
                        }
                        argument.pattern = None;
                    } else {
                        let child = argument.pattern.take().ok_or_else(|| {
                            deferred_pattern_parse_error(
                                argument.pattern_error.clone(),
                                "Extractor payload argument must be a Pattern",
                                argument.span.clone(),
                            )
                        })?;
                        argument.pattern =
                            Some(Box::new(self.select_pattern_argument_roles(*child)?));
                        argument.expression = None;
                    }
                }
            }
            AstPattern::Constructor(_, _, items)
            | AstPattern::Tuple(_, items)
            | AstPattern::Or(_, items) => {
                for item in items {
                    *item = self.select_pattern_argument_roles(item.clone())?;
                }
            }
            AstPattern::ListCons(_, head, tail) => {
                **head = self.select_pattern_argument_roles(*head.clone())?;
                **tail = self.select_pattern_argument_roles(*tail.clone())?;
            }
            AstPattern::As(_, inner, _, _, _) => {
                **inner = self.select_pattern_argument_roles(*inner.clone())?
            }
            _ => {}
        }
        Ok(pattern)
    }

    pub(super) fn resolve_pattern(
        &mut self,
        pat: AstPattern,
    ) -> Result<ResolvedPattern, ResolveError> {
        let outer = self.scope.clone();
        let pat = self.select_pattern_argument_roles(pat)?;
        if self.pattern_has_deferred_application(&pat) {
            let mut candidates = Vec::new();
            collect_candidate_binding_names(&pat, &mut candidates);
            let mut proxies = HashMap::new();
            let mut bindings = Vec::new();
            for (name, span) in candidates {
                if proxies.contains_key(&name) {
                    continue;
                }
                let outer_id = outer
                    .lookup(&name)
                    .map(|uid| self.pattern_id(name.clone(), uid, span.clone()));
                let uid = self.scope.define(&name, span.clone());
                let proxy = self.pattern_id(name.clone(), uid, span);
                proxies.insert(name, proxy.clone());
                bindings.push(ResolvedPatternBinding {
                    proxy,
                    outer: outer_id,
                });
            }
            let saved = self.pattern_proxies.replace(proxies);
            let result = self.resolve_pattern_inner(pat, &mut HashMap::new(), &outer);
            self.pattern_proxies = saved;
            return result.map(|pattern| ResolvedPattern::Deferred {
                pattern: Box::new(pattern),
                bindings,
                allow_bindings: true,
            });
        }
        if let Some(error) = duplicate_pattern_binding_error(&pat)? {
            return Err(error);
        }
        let mut seen = HashMap::<String, Span>::new();
        self.resolve_pattern_inner(pat, &mut seen, &outer)
    }

    pub(super) fn define_pattern_binding(
        &mut self,
        name: String,
        span: Span,
        seen: &mut HashMap<String, Span>,
    ) -> Result<ResolvedId, ResolveError> {
        if let Some(proxies) = &self.pattern_proxies {
            let mut id = proxies.get(&name).cloned().ok_or_else(|| {
                let mut error =
                    pattern_argument_error("Missing provisional Pattern binding", span.clone());
                error.diagnostic.reason = crate::error::ResolveErrorReason::CompilerInvariant;
                error
            })?;
            id.span = span;
            return Ok(id);
        }
        if let Some(prev_span) = seen.get(&name) {
            return Err(ResolveError {
                message: format!("Duplicate binding in pattern: {}", name),
                span: Span {
                    start: prev_span.start,
                    end: span.end,
                },
                diagnostic: crate::error::ResolveErrorDiagnostic {
                    reason: crate::error::ResolveErrorReason::Pattern,
                    subject: None,
                },
                related_labels: Vec::new(),
            });
        }
        seen.insert(name.clone(), span.clone());
        let uid = self.scope.define(&name, span.clone());
        Ok(ResolvedId {
            name,
            qualified_name: None,
            unique_id: uid,
            compiler_generated: false,
            symbol_info: None,
            span,
        })
    }

    pub(super) fn resolve_pattern_inner(
        &mut self,
        pat: AstPattern,
        seen: &mut HashMap<String, Span>,
        outer: &Scope,
    ) -> Result<ResolvedPattern, ResolveError> {
        match pat {
            AstPattern::Var(span, name) => Ok(ResolvedPattern::Var(
                self.define_pattern_binding(name, span, seen)?,
            )),
            AstPattern::Annotated(span, name, ty) => Ok(ResolvedPattern::Annotated(
                self.define_pattern_binding(name, span, seen)?,
                ty,
            )),
            AstPattern::Pin(span, name) => {
                let uid = outer.lookup(&name).ok_or_else(|| ResolveError {
                    message: format!("Pinned pattern requires an existing value `{}`", name),
                    span: span.clone(),
                    diagnostic: crate::error::ResolveErrorDiagnostic {
                        reason: crate::error::ResolveErrorReason::Pattern,
                        subject: None,
                    },
                    related_labels: Vec::new(),
                })?;
                Ok(ResolvedPattern::Pin(ResolvedId {
                    name,
                    qualified_name: None,
                    unique_id: uid,
                    compiler_generated: false,
                    symbol_info: None,
                    span,
                }))
            }
            AstPattern::Wildcard(span) => Ok(ResolvedPattern::Wildcard(span)),
            AstPattern::AnnotatedWildcard(span, ty) => {
                Ok(ResolvedPattern::AnnotatedWildcard(span, ty))
            }
            AstPattern::ListNil(span) => Ok(ResolvedPattern::ListNil(span)),
            AstPattern::ListCons(_, head, tail) => Ok(ResolvedPattern::ListCons(
                Box::new(self.resolve_pattern_inner(*head, seen, outer)?),
                Box::new(self.resolve_pattern_inner(*tail, seen, outer)?),
            )),
            AstPattern::IntLit(span, n) => Ok(ResolvedPattern::IntLit(span, n)),
            AstPattern::StrLit(span, s) => Ok(ResolvedPattern::StrLit(span, s)),
            AstPattern::BoolLit(span, b) => Ok(ResolvedPattern::BoolLit(span, b)),
            AstPattern::DurationLit(span, n) => Ok(ResolvedPattern::DurationLit(span, n)),
            AstPattern::Constructor(span, ctor_name, inners) => {
                let ctor_uid = outer.lookup(&ctor_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined constructor: {}", ctor_name),
                    span: span.clone(),
                    diagnostic: crate::error::ResolveErrorDiagnostic {
                        reason: crate::error::ResolveErrorReason::Pattern,
                        subject: None,
                    },
                    related_labels: Vec::new(),
                })?;
                let symbol_info = self.symbol_info_for_uid(&ctor_name, ctor_uid);
                Ok(ResolvedPattern::Constructor(
                    ResolvedId {
                        name: ctor_name,
                        qualified_name: None,
                        unique_id: ctor_uid,
                        compiler_generated: false,
                        symbol_info,
                        span,
                    },
                    inners
                        .into_iter()
                        .map(|inner| self.resolve_pattern_inner(inner, seen, outer))
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
            AstPattern::Call(span, head_name, inners) => {
                let head_uid = outer.lookup(&head_name).ok_or_else(|| ResolveError {
                    message: if Self::is_constructor_style_head(&head_name) {
                        format!("Undefined constructor: {}", head_name)
                    } else {
                        format!("Undefined MatchBlock head: {}", head_name)
                    },
                    span: span.clone(),
                    diagnostic: crate::error::ResolveErrorDiagnostic {
                        reason: crate::error::ResolveErrorReason::Pattern,
                        subject: None,
                    },
                    related_labels: Vec::new(),
                })?;
                if !matches!(
                    self.declaration_uid_kinds.get(&head_uid),
                    Some(
                        DeclarationKind::Extractor
                            | DeclarationKind::Enum
                            | DeclarationKind::Struct
                            | DeclarationKind::EnumVariant
                            | DeclarationKind::ResultCtor
                            | DeclarationKind::Record
                    )
                ) {
                    let head = self.pattern_id(head_name, head_uid, span);
                    let mut args = Vec::new();
                    for argument in inners {
                        let next_id = self.scope.next_id();
                        let expr = match argument.expression {
                            Some(expr) => self
                                .with_child_scope(|child| {
                                    child.scope = outer.clone();
                                    child.scope.advance_next_id_to(next_id);
                                    child.pattern_proxies = None;
                                    child.resolve_node(*expr)
                                })
                                .map(Box::new),
                            None => Err(deferred_pattern_parse_error(
                                argument.expression_error,
                                "Extractor pre-argument must be an expression",
                                argument.span.clone(),
                            )),
                        };
                        let next_id = self.scope.next_id();
                        let pattern = match argument.pattern {
                            Some(pattern) => self
                                .with_child_scope(|child| {
                                    child.scope = outer.clone();
                                    child.scope.advance_next_id_to(next_id);
                                    let pattern = child.select_pattern_argument_roles(*pattern)?;
                                    child.resolve_pattern_inner(pattern, &mut HashMap::new(), outer)
                                })
                                .map(Box::new),
                            None => Err(deferred_pattern_parse_error(
                                argument.pattern_error,
                                "Extractor payload argument must be a Pattern",
                                argument.span.clone(),
                            )),
                        };
                        args.push(ResolvedPatternArgument {
                            span: argument.span,
                            expr,
                            pattern,
                        });
                    }
                    return Ok(ResolvedPattern::ExtractorApplication { head, args });
                }
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
                        diagnostic: crate::error::ResolveErrorDiagnostic {
                            reason: crate::error::ResolveErrorReason::Pattern,
                            subject: None,
                        },
                        related_labels: Vec::new(),
                    })?;
                let resolved_id = ResolvedId {
                    name: head_name.clone(),
                    qualified_name: None,
                    unique_id: head_uid,
                    compiler_generated: false,
                    symbol_info: self.symbol_info_for_uid(&head_name, head_uid),
                    span: span.clone(),
                };
                let mut pre_args = Vec::new();
                let mut resolved_inners = Vec::new();
                for argument in inners {
                    match (argument.expression, argument.pattern) {
                        (Some(expression), None) => {
                            let next_id = self.scope.next_id();
                            let resolved = self.with_child_scope(|child| {
                                child.scope = outer.clone();
                                child.scope.advance_next_id_to(next_id);
                                child.pattern_proxies = None;
                                child.resolve_node(*expression)
                            })?;
                            pre_args.push(resolved);
                        }
                        (None, Some(pattern)) => {
                            resolved_inners.push(self.resolve_pattern_inner(*pattern, seen, outer)?)
                        }
                        _ => {
                            return Err(pattern_argument_error(
                                "Pattern argument role was not resolved",
                                argument.span,
                            ))
                        }
                    }
                }
                match head_kind {
                    DeclarationKind::Extractor => {
                        if Self::is_constructor_style_head(&head_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Extractor names must not use constructor-style names like `{}`; implement `impl {} {{ defextractor deconstruct(...) ... }}` instead",
                                    head_name, head_name
                                ),
                                span,
                                diagnostic: crate::error::ResolveErrorDiagnostic {
                                    reason: crate::error::ResolveErrorReason::Pattern,
                                    subject: None,
                                },
                                related_labels: Vec::new(),
                            });
                        }
                        Ok(ResolvedPattern::Extractor(
                            resolved_id,
                            pre_args,
                            resolved_inners,
                        ))
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
                                diagnostic: crate::error::ResolveErrorDiagnostic {
                                    reason: crate::error::ResolveErrorReason::Pattern,
                                    subject: None,
                                },
                                related_labels: Vec::new(),
                            });
                        };
                        if !matches!(extractor_kind, DeclarationKind::Extractor) {
                            return Err(ResolveError {
                                message: format!(
                                    "Attached extractor for `{}` must be implemented as `impl {} {{ defextractor deconstruct(...) ... }}`",
                                    head_name, head_name
                                ),
                                span,
                                diagnostic: crate::error::ResolveErrorDiagnostic {
                                    reason: crate::error::ResolveErrorReason::Pattern,
                                    subject: None,
                                },
                                related_labels: Vec::new(),
                            });
                        }
                        Ok(ResolvedPattern::Extractor(
                            ResolvedId {
                                name: format!("{}::deconstruct", head_name),
                                qualified_name: extractor_qualified_name,
                                unique_id: extractor_uid,
                                compiler_generated: false,
                                symbol_info: self.symbol_info_for_uid(
                                    &format!("{}::deconstruct", head_name),
                                    extractor_uid,
                                ),
                                span,
                            },
                            pre_args,
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
                            diagnostic: crate::error::ResolveErrorDiagnostic {
                                reason: crate::error::ResolveErrorReason::Pattern,
                                subject: None,
                            },
                            related_labels: Vec::new(),
                        })
                    }
                    other => Err(ResolveError {
                        message: format!(
                            "MatchBlock head `{}` is not a constructor or extractor ({:?})",
                            head_name, other
                        ),
                        span,
                        diagnostic: crate::error::ResolveErrorDiagnostic {
                            reason: crate::error::ResolveErrorReason::Pattern,
                            subject: None,
                        },
                        related_labels: Vec::new(),
                    }),
                }
            }
            AstPattern::Tuple(_, items) => Ok(ResolvedPattern::Tuple(
                items
                    .into_iter()
                    .map(|item| self.resolve_pattern_inner(item, seen, outer))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            AstPattern::Or(span, items) => {
                if self.pattern_proxies.is_some() {
                    return Ok(ResolvedPattern::Or(
                        items
                            .into_iter()
                            .map(|item| {
                                self.resolve_pattern_inner(item, &mut HashMap::new(), outer)
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                }
                let mut resolved_items = Vec::with_capacity(items.len());
                let mut common_ids = HashMap::<String, u32>::new();
                let mut common_bindings = Vec::<(String, Span)>::new();
                for (index, item) in items.into_iter().enumerate() {
                    let mut alternative_seen = seen.clone();
                    let mut bindings = Vec::new();
                    collect_pattern_bindings_preorder(&item, &mut bindings)?;
                    let (mut resolved, ids) = self.with_child_scope(|child| {
                        let resolved = child.resolve_pattern_inner(item, &mut alternative_seen, outer)?;
                        let ids = bindings
                            .iter()
                            .map(|(name, _)| {
                                child.scope.lookup(name).ok_or_else(|| ResolveError {
                                    message: format!(
                                        "Internal invariant broken: OR binding `{name}` was not defined"
                                    ),
                                    span: span.clone(),
                                    diagnostic: crate::error::ResolveErrorDiagnostic {
                                        reason: crate::error::ResolveErrorReason::Pattern,
                                        subject: None,
                                    },
                                    related_labels: Vec::new(),
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok((resolved, ids))
                    })?;
                    if index == 0 {
                        for ((name, _), id) in bindings.iter().zip(ids) {
                            common_ids.insert(name.clone(), id);
                        }
                        common_bindings = bindings;
                    } else {
                        remap_or_pattern_bindings(&mut resolved, &common_ids)?;
                    }
                    resolved_items.push(resolved);
                }
                for (name, span) in common_bindings {
                    seen.insert(name.clone(), span);
                    let id = common_ids[&name];
                    self.scope.define_with_id(&name, id);
                }
                Ok(ResolvedPattern::Or(resolved_items))
            }
            AstPattern::As(_span, inner, alias, alias_ty, alias_span) => {
                let resolved_inner = self.resolve_pattern_inner(*inner, seen, outer)?;
                let alias_id = self.define_pattern_binding(alias, alias_span, seen)?;
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

fn remap_or_pattern_bindings(
    pattern: &mut ResolvedPattern,
    common_ids: &HashMap<String, u32>,
) -> Result<(), ResolveError> {
    match pattern {
        ResolvedPattern::Deferred { .. } | ResolvedPattern::ExtractorApplication { .. } => {
            return Err(pattern_argument_error(
                "Deferred Pattern reached eager OR remapping",
                Span { start: 0, end: 0 },
            ))
        }
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
            id.unique_id = common_ids
                .get(&id.name)
                .copied()
                .ok_or_else(|| ResolveError {
                    message: format!(
                        "Internal invariant broken: OR binding `{}` has no common identity",
                        id.name
                    ),
                    span: id.span.clone(),
                    diagnostic: crate::error::ResolveErrorDiagnostic {
                        reason: crate::error::ResolveErrorReason::Pattern,
                        subject: None,
                    },
                    related_labels: Vec::new(),
                })?;
        }
        ResolvedPattern::As(inner, alias, _) => {
            remap_or_pattern_bindings(inner, common_ids)?;
            alias.unique_id = common_ids
                .get(&alias.name)
                .copied()
                .ok_or_else(|| ResolveError {
                    message: format!(
                        "Internal invariant broken: OR alias `{}` has no common identity",
                        alias.name
                    ),
                    span: alias.span.clone(),
                    diagnostic: crate::error::ResolveErrorDiagnostic {
                        reason: crate::error::ResolveErrorReason::Pattern,
                        subject: None,
                    },
                    related_labels: Vec::new(),
                })?;
        }
        ResolvedPattern::Constructor(_, items)
        | ResolvedPattern::Extractor(_, _, items)
        | ResolvedPattern::Tuple(items)
        | ResolvedPattern::Or(items) => {
            for item in items {
                remap_or_pattern_bindings(item, common_ids)?;
            }
        }
        ResolvedPattern::ListCons(head, tail) => {
            remap_or_pattern_bindings(head, common_ids)?;
            remap_or_pattern_bindings(tail, common_ids)?;
        }
        ResolvedPattern::Pin(_)
        | ResolvedPattern::Wildcard(_)
        | ResolvedPattern::AnnotatedWildcard(_, _)
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(_, _)
        | ResolvedPattern::StrLit(_, _)
        | ResolvedPattern::BoolLit(_, _)
        | ResolvedPattern::DurationLit(_, _) => {}
    }
    Ok(())
}

fn collect_candidate_binding_names(pattern: &AstPattern, out: &mut Vec<(String, Span)>) {
    match pattern {
        AstPattern::Var(span, name) | AstPattern::Annotated(span, name, _) => {
            out.push((name.clone(), span.clone()))
        }
        AstPattern::As(_, inner, name, _, span) => {
            collect_candidate_binding_names(inner, out);
            out.push((name.clone(), span.clone()));
        }
        AstPattern::Call(_, _, args) => {
            for inner in args.iter().filter_map(|arg| arg.pattern.as_deref()) {
                collect_candidate_binding_names(inner, out);
            }
        }
        AstPattern::Constructor(_, _, items)
        | AstPattern::Tuple(_, items)
        | AstPattern::Or(_, items) => {
            for inner in items {
                collect_candidate_binding_names(inner, out);
            }
        }
        AstPattern::ListCons(_, head, tail) => {
            collect_candidate_binding_names(head, out);
            collect_candidate_binding_names(tail, out);
        }
        _ => {}
    }
}

fn duplicate_pattern_binding_error(pat: &AstPattern) -> Result<Option<ResolveError>, ResolveError> {
    let mut occurrences = Vec::new();
    collect_pattern_bindings_preorder(pat, &mut occurrences)?;

    Ok(duplicate_binding_occurrences_error(occurrences))
}

fn duplicate_binding_occurrences_error(occurrences: Vec<(String, Span)>) -> Option<ResolveError> {
    let mut by_name = HashMap::<String, Vec<Span>>::new();
    let mut duplicate_name = None;
    for (name, span) in occurrences {
        let spans = by_name.entry(name.clone()).or_default();
        if spans.len() < DUPLICATE_PATTERN_LABELS.len() {
            spans.push(span);
        }
        if spans.len() == 2 && duplicate_name.is_none() {
            duplicate_name = Some(name);
        }
    }

    let name = duplicate_name?;
    let spans = by_name.get(&name)?;
    let first = spans.first()?.clone();
    Some(ResolveError {
        message: format!("Duplicate binding in pattern: {}", name),
        span: first,
        diagnostic: crate::error::ResolveErrorDiagnostic {
            reason: crate::error::ResolveErrorReason::Pattern,
            subject: None,
        },
        related_labels: spans
            .iter()
            .zip(DUPLICATE_PATTERN_LABELS)
            .map(|(span, message)| ResolveErrorLabel {
                span: span.clone(),
                message: message.to_string(),
                source: None,
            })
            .collect(),
    })
}

fn collect_pattern_bindings_preorder(
    pat: &AstPattern,
    out: &mut Vec<(String, Span)>,
) -> Result<(), ResolveError> {
    match pat {
        AstPattern::Var(span, name) | AstPattern::Annotated(span, name, _) => {
            out.push((name.clone(), span.clone()));
        }
        AstPattern::As(_, inner, alias, _, alias_span) => {
            if let Some(items) = pattern_sequence_items(inner) {
                collect_as_sequence_bindings(items, alias, alias_span, out)?;
            } else {
                collect_pattern_bindings_preorder(inner, out)?;
                out.push((alias.clone(), alias_span.clone()));
            }
        }
        AstPattern::ListCons(_, head, tail) => {
            collect_pattern_bindings_preorder(head, out)?;
            collect_pattern_bindings_preorder(tail, out)?;
        }
        AstPattern::Constructor(_, _, inners) | AstPattern::Tuple(_, inners) => {
            for inner in inners {
                collect_pattern_bindings_preorder(inner, out)?;
            }
        }
        AstPattern::Call(_, _, args) => {
            for pattern in args.iter().filter_map(|arg| arg.pattern.as_deref()) {
                collect_pattern_bindings_preorder(pattern, out)?;
            }
        }
        AstPattern::Or(span, alternatives) => {
            let mut common = None::<Vec<(String, Span)>>;
            for alternative in alternatives {
                let mut bindings = Vec::new();
                collect_pattern_bindings_preorder(alternative, &mut bindings)?;
                if let Some(error) = duplicate_binding_occurrences_error(bindings.clone()) {
                    return Err(error);
                }
                if let Some(expected) = &common {
                    let expected_names = expected.iter().map(|(name, _)| name).collect::<Vec<_>>();
                    let actual_names = bindings.iter().map(|(name, _)| name).collect::<Vec<_>>();
                    if expected_names != actual_names {
                        return Err(ResolveError {
                            message: format!(
                                "OR pattern alternatives must bind the same binding names in the same order: expected {:?}, got {:?}",
                                expected_names, actual_names
                            ),
                            span: span.clone(),
                            diagnostic: crate::error::ResolveErrorDiagnostic {
                                reason: crate::error::ResolveErrorReason::Pattern,
                                subject: None,
                            },
                            related_labels: Vec::new(),
                        });
                    }
                } else {
                    common = Some(bindings);
                }
            }
            out.extend(common.unwrap_or_default());
        }
        AstPattern::Pin(_, _)
        | AstPattern::Wildcard(_)
        | AstPattern::AnnotatedWildcard(_, _)
        | AstPattern::ListNil(_)
        | AstPattern::IntLit(_, _)
        | AstPattern::StrLit(_, _)
        | AstPattern::BoolLit(_, _)
        | AstPattern::DurationLit(_, _) => {}
    }
    Ok(())
}

fn collect_as_sequence_bindings(
    items: Vec<&AstPattern>,
    alias: &str,
    alias_span: &Span,
    out: &mut Vec<(String, Span)>,
) -> Result<(), ResolveError> {
    // Binding order contract: direct bindings of an as-pattern sequence come
    // first, followed by the parent alias, direct child aliases, and then
    // recursively deferred child patterns. `pattern_sequence_items` flattens
    // lists and presents tuple/record/enum/extractor patterns as one sequence.
    // Duplicate-binding diagnostics use this same order as REPL metadata.
    let mut deferred_aliases = Vec::new();
    let mut deferred_patterns = Vec::new();

    for item in items {
        match item {
            AstPattern::Var(..) | AstPattern::Annotated(..) => {
                collect_pattern_bindings_preorder(item, out)?;
            }
            AstPattern::As(_, child, child_alias, _, child_alias_span)
                if matches!(
                    child.as_ref(),
                    AstPattern::Var(..) | AstPattern::Annotated(..)
                ) =>
            {
                collect_pattern_bindings_preorder(child, out)?;
                deferred_aliases.push((child_alias.clone(), child_alias_span.clone()));
            }
            _ => deferred_patterns.push(item),
        }
    }

    out.push((alias.to_string(), alias_span.clone()));
    out.extend(deferred_aliases);
    for item in deferred_patterns {
        collect_pattern_bindings_preorder(item, out)?;
    }
    Ok(())
}

fn pattern_sequence_items(pattern: &AstPattern) -> Option<Vec<&AstPattern>> {
    match pattern {
        AstPattern::Tuple(_, items) | AstPattern::Constructor(_, _, items) => {
            Some(items.iter().collect())
        }
        AstPattern::Call(_, _, args) => Some(
            args.iter()
                .filter_map(|arg| arg.pattern.as_deref())
                .collect(),
        ),
        AstPattern::ListCons(..) => {
            let mut items = Vec::new();
            flatten_list_pattern(pattern, &mut items);
            Some(items)
        }
        _ => None,
    }
}

fn flatten_list_pattern<'a>(pattern: &'a AstPattern, out: &mut Vec<&'a AstPattern>) {
    match pattern {
        AstPattern::ListCons(_, head, tail) => {
            out.push(head);
            flatten_list_pattern(tail, out);
        }
        AstPattern::ListNil(_) => {}
        other => out.push(other),
    }
}

fn pattern_argument_error(message: impl Into<String>, span: Span) -> ResolveError {
    ResolveError {
        message: message.into(),
        span,
        diagnostic: crate::error::ResolveErrorDiagnostic {
            reason: crate::error::ResolveErrorReason::Pattern,
            subject: None,
        },
        related_labels: Vec::new(),
    }
}

fn deferred_pattern_parse_error(
    error: Option<spire::error::ParseError>,
    message: &str,
    span: Span,
) -> ResolveError {
    let Some(error) = error else {
        return ResolveError {
            message: format!("Missing deferred parse diagnostic: {message}"),
            span,
            diagnostic: crate::error::ResolveErrorDiagnostic {
                reason: crate::error::ResolveErrorReason::CompilerInvariant,
                subject: None,
            },
            related_labels: Vec::new(),
        };
    };
    ResolveError {
        message: error.message().to_string(),
        span: error.span().clone(),
        diagnostic: crate::error::ResolveErrorDiagnostic {
            reason: crate::error::ResolveErrorReason::DeferredParse(error),
            subject: None,
        },
        related_labels: Vec::new(),
    }
}
