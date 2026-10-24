use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

#[derive(Clone)]
pub(super) struct ExpectedTypeRelation {
    span: Span,
    role: SourceRole,
    reason: TypeDiagnosticReason,
    origin: DiagnosticOrigin,
    callable: &'static str,
    ordinal: u32,
    explicit_carrier_span: Option<Span>,
    synthetic_do_match_pending: bool,
}

impl ExpectedTypeRelation {
    fn contextual(span: &Span) -> Self {
        Self {
            span: span.clone(),
            role: SourceRole::Expected,
            reason: TypeDiagnosticReason::TypePayloadMismatch,
            origin: DiagnosticOrigin::Intrinsic,
            callable: "do",
            ordinal: 0,
            explicit_carrier_span: None,
            synthetic_do_match_pending: false,
        }
    }

    fn function_return(span: &Span) -> Self {
        Self {
            span: span.clone(),
            role: SourceRole::Expected,
            reason: TypeDiagnosticReason::ReturnTypeMismatch,
            origin: DiagnosticOrigin::Return,
            callable: "function",
            ordinal: 0,
            explicit_carrier_span: None,
            synthetic_do_match_pending: false,
        }
    }

    fn annotation(span: &Span) -> Self {
        Self {
            span: span.clone(),
            role: SourceRole::Annotation,
            reason: TypeDiagnosticReason::AnnotationTypeMismatch,
            origin: DiagnosticOrigin::Annotation,
            callable: "binding",
            ordinal: 0,
            explicit_carrier_span: None,
            synthetic_do_match_pending: false,
        }
    }

    fn return_type_argument(span: &Span) -> Self {
        Self {
            span: span.clone(),
            role: SourceRole::ReturnTypeArgument,
            reason: TypeDiagnosticReason::ReturnTypeArgumentMismatch,
            origin: DiagnosticOrigin::ReturnTypeArgument { ordinal: 0 },
            callable: "do",
            ordinal: 0,
            explicit_carrier_span: Some(span.clone()),
            synthetic_do_match_pending: false,
        }
    }

    fn for_do_continuation(&self) -> Self {
        let mut relation = self.clone();
        relation.synthetic_do_match_pending = true;
        relation
    }

    fn after_synthetic_do_match(&self) -> Self {
        let mut relation = self.clone();
        relation.synthetic_do_match_pending = false;
        relation
    }
}

mod arguments;
mod calls;
mod closures;
mod constructors;
mod control;
mod do_block;
mod extractors;
mod facet;
mod lazy_calls;
mod literals;
mod operators;
mod probes;
mod process_calls;
mod trait_calls;

pub(super) use calls::callable_definition_display_name;

#[cfg(test)]
mod safe_operator_provenance_tests;
#[cfg(test)]
mod tests;

impl Checker {
    // Recursive block/callable traversal must not retain the large temporary
    // frame used to check declarations and other individual expressions.
    pub(super) fn check_node(&mut self, node: &Resolved) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Block(span, stmts) => self.check_block(span, stmts, None, None),
            Resolved::ExtractorClosure(span, params, captures, body) => {
                self.check_extractor_closure(span, params, captures, body, None, None)
            }
            Resolved::Closure(span, params, captures, body) => {
                self.check_closure(span, params, captures, body, None, None)
            }
            Resolved::CaptureClosure(span, params, captures, body) => {
                self.check_capture_closure(span, params, captures, body, None)
            }
            Resolved::Do(span, intrinsic, contract, return_type_arguments, statements) => self
                .check_do(
                    span,
                    *intrinsic,
                    contract,
                    return_type_arguments,
                    statements,
                    None,
                    None,
                ),
            Resolved::App(span, func, args) => self.check_app(span, func, args),
            Resolved::LazyApp(span, kind, func, args) => {
                self.check_lazy_app(span, *kind, func, args, None)
            }
            Resolved::Match(span, scrutinee, arms) | Resolved::IsMatch(span, scrutinee, arms) => {
                self.check_match(span, scrutinee, arms, None, None)
            }
            _ => self.check_non_structural_node(node),
        }
    }

    #[inline(never)]
    fn check_non_structural_node(&mut self, node: &Resolved) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Lit(span, lit) => {
                let ty = self.lit_type(lit);
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::Lit(lit.clone()),
                })
            }

            Resolved::ErrorKind(span, id) => {
                if !self.env.is_error_constructor(id.unique_id) {
                    return Err(TypeError::new("ErrorKind requires a canonical deferror declaration", span.clone()));
                }
                let kind = id.qualified_name.clone().ok_or_else(|| {
                    TypeError::new("ErrorKind identity has no canonical declaration name", span.clone())
                })?;
                Ok(TypedNode {
                    ty: Ty::Enum("ErrorKind".into(), Vec::new()),
                    span: span.clone(),
                    node: TypedInner::ErrorKind(kind),
                })
            }

            Resolved::Var(span, id) => {
                let canonical = self.canonical_pattern_id(id)?;
                if canonical.unique_id != id.unique_id { return self.check_node(&Resolved::Var(span.clone(), canonical)); }
                if self.trait_method_ref(node).is_some() {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait helper `{}` cannot be referenced directly",
                            id.name
                        ),
                        span: span.clone(),
                        hint: Some(
                            "Call the helper with arguments so the receiver type can choose an impl."
                                .into(),
                        ),
                    });
                }

                if let Some(const_meta) = self.consts.get(&id.unique_id) {
                    return Ok(match &const_meta.value {
                        StoredConstValue::Literal(lit) => TypedNode {
                            ty: const_meta.ty.clone(),
                            span: span.clone(),
                            node: TypedInner::Lit(lit.clone()),
                        },
                        StoredConstValue::FacetPath(path) => TypedNode {
                            ty: const_meta.ty.clone(),
                            span: span.clone(),
                            node: TypedInner::FacetPath(path.clone()),
                        },
                    });
                }

                if let Some(stored_ty) = self.env.lookup_var(id.unique_id).cloned() {
                    let ty = match &stored_ty {
                        Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } => {
                            self.instantiate_callable_ty(&stored_ty)
                        }
                        _ => self.resolve_ty(&stored_ty),
                    };
                    if matches!(ty, Ty::Facet(..)) {
                        if let Some(path) = self.facet_bindings.get(&id.unique_id).cloned() {
                            return Ok(match path {
                                StoredFacetPath::Concrete(path) => {
                                    let source_ty = self.resolve_ty(&path.source_ty);
                                    let focus_ty = self.resolve_ty(&path.focus_ty);
                                    let update_source_ty = self.resolve_ty(&path.update_source_ty);
                                    let update_focus_ty = self.resolve_ty(&path.update_focus_ty);
                                    TypedNode {
                                        ty: Ty::Facet(
                                            Self::facet_kind_from_path_kind(path.path_kind),
                                            Box::new(source_ty.clone()),
                                            Box::new(focus_ty.clone()),
                                            Box::new(update_source_ty.clone()),
                                            Box::new(update_focus_ty.clone()),
                                        ),
                                        span: span.clone(),
                                        node: TypedInner::FacetPath(TypedFacetPath {
                                            source_ty,
                                            focus_ty,
                                            update_source_ty,
                                            update_focus_ty,
                                            path_kind: path.path_kind,
                                            may_fail: path.may_fail,
                                            segment_focus_tys: path.segment_focus_tys.iter().map(|ty| self.resolve_ty(ty)).collect(),
                                            segments: path.segments,
                                        }),
                                    }
                                }
                                StoredFacetPath::Pending(path) => TypedNode {
                                    ty,
                                    span: span.clone(),
                                    node: TypedInner::PendingFacetPath(path),
                                },
                            });
                        }
                        return Err(TypeError {
                            structured: None,
                            message: "Facet value is not statically resolvable at this usage site"
                                .into(),
                            span: span.clone(),
                            hint: Some(
                                "Use a concrete path expression like User.name or pair._0.".into(),
                            ),
                        });
                    }
                    return Ok(TypedNode {
                        ty,
                        span: span.clone(),
                        node: TypedInner::Var(id.clone()),
                    });
                }

                if let Some(variant) = self.lookup_enum_variant_by_constructor_id(id.unique_id) {
                    let variant = self.instantiate_enum_variant(&variant);
                    if let Some(sindr::names::SpecialEnumVariantLowering::Boolean(value)) = variant.special_variant {
                        if !variant.payload.is_empty() {
                            return Err(TypeError {
                                structured: None,
                                message: format!(
                                    "Builtin Boolean variant {} unexpectedly requires payload",
                                    id.name
                                ),
                                span: span.clone(),
                                hint: None,
                            });
                        }
                        return Ok(TypedNode {
                            ty: Ty::Bool,
                            span: span.clone(),
                            node: TypedInner::Lit(Lit::Bool(value)),
                        });
                    }
                    if matches!(
                        Self::surface_name(&variant.enum_name),
                        "StopReply" | "StopReason"
                    ) && !self.stop_constructor_allowed()
                    {
                        return Err(self.stop_constructor_error(span, &variant.enum_name));
                    }
                    if !variant.payload.is_empty() {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Enum constructor {} expects {} argument(s)",
                                id.name,
                                variant.payload.len()
                            ),
                            span: span.clone(),
                            hint: Some("Call it as `Enum::Variant(...)`".into()),
                        });
                    }
                    let idx_node = TypedNode {
                        ty: Ty::Int,
                        span: span.clone(),
                        node: TypedInner::Lit(Lit::Int(variant.discriminant.clone())),
                    };
                    return Ok(TypedNode {
                        ty: self.resolve_ty(&variant.enum_ty),
                        span: span.clone(),
                        node: TypedInner::ConstructorCall(variant.tag, vec![idx_node]),
                    });
                }

                if let Some(index) = Self::parse_tuple_index_name(id.name.as_str()) {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Standalone tuple root _{} is not allowed; use tuple access with ._{}",
                            index, index
                        ),
                        span: span.clone(),
                        hint: Some("Tuple elements are accessed as value._0, value._1, ...".into()),
                    });
                }

                Err(TypeError {
                    structured: None,
                    message: format!("Undefined variable: {}", id.name),
                    span: span.clone(),
                    hint: None,
                })
            }

            Resolved::ReturnTypeArgumentApply(span, target, args) => {
                self.check_explicit_return_type_argument_apply(
                    span,
                    target,
                    &args
                        .iter()
                        .map(|argument| argument.ty.syntax.clone())
                        .collect::<Vec<_>>(),
                )
            }

            Resolved::Bind(span, pat, rhs) => {
                if !Self::is_total_bind_pattern(pat) {
                    return Err(self
                        .pattern_error(
                            TypeDiagnosticReason::NonTotalBindingPattern,
                            diagnostics::PatternKind::Other,
                            Some("=".into()),
                            None,
                            None,
                            None,
                            None,
                            Vec::new(),
                            span,
                        )
                        .with_hint(
                            "Use `=?` for partial destructuring and extractor-driven matches.",
                        ));
                }
                let inherited_error_kind = self.resolved_concrete_error_identity(rhs).map(str::to_string);
                if let ResolvedPattern::Annotated(_, ast_ty) = pat {
                    let annotation_kind = self.concrete_error_annotation_identity(ast_ty);
                    if let Some(kind) = inherited_error_kind.as_deref() {
                        if annotation_kind.as_deref() != Some(kind) {
                            return Err(TypeError::new("A narrowed Error local annotation must match its concrete declaration", Self::ast_ty_span(ast_ty).clone()));
                        }
                    } else if annotation_kind.is_some() {
                        return Err(TypeError::new("A common Error cannot be cast by a concrete Error annotation", Self::ast_ty_span(ast_ty).clone()));
                    }
                }
                let typed_rhs = if let ResolvedPattern::Annotated(_, ast_ty) = pat {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, if inherited_error_kind.is_some() {
                            TypeSyntaxContext::ConcreteErrorLocal
                        } else { self.local_type_syntax_context() })?;
                    let relation = ExpectedTypeRelation::annotation(Self::ast_ty_span(ast_ty));
                    let mut typed_rhs = self.check_node_with_expected_relation(
                        rhs,
                        Some(&expected),
                        Some(&relation),
                    )?;
                    self.apply_facet_annotation(&mut typed_rhs, &expected, span)?;
                    let relation = self.assert_type_relation(&expected, &typed_rhs.ty,
                        self.type_fact(SourceRole::Annotation, Self::ast_ty_span(ast_ty), &expected), self.type_fact(SourceRole::Value, &typed_rhs.span, &typed_rhs.ty),
                        TypeDiagnosticReason::AnnotationTypeMismatch, DiagnosticOrigin::Annotation, "binding", 0);
                    if let Err(error) = relation {
                        if error.reason() != Some(TypeDiagnosticReason::AnnotationTypeMismatch) {
                            return Err(error);
                        }
                        let constructor_coercion = self
                            .constructor_trait_key_for_ast_ty(ast_ty)
                            .map(|trait_key| {
                                self.constructor_annotation_compatible(
                                    &trait_key,
                                    &expected,
                                    &typed_rhs.ty,
                                )
                            }).transpose().map_err(|error| error.at_span(Self::ast_ty_span(ast_ty)))?.unwrap_or(false);
                        if constructor_coercion {
                            typed_rhs.ty = expected.clone();
                        } else {
                            return Err(error);
                        }
                    }
                    typed_rhs
                } else {
                    self.check_node(rhs)?
                };
                let typed_rhs = *self.concretize_pending_trait_calls(typed_rhs).map_err(|error| *error)?;
                if self.current_function_symbol.is_none() {
                    let can_instantiate = match self.resolve_ty(&typed_rhs.ty) {
                        Ty::Func(inputs, _) => self.local_callable_obligations_depend_on(&typed_rhs, &inputs),
                        _ => false,
                    };
                    if !can_instantiate {
                    if let Some((trait_name, method_name, subject, pending_span)) = self.first_pending_trait_helper(&typed_rhs) {
                        return Err(self.pending_trait_helper_error(trait_name, method_name, subject, pending_span));
                    }
                    }
                }
                self.require_concrete_callable_binding(&typed_rhs.ty, &typed_rhs.span).map_err(|error| self.lazy_unknown_binding_error(error, rhs))?;
                self.ensure_no_match_result_value(&typed_rhs.ty, &typed_rhs.span)?;
                let facet_path = if matches!(typed_rhs.ty, Ty::Facet(..)) {
                    Some(self.stored_facet_path_from_node(typed_rhs.clone(), span)?)
                } else {
                    None
                };
                let inherited_constructor_provenance =
                    self.constructor_capability_for_node(&typed_rhs);
                if let (Some(kind), ResolvedPattern::Annotated(binding, _)) = (&inherited_error_kind, pat) {
                    self.env.concrete_error_bindings.insert(binding.unique_id, kind.clone());
                }
                let (typed_pat, pat_ty) = self.check_pattern(pat, &typed_rhs.ty, span)?;
                self.ensure_self_rebinding_types(&typed_pat, span)?;

                self.bind_typed_pattern(&typed_pat, &self.resolve_ty(&pat_ty));
                if let Some(kind) = inherited_error_kind {
                    self.bind_local_error_identity(&typed_pat, &kind);
                }
                let binding_constructor_provenance = match pat {
                    ResolvedPattern::Annotated(_, ast_ty) => self
                        .constructor_trait_key_for_ast_ty(ast_ty)
                        .map(ConstructorCapabilityProvenance::constrained)
                        .unwrap_or(inherited_constructor_provenance),
                    _ => inherited_constructor_provenance,
                };
                self.bind_constructor_provenance(
                    &typed_pat,
                    (binding_constructor_provenance, typed_rhs.ty.clone()),
                );
                if let Some(path) = &facet_path {
                    self.bind_facet_pattern_bindings(&typed_pat, path, span)?;
                } else {
                    self.clear_facet_pattern_bindings(&typed_pat);
                }
                self.bind_lazy_capture_diagnostic(&typed_pat, rhs, &typed_rhs);
                self.normalize_env_bindings();

                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Bind(typed_pat, Box::new(typed_rhs)),
                })
            }

            Resolved::ApplyPattern(span, value, pattern) => self.check_apply_pattern(span, value, pattern, None),
            Resolved::SafeBind(span, pat, rhs, operator_span) => self.check_safebind(span, operator_span, pat, rhs, false),
            Resolved::StatementQuestion(span, rhs) => {
                // Spire ends this statement span at the trailing ASCII `?` token.
                let operator_span = Span { start: span.end - 1, end: span.end };
                self.check_safebind(
                    span,
                    &operator_span,
                    &ResolvedPattern::Wildcard(span.clone()),
                    rhs,
                    true,
                )
            },

            Resolved::BinOp(span, op, left, right) => self.check_binop(span, op, left, right),
            Resolved::Pipe(span, left, right) => self.check_pipe(span, left, right),
            Resolved::ContextMap(span, left, right) => self.check_context_map(span, left, right),
            Resolved::ContextApply(span, left, right) => {
                self.check_context_apply(span, left, right)
            }
            Resolved::ContextBind(span, left, right) => self.check_context_bind(span, left, right),
            Resolved::Compose(span, left, right) => self.check_compose(span, left, right),
            Resolved::LiftedCompose(span, left, right) => {
                self.check_lifted_compose(span, left, right)
            }
            Resolved::KleisliCompose(span, left, right) => {
                self.check_kleisli_compose(span, left, right)
            }

            Resolved::ListNil(span) => self.check_list_nil(span),
            Resolved::ListCons(span, head, tail) => self.check_list_cons(span, head, tail),
            Resolved::ListLiteral(span, elems) => self.check_list_literal(span, elems),
            Resolved::HashMapLiteral(span, entries) => self.check_hash_map_literal(span, entries),
            Resolved::RangeLiteral(span, start, stop) => {
                self.check_range_literal(span, start, stop)
            }
            Resolved::TupleLiteral(span, elems) => self.check_tuple_literal(span, elems),
            Resolved::Grouped(span, inner) => {
                let mut typed = self.check_node(inner)?;
                self.propagate_safe_operator_result(&typed.span, span);
                typed.span = span.clone();
                Ok(typed)
            }

            Resolved::InterpolatedStr(span, parts) => self.check_interpolated_str(span, parts),
            Resolved::Dbg(span, args) => self.check_dbg(span, args),

            Resolved::Cond(span, clauses) => self.check_cond(span, clauses, None),
            Resolved::If(span, cond, then, else_opt) => self.check_if(span, cond, then, else_opt),
            Resolved::Cause(span, value, err) => self.check_cause(span, value, err),
            Resolved::IfLet(span, scrutinee, arms, then_only) => {
                self.check_if_let(span, scrutinee, arms, None, *then_only)
            }

            Resolved::FieldAccess(span, expr, field) => self.check_field_access(span, expr, field),
            Resolved::FacetSegmentAccess(span, expr, segment) => {
                self.check_facet_segment_access_with_expected(span, expr, segment, None)
            }
            Resolved::InferredFacetCapture(span, segments) => Err(TypeError {
                structured: None,
                message: format!(
                    "{} requires expected unary function context",
                    Self::inferred_facet_capture_display(segments)
                ),
                span: span.clone(),
                hint: Some(
                    "Use inferred capture where a unary function is expected, or use an explicit capture such as `&Type.field`."
                        .into(),
                ),
            }),
            Resolved::FacetCapture(span, _) => Err(TypeError {
                structured: None,
                message: "`~source.path` is Facet API shorthand and must be consumed as the first argument of Facet::view/preview/put/set/over/over_result".into(),
                span: span.clone(),
                hint: Some(
                    "Use the shorthand only inside a Facet API call such as `Facet::set(~user.name, value)`."
                        .into(),
                ),
            }),
            Resolved::ProcessContextHandler(span, slot) => {
                self.check_process_context_handler(span, slot)
            }

            Resolved::Semi(span, inner) => {
                let typed_inner = self.check_node(inner)?;
                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Semi(Box::new(typed_inner)),
                })
            }

            Resolved::StructDef(span, id, type_params, fields, _) => {
                self.check_struct_def(span, id, type_params, fields)
            }
            Resolved::RecordDef(span, id, fields, _) => self.check_record_def(span, id, fields),
            Resolved::EnumDef(span, id, type_params, variants, attrs) => {
                self.check_enum_def(span, id, type_params, variants, attrs)
            }
            Resolved::StructLit(span, id, field_vals) => {
                self.check_struct_lit(span, id, field_vals)
            }
            Resolved::ConstructorCall(span, id, args) => {
                self.check_constructor_call(span, id, args, None)
            }
            Resolved::EnumConstructorCall(span, id, type_args, args) => {
                self.check_explicit_enum_constructor_call(span, id, type_args, args, None)
            }
            Resolved::DeferrorDef(span, id, fields, show_expr) => {
                self.check_deferror_def(span, id, fields, show_expr)
            }
            Resolved::Def(span, id, return_type_arguments, params, ret_ty, where_clause, body, attrs) => {
                self.check_def(
                    span,
                    id,
                    return_type_arguments,
                    params,
                    ret_ty,
                    where_clause.as_ref(),
                    body,
                    attrs,
                )
            }
            Resolved::ConstDef(span, _id, _ast_ty, _value, _) => Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Unit),
            }),
            Resolved::ExtractorDef(span, id, type_params, param, ret_ty, body, attrs) => {
                self.check_extractor_def(span, id, type_params, param, ret_ty, body, attrs)
            }
            Resolved::TraitDef(span, id, _, where_clause, methods, _) => {
                let trait_key = self.trait_key(id);
                let trait_info = self.traits.get(&trait_key).cloned().ok_or_else(|| TypeError {
                    structured: None,
                    message: format!("Unknown trait: {}", id.name),
                    span: span.clone(),
                    hint: None,
                })?;
                let deferred_self = self.env.fresh_tyvar();
                if let Ty::Var(var) = deferred_self { self.register_tyvar_bound(var, &trait_key); }
                let mut typed_methods = Vec::with_capacity(methods.len());
                for method in methods {
                    let method_info = trait_info.methods.get(&method.id.name).ok_or_else(|| {
                        TypeError {
                            structured: None,
                            message: format!("Unknown trait method: {}::{}", id.name, method.id.name),
                            span: method.span.clone(),
                            hint: None,
                        }
                    })?;
                    let (params, mut ret_ty, _, _, _) = self.resolve_trait_method_signature(
                        &trait_info,
                        method_info,
                        &deferred_self,
                    )?;
                    if let Some(body) = &method.body {
                        // Default bodies need an application witness for abstract
                        // Self, just as generic constructor parameters do.
                        let (body_self, slot_vars) = if trait_info.constructor_slots.is_empty() {
                            (deferred_self.clone(), Vec::new())
                        } else {
                            let witness = self.env.fresh_tyvar();
                            let Ty::Var(variable) = witness else { unreachable!() };
                            self.register_tyvar_bound(variable, &trait_key);
                            self.constructor_witness_traits.insert(variable, trait_key.clone());
                            let mut application = vec![Ty::Hole, witness];
                            let mut slots = Vec::new();
                            for _ in &trait_info.constructor_slots {
                                let slot = self.env.fresh_tyvar();
                                let Ty::Var(variable) = slot else { unreachable!() };
                                slots.push(variable);
                                application.push(slot);
                            }
                            (Ty::SelfApp(application), slots)
                        };
                        let (body_params, body_ret) = if trait_info.constructor_slots.is_empty() {
                            (params.clone(), ret_ty.clone())
                        } else {
                            let (body_params, body_ret, _, _, _) = self.resolve_trait_method_signature(
                                &trait_info,
                                method_info,
                                &body_self,
                            )?;
                            let body_params = body_params.into_iter()
                                .map(|ty| self.expand_trait_self_apps(ty, &body_self, &slot_vars))
                                .collect::<Result<Vec<_>, _>>()?;
                            let body_ret = self.expand_trait_self_apps(body_ret, &body_self, &slot_vars)?;
                            (body_params, body_ret)
                        };
                        let mut tyvars = HashMap::from([("Self".into(), body_self)]);
                        for (param, resolved) in method.value_parameters.iter().zip(body_params.iter()) {
                            self.collect_signature_ty_bindings(
                                &param.ty,
                                resolved,
                                &mut tyvars,
                            );
                        }
                        self.collect_signature_ty_bindings(
                            &method.ret_ty,
                            &body_ret,
                            &mut tyvars,
                        );
                        self.seed_missing_method_type_params(&method.type_params, &mut tyvars);
                        let declaration_capabilities = self.resolved_capability_uses(
                            method.where_clause.as_ref(),
                            &tyvars,
                        )?;
                        let local_bindings = method
                            .value_parameters
                            .iter()
                            .zip(body_params.iter())
                            .map(|(param, ty)| (param.id.unique_id, ty.clone()))
                            .collect::<Vec<_>>();
                        let rigid_tyvars = Self::signature_tyvar_ids(&tyvars);
                        let typed_body = self.check_body_in_isolated_scope(
                            &local_bindings,
                            &[],
                            &declaration_capabilities,
                            &mut [],
                            tyvars.clone(),
                            rigid_tyvars.clone(),
                            body_ret.clone(),
                            Self::ast_ty_span(method.ret_ty.syntax()),
                            Some(Self::ast_ty_span(method.ret_ty.syntax())),
                            method.id.name.clone(),
                            None,
                            None,
                            false,
                            body,
                        )?;
                        if let Some(concrete) = self.resolve_contextual_return_body(
                            &method.ret_ty,
                            &body_ret,
                            &typed_body,
                            &rigid_tyvars,
                        )? {
                            ret_ty = concrete;
                        }
                    }
                    typed_methods.push(TypedTraitMethodInfo {
                        name: method.id.name.clone(),
                        params,
                        ret_ty,
                        where_clause: method
                            .where_clause
                            .as_ref()
                            .map(TypedWhereClause::from),
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::TraitDef(
                        trait_key,
                        where_clause.as_ref().map(TypedWhereClause::from),
                        typed_methods,
                    ),
                })
            }
            Resolved::TraitImplDef(
                span,
                _,
                trait_id,
                trait_args,
                target_ty,
                where_clause,
                methods,
            ) => {
                self.check_trait_impl_def(
                    span,
                    trait_id,
                    trait_args,
                    target_ty,
                    where_clause.as_ref(),
                    methods,
                )
            }
            Resolved::BuiltinDecl(
                span,
                id,
                return_type_arguments,
                params,
                ret_ty,
                where_clause,
                _,
            ) => {
                self.check_builtin_decl(
                    span,
                    id,
                    return_type_arguments,
                    params,
                    ret_ty,
                    where_clause.as_ref(),
                )
            }
            Resolved::BuiltinExtractorDecl(span, id, param, ret_ty, _) => {
                self.check_builtin_extractor_decl(span, id, param, ret_ty)
            }
            Resolved::BuiltinTypeDecl(span, id, params, attrs) => {
                self.check_builtin_type_decl(span, id, params, attrs)
            }
            Resolved::TypeAlias(span, _, _, _, _) => Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Unit),
            }),

            Resolved::Capture(span, target, args) => self.check_capture(span, target, args, None),
            Resolved::Block(..)
            | Resolved::ExtractorClosure(..)
            | Resolved::Closure(..)
            | Resolved::CaptureClosure(..)
            | Resolved::Do(..)
            | Resolved::App(..)
            | Resolved::LazyApp(..)
            | Resolved::Match(..)
            | Resolved::IsMatch(..) => unreachable!("structural expressions are dispatched by check_node"),
        }
    }

    pub(super) fn check_node_with_expected(
        &mut self,
        node: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let relation = expected.map(|_| ExpectedTypeRelation::contextual(self.resolved_span(node)));
        self.check_node_with_expected_relation(node, expected, relation.as_ref())
    }

    pub(super) fn check_node_with_return_expected(
        &mut self,
        node: &Resolved,
        expected: &Ty,
        expected_span: &Span,
    ) -> Result<TypedNode, TypeError> {
        let relation = ExpectedTypeRelation::function_return(expected_span);
        self.check_node_with_expected_relation(node, Some(expected), Some(&relation))
    }

    // Recursive structural nodes must not retain the temporary frame used by
    // contextual collection inference and the other individual expressions.
    pub(super) fn check_node_with_expected_relation(
        &mut self,
        node: &Resolved,
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        match (node, expected) {
            (Resolved::Block(span, stmts), expected) => {
                self.check_block(span, stmts, expected, expected_relation)
            }
            (Resolved::ExtractorClosure(span, params, captures, body), Some(expected_ty)) => self
                .check_extractor_closure(
                    span,
                    params,
                    captures,
                    body,
                    Some(expected_ty),
                    expected_relation,
                ),
            (Resolved::Closure(span, params, captures, body), Some(expected_ty)) => self
                .check_literal_closure_with_expected_relation(
                    span,
                    params,
                    captures,
                    body,
                    expected_ty,
                    expected_relation,
                ),
            (Resolved::CaptureClosure(span, params, captures, body), Some(expected_ty)) => self
                .check_capture_closure_with_expected_type(
                    span,
                    params,
                    captures,
                    body,
                    expected_ty,
                ),
            (
                Resolved::Do(span, intrinsic, contract, return_type_arguments, statements),
                expected,
            ) => self.check_do(
                span,
                *intrinsic,
                contract,
                return_type_arguments,
                statements,
                expected,
                expected_relation,
            ),
            (Resolved::LazyApp(span, kind, func, args), Some(expected_ty)) => {
                self.check_lazy_app(span, *kind, func, args, Some(expected_ty))
            }
            (Resolved::App(span, func, args), Some(expected_ty)) => {
                self.check_application_with_expected_type(span, func, args, expected_ty)
            }
            (Resolved::FieldAccess(span, expr, field), expected_ty) => {
                self.check_field_access_with_expected(span, expr, field, expected_ty)
            }
            (Resolved::FacetSegmentAccess(span, expr, segment), expected_ty) => {
                self.check_facet_segment_access_with_expected(span, expr, segment, expected_ty)
            }
            (Resolved::Grouped(span, inner), Some(expected_ty)) => {
                let mut typed = self.check_node_with_expected_relation(
                    inner,
                    Some(expected_ty),
                    expected_relation,
                )?;
                self.propagate_safe_operator_result(&typed.span, span);
                typed.span = span.clone();
                Ok(typed)
            }
            (
                Resolved::Match(span, scrutinee, arms) | Resolved::IsMatch(span, scrutinee, arms),
                Some(expected_ty),
            ) => {
                let branch_relation = expected_relation
                    .filter(|relation| relation.synthetic_do_match_pending)
                    .map(ExpectedTypeRelation::after_synthetic_do_match);
                self.check_match(
                    span,
                    scrutinee,
                    arms,
                    Some(expected_ty),
                    branch_relation.as_ref(),
                )
            }
            (Resolved::ProcessContextHandler(span, slot), _) => {
                self.check_process_context_handler(span, slot)
            }
            (_, None) => self.check_node(node),
            (_, Some(expected_ty)) => {
                self.check_non_structural_node_with_expected(node, expected_ty)
            }
        }
    }

    #[inline(never)]
    fn check_non_structural_node_with_expected(
        &mut self,
        node: &Resolved,
        expected_ty: &Ty,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::ApplyPattern(span, value, pattern) => {
                self.check_apply_pattern(span, value, pattern, Some(expected_ty))
            }
            Resolved::Capture(span, target, args) => {
                self.check_capture(span, target, args, Some(expected_ty))
            }
            Resolved::ListLiteral(span, elems) => {
                let expected_ty = self.resolve_ty(expected_ty);
                let Ty::List(element_ty) = expected_ty else {
                    return self.check_list_literal(span, elems);
                };
                if matches!(self.resolve_ty(element_ty.as_ref()), Ty::Var(_)) {
                    return self.check_list_literal(span, elems);
                }
                let typed_elems = elems
                    .iter()
                    .map(|elem| self.check_node_with_expected(elem, Some(element_ty.as_ref())))
                    .collect::<Result<Vec<_>, _>>()?;
                for (ordinal, typed) in typed_elems.iter().enumerate() {
                    self.ensure_no_runtime_facet_value(typed, "List literal")?;
                    self.assert_type_relation(
                        element_ty.as_ref(),
                        &typed.ty,
                        self.type_fact(SourceRole::Expected, span, element_ty.as_ref()),
                        self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::Call,
                        "list element",
                        ordinal as u32,
                    )?;
                }
                Ok(TypedNode {
                    ty: Ty::List(element_ty),
                    span: span.clone(),
                    node: TypedInner::ListLiteral(typed_elems),
                })
            }
            Resolved::TupleLiteral(span, elems) => {
                let expected_ty = self.resolve_ty(expected_ty);
                let Ty::Tuple(item_tys) = expected_ty else {
                    return self.check_tuple_literal(span, elems);
                };
                if elems.len() != item_tys.len() {
                    return self.check_tuple_literal(span, elems);
                }
                let typed_elems = elems
                    .iter()
                    .zip(item_tys.iter())
                    .map(|(elem, expected)| self.check_node_with_expected(elem, Some(expected)))
                    .collect::<Result<Vec<_>, _>>()?;
                for (ordinal, (expected, typed)) in item_tys.iter().zip(&typed_elems).enumerate() {
                    self.ensure_no_runtime_facet_value(typed, "Tuple literal")?;
                    self.assert_type_relation(
                        expected,
                        &typed.ty,
                        self.type_fact(SourceRole::Expected, span, expected),
                        self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::Call,
                        "tuple element",
                        ordinal as u32,
                    )?;
                }
                Ok(TypedNode {
                    ty: Ty::Tuple(item_tys),
                    span: span.clone(),
                    node: TypedInner::TupleLiteral(typed_elems),
                })
            }
            Resolved::InferredFacetCapture(span, segments) => {
                self.check_inferred_facet_capture(span, segments, expected_ty)
            }
            Resolved::Pipe(span, left, right) => {
                self.check_pipe_with_expected(span, left, right, Some(expected_ty))
            }
            Resolved::ContextMap(span, left, right) => {
                self.check_context_map_with_expected(span, left, right, Some(expected_ty))
            }
            Resolved::ContextApply(span, left, right) => {
                self.check_context_apply_with_expected(span, left, right, Some(expected_ty))
            }
            Resolved::ContextBind(span, left, right) => {
                self.check_context_bind_with_expected(span, left, right, Some(expected_ty))
            }
            Resolved::Cond(span, clauses) => self.check_cond(span, clauses, Some(expected_ty)),
            Resolved::If(span, cond, then, else_opt) => {
                self.check_if_with_expected(span, cond, then, else_opt, expected_ty)
            }
            Resolved::IfLet(span, scrutinee, arms, then_only) => {
                self.check_if_let(span, scrutinee, arms, Some(expected_ty), *then_only)
            }
            Resolved::BinOp(span, op, left, right) => {
                self.check_binop_with_expected(span, op, left, right, Some(expected_ty))
            }
            Resolved::Compose(span, left, right) => {
                self.check_compose_with_expected(span, left, right, Some(expected_ty))
            }
            Resolved::LiftedCompose(span, left, right) => {
                self.check_lifted_compose_with_expected(span, left, right, Some(expected_ty))
            }
            Resolved::KleisliCompose(span, left, right) => {
                self.check_kleisli_compose_with_expected(span, left, right, Some(expected_ty))
            }
            Resolved::ConstructorCall(span, id, args) => {
                self.check_constructor_call(span, id, args, Some(expected_ty))
            }
            Resolved::EnumConstructorCall(span, id, type_args, args) => self
                .check_explicit_enum_constructor_call(span, id, type_args, args, Some(expected_ty)),
            _ => {
                let typed = self.check_node(node)?;
                Ok(typed)
            }
        }
    }
}
