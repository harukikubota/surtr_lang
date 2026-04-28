use super::*;

impl Checker {
    pub(super) fn specialize_program(
        &mut self,
        stmts: Vec<TypedNode>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut defs_by_fun_idx = HashMap::new();
        for stmt in &stmts {
            if let Some(fun_idx) = Self::def_fun_idx(stmt) {
                defs_by_fun_idx.insert(fun_idx, stmt.clone());
            }
        }

        let mut needs_specialization = HashSet::new();
        let mut bound_tyvars_by_fun_idx = HashMap::new();
        for (fun_idx, def) in &defs_by_fun_idx {
            let bound_tyvars = self.collect_bound_tyvars_for_def(def);
            let needs = Self::typed_node_has_pending_trait_call(def);
            if needs {
                needs_specialization.insert(*fun_idx);
            }
            bound_tyvars_by_fun_idx.insert(*fun_idx, bound_tyvars);
        }

        let mut rewritten = Vec::new();
        let mut generated_defs = Vec::new();
        let mut specialization_fun_idxs: HashMap<(u32, Vec<String>), u32> = HashMap::new();

        for stmt in stmts {
            if let Some(fun_idx) = Self::def_fun_idx(&stmt) {
                if needs_specialization.contains(&fun_idx) {
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
        Ok(rewritten)
    }

    fn rewrite_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &HashMap<u32, TypedNode>,
        bound_tyvars_by_fun_idx: &HashMap<u32, Vec<u32>>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<(u32, Vec<String>), u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<TypedNode, TypeError> {
        let span = node.span.clone();
        let ty = node.ty.clone();
        let node = match node.node {
            TypedInner::Lit(lit) => TypedInner::Lit(lit),
            TypedInner::Var(id) => TypedInner::Var(id),
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
                                labels: Vec::new(),
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
                            labels: Vec::new(),
                            message: format!(
                                "{}::{} could not be specialized to a concrete dispatch target",
                                trait_name, method_name
                            ),
                            span: span.clone(),
                            hint: None,
                        })?,
                    other => other,
                };
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    receiver_ty,
                    dispatch,
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
            TypedInner::ResultMap(left, right) => TypedInner::ResultMap(
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
            TypedInner::ResultBind(left, right) => TypedInner::ResultBind(
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
            TypedInner::RecoverKind(value, marker, handler) => TypedInner::RecoverKind(
                Box::new(self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?),
                marker,
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
            TypedInner::LensPath(path) => TypedInner::LensPath(path),
            TypedInner::LensView {
                source,
                path,
                source_is_result,
            } => TypedInner::LensView {
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
            TypedInner::LensSet {
                source,
                path,
                value,
                source_is_result,
            } => TypedInner::LensSet {
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
            },
            TypedInner::LensOver {
                source,
                path,
                update_fun,
                source_is_result,
            } => TypedInner::LensOver {
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
            TypedInner::Def(fun_idx, id, type_params, params, ret_ty, body, visibility) => {
                TypedInner::Def(
                    fun_idx,
                    id,
                    type_params,
                    params,
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
            TypedInner::StructDef(tag, name, field_names, private_flags) => {
                TypedInner::StructDef(tag, name, field_names, private_flags)
            }
            TypedInner::RecordDef(tag, name, field_names, private_flags) => {
                TypedInner::RecordDef(tag, name, field_names, private_flags)
            }
            TypedInner::EnumDef(name, variants) => TypedInner::EnumDef(name, variants),
            TypedInner::TraitDef(name, methods) => TypedInner::TraitDef(name, methods),
            TypedInner::TraitImplDef(trait_name, target_name) => {
                TypedInner::TraitImplDef(trait_name, target_name)
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
        specialization_fun_idxs: &mut HashMap<(u32, Vec<String>), u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<u32, TypeError> {
        let key = (
            original_fun_idx,
            concrete_tys
                .iter()
                .map(|ty| self.ty_name(ty))
                .collect::<Vec<_>>(),
        );
        if let Some(existing) = specialization_fun_idxs.get(&key) {
            return Ok(*existing);
        }

        let specialized_fun_idx = self.env.next_fun_idx;
        self.env.next_fun_idx += 1;
        specialization_fun_idxs.insert(key, specialized_fun_idx);

        let original_def = defs_by_fun_idx
            .get(&original_fun_idx)
            .cloned()
            .ok_or_else(|| TypeError {
                labels: Vec::new(),
                message: format!(
                    "Missing generic definition for fun_idx {}",
                    original_fun_idx
                ),
                span: Span { start: 0, end: 0 },
                hint: None,
            })?;

        let substituted_def =
            self.substitute_specialized_def(original_def, specialized_fun_idx, mapping)?;
        let rewritten_def = self.rewrite_specializations_in_node(
            substituted_def,
            defs_by_fun_idx,
            bound_tyvars_by_fun_idx,
            needs_specialization,
            specialization_fun_idxs,
            generated_defs,
        )?;
        generated_defs.push(rewritten_def);
        Ok(specialized_fun_idx)
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
            TypedInner::Def(_, id, _type_params, params, ret_ty, body, visibility) => {
                TypedInner::Def(
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
                    Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                    visibility,
                )
            }
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
                    labels: Vec::new(),
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
            TypedInner::Def(_, _, _, params, _, _, _) => params
                .iter()
                .map(|param| param.ty.clone())
                .collect::<Vec<_>>(),
            TypedInner::ExtractorDef(_, _, _, param, _, _, _) => vec![param.ty.clone()],
            other => {
                return Err(TypeError {
                    labels: Vec::new(),
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
            (Ty::List(left), Ty::List(right)) => {
                self.match_specialization_ty(left, right, bound_tyvars, mapping)
            }
            (Ty::Tuple(left), Ty::Tuple(right)) => {
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
            (Ty::Enum(left_name, left_args), Ty::Enum(right_name, right_args))
                if left_name == right_name =>
            {
                for (left, right) in left_args.iter().zip(right_args.iter()) {
                    self.match_specialization_ty(left, right, bound_tyvars, mapping);
                }
            }
            _ => {}
        }
    }

    fn collect_bound_tyvars_for_def(&self, def: &TypedNode) -> Vec<u32> {
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        match &def.node {
            TypedInner::Def(_, _, type_params, params, ret_ty, body, _) => {
                for type_param in type_params {
                    if type_param.bound.is_some() && seen.insert(type_param.ty_var) {
                        ordered.push(type_param.ty_var);
                    }
                }
                for param in params {
                    self.collect_bound_tyvars_in_ty(&param.ty, &mut ordered, &mut seen);
                }
                self.collect_bound_tyvars_in_ty(ret_ty, &mut ordered, &mut seen);
                self.collect_bound_tyvars_in_node(body, &mut ordered, &mut seen);
            }
            TypedInner::ExtractorDef(_, _, type_params, param, ret_ty, body, _) => {
                for type_param in type_params {
                    if type_param.bound.is_some() && seen.insert(type_param.ty_var) {
                        ordered.push(type_param.ty_var);
                    }
                }
                self.collect_bound_tyvars_in_ty(&param.ty, &mut ordered, &mut seen);
                self.collect_bound_tyvars_in_ty(ret_ty, &mut ordered, &mut seen);
                self.collect_bound_tyvars_in_node(body, &mut ordered, &mut seen);
            }
            _ => {}
        }
        ordered
    }

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
            | TypedInner::ResultMap(left, right)
            | TypedInner::ResultBind(left, right)
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
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        self.collect_bound_tyvars_in_node(expr, ordered, seen);
                    }
                }
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
            TypedInner::RecoverKind(value, _, handler) => {
                self.collect_bound_tyvars_in_node(value, ordered, seen);
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
            TypedInner::LensPath(path) => {
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
            }
            TypedInner::LensView { source, path, .. } => {
                self.collect_bound_tyvars_in_node(source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
            }
            TypedInner::LensSet {
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
            TypedInner::LensOver {
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
            TypedInner::Def(_, _, _, _, _, body, _)
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
            Ty::List(inner) => self.collect_bound_tyvars_in_ty(&inner, ordered, seen),
            Ty::TypeRef(inner) => self.collect_bound_tyvars_in_ty(&inner, ordered, seen),
            Ty::Lens(source, focus) => {
                self.collect_bound_tyvars_in_ty(&source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&focus, ordered, seen);
            }
            Ty::Tuple(items) => {
                for item in items {
                    self.collect_bound_tyvars_in_ty(&item, ordered, seen);
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
            Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Unit | Ty::Error | Ty::Hole => {}
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
                args,
            } => TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty: self.substitute_ty_with_mapping(&receiver_ty, mapping),
                dispatch,
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
            TypedInner::ResultMap(left, right) => TypedInner::ResultMap(
                Box::new(self.substitute_typed_node_with_mapping(*left, mapping)),
                Box::new(self.substitute_typed_node_with_mapping(*right, mapping)),
            ),
            TypedInner::ResultBind(left, right) => TypedInner::ResultBind(
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
            TypedInner::RecoverKind(value, marker, handler) => TypedInner::RecoverKind(
                Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                marker,
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
            TypedInner::LensPath(path) => TypedInner::LensPath(TypedLensPath {
                source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                may_fail: path.may_fail,
                segments: path.segments,
            }),
            TypedInner::LensView {
                source,
                path,
                source_is_result,
            } => TypedInner::LensView {
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: TypedLensPath {
                    source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                    focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                    may_fail: path.may_fail,
                    segments: path.segments,
                },
                source_is_result,
            },
            TypedInner::LensSet {
                source,
                path,
                value,
                source_is_result,
            } => TypedInner::LensSet {
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: TypedLensPath {
                    source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                    focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                    may_fail: path.may_fail,
                    segments: path.segments,
                },
                value: Box::new(self.substitute_typed_node_with_mapping(*value, mapping)),
                source_is_result,
            },
            TypedInner::LensOver {
                source,
                path,
                update_fun,
                source_is_result,
            } => TypedInner::LensOver {
                source: Box::new(self.substitute_typed_node_with_mapping(*source, mapping)),
                path: TypedLensPath {
                    source_ty: self.substitute_ty_with_mapping(&path.source_ty, mapping),
                    focus_ty: self.substitute_ty_with_mapping(&path.focus_ty, mapping),
                    may_fail: path.may_fail,
                    segments: path.segments,
                },
                update_fun: Box::new(self.substitute_typed_node_with_mapping(*update_fun, mapping)),
                source_is_result,
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
            TypedInner::Def(fun_idx, id, type_params, params, ret_ty, body, visibility) => {
                TypedInner::Def(
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
                    Box::new(self.substitute_typed_node_with_mapping(*body, mapping)),
                    visibility,
                )
            }
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
            TypedInner::StructDef(tag, name, field_names, private_flags) => {
                TypedInner::StructDef(tag, name, field_names, private_flags)
            }
            TypedInner::RecordDef(tag, name, field_names, private_flags) => {
                TypedInner::RecordDef(tag, name, field_names, private_flags)
            }
            TypedInner::EnumDef(name, variants) => TypedInner::EnumDef(name, variants),
            TypedInner::TraitDef(name, methods) => TypedInner::TraitDef(name, methods),
            TypedInner::TraitImplDef(trait_name, target_name) => {
                TypedInner::TraitImplDef(trait_name, target_name)
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
            TypedMatchPattern::As(inner, id) => TypedMatchPattern::As(
                Box::new(self.substitute_typed_match_pattern_with_mapping(*inner, mapping)),
                id,
            ),
            TypedMatchPattern::Wildcard => TypedMatchPattern::Wildcard,
            TypedMatchPattern::BoolLit(value) => TypedMatchPattern::BoolLit(value),
            TypedMatchPattern::IntLit(value) => TypedMatchPattern::IntLit(value),
            TypedMatchPattern::StrLit(value) => TypedMatchPattern::StrLit(value),
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

    fn substitute_ty_with_mapping(&self, ty: &Ty, mapping: &HashMap<u32, Ty>) -> Ty {
        match ty {
            Ty::Var(var) => mapping
                .get(var)
                .cloned()
                .unwrap_or_else(|| self.resolve_ty(ty)),
            Ty::List(inner) => Ty::List(Box::new(self.substitute_ty_with_mapping(inner, mapping))),
            Ty::Lens(source, focus) => Ty::Lens(
                Box::new(self.substitute_ty_with_mapping(source, mapping)),
                Box::new(self.substitute_ty_with_mapping(focus, mapping)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
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

    fn typed_node_has_pending_trait_call(node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::TraitCall { dispatch, args, .. } => {
                matches!(dispatch, TraitDispatch::Pending)
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
            | TypedInner::ResultMap(left, right)
            | TypedInner::ResultBind(left, right)
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
            TypedInner::InterpolatedStr(parts) => parts.iter().any(|part| match part {
                TypedInterpolatedPart::Text(_) => false,
                TypedInterpolatedPart::Expr(expr) => Self::typed_node_has_pending_trait_call(expr),
            }),
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
            TypedInner::RecoverKind(value, _, handler) => {
                Self::typed_node_has_pending_trait_call(value)
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
            TypedInner::LensPath(_) => false,
            TypedInner::LensView { source, .. } => Self::typed_node_has_pending_trait_call(source),
            TypedInner::LensSet { source, value, .. } => {
                Self::typed_node_has_pending_trait_call(source)
                    || Self::typed_node_has_pending_trait_call(value)
            }
            TypedInner::LensOver {
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
            TypedInner::Def(_, _, _, _, _, body, _)
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
