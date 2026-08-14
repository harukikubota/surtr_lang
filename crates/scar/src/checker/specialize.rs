use super::*;

impl Checker {
    pub(super) fn specialize_program(
        &mut self,
        stmts: Vec<TypedNode>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut defs_by_fun_idx = HashMap::new();
        defs_by_fun_idx.extend(self.specializable_defs.clone());
        for stmt in &stmts {
            if let Some(fun_idx) = Self::def_fun_idx(stmt) {
                defs_by_fun_idx.insert(fun_idx, stmt.clone());
            }
        }

        let mut needs_specialization = HashSet::new();
        let mut bound_tyvars_by_fun_idx = HashMap::new();
        for (fun_idx, def) in &defs_by_fun_idx {
            let bound_tyvars = self.collect_bound_tyvars_for_def(def);
            let needs = Self::typed_node_has_pending_trait_call(def) || !bound_tyvars.is_empty();
            if needs {
                needs_specialization.insert(*fun_idx);
            }
            bound_tyvars_by_fun_idx.insert(*fun_idx, bound_tyvars);
        }

        let mut rewritten = Vec::new();
        let mut generated_defs = Vec::new();
        let mut specialization_fun_idxs = self.specialization_fun_idxs.clone();

        for stmt in stmts {
            if let Some(fun_idx) = Self::def_fun_idx(&stmt) {
                if needs_specialization.contains(&fun_idx) {
                    self.specializable_defs.insert(fun_idx, stmt);
                    continue;
                }
            }
            let rewritten_stmt = self.rewrite_specializations_in_node(
                stmt,
                &defs_by_fun_idx,
                &bound_tyvars_by_fun_idx,
                &needs_specialization,
                &mut specialization_fun_idxs,
                &mut generated_defs,
            )?;
            rewritten.push(rewritten_stmt);
        }

        rewritten.extend(generated_defs);
        self.specialization_fun_idxs = specialization_fun_idxs;
        Ok(rewritten)
    }

    fn rewrite_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &HashMap<u32, TypedNode>,
        bound_tyvars_by_fun_idx: &HashMap<u32, Vec<u32>>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<SpecializationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<TypedNode, TypeError> {
        let span = node.span.clone();
        let ty = node.ty.clone();
        let node = match node.node {
            TypedInner::Lit(lit) => TypedInner::Lit(lit),
            TypedInner::Var(id) => TypedInner::Var(id),
            TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init,
            } => TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init: Box::new(self.rewrite_specializations_in_node(
                    *init,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            },
            TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid,
            } => TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: Box::new(self.rewrite_specializations_in_node(
                    *pid,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
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
                init: Box::new(self.rewrite_specializations_in_node(
                    *init,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                strategy: Box::new(self.rewrite_specializations_in_node(
                    *strategy,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            },
            TypedInner::App(func, args) => {
                let func = self.rewrite_specializations_in_node(
                    *func,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?;
                let args = args
                    .into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                if let Ty::UserFunc { fun_idx, .. } = &func.ty {
                    if needs_specialization.contains(fun_idx) {
                        let original_def =
                            defs_by_fun_idx.get(fun_idx).ok_or_else(|| TypeError {
                                message: format!(
                                    "Missing generic definition for fun_idx {}",
                                    fun_idx
                                ),
                                span: span.clone(),
                                hint: None,
                            })?;
                        let bound_tyvars = bound_tyvars_by_fun_idx
                            .get(fun_idx)
                            .cloned()
                            .unwrap_or_default();
                        let mapping =
                            self.infer_specialization_mapping(original_def, &args, &bound_tyvars)?;
                        if mapping.len() == bound_tyvars.len()
                            && bound_tyvars.iter().all(|var| {
                                mapping.get(var).is_some_and(|ty| !matches!(ty, Ty::Var(_)))
                            })
                        {
                            let concrete_tys = bound_tyvars
                                .iter()
                                .filter_map(|var| mapping.get(var).cloned())
                                .collect::<Vec<_>>();
                            let specialized_fun_idx = self.ensure_specialized_def(
                                *fun_idx,
                                &concrete_tys,
                                &mapping,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?;
                            let specialized_func_ty = match func.ty.clone() {
                                Ty::UserFunc {
                                    type_params,
                                    params,
                                    ret,
                                    ..
                                } => Ty::UserFunc {
                                    fun_idx: specialized_fun_idx,
                                    type_params,
                                    params,
                                    ret,
                                },
                                other => other,
                            };
                            let specialized_func = TypedNode {
                                ty: specialized_func_ty,
                                span: func.span.clone(),
                                node: func.node,
                            };
                            TypedInner::App(Box::new(specialized_func), args)
                        } else {
                            TypedInner::App(Box::new(func), args)
                        }
                    } else {
                        TypedInner::App(Box::new(func), args)
                    }
                } else {
                    TypedInner::App(Box::new(func), args)
                }
            }
            TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty,
                dispatch,
                origin,
                args,
            } => {
                let args = args
                    .into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let receiver_ty = self.resolve_ty(&receiver_ty);
                let dispatch = match dispatch {
                    TraitDispatch::Pending => self
                        .trait_dispatch_target(&trait_name, &method_name, &receiver_ty)
                        .ok_or_else(|| TypeError {
                            message: format!(
                                "{}::{} could not be specialized to a concrete dispatch target",
                                trait_name, method_name
                            ),
                            span: span.clone(),
                            hint: None,
                        })?,
                    other => other,
                };
                let dispatch = match dispatch {
                    TraitDispatch::Static(TraitDispatchTarget::UserFunction { name, fun_idx })
                        if needs_specialization.contains(&fun_idx) =>
                    {
                        let original_def =
                            defs_by_fun_idx.get(&fun_idx).ok_or_else(|| TypeError {
                                message: format!(
                                    "Missing generic definition for fun_idx {}",
                                    fun_idx
                                ),
                                span: span.clone(),
                                hint: None,
                            })?;
                        let bound_tyvars = bound_tyvars_by_fun_idx
                            .get(&fun_idx)
                            .cloned()
                            .unwrap_or_default();
                        let mapping =
                            self.infer_specialization_mapping(original_def, &args, &bound_tyvars)?;
                        if mapping.len() == bound_tyvars.len()
                            && bound_tyvars.iter().all(|var| {
                                mapping.get(var).is_some_and(|ty| !matches!(ty, Ty::Var(_)))
                            })
                        {
                            let concrete_tys = bound_tyvars
                                .iter()
                                .filter_map(|var| mapping.get(var).cloned())
                                .collect::<Vec<_>>();
                            let specialized_fun_idx = self.ensure_specialized_def(
                                fun_idx,
                                &concrete_tys,
                                &mapping,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?;
                            TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                                name,
                                fun_idx: specialized_fun_idx,
                            })
                        } else {
                            TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                                name,
                                fun_idx,
                            })
                        }
                    }
                    other => other,
                };
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    receiver_ty,
                    dispatch,
                    origin,
                    args,
                }
            }
            TypedInner::InjectCall(func, args) => TypedInner::InjectCall(
                Box::new(self.rewrite_specializations_in_node(
                    *func,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                args.into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::Block(stmts) => TypedInner::Block(
                stmts
                    .into_iter()
                    .map(|stmt| {
                        self.rewrite_specializations_in_node(
                            stmt,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::Bind(pattern, rhs) => TypedInner::Bind(
                pattern,
                Box::new(self.rewrite_specializations_in_node(
                    *rhs,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::SafeBind(pattern, rhs) => TypedInner::SafeBind(
                pattern,
                Box::new(self.rewrite_specializations_in_node(
                    *rhs,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::BinOp(op, left, right) => TypedInner::BinOp(
                op,
                Box::new(self.rewrite_specializations_in_node(
                    *left,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *right,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::Pipe(left, right) => TypedInner::Pipe(
                Box::new(self.rewrite_specializations_in_node(
                    *left,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *right,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::Compose(flavor, left, right) => TypedInner::Compose(
                flavor,
                Box::new(self.rewrite_specializations_in_node(
                    *left,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *right,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::ListNil => TypedInner::ListNil,
            TypedInner::ListCons(head, tail) => TypedInner::ListCons(
                Box::new(self.rewrite_specializations_in_node(
                    *head,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *tail,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::ListLiteral(items) => TypedInner::ListLiteral(
                items
                    .into_iter()
                    .map(|item| {
                        self.rewrite_specializations_in_node(
                            item,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::HashMapLiteral(entries) => TypedInner::HashMapLiteral(
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        Ok((
                            self.rewrite_specializations_in_node(
                                key,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?,
                            self.rewrite_specializations_in_node(
                                value,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?,
                        ))
                    })
                    .collect::<Result<Vec<_>, TypeError>>()?,
            ),
            TypedInner::TupleLiteral(items) => TypedInner::TupleLiteral(
                items
                    .into_iter()
                    .map(|item| {
                        self.rewrite_specializations_in_node(
                            item,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::InterpolatedStr(parts) => TypedInner::InterpolatedStr(
                parts
                    .into_iter()
                    .map(|part| match part {
                        TypedInterpolatedPart::Text(text) => Ok(TypedInterpolatedPart::Text(text)),
                        TypedInterpolatedPart::Expr(expr) => Ok(TypedInterpolatedPart::Expr(
                            Box::new(self.rewrite_specializations_in_node(
                                *expr,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?),
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::Dbg(args) => TypedInner::Dbg(
                args.into_iter()
                    .map(|arg| {
                        Ok(TypedDbgArg {
                            span: arg.span,
                            ty_name: arg.ty_name,
                            expr: self.rewrite_specializations_in_node(
                                arg.expr,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::EagerBoundary(inner) => {
                TypedInner::EagerBoundary(Box::new(self.rewrite_specializations_in_node(
                    *inner,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?))
            }
            TypedInner::If(cond, then_branch, else_branch) => TypedInner::If(
                Box::new(self.rewrite_specializations_in_node(
                    *cond,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *then_branch,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                else_branch
                    .map(|branch| {
                        self.rewrite_specializations_in_node(
                            *branch,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .transpose()?
                    .map(Box::new),
            ),
            TypedInner::Assert(cond, err) => TypedInner::Assert(
                Box::new(self.rewrite_specializations_in_node(
                    *cond,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *err,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::Ensure(value, pred, err) => TypedInner::Ensure(
                Box::new(self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *pred,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *err,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::MapErr(value, err) => TypedInner::MapErr(
                Box::new(self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *err,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::Cause(value, err) => TypedInner::Cause(
                Box::new(self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *err,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::RecoverKind(value, marker, handler) => TypedInner::RecoverKind(
                Box::new(self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *marker,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                Box::new(self.rewrite_specializations_in_node(
                    *handler,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                Box::new(self.rewrite_specializations_in_node(
                    *scrutinee,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                arms.into_iter()
                    .map(|arm| {
                        Ok(TypedMatchArm {
                            pattern: arm.pattern,
                            guard: arm
                                .guard
                                .map(|guard| {
                                    self.rewrite_specializations_in_node(
                                        guard,
                                        defs_by_fun_idx,
                                        bound_tyvars_by_fun_idx,
                                        needs_specialization,
                                        specialization_fun_idxs,
                                        generated_defs,
                                    )
                                })
                                .transpose()?,
                            body: self.rewrite_specializations_in_node(
                                arm.body,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::FieldAccess(expr, index) => TypedInner::FieldAccess(
                Box::new(self.rewrite_specializations_in_node(
                    *expr,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                index,
            ),
            TypedInner::ProcessContextHandler { process_name, slot } => {
                TypedInner::ProcessContextHandler { process_name, slot }
            }
            TypedInner::FacetPath(path) => TypedInner::FacetPath(path),
            TypedInner::PendingFacetPath(path) => TypedInner::PendingFacetPath(path),
            TypedInner::FacetView {
                source,
                path,
                source_is_result,
            } => TypedInner::FacetView {
                source: Box::new(self.rewrite_specializations_in_node(
                    *source,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                path,
                source_is_result,
            },
            TypedInner::FacetSet {
                source,
                path,
                value,
                source_is_result,
                mode,
            } => TypedInner::FacetSet {
                source: Box::new(self.rewrite_specializations_in_node(
                    *source,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                path,
                value: Box::new(self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                source_is_result,
                mode,
            },
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                source_is_result,
                mode,
            } => TypedInner::FacetOver {
                source: Box::new(self.rewrite_specializations_in_node(
                    *source,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                path,
                update_fun: Box::new(self.rewrite_specializations_in_node(
                    *update_fun,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                source_is_result,
                mode,
            },
            TypedInner::StructLit(tag, fields) => TypedInner::StructLit(
                tag,
                fields
                    .into_iter()
                    .map(|field| {
                        self.rewrite_specializations_in_node(
                            field,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::ConstructorCall(tag, fields) => TypedInner::ConstructorCall(
                tag,
                fields
                    .into_iter()
                    .map(|field| {
                        self.rewrite_specializations_in_node(
                            field,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::DeferrorDef(tag, binding, id, params, show) => TypedInner::DeferrorDef(
                tag,
                binding,
                id,
                params,
                Box::new(self.rewrite_specializations_in_node(
                    *show,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
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
                type_params,
                params,
                ret_ty,
                where_clause,
                Box::new(self.rewrite_specializations_in_node(
                    *body,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                visibility,
            ),
            TypedInner::ExtractorDef(fun_idx, id, type_params, param, ret_ty, body, visibility) => {
                TypedInner::ExtractorDef(
                    fun_idx,
                    id,
                    type_params,
                    param,
                    ret_ty,
                    Box::new(self.rewrite_specializations_in_node(
                        *body,
                        defs_by_fun_idx,
                        bound_tyvars_by_fun_idx,
                        needs_specialization,
                        specialization_fun_idxs,
                        generated_defs,
                    )?),
                    visibility,
                )
            }
            TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty) => {
                TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty)
            }
            TypedInner::Closure(params, captures, body) => TypedInner::Closure(
                params,
                captures,
                Box::new(self.rewrite_specializations_in_node(
                    *body,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
            ),
            TypedInner::Capture(target, args) => TypedInner::Capture(
                Box::new(self.rewrite_specializations_in_node(
                    *target,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                args.into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::StructDef(tag, name, field_names, field_policies, readonly_root) => {
                TypedInner::StructDef(tag, name, field_names, field_policies, readonly_root)
            }
            TypedInner::RecordDef(tag, name, field_names, field_policies, readonly_root) => {
                TypedInner::RecordDef(tag, name, field_names, field_policies, readonly_root)
            }
            TypedInner::EnumDef(name, variants) => TypedInner::EnumDef(name, variants),
            TypedInner::TraitDef(name, where_clause, methods) => {
                TypedInner::TraitDef(name, where_clause, methods)
            }
            TypedInner::TraitImplDef(trait_name, target_name, where_clause) => {
                TypedInner::TraitImplDef(trait_name, target_name, where_clause)
            }
            TypedInner::Semi(inner) => {
                TypedInner::Semi(Box::new(self.rewrite_specializations_in_node(
                    *inner,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?))
            }
        };

        Ok(TypedNode { ty, span, node })
    }

    fn ensure_specialized_def(
        &mut self,
        original_fun_idx: u32,
        concrete_tys: &[Ty],
        mapping: &HashMap<u32, Ty>,
        defs_by_fun_idx: &HashMap<u32, TypedNode>,
        bound_tyvars_by_fun_idx: &HashMap<u32, Vec<u32>>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<SpecializationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<u32, TypeError> {
        let original_def = defs_by_fun_idx
            .get(&original_fun_idx)
            .ok_or_else(|| TypeError {
                message: format!(
                    "Missing generic definition for fun_idx {}",
                    original_fun_idx
                ),
                span: Span { start: 0, end: 0 },
                hint: None,
            })?;
        let key = self.specialization_key_for_def(original_def, concrete_tys)?;
        if let Some(existing) = specialization_fun_idxs.get(&key).copied() {
            let is_available = defs_by_fun_idx.contains_key(&existing)
                || generated_defs
                    .iter()
                    .any(|def| Self::def_fun_idx(def) == Some(existing));
            if is_available {
                return Ok(existing);
            }
            // Persistent sessions can retain a specialization key after the
            // corresponding generated definition was discarded with an older
            // compilation unit.  Never emit a stale function index.
            specialization_fun_idxs.remove(&key);
        }

        let specialized_fun_idx = self.env.next_fun_idx;
        self.env.next_fun_idx += 1;
        specialization_fun_idxs.insert(key, specialized_fun_idx);

        let substituted_def =
            self.substitute_specialized_def(original_def.clone(), specialized_fun_idx, mapping)?;
        let rewritten_def = self.rewrite_specializations_in_node(
            substituted_def,
            defs_by_fun_idx,
            bound_tyvars_by_fun_idx,
            needs_specialization,
            specialization_fun_idxs,
            generated_defs,
        )?;
        self.specializable_defs
            .insert(specialized_fun_idx, rewritten_def.clone());
        generated_defs.push(rewritten_def);
        Ok(specialized_fun_idx)
    }

    fn specialization_key_for_def(
        &self,
        def: &TypedNode,
        concrete_tys: &[Ty],
    ) -> Result<SpecializationKey, TypeError> {
        let function_name = Self::specialization_function_name(def).ok_or_else(|| TypeError {
            message: "Expected def/extractor for specialization key".into(),
            span: def.span.clone(),
            hint: None,
        })?;
        Ok(SpecializationKey {
            function_name,
            type_args: concrete_tys
                .iter()
                .map(|ty| self.canonical_ty_key(ty))
                .collect(),
        })
    }

    fn specialization_function_name(def: &TypedNode) -> Option<String> {
        match &def.node {
            TypedInner::Def(_, id, ..) | TypedInner::ExtractorDef(_, id, ..) => {
                Some(id.qualified_name.clone().unwrap_or_else(|| id.name.clone()))
            }
            _ => None,
        }
    }

    pub(super) fn canonical_ty_key(&self, ty: &Ty) -> CanonicalTyKey {
        match self.resolve_ty(ty) {
            Ty::Int => CanonicalTyKey::Int,
            Ty::Float => CanonicalTyKey::Float,
            Ty::Str => CanonicalTyKey::String,
            Ty::Bool => CanonicalTyKey::Boolean,
            Ty::Unit => CanonicalTyKey::Unit,
            Ty::Error => CanonicalTyKey::Error,
            Ty::Hole => CanonicalTyKey::Hole,
            Ty::Var(var) => CanonicalTyKey::Var(var),
            Ty::SelfApp(args) => {
                CanonicalTyKey::SelfApp(args.iter().map(|arg| self.canonical_ty_key(arg)).collect())
            }
            Ty::List(inner) => CanonicalTyKey::List(Box::new(self.canonical_ty_key(&inner))),
            Ty::Tuple(items) => CanonicalTyKey::Tuple(
                items
                    .iter()
                    .map(|item| self.canonical_ty_key(item))
                    .collect(),
            ),
            Ty::Func(params, ret) => CanonicalTyKey::Func {
                params: params
                    .iter()
                    .map(|param| self.canonical_ty_key(param))
                    .collect(),
                ret: Box::new(self.canonical_ty_key(&ret)),
            },
            Ty::Lazy(inner) => CanonicalTyKey::Lazy(Box::new(self.canonical_ty_key(&inner))),
            Ty::Facet(kind, source, focus, update_source, update_focus) => CanonicalTyKey::Facet {
                kind,
                source: Box::new(self.canonical_ty_key(&source)),
                focus: Box::new(self.canonical_ty_key(&focus)),
                update_source: Box::new(self.canonical_ty_key(&update_source)),
                update_focus: Box::new(self.canonical_ty_key(&update_focus)),
            },
            Ty::Pid(name) => CanonicalTyKey::Pid(Self::canonical_specialization_name(&name)),
            Ty::BuiltinFunc { name, params, ret } => CanonicalTyKey::BuiltinFunc {
                name,
                params: params
                    .iter()
                    .map(|param| self.canonical_ty_key(param))
                    .collect(),
                ret: Box::new(self.canonical_ty_key(&ret)),
            },
            Ty::UserFunc {
                type_params,
                params,
                ret,
                ..
            } => CanonicalTyKey::UserFunc {
                type_params,
                params: params
                    .iter()
                    .map(|param| self.canonical_ty_key(param))
                    .collect(),
                ret: Box::new(self.canonical_ty_key(&ret)),
            },
            Ty::Struct(name, fields) => CanonicalTyKey::Struct {
                name: Self::canonical_specialization_name(&name),
                fields: self.canonical_field_keys(&fields),
            },
            Ty::Record(name, fields) => CanonicalTyKey::Record {
                name: Self::canonical_specialization_name(&name),
                fields: self.canonical_field_keys(&fields),
            },
            Ty::Enum(name, args) => CanonicalTyKey::Enum {
                name: Self::canonical_specialization_name(&name),
                args: args.iter().map(|arg| self.canonical_ty_key(arg)).collect(),
            },
            Ty::Result(ok, err) => CanonicalTyKey::Result {
                ok: Box::new(self.canonical_ty_key(&ok)),
                err: Box::new(self.canonical_ty_key(&err)),
            },
        }
    }

    fn canonical_field_keys(&self, fields: &[(String, Ty)]) -> Vec<(String, CanonicalTyKey)> {
        let mut fields = fields
            .iter()
            .map(|(name, ty)| (name.clone(), self.canonical_ty_key(ty)))
            .collect::<Vec<_>>();
        fields.sort_by(|(left, _), (right, _)| left.cmp(right));
        fields
    }

    fn canonical_specialization_name(name: &str) -> String {
        if name.starts_with('$') || name.contains("::") {
            name.to_string()
        } else {
            format!("Global::{name}")
        }
    }

    fn substitute_specialized_def(
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
                    .map(|param| TypedFunParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
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
                    TypedFunParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                    },
                    self.substitute_ty_with_mapping(&ret_ty, mapping),
                    Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                    visibility,
                )
            }
            other => {
                return Err(TypeError {
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

    fn infer_specialization_mapping(
        &self,
        def: &TypedNode,
        args: &[TypedNode],
        bound_tyvars: &[u32],
    ) -> Result<HashMap<u32, Ty>, TypeError> {
        let mut mapping = HashMap::new();
        let param_tys = match &def.node {
            TypedInner::Def(_, _, _, params, _, _, _, _) => params
                .iter()
                .map(|param| param.ty.clone())
                .collect::<Vec<_>>(),
            TypedInner::ExtractorDef(_, _, _, param, _, _, _) => vec![param.ty.clone()],
            other => {
                return Err(TypeError {
                    message: format!("Expected def/extractor for specialization, got {:?}", other),
                    span: def.span.clone(),
                    hint: None,
                });
            }
        };

        for (expected, arg) in param_tys.iter().zip(args.iter()) {
            self.match_specialization_ty(expected, &arg.ty, bound_tyvars, &mut mapping);
        }

        Ok(mapping)
    }

    fn match_specialization_ty(
        &self,
        expected: &Ty,
        actual: &Ty,
        bound_tyvars: &[u32],
        mapping: &mut HashMap<u32, Ty>,
    ) {
        match (expected, actual) {
            (Ty::Var(var), ty) if bound_tyvars.contains(var) => {
                mapping.entry(*var).or_insert_with(|| self.resolve_ty(ty));
            }
            (Ty::SelfApp(items), actual)
                if Self::constructor_application_parts(items).is_some() =>
            {
                let (witness, expected_slots) =
                    Self::constructor_application_parts(items).expect("checked above");
                if let Some(actual_slots) = Self::constructor_application_slots(actual) {
                    self.match_specialization_ty(witness, actual, bound_tyvars, mapping);
                    for (expected_slot, actual_slot) in
                        expected_slots.iter().zip(actual_slots.iter())
                    {
                        self.match_specialization_ty(
                            expected_slot,
                            actual_slot,
                            bound_tyvars,
                            mapping,
                        );
                    }
                }
            }
            (Ty::List(left), Ty::List(right)) => {
                self.match_specialization_ty(left, right, bound_tyvars, mapping)
            }
            (Ty::Lazy(left), Ty::Lazy(right)) => {
                self.match_specialization_ty(left, right, bound_tyvars, mapping)
            }
            (
                Ty::Facet(_, left_source, left_focus, left_update_source, left_update_focus),
                Ty::Facet(_, right_source, right_focus, right_update_source, right_update_focus),
            ) => {
                self.match_specialization_ty(left_source, right_source, bound_tyvars, mapping);
                self.match_specialization_ty(left_focus, right_focus, bound_tyvars, mapping);
                self.match_specialization_ty(
                    left_update_source,
                    right_update_source,
                    bound_tyvars,
                    mapping,
                );
                self.match_specialization_ty(
                    left_update_focus,
                    right_update_focus,
                    bound_tyvars,
                    mapping,
                );
            }
            (Ty::Tuple(left), Ty::Tuple(right)) => {
                for (left, right) in left.iter().zip(right.iter()) {
                    self.match_specialization_ty(left, right, bound_tyvars, mapping);
                }
            }
            (Ty::SelfApp(left), Ty::SelfApp(right)) => {
                for (left, right) in left.iter().zip(right.iter()) {
                    self.match_specialization_ty(left, right, bound_tyvars, mapping);
                }
            }
            (Ty::Func(left_params, left_ret), Ty::Func(right_params, right_ret)) => {
                for (left, right) in left_params.iter().zip(right_params.iter()) {
                    self.match_specialization_ty(left, right, bound_tyvars, mapping);
                }
                self.match_specialization_ty(left_ret, right_ret, bound_tyvars, mapping);
            }
            (Ty::Result(left_ok, left_err), Ty::Result(right_ok, right_err)) => {
                self.match_specialization_ty(left_ok, right_ok, bound_tyvars, mapping);
                self.match_specialization_ty(left_err, right_err, bound_tyvars, mapping);
            }
            (Ty::Pid(left_name), Ty::Pid(right_name))
                if left_name == right_name
                    || left_name.starts_with('$')
                    || right_name.starts_with('$') => {}
            (Ty::Enum(left_name, left_args), Ty::Enum(right_name, right_args))
                if left_name == right_name =>
            {
                for (left, right) in left_args.iter().zip(right_args.iter()) {
                    self.match_specialization_ty(left, right, bound_tyvars, mapping);
                }
            }
            (Ty::Struct(left_name, left_fields), Ty::Struct(right_name, right_fields))
                if left_name == right_name =>
            {
                for ((left_field_name, left_field_ty), (right_field_name, right_field_ty)) in
                    left_fields.iter().zip(right_fields.iter())
                {
                    if left_field_name == right_field_name {
                        self.match_specialization_ty(
                            left_field_ty,
                            right_field_ty,
                            bound_tyvars,
                            mapping,
                        );
                    }
                }
            }
            (Ty::Record(left_name, left_fields), Ty::Record(right_name, right_fields))
                if left_name == right_name =>
            {
                for ((left_field_name, left_field_ty), (right_field_name, right_field_ty)) in
                    left_fields.iter().zip(right_fields.iter())
                {
                    if left_field_name == right_field_name {
                        self.match_specialization_ty(
                            left_field_ty,
                            right_field_ty,
                            bound_tyvars,
                            mapping,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn collect_bound_tyvars_for_def(&self, def: &TypedNode) -> Vec<u32> {
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        match &def.node {
            TypedInner::Def(_, _, type_params, params, ret_ty, _, _body, _) => {
                for type_param in type_params {
                    if type_param.bound.is_some() && seen.insert(type_param.ty_var) {
                        ordered.push(type_param.ty_var);
                    }
                }
                for param in params {
                    self.collect_bound_tyvars_in_ty(&param.ty, &mut ordered, &mut seen);
                }
                self.collect_bound_tyvars_in_ty(ret_ty, &mut ordered, &mut seen);
                self.collect_pending_trait_receiver_tyvars_in_node(
                    _body,
                    &mut ordered,
                    &mut seen,
                );
                // Function-local inference variables can carry trait bounds,
                // but callers cannot infer them from a call site. Only the
                // declared signature determines a valid specialization key.
            }
            TypedInner::ExtractorDef(_, _, type_params, param, ret_ty, _body, _) => {
                for type_param in type_params {
                    if type_param.bound.is_some() && seen.insert(type_param.ty_var) {
                        ordered.push(type_param.ty_var);
                    }
                }
                self.collect_bound_tyvars_in_ty(&param.ty, &mut ordered, &mut seen);
                self.collect_bound_tyvars_in_ty(ret_ty, &mut ordered, &mut seen);
                self.collect_pending_trait_receiver_tyvars_in_node(
                    _body,
                    &mut ordered,
                    &mut seen,
                );
                // See `Def` above: exclude function-local inference variables.
            }
            _ => {}
        }
        ordered
    }

    /// A generic trait instance may be pending in a definition body even when
    /// the current surface syntax cannot express that fully-instantiated
    /// bound in the enclosing signature. It still has to participate in the
    /// definition's specialization key, otherwise Forge sees a Pending call.
    fn collect_pending_trait_receiver_tyvars_in_node(
        &self,
        node: &TypedNode,
        ordered: &mut Vec<u32>,
        seen: &mut HashSet<u32>,
    ) {
        match &node.node {
            TypedInner::TraitCall {
                dispatch: TraitDispatch::Pending,
                receiver_ty,
                args,
                ..
            } => {
                let mut vars = Vec::new();
                Self::collect_ty_vars(receiver_ty, &mut vars);
                for var in vars {
                    if seen.insert(var) {
                        ordered.push(var);
                    }
                }
                for arg in args {
                    self.collect_pending_trait_receiver_tyvars_in_node(arg, ordered, seen);
                }
            }
            TypedInner::TraitCall { args, .. }
            | TypedInner::ListLiteral(args)
            | TypedInner::TupleLiteral(args) => {
                for arg in args {
                    self.collect_pending_trait_receiver_tyvars_in_node(arg, ordered, seen);
                }
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.collect_pending_trait_receiver_tyvars_in_node(func, ordered, seen);
                for arg in args {
                    self.collect_pending_trait_receiver_tyvars_in_node(arg, ordered, seen);
                }
            }
            TypedInner::Block(stmts) => {
                for stmt in stmts {
                    self.collect_pending_trait_receiver_tyvars_in_node(stmt, ordered, seen);
                }
            }
            TypedInner::Bind(_, rhs) | TypedInner::SafeBind(_, rhs) | TypedInner::Semi(rhs) => {
                self.collect_pending_trait_receiver_tyvars_in_node(rhs, ordered, seen)
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                self.collect_pending_trait_receiver_tyvars_in_node(left, ordered, seen);
                self.collect_pending_trait_receiver_tyvars_in_node(right, ordered, seen);
            }
            _ => {}
        }
    }

    #[allow(dead_code)]
    fn collect_bound_tyvars_in_node(
        &self,
        node: &TypedNode,
        ordered: &mut Vec<u32>,
        seen: &mut HashSet<u32>,
    ) {
        self.collect_bound_tyvars_in_ty(&node.ty, ordered, seen);
        match &node.node {
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.collect_bound_tyvars_in_node(func, ordered, seen);
                for arg in args {
                    self.collect_bound_tyvars_in_node(arg, ordered, seen);
                }
            }
            TypedInner::TraitCall { args, .. } => {
                for arg in args {
                    self.collect_bound_tyvars_in_node(arg, ordered, seen);
                }
            }
            TypedInner::Block(stmts) => {
                for stmt in stmts {
                    self.collect_bound_tyvars_in_node(stmt, ordered, seen);
                }
            }
            TypedInner::Bind(_, rhs) | TypedInner::SafeBind(_, rhs) | TypedInner::Semi(rhs) => {
                self.collect_bound_tyvars_in_node(rhs, ordered, seen)
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right) => {
                self.collect_bound_tyvars_in_node(left, ordered, seen);
                self.collect_bound_tyvars_in_node(right, ordered, seen);
            }
            TypedInner::ListCons(head, tail) => {
                self.collect_bound_tyvars_in_node(head, ordered, seen);
                self.collect_bound_tyvars_in_node(tail, ordered, seen);
            }
            TypedInner::ListLiteral(items) | TypedInner::TupleLiteral(items) => {
                for item in items {
                    self.collect_bound_tyvars_in_node(item, ordered, seen);
                }
            }
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    self.collect_bound_tyvars_in_node(key, ordered, seen);
                    self.collect_bound_tyvars_in_node(value, ordered, seen);
                }
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        self.collect_bound_tyvars_in_node(expr, ordered, seen);
                    }
                }
            }
            TypedInner::Dbg(args) => {
                for arg in args {
                    self.collect_bound_tyvars_in_node(&arg.expr, ordered, seen);
                }
            }
            TypedInner::EagerBoundary(inner) => {
                self.collect_bound_tyvars_in_node(inner, ordered, seen)
            }
            TypedInner::If(cond, then_branch, else_branch) => {
                self.collect_bound_tyvars_in_node(cond, ordered, seen);
                self.collect_bound_tyvars_in_node(then_branch, ordered, seen);
                if let Some(else_branch) = else_branch {
                    self.collect_bound_tyvars_in_node(else_branch, ordered, seen);
                }
            }
            TypedInner::Assert(cond, err) => {
                self.collect_bound_tyvars_in_node(cond, ordered, seen);
                self.collect_bound_tyvars_in_node(err, ordered, seen);
            }
            TypedInner::Ensure(value, pred, err) => {
                self.collect_bound_tyvars_in_node(value, ordered, seen);
                self.collect_bound_tyvars_in_node(pred, ordered, seen);
                self.collect_bound_tyvars_in_node(err, ordered, seen);
            }
            TypedInner::MapErr(value, err) | TypedInner::Cause(value, err) => {
                self.collect_bound_tyvars_in_node(value, ordered, seen);
                self.collect_bound_tyvars_in_node(err, ordered, seen);
            }
            TypedInner::RecoverKind(value, marker, handler) => {
                self.collect_bound_tyvars_in_node(value, ordered, seen);
                self.collect_bound_tyvars_in_node(marker, ordered, seen);
                self.collect_bound_tyvars_in_node(handler, ordered, seen);
            }
            TypedInner::Match(scrutinee, arms) => {
                self.collect_bound_tyvars_in_node(scrutinee, ordered, seen);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_bound_tyvars_in_node(guard, ordered, seen);
                    }
                    self.collect_bound_tyvars_in_node(&arm.body, ordered, seen);
                }
            }
            TypedInner::FieldAccess(expr, _) => {
                self.collect_bound_tyvars_in_node(expr, ordered, seen);
            }
            TypedInner::ProcessContextHandler { .. } => {}
            TypedInner::SupervisorSpawn { init, .. } => {
                self.collect_bound_tyvars_in_node(init, ordered, seen);
            }
            TypedInner::SupervisorAdopt { pid, .. } => {
                self.collect_bound_tyvars_in_node(pid, ordered, seen);
            }
            TypedInner::SupervisorStatus { .. } => {}
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                self.collect_bound_tyvars_in_node(init, ordered, seen);
                self.collect_bound_tyvars_in_node(strategy, ordered, seen);
            }
            TypedInner::FacetPath(path) => {
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
            }
            TypedInner::PendingFacetPath(path) => {
                if let Some(source_ty_hint) = &path.source_ty_hint {
                    self.collect_bound_tyvars_in_ty(source_ty_hint, ordered, seen);
                }
            }
            TypedInner::FacetView { source, path, .. } => {
                self.collect_bound_tyvars_in_node(source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
            }
            TypedInner::FacetSet {
                source,
                path,
                value,
                ..
            } => {
                self.collect_bound_tyvars_in_node(source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
                self.collect_bound_tyvars_in_node(value, ordered, seen);
            }
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                ..
            } => {
                self.collect_bound_tyvars_in_node(source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
                self.collect_bound_tyvars_in_node(update_fun, ordered, seen);
            }
            TypedInner::StructLit(_, fields) | TypedInner::ConstructorCall(_, fields) => {
                for field in fields {
                    self.collect_bound_tyvars_in_node(field, ordered, seen);
                }
            }
            TypedInner::DeferrorDef(_, _, _, _, show) => {
                self.collect_bound_tyvars_in_node(show, ordered, seen);
            }
            TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body) => {
                self.collect_bound_tyvars_in_node(body, ordered, seen);
            }
            TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..) => {}
        }
    }

    fn collect_bound_tyvars_in_ty(&self, ty: &Ty, ordered: &mut Vec<u32>, seen: &mut HashSet<u32>) {
        match self.resolve_ty(ty) {
            Ty::Var(var) => {
                if !self.tyvar_bound_names(var).is_empty() && seen.insert(var) {
                    ordered.push(var);
                }
            }
            Ty::List(inner) | Ty::Lazy(inner) => {
                self.collect_bound_tyvars_in_ty(&inner, ordered, seen)
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.collect_bound_tyvars_in_ty(&source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&focus, ordered, seen);
                self.collect_bound_tyvars_in_ty(&update_source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&update_focus, ordered, seen);
            }
            Ty::Tuple(items) => {
                for item in items {
                    self.collect_bound_tyvars_in_ty(&item, ordered, seen);
                }
            }
            Ty::SelfApp(items) => {
                if let Some((witness, slots)) = Self::constructor_application_parts(&items) {
                    if let Ty::Var(var) = self.resolve_ty(witness) {
                        if seen.insert(var) {
                            ordered.push(var);
                        }
                    }
                    for slot in slots {
                        self.collect_bound_tyvars_in_ty(slot, ordered, seen);
                    }
                } else {
                    for item in items {
                        self.collect_bound_tyvars_in_ty(&item, ordered, seen);
                    }
                }
            }
            Ty::Func(params, ret) => {
                for param in params {
                    self.collect_bound_tyvars_in_ty(&param, ordered, seen);
                }
                self.collect_bound_tyvars_in_ty(&ret, ordered, seen);
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                for param in params {
                    self.collect_bound_tyvars_in_ty(&param, ordered, seen);
                }
                self.collect_bound_tyvars_in_ty(&ret, ordered, seen);
            }
            Ty::Result(ok, err) => {
                self.collect_bound_tyvars_in_ty(&ok, ordered, seen);
                self.collect_bound_tyvars_in_ty(&err, ordered, seen);
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => {
                for (_, field_ty) in fields {
                    self.collect_bound_tyvars_in_ty(&field_ty, ordered, seen);
                }
            }
            Ty::Enum(_, args) => {
                for arg in args {
                    self.collect_bound_tyvars_in_ty(&arg, ordered, seen);
                }
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::Hole
            | Ty::Pid(_) => {}
        }
    }

    fn substitute_typed_node_with_mapping(
        &self,
        node: TypedNode,
        mapping: &HashMap<u32, Ty>,
    ) -> TypedNode {
        let span = node.span.clone();
        let ty = self.substitute_ty_with_mapping(&node.ty, mapping);
        let node = match node.node {
            TypedInner::Lit(lit) => TypedInner::Lit(lit),
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
                dispatch,
                origin,
                args,
            } => TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty: self.substitute_ty_with_mapping(&receiver_ty, mapping),
                dispatch,
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
            TypedInner::SafeBind(pattern, rhs) => TypedInner::SafeBind(
                self.substitute_typed_pattern_with_mapping(pattern, mapping),
                Box::new(self.substitute_typed_node_with_mapping(*rhs, mapping)),
            ),
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
            TypedInner::Assert(cond, err) => TypedInner::Assert(
                Box::new(self.substitute_typed_node_with_mapping(*cond, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*err, mapping)),
            ),
            TypedInner::Ensure(value, pred, err) => TypedInner::Ensure(
                Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*pred, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*err, mapping)),
            ),
            TypedInner::MapErr(value, err) => TypedInner::MapErr(
                Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*err, mapping)),
            ),
            TypedInner::Cause(value, err) => TypedInner::Cause(
                Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*err, mapping)),
            ),
            TypedInner::RecoverKind(value, marker, handler) => TypedInner::RecoverKind(
                Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*marker, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*handler, mapping)),
            ),
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                Box::new(self.substitute_typed_node_with_mapping(*scrutinee, mapping)),
                arms.into_iter()
                    .map(|arm| TypedMatchArm {
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
            TypedInner::FacetPath(path) => TypedInner::FacetPath(TypedFacetPath {
                source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                update_source_ty: self.substitute_ty_with_mapping(&path.update_source_ty, mapping),
                update_focus_ty: self.substitute_ty_with_mapping(&path.update_focus_ty, mapping),
                path_kind: path.path_kind,
                may_fail: path.may_fail,
                source_readonly_root: path.source_readonly_root,
                segments: path.segments,
            }),
            TypedInner::PendingFacetPath(path) => TypedInner::PendingFacetPath(PendingFacetPath {
                root_path_name: path.root_path_name,
                source_ty_hint: path
                    .source_ty_hint
                    .map(|ty| self.substitute_ty_with_mapping(&ty, mapping)),
                segments: path.segments,
            }),
            TypedInner::FacetView {
                source,
                path,
                source_is_result,
            } => TypedInner::FacetView {
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: TypedFacetPath {
                    source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                    focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                    update_source_ty: self
                        .substitute_ty_with_mapping(&path.update_source_ty, mapping),
                    update_focus_ty: self
                        .substitute_ty_with_mapping(&path.update_focus_ty, mapping),
                    path_kind: path.path_kind,
                    may_fail: path.may_fail,
                    source_readonly_root: path.source_readonly_root,
                    segments: path.segments,
                },
                source_is_result,
            },
            TypedInner::FacetSet {
                source,
                path,
                value,
                source_is_result,
                mode,
            } => TypedInner::FacetSet {
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: TypedFacetPath {
                    source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                    focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                    update_source_ty: self
                        .substitute_ty_with_mapping(&path.update_source_ty, mapping),
                    update_focus_ty: self
                        .substitute_ty_with_mapping(&path.update_focus_ty, mapping),
                    path_kind: path.path_kind,
                    may_fail: path.may_fail,
                    source_readonly_root: path.source_readonly_root,
                    segments: path.segments,
                },
                value: Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                source_is_result,
                mode,
            },
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                source_is_result,
                mode,
            } => TypedInner::FacetOver {
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: TypedFacetPath {
                    source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                    focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                    update_source_ty: self
                        .substitute_ty_with_mapping(&path.update_source_ty, mapping),
                    update_focus_ty: self
                        .substitute_ty_with_mapping(&path.update_focus_ty, mapping),
                    path_kind: path.path_kind,
                    may_fail: path.may_fail,
                    source_readonly_root: path.source_readonly_root,
                    segments: path.segments,
                },
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
                    .map(|param| TypedFunParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
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
                    .map(|param| TypedTypeParam {
                        name: param.name,
                        ty_var: param.ty_var,
                        bound: param.bound,
                    })
                    .collect(),
                params
                    .into_iter()
                    .map(|param| TypedFunParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
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
                    TypedFunParam {
                        id: param.id,
                        ty: self.substitute_ty_with_mapping(&param.ty, mapping),
                    },
                    self.substitute_ty_with_mapping(&ret_ty, mapping),
                    Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                    visibility,
                )
            }
            TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty) => {
                TypedInner::BuiltinExtractorDecl(
                    id,
                    self.substitute_ty_with_mapping(&param_ty, mapping),
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
            TypedInner::Capture(target, args) => TypedInner::Capture(
                Box::new(self.substitute_typed_node_with_mapping(*target, mapping)),
                args.into_iter()
                    .map(|arg| self.substitute_typed_node_with_mapping(arg, mapping))
                    .collect(),
            ),
            TypedInner::StructDef(tag, name, field_names, field_policies, readonly_root) => {
                TypedInner::StructDef(tag, name, field_names, field_policies, readonly_root)
            }
            TypedInner::RecordDef(tag, name, field_names, field_policies, readonly_root) => {
                TypedInner::RecordDef(tag, name, field_names, field_policies, readonly_root)
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

    fn substitute_typed_pattern_with_mapping(
        &self,
        pattern: TypedPattern,
        mapping: &HashMap<u32, Ty>,
    ) -> TypedPattern {
        match pattern {
            TypedPattern::Var(ty, id) => {
                TypedPattern::Var(self.substitute_ty_with_mapping(&ty, mapping), id)
            }
            TypedPattern::Pin(ty, id, dispatch) => {
                TypedPattern::Pin(self.substitute_ty_with_mapping(&ty, mapping), id, dispatch)
            }
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
            TypedPattern::ResultOk(ty, inner) => TypedPattern::ResultOk(
                self.substitute_ty_with_mapping(&ty, mapping),
                Box::new(self.substitute_typed_pattern_with_mapping(*inner, mapping)),
            ),
            TypedPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys,
                items,
            } => TypedPattern::Extractor {
                input_ty: self.substitute_ty_with_mapping(&input_ty, mapping),
                extractor,
                extractor_ty: self.substitute_ty_with_mapping(&extractor_ty, mapping),
                success_tag,
                no_match_tag,
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
            TypedMatchPattern::Binding(id) => TypedMatchPattern::Binding(id),
            TypedMatchPattern::Pin { id, ty, dispatch } => TypedMatchPattern::Pin {
                id,
                ty: self.substitute_ty_with_mapping(&ty, mapping),
                dispatch,
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
            TypedMatchPattern::Tuple(items) => TypedMatchPattern::Tuple(
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
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys,
                items,
            } => TypedMatchPattern::Extractor {
                input_ty: self.substitute_ty_with_mapping(&input_ty, mapping),
                extractor,
                extractor_ty: self.substitute_ty_with_mapping(&extractor_ty, mapping),
                success_tag,
                no_match_tag,
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

    pub(super) fn substitute_ty_with_mapping(&self, ty: &Ty, mapping: &HashMap<u32, Ty>) -> Ty {
        match ty {
            Ty::Var(var) => mapping
                .get(var)
                .cloned()
                .unwrap_or_else(|| self.resolve_ty(ty)),
            Ty::List(inner) => Ty::List(Box::new(self.substitute_ty_with_mapping(inner, mapping))),
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.substitute_ty_with_mapping(inner, mapping))),
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
            Ty::SelfApp(items) => Ty::SelfApp(
                items
                    .iter()
                    .map(|item| self.substitute_ty_with_mapping(item, mapping))
                    .collect(),
            ),
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
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                params: params
                    .iter()
                    .map(|param| self.substitute_ty_with_mapping(param, mapping))
                    .collect(),
                ret: Box::new(self.substitute_ty_with_mapping(ret, mapping)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| {
                        (
                            field.clone(),
                            self.substitute_ty_with_mapping(field_ty, mapping),
                        )
                    })
                    .collect(),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| {
                        (
                            field.clone(),
                            self.substitute_ty_with_mapping(field_ty, mapping),
                        )
                    })
                    .collect(),
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

    fn typed_node_has_pending_trait_call(node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::TraitCall {
                dispatch,
                receiver_ty,
                args,
                ..
            } => {
                matches!(dispatch, TraitDispatch::Pending)
                    || matches!(receiver_ty, Ty::Var(_))
                    || args.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                Self::typed_node_has_pending_trait_call(func)
                    || args.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::Block(stmts) => stmts.iter().any(Self::typed_node_has_pending_trait_call),
            TypedInner::Bind(_, rhs) | TypedInner::SafeBind(_, rhs) | TypedInner::Semi(rhs) => {
                Self::typed_node_has_pending_trait_call(rhs)
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right) => {
                Self::typed_node_has_pending_trait_call(left)
                    || Self::typed_node_has_pending_trait_call(right)
            }
            TypedInner::ListCons(head, tail) => {
                Self::typed_node_has_pending_trait_call(head)
                    || Self::typed_node_has_pending_trait_call(tail)
            }
            TypedInner::ListLiteral(items) | TypedInner::TupleLiteral(items) => {
                items.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::HashMapLiteral(entries) => entries.iter().any(|(key, value)| {
                Self::typed_node_has_pending_trait_call(key)
                    || Self::typed_node_has_pending_trait_call(value)
            }),
            TypedInner::InterpolatedStr(parts) => parts.iter().any(|part| match part {
                TypedInterpolatedPart::Text(_) => false,
                TypedInterpolatedPart::Expr(expr) => Self::typed_node_has_pending_trait_call(expr),
            }),
            TypedInner::Dbg(args) => args
                .iter()
                .any(|arg| Self::typed_node_has_pending_trait_call(&arg.expr)),
            TypedInner::EagerBoundary(inner) => Self::typed_node_has_pending_trait_call(inner),
            TypedInner::If(cond, then_branch, else_branch) => {
                Self::typed_node_has_pending_trait_call(cond)
                    || Self::typed_node_has_pending_trait_call(then_branch)
                    || else_branch
                        .as_ref()
                        .is_some_and(|branch| Self::typed_node_has_pending_trait_call(branch))
            }
            TypedInner::Assert(cond, err) => {
                Self::typed_node_has_pending_trait_call(cond)
                    || Self::typed_node_has_pending_trait_call(err)
            }
            TypedInner::Ensure(value, pred, err) => {
                Self::typed_node_has_pending_trait_call(value)
                    || Self::typed_node_has_pending_trait_call(pred)
                    || Self::typed_node_has_pending_trait_call(err)
            }
            TypedInner::MapErr(value, err) | TypedInner::Cause(value, err) => {
                Self::typed_node_has_pending_trait_call(value)
                    || Self::typed_node_has_pending_trait_call(err)
            }
            TypedInner::RecoverKind(value, marker, handler) => {
                Self::typed_node_has_pending_trait_call(value)
                    || Self::typed_node_has_pending_trait_call(marker)
                    || Self::typed_node_has_pending_trait_call(handler)
            }
            TypedInner::Match(scrutinee, arms) => {
                Self::typed_node_has_pending_trait_call(scrutinee)
                    || arms.iter().any(|arm| {
                        arm.guard
                            .as_ref()
                            .is_some_and(Self::typed_node_has_pending_trait_call)
                            || Self::typed_node_has_pending_trait_call(&arm.body)
                    })
            }
            TypedInner::FieldAccess(expr, _) => Self::typed_node_has_pending_trait_call(expr),
            TypedInner::SupervisorSpawn { init, .. } => {
                Self::typed_node_has_pending_trait_call(init)
            }
            TypedInner::SupervisorAdopt { pid, .. } => Self::typed_node_has_pending_trait_call(pid),
            TypedInner::SupervisorStatus { .. } => false,
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                Self::typed_node_has_pending_trait_call(init)
                    || Self::typed_node_has_pending_trait_call(strategy)
            }
            TypedInner::ProcessContextHandler { .. } => false,
            TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_) => false,
            TypedInner::FacetView { source, .. } => Self::typed_node_has_pending_trait_call(source),
            TypedInner::FacetSet { source, value, .. } => {
                Self::typed_node_has_pending_trait_call(source)
                    || Self::typed_node_has_pending_trait_call(value)
            }
            TypedInner::FacetOver {
                source, update_fun, ..
            } => {
                Self::typed_node_has_pending_trait_call(source)
                    || Self::typed_node_has_pending_trait_call(update_fun)
            }
            TypedInner::StructLit(_, fields) | TypedInner::ConstructorCall(_, fields) => {
                fields.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::DeferrorDef(_, _, _, _, show) => {
                Self::typed_node_has_pending_trait_call(show)
            }
            TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body) => Self::typed_node_has_pending_trait_call(body),
            TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..) => false,
        }
    }

    fn def_fun_idx(node: &TypedNode) -> Option<u32> {
        match &node.node {
            TypedInner::Def(fun_idx, ..) | TypedInner::ExtractorDef(fun_idx, ..) => Some(*fun_idx),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spire::ast::Visibility;

    fn test_span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn resolved_id(name: &str, qualified_name: Option<&str>, unique_id: u32) -> ResolvedId {
        ResolvedId {
            name: name.to_string(),
            qualified_name: qualified_name.map(str::to_string),
            symbol_info: None,
            unique_id,
            compiler_generated: false,
            span: test_span(),
        }
    }

    fn generic_identity_ty(fun_idx: u32, ty_var: u32) -> Ty {
        Ty::UserFunc {
            fun_idx,
            type_params: vec![ty_var],
            params: vec![Ty::Var(ty_var)],
            ret: Box::new(Ty::Var(ty_var)),
        }
    }

    fn generic_identity_def(
        fun_idx: u32,
        id: ResolvedId,
        param_id: ResolvedId,
        ty_var: u32,
    ) -> TypedNode {
        TypedNode {
            ty: generic_identity_ty(fun_idx, ty_var),
            span: test_span(),
            node: TypedInner::Def(
                fun_idx,
                id,
                vec![TypedTypeParam {
                    name: "$A".to_string(),
                    ty_var,
                    bound: Some("Add".to_string()),
                }],
                vec![TypedFunParam {
                    id: param_id.clone(),
                    ty: Ty::Var(ty_var),
                }],
                Ty::Var(ty_var),
                None,
                Box::new(TypedNode {
                    ty: Ty::Var(ty_var),
                    span: test_span(),
                    node: TypedInner::Var(param_id),
                }),
                Visibility::Public,
            ),
        }
    }

    fn typed_arg(unique_id: u32, ty: Ty) -> TypedNode {
        TypedNode {
            ty,
            span: test_span(),
            node: TypedInner::Var(resolved_id("arg", None, unique_id)),
        }
    }

    fn call_generic(id: ResolvedId, fun_idx: u32, ty_var: u32, arg: TypedNode) -> TypedNode {
        let ret_ty = arg.ty.clone();
        TypedNode {
            ty: ret_ty,
            span: test_span(),
            node: TypedInner::App(
                Box::new(TypedNode {
                    ty: generic_identity_ty(fun_idx, ty_var),
                    span: test_span(),
                    node: TypedInner::Var(id),
                }),
                vec![arg],
            ),
        }
    }

    fn generated_def_fun_idxs(nodes: &[TypedNode], qualified_name: &str) -> Vec<u32> {
        nodes
            .iter()
            .filter_map(|node| match &node.node {
                TypedInner::Def(fun_idx, id, ..)
                    if id.qualified_name.as_deref() == Some(qualified_name) =>
                {
                    Some(*fun_idx)
                }
                _ => None,
            })
            .collect()
    }

    fn app_fun_idxs(nodes: &[TypedNode]) -> Vec<u32> {
        nodes
            .iter()
            .filter_map(|node| match &node.node {
                TypedInner::App(func, _) => match &func.ty {
                    Ty::UserFunc { fun_idx, .. } => Some(*fun_idx),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn specializations_reuse_identity_across_incremental_runs() {
        let mut checker = Checker::new(TypecheckContext::default());
        checker.env.next_fun_idx = 100;
        let ty_var = 1;
        let original_fun_idx = 20;
        let function_id = resolved_id("id", Some("Global::id"), 10);
        let param_id = resolved_id("x", None, 11);
        let original_def =
            generic_identity_def(original_fun_idx, function_id.clone(), param_id, ty_var);

        let first = checker
            .specialize_program(vec![
                original_def,
                call_generic(
                    function_id.clone(),
                    original_fun_idx,
                    ty_var,
                    typed_arg(1000, Ty::Int),
                ),
            ])
            .expect("first specialization should succeed");
        let first_generated = generated_def_fun_idxs(&first, "Global::id");
        assert_eq!(first_generated.len(), 1);

        let second = checker
            .specialize_program(vec![call_generic(
                function_id,
                original_fun_idx,
                ty_var,
                typed_arg(1001, Ty::Int),
            )])
            .expect("second specialization should succeed");

        assert!(
            generated_def_fun_idxs(&second, "Global::id").is_empty(),
            "incremental reuse should not emit another specialization def"
        );
        assert_eq!(app_fun_idxs(&second), first_generated);
    }

    #[test]
    fn specialization_keys_distinguish_structural_type_arguments() {
        let mut checker = Checker::new(TypecheckContext::default());
        checker.env.next_fun_idx = 100;
        let ty_var = 1;
        let original_fun_idx = 20;
        let function_id = resolved_id("id", Some("Global::id"), 10);
        let param_id = resolved_id("x", None, 11);
        let original_def =
            generic_identity_def(original_fun_idx, function_id.clone(), param_id, ty_var);

        let box_int = Ty::Struct(
            "Global::Box".to_string(),
            vec![("value".to_string(), Ty::Int)],
        );
        let box_string = Ty::Struct(
            "Global::Box".to_string(),
            vec![("value".to_string(), Ty::Str)],
        );
        let typed = checker
            .specialize_program(vec![
                original_def,
                call_generic(
                    function_id.clone(),
                    original_fun_idx,
                    ty_var,
                    typed_arg(1000, box_int),
                ),
                call_generic(
                    function_id,
                    original_fun_idx,
                    ty_var,
                    typed_arg(1001, box_string),
                ),
            ])
            .expect("specialization should succeed");

        let generated = generated_def_fun_idxs(&typed, "Global::id");
        assert_eq!(generated.len(), 2);
        let app_fun_idxs = app_fun_idxs(&typed);
        assert_eq!(app_fun_idxs.len(), 2);
        assert_ne!(app_fun_idxs[0], app_fun_idxs[1]);
    }
}
