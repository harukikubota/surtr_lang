use super::*;
use diagnostics::{
    ConstDeclarationData, DeclarationIdentity, DiagnosticData, DiagnosticOrigin, Remediation,
    SourceFact, SourceId, SourceRole, StructuredDiagnostic, TypeDiagnosticReason,
};

struct ConstDeclaration<'a> {
    span: &'a Span,
    id: &'a ResolvedId,
    annotation: Option<&'a AstTy>,
    value: &'a Resolved,
    visibility: spire::ast::Visibility,
    references: Vec<(&'a Span, &'a ResolvedId)>,
}

impl Checker {
    fn const_error(
        reason: TypeDiagnosticReason,
        id: &ResolvedId,
        span: &Span,
        referenced: Option<&ResolvedId>,
        cycle: Vec<String>,
        related: Vec<SourceFact>,
    ) -> TypeError {
        TypeError::from_structured(StructuredDiagnostic {
            reason: reason.into(),
            origin: DiagnosticOrigin::Intrinsic,
            data: DiagnosticData::ConstDeclaration(ConstDeclarationData {
                constant: id.name.clone(),
                referenced_constant: referenced.map(|id| id.name.clone()),
                cycle,
            }),
            primary: SourceFact::untyped(SourceRole::Value, SourceId(0), span.clone()),
            related,
            remediation: Some(Remediation::Help {
                text: match reason {
                    TypeDiagnosticReason::ConstDynamicBracket => "Use literal container keys in const Facet paths and keep dynamic bracket expressions in local bindings or Facet API calls.",
                    TypeDiagnosticReason::CyclicConst => "Remove the cyclic Facet const reference.",
                    _ => "Use a primitive literal, a Facet path, a Facet const reference, or `->` composition of Facet paths.",
                }.into(),
            }),
        })
    }

    fn const_declaration_fact(id: &ResolvedId, span: &Span) -> SourceFact {
        let mut fact = SourceFact::untyped(SourceRole::Declaration, SourceId(0), span.clone());
        fact.declaration_identity = Some(DeclarationIdentity {
            owner: id.qualified_name.clone().unwrap_or_else(|| id.name.clone()),
            name: id.name.clone(),
        });
        fact
    }

    /// Validate the entire permitted expression shape before inspecting any
    /// dependency. In particular `f(SELF)` is a forbidden call, not a cycle.
    fn const_references<'a>(
        &self,
        id: &ResolvedId,
        declaration_span: &Span,
        value: &'a Resolved,
    ) -> Result<Vec<(&'a Span, &'a ResolvedId)>, TypeError> {
        let mut references = Vec::new();
        let mut pending = vec![value];
        while let Some(node) = pending.pop() {
            match node {
                Resolved::Lit(_, _) => {}
                Resolved::Var(span, referenced) => references.push((span, referenced)),
                Resolved::FieldAccess(_, inner, _) => pending.push(inner),
                Resolved::FacetSegmentAccess(_, inner, segment) => {
                    if let ResolvedFacetPathSegment::Bracket(expr) = segment {
                        let allowed = match expr.expr.as_ref() {
                            Resolved::Lit(_, Lit::Int(_) | Lit::Str(_)) => true,
                            Resolved::RangeLiteral(_, start, end) => matches!(
                                (start.as_ref(), end.as_ref()),
                                (Resolved::Lit(_, Lit::Int(_)), Resolved::Lit(_, Lit::Int(_)))
                            ),
                            _ => false,
                        };
                        if !allowed {
                            return Err(Self::const_error(
                                TypeDiagnosticReason::ConstDynamicBracket,
                                id,
                                self.resolved_span(&expr.expr),
                                None,
                                Vec::new(),
                                vec![Self::const_declaration_fact(id, declaration_span)],
                            ));
                        }
                    }
                    pending.push(inner);
                }
                Resolved::BinOp(_, BinOp::FacetChain, left, right) => {
                    pending.push(right);
                    pending.push(left);
                }
                _ => {
                    return Err(Self::const_error(
                        TypeDiagnosticReason::ConstValueForbidden,
                        id,
                        self.resolved_span(node),
                        None,
                        Vec::new(),
                        vec![Self::const_declaration_fact(id, declaration_span)],
                    ));
                }
            }
        }
        Ok(references)
    }

    pub(in crate::checker) fn predeclare_consts(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut declarations = Vec::new();
        let mut indices = HashMap::new();
        for stmt in stmts {
            let Resolved::ConstDef(span, id, annotation, value, attrs) = stmt else {
                continue;
            };
            let references = self.const_references(id, span, value)?;
            indices.insert(id.unique_id, declarations.len());
            declarations.push(ConstDeclaration {
                span,
                id,
                annotation: annotation.as_ref(),
                value,
                visibility: attrs.visibility,
                references,
            });
        }

        // These states are local to the stage. Only completed values enter the
        // persistent registry, so snapshots never retain pending declarations.
        let mut complete = vec![false; declarations.len()];
        let mut active_positions = vec![None; declarations.len()];
        for root in 0..declarations.len() {
            if complete[root] {
                continue;
            }
            let mut stack = vec![(root, 0usize)];
            active_positions[root] = Some(0);
            while let Some(&(index, next_reference)) = stack.last() {
                let declaration = &declarations[index];
                if let Some(&(reference_span, reference)) =
                    declaration.references.get(next_reference)
                {
                    if let Some(&dependency) = indices.get(&reference.unique_id) {
                        if let Some(position) = active_positions[dependency] {
                            let cycle = &stack[position..];
                            let names = cycle
                                .iter()
                                .map(|(index, _)| declarations[*index].id.name.clone())
                                .collect();
                            let mut related = Vec::with_capacity(cycle.len() * 2);
                            for (cycle_index, cursor) in cycle {
                                let member = &declarations[*cycle_index];
                                related.push(Self::const_declaration_fact(member.id, member.span));
                                // The frame cursor always points at the edge
                                // which entered the next active declaration.
                                let (edge_span, target) = member.references[*cursor];
                                let mut fact = SourceFact::untyped(
                                    SourceRole::Value,
                                    SourceId(0),
                                    edge_span.clone(),
                                );
                                fact.declaration_identity = Some(DeclarationIdentity {
                                    owner: target
                                        .qualified_name
                                        .clone()
                                        .unwrap_or_else(|| target.name.clone()),
                                    name: target.name.clone(),
                                });
                                related.push(fact);
                            }
                            return Err(Self::const_error(
                                TypeDiagnosticReason::CyclicConst,
                                declaration.id,
                                reference_span,
                                Some(reference),
                                names,
                                related,
                            ));
                        }
                        if !complete[dependency] {
                            active_positions[dependency] = Some(stack.len());
                            stack.push((dependency, 0));
                            continue;
                        }
                    }
                    if let Some(meta) = self.consts.get(&reference.unique_id) {
                        if meta.kind != ConstKind::FacetPath {
                            return Err(Self::const_error(
                                TypeDiagnosticReason::ConstValueForbidden,
                                declaration.id,
                                reference_span,
                                Some(reference),
                                Vec::new(),
                                vec![
                                    Self::const_declaration_fact(declaration.id, declaration.span),
                                    Self::const_declaration_fact(reference, &meta.span),
                                ],
                            ));
                        }
                    }
                    stack.last_mut().expect("active declaration").1 += 1;
                    continue;
                }

                let expected = declaration
                    .annotation
                    .map(|annotation| {
                        self.resolve_ast_ty_in_context(
                            annotation,
                            TypeSyntaxContext::BindingAnnotation,
                        )
                    })
                    .transpose()?;
                let checked =
                    self.check_node_with_expected(declaration.value, expected.as_ref())?;
                let typed = *self.resolve_typed_node(checked);
                let (kind, value) = match typed.node {
                    TypedInner::Lit(literal) => (
                        ConstKind::PrimitiveLiteral,
                        StoredConstValue::Literal(literal),
                    ),
                    TypedInner::FacetPath(path) => {
                        (ConstKind::FacetPath, StoredConstValue::FacetPath(path))
                    }
                    _ => {
                        return Err(Self::const_error(
                            TypeDiagnosticReason::ConstValueForbidden,
                            declaration.id,
                            self.resolved_span(declaration.value),
                            None,
                            Vec::new(),
                            vec![Self::const_declaration_fact(
                                declaration.id,
                                declaration.span,
                            )],
                        ))
                    }
                };
                self.env
                    .bind_var(declaration.id.unique_id, typed.ty.clone());
                self.consts.insert(
                    declaration.id.unique_id,
                    ConstMeta {
                        name: declaration.id.name.clone(),
                        visibility: declaration.visibility,
                        ty: typed.ty,
                        kind,
                        value,
                        span: declaration.span.clone(),
                    },
                );
                complete[index] = true;
                active_positions[index] = None;
                stack.pop();
            }
        }
        Ok(())
    }
}
