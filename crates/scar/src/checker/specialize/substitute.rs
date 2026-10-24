use super::*;

impl Checker {
    pub(super) fn substitute_specialized_def(
        &self,
        def: TypedNode,
        specialized_fun_idx: u32,
        mapping: &HashMap<u32, Ty>,
    ) -> Result<TypedNode, TypeError> {
        let span = def.span.clone();
        let ty = self.substitute_ty_with_mapping(&def.ty, mapping);
        let node = match def.node {
            TypedInner::Def(
                _,
                id,
                _type_params,
                params,
                ret_ty,
                where_clause,
                body,
                visibility,
            ) => TypedInner::Def(
                specialized_fun_idx,
                id,
                Vec::new(),
                params
                    .into_iter()
                    .map(|param| TypedValueParameter {
                        id: param.id,
                        mode: param.mode,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                        span: param.span,
                    })
                    .collect(),
                self.substitute_ty_with_mapping(&ret_ty, mapping),
                where_clause,
                Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                visibility,
            ),
            TypedInner::ExtractorDef(_, id, _type_params, param, ret_ty, body, visibility) => {
                TypedInner::ExtractorDef(
                    specialized_fun_idx,
                    id,
                    Vec::new(),
                    param
                        .into_iter()
                        .map(|param| TypedValueParameter {
                            id: param.id,
                            mode: param.mode,
                            ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                            span: param.span,
                        })
                        .collect(),
                    self.substitute_ty_with_mapping(&ret_ty, mapping),
                    Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                    visibility,
                )
            }
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!("Expected def/extractor for specialization, got {:?}", other),
                    span,
                    hint: None,
                });
            }
        };
        Ok(TypedNode {
            ty,
            span: def.span,
            node,
        })
    }

    pub(in crate::checker) fn substitute_typed_node_with_mapping(
        &self,
        node: TypedNode,
        mapping: &HashMap<u32, Ty>,
    ) -> TypedNode {
        let span = node.span.clone();
        let ty = self.substitute_ty_with_mapping(&node.ty, mapping);
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
                init: Box::new(self.substitute_typed_node_with_mapping(*init, mapping)),
            },
            TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid,
            } => TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: Box::new(self.substitute_typed_node_with_mapping(*pid, mapping)),
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
                init: Box::new(self.substitute_typed_node_with_mapping(*init, mapping)),
                strategy: Box::new(self.substitute_typed_node_with_mapping(*strategy, mapping)),
            },
            TypedInner::App(func, args) => TypedInner::App(
                Box::new(self.substitute_typed_node_with_mapping(*func, mapping)),
                args.into_iter()
                    .map(|arg| self.substitute_typed_node_with_mapping(arg, mapping))
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
                receiver_ty: self.substitute_ty_with_mapping(&receiver_ty, mapping),
                obligation: TraitObligation {
                    trait_id: obligation.trait_id,
                    trait_args: obligation
                        .trait_args
                        .iter()
                        .map(|arg| self.substitute_ty_with_mapping(arg, mapping))
                        .collect(),
                    receiver: self.substitute_ty_with_mapping(&obligation.receiver, mapping),
                },
                dispatch: self.substitute_selected_dispatch(dispatch, mapping),
                origin: self.substitute_trait_call_origin_with_mapping(origin, mapping),
                args: args
                    .into_iter()
                    .map(|arg| self.substitute_typed_node_with_mapping(arg, mapping))
                    .collect(),
            },
            TypedInner::InjectCall(func, args) => TypedInner::InjectCall(
                Box::new(self.substitute_typed_node_with_mapping(*func, mapping)),
                args.into_iter()
                    .map(|arg| self.substitute_typed_node_with_mapping(arg, mapping))
                    .collect(),
            ),
            TypedInner::Block(stmts) => TypedInner::Block(
                stmts
                    .into_iter()
                    .map(|stmt| self.substitute_typed_node_with_mapping(stmt, mapping))
                    .collect(),
            ),
            TypedInner::Bind(pattern, rhs) => TypedInner::Bind(
                self.substitute_typed_pattern_with_mapping(pattern, mapping),
                Box::new(self.substitute_typed_node_with_mapping(*rhs, mapping)),
            ),
            TypedInner::ApplyPattern {
                value,
                pattern,
                projections,
            } => TypedInner::ApplyPattern {
                value: Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                pattern: self.substitute_typed_pattern_with_mapping(pattern, mapping),
                projections: projections
                    .into_iter()
                    .map(|(id, ty)| (id, self.substitute_ty_with_mapping(&ty, mapping)))
                    .collect(),
            },
            TypedInner::SafeBind(pattern, rhs, projection, failure_target) => TypedInner::SafeBind(
                self.substitute_typed_pattern_with_mapping(pattern, mapping),
                Box::new(self.substitute_typed_node_with_mapping(*rhs, mapping)),
                match projection {
                    SafeBindRhsProjection::CanonicalResultOnce {
                        payload_ty,
                        error_ty,
                    } => SafeBindRhsProjection::CanonicalResultOnce {
                        payload_ty: self.substitute_ty_with_mapping(&payload_ty, mapping),
                        error_ty: self.substitute_ty_with_mapping(&error_ty, mapping),
                    },
                    SafeBindRhsProjection::PatternInput { pattern_input_ty } => {
                        SafeBindRhsProjection::PatternInput {
                            pattern_input_ty: self
                                .substitute_ty_with_mapping(&pattern_input_ty, mapping),
                        }
                    }
                },
                match failure_target {
                    SafeBindFailureTarget::EnclosingMonadFail(mut target) => {
                        target.carrier_ty =
                            self.substitute_ty_with_mapping(&target.carrier_ty, mapping);
                        target.call = Box::new(
                            self.substitute_typed_node_with_mapping(*target.call, mapping),
                        );
                        SafeBindFailureTarget::EnclosingMonadFail(target)
                    }
                    SafeBindFailureTarget::EnclosingMatchResultContext { err_tag } => {
                        SafeBindFailureTarget::EnclosingMatchResultContext { err_tag }
                    }
                    SafeBindFailureTarget::TopLevel => SafeBindFailureTarget::TopLevel,
                    SafeBindFailureTarget::DoMonadFail(mut target) => {
                        target.carrier_ty =
                            self.substitute_ty_with_mapping(&target.carrier_ty, mapping);
                        target.call = Box::new(
                            self.substitute_typed_node_with_mapping(*target.call, mapping),
                        );
                        SafeBindFailureTarget::DoMonadFail(target)
                    }
                    SafeBindFailureTarget::DoAlternative { empty } => {
                        SafeBindFailureTarget::DoAlternative {
                            empty: Box::new(
                                self.substitute_typed_node_with_mapping(*empty, mapping),
                            ),
                        }
                    }
                    SafeBindFailureTarget::Deferred(mut deferred) => {
                        deferred.carrier_ty =
                            self.substitute_ty_with_mapping(&deferred.carrier_ty, mapping);
                        deferred.propagated_error_tys = deferred
                            .propagated_error_tys
                            .iter()
                            .map(|ty| self.substitute_ty_with_mapping(ty, mapping))
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
                    pattern: self.substitute_typed_pattern_with_mapping(pattern, mapping),
                    rhs: Box::new(self.substitute_typed_node_with_mapping(*rhs, mapping)),
                    projection: match projection {
                        SafeBindRhsProjection::CanonicalResultOnce {
                            payload_ty,
                            error_ty,
                        } => SafeBindRhsProjection::CanonicalResultOnce {
                            payload_ty: self.substitute_ty_with_mapping(&payload_ty, mapping),
                            error_ty: self.substitute_ty_with_mapping(&error_ty, mapping),
                        },
                        SafeBindRhsProjection::PatternInput { pattern_input_ty } => {
                            SafeBindRhsProjection::PatternInput {
                                pattern_input_ty: self
                                    .substitute_ty_with_mapping(&pattern_input_ty, mapping),
                            }
                        }
                    },
                    failure_target: match failure_target {
                        SafeBindFailureTarget::DoMonadFail(mut target) => {
                            target.carrier_ty =
                                self.substitute_ty_with_mapping(&target.carrier_ty, mapping);
                            target.call = Box::new(
                                self.substitute_typed_node_with_mapping(*target.call, mapping),
                            );
                            SafeBindFailureTarget::DoMonadFail(target)
                        }
                        SafeBindFailureTarget::DoAlternative { empty } => {
                            SafeBindFailureTarget::DoAlternative {
                                empty: Box::new(
                                    self.substitute_typed_node_with_mapping(*empty, mapping),
                                ),
                            }
                        }
                        SafeBindFailureTarget::Deferred(mut deferred) => {
                            deferred.carrier_ty =
                                self.substitute_ty_with_mapping(&deferred.carrier_ty, mapping);
                            deferred.propagated_error_tys = deferred
                                .propagated_error_tys
                                .iter()
                                .map(|ty| self.substitute_ty_with_mapping(ty, mapping))
                                .collect();
                            SafeBindFailureTarget::Deferred(deferred)
                        }
                        other => other,
                    },
                    continuation: Box::new(
                        self.substitute_typed_node_with_mapping(*continuation, mapping),
                    ),
                    origins,
                }))
            }
            TypedInner::BinOp(op, left, right) => TypedInner::BinOp(
                op,
                Box::new(self.substitute_typed_node_with_mapping(*left, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*right, mapping)),
            ),
            TypedInner::Pipe(left, right) => TypedInner::Pipe(
                Box::new(self.substitute_typed_node_with_mapping(*left, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*right, mapping)),
            ),
            TypedInner::Compose(flavor, left, right) => TypedInner::Compose(
                flavor,
                Box::new(self.substitute_typed_node_with_mapping(*left, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*right, mapping)),
            ),
            TypedInner::ListNil => TypedInner::ListNil,
            TypedInner::ListCons(head, tail) => TypedInner::ListCons(
                Box::new(self.substitute_typed_node_with_mapping(*head, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*tail, mapping)),
            ),
            TypedInner::ListLiteral(items) => TypedInner::ListLiteral(
                items
                    .into_iter()
                    .map(|item| self.substitute_typed_node_with_mapping(item, mapping))
                    .collect(),
            ),
            TypedInner::HashMapLiteral(entries) => TypedInner::HashMapLiteral(
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            self.substitute_typed_node_with_mapping(key, mapping),
                            self.substitute_typed_node_with_mapping(value, mapping),
                        )
                    })
                    .collect(),
            ),
            TypedInner::ErrorConstruct {
                kind,
                message,
                payload,
                payload_fields,
            } => TypedInner::ErrorConstruct {
                kind,
                message: Box::new(self.substitute_typed_node_with_mapping(*message, mapping)),
                payload: payload
                    .into_iter()
                    .map(|value| self.substitute_typed_node_with_mapping(value, mapping))
                    .collect(),
                payload_fields: payload_fields
                    .into_iter()
                    .map(|(name, ty)| (name, self.substitute_ty_with_mapping(&ty, mapping)))
                    .collect(),
            },
            TypedInner::TupleLiteral(items) => TypedInner::TupleLiteral(
                items
                    .into_iter()
                    .map(|item| self.substitute_typed_node_with_mapping(item, mapping))
                    .collect(),
            ),
            TypedInner::InterpolatedStr(parts) => TypedInner::InterpolatedStr(
                parts
                    .into_iter()
                    .map(|part| match part {
                        TypedInterpolatedPart::Text(text) => TypedInterpolatedPart::Text(text),
                        TypedInterpolatedPart::Expr(expr) => TypedInterpolatedPart::Expr(Box::new(
                            self.substitute_typed_node_with_mapping(*expr, mapping),
                        )),
                    })
                    .collect(),
            ),
            TypedInner::Dbg(args) => TypedInner::Dbg(
                args.into_iter()
                    .map(|arg| TypedDbgArg {
                        span: arg.span,
                        ty_name: arg.ty_name,
                        expr: self.substitute_typed_node_with_mapping(arg.expr, mapping),
                    })
                    .collect(),
            ),
            TypedInner::EagerBoundary(inner) => TypedInner::EagerBoundary(Box::new(
                self.substitute_typed_node_with_mapping(*inner, mapping),
            )),
            TypedInner::If(cond, then_branch, else_branch) => TypedInner::If(
                Box::new(self.substitute_typed_node_with_mapping(*cond, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*then_branch, mapping)),
                else_branch.map(|branch| {
                    Box::new(self.substitute_typed_node_with_mapping(*branch, mapping))
                }),
            ),
            TypedInner::Cause(value, err) => TypedInner::Cause(
                Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*err, mapping)),
            ),
            TypedInner::AssertErrorMatches(value, arms, external_ty) => {
                TypedInner::AssertErrorMatches(
                    Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                    arms.into_iter()
                        .map(|arm| TypedMatchArm {
                            direct_expression: arm.direct_expression,
                            pattern: self
                                .substitute_typed_match_pattern_with_mapping(arm.pattern, mapping),
                            guard: arm
                                .guard
                                .map(|node| self.substitute_typed_node_with_mapping(node, mapping)),
                            body: self.substitute_typed_node_with_mapping(arm.body, mapping),
                        })
                        .collect(),
                    external_ty.map(|ty| self.substitute_ty_with_mapping(&ty, mapping)),
                )
            }
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                Box::new(self.substitute_typed_node_with_mapping(*scrutinee, mapping)),
                arms.into_iter()
                    .map(|arm| TypedMatchArm {
                        direct_expression: arm.direct_expression,
                        pattern: self
                            .substitute_typed_match_pattern_with_mapping(arm.pattern, mapping),
                        guard: arm
                            .guard
                            .map(|guard| self.substitute_typed_node_with_mapping(guard, mapping)),
                        body: self.substitute_typed_node_with_mapping(arm.body, mapping),
                    })
                    .collect(),
            ),
            TypedInner::FieldAccess(expr, index) => TypedInner::FieldAccess(
                Box::new(self.substitute_typed_node_with_mapping(*expr, mapping)),
                index,
            ),
            TypedInner::ProcessContextHandler { process_name, slot } => {
                TypedInner::ProcessContextHandler { process_name, slot }
            }
            TypedInner::FacetPath(path) => {
                TypedInner::FacetPath(self.substitute_typed_facet_path_with_mapping(path, mapping))
            }
            TypedInner::PendingFacetPath(path) => TypedInner::PendingFacetPath(
                self.substitute_pending_facet_path_with_mapping(path, mapping),
            ),
            TypedInner::FacetView {
                api,
                source,
                path,
                source_is_result,
            } => TypedInner::FacetView {
                api,
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: self.substitute_typed_facet_path_with_mapping(path, mapping),
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
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: self.substitute_typed_facet_path_with_mapping(path, mapping),
                value: Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
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
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: self.substitute_typed_facet_path_with_mapping(path, mapping),
                update_fun: Box::new(self.substitute_typed_node_with_mapping(*update_fun, mapping)),
                source_is_result,
                mode,
            },
            TypedInner::StructLit(tag, fields) => TypedInner::StructLit(
                tag,
                fields
                    .into_iter()
                    .map(|field| self.substitute_typed_node_with_mapping(field, mapping))
                    .collect(),
            ),
            TypedInner::ConstructorCall(tag, fields) => TypedInner::ConstructorCall(
                tag,
                fields
                    .into_iter()
                    .map(|field| self.substitute_typed_node_with_mapping(field, mapping))
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
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                        span: param.span,
                    })
                    .collect(),
                Box::new(self.substitute_typed_node_with_mapping(*show, mapping)),
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
                        ty: self.substitute_ty_with_mapping(&argument.ty, mapping),
                        span: argument.span,
                    })
                    .collect(),
                params
                    .into_iter()
                    .map(|param| TypedValueParameter {
                        id: param.id,
                        mode: param.mode,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                        span: param.span,
                    })
                    .collect(),
                self.substitute_ty_with_mapping(&ret_ty, mapping),
                where_clause,
                Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                visibility,
            ),
            TypedInner::ExtractorDef(fun_idx, id, type_params, param, ret_ty, body, visibility) => {
                TypedInner::ExtractorDef(
                    fun_idx,
                    id,
                    type_params
                        .into_iter()
                        .map(|typed_param| TypedTypeParam {
                            name: typed_param.name,
                            ty_var: typed_param.ty_var,
                            bound: typed_param.bound,
                        })
                        .collect(),
                    param
                        .into_iter()
                        .map(|param| TypedValueParameter {
                            id: param.id,
                            mode: param.mode,
                            ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                            span: param.span,
                        })
                        .collect(),
                    self.substitute_ty_with_mapping(&ret_ty, mapping),
                    Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                    visibility,
                )
            }
            TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty) => {
                TypedInner::BuiltinExtractorDecl(
                    id,
                    param_ty
                        .iter()
                        .map(|ty| self.substitute_ty_with_mapping(ty, mapping))
                        .collect(),
                    self.substitute_ty_with_mapping(&ret_ty, mapping),
                )
            }
            TypedInner::Closure(params, captures, body) => TypedInner::Closure(
                params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                    })
                    .collect(),
                captures,
                Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
            ),
            TypedInner::ExtractorClosure(params, captures, body) => TypedInner::ExtractorClosure(
                params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                    })
                    .collect(),
                captures,
                Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
            ),
            TypedInner::CaptureClosure(params, captures, body) => TypedInner::CaptureClosure(
                params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                    })
                    .collect(),
                captures,
                Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
            ),
            TypedInner::CaptureConstructorClosure(id, params, captures, body) => {
                TypedInner::CaptureConstructorClosure(
                    id,
                    params
                        .into_iter()
                        .map(|param| TypedClosureParam {
                            id: param.id,
                            ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                        })
                        .collect(),
                    captures,
                    Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                )
            }
            TypedInner::Capture(target, args) => TypedInner::Capture(
                Box::new(self.substitute_typed_node_with_mapping(*target, mapping)),
                args.into_iter()
                    .map(|arg| self.substitute_typed_node_with_mapping(arg, mapping))
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
            TypedInner::Semi(inner) => TypedInner::Semi(Box::new(
                self.substitute_typed_node_with_mapping(*inner, mapping),
            )),
        };

        TypedNode { ty, span, node }
    }

    fn substitute_typed_facet_path_with_mapping(
        &self,
        path: TypedFacetPath,
        mapping: &HashMap<u32, Ty>,
    ) -> TypedFacetPath {
        TypedFacetPath {
            source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
            focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
            segment_focus_tys: path
                .segment_focus_tys
                .iter()
                .map(|ty| self.substitute_ty_with_mapping(ty, mapping))
                .collect(),
            update_source_ty: self.substitute_ty_with_mapping(&path.update_source_ty, mapping),
            update_focus_ty: self.substitute_ty_with_mapping(&path.update_focus_ty, mapping),
            path_kind: path.path_kind,
            may_fail: path.may_fail,
            segments: path
                .segments
                .into_iter()
                .map(|segment| self.substitute_typed_facet_segment_with_mapping(segment, mapping))
                .collect(),
        }
    }

    fn substitute_typed_facet_segment_with_mapping(
        &self,
        segment: TypedFacetSegment,
        mapping: &HashMap<u32, Ty>,
    ) -> TypedFacetSegment {
        match segment {
            TypedFacetSegment::ListIndex {
                index,
                display,
                literal_index,
            } => TypedFacetSegment::ListIndex {
                index: Box::new(self.substitute_typed_node_with_mapping(*index, mapping)),
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
                start: Box::new(self.substitute_typed_node_with_mapping(*start, mapping)),
                end: Box::new(self.substitute_typed_node_with_mapping(*end, mapping)),
                display,
                literal_start,
                literal_end,
            },
            TypedFacetSegment::MapKey {
                key,
                display,
                literal_key,
            } => TypedFacetSegment::MapKey {
                key: Box::new(self.substitute_typed_node_with_mapping(*key, mapping)),
                display,
                literal_key,
            },
            other => other,
        }
    }

    fn substitute_pending_facet_path_with_mapping(
        &self,
        path: PendingFacetPath,
        mapping: &HashMap<u32, Ty>,
    ) -> PendingFacetPath {
        PendingFacetPath {
            root_path_name: path.root_path_name,
            source_ty_hint: path
                .source_ty_hint
                .map(|ty| self.substitute_ty_with_mapping(&ty, mapping)),
            segments: path
                .segments
                .into_iter()
                .map(|segment| self.substitute_pending_facet_segment_with_mapping(segment, mapping))
                .collect(),
        }
    }

    fn substitute_pending_facet_segment_with_mapping(
        &self,
        segment: PendingFacetSegment,
        mapping: &HashMap<u32, Ty>,
    ) -> PendingFacetSegment {
        let substitute_expr = |expr| match expr {
            PendingFacetExpr::Resolved(expr) => PendingFacetExpr::Resolved(expr),
            PendingFacetExpr::Typed(expr) => PendingFacetExpr::Typed(Box::new(
                self.substitute_typed_node_with_mapping(*expr, mapping),
            )),
        };
        match segment {
            PendingFacetSegment::Bracket { expr, display } => PendingFacetSegment::Bracket {
                expr: substitute_expr(expr),
                display,
            },
            PendingFacetSegment::RangeBracket {
                start,
                end,
                display,
            } => PendingFacetSegment::RangeBracket {
                start: substitute_expr(start),
                end: substitute_expr(end),
                display,
            },
            other => other,
        }
    }

    fn substitute_typed_pattern_with_mapping(
        &self,
        pattern: TypedPattern,
        mapping: &HashMap<u32, Ty>,
    ) -> TypedPattern {
        match pattern {
            TypedPattern::Located(source, inner) => TypedPattern::Located(
                source,
                Box::new(self.substitute_typed_pattern_with_mapping(*inner, mapping)),
            ),
            TypedPattern::HashMap(ty, entries) => TypedPattern::HashMap(
                self.substitute_ty_with_mapping(&ty, mapping),
                entries
                    .into_iter()
                    .map(|entry| TypedHashMapPatternEntry {
                        key: self.substitute_typed_node_with_mapping(entry.key, mapping),
                        pattern: self.substitute_typed_pattern_with_mapping(entry.pattern, mapping),
                        key_span: entry.key_span,
                    })
                    .collect(),
            ),
            TypedPattern::Var(ty, id) => {
                TypedPattern::Var(self.substitute_ty_with_mapping(&ty, mapping), id)
            }
            TypedPattern::Pin(ty, id, dispatch) => TypedPattern::Pin(
                self.substitute_ty_with_mapping(&ty, mapping),
                id,
                self.substitute_selected_dispatch(dispatch, mapping),
            ),
            TypedPattern::As(ty, inner, id) => TypedPattern::As(
                self.substitute_ty_with_mapping(&ty, mapping),
                Box::new(self.substitute_typed_pattern_with_mapping(*inner, mapping)),
                id,
            ),
            TypedPattern::Wildcard(ty) => {
                TypedPattern::Wildcard(self.substitute_ty_with_mapping(&ty, mapping))
            }
            TypedPattern::ListNil(ty) => {
                TypedPattern::ListNil(self.substitute_ty_with_mapping(&ty, mapping))
            }
            TypedPattern::ListCons(ty, head, tail) => TypedPattern::ListCons(
                self.substitute_ty_with_mapping(&ty, mapping),
                Box::new(self.substitute_typed_pattern_with_mapping(*head, mapping)),
                Box::new(self.substitute_typed_pattern_with_mapping(*tail, mapping)),
            ),
            TypedPattern::IntLit(ty, value) => {
                TypedPattern::IntLit(self.substitute_ty_with_mapping(&ty, mapping), value)
            }
            TypedPattern::StrLit(ty, value) => {
                TypedPattern::StrLit(self.substitute_ty_with_mapping(&ty, mapping), value)
            }
            TypedPattern::BoolLit(ty, value) => {
                TypedPattern::BoolLit(self.substitute_ty_with_mapping(&ty, mapping), value)
            }
            TypedPattern::DurationLit(ty, value) => {
                TypedPattern::DurationLit(self.substitute_ty_with_mapping(&ty, mapping), value)
            }
            TypedPattern::Tuple(ty, items) => TypedPattern::Tuple(
                self.substitute_ty_with_mapping(&ty, mapping),
                items
                    .into_iter()
                    .map(|item| self.substitute_typed_pattern_with_mapping(item, mapping))
                    .collect(),
            ),
            TypedPattern::Constructor {
                ty,
                tag,
                field_tys,
                fields,
                field_offset,
            } => TypedPattern::Constructor {
                ty: self.substitute_ty_with_mapping(&ty, mapping),
                tag,
                field_tys: field_tys
                    .into_iter()
                    .map(|ty| self.substitute_ty_with_mapping(&ty, mapping))
                    .collect(),
                fields: fields
                    .into_iter()
                    .map(|field| self.substitute_typed_pattern_with_mapping(field, mapping))
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
                input_ty: self.substitute_ty_with_mapping(&input_ty, mapping),
                extractor,
                extractor_ty: self.substitute_ty_with_mapping(&extractor_ty, mapping),
                pre_args: pre_args
                    .into_iter()
                    .map(|arg| self.substitute_typed_node_with_mapping(arg, mapping))
                    .collect(),
                success_tag,
                err_tag,
                seq_tys: seq_tys
                    .into_iter()
                    .map(|ty| self.substitute_ty_with_mapping(&ty, mapping))
                    .collect(),
                items: items
                    .into_iter()
                    .map(|item| self.substitute_typed_pattern_with_mapping(item, mapping))
                    .collect(),
            },
        }
    }

    fn substitute_typed_match_pattern_with_mapping(
        &self,
        pattern: TypedMatchPattern,
        mapping: &HashMap<u32, Ty>,
    ) -> TypedMatchPattern {
        match pattern {
            TypedMatchPattern::Located(span, ty, inner) => TypedMatchPattern::Located(
                span,
                self.substitute_ty_with_mapping(&ty, mapping),
                Box::new(self.substitute_typed_match_pattern_with_mapping(*inner, mapping)),
            ),
            TypedMatchPattern::HashMap(entries) => TypedMatchPattern::HashMap(
                entries
                    .into_iter()
                    .map(|entry| TypedHashMapMatchPatternEntry {
                        key: self.substitute_typed_node_with_mapping(entry.key, mapping),
                        pattern: self
                            .substitute_typed_match_pattern_with_mapping(entry.pattern, mapping),
                        key_span: entry.key_span,
                    })
                    .collect(),
            ),
            TypedMatchPattern::Binding(id) => TypedMatchPattern::Binding(id),
            TypedMatchPattern::Pin { id, ty, dispatch } => TypedMatchPattern::Pin {
                id,
                ty: self.substitute_ty_with_mapping(&ty, mapping),
                dispatch: self.substitute_selected_dispatch(dispatch, mapping),
            },
            TypedMatchPattern::As(inner, id) => TypedMatchPattern::As(
                Box::new(self.substitute_typed_match_pattern_with_mapping(*inner, mapping)),
                id,
            ),
            TypedMatchPattern::Wildcard => TypedMatchPattern::Wildcard,
            TypedMatchPattern::BoolLit(value) => TypedMatchPattern::BoolLit(value),
            TypedMatchPattern::IntLit(value) => TypedMatchPattern::IntLit(value),
            TypedMatchPattern::StrLit(value) => TypedMatchPattern::StrLit(value),
            TypedMatchPattern::DurationLit(value) => TypedMatchPattern::DurationLit(value),
            TypedMatchPattern::ErrorKind(value) => TypedMatchPattern::ErrorKind(value),
            TypedMatchPattern::Or(items) => TypedMatchPattern::Or(
                items
                    .into_iter()
                    .map(|item| self.substitute_typed_match_pattern_with_mapping(item, mapping))
                    .collect(),
            ),
            TypedMatchPattern::ErrorPayload {
                kind,
                fields: items,
            } => TypedMatchPattern::ErrorPayload {
                kind,
                fields: items
                    .into_iter()
                    .map(|item| self.substitute_typed_match_pattern_with_mapping(item, mapping))
                    .collect(),
            },
            TypedMatchPattern::Tuple(items) => TypedMatchPattern::Tuple(
                items
                    .into_iter()
                    .map(|item| self.substitute_typed_match_pattern_with_mapping(item, mapping))
                    .collect(),
            ),
            TypedMatchPattern::Record(items) => TypedMatchPattern::Record(
                items
                    .into_iter()
                    .map(|item| self.substitute_typed_match_pattern_with_mapping(item, mapping))
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
                    .map(|field| self.substitute_typed_match_pattern_with_mapping(field, mapping))
                    .collect(),
                field_offset,
            },
            TypedMatchPattern::ListNil => TypedMatchPattern::ListNil,
            TypedMatchPattern::ListCons(head, tail) => TypedMatchPattern::ListCons(
                Box::new(self.substitute_typed_match_pattern_with_mapping(*head, mapping)),
                Box::new(self.substitute_typed_match_pattern_with_mapping(*tail, mapping)),
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
                input_ty: self.substitute_ty_with_mapping(&input_ty, mapping),
                extractor,
                extractor_ty: self.substitute_ty_with_mapping(&extractor_ty, mapping),
                pre_args: pre_args
                    .into_iter()
                    .map(|arg| self.substitute_typed_node_with_mapping(arg, mapping))
                    .collect(),
                success_tag,
                err_tag,
                seq_tys: seq_tys
                    .into_iter()
                    .map(|ty| self.substitute_ty_with_mapping(&ty, mapping))
                    .collect(),
                items: items
                    .into_iter()
                    .map(|item| self.substitute_typed_match_pattern_with_mapping(item, mapping))
                    .collect(),
            },
        }
    }

    pub(in crate::checker) fn substitute_ty_with_mapping(
        &self,
        ty: &Ty,
        mapping: &HashMap<u32, Ty>,
    ) -> Ty {
        match ty {
            Ty::Var(var) => {
                if let Some(mapped) = mapping.get(var) {
                    self.resolve_ty(mapped)
                } else {
                    let resolved = self.resolve_ty(ty);
                    if resolved == *ty {
                        resolved
                    } else {
                        self.substitute_ty_with_mapping(&resolved, mapping)
                    }
                }
            }
            Ty::ExtractorClosure(inner) => {
                Ty::ExtractorClosure(Box::new(self.substitute_ty_with_mapping(inner, mapping)))
            }
            Ty::MatchResult(inner) => {
                Ty::MatchResult(Box::new(self.substitute_ty_with_mapping(inner, mapping)))
            }
            Ty::List(inner) => Ty::List(Box::new(self.substitute_ty_with_mapping(inner, mapping))),
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.substitute_ty_with_mapping(inner, mapping))),
            Ty::Pid(inner) => Ty::Pid(Box::new(self.substitute_ty_with_mapping(inner, mapping))),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                *kind,
                Box::new(self.substitute_ty_with_mapping(source, mapping)),
                Box::new(self.substitute_ty_with_mapping(focus, mapping)),
                Box::new(self.substitute_ty_with_mapping(update_source, mapping)),
                Box::new(self.substitute_ty_with_mapping(update_focus, mapping)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.substitute_ty_with_mapping(item, mapping))
                    .collect(),
            ),
            Ty::SelfApp(items) => {
                let source_witness =
                    Self::constructor_application_parts(items).map(|(witness, _)| witness.clone());
                let substituted = items
                    .iter()
                    .map(|item| self.substitute_ty_with_mapping(item, mapping))
                    .collect::<Vec<_>>();
                if let Some((witness, slots)) = Self::constructor_application_parts(&substituted) {
                    if let Some(source) = source_witness.as_ref() {
                        if let ConstructorApplicationOutcome::Applied(applied) =
                            self.apply_constructor_application(source, witness, slots)
                        {
                            return applied;
                        }
                    }
                }
                Ty::SelfApp(substituted)
            }
            Ty::Func(params, ret) => Ty::Func(
                params
                    .iter()
                    .map(|param| self.substitute_ty_with_mapping(param, mapping))
                    .collect(),
                Box::new(self.substitute_ty_with_mapping(ret, mapping)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|param| self.substitute_ty_with_mapping(param, mapping))
                    .collect(),
                ret: Box::new(self.substitute_ty_with_mapping(ret, mapping)),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.substitute_ty_with_mapping(ty, mapping)))
                    .collect(),
                params: params
                    .iter()
                    .map(|param| self.substitute_ty_with_mapping(param, mapping))
                    .collect(),
                ret: Box::new(self.substitute_ty_with_mapping(ret, mapping)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields.map_types(|ty| self.substitute_ty_with_mapping(ty, mapping)),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                fields.map_types(|ty| self.substitute_ty_with_mapping(ty, mapping)),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter()
                    .map(|arg| self.substitute_ty_with_mapping(arg, mapping))
                    .collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.substitute_ty_with_mapping(ok, mapping)),
                Box::new(self.substitute_ty_with_mapping(err, mapping)),
            ),
            other => other.clone(),
        }
    }

    fn substitute_trait_call_origin_with_mapping(
        &self,
        origin: TraitCallOrigin,
        mapping: &HashMap<u32, Ty>,
    ) -> TraitCallOrigin {
        match origin {
            TraitCallOrigin::Explicit => TraitCallOrigin::Explicit,
            TraitCallOrigin::Operator { op, lhs_ty, rhs_ty } => TraitCallOrigin::Operator {
                op,
                lhs_ty: self.substitute_ty_with_mapping(&lhs_ty, mapping),
                rhs_ty: self.substitute_ty_with_mapping(&rhs_ty, mapping),
            },
            TraitCallOrigin::Comparison { op, lhs_ty, rhs_ty } => TraitCallOrigin::Comparison {
                op,
                lhs_ty: self.substitute_ty_with_mapping(&lhs_ty, mapping),
                rhs_ty: self.substitute_ty_with_mapping(&rhs_ty, mapping),
            },
        }
    }

    fn substitute_selected_dispatch(
        &self,
        dispatch: TraitDispatch,
        mapping: &CanonicalSubstitution,
    ) -> TraitDispatch {
        let TraitDispatch::Selected(mut instantiation) = dispatch else {
            return dispatch;
        };
        for ty in instantiation.substitution.values_mut() {
            *ty = self.substitute_ty_with_mapping(ty, mapping);
        }
        for ty in instantiation.caller_substitution.values_mut() {
            *ty = self.substitute_ty_with_mapping(ty, mapping);
        }
        let signature = &mut instantiation.callable_signature;
        for ty in signature
            .return_type_arguments
            .iter_mut()
            .map(|argument| &mut argument.ty)
            .chain(
                signature
                    .value_parameters
                    .iter_mut()
                    .map(|parameter| &mut parameter.ty),
            )
            .chain(std::iter::once(&mut signature.return_type.ty))
            .chain(
                signature
                    .where_constraints
                    .constraints
                    .iter_mut()
                    .map(|constraint| &mut constraint.subject),
            )
        {
            *ty = self
                .substitute_canonical_type(ty, mapping)
                .expect("selected canonical substitution must preserve canonical types");
        }
        TraitDispatch::Selected(instantiation)
    }

    pub(super) fn materialize_trait_method_instantiation(
        &mut self,
        mut instantiation: TraitMethodInstantiation,
        span: &Span,
        context: &mut SpecializationContext<'_>,
    ) -> Result<TraitDispatch, TypeError> {
        for (variable, ty) in &mut instantiation.substitution {
            *ty = self.normalize_executable_constructor_binding(*variable, ty);
        }
        for (variable, ty) in &mut instantiation.caller_substitution {
            *ty = self.normalize_executable_constructor_binding(*variable, ty);
        }
        let signature = &instantiation.callable_signature;
        if signature
            .return_type_arguments
            .iter()
            .map(|argument| &argument.ty)
            .chain(
                signature
                    .value_parameters
                    .iter()
                    .map(|parameter| &parameter.ty),
            )
            .chain(std::iter::once(&signature.return_type.ty))
            .chain(
                signature
                    .where_constraints
                    .constraints
                    .iter()
                    .map(|constraint| &constraint.subject),
            )
            .any(CanonicalTy::has_pending_instantiation)
        {
            return Err(self.trait_dispatch_failure(
                TypeDiagnosticReason::UnresolvedTraitMethodInstantiation,
                signature
                    .identity
                    .owner
                    .as_deref()
                    .expect("trait method signature owner"),
                &signature.identity.name,
                None,
                span,
            ));
        }
        match instantiation.dispatch_target {
            TraitDispatchTarget::UserFunction { name, fun_idx }
                if context.needs_specialization.contains(&fun_idx) =>
            {
                let original_def = context.defs_by_fun_idx.get(&fun_idx).ok_or_else(|| {
                    TypeError::new(
                        format!("Missing generic definition for fun_idx {}", fun_idx),
                        span.clone(),
                    )
                })?;
                let bound_tyvars =
                    context
                        .bound_tyvars_by_fun_idx
                        .get(&fun_idx)
                        .ok_or_else(|| {
                            TypeError::new(
                                format!("Missing specialization binders for fun_idx {}", fun_idx),
                                span.clone(),
                            )
                        })?;
                let concrete_tys = self.trait_specialization_arguments(
                    original_def,
                    &bound_tyvars.tyvars,
                    &instantiation.substitution,
                )?;
                let fun_idx = self.ensure_specialized_def(
                    fun_idx,
                    &concrete_tys,
                    &instantiation.substitution,
                    context.defs_by_fun_idx,
                    context.bound_tyvars_by_fun_idx,
                    context.needs_specialization,
                    context.specialization_fun_idxs,
                    context.generated_defs,
                )?;
                Ok(TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                    name,
                    fun_idx,
                }))
            }
            target => Ok(TraitDispatch::Static(target)),
        }
    }
}
