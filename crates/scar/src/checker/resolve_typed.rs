use super::*;

impl Checker {
    fn resolve_typed_facet_path(&self, path: TypedFacetPath) -> TypedFacetPath {
        TypedFacetPath {
            source_ty: self.resolve_ty(&path.source_ty),
            focus_ty: self.resolve_ty(&path.focus_ty),
            segment_focus_tys: path
                .segment_focus_tys
                .iter()
                .map(|ty| self.resolve_ty(ty))
                .collect(),
            update_source_ty: self.resolve_ty(&path.update_source_ty),
            update_focus_ty: self.resolve_ty(&path.update_focus_ty),
            path_kind: path.path_kind,
            may_fail: path.may_fail,
            segments: path
                .segments
                .into_iter()
                .map(|segment| self.resolve_typed_facet_segment(segment))
                .collect(),
        }
    }

    fn resolve_typed_facet_segment(&self, segment: TypedFacetSegment) -> TypedFacetSegment {
        match segment {
            TypedFacetSegment::ListIndex {
                index,
                display,
                literal_index,
            } => TypedFacetSegment::ListIndex {
                index: self.resolve_typed_node(*index),
                display,
                literal_index,
            },
            TypedFacetSegment::ListRange {
                start,
                end,
                display,
                literal_start,
                literal_end,
            } => TypedFacetSegment::ListRange {
                start: self.resolve_typed_node(*start),
                end: self.resolve_typed_node(*end),
                display,
                literal_start,
                literal_end,
            },
            TypedFacetSegment::MapKey {
                key,
                display,
                literal_key,
            } => TypedFacetSegment::MapKey {
                key: self.resolve_typed_node(*key),
                display,
                literal_key,
            },
            other => other,
        }
    }

    fn resolve_pending_facet_path(&self, path: PendingFacetPath) -> PendingFacetPath {
        PendingFacetPath {
            root_path_name: path.root_path_name,
            source_ty_hint: path.source_ty_hint.map(|ty| self.resolve_ty(&ty)),
            segments: path
                .segments
                .into_iter()
                .map(|segment| self.resolve_pending_facet_segment(segment))
                .collect(),
        }
    }

    fn resolve_pending_facet_segment(&self, segment: PendingFacetSegment) -> PendingFacetSegment {
        let resolve_expr = |expr| match expr {
            PendingFacetExpr::Resolved(expr) => PendingFacetExpr::Resolved(expr),
            PendingFacetExpr::Typed(expr) => {
                PendingFacetExpr::Typed(self.resolve_typed_node(*expr))
            }
        };
        match segment {
            PendingFacetSegment::Bracket { expr, display } => PendingFacetSegment::Bracket {
                expr: resolve_expr(expr),
                display,
            },
            PendingFacetSegment::RangeBracket {
                start,
                end,
                display,
            } => PendingFacetSegment::RangeBracket {
                start: resolve_expr(start),
                end: resolve_expr(end),
                display,
            },
            other => other,
        }
    }

    fn resolve_trait_call_origin(&self, origin: TraitCallOrigin) -> TraitCallOrigin {
        match origin {
            TraitCallOrigin::Explicit => TraitCallOrigin::Explicit,
            TraitCallOrigin::Operator { op, lhs_ty, rhs_ty } => TraitCallOrigin::Operator {
                op,
                lhs_ty: self.resolve_ty(&lhs_ty),
                rhs_ty: self.resolve_ty(&rhs_ty),
            },
            TraitCallOrigin::Comparison { op, lhs_ty, rhs_ty } => TraitCallOrigin::Comparison {
                op,
                lhs_ty: self.resolve_ty(&lhs_ty),
                rhs_ty: self.resolve_ty(&rhs_ty),
            },
        }
    }

    // Keep recursive results off the visitor's stack frame. The returned box
    // is used directly by child fields that already store boxed nodes.
    pub(super) fn resolve_typed_node(&self, node: TypedNode) -> Box<TypedNode> {
        let span = node.span.clone();
        let ty = self.resolve_ty(&node.ty);
        let node = match node.node {
            TypedInner::Lit(lit) => TypedInner::Lit(lit),
            TypedInner::ErrorKind(kind) => TypedInner::ErrorKind(kind),
            TypedInner::Var(id) => TypedInner::Var(id),
            TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init,
            } => TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init: self.resolve_typed_node(*init),
            },
            TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid,
            } => TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: self.resolve_typed_node(*pid),
            },
            TypedInner::SupervisorStatus { supervisor_process } => {
                TypedInner::SupervisorStatus { supervisor_process }
            }
            TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init,
                strategy,
            } => TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init: self.resolve_typed_node(*init),
                strategy: self.resolve_typed_node(*strategy),
            },
            TypedInner::App(func, args) => TypedInner::App(
                self.resolve_typed_node(*func),
                args.into_iter()
                    .map(|arg| *self.resolve_typed_node(arg))
                    .collect(),
            ),
            TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty,
                obligation,
                dispatch,
                origin,
                args,
            } => TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty: self.resolve_ty(&receiver_ty),
                obligation: TraitObligation {
                    trait_id: obligation.trait_id,
                    trait_args: obligation
                        .trait_args
                        .iter()
                        .map(|arg| self.resolve_ty(arg))
                        .collect(),
                    receiver: self.resolve_ty(&obligation.receiver),
                },
                dispatch,
                origin: self.resolve_trait_call_origin(origin),
                args: args
                    .into_iter()
                    .map(|arg| *self.resolve_typed_node(arg))
                    .collect(),
            },
            TypedInner::InjectCall(func, args) => TypedInner::InjectCall(
                self.resolve_typed_node(*func),
                args.into_iter()
                    .map(|arg| *self.resolve_typed_node(arg))
                    .collect(),
            ),
            TypedInner::Block(stmts) => TypedInner::Block(
                stmts
                    .into_iter()
                    .map(|stmt| *self.resolve_typed_node(stmt))
                    .collect(),
            ),
            TypedInner::Bind(pattern, rhs) => TypedInner::Bind(
                self.resolve_typed_pattern(pattern),
                self.resolve_typed_node(*rhs),
            ),
            TypedInner::ApplyPattern {
                value,
                pattern,
                projections,
            } => TypedInner::ApplyPattern {
                value: self.resolve_typed_node(*value),
                pattern: self.resolve_typed_pattern(pattern),
                projections: projections
                    .into_iter()
                    .map(|(id, ty)| (id, self.resolve_ty(&ty)))
                    .collect(),
            },
            TypedInner::SafeBind(pattern, rhs, projection, failure_target) => TypedInner::SafeBind(
                self.resolve_typed_pattern(pattern),
                self.resolve_typed_node(*rhs),
                match projection {
                    SafeBindRhsProjection::CanonicalResultOnce {
                        payload_ty,
                        error_ty,
                    } => SafeBindRhsProjection::CanonicalResultOnce {
                        payload_ty: self.resolve_ty(&payload_ty),
                        error_ty: self.resolve_ty(&error_ty),
                    },
                    SafeBindRhsProjection::PatternInput { pattern_input_ty } => {
                        SafeBindRhsProjection::PatternInput {
                            pattern_input_ty: self.resolve_ty(&pattern_input_ty),
                        }
                    }
                },
                match failure_target {
                    SafeBindFailureTarget::EnclosingMonadFail(mut target) => {
                        target.carrier_ty = self.resolve_ty(&target.carrier_ty);
                        target.call = self.resolve_typed_node(*target.call);
                        SafeBindFailureTarget::EnclosingMonadFail(target)
                    }
                    SafeBindFailureTarget::EnclosingMatchResultContext { err_tag } => {
                        SafeBindFailureTarget::EnclosingMatchResultContext { err_tag }
                    }
                    SafeBindFailureTarget::TopLevel => SafeBindFailureTarget::TopLevel,
                    SafeBindFailureTarget::DoMonadFail(mut target) => {
                        target.carrier_ty = self.resolve_ty(&target.carrier_ty);
                        target.call = self.resolve_typed_node(*target.call);
                        SafeBindFailureTarget::DoMonadFail(target)
                    }
                    SafeBindFailureTarget::DoAlternative { empty } => {
                        SafeBindFailureTarget::DoAlternative {
                            empty: self.resolve_typed_node(*empty),
                        }
                    }
                    SafeBindFailureTarget::Deferred(mut deferred) => {
                        deferred.carrier_ty = self.resolve_ty(&deferred.carrier_ty);
                        deferred.propagated_error_tys = deferred
                            .propagated_error_tys
                            .iter()
                            .map(|ty| self.resolve_ty(ty))
                            .collect();
                        SafeBindFailureTarget::Deferred(deferred)
                    }
                },
            ),
            TypedInner::DoSafeBind(control) => {
                let TypedDoSafeBind {
                    pattern,
                    rhs,
                    projection,
                    failure_target,
                    continuation,
                    origins,
                } = *control;
                TypedInner::DoSafeBind(Box::new(TypedDoSafeBind {
                    pattern: self.resolve_typed_pattern(pattern),
                    rhs: self.resolve_typed_node(*rhs),
                    projection: match projection {
                        SafeBindRhsProjection::CanonicalResultOnce {
                            payload_ty,
                            error_ty,
                        } => SafeBindRhsProjection::CanonicalResultOnce {
                            payload_ty: self.resolve_ty(&payload_ty),
                            error_ty: self.resolve_ty(&error_ty),
                        },
                        SafeBindRhsProjection::PatternInput { pattern_input_ty } => {
                            SafeBindRhsProjection::PatternInput {
                                pattern_input_ty: self.resolve_ty(&pattern_input_ty),
                            }
                        }
                    },
                    failure_target: match failure_target {
                        SafeBindFailureTarget::DoMonadFail(mut target) => {
                            target.carrier_ty = self.resolve_ty(&target.carrier_ty);
                            target.call = self.resolve_typed_node(*target.call);
                            SafeBindFailureTarget::DoMonadFail(target)
                        }
                        SafeBindFailureTarget::DoAlternative { empty } => {
                            SafeBindFailureTarget::DoAlternative {
                                empty: self.resolve_typed_node(*empty),
                            }
                        }
                        SafeBindFailureTarget::Deferred(mut deferred) => {
                            deferred.carrier_ty = self.resolve_ty(&deferred.carrier_ty);
                            deferred.propagated_error_tys = deferred
                                .propagated_error_tys
                                .iter()
                                .map(|ty| self.resolve_ty(ty))
                                .collect();
                            SafeBindFailureTarget::Deferred(deferred)
                        }
                        other => other,
                    },
                    continuation: self.resolve_typed_node(*continuation),
                    origins,
                }))
            }
            TypedInner::BinOp(op, left, right) => TypedInner::BinOp(
                op,
                self.resolve_typed_node(*left),
                self.resolve_typed_node(*right),
            ),
            TypedInner::Pipe(left, right) => TypedInner::Pipe(
                self.resolve_typed_node(*left),
                self.resolve_typed_node(*right),
            ),
            TypedInner::Compose(flavor, left, right) => TypedInner::Compose(
                flavor,
                self.resolve_typed_node(*left),
                self.resolve_typed_node(*right),
            ),
            TypedInner::ListNil => TypedInner::ListNil,
            TypedInner::ListCons(head, tail) => TypedInner::ListCons(
                self.resolve_typed_node(*head),
                self.resolve_typed_node(*tail),
            ),
            TypedInner::ListLiteral(elems) => TypedInner::ListLiteral(
                elems
                    .into_iter()
                    .map(|elem| *self.resolve_typed_node(elem))
                    .collect(),
            ),
            TypedInner::HashMapLiteral(entries) => TypedInner::HashMapLiteral(
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            *self.resolve_typed_node(key),
                            *self.resolve_typed_node(value),
                        )
                    })
                    .collect(),
            ),
            TypedInner::ErrorConstruct {
                kind,
                message,
                payload,
                payload_fields,
            } => *self.resolve_error_construct(kind, message, payload, payload_fields),
            TypedInner::TupleLiteral(elems) => TypedInner::TupleLiteral(
                elems
                    .into_iter()
                    .map(|elem| *self.resolve_typed_node(elem))
                    .collect(),
            ),
            TypedInner::InterpolatedStr(parts) => TypedInner::InterpolatedStr(
                parts
                    .into_iter()
                    .map(|part| match part {
                        TypedInterpolatedPart::Text(text) => TypedInterpolatedPart::Text(text),
                        TypedInterpolatedPart::Expr(expr) => {
                            TypedInterpolatedPart::Expr(self.resolve_typed_node(*expr))
                        }
                    })
                    .collect(),
            ),
            TypedInner::Dbg(args) => TypedInner::Dbg(
                args.into_iter()
                    .map(|arg| TypedDbgArg {
                        span: arg.span,
                        ty_name: arg.ty_name,
                        expr: *self.resolve_typed_node(arg.expr),
                    })
                    .collect(),
            ),
            TypedInner::EagerBoundary(inner) => {
                TypedInner::EagerBoundary(self.resolve_typed_node(*inner))
            }
            TypedInner::If(cond, then, else_opt) => TypedInner::If(
                self.resolve_typed_node(*cond),
                self.resolve_typed_node(*then),
                else_opt.map(|node| self.resolve_typed_node(*node)),
            ),
            TypedInner::Cause(value, err) => TypedInner::Cause(
                self.resolve_typed_node(*value),
                self.resolve_typed_node(*err),
            ),
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                self.resolve_typed_node(*scrutinee),
                arms.into_iter()
                    .map(|arm| TypedMatchArm {
                        direct_expression: arm.direct_expression,
                        pattern: self.resolve_typed_match_pattern(arm.pattern),
                        guard: arm.guard.map(|guard| *self.resolve_typed_node(guard)),
                        body: *self.resolve_typed_node(arm.body),
                    })
                    .collect(),
            ),
            TypedInner::FieldAccess(expr, idx) => {
                TypedInner::FieldAccess(self.resolve_typed_node(*expr), idx)
            }
            TypedInner::ProcessContextHandler { process_name, slot } => {
                TypedInner::ProcessContextHandler { process_name, slot }
            }
            TypedInner::FacetPath(path) => {
                TypedInner::FacetPath(self.resolve_typed_facet_path(path))
            }
            TypedInner::PendingFacetPath(path) => {
                TypedInner::PendingFacetPath(self.resolve_pending_facet_path(path))
            }
            TypedInner::FacetView {
                api,
                source,
                path,
                source_is_result,
            } => TypedInner::FacetView {
                api,
                source: self.resolve_typed_node(*source),
                path: self.resolve_typed_facet_path(path),
                source_is_result,
            },
            TypedInner::FacetSet {
                api,
                source,
                path,
                value,
                source_is_result,
                mode,
            } => TypedInner::FacetSet {
                api,
                source: self.resolve_typed_node(*source),
                path: self.resolve_typed_facet_path(path),
                value: self.resolve_typed_node(*value),
                source_is_result,
                mode,
            },
            TypedInner::FacetOver {
                api,
                source,
                path,
                update_fun,
                source_is_result,
                mode,
            } => TypedInner::FacetOver {
                api,
                source: self.resolve_typed_node(*source),
                path: self.resolve_typed_facet_path(path),
                update_fun: self.resolve_typed_node(*update_fun),
                source_is_result,
                mode,
            },
            TypedInner::StructLit(tag, fields) => TypedInner::StructLit(
                tag,
                fields
                    .into_iter()
                    .map(|field| *self.resolve_typed_node(field))
                    .collect(),
            ),
            TypedInner::ConstructorCall(tag, fields) => TypedInner::ConstructorCall(
                tag,
                fields
                    .into_iter()
                    .map(|field| *self.resolve_typed_node(field))
                    .collect(),
            ),
            TypedInner::DeferrorDef(tag, binding, id, params, show) => TypedInner::DeferrorDef(
                tag,
                binding,
                id,
                params
                    .into_iter()
                    .map(|param| TypedValueParameter {
                        id: param.id,
                        mode: param.mode,
                        ty: self.resolve_ty(&param.ty),
                        span: param.span,
                    })
                    .collect(),
                self.resolve_typed_node(*show),
            ),
            TypedInner::Def(
                fun_idx,
                id,
                type_params,
                params,
                ret_ty,
                where_clause,
                body,
                visibility,
            ) => TypedInner::Def(
                fun_idx,
                id,
                type_params
                    .into_iter()
                    .map(|argument| TypedReturnTypeArgument {
                        ordinal: argument.ordinal,
                        ty: self.resolve_ty(&argument.ty),
                        span: argument.span,
                    })
                    .collect(),
                params
                    .into_iter()
                    .map(|param| TypedValueParameter {
                        id: param.id,
                        mode: param.mode,
                        ty: self.resolve_ty(&param.ty),
                        span: param.span,
                    })
                    .collect(),
                self.resolve_ty(&ret_ty),
                where_clause,
                self.resolve_typed_node(*body),
                visibility,
            ),
            TypedInner::ExtractorDef(fun_idx, id, type_params, param, ret_ty, body, visibility) => {
                TypedInner::ExtractorDef(
                    fun_idx,
                    id,
                    type_params
                        .into_iter()
                        .map(|param| TypedTypeParam {
                            name: param.name,
                            ty_var: param.ty_var,
                            bound: param.bound,
                        })
                        .collect(),
                    param
                        .into_iter()
                        .map(|param| TypedValueParameter {
                            id: param.id,
                            mode: param.mode,
                            ty: self.resolve_ty(&param.ty),
                            span: param.span,
                        })
                        .collect(),
                    self.resolve_ty(&ret_ty),
                    self.resolve_typed_node(*body),
                    visibility,
                )
            }
            TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty) => {
                TypedInner::BuiltinExtractorDecl(
                    id,
                    param_ty.iter().map(|ty| self.resolve_ty(ty)).collect(),
                    self.resolve_ty(&ret_ty),
                )
            }
            TypedInner::Closure(params, captures, body) => TypedInner::Closure(
                params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: self.resolve_ty(&param.ty),
                    })
                    .collect(),
                captures,
                self.resolve_typed_node(*body),
            ),
            TypedInner::ExtractorClosure(params, captures, body) => TypedInner::ExtractorClosure(
                params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: self.resolve_ty(&param.ty),
                    })
                    .collect(),
                captures,
                self.resolve_typed_node(*body),
            ),
            TypedInner::CaptureClosure(params, captures, body) => TypedInner::CaptureClosure(
                params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: self.resolve_ty(&param.ty),
                    })
                    .collect(),
                captures,
                self.resolve_typed_node(*body),
            ),
            TypedInner::CaptureConstructorClosure(id, params, captures, body) => {
                TypedInner::CaptureConstructorClosure(
                    id,
                    params
                        .into_iter()
                        .map(|param| TypedClosureParam {
                            id: param.id,
                            ty: self.resolve_ty(&param.ty),
                        })
                        .collect(),
                    captures,
                    self.resolve_typed_node(*body),
                )
            }
            TypedInner::Capture(target, args) => TypedInner::Capture(
                self.resolve_typed_node(*target),
                args.into_iter()
                    .map(|arg| *self.resolve_typed_node(arg))
                    .collect(),
            ),
            TypedInner::StructDef(tag, name, field_names, field_policies) => {
                TypedInner::StructDef(tag, name, field_names, field_policies)
            }
            TypedInner::RecordDef(tag, name, field_names, field_policies) => {
                TypedInner::RecordDef(tag, name, field_names, field_policies)
            }
            TypedInner::EnumDef(name, variants) => TypedInner::EnumDef(name, variants),
            TypedInner::TraitDef(name, where_clause, methods) => {
                TypedInner::TraitDef(name, where_clause, methods)
            }
            TypedInner::TraitImplDef(trait_name, target_name, where_clause) => {
                TypedInner::TraitImplDef(trait_name, target_name, where_clause)
            }
            TypedInner::Semi(inner) => TypedInner::Semi(self.resolve_typed_node(*inner)),
        };

        Box::new(TypedNode { ty, span, node })
    }

    // Keep the new construction arm's recursive collection temporaries out of
    // the common visitor frame, which also visits deeply nested do chains.
    fn resolve_error_construct(
        &self,
        kind: String,
        message: Box<TypedNode>,
        payload: Vec<TypedNode>,
        payload_fields: Vec<(String, Ty)>,
    ) -> Box<TypedInner> {
        Box::new(TypedInner::ErrorConstruct {
            kind,
            message: self.resolve_typed_node(*message),
            payload: payload
                .into_iter()
                .map(|value| *self.resolve_typed_node(value))
                .collect(),
            payload_fields: payload_fields
                .into_iter()
                .map(|(name, ty)| (name, self.resolve_ty(&ty)))
                .collect(),
        })
    }

    pub(super) fn resolve_typed_pattern(&self, pattern: TypedPattern) -> TypedPattern {
        match pattern {
            TypedPattern::Located(source, inner) => {
                TypedPattern::Located(source, Box::new(self.resolve_typed_pattern(*inner)))
            }
            TypedPattern::HashMap(ty, entries) => TypedPattern::HashMap(
                self.resolve_ty(&ty),
                entries
                    .into_iter()
                    .map(|entry| TypedHashMapPatternEntry {
                        key: *self.resolve_typed_node(entry.key),
                        pattern: self.resolve_typed_pattern(entry.pattern),
                        key_span: entry.key_span,
                    })
                    .collect(),
            ),
            TypedPattern::Var(ty, id) => TypedPattern::Var(self.resolve_ty(&ty), id),
            TypedPattern::Pin(ty, id, dispatch) => {
                TypedPattern::Pin(self.resolve_ty(&ty), id, dispatch)
            }
            TypedPattern::As(ty, inner, id) => TypedPattern::As(
                self.resolve_ty(&ty),
                Box::new(self.resolve_typed_pattern(*inner)),
                id,
            ),
            TypedPattern::Wildcard(ty) => TypedPattern::Wildcard(self.resolve_ty(&ty)),
            TypedPattern::ListNil(ty) => TypedPattern::ListNil(self.resolve_ty(&ty)),
            TypedPattern::ListCons(ty, head, tail) => TypedPattern::ListCons(
                self.resolve_ty(&ty),
                Box::new(self.resolve_typed_pattern(*head)),
                Box::new(self.resolve_typed_pattern(*tail)),
            ),
            TypedPattern::IntLit(ty, n) => TypedPattern::IntLit(self.resolve_ty(&ty), n),
            TypedPattern::StrLit(ty, s) => TypedPattern::StrLit(self.resolve_ty(&ty), s),
            TypedPattern::BoolLit(ty, b) => TypedPattern::BoolLit(self.resolve_ty(&ty), b),
            TypedPattern::DurationLit(ty, n) => TypedPattern::DurationLit(self.resolve_ty(&ty), n),
            TypedPattern::Tuple(ty, items) => TypedPattern::Tuple(
                self.resolve_ty(&ty),
                items
                    .into_iter()
                    .map(|item| self.resolve_typed_pattern(item))
                    .collect(),
            ),
            TypedPattern::Constructor {
                ty,
                tag,
                field_tys,
                fields,
                field_offset,
            } => TypedPattern::Constructor {
                ty: self.resolve_ty(&ty),
                tag,
                field_tys: field_tys
                    .into_iter()
                    .map(|ty| self.resolve_ty(&ty))
                    .collect(),
                fields: fields
                    .into_iter()
                    .map(|field| self.resolve_typed_pattern(field))
                    .collect(),
                field_offset,
            },
            TypedPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                pre_args,
                success_tag,
                err_tag,
                seq_tys,
                items,
            } => TypedPattern::Extractor {
                input_ty: self.resolve_ty(&input_ty),
                extractor,
                extractor_ty: self.resolve_ty(&extractor_ty),
                pre_args: pre_args
                    .into_iter()
                    .map(|arg| *self.resolve_typed_node(arg))
                    .collect(),
                success_tag,
                err_tag,
                seq_tys: seq_tys.into_iter().map(|ty| self.resolve_ty(&ty)).collect(),
                items: items
                    .into_iter()
                    .map(|item| self.resolve_typed_pattern(item))
                    .collect(),
            },
        }
    }

    pub(super) fn resolve_typed_match_pattern(
        &self,
        pattern: TypedMatchPattern,
    ) -> TypedMatchPattern {
        match pattern {
            TypedMatchPattern::HashMap(entries) => TypedMatchPattern::HashMap(
                entries
                    .into_iter()
                    .map(|entry| TypedHashMapMatchPatternEntry {
                        key: *self.resolve_typed_node(entry.key),
                        pattern: self.resolve_typed_match_pattern(entry.pattern),
                        key_span: entry.key_span,
                    })
                    .collect(),
            ),
            TypedMatchPattern::Binding(id) => TypedMatchPattern::Binding(id),
            TypedMatchPattern::Pin { id, ty, dispatch } => TypedMatchPattern::Pin {
                id,
                ty: self.resolve_ty(&ty),
                dispatch,
            },
            TypedMatchPattern::As(inner, id) => {
                TypedMatchPattern::As(Box::new(self.resolve_typed_match_pattern(*inner)), id)
            }
            TypedMatchPattern::Wildcard => TypedMatchPattern::Wildcard,
            TypedMatchPattern::BoolLit(value) => TypedMatchPattern::BoolLit(value),
            TypedMatchPattern::IntLit(value) => TypedMatchPattern::IntLit(value),
            TypedMatchPattern::StrLit(value) => TypedMatchPattern::StrLit(value),
            TypedMatchPattern::DurationLit(value) => TypedMatchPattern::DurationLit(value),
            TypedMatchPattern::ErrorKind(value) => TypedMatchPattern::ErrorKind(value),
            TypedMatchPattern::Or(items) => TypedMatchPattern::Or(
                items
                    .into_iter()
                    .map(|item| self.resolve_typed_match_pattern(item))
                    .collect(),
            ),
            TypedMatchPattern::ErrorPayload {
                kind,
                fields: items,
            } => TypedMatchPattern::ErrorPayload {
                kind,
                fields: items
                    .into_iter()
                    .map(|item| self.resolve_typed_match_pattern(item))
                    .collect(),
            },
            TypedMatchPattern::Tuple(items) => TypedMatchPattern::Tuple(
                items
                    .into_iter()
                    .map(|item| self.resolve_typed_match_pattern(item))
                    .collect(),
            ),
            TypedMatchPattern::Record(items) => TypedMatchPattern::Record(
                items
                    .into_iter()
                    .map(|item| self.resolve_typed_match_pattern(item))
                    .collect(),
            ),
            TypedMatchPattern::Constructor {
                tag,
                fields,
                field_offset,
            } => TypedMatchPattern::Constructor {
                tag,
                fields: fields
                    .into_iter()
                    .map(|field| self.resolve_typed_match_pattern(field))
                    .collect(),
                field_offset,
            },
            TypedMatchPattern::ListNil => TypedMatchPattern::ListNil,
            TypedMatchPattern::ListCons(head, tail) => TypedMatchPattern::ListCons(
                Box::new(self.resolve_typed_match_pattern(*head)),
                Box::new(self.resolve_typed_match_pattern(*tail)),
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
            } => TypedMatchPattern::Extractor {
                input_ty: self.resolve_ty(&input_ty),
                extractor,
                extractor_ty: self.resolve_ty(&extractor_ty),
                pre_args: pre_args
                    .into_iter()
                    .map(|arg| *self.resolve_typed_node(arg))
                    .collect(),
                success_tag,
                err_tag,
                seq_tys: seq_tys.into_iter().map(|ty| self.resolve_ty(&ty)).collect(),
                items: items
                    .into_iter()
                    .map(|item| self.resolve_typed_match_pattern(item))
                    .collect(),
            },
        }
    }
}
