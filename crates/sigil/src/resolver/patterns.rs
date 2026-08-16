use super::*;

const DUPLICATE_PATTERN_LABELS: [&str; 5] = ["first", "second", "third", "fourth", "fifth"];

impl Resolver {
    pub(super) fn resolve_pattern(
        &mut self,
        pat: AstPattern,
    ) -> Result<ResolvedPattern, ResolveError> {
        if let Some(error) = duplicate_pattern_binding_error(&pat) {
            return Err(error);
        }
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
                if seen.contains_key(&name) {
                    return Err(ResolveError {
                        message: format!(
                            "Pinned pattern requires an existing value `{}` outside the same pattern",
                            name
                        ),
                        span,
                        related_labels: Vec::new(),
                    });
                }
                let uid = self.scope.lookup(&name).ok_or_else(|| ResolveError {
                    message: format!("Pinned pattern requires an existing value `{}`", name),
                    span: span.clone(),
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
            AstPattern::ListNil(span) => Ok(ResolvedPattern::ListNil(span)),
            AstPattern::ListCons(_, head, tail) => Ok(ResolvedPattern::ListCons(
                Box::new(self.resolve_pattern_inner(*head, seen)?),
                Box::new(self.resolve_pattern_inner(*tail, seen)?),
            )),
            AstPattern::IntLit(span, n) => Ok(ResolvedPattern::IntLit(span, n)),
            AstPattern::StrLit(span, s) => Ok(ResolvedPattern::StrLit(span, s)),
            AstPattern::BoolLit(span, b) => Ok(ResolvedPattern::BoolLit(span, b)),
            AstPattern::DurationLit(span, n) => Ok(ResolvedPattern::DurationLit(span, n)),
            AstPattern::Constructor(span, ctor_name, inners) => {
                let ctor_uid = self.scope.lookup(&ctor_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined constructor: {}", ctor_name),
                    span: span.clone(),
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
                    related_labels: Vec::new(),
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
                            related_labels: Vec::new(),
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
                            related_labels: Vec::new(),
                        })
                    }
                    other => Err(ResolveError {
                        message: format!(
                            "MatchBlock head `{}` is not a constructor or extractor ({:?})",
                            head_name, other
                        ),
                        span,
                        related_labels: Vec::new(),
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
            AstPattern::As(_span, inner, alias, alias_ty, alias_span) => {
                let resolved_inner = self.resolve_pattern_inner(*inner, seen)?;
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

fn duplicate_pattern_binding_error(pat: &AstPattern) -> Option<ResolveError> {
    let mut occurrences = Vec::new();
    collect_pattern_bindings_preorder(pat, &mut occurrences);

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
        related_labels: spans
            .iter()
            .zip(DUPLICATE_PATTERN_LABELS)
            .map(|(span, message)| ResolveErrorLabel {
                span: span.clone(),
                message: message.to_string(),
            })
            .collect(),
    })
}

fn collect_pattern_bindings_preorder(pat: &AstPattern, out: &mut Vec<(String, Span)>) {
    match pat {
        AstPattern::Var(span, name) | AstPattern::Annotated(span, name, _) => {
            out.push((name.clone(), span.clone()));
        }
        AstPattern::As(_, inner, alias, _, alias_span) => {
            if let Some(items) = pattern_sequence_items(inner) {
                collect_as_sequence_bindings(items, alias, alias_span, out);
            } else {
                collect_pattern_bindings_preorder(inner, out);
                out.push((alias.clone(), alias_span.clone()));
            }
        }
        AstPattern::ListCons(_, head, tail) => {
            collect_pattern_bindings_preorder(head, out);
            collect_pattern_bindings_preorder(tail, out);
        }
        AstPattern::Constructor(_, _, inners)
        | AstPattern::Call(_, _, inners)
        | AstPattern::Tuple(_, inners)
        | AstPattern::Or(_, inners) => {
            for inner in inners {
                collect_pattern_bindings_preorder(inner, out);
            }
        }
        AstPattern::Pin(_, _)
        | AstPattern::Wildcard(_)
        | AstPattern::ListNil(_)
        | AstPattern::IntLit(_, _)
        | AstPattern::StrLit(_, _)
        | AstPattern::BoolLit(_, _)
        | AstPattern::DurationLit(_, _) => {}
    }
}

fn collect_as_sequence_bindings(
    items: Vec<&AstPattern>,
    alias: &str,
    alias_span: &Span,
    out: &mut Vec<(String, Span)>,
) {
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
                collect_pattern_bindings_preorder(item, out);
            }
            AstPattern::As(_, child, child_alias, _, child_alias_span)
                if matches!(
                    child.as_ref(),
                    AstPattern::Var(..) | AstPattern::Annotated(..)
                ) =>
            {
                collect_pattern_bindings_preorder(child, out);
                deferred_aliases.push((child_alias.clone(), child_alias_span.clone()));
            }
            _ => deferred_patterns.push(item),
        }
    }

    out.push((alias.to_string(), alias_span.clone()));
    out.extend(deferred_aliases);
    for item in deferred_patterns {
        collect_pattern_bindings_preorder(item, out);
    }
}

fn pattern_sequence_items(pattern: &AstPattern) -> Option<Vec<&AstPattern>> {
    match pattern {
        AstPattern::Tuple(_, items)
        | AstPattern::Constructor(_, _, items)
        | AstPattern::Call(_, _, items) => Some(items.iter().collect()),
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
