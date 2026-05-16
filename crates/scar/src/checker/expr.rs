use super::*;
use sindr::primitives::int;
use std::sync::atomic::{AtomicU32, Ordering};

static SYNTHETIC_RANGE_UID: AtomicU32 = AtomicU32::new(3_000_000_000);

fn combine_hint_parts(parts: &[Option<String>]) -> Option<String> {
    let joined = parts
        .iter()
        .filter_map(|part| part.as_deref())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

enum FacetPathInput<'a> {
    Expr(&'a Resolved),
    Capture(PendingFacetPath),
}

struct PreparedFacetInput {
    typed_source: TypedNode,
    source_is_result: bool,
    source_value_ty: Ty,
    path: TypedFacetPath,
}

#[derive(Clone, Copy)]
enum ExpectedCallableSlot {
    Plain,
    Contextual,
}

struct ExpectedCallableContract {
    input: Ty,
    ret: Option<Ty>,
    slot: ExpectedCallableSlot,
}

impl Checker {
    pub(super) fn match_result_value_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message: "MatchResult values are extractor-only and can only be constructed inside extractor definitions"
                .into(),
            span: span.clone(),
            hint: Some(
                "Use MatchResult::Success, MatchResult::NoMatch, or MatchResult::Err only in a defextractor body. Ordinary APIs should return Result, Option, or a user-defined enum explicitly."
                    .into(),
            ),
        }
    }

    fn parse_standalone_tuple_root_index(name: &str) -> Option<usize> {
        let suffix = name.strip_prefix('_')?;
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        suffix.parse::<usize>().ok()
    }

    fn parse_tuple_segment_index(field: &str) -> Option<usize> {
        let suffix = field.strip_prefix('_')?;
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        suffix.parse::<usize>().ok()
    }

    fn pending_trait_helper_error(&self, method_name: &str, span: &Span) -> TypeError {
        TypeError {
            message: format!(
                "Trait helper `{}` could not be concretized for this callable binding",
                method_name
            ),
            span: span.clone(),
            hint: Some(
                "Add parameter or binding type annotations, or pass the callable where an expected function type is available."
                    .into(),
            ),
        }
    }

    fn first_pending_trait_helper<'a>(&self, node: &'a TypedNode) -> Option<(&'a str, &'a Span)> {
        match &node.node {
            TypedInner::TraitCall {
                method_name,
                dispatch,
                args,
                ..
            } => {
                if matches!(dispatch, crate::typed::TraitDispatch::Pending) {
                    Some((method_name.as_str(), &node.span))
                } else {
                    args.iter()
                        .find_map(|arg| self.first_pending_trait_helper(arg))
                }
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.first_pending_trait_helper(func).or_else(|| {
                    args.iter()
                        .find_map(|arg| self.first_pending_trait_helper(arg))
                })
            }
            TypedInner::Block(stmts)
            | TypedInner::TupleLiteral(stmts)
            | TypedInner::ListLiteral(stmts)
            | TypedInner::ConstructorCall(_, stmts)
            | TypedInner::StructLit(_, stmts) => stmts
                .iter()
                .find_map(|stmt| self.first_pending_trait_helper(stmt)),
            TypedInner::Bind(_, rhs)
            | TypedInner::SafeBind(_, rhs)
            | TypedInner::Semi(rhs)
            | TypedInner::FieldAccess(rhs, _) => self.first_pending_trait_helper(rhs),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => self
                .first_pending_trait_helper(left)
                .or_else(|| self.first_pending_trait_helper(right)),
            TypedInner::If(cond, then_branch, else_branch) => self
                .first_pending_trait_helper(cond)
                .or_else(|| self.first_pending_trait_helper(then_branch))
                .or_else(|| {
                    else_branch
                        .as_deref()
                        .and_then(|branch| self.first_pending_trait_helper(branch))
                }),
            TypedInner::Assert(cond, err) => self
                .first_pending_trait_helper(cond)
                .or_else(|| self.first_pending_trait_helper(err)),
            TypedInner::Ensure(value, pred, err) => self
                .first_pending_trait_helper(value)
                .or_else(|| self.first_pending_trait_helper(pred))
                .or_else(|| self.first_pending_trait_helper(err)),
            TypedInner::MapErr(value, err) | TypedInner::Cause(value, err) => self
                .first_pending_trait_helper(value)
                .or_else(|| self.first_pending_trait_helper(err)),
            TypedInner::RecoverKind(value, marker, handler) => self
                .first_pending_trait_helper(value)
                .or_else(|| self.first_pending_trait_helper(marker))
                .or_else(|| self.first_pending_trait_helper(handler)),
            TypedInner::Match(scrutinee, arms) => {
                self.first_pending_trait_helper(scrutinee).or_else(|| {
                    arms.iter().find_map(|arm| {
                        arm.guard
                            .as_ref()
                            .and_then(|guard| self.first_pending_trait_helper(guard))
                            .or_else(|| self.first_pending_trait_helper(&arm.body))
                    })
                })
            }
            TypedInner::InterpolatedStr(parts) => parts.iter().find_map(|part| match part {
                crate::typed::TypedInterpolatedPart::Text(_) => None,
                crate::typed::TypedInterpolatedPart::Expr(expr) => {
                    self.first_pending_trait_helper(expr)
                }
            }),
            TypedInner::Dbg(args) => args
                .iter()
                .find_map(|arg| self.first_pending_trait_helper(&arg.expr)),
            TypedInner::Def(_, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body) => self.first_pending_trait_helper(body),
            TypedInner::SupervisorSpawn { init, .. } => self.first_pending_trait_helper(init),
            TypedInner::SupervisorAdopt { pid, .. } => self.first_pending_trait_helper(pid),
            TypedInner::SupervisorWorkers { init, strategy, .. } => self
                .first_pending_trait_helper(init)
                .or_else(|| self.first_pending_trait_helper(strategy)),
            TypedInner::FacetView { source, .. } => self.first_pending_trait_helper(source),
            TypedInner::FacetSet { source, value, .. } => self
                .first_pending_trait_helper(source)
                .or_else(|| self.first_pending_trait_helper(value)),
            TypedInner::FacetOver {
                source, update_fun, ..
            } => self
                .first_pending_trait_helper(source)
                .or_else(|| self.first_pending_trait_helper(update_fun)),
            TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::FacetPath(_)
            | TypedInner::PendingFacetPath(_)
            | TypedInner::DeferrorDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => None,
        }
    }

    fn concretize_pending_trait_calls(&mut self, node: TypedNode) -> Result<TypedNode, TypeError> {
        let span = node.span.clone();
        let ty = self.resolve_ty(&node.ty);
        let node = match node.node {
            TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty,
                dispatch,
                origin,
                args,
            } => {
                let receiver_ty = self.resolve_ty(&receiver_ty);
                let args = args
                    .into_iter()
                    .map(|arg| self.concretize_pending_trait_calls(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let dispatch = match dispatch {
                    crate::typed::TraitDispatch::Pending
                        if !trait_name.contains('<') && !matches!(receiver_ty, Ty::Var(_)) =>
                    {
                        self.trait_dispatch_target(&trait_name, &method_name, &receiver_ty)
                            .ok_or_else(|| {
                                self.pending_trait_helper_error(method_name.as_str(), &span)
                            })?
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
            TypedInner::App(func, args) => TypedInner::App(
                Box::new(self.concretize_pending_trait_calls(*func)?),
                args.into_iter()
                    .map(|arg| self.concretize_pending_trait_calls(arg))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::InjectCall(func, args) => TypedInner::InjectCall(
                Box::new(self.concretize_pending_trait_calls(*func)?),
                args.into_iter()
                    .map(|arg| self.concretize_pending_trait_calls(arg))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::Capture(func, args) => TypedInner::Capture(
                Box::new(self.concretize_pending_trait_calls(*func)?),
                args.into_iter()
                    .map(|arg| self.concretize_pending_trait_calls(arg))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::Block(stmts) => TypedInner::Block(
                stmts
                    .into_iter()
                    .map(|stmt| self.concretize_pending_trait_calls(stmt))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::Bind(pattern, rhs) => TypedInner::Bind(
                pattern,
                Box::new(self.concretize_pending_trait_calls(*rhs)?),
            ),
            TypedInner::SafeBind(pattern, rhs) => TypedInner::SafeBind(
                pattern,
                Box::new(self.concretize_pending_trait_calls(*rhs)?),
            ),
            TypedInner::BinOp(op, left, right) => TypedInner::BinOp(
                op,
                Box::new(self.concretize_pending_trait_calls(*left)?),
                Box::new(self.concretize_pending_trait_calls(*right)?),
            ),
            TypedInner::Pipe(left, right) => TypedInner::Pipe(
                Box::new(self.concretize_pending_trait_calls(*left)?),
                Box::new(self.concretize_pending_trait_calls(*right)?),
            ),
            TypedInner::Compose(flavor, left, right) => TypedInner::Compose(
                flavor,
                Box::new(self.concretize_pending_trait_calls(*left)?),
                Box::new(self.concretize_pending_trait_calls(*right)?),
            ),
            TypedInner::ListCons(head, tail) => TypedInner::ListCons(
                Box::new(self.concretize_pending_trait_calls(*head)?),
                Box::new(self.concretize_pending_trait_calls(*tail)?),
            ),
            TypedInner::TupleLiteral(items) => TypedInner::TupleLiteral(
                items
                    .into_iter()
                    .map(|item| self.concretize_pending_trait_calls(item))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::ListLiteral(items) => TypedInner::ListLiteral(
                items
                    .into_iter()
                    .map(|item| self.concretize_pending_trait_calls(item))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::InterpolatedStr(parts) => TypedInner::InterpolatedStr(
                parts
                    .into_iter()
                    .map(|part| match part {
                        crate::typed::TypedInterpolatedPart::Text(text) => {
                            Ok(crate::typed::TypedInterpolatedPart::Text(text))
                        }
                        crate::typed::TypedInterpolatedPart::Expr(expr) => {
                            Ok(crate::typed::TypedInterpolatedPart::Expr(Box::new(
                                self.concretize_pending_trait_calls(*expr)?,
                            )))
                        }
                    })
                    .collect::<Result<Vec<_>, TypeError>>()?,
            ),
            TypedInner::Dbg(args) => TypedInner::Dbg(
                args.into_iter()
                    .map(|arg| {
                        Ok(crate::typed::TypedDbgArg {
                            span: arg.span,
                            ty_name: arg.ty_name,
                            expr: self.concretize_pending_trait_calls(arg.expr)?,
                        })
                    })
                    .collect::<Result<Vec<_>, TypeError>>()?,
            ),
            TypedInner::If(cond, then_branch, else_branch) => TypedInner::If(
                Box::new(self.concretize_pending_trait_calls(*cond)?),
                Box::new(self.concretize_pending_trait_calls(*then_branch)?),
                else_branch
                    .map(|branch| self.concretize_pending_trait_calls(*branch))
                    .transpose()?
                    .map(Box::new),
            ),
            TypedInner::Assert(cond, err) => TypedInner::Assert(
                Box::new(self.concretize_pending_trait_calls(*cond)?),
                Box::new(self.concretize_pending_trait_calls(*err)?),
            ),
            TypedInner::Ensure(value, pred, err) => TypedInner::Ensure(
                Box::new(self.concretize_pending_trait_calls(*value)?),
                Box::new(self.concretize_pending_trait_calls(*pred)?),
                Box::new(self.concretize_pending_trait_calls(*err)?),
            ),
            TypedInner::MapErr(value, err) => TypedInner::MapErr(
                Box::new(self.concretize_pending_trait_calls(*value)?),
                Box::new(self.concretize_pending_trait_calls(*err)?),
            ),
            TypedInner::Cause(value, err) => TypedInner::Cause(
                Box::new(self.concretize_pending_trait_calls(*value)?),
                Box::new(self.concretize_pending_trait_calls(*err)?),
            ),
            TypedInner::RecoverKind(value, marker, handler) => TypedInner::RecoverKind(
                Box::new(self.concretize_pending_trait_calls(*value)?),
                Box::new(self.concretize_pending_trait_calls(*marker)?),
                Box::new(self.concretize_pending_trait_calls(*handler)?),
            ),
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                Box::new(self.concretize_pending_trait_calls(*scrutinee)?),
                arms.into_iter()
                    .map(|arm| {
                        Ok(crate::typed::TypedMatchArm {
                            pattern: arm.pattern,
                            guard: arm
                                .guard
                                .map(|guard| self.concretize_pending_trait_calls(guard))
                                .transpose()?,
                            body: self.concretize_pending_trait_calls(arm.body)?,
                        })
                    })
                    .collect::<Result<Vec<_>, TypeError>>()?,
            ),
            TypedInner::FieldAccess(expr, index) => TypedInner::FieldAccess(
                Box::new(self.concretize_pending_trait_calls(*expr)?),
                index,
            ),
            TypedInner::Semi(expr) => {
                TypedInner::Semi(Box::new(self.concretize_pending_trait_calls(*expr)?))
            }
            TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init,
            } => TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init: Box::new(self.concretize_pending_trait_calls(*init)?),
            },
            TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid,
            } => TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: Box::new(self.concretize_pending_trait_calls(*pid)?),
            },
            TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init,
                strategy,
            } => TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init: Box::new(self.concretize_pending_trait_calls(*init)?),
                strategy: Box::new(self.concretize_pending_trait_calls(*strategy)?),
            },
            TypedInner::FacetView {
                source,
                path,
                source_is_result,
            } => TypedInner::FacetView {
                source: Box::new(self.concretize_pending_trait_calls(*source)?),
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
                source: Box::new(self.concretize_pending_trait_calls(*source)?),
                path,
                value: Box::new(self.concretize_pending_trait_calls(*value)?),
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
                source: Box::new(self.concretize_pending_trait_calls(*source)?),
                path,
                update_fun: Box::new(self.concretize_pending_trait_calls(*update_fun)?),
                source_is_result,
                mode,
            },
            TypedInner::ConstructorCall(tag, fields) => TypedInner::ConstructorCall(
                tag,
                fields
                    .into_iter()
                    .map(|field| self.concretize_pending_trait_calls(field))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::StructLit(name, fields) => TypedInner::StructLit(
                name,
                fields
                    .into_iter()
                    .map(|field| self.concretize_pending_trait_calls(field))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::Closure(params, captures, body) => TypedInner::Closure(
                params,
                captures,
                Box::new(self.concretize_pending_trait_calls(*body)?),
            ),
            TypedInner::DeferrorDef(tag, fun_idx, id, params, body) => TypedInner::DeferrorDef(
                tag,
                fun_idx,
                id,
                params,
                Box::new(self.concretize_pending_trait_calls(*body)?),
            ),
            TypedInner::Def(fun_idx, id, type_params, params, ret_ty, body, attrs) => {
                TypedInner::Def(
                    fun_idx,
                    id,
                    type_params,
                    params,
                    ret_ty,
                    Box::new(self.concretize_pending_trait_calls(*body)?),
                    attrs,
                )
            }
            TypedInner::ExtractorDef(fun_idx, id, type_params, param, ret_ty, body, attrs) => {
                TypedInner::ExtractorDef(
                    fun_idx,
                    id,
                    type_params,
                    param,
                    ret_ty,
                    Box::new(self.concretize_pending_trait_calls(*body)?),
                    attrs,
                )
            }
            other => other,
        };
        Ok(TypedNode { ty, span, node })
    }

    pub(super) fn check_node(&mut self, node: &Resolved) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Lit(span, lit) => {
                let ty = self.lit_type(lit);
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::Lit(lit.clone()),
                })
            }

            Resolved::Var(span, id) => {
                if self.trait_method_ref(node).is_some() {
                    return Err(TypeError {
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
                            self.instantiate_builtin_ty(&stored_ty)
                        }
                        _ => self.resolve_ty(&stored_ty),
                    };
                    if matches!(ty, Ty::Facet(_, _)) {
                        if let Some(path) = self.facet_bindings.get(&id.unique_id).cloned() {
                            return Ok(match path {
                                StoredFacetPath::Concrete(path) => {
                                    let source_ty = self.resolve_ty(&path.source_ty);
                                    let focus_ty = self.resolve_ty(&path.focus_ty);
                                    TypedNode {
                                        ty: Ty::Facet(
                                            Box::new(source_ty.clone()),
                                            Box::new(focus_ty.clone()),
                                        ),
                                        span: span.clone(),
                                        node: TypedInner::FacetPath(TypedFacetPath {
                                            source_ty,
                                            focus_ty,
                                            path_kind: path.path_kind,
                                            may_fail: path.may_fail,
                                            source_readonly_root: path.source_readonly_root,
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
                    if Self::surface_name(&variant.enum_name) == "MatchResult"
                        && !self.in_extractor_body
                    {
                        return Err(self.match_result_value_not_allowed_error(span));
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

                if let Some(index) = Self::parse_standalone_tuple_root_index(id.name.as_str()) {
                    return Err(TypeError {
                        message: format!(
                            "Standalone tuple root _{} is not allowed; use tuple access with ._{}",
                            index, index
                        ),
                        span: span.clone(),
                        hint: Some("Tuple elements are accessed as value._0, value._1, ...".into()),
                    });
                }

                Err(TypeError {
                    message: format!("Undefined variable: {}", id.name),
                    span: span.clone(),
                    hint: None,
                })
            }

            Resolved::TypeRefWitness(span, ast_ty) => {
                let target_ty = match ast_ty {
                    spire::ast::AstTy::Named(_, name) if name == "Result" => {
                        Ty::Result(Box::new(self.env.fresh_tyvar()), Box::new(Ty::Error))
                    }
                    spire::ast::AstTy::Named(_, name) if name == "Option" => {
                        Ty::Enum("Option".into(), vec![self.env.fresh_tyvar()])
                    }
                    _ => self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?,
                };
                Ok(TypedNode {
                    ty: Ty::TypeRef(Box::new(target_ty)),
                    span: span.clone(),
                    node: TypedInner::Lit(Lit::Unit),
                })
            }

            Resolved::Bind(span, pat, rhs) => {
                if !Self::is_total_bind_pattern(pat) {
                    return Err(TypeError {
                        message: "Only total MatchBlock patterns can be used with `=`".into(),
                        span: span.clone(),
                        hint: Some(
                            "Use `=?` for partial destructuring and extractor-driven matches."
                                .into(),
                        ),
                    });
                }
                let typed_rhs = if let ResolvedPattern::Annotated(_, ast_ty) = pat {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                    let typed_rhs = self.check_node_with_expected(rhs, Some(&expected))?;
                    if !self.types_compatible(&expected, &typed_rhs.ty) {
                        if let Some(err) =
                            self.facet_replace_result_context_error(&typed_rhs, &expected, span)
                        {
                            return Err(err);
                        }
                        if let Some(err) =
                            self.plain_value_result_context_error(&expected, &typed_rhs.ty, span)
                        {
                            return Err(err);
                        }
                    }
                    typed_rhs
                } else {
                    self.check_node(rhs)?
                };
                let typed_rhs = self.concretize_pending_trait_calls(typed_rhs)?;
                if let Some((method_name, pending_span)) =
                    self.first_pending_trait_helper(&typed_rhs)
                {
                    return Err(self.pending_trait_helper_error(method_name, pending_span));
                }
                let facet_path = if matches!(typed_rhs.ty, Ty::Facet(_, _)) {
                    Some(self.stored_facet_path_from_node(typed_rhs.clone(), span)?)
                } else {
                    None
                };
                if matches!(typed_rhs.ty, Ty::Error) {
                    return Err(TypeError {
                        message: "Error values must be wrapped with Err(...)".into(),
                        span: typed_rhs.span.clone(),
                        hint: None,
                    });
                }
                let (typed_pat, pat_ty) = self.check_pattern(pat, &typed_rhs.ty, span)?;
                self.ensure_self_rebinding_types(&typed_pat, span)?;

                self.bind_typed_pattern(&typed_pat, &self.resolve_ty(&pat_ty));
                if let Some(path) = &facet_path {
                    self.bind_facet_pattern_bindings(&typed_pat, path, span)?;
                } else {
                    self.clear_facet_pattern_bindings(&typed_pat);
                }
                self.normalize_env_bindings();

                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Bind(typed_pat, Box::new(typed_rhs)),
                })
            }

            Resolved::SafeBind(span, pat, rhs) => self.check_safebind(span, pat, rhs),

            Resolved::App(span, func, args) => self.check_app(span, func, args),

            Resolved::BinOp(span, op, left, right) => self.check_binop(span, op, left, right),
            Resolved::Pipe(span, left, right) => self.check_pipe(span, left, right),
            Resolved::ContextMap(span, left, right) => self.check_context_map(span, left, right),
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
            Resolved::RangeLiteral(span, start, stop) => {
                self.check_range_literal(span, start, stop)
            }
            Resolved::TupleLiteral(span, elems) => self.check_tuple_literal(span, elems),
            Resolved::Grouped(span, inner) => {
                let mut typed = self.check_node(inner)?;
                typed.span = span.clone();
                Ok(typed)
            }

            Resolved::InterpolatedStr(span, parts) => self.check_interpolated_str(span, parts),
            Resolved::Dbg(span, args) => self.check_dbg(span, args),

            Resolved::If(span, cond, then, else_opt) => self.check_if(span, cond, then, else_opt),
            Resolved::Assert(span, cond, err) => self.check_assert(span, cond, err),
            Resolved::Ensure(span, value, pred, err) => self.check_ensure(span, value, pred, err),
            Resolved::MapErr(span, value, err) => self.check_map_err(span, value, err),
            Resolved::Cause(span, value, err) => self.check_cause(span, value, err),
            Resolved::RecoverKind(span, value, marker, handler) => {
                self.check_recover_kind(span, value, marker, handler)
            }

            Resolved::Match(span, scrutinee, arms) => self.check_match(span, scrutinee, arms),

            Resolved::FieldAccess(span, expr, field) => self.check_field_access(span, expr, field),
            Resolved::FacetSegmentAccess(span, expr, segment) => {
                self.check_facet_segment_access_with_expected(span, expr, segment, None)
            }
            Resolved::InferredFacetCapture(span, segments) => Err(TypeError {
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

            Resolved::Block(span, stmts) => {
                let mut typed_stmts = Vec::new();
                let mut last_ty = Ty::Unit;
                for s in stmts {
                    let t = self.check_node(s)?;
                    last_ty = t.ty.clone();
                    typed_stmts.push(t);
                }
                Ok(TypedNode {
                    ty: last_ty,
                    span: span.clone(),
                    node: TypedInner::Block(typed_stmts),
                })
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
            Resolved::RecordDef(span, id, fields) => self.check_record_def(span, id, fields),
            Resolved::EnumDef(span, id, type_params, variants, _) => {
                self.check_enum_def(span, id, type_params, variants)
            }
            Resolved::StructLit(span, id, field_vals) => {
                self.check_struct_lit(span, id, field_vals)
            }
            Resolved::ConstructorCall(span, id, args) => {
                self.check_constructor_call(span, id, args)
            }
            Resolved::DeferrorDef(span, id, fields, show_expr) => {
                self.check_deferror_def(span, id, fields, show_expr)
            }
            Resolved::Def(span, id, type_params, params, ret_ty, body, attrs) => {
                self.check_def(span, id, type_params, params, ret_ty, body, attrs)
            }
            Resolved::ConstDef(span, _id, _ast_ty, _value, _) => Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Unit),
            }),
            Resolved::ExtractorDef(span, id, type_params, param, ret_ty, body, attrs) => {
                self.check_extractor_def(span, id, type_params, param, ret_ty, body, attrs)
            }
            Resolved::TraitDef(span, id, _, methods, _) => Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::TraitDef(
                    self.trait_key(id),
                    methods
                        .iter()
                        .map(|method| method.id.name.clone())
                        .collect(),
                ),
            }),
            Resolved::TraitImplDef(span, trait_id, trait_args, target_ty, methods) => {
                self.check_trait_impl_def(span, trait_id, trait_args, target_ty, methods)
            }
            Resolved::BuiltinDecl(span, id, params, ret_ty, _) => {
                self.check_builtin_decl(span, id, params, ret_ty)
            }
            Resolved::BuiltinExtractorDecl(span, id, param, ret_ty, _) => {
                self.check_builtin_extractor_decl(span, id, param, ret_ty)
            }
            Resolved::BuiltinTypeDecl(span, id, params, attrs) => {
                self.check_builtin_type_decl(span, id, params, attrs)
            }
            Resolved::ResultCtorDecl(span, id, param_ty, ret_ty, attrs) => {
                self.check_result_ctor_decl(span, id, param_ty, ret_ty, attrs)
            }
            Resolved::Closure(span, params, captures, body) => {
                self.check_closure(span, params, captures, body, None)
            }
            Resolved::Capture(span, target, args) => self.check_capture(span, target, args, None),
        }
    }

    pub(super) fn check_node_with_expected(
        &mut self,
        node: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        match (node, expected) {
            (Resolved::Closure(span, params, captures, body), Some(expected_ty)) => {
                self.check_closure(span, params, captures, body, Some(expected_ty))
            }
            (Resolved::Capture(span, target, args), Some(expected_ty)) => {
                self.check_capture(span, target, args, Some(expected_ty))
            }
            (Resolved::InferredFacetCapture(span, segments), Some(expected_ty)) => {
                self.check_inferred_facet_capture(span, segments, expected_ty)
            }
            (Resolved::Pipe(span, left, right), Some(expected_ty)) => {
                self.check_pipe_with_expected(span, left, right, Some(expected_ty))
            }
            (Resolved::ContextMap(span, left, right), Some(expected_ty)) => {
                self.check_context_map_with_expected(span, left, right, Some(expected_ty))
            }
            (Resolved::ContextBind(span, left, right), Some(expected_ty)) => {
                self.check_context_bind_with_expected(span, left, right, Some(expected_ty))
            }
            (Resolved::Compose(span, left, right), Some(expected_ty)) => {
                self.check_compose_with_expected(span, left, right, Some(expected_ty))
            }
            (Resolved::LiftedCompose(span, left, right), Some(expected_ty)) => {
                self.check_lifted_compose_with_expected(span, left, right, Some(expected_ty))
            }
            (Resolved::KleisliCompose(span, left, right), Some(expected_ty)) => {
                self.check_kleisli_compose_with_expected(span, left, right, Some(expected_ty))
            }
            (Resolved::App(span, func, args), Some(expected_ty))
                if self.is_function_on_callee(func) =>
            {
                self.check_function_on_with_expected(span, func, args, expected_ty)
            }
            (Resolved::FieldAccess(span, expr, field), expected_ty) => {
                self.check_field_access_with_expected(span, expr, field, expected_ty)
            }
            (Resolved::FacetSegmentAccess(span, expr, segment), expected_ty) => {
                self.check_facet_segment_access_with_expected(span, expr, segment, expected_ty)
            }
            (Resolved::Grouped(span, inner), Some(expected_ty)) => {
                let mut typed = self.check_node_with_expected(inner, Some(expected_ty))?;
                typed.span = span.clone();
                Ok(typed)
            }
            (Resolved::ProcessContextHandler(span, slot), _) => {
                self.check_process_context_handler(span, slot)
            }
            (_, Some(expected_ty)) => {
                let typed = self.check_node(node)?;
                if matches!(expected_ty, Ty::Error) && self.is_concrete_error_value(&typed) {
                    let call_span = typed.span.clone();
                    return Ok(self.maybe_call_zero_arg_function(typed, call_span));
                }
                Ok(typed)
            }
            _ => self.check_node(node),
        }
    }

    pub(super) fn facet_replace_result_context_error(
        &self,
        typed: &TypedNode,
        expected: &Ty,
        span: &Span,
    ) -> Option<TypeError> {
        let typed = self.tail_typed_node(typed);
        let TypedInner::FacetSet { mode, .. } = &typed.node else {
            return None;
        };
        if *mode != TypedFacetSetMode::Exact {
            return None;
        }

        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(&typed.ty);
        let Ty::Result(ok_ty, _) = &expected else {
            return None;
        };
        if self.resolve_ty(ok_ty.as_ref()) != actual {
            return None;
        }

        Some(TypeError {
            message: format!(
                "Facet::put returns plain {}, not {}",
                self.ty_name(&actual),
                self.ty_name(&expected)
            ),
            span: span.clone(),
            hint: Some(format!(
                "Use Facet::set when the update should stay in {} context, or wrap the replaced value with Ok(...).",
                self.ty_name(&expected)
            )),
        })
    }

    pub(super) fn plain_value_result_context_error(
        &self,
        expected: &Ty,
        actual: &Ty,
        span: &Span,
    ) -> Option<TypeError> {
        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(actual);
        let Ty::Result(ok_ty, _) = &expected else {
            return None;
        };
        if self.resolve_ty(ok_ty.as_ref()) != actual {
            return None;
        }
        Some(TypeError {
            message: format!(
                "Result context expects {}, but the expression returns plain {}",
                self.ty_name(&expected),
                self.ty_name(&actual)
            ),
            span: span.clone(),
            hint: Some(format!(
                "Wrap the value with Ok(...), or use an API that already returns {}.",
                self.ty_name(&expected)
            )),
        })
    }

    fn tail_typed_node<'a>(&self, typed: &'a TypedNode) -> &'a TypedNode {
        match &typed.node {
            TypedInner::Semi(inner) => self.tail_typed_node(inner),
            TypedInner::Block(items) => items
                .last()
                .map(|last| self.tail_typed_node(last))
                .unwrap_or(typed),
            _ => typed,
        }
    }

    fn stored_facet_path_from_node(
        &self,
        typed: TypedNode,
        span: &Span,
    ) -> Result<StoredFacetPath, TypeError> {
        match typed.node {
            TypedInner::FacetPath(path) => Ok(StoredFacetPath::Concrete(TypedFacetPath {
                source_ty: self.resolve_ty(&path.source_ty),
                focus_ty: self.resolve_ty(&path.focus_ty),
                path_kind: path.path_kind,
                may_fail: path.may_fail,
                source_readonly_root: path.source_readonly_root,
                segments: path.segments,
            })),
            TypedInner::PendingFacetPath(path) => Ok(StoredFacetPath::Pending(path)),
            _ => Err(TypeError {
                message:
                    "Facet values are compile-time only in Stage1 and cannot be stored or passed around"
                        .into(),
                span: span.clone(),
                hint: Some("Use type-root path expressions inline (e.g. User.name).".into()),
            }),
        }
    }

    fn bind_facet_pattern_bindings(
        &mut self,
        pattern: &TypedPattern,
        path: &StoredFacetPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedPattern::Var(_, id) => {
                self.facet_bindings.insert(id.unique_id, path.clone());
                Ok(())
            }
            TypedPattern::As(_, inner, alias) => {
                self.bind_facet_pattern_bindings(inner, path, span)?;
                self.facet_bindings.insert(alias.unique_id, path.clone());
                Ok(())
            }
            TypedPattern::Wildcard(_) => Ok(()),
            _ => Err(TypeError {
                message: "Facet values can only be bound to variables or `_` patterns".into(),
                span: span.clone(),
                hint: Some("Use `facet = User.name` or `_ = User.name`.".into()),
            }),
        }
    }

    fn clear_facet_pattern_bindings(&mut self, pattern: &TypedPattern) {
        match pattern {
            TypedPattern::Var(_, id) => {
                self.facet_bindings.remove(&id.unique_id);
            }
            TypedPattern::As(_, inner, alias) => {
                self.clear_facet_pattern_bindings(inner);
                self.facet_bindings.remove(&alias.unique_id);
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.clear_facet_pattern_bindings(head);
                self.clear_facet_pattern_bindings(tail);
            }
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    self.clear_facet_pattern_bindings(item);
                }
            }
            TypedPattern::ResultOk(_, inner) => self.clear_facet_pattern_bindings(inner),
            TypedPattern::Extractor { items, .. } => {
                for item in items {
                    self.clear_facet_pattern_bindings(item);
                }
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::Pin(_, _, _)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
        }
    }

    pub(super) fn check_safebind(
        &mut self,
        span: &Span,
        pat: &ResolvedPattern,
        rhs: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_rhs = self.check_node(rhs)?;
        if matches!(typed_rhs.ty, Ty::Facet(_, _)) {
            return Err(TypeError {
                message: "Facet values cannot be bound with `=?`".into(),
                span: typed_rhs.span.clone(),
                hint: Some("Use `=` for compile-time Facet bindings.".into()),
            });
        }
        let rhs_ty = self.resolve_ty(&typed_rhs.ty);
        if matches!(&rhs_ty, Ty::Enum(name, _) if Self::surface_name(name) == "Option") {
            return Err(TypeError {
                message: "Option is not a SafeBind target; `=?` propagates Result-style failures, not optional values.".into(),
                span: typed_rhs.span.clone(),
                hint: Some(
                    "Convert explicitly with Option::to_result(value, err) before using `=?`."
                        .into(),
                ),
            });
        }
        let pattern_can_nomatch = !Self::is_total_bind_pattern(pat);
        let (ok_ty, mut propagated_err_tys) = match rhs_ty {
            Ty::Result(ok, err) => {
                let mut err_tys = vec![err.as_ref().clone()];
                if pattern_can_nomatch {
                    err_tys.push(Ty::Error);
                }
                (ok.as_ref().clone(), err_tys)
            }
            other => {
                let err_tys = if pattern_can_nomatch {
                    vec![Ty::Error]
                } else {
                    Vec::new()
                };
                (other, err_tys)
            }
        };

        let (typed_pat, pat_ty) = self.check_pattern(pat, &ok_ty, span)?;
        self.ensure_self_rebinding_types(&typed_pat, span)?;
        if let Some(ret_ty) = self.function_return_ty.clone() {
            let fn_err_ty = match ret_ty {
                Ty::Result(_, fn_err_ty) => fn_err_ty,
                other => {
                    return Err(TypeError {
                        message: format!(
                            "`=?` can only be used in functions returning Result<...>, got {}",
                            self.ty_name(&other)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            };

            self.collect_pattern_result_error_types(&typed_pat, &mut propagated_err_tys);

            for propagated in propagated_err_tys {
                if !self.types_compatible(fn_err_ty.as_ref(), &propagated) {
                    return Err(TypeError {
                        message: format!(
                            "`=?` error type mismatch: function returns {}, but expression returns {}",
                            self.ty_name(fn_err_ty.as_ref()),
                            self.ty_name(&propagated)
                        ),
                        span: typed_rhs.span.clone(),
                        hint: None,
                    });
                }
            }
        }

        self.bind_typed_pattern(&typed_pat, &pat_ty);

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::SafeBind(typed_pat, Box::new(typed_rhs)),
        })
    }

    pub(super) fn check_compose_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _)
            | Resolved::Closure(_, _, _, _)
            | Resolved::Compose(_, _, _)
            | Resolved::LiftedCompose(_, _, _)
            | Resolved::KleisliCompose(_, _, _) => self.check_node(node),
            Resolved::Var(_, _) | Resolved::Grouped(_, _) => {
                self.check_function_value_operand(node, op_name)
            }
            _ => Err(TypeError {
                message: format!("{} requires a function value", op_name),
                span: self.resolved_span(node).clone(),
                hint: Some(format!(
                    "{} composes one-argument function values. This operand is being parsed as a call/result expression first. Use `&f`, a closure, a function-typed variable, or another compose expression. If you meant to compose a function returned by a call, parenthesize the call like `(make_fn(...)) {} (other_fn(...))`.",
                    op_name, op_name
                )),
            }),
        }
    }

    pub(super) fn check_operator_compose_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        self.check_function_value_operand(node, op_name)
    }

    pub(super) fn check_apply_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _) | Resolved::Closure(_, _, _, _) => self.check_node(node),
            Resolved::Var(_, _) | Resolved::Grouped(_, _) => {
                self.check_function_value_operand(node, op_name)
            }
            Resolved::App(span, func, args) => self.check_injected_call(span, func, args, op_name),
            _ => Err(TypeError {
                message: format!(
                    "{} requires a function value or a function call like `f(...)`",
                    op_name
                ),
                span: self.resolved_span(node).clone(),
                hint: Some("Use `&f`, a closure, a function-typed variable, or wrap a callable-returning call in parentheses.".into()),
            }),
        }
    }

    fn can_use_expected_callable_context(node: &Resolved) -> bool {
        matches!(
            node,
            Resolved::InferredFacetCapture(_, _)
                | Resolved::Capture(_, _, _)
                | Resolved::Closure(_, _, _, _)
                | Resolved::Grouped(_, _)
        )
    }

    fn expected_callable_ty(&mut self, contract: &ExpectedCallableContract) -> Ty {
        match contract.slot {
            ExpectedCallableSlot::Plain | ExpectedCallableSlot::Contextual => {}
        }
        Ty::Func(
            vec![self.resolve_ty(&contract.input)],
            Box::new(
                contract
                    .ret
                    .as_ref()
                    .map(|ret| self.resolve_ty(ret))
                    .unwrap_or_else(|| self.env.fresh_tyvar()),
            ),
        )
    }

    fn callable_contract(
        &mut self,
        input_ty: &Ty,
        ret_ty: Option<Ty>,
        slot: ExpectedCallableSlot,
    ) -> ExpectedCallableContract {
        ExpectedCallableContract {
            input: self.resolve_ty(input_ty),
            ret: ret_ty.map(|ty| self.resolve_ty(&ty)),
            slot,
        }
    }

    fn check_apply_callable_with_contract(
        &mut self,
        node: &Resolved,
        contract: &ExpectedCallableContract,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if Self::can_use_expected_callable_context(node) {
            let expected = self.expected_callable_ty(contract);
            self.check_node_with_expected(node, Some(&expected))
        } else {
            self.check_apply_callable(node, op_name)
        }
    }

    fn check_compose_callable_with_contract(
        &mut self,
        node: &Resolved,
        contract: &ExpectedCallableContract,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if Self::can_use_expected_callable_context(node) {
            let expected = self.expected_callable_ty(contract);
            self.check_node_with_expected(node, Some(&expected))
        } else {
            self.check_operator_compose_callable(node, op_name)
        }
    }

    fn expected_unary_function_parts(&mut self, expected: Option<&Ty>) -> Option<(Ty, Ty)> {
        let expected = self.resolve_ty(expected?);
        let Ty::Func(params, ret) = expected else {
            return None;
        };
        if params.len() == 1 {
            Some((self.resolve_ty(&params[0]), self.resolve_ty(ret.as_ref())))
        } else {
            None
        }
    }

    fn context_payload_ty(&mut self, ty: &Ty) -> Option<Ty> {
        match self.resolve_ty(ty) {
            Ty::Result(ok, _) => Some(self.resolve_ty(ok.as_ref())),
            Ty::List(item) => Some(self.resolve_ty(item.as_ref())),
            Ty::Enum(name, args) if Self::surface_name(&name) == "Option" && args.len() == 1 => {
                Some(self.resolve_ty(&args[0]))
            }
            _ => None,
        }
    }

    fn map_rhs_output_from_expected(
        &mut self,
        receiver_ty: &Ty,
        expected: Option<&Ty>,
    ) -> Option<Ty> {
        let expected = self.resolve_ty(expected?);
        match (self.resolve_ty(receiver_ty), expected) {
            (Ty::Result(_, _), Ty::Result(ok, _)) => Some(self.resolve_ty(ok.as_ref())),
            (Ty::List(_), Ty::List(item)) => Some(self.resolve_ty(item.as_ref())),
            (Ty::Enum(receiver_name, _), Ty::Enum(expected_name, expected_args))
                if Self::surface_name(&receiver_name) == "Option"
                    && Self::surface_name(&expected_name) == "Option"
                    && expected_args.len() == 1 =>
            {
                Some(self.resolve_ty(&expected_args[0]))
            }
            _ => None,
        }
    }

    fn pending_segment_from_syntax(segment: &ResolvedFacetPathSegment) -> PendingFacetSegment {
        match segment {
            ResolvedFacetPathSegment::Field { name, optional } => PendingFacetSegment::Field {
                name: name.clone(),
                optional: *optional,
            },
            ResolvedFacetPathSegment::Bracket(expr) => match expr.expr.as_ref() {
                Resolved::RangeLiteral(_, start, end) => PendingFacetSegment::RangeBracket {
                    start: PendingFacetExpr::Resolved(start.clone()),
                    end: PendingFacetExpr::Resolved(end.clone()),
                    display: expr.display.clone(),
                },
                _ => PendingFacetSegment::Bracket {
                    expr: PendingFacetExpr::Resolved(expr.expr.clone()),
                    display: expr.display.clone(),
                },
            },
        }
    }

    fn pending_field_segment(name: impl Into<String>) -> PendingFacetSegment {
        PendingFacetSegment::Field {
            name: name.into(),
            optional: false,
        }
    }

    fn pending_segment_display(segment: &PendingFacetSegment) -> String {
        match segment {
            PendingFacetSegment::Field { name, optional } => {
                if *optional {
                    format!("{name}?")
                } else {
                    name.clone()
                }
            }
            PendingFacetSegment::Bracket { display, .. }
            | PendingFacetSegment::RangeBracket { display, .. } => format!("[{display}]"),
        }
    }

    fn inferred_facet_capture_display(segments: &[ResolvedFacetPathSegment]) -> String {
        if segments.is_empty() {
            "_".to_string()
        } else {
            format!(
                "_.{}",
                segments
                    .iter()
                    .map(|segment| match segment {
                        ResolvedFacetPathSegment::Field { name, optional } => {
                            if *optional {
                                format!("{name}?")
                            } else {
                                name.clone()
                            }
                        }
                        ResolvedFacetPathSegment::Bracket(expr) => format!("[{}]", expr.display),
                    })
                    .collect::<Vec<_>>()
                    .join(".")
            )
        }
    }

    fn inferred_capture_body(
        span: &Span,
        param_id: &ResolvedId,
        segments: &[ResolvedFacetPathSegment],
    ) -> Resolved {
        segments.iter().fold(
            Resolved::Var(span.clone(), param_id.clone()),
            |expr, segment| match segment {
                ResolvedFacetPathSegment::Field {
                    name,
                    optional: false,
                } => Resolved::FieldAccess(span.clone(), Box::new(expr), name.clone()),
                other => Resolved::FacetSegmentAccess(span.clone(), Box::new(expr), other.clone()),
            },
        )
    }

    fn check_inferred_facet_capture(
        &mut self,
        span: &Span,
        segments: &[ResolvedFacetPathSegment],
        expected_ty: &Ty,
    ) -> Result<TypedNode, TypeError> {
        let expected_ty = self.resolve_ty(expected_ty);
        let Ty::Func(params, _) = &expected_ty else {
            return Err(TypeError {
                message: format!(
                    "{} requires expected unary function context",
                    Self::inferred_facet_capture_display(segments)
                ),
                span: span.clone(),
                hint: Some(format!(
                    "Expected a function type like `(A -> B)`, got {}. Use an explicit capture such as `&Type.field` when no context is available.",
                    self.ty_name(&expected_ty)
                )),
            });
        };
        if params.len() != 1 {
            return Err(TypeError {
                message: format!(
                    "{} requires expected unary function context",
                    Self::inferred_facet_capture_display(segments)
                ),
                span: span.clone(),
                hint: Some(format!(
                    "Expected a one-argument function type, got {}.",
                    self.ty_name(&expected_ty)
                )),
            });
        }

        let param_id = ResolvedId {
            name: "__inferred_facet_capture_arg".to_string(),
            qualified_name: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: span.clone(),
        };
        let body = Self::inferred_capture_body(span, &param_id, segments);
        let synthetic = Resolved::Closure(
            span.clone(),
            vec![ResolvedClosureParam {
                id: param_id,
                ty: None,
            }],
            Vec::new(),
            Box::new(body),
        );
        let typed = self.check_node_with_expected(&synthetic, Some(&expected_ty))?;
        if !self.types_compatible(&expected_ty, &typed.ty) {
            return Err(TypeError {
                message: format!(
                    "{} inferred type mismatch: expected {}, got {}",
                    Self::inferred_facet_capture_display(segments),
                    self.ty_name(&expected_ty),
                    self.ty_name(&typed.ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(typed)
    }

    fn is_function_on_callee(&self, func: &Resolved) -> bool {
        matches!(
            func,
            Resolved::Var(_, id)
                if id.name == "on"
                    || id.name == "Function::on"
                    || id.qualified_name.as_deref() == Some("Function::on")
        )
    }

    fn check_function_on_with_expected(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        expected_ty: &Ty,
    ) -> Result<TypedNode, TypeError> {
        let expected_ty = self.resolve_ty(expected_ty);
        let Ty::Func(params, ret) = &expected_ty else {
            return self.check_app(span, func, args);
        };
        if params.len() != 2 || !self.types_compatible(&params[0], &params[1]) {
            return self.check_app(span, func, args);
        }
        if args.len() != 2
            || args
                .iter()
                .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return self.check_app(span, func, args);
        }

        let ResolvedRecordLitArg::Positional(compare_expr) = &args[0] else {
            unreachable!("named arguments rejected above")
        };
        let ResolvedRecordLitArg::Positional(key_expr) = &args[1] else {
            unreachable!("named arguments rejected above")
        };

        let source_ty = self.resolve_ty(&params[0]);
        let key_ty = self.env.fresh_tyvar();
        let key_expected = Ty::Func(vec![source_ty.clone()], Box::new(key_ty.clone()));
        let typed_key = self.check_node_with_expected(key_expr, Some(&key_expected))?;
        if !self.types_compatible(&key_expected, &typed_key.ty) {
            return Err(TypeError {
                message: format!(
                    "Function::on key type mismatch: expected {}, got {}",
                    self.ty_name(&key_expected),
                    self.ty_name(&typed_key.ty)
                ),
                span: typed_key.span.clone(),
                hint: None,
            });
        }

        let key_ty = match self.resolve_ty(&typed_key.ty) {
            Ty::Func(_, focus) => focus.as_ref().clone(),
            other => {
                return Err(TypeError {
                    message: format!(
                        "Function::on key must be unary, got {}",
                        self.ty_name(&other)
                    ),
                    span: typed_key.span.clone(),
                    hint: None,
                })
            }
        };
        let compare_expected = Ty::Func(
            vec![self.resolve_ty(&key_ty), self.resolve_ty(&key_ty)],
            Box::new(self.resolve_ty(ret.as_ref())),
        );
        let typed_compare = self.check_node_with_expected(compare_expr, Some(&compare_expected))?;
        if !self.types_compatible(&compare_expected, &typed_compare.ty) {
            return Err(TypeError {
                message: format!(
                    "Function::on comparator type mismatch: expected {}, got {}",
                    self.ty_name(&compare_expected),
                    self.ty_name(&typed_compare.ty)
                ),
                span: typed_compare.span.clone(),
                hint: None,
            });
        }

        let typed_func = self.check_node(func)?;
        let result_ty = Ty::Func(
            vec![source_ty.clone(), source_ty],
            Box::new(self.resolve_ty(ret)),
        );
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::App(Box::new(typed_func), vec![typed_compare, typed_key]),
        })
    }

    pub(super) fn check_function_value_operand(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        let typed = match self.check_node(node) {
            Ok(typed) => typed,
            Err(err) => return Err(self.remap_compose_operand_error(node, op_name, err)),
        };
        if matches!(self.resolve_ty(&typed.ty), Ty::Func(_, _)) {
            Ok(typed)
        } else {
            let span = typed.span.clone();
            let hint = self.compose_function_value_hint(&typed, op_name);
            Err(TypeError {
                message: format!("{} requires a function value", op_name),
                span,
                hint: Some(hint),
            })
        }
    }

    fn remap_compose_operand_error(
        &self,
        node: &Resolved,
        op_name: &str,
        err: TypeError,
    ) -> TypeError {
        let Resolved::App(_, func, args) = node else {
            return err;
        };

        let Some(name) = self.compose_operand_callee_name(func) else {
            return err;
        };
        let arity = args.len();
        let is_arity_error = err.message.contains("expects")
            && err.message.contains("argument(s)")
            && err.message.contains(", got ");
        if !is_arity_error {
            return err;
        }

        TypeError {
            message: format!("Undefined function {}/{}", name, arity),
            span: self.resolved_span(func).clone(),
            hint: Some(format!(
                "{} composes one-argument function values. `{}` is being called with arity {}, but no callable with that arity is available here.",
                op_name, name, arity
            )),
        }
    }

    fn compose_operand_callee_name(&self, node: &Resolved) -> Option<String> {
        match node {
            Resolved::Var(_, id) => Some(id.name.clone()),
            Resolved::Grouped(_, inner) => self.compose_operand_callee_name(inner),
            _ => None,
        }
    }

    fn compose_call_signature_hint(&self, func: &TypedNode) -> Option<String> {
        match (&func.node, self.resolve_ty(&func.ty)) {
            (TypedInner::Var(id), Ty::BuiltinFunc { params, ret, .. })
            | (TypedInner::Var(id), Ty::UserFunc { params, ret, .. }) => {
                Some(self.call_target_signature_hint_for_id(id, &params, ret.as_ref()))
            }
            (_, Ty::BuiltinFunc { name, params, ret }) => {
                Some(self.call_target_signature_hint(&name, &params, ret.as_ref(), None))
            }
            (_, Ty::UserFunc { params, ret, .. }) => {
                self.callable_definition_signature_hint(func, &params, ret.as_ref())
            }
            (_, Ty::Func(params, ret)) => {
                self.callable_signature_hint(&Ty::Func(params.clone(), ret.clone()))
            }
            _ => None,
        }
    }

    fn argument_type_mismatch_message(&mut self, expected: &Ty, actual: &Ty) -> String {
        if let Some(message) = self.bound_mismatch_message(expected, actual) {
            return message;
        }
        format!(
            "Argument type mismatch: expected {}, got {}",
            self.ty_name(expected),
            self.ty_name(actual)
        )
    }

    fn bound_mismatch_message(&mut self, expected: &Ty, actual: &Ty) -> Option<String> {
        match (self.resolve_ty(expected), self.resolve_ty(actual)) {
            (Ty::Var(var), actual_ty) => {
                let bounds = self.tyvar_bound_names(var);
                if !bounds.is_empty() && !self.ty_satisfies_bounds(&actual_ty, &bounds) {
                    Some(format!(
                        "Argument type mismatch: expected {} implementing {}, got {}",
                        self.ty_name(expected),
                        bounds.join(" + "),
                        self.ty_name(&actual_ty)
                    ))
                } else {
                    None
                }
            }
            (Ty::List(expected_inner), Ty::List(actual_inner))
            | (Ty::TypeRef(expected_inner), Ty::TypeRef(actual_inner)) => {
                self.bound_mismatch_message(&expected_inner, &actual_inner)
            }
            (Ty::Tuple(expected_items), Ty::Tuple(actual_items)) => expected_items
                .iter()
                .zip(actual_items.iter())
                .find_map(|(expected_item, actual_item)| {
                    self.bound_mismatch_message(expected_item, actual_item)
                }),
            (Ty::Func(expected_params, expected_ret), Ty::Func(actual_params, actual_ret)) => {
                expected_params
                    .iter()
                    .zip(actual_params.iter())
                    .find_map(|(expected_param, actual_param)| {
                        self.bound_mismatch_message(expected_param, actual_param)
                    })
                    .or_else(|| self.bound_mismatch_message(&expected_ret, &actual_ret))
            }
            (Ty::Result(expected_ok, expected_err), Ty::Result(actual_ok, actual_err)) => self
                .bound_mismatch_message(&expected_ok, &actual_ok)
                .or_else(|| self.bound_mismatch_message(&expected_err, &actual_err)),
            _ => None,
        }
    }

    fn compose_function_value_hint(&self, typed: &TypedNode, op_name: &str) -> String {
        if let TypedInner::App(func, _) = &typed.node {
            let signature = self
                .compose_call_signature_hint(func)
                .unwrap_or_else(|| "Call target signature: <unknown>".into());
            format!(
                "{}\n`{}` evaluates this call before composition; the result type {} is not a function value.",
                signature,
                op_name,
                self.ty_name(&typed.ty)
            )
        } else {
            format!(
                "{} works on one-argument function values. Bare function names are not function values; use `&name`, a closure, a function-typed variable, or a call that returns a function value.",
                op_name
            )
        }
    }

    fn tuple_index_hint(tuple_len: usize) -> String {
        if tuple_len == 0 {
            "This tuple has 0 elements, so no tuple selectors are available.".into()
        } else {
            let selectors = (0..tuple_len)
                .map(|idx| format!("._{}", idx))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "This tuple has {} element(s); valid selectors are {}.",
                tuple_len, selectors
            )
        }
    }

    pub(super) fn resolved_span<'a>(&self, node: &'a Resolved) -> &'a Span {
        match node {
            Resolved::Lit(span, _)
            | Resolved::Var(span, _)
            | Resolved::App(span, _, _)
            | Resolved::Block(span, _)
            | Resolved::Bind(span, _, _)
            | Resolved::SafeBind(span, _, _)
            | Resolved::BinOp(span, _, _, _)
            | Resolved::Pipe(span, _, _)
            | Resolved::ContextMap(span, _, _)
            | Resolved::ContextBind(span, _, _)
            | Resolved::Compose(span, _, _)
            | Resolved::LiftedCompose(span, _, _)
            | Resolved::KleisliCompose(span, _, _)
            | Resolved::ListNil(span)
            | Resolved::ListCons(span, _, _)
            | Resolved::ListLiteral(span, _)
            | Resolved::RangeLiteral(span, _, _)
            | Resolved::TupleLiteral(span, _)
            | Resolved::Grouped(span, _)
            | Resolved::InterpolatedStr(span, _)
            | Resolved::Dbg(span, _)
            | Resolved::If(span, _, _, _)
            | Resolved::Assert(span, _, _)
            | Resolved::Ensure(span, _, _, _)
            | Resolved::MapErr(span, _, _)
            | Resolved::Cause(span, _, _)
            | Resolved::RecoverKind(span, _, _, _)
            | Resolved::Match(span, _, _)
            | Resolved::FieldAccess(span, _, _)
            | Resolved::FacetSegmentAccess(span, _, _)
            | Resolved::InferredFacetCapture(span, _)
            | Resolved::FacetCapture(span, _)
            | Resolved::ProcessContextHandler(span, _)
            | Resolved::StructLit(span, _, _)
            | Resolved::ConstructorCall(span, _, _)
            | Resolved::TypeRefWitness(span, _)
            | Resolved::StructDef(span, ..)
            | Resolved::RecordDef(span, _, _)
            | Resolved::DeferrorDef(span, _, _, _)
            | Resolved::EnumDef(span, _, _, _, _)
            | Resolved::Def(span, _, _, _, _, _, _)
            | Resolved::ConstDef(span, _, _, _, _)
            | Resolved::ExtractorDef(span, _, _, _, _, _, _)
            | Resolved::BuiltinDecl(span, _, _, _, _)
            | Resolved::BuiltinExtractorDecl(span, _, _, _, _)
            | Resolved::BuiltinTypeDecl(span, _, _, _)
            | Resolved::ResultCtorDecl(span, _, _, _, _)
            | Resolved::TraitDef(span, _, _, _, _)
            | Resolved::TraitImplDef(span, _, _, _, _)
            | Resolved::Closure(span, _, _, _)
            | Resolved::Capture(span, _, _)
            | Resolved::Semi(span, _) => span,
        }
    }

    pub(super) fn function_parts<'a>(&'a self, ty: &'a Ty) -> Option<(&'a [Ty], &'a Ty)> {
        match ty {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                Some((params.as_slice(), ret.as_ref()))
            }
            Ty::Func(params, ret) => Some((params.as_slice(), ret.as_ref())),
            _ => None,
        }
    }

    pub(super) fn callable_signature_from_parts(&self, params: &[Ty], ret: &Ty) -> String {
        let param_str = params
            .iter()
            .map(|ty| self.ty_name(ty))
            .collect::<Vec<_>>()
            .join(", ");
        if param_str.is_empty() {
            format!("(-> {})", self.ty_name(ret))
        } else {
            format!("({} -> {})", param_str, self.ty_name(ret))
        }
    }

    pub(super) fn callable_signature_for_ty(&self, ty: &Ty) -> Option<String> {
        self.function_parts(ty)
            .map(|(params, ret)| self.callable_signature_from_parts(params, ret))
    }

    pub(super) fn callable_signature_hint(&self, ty: &Ty) -> Option<String> {
        self.callable_signature_for_ty(ty)
            .map(|sig| format!("Callable type signature: {}", sig))
    }

    pub(super) fn call_target_signature_hint(
        &self,
        display_name: &str,
        params: &[Ty],
        ret: &Ty,
        first_param_name: Option<&str>,
    ) -> String {
        let param_list = params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let name = match (idx, first_param_name) {
                    (0, Some(name)) => name.to_string(),
                    _ => format!("arg{}", idx + 1),
                };
                format!("{}: {}", name, self.ty_name(ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Call target signature: {}({}) -> {}",
            display_name,
            param_list,
            self.ty_name(ret)
        )
    }

    pub(super) fn callable_target_display_name(&self, id: &ResolvedId) -> String {
        if let Some(qualified_name) = id.qualified_name.as_deref() {
            return callable_definition_display_name(qualified_name, &id.name);
        }
        if sindr::builtin::builtin_meta_by_name(&id.name).is_some() {
            return format!("Kernel::{}", id.name);
        }
        id.name.clone()
    }

    pub(super) fn call_target_signature_hint_for_id(
        &self,
        id: &ResolvedId,
        params: &[Ty],
        ret: &Ty,
    ) -> String {
        let display_name = self.callable_target_display_name(id);
        self.call_target_signature_hint(&display_name, params, ret, None)
    }

    pub(super) fn trait_method_signature_hint(
        &self,
        trait_display_name: &str,
        method_name: &str,
        params: &[Ty],
        ret: &Ty,
    ) -> String {
        self.call_target_signature_hint(
            &format!("{}::{}", trait_display_name, method_name),
            params,
            ret,
            Some("self"),
        )
    }

    pub(super) fn callable_definition_signature_hint(
        &self,
        func: &TypedNode,
        params: &[Ty],
        ret: &Ty,
    ) -> Option<String> {
        let TypedInner::Var(id) = &func.node else {
            return None;
        };
        let qualified_name = id.qualified_name.as_deref().unwrap_or(&id.name);
        let display_name = callable_definition_display_name(qualified_name, &id.name);
        let mut hint = format!(
            "Callable definition signature: {}",
            self.callable_definition_signature(&display_name, id.unique_id, params, ret)
        );
        if let Some(def_span) = self
            .function_ids_by_name
            .values()
            .find(|decl| decl.unique_id == id.unique_id)
            .map(|decl| decl.span.clone())
        {
            hint.push_str(&format!(
                "\nCallable definition span: {}..{}",
                def_span.start, def_span.end
            ));
        }
        Some(hint)
    }

    fn callable_definition_signature(
        &self,
        qualified_name: &str,
        uid: u32,
        params: &[Ty],
        ret: &Ty,
    ) -> String {
        let param_names = self.user_func_params.get(&uid);
        let param_list = params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let name = param_names
                    .and_then(|names| names.get(idx))
                    .map(String::as_str)
                    .unwrap_or("_");
                format!("{}: {}", name, self.ty_name(ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let ret = self.ty_name(ret);

        if let Some(display) = trait_impl_signature_display(qualified_name, &param_list, &ret) {
            return display;
        }

        format!("{}({}) -> {}", qualified_name, param_list, ret)
    }

    pub(super) fn operator_type_display(&self, ty: &Ty) -> String {
        self.callable_signature_for_ty(ty)
            .unwrap_or_else(|| self.ty_name(ty))
    }

    pub(super) fn operator_rule_hint(
        &self,
        op_name: &str,
        rule: &str,
        lhs_ty: &Ty,
        rhs_ty: &Ty,
        extra: Option<String>,
    ) -> String {
        let mut hint = format!(
            "{} signature rule: {}. LHS: {}. RHS: {}. Operators share precedence and resolve left-to-right, so LHS is the type produced so far.",
            op_name,
            rule,
            self.operator_type_display(lhs_ty),
            self.operator_type_display(rhs_ty)
        );
        if let Some(extra) = extra {
            hint.push(' ');
            hint.push_str(&extra);
        }
        hint
    }

    pub(super) fn unary_function_parts(
        &self,
        ty: &Ty,
        op_name: &str,
        span: &Span,
    ) -> Result<(Ty, Ty), TypeError> {
        let Some((params, ret)) = self.function_parts(ty) else {
            return Err(TypeError {
                message: format!("{} expects a function value", op_name),
                span: span.clone(),
                hint: None,
            });
        };
        if params.len() != 1 {
            return Err(TypeError {
                message: format!("{} expects a unary callable", op_name),
                span: span.clone(),
                hint: Some(format!(
                    "{} can compose/apply only one-argument callables. Callable type signature: {}",
                    op_name,
                    self.callable_signature_from_parts(params, ret)
                )),
            });
        }
        Ok((self.resolve_ty(&params[0]), self.resolve_ty(ret)))
    }

    pub(super) fn trait_method_ref<'a>(
        &self,
        func: &'a Resolved,
    ) -> Option<(&'a ResolvedId, String, String)> {
        let Resolved::Var(_, id) = func else {
            return None;
        };
        let qualified_name = id.qualified_name.as_ref()?;
        let (trait_name, method_name) = self
            .trait_methods_by_qualified_name
            .get(qualified_name)?
            .clone();
        Some((id, trait_name, method_name))
    }

    pub(super) fn trait_dispatch_target(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
    ) -> Option<TraitDispatch> {
        self.trait_dispatch_target_for_args(trait_name, method_name, receiver_ty, &[])
    }

    pub(super) fn trait_dispatch_target_for_args(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: &[Ty],
    ) -> Option<TraitDispatch> {
        let profile = self.profiler.start();
        let receiver_ty = self.resolve_ty(receiver_ty);
        let result = match receiver_ty {
            Ty::Var(var) => {
                if self.tyvar_has_bound(var, trait_name)
                    || self.tyvar_satisfies_compiler_trait(var, trait_name)
                {
                    Some(TraitDispatch::Pending)
                } else {
                    None
                }
            }
            concrete => {
                if let Some(target_name) = self.trait_target_name(&concrete) {
                    if let Some(impl_info) = self
                        .trait_impls
                        .get(&(trait_name.into(), target_name.clone()))
                    {
                        let method = impl_info.methods.get(method_name)?;

                        if let Some(dispatch_override) = &method.dispatch_override {
                            return Some(TraitDispatch::Static(dispatch_override.clone()));
                        }
                        let function_key = method
                            .function_id
                            .qualified_name
                            .as_ref()
                            .unwrap_or(&method.function_id.name);
                        let function_id = self.function_ids_by_name.get(function_key)?;
                        let function_ty = self.env.lookup_var(function_id.unique_id)?;
                        let Ty::UserFunc { fun_idx, .. } = function_ty else {
                            return None;
                        };
                        let display_name =
                            method
                                .display_name_override
                                .clone()
                                .or_else(|| {
                                    method.function_id.qualified_name.as_deref().map(
                                        |qualified_name| {
                                            callable_definition_display_name(
                                                qualified_name,
                                                &method.function_id.name,
                                            )
                                        },
                                    )
                                })
                                .unwrap_or_else(|| {
                                    Checker::surface_name(&method.function_id.name).into()
                                });
                        return Some(TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                            name: display_name,
                            fun_idx: *fun_idx,
                        }));
                    }
                }
                if let Some(dispatch) = self.generic_trait_dispatch_target(
                    trait_name,
                    method_name,
                    &concrete,
                    requested_trait_args,
                ) {
                    return Some(dispatch);
                }
                self.compiler_trait_dispatch_target(trait_name, method_name, &concrete)
                    .map(TraitDispatch::Static)
            }
        };
        self.profiler
            .finish(ProfileEvent::TraitDispatchLookup, profile);
        result
    }

    fn generic_trait_dispatch_target(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: &[Ty],
    ) -> Option<TraitDispatch> {
        let profile = self.profiler.start();
        let result = (|| {
            for impl_key in self.trait_impl_candidate_keys(trait_name) {
                let Some(impl_info) = self.trait_impls.get(&impl_key).cloned() else {
                    continue;
                };
                let Some(method) = impl_info.methods.get(method_name) else {
                    continue;
                };
                if impl_info.trait_arg_tys.len() != requested_trait_args.len() {
                    continue;
                }
                let mut fresh = HashMap::new();
                let impl_target = self.instantiate_ty_with_fresh(&impl_info.target_ty, &mut fresh);
                let impl_trait_args = impl_info
                    .trait_arg_tys
                    .iter()
                    .map(|ty| self.instantiate_ty_with_fresh(ty, &mut fresh))
                    .collect::<Vec<_>>();
                let before = self.substitutions.clone();
                let target_matches = self.types_compatible(&impl_target, receiver_ty);
                let args_match = target_matches
                    && impl_trait_args
                        .iter()
                        .zip(requested_trait_args.iter())
                        .all(|(expected, actual)| self.types_compatible(expected, actual));
                if !args_match {
                    self.substitutions = before;
                    continue;
                }

                if let Some(dispatch_override) = &method.dispatch_override {
                    return Some(TraitDispatch::Static(dispatch_override.clone()));
                }
                let function_key = method
                    .function_id
                    .qualified_name
                    .as_ref()
                    .unwrap_or(&method.function_id.name);
                let function_id = self.function_ids_by_name.get(function_key)?;
                let function_ty = self.env.lookup_var(function_id.unique_id)?;
                let Ty::UserFunc { fun_idx, .. } = function_ty else {
                    return None;
                };
                let display_name = method
                    .display_name_override
                    .clone()
                    .or_else(|| {
                        method
                            .function_id
                            .qualified_name
                            .as_deref()
                            .map(|qualified_name| {
                                callable_definition_display_name(
                                    qualified_name,
                                    &method.function_id.name,
                                )
                            })
                    })
                    .unwrap_or_else(|| Checker::surface_name(&method.function_id.name).into());
                return Some(TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                    name: display_name,
                    fun_idx: *fun_idx,
                }));
            }
            None
        })();
        self.profiler
            .finish(ProfileEvent::GenericTraitCandidateScan, profile);
        result
    }

    fn operator_trait_dispatch_for_args(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: &[Ty],
    ) -> Option<(TraitDispatch, Vec<Ty>)> {
        let profile = self.profiler.start();
        let result = (|| {
            for impl_key in self.trait_impl_candidate_keys(trait_name) {
                let Some(impl_info) = self.trait_impls.get(&impl_key).cloned() else {
                    continue;
                };
                let Some(method) = impl_info.methods.get(method_name) else {
                    continue;
                };
                if impl_info.trait_arg_tys.len() != requested_trait_args.len() {
                    continue;
                }

                let mut fresh = HashMap::new();
                let impl_target = self.instantiate_ty_with_fresh(&impl_info.target_ty, &mut fresh);
                let impl_trait_args = impl_info
                    .trait_arg_tys
                    .iter()
                    .map(|ty| self.instantiate_ty_with_fresh(ty, &mut fresh))
                    .collect::<Vec<_>>();

                let before = self.substitutions.clone();
                let target_matches = self.types_compatible(&impl_target, receiver_ty);
                let args_match = target_matches
                    && impl_trait_args
                        .iter()
                        .zip(requested_trait_args.iter())
                        .all(|(expected, actual)| self.types_compatible(expected, actual));
                if !args_match {
                    self.substitutions = before;
                    continue;
                }
                let resolved_trait_args = requested_trait_args
                    .iter()
                    .map(|ty| self.resolve_ty(ty))
                    .collect::<Vec<_>>();

                if let Some(dispatch_override) = &method.dispatch_override {
                    return Some((
                        TraitDispatch::Static(dispatch_override.clone()),
                        resolved_trait_args,
                    ));
                }
                let function_key = method
                    .function_id
                    .qualified_name
                    .as_ref()
                    .unwrap_or(&method.function_id.name);
                let function_id = self.function_ids_by_name.get(function_key)?;
                let function_ty = self.env.lookup_var(function_id.unique_id)?;
                let Ty::UserFunc { fun_idx, .. } = function_ty else {
                    self.substitutions = before;
                    continue;
                };
                let display_name = method
                    .function_id
                    .qualified_name
                    .as_deref()
                    .map(|qualified_name| {
                        callable_definition_display_name(qualified_name, &method.function_id.name)
                    })
                    .unwrap_or_else(|| Checker::surface_name(&method.function_id.name).into());
                return Some((
                    TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                        name: display_name,
                        fun_idx: *fun_idx,
                    }),
                    resolved_trait_args,
                ));
            }
            None
        })();
        self.profiler
            .finish(ProfileEvent::OperatorTraitCandidateScan, profile);
        result
    }

    fn opposite_conversion_hint(
        &self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        typed_args: &[TypedNode],
        span: &Span,
    ) -> Option<TypeError> {
        let requested_trait = if self.trait_matches_short_name(trait_name, "From") {
            "From"
        } else if self.trait_matches_short_name(trait_name, "TryFrom") {
            "TryFrom"
        } else {
            return None;
        };
        let opposite_trait = if requested_trait == "From" {
            "TryFrom"
        } else {
            "From"
        };
        let receiver_name = self.trait_target_name(receiver_ty)?;
        let witness_ty = self.resolve_ty(&typed_args.get(1)?.ty);
        let Ty::TypeRef(target_ty) = witness_ty else {
            return None;
        };
        let opposite_trait_key = self.trait_key_by_short_name(opposite_trait)?;
        let opposite_instance_key =
            self.trait_instance_key_from_tys(&opposite_trait_key, &[target_ty.as_ref().clone()]);
        if !self
            .trait_impls
            .contains_key(&(opposite_instance_key, receiver_name.clone()))
        {
            return None;
        }

        let target_name = self.ty_name(&target_ty);
        let opposite_method = if opposite_trait == "From" {
            "from"
        } else {
            "try_from"
        };
        Some(TypeError {
            message: format!(
                "{} -> {} implements {}, not {}. Use {}(value, {}).",
                receiver_name,
                target_name,
                opposite_trait,
                requested_trait,
                opposite_method,
                target_name
            ),
            span: span.clone(),
            hint: Some(format!(
                "{}::{} is not available for this conversion pair.",
                self.trait_display_name(trait_name),
                method_name
            )),
        })
    }

    pub(super) fn check_trait_method_call(
        &mut self,
        span: &Span,
        trait_name: &str,
        method_name: &str,
        args: &[ResolvedRecordLitArg],
        receiver_owner_hint: Option<&str>,
    ) -> Result<TypedNode, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!(
                    "{}::{} does not accept named arguments",
                    trait_name, method_name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let trait_info = self
            .traits
            .get(trait_name)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Unknown trait: {}", trait_name),
                span: span.clone(),
                hint: None,
            })?;
        let method = trait_info
            .methods
            .get(method_name)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Unknown trait method: {}::{}", trait_name, method_name),
                span: span.clone(),
                hint: None,
            })?;

        let self_ty = self.env.fresh_tyvar();
        let (param_tys, ret_ty, trait_arg_tys) =
            self.resolve_trait_method_signature(&trait_info, &method, &self_ty)?;

        let trait_display_name = self.trait_display_name(trait_name);
        let trait_impl_summary = self.trait_implementation_summary(trait_name);
        let trait_signature_hint = |checker: &Self| {
            let params = param_tys
                .iter()
                .map(|ty| checker.resolve_ty(ty))
                .collect::<Vec<_>>();
            let ret = checker.resolve_ty(&ret_ty);
            checker.trait_method_signature_hint(&trait_display_name, method_name, &params, &ret)
        };
        let trait_hint = |checker: &Self| {
            combine_hint_parts(&[
                Some(trait_signature_hint(checker)),
                Some(trait_impl_summary.clone()),
            ])
        };

        if args.len() != param_tys.len() {
            return Err(TypeError {
                message: format!(
                    "{}::{} expects {} argument(s), got {}",
                    trait_name,
                    method_name,
                    param_tys.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: Some(trait_signature_hint(self)),
            });
        }

        let typed_args = args
            .iter()
            .zip(param_tys.iter())
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.ensure_no_runtime_facet_args(&typed_args, span, "Trait method call")?;

        if let Some(owner_hint) = receiver_owner_hint {
            if let Some(receiver) = typed_args.first() {
                let receiver_ty = self.resolve_ty(&receiver.ty);
                if let Some(receiver_name) = self.trait_target_name(&receiver_ty) {
                    if Self::surface_name(&receiver_name) != owner_hint {
                        return Err(TypeError {
                            message: format!(
                                "{}::{} helper requires receiver type {}, got {}",
                                owner_hint,
                                method_name,
                                owner_hint,
                                self.ty_name(&receiver_ty)
                            ),
                            span: receiver.span.clone(),
                            hint: Some(format!(
                                "Use {}::{} only for {} values.",
                                owner_hint, method_name, owner_hint
                            )),
                        });
                    }
                }
            }
        }

        for (idx, (expected, arg)) in param_tys.iter().zip(&typed_args).enumerate() {
            if !self.types_compatible(expected, &arg.ty) {
                if typed_args.len() == 2 {
                    let left_ty = self.ty_name(&typed_args[0].ty);
                    let right_ty = self.ty_name(&typed_args[1].ty);
                    if self.trait_matches_short_name(trait_name, "Eq") && method_name == "eq" {
                        return Err(TypeError {
                            message: format!(
                                "Eq::eq helper cannot compare {} and {}",
                                left_ty, right_ty
                            ),
                            span: arg.span.clone(),
                            hint: trait_hint(self),
                        });
                    }
                    if self.trait_matches_short_name(trait_name, "Neq") && method_name == "neq" {
                        return Err(TypeError {
                            message: format!(
                                "Neq::neq helper cannot compare {} and {}",
                                left_ty, right_ty
                            ),
                            span: arg.span.clone(),
                            hint: trait_hint(self),
                        });
                    }
                    if self.trait_matches_short_name(trait_name, "Compare")
                        && method_name == "compare"
                    {
                        return Err(TypeError {
                            message: format!(
                                "Compare::compare helper cannot compare {} and {}",
                                left_ty, right_ty
                            ),
                            span: arg.span.clone(),
                            hint: trait_hint(self),
                        });
                    }
                    if self.trait_matches_short_name(trait_name, "Concat") {
                        return Err(TypeError {
                            message: format!(
                                "Concat::concat helper requires String on both sides, but got {} and {}",
                                left_ty, right_ty,
                            ),
                            span: arg.span.clone(),
                            hint: trait_hint(self),
                        });
                    }
                }
                let receiver_ty = self.resolve_ty(&self_ty);
                if !matches!(receiver_ty, Ty::Var(_))
                    && self.trait_impl_exists(trait_name, &receiver_ty)
                {
                    return Err(TypeError {
                        message: format!(
                            "{}::{} expects argument {} to match receiver type {}, got {}",
                            trait_display_name,
                            method_name,
                            idx + 1,
                            self.ty_name(&receiver_ty),
                            self.ty_name(&arg.ty)
                        ),
                        span: arg.span.clone(),
                        hint: trait_hint(self),
                    });
                }
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch in {}::{}: expected {}, got {}",
                        trait_display_name,
                        method_name,
                        self.ty_name(expected),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: trait_hint(self),
                });
            }
        }

        let trait_call_name = self.trait_instance_key_from_tys(trait_name, &trait_arg_tys);
        let trait_call_display_name = self.trait_display_name(&trait_call_name);
        let trait_call_summary = self.trait_implementation_summary(&trait_call_name);
        let receiver_ty = self.resolve_ty(&self_ty);
        if let Ty::Var(var) = receiver_ty {
            self.register_tyvar_bound(var, &trait_call_name);
        }
        let receiver_span = typed_args
            .first()
            .map(|arg| arg.span.clone())
            .unwrap_or_else(|| span.clone());

        if let Some(err) = self.opposite_conversion_hint(
            &trait_call_name,
            method_name,
            &receiver_ty,
            &typed_args,
            &receiver_span,
        ) {
            if self
                .trait_dispatch_target_for_args(
                    &trait_call_name,
                    method_name,
                    &receiver_ty,
                    &trait_arg_tys,
                )
                .is_none()
            {
                return Err(err);
            }
        }

        let receiver_ty = self.resolve_ty(&self_ty);
        let dispatch = self
            .trait_dispatch_target_for_args(
                &trait_call_name,
                method_name,
                &receiver_ty,
                &trait_arg_tys,
            )
            .ok_or_else(|| TypeError {
                message: format!(
                    "{}::{} requires a receiver type implementing {}, got {}",
                    trait_call_display_name,
                    method_name,
                    trait_call_display_name,
                    self.ty_name(&receiver_ty)
                ),
                span: receiver_span,
                hint: combine_hint_parts(&[
                    Some(trait_signature_hint(self)),
                    Some(trait_call_summary),
                ]),
            })?;

        Ok(TypedNode {
            ty: self.resolve_ty(&ret_ty),
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name: trait_call_name,
                method_name: method_name.into(),
                receiver_ty,
                dispatch,
                origin: TraitCallOrigin::Explicit,
                args: typed_args,
            },
        })
    }

    pub(super) fn check_injected_call(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!(
                    "{} does not support named arguments on the right-hand side",
                    op_name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_func = self.check_node(func)?;
        let (params, ret) = match self.resolve_ty(&typed_func.ty) {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                (params, ret.as_ref().clone())
            }
            Ty::Func(params, ret) => (params, ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "{} right-hand side is not a function call target: {}",
                        op_name,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: None,
                })
            }
        };

        if params.len() != args.len() + 1 {
            return Err(TypeError {
                message: format!(
                    "{} injects the left value as the first argument, so the call expects {} explicit argument(s), got {}",
                    op_name,
                    params.len().saturating_sub(1),
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_args: Vec<TypedNode> = args
            .iter()
            .zip(params.iter().skip(1))
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.ensure_no_runtime_facet_args(&typed_args, span, op_name)?;

        for (expected, arg) in params.iter().skip(1).zip(&typed_args) {
            if !matches!(self.resolve_ty(expected), Ty::Hole)
                && !self.types_compatible(expected, &arg.ty)
            {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(expected),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::Func(
                vec![self.resolve_ty(&params[0])],
                Box::new(self.resolve_ty(&ret)),
            ),
            span: span.clone(),
            node: TypedInner::InjectCall(Box::new(typed_func), typed_args),
        })
    }

    fn check_trait_helper_pipe_callable(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        input_ty: Ty,
        ret_ty: Option<Ty>,
        op_name: &str,
    ) -> Result<Option<TypedNode>, TypeError> {
        if self.trait_method_ref(func).is_none() {
            return Ok(None);
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!(
                    "{} does not support named arguments on the right-hand side",
                    op_name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let param_id = ResolvedId {
            name: "__pipe_trait_helper_arg".into(),
            qualified_name: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: span.clone(),
        };
        let mut body_args = Vec::with_capacity(args.len() + 1);
        body_args.push(ResolvedRecordLitArg::Positional(Resolved::Var(
            span.clone(),
            param_id.clone(),
        )));
        body_args.extend(args.iter().cloned());

        let synthetic = Resolved::Closure(
            span.clone(),
            vec![ResolvedClosureParam {
                id: param_id,
                ty: None,
            }],
            Vec::new(),
            Box::new(Resolved::App(
                span.clone(),
                Box::new(func.clone()),
                body_args,
            )),
        );
        let contract = self.callable_contract(&input_ty, ret_ty, ExpectedCallableSlot::Contextual);
        let expected = self.expected_callable_ty(&contract);
        self.check_node_with_expected(&synthetic, Some(&expected))
            .map(Some)
    }

    pub(super) fn ensure_plain_map_output(
        &self,
        output_ty: &Ty,
        op_name: &str,
        span: &Span,
    ) -> Result<(), TypeError> {
        match self.resolve_ty(output_ty) {
            Ty::Result(_, _) | Ty::List(_) => Err(TypeError {
                message: format!(
                    "{} expects a plain function on the right-hand side; use {} for contextual output",
                    op_name,
                    if op_name == "`>*`" { "`>=>`" } else { "`|>=`" }
                ),
                span: span.clone(),
                hint: None,
            }),
            Ty::Enum(name, _) if Self::surface_name(&name) == "Option" => Err(TypeError {
                message: format!(
                    "{} expects a plain function on the right-hand side; use {} for contextual output",
                    op_name,
                    if op_name == "`>*`" { "`>=>`" } else { "`|>=`" }
                ),
                span: span.clone(),
                hint: None,
            }),
            _ => Ok(()),
        }
    }

    fn flow_operator_trait_call(
        &mut self,
        span: &Span,
        trait_short_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: Vec<Ty>,
        op: OperatorTraitOp,
        args: Vec<TypedNode>,
        result_ty: Ty,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        let trait_key = self
            .trait_key_by_short_name(trait_short_name)
            .ok_or_else(|| TypeError {
                message: format!("Unknown trait: {}", trait_short_name),
                span: span.clone(),
                hint: None,
            })?;
        let Some((dispatch, resolved_trait_args)) = self.operator_trait_dispatch_for_args(
            &trait_key,
            method_name,
            receiver_ty,
            &requested_trait_args,
        ) else {
            let summary = self.trait_implementation_summary(trait_short_name);
            return Err(TypeError {
                message: format!(
                    "{} requires {} implementation on the left, got {}",
                    op_name,
                    trait_short_name,
                    self.ty_name(receiver_ty)
                ),
                span: span.clone(),
                hint: Some(summary),
            });
        };
        let trait_name = self.trait_instance_key_from_tys(&trait_key, &resolved_trait_args);
        let (lhs_ty, rhs_ty) = if op == OperatorTraitOp::PipeApply {
            (
                args.get(1)
                    .map(|arg| self.resolve_ty(&arg.ty))
                    .unwrap_or_else(|| Ty::Unit),
                args.first()
                    .map(|arg| self.resolve_ty(&arg.ty))
                    .unwrap_or_else(|| self.resolve_ty(receiver_ty)),
            )
        } else {
            (
                args.first()
                    .map(|arg| self.resolve_ty(&arg.ty))
                    .unwrap_or_else(|| self.resolve_ty(receiver_ty)),
                args.get(1)
                    .map(|arg| self.resolve_ty(&arg.ty))
                    .unwrap_or_else(|| Ty::Unit),
            )
        };
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name,
                method_name: method_name.into(),
                receiver_ty: self.resolve_ty(receiver_ty),
                dispatch,
                origin: TraitCallOrigin::Operator { op, lhs_ty, rhs_ty },
                args,
            },
        })
    }

    pub(super) fn check_pipe(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_pipe_with_expected(span, left, right, None)
    }

    fn check_pipe_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let rhs_ret_expected = expected.map(|ty| self.resolve_ty(ty));
        let typed_right = match right {
            Resolved::InferredFacetCapture(_, _)
            | Resolved::Capture(_, _, _)
            | Resolved::Closure(_, _, _, _)
            | Resolved::Grouped(_, _) => {
                let contract = self.callable_contract(
                    &typed_left.ty,
                    rhs_ret_expected.clone(),
                    ExpectedCallableSlot::Plain,
                );
                self.check_apply_callable_with_contract(right, &contract, "`|>`")?
            }
            Resolved::App(call_span, func, args) => {
                if let Some(typed) = self.check_trait_helper_pipe_callable(
                    call_span,
                    func,
                    args,
                    self.resolve_ty(&typed_left.ty),
                    rhs_ret_expected.clone(),
                    "`|>`",
                )? {
                    typed
                } else {
                    self.check_apply_callable(right, "`|>`")?
                }
            }
            _ => self.check_apply_callable(right, "`|>`")?,
        };
        let (param, ret) = self.unary_function_parts(&typed_right.ty, "`|>`", &typed_right.span)?;
        if !matches!(self.resolve_ty(&param), Ty::Hole)
            && !self.types_compatible(&param, &typed_left.ty)
        {
            return Err(TypeError {
                message: format!(
                    "`|>` type mismatch: expected {}, got {}",
                    self.ty_name(&param),
                    self.ty_name(&typed_left.ty)
                ),
                span: typed_left.span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`|>`",
                    "LHS: A; RHS: (A -> B); result: B",
                    &typed_left.ty,
                    &typed_right.ty,
                    None,
                )),
            });
        }
        let receiver_ty = self.resolve_ty(&typed_right.ty);
        let left_ty = self.resolve_ty(&typed_left.ty);
        self.flow_operator_trait_call(
            span,
            "PipeApply",
            "pipe_apply",
            &receiver_ty,
            vec![left_ty, self.resolve_ty(&ret)],
            OperatorTraitOp::PipeApply,
            vec![typed_right, typed_left],
            ret,
            "`|>`",
        )
    }

    pub(super) fn check_context_map(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_context_map_with_expected(span, left, right, None)
    }

    fn check_context_map_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let receiver_ty = self.resolve_ty(&typed_left.ty);
        let rhs_input_hint = match &receiver_ty {
            Ty::Result(ok, _) => Some(ok.as_ref().clone()),
            Ty::List(item) => Some(item.as_ref().clone()),
            Ty::Enum(name, args) if Self::surface_name(name) == "Option" && args.len() == 1 => {
                Some(args[0].clone())
            }
            _ => None,
        };
        let rhs_ret_expected = self.map_rhs_output_from_expected(&receiver_ty, expected);
        let allow_contextual_map_output = rhs_ret_expected
            .as_ref()
            .map(|ty| match self.resolve_ty(ty) {
                Ty::Result(_, _) | Ty::List(_) => true,
                Ty::Enum(name, _) if Self::surface_name(&name) == "Option" => true,
                _ => false,
            })
            .unwrap_or(false);
        let typed_right = if let Some(rhs_in) = &rhs_input_hint {
            if matches!(
                right,
                Resolved::InferredFacetCapture(_, _)
                    | Resolved::Capture(_, _, _)
                    | Resolved::Closure(_, _, _, _)
                    | Resolved::Grouped(_, _)
            ) {
                let contract = self.callable_contract(
                    rhs_in,
                    rhs_ret_expected.clone(),
                    ExpectedCallableSlot::Plain,
                );
                self.check_apply_callable_with_contract(right, &contract, "`|*>`")?
            } else {
                let contract = self.callable_contract(
                    rhs_in,
                    rhs_ret_expected.clone(),
                    ExpectedCallableSlot::Plain,
                );
                let expected_callable = self.expected_callable_ty(&contract);
                self.check_node_with_expected(right, Some(&expected_callable))
                    .or_else(|_| self.check_apply_callable(right, "`|*>`"))?
            }
        } else {
            self.check_apply_callable(right, "`|*>`")?
        };
        let (rhs_in, rhs_out) =
            self.unary_function_parts(&typed_right.ty, "`|*>`", &typed_right.span)?;
        if !allow_contextual_map_output {
            self.ensure_plain_map_output(&rhs_out, "`|*>`", &typed_right.span)?;
        }

        match &receiver_ty {
            Ty::Result(ok, _) => {
                if !self.types_compatible(ok.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|*>` type mismatch: expected {}, got {}",
                            self.ty_name(ok.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`|*>`",
                            "LHS: Functor container such as Result<A>, List<A>, or Option<A>; RHS: (A -> B); result: mapped container",
                            &typed_left.ty,
                            &typed_right.ty,
                            None,
                        )),
                    });
                }
            }
            Ty::List(item) => {
                if !self.types_compatible(item.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|*>` type mismatch: expected {}, got {}",
                            self.ty_name(item.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`|*>`",
                            "LHS: Functor container such as Result<A>, List<A>, or Option<A>; RHS: (A -> B); result: mapped container",
                            &typed_left.ty,
                            &typed_right.ty,
                            None,
                        )),
                    });
                }
            }
            Ty::Enum(name, args) if Self::surface_name(&name) == "Option" && args.len() == 1 => {
                if !self.types_compatible(&args[0], &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|*>` type mismatch: expected {}, got {}",
                            self.ty_name(&args[0]),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`|*>`",
                            "LHS: Functor container such as Result<A>, List<A>, or Option<A>; RHS: (A -> B); result: mapped container",
                            &typed_left.ty,
                            &typed_right.ty,
                            None,
                        )),
                    });
                }
            }
            _ => {}
        }

        let functor_trait = self
            .trait_key_by_short_name("Functor")
            .ok_or_else(|| TypeError {
                message: "Unknown trait: Functor".into(),
                span: span.clone(),
                hint: None,
            })?;
        let result_ty = self.env.fresh_tyvar();
        let requested_trait_args = vec![rhs_in.clone(), rhs_out.clone(), result_ty.clone()];
        let Some((dispatch, resolved_trait_args)) = self.operator_trait_dispatch_for_args(
            &functor_trait,
            "map",
            &receiver_ty,
            &requested_trait_args,
        ) else {
            let functor_summary = self.trait_implementation_summary("Functor");
            return Err(TypeError {
                message: format!(
                    "`|*>` requires Functor implementation on the left, got {}",
                    self.ty_name(&receiver_ty)
                ),
                span: typed_left.span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`|*>`",
                    "LHS: Functor container such as Result<A>, List<A>, or Option<A>; RHS: (A -> B); result: mapped container",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "{} The evaluated LHS is {}.",
                        functor_summary,
                        self.ty_name(&receiver_ty),
                    )),
                )),
            });
        };
        let result_ty = resolved_trait_args
            .get(2)
            .cloned()
            .unwrap_or_else(|| self.resolve_ty(&result_ty));
        let trait_name = self.trait_instance_key_from_tys(&functor_trait, &resolved_trait_args);
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name,
                method_name: "map".into(),
                receiver_ty: receiver_ty.clone(),
                dispatch,
                origin: TraitCallOrigin::Operator {
                    op: OperatorTraitOp::PipeMap,
                    lhs_ty: receiver_ty,
                    rhs_ty: self.resolve_ty(&typed_right.ty),
                },
                args: vec![typed_left, typed_right],
            },
        })
    }

    pub(super) fn check_context_bind(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_context_bind_with_expected(span, left, right, None)
    }

    fn check_context_bind_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        _expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let receiver_ty = self.resolve_ty(&typed_left.ty);
        let trait_helper_contract = match &receiver_ty {
            Ty::Result(ok, err) => {
                let next_ok = self.env.fresh_tyvar();
                let err_ty = self.resolve_ty(err.as_ref());
                Some(self.callable_contract(
                    ok.as_ref(),
                    Some(Ty::Result(Box::new(next_ok), Box::new(err_ty))),
                    ExpectedCallableSlot::Contextual,
                ))
            }
            Ty::List(item) => {
                let next_item = self.env.fresh_tyvar();
                Some(self.callable_contract(
                    item.as_ref(),
                    Some(Ty::List(Box::new(next_item))),
                    ExpectedCallableSlot::Contextual,
                ))
            }
            Ty::Enum(name, args) if Self::surface_name(name) == "Option" && args.len() == 1 => {
                let next_item = self.env.fresh_tyvar();
                Some(self.callable_contract(
                    &args[0],
                    Some(Ty::Enum(name.clone(), vec![next_item])),
                    ExpectedCallableSlot::Contextual,
                ))
            }
            _ => None,
        };
        let typed_right = match (right, trait_helper_contract) {
            (
                Resolved::InferredFacetCapture(_, _)
                | Resolved::Capture(_, _, _)
                | Resolved::Closure(_, _, _, _)
                | Resolved::Grouped(_, _),
                Some(contract),
            ) => self.check_apply_callable_with_contract(right, &contract, "`|>=`")?,
            (Resolved::App(call_span, func, args), Some(contract)) => {
                if let Some(typed) = self.check_trait_helper_pipe_callable(
                    call_span,
                    func,
                    args,
                    contract.input.clone(),
                    contract.ret.clone(),
                    "`|>=`",
                )? {
                    typed
                } else {
                    self.check_apply_callable(right, "`|>=`")?
                }
            }
            _ => self.check_apply_callable(right, "`|>=`")?,
        };
        let (rhs_in, rhs_ret) =
            self.unary_function_parts(&typed_right.ty, "`|>=`", &typed_right.span)?;

        let is_option_ctx =
            |ty: &Ty| matches!(ty, Ty::Enum(name, _) if Self::surface_name(name) == "Option");
        match (&receiver_ty, self.resolve_ty(&rhs_ret)) {
            (Ty::Result(ok, err), Ty::Result(next_ok, next_err)) => {
                if !self.types_compatible(ok.as_ref(), &rhs_in)
                    || !self.types_compatible(err.as_ref(), next_err.as_ref())
                {
                    return Err(TypeError {
                        message: "`|>=` requires matching Result context on both sides".into(),
                        span: span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`|>=`",
                            "LHS: Chainable container such as Result<A>, List<A>, or Option<A>; RHS: contextual function; result: same context family",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some(format!(
                                "LHS Ok type is {}; RHS input is {}; RHS returns Result<{}>. Error types must match. Use `|*>` when the RHS is a plain function and you only want to map over a Result.",
                                self.ty_name(ok.as_ref()),
                                self.ty_name(&rhs_in),
                                self.ty_name(next_ok.as_ref())
                            )),
                        )),
                    });
                }
            }
            (Ty::List(item), Ty::List(_)) => {
                if !self.types_compatible(item.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|>=` type mismatch: expected {}, got {}",
                            self.ty_name(item.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`|>=`",
                            "LHS: Chainable container such as Result<A>, List<A>, or Option<A>; RHS: contextual function; result: same context family",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some("Use `|*>` when the RHS is a plain function and you only want to map over a Result/List/Option.".into()),
                        )),
                    });
                }
            }
            (Ty::Enum(name, args), Ty::Enum(next_name, _))
                if Self::surface_name(&name) == "Option"
                    && args.len() == 1
                    && Self::surface_name(&next_name) == "Option" =>
            {
                if !self.types_compatible(&args[0], &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|>=` type mismatch: expected {}, got {}",
                            self.ty_name(&args[0]),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`|>=`",
                            "LHS: Chainable container such as Result<A>, List<A>, or Option<A>; RHS: contextual function; result: same context family",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some("Use `|*>` when the RHS is a plain function and you only want to map over a Result/List/Option.".into()),
                        )),
                    });
                }
            }
            (lhs_ctx, Ty::Result(_, _)) if is_option_ctx(lhs_ctx) => {
                return Err(TypeError {
                    message: "`|>=` cannot use Option as a standard failure container for Result bind"
                        .into(),
                    span: span.clone(),
                    hint: Some(self.operator_rule_hint(
                        "`|>=`",
                        "LHS: Option<A>; RHS: (A -> Result<B, E>); Option is not the standard failure container for this bind",
                        &typed_left.ty,
                        &typed_right.ty,
                        Some(
                            "Option is not standard for failure propagation. Convert explicitly with `from(value, Result)` before using `|>=`, for example `from(option_value, Result) |>= rhs()`.".into(),
                        ),
                    )),
                });
            }
            (Ty::Result(_, _), rhs_ctx) if is_option_ctx(&rhs_ctx) => {
                return Err(TypeError {
                    message: "`|>=` cannot switch from Result into Option bind context".into(),
                    span: span.clone(),
                    hint: Some(self.operator_rule_hint(
                        "`|>=`",
                        "LHS: Result<A, E>; RHS: (A -> Option<B>); Option is not the standard failure container for this bind",
                        &typed_left.ty,
                        &typed_right.ty,
                        Some(
                            "Option is not standard for failure propagation. Convert the RHS explicitly to Result with `from(value, Result)` around the Option result, for example `result_value |>= {|value| from(option_rhs(value), Result)}`.".into(),
                        ),
                    )),
                });
            }
            (lhs_ctx, rhs_ctx)
                if (matches!(lhs_ctx, Ty::Result(_, _))
                    && (matches!(rhs_ctx, Ty::List(_)) || is_option_ctx(&rhs_ctx)))
                    || (matches!(lhs_ctx, Ty::List(_))
                        && (matches!(rhs_ctx, Ty::Result(_, _)) || is_option_ctx(&rhs_ctx)))
                    || (is_option_ctx(lhs_ctx)
                        && (matches!(rhs_ctx, Ty::Result(_, _))
                            || matches!(rhs_ctx, Ty::List(_)))) =>
            {
                return Err(TypeError {
                message: "`|>=` container context mismatch: cannot mix Result, List, and Option context"
                    .into(),
                span: span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`|>=`",
                    "LHS: Chainable container such as Result<A>, List<A>, or Option<A>; RHS: contextual function; result: same context family",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some("Result, List, and Option containers cannot be mixed in one bind operator.".into()),
                )),
                });
            }
            (Ty::Result(_, _), rhs_plain) => {
                return Err(TypeError {
                message: format!(
                    "`|>=` requires the right-hand side to return Result, got {}",
                    self.ty_name(&rhs_plain)
                ),
                span: typed_right.span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`|>=`",
                    "LHS: Result<A, E>; RHS: (A -> Result<B, E>); result: Result<B, E>",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "RHS returns plain {}. Use `|*>` when the RHS is a plain function and you want to keep the Result context.",
                        self.ty_name(&rhs_plain)
                    )),
                )),
                });
            }
            (Ty::List(_), rhs_plain) => {
                return Err(TypeError {
                message: format!(
                    "`|>=` requires the right-hand side to return List, got {}",
                    self.ty_name(&rhs_plain)
                ),
                span: typed_right.span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`|>=`",
                    "LHS: List<A>; RHS: (A -> List<B>); result: List<B>",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "RHS returns plain {}. Use `|*>` when the RHS is a plain function and you want to keep the List context.",
                        self.ty_name(&rhs_plain)
                    )),
                )),
                });
            }
            (Ty::Enum(name, _), rhs_plain) if Self::surface_name(&name) == "Option" => {
                return Err(TypeError {
                message: format!(
                    "`|>=` requires the right-hand side to return Option, got {}",
                    self.ty_name(&rhs_plain)
                ),
                span: typed_right.span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`|>=`",
                    "LHS: Option<A>; RHS: (A -> Option<B>); result: Option<B>",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "RHS returns plain {}. Use `|*>` when the RHS is a plain function and you want to keep the Option context.",
                        self.ty_name(&rhs_plain)
                    )),
                )),
                });
            }
            _ => {}
        }

        let chainable_trait =
            self.trait_key_by_short_name("Chainable")
                .ok_or_else(|| TypeError {
                    message: "Unknown trait: Chainable".into(),
                    span: span.clone(),
                    hint: None,
                })?;
        let requested_trait_args = vec![rhs_in.clone(), self.resolve_ty(&rhs_ret)];
        let Some((dispatch, resolved_trait_args)) = self.operator_trait_dispatch_for_args(
            &chainable_trait,
            "chain",
            &receiver_ty,
            &requested_trait_args,
        ) else {
            let chainable_summary = self.trait_implementation_summary("Chainable");
            return Err(TypeError {
                message: format!(
                    "`|>=` requires Chainable implementation on the left, got {}",
                    self.ty_name(&receiver_ty)
                ),
                span: typed_left.span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`|>=`",
                    "LHS: Chainable container such as Result<A>, List<A>, or Option<A>; RHS: contextual function; result: same context family",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "{} The evaluated LHS is {}. Use `|*>` after a contextual value when the RHS is plain.",
                        chainable_summary,
                        self.ty_name(&receiver_ty)
                    )),
                )),
            });
        };
        let result_ty = resolved_trait_args
            .get(1)
            .cloned()
            .unwrap_or_else(|| self.resolve_ty(&rhs_ret));
        let trait_name = self.trait_instance_key_from_tys(&chainable_trait, &resolved_trait_args);
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name,
                method_name: "compose".into(),
                receiver_ty: receiver_ty.clone(),
                dispatch,
                origin: TraitCallOrigin::Operator {
                    op: OperatorTraitOp::PipeBind,
                    lhs_ty: receiver_ty,
                    rhs_ty: self.resolve_ty(&typed_right.ty),
                },
                args: vec![typed_left, typed_right],
            },
        })
    }

    pub(super) fn check_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_compose_with_expected(span, left, right, None)
    }

    fn check_compose_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let expected_parts = self.expected_unary_function_parts(expected);
        let typed_left = if let Some((expected_in, _)) = &expected_parts {
            let contract = self.callable_contract(expected_in, None, ExpectedCallableSlot::Plain);
            self.check_compose_callable_with_contract(left, &contract, "`>>`")?
        } else {
            self.check_operator_compose_callable(left, "`>>`")?
        };
        let (left_in, left_out) =
            self.unary_function_parts(&typed_left.ty, "`>>`", &typed_left.span)?;
        let right_ret_expected = expected_parts.map(|(_, ret)| ret);
        let right_contract =
            self.callable_contract(&left_out, right_ret_expected, ExpectedCallableSlot::Plain);
        let typed_right =
            self.check_compose_callable_with_contract(right, &right_contract, "`>>`")?;
        let (right_in, right_out) =
            self.unary_function_parts(&typed_right.ty, "`>>`", &typed_right.span)?;
        if !self.types_compatible(&left_out, &right_in) {
            let extra = match self.resolve_ty(&left_out) {
                Ty::Result(ok, _) if self.resolve_ty(ok.as_ref()) == self.resolve_ty(&right_in) => {
                    Some(format!(
                        "`>>` is plain composition, so it passes the whole Result<{}> onward. Use `>*` to compose a plain RHS over the Ok value.",
                        self.ty_name(ok.as_ref())
                    ))
                }
                _ => None,
            };
            return Err(TypeError {
                message: "`>>` requires the left output type to match the right input type".into(),
                span: span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`>>`",
                    "LHS: (A -> B); RHS: (B -> C); result: (A -> C)",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "Left output is {}; right input is {}.{}",
                        self.ty_name(&left_out),
                        self.ty_name(&right_in),
                        extra.map(|msg| format!(" {}", msg)).unwrap_or_default()
                    )),
                )),
            });
        }
        let result_ty = Ty::Func(
            vec![self.resolve_ty(&left_in)],
            Box::new(self.resolve_ty(&right_out)),
        );
        let receiver_ty = self.resolve_ty(&typed_left.ty);
        self.flow_operator_trait_call(
            span,
            "Composable",
            "compose",
            &receiver_ty,
            vec![
                self.resolve_ty(&left_in),
                self.resolve_ty(&left_out),
                self.resolve_ty(&right_out),
            ],
            OperatorTraitOp::Compose,
            vec![typed_left, typed_right],
            result_ty,
            "`>>`",
        )
    }

    pub(super) fn check_lifted_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_lifted_compose_with_expected(span, left, right, None)
    }

    fn check_lifted_compose_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let expected_parts = self.expected_unary_function_parts(expected);
        let expected_ret = expected_parts.as_ref().map(|(_, ret)| ret.clone());
        let typed_left = if let Some((expected_in, expected_ret)) = &expected_parts {
            let left_context_ret = match self.resolve_ty(expected_ret) {
                Ty::Result(_, err) => Some(Ty::Result(
                    Box::new(self.env.fresh_tyvar()),
                    Box::new(self.resolve_ty(err.as_ref())),
                )),
                Ty::List(_) => Some(Ty::List(Box::new(self.env.fresh_tyvar()))),
                Ty::Enum(name, args)
                    if Self::surface_name(&name) == "Option" && args.len() == 1 =>
                {
                    Some(Ty::Enum(name, vec![self.env.fresh_tyvar()]))
                }
                _ => None,
            };
            if let Some(left_ret) = left_context_ret {
                let contract = self.callable_contract(
                    expected_in,
                    Some(left_ret),
                    ExpectedCallableSlot::Contextual,
                );
                self.check_compose_callable_with_contract(left, &contract, "`>*`")?
            } else {
                self.check_operator_compose_callable(left, "`>*`")?
            }
        } else {
            self.check_operator_compose_callable(left, "`>*`")?
        };
        let (left_in, left_out) =
            self.unary_function_parts(&typed_left.ty, "`>*`", &typed_left.span)?;
        let rhs_input_hint = match self.resolve_ty(&left_out) {
            Ty::Result(ok, _) => Some(self.resolve_ty(ok.as_ref())),
            Ty::List(item) => Some(self.resolve_ty(item.as_ref())),
            Ty::Enum(name, args) if Self::surface_name(&name) == "Option" && args.len() == 1 => {
                Some(self.resolve_ty(&args[0]))
            }
            _ => None,
        };
        let mut allow_contextual_lift_output = false;
        let typed_right = if let Some(rhs_in) = &rhs_input_hint {
            let rhs_ret_expected = expected_ret.as_ref().and_then(|ret| {
                let payload = self.context_payload_ty(ret)?;
                allow_contextual_lift_output = matches!(self.resolve_ty(&payload), Ty::Result(_, _) | Ty::List(_))
                    || matches!(self.resolve_ty(&payload), Ty::Enum(name, _) if Self::surface_name(&name) == "Option");
                Some(payload)
            });
            let contract =
                self.callable_contract(rhs_in, rhs_ret_expected, ExpectedCallableSlot::Plain);
            self.check_compose_callable_with_contract(right, &contract, "`>*`")?
        } else {
            self.check_operator_compose_callable(right, "`>*`")?
        };
        let (right_in, right_out) =
            self.unary_function_parts(&typed_right.ty, "`>*`", &typed_right.span)?;
        if !allow_contextual_lift_output {
            self.ensure_plain_map_output(&right_out, "`>*`", &typed_right.span)?;
        }
        match self.resolve_ty(&left_out) {
            Ty::Result(ok, err) => {
                if !self.types_compatible(ok.as_ref(), &right_in) {
                    return Err(TypeError {
                        message:
                            "`>*` requires the left contextual output to match the right input type"
                                .into(),
                        span: span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`>*`",
                            "LHS: (A -> Result<B, E>) or (A -> List<B>); RHS: (B -> C); result: (A -> Result<C, E>) or (A -> List<C>)",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some(format!(
                                "Left contextual output is Result<{}>; right input is {}.",
                                self.ty_name(ok.as_ref()),
                                self.ty_name(&right_in)
                            )),
                        )),
                    });
                }
                let mapped_ty = Ty::Result(
                    Box::new(self.resolve_ty(&right_out)),
                    Box::new(self.resolve_ty(err.as_ref())),
                );
                let result_ty =
                    Ty::Func(vec![self.resolve_ty(&left_in)], Box::new(mapped_ty.clone()));
                let receiver_ty = self.resolve_ty(&typed_left.ty);
                self.flow_operator_trait_call(
                    span,
                    "LiftComposable",
                    "lift_compose",
                    &receiver_ty,
                    vec![
                        self.resolve_ty(&left_in),
                        self.resolve_ty(ok.as_ref()),
                        self.resolve_ty(&right_out),
                        mapped_ty,
                    ],
                    OperatorTraitOp::LiftCompose,
                    vec![typed_left, typed_right],
                    result_ty,
                    "`>*`",
                )
            }
            Ty::List(item) => {
                if !self.types_compatible(item.as_ref(), &right_in) {
                    return Err(TypeError {
                        message:
                            "`>*` requires the left contextual output to match the right input type"
                                .into(),
                        span: span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`>*`",
                            "LHS: (A -> Result<B, E>) or (A -> List<B>); RHS: (B -> C); result: (A -> Result<C, E>) or (A -> List<C>)",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some(format!(
                                "Left contextual output is List<{}>; right input is {}.",
                                self.ty_name(item.as_ref()),
                                self.ty_name(&right_in)
                            )),
                        )),
                    });
                }
                let mapped_ty = Ty::List(Box::new(self.resolve_ty(&right_out)));
                let result_ty =
                    Ty::Func(vec![self.resolve_ty(&left_in)], Box::new(mapped_ty.clone()));
                let receiver_ty = self.resolve_ty(&typed_left.ty);
                self.flow_operator_trait_call(
                    span,
                    "LiftComposable",
                    "lift_compose",
                    &receiver_ty,
                    vec![
                        self.resolve_ty(&left_in),
                        self.resolve_ty(item.as_ref()),
                        self.resolve_ty(&right_out),
                        mapped_ty,
                    ],
                    OperatorTraitOp::LiftCompose,
                    vec![typed_left, typed_right],
                    result_ty,
                    "`>*`",
                )
            }
            Ty::Enum(name, args) if Self::surface_name(&name) == "Option" && args.len() == 1 => {
                if !self.types_compatible(&args[0], &right_in) {
                    return Err(TypeError {
                        message:
                            "`>*` requires the left contextual output to match the right input type"
                                .into(),
                        span: span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`>*`",
                            "LHS: (A -> Result<B, E>) or (A -> List<B>) or (A -> Option<B>); RHS: (B -> C); result: (A -> Result<C, E>) or (A -> List<C>) or (A -> Option<C>)",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some(format!(
                                "Left contextual output is Option<{}>; right input is {}.",
                                self.ty_name(&args[0]),
                                self.ty_name(&right_in)
                            )),
                        )),
                    });
                }
                let mapped_ty = Ty::Enum(name, vec![self.resolve_ty(&right_out)]);
                let result_ty =
                    Ty::Func(vec![self.resolve_ty(&left_in)], Box::new(mapped_ty.clone()));
                let receiver_ty = self.resolve_ty(&typed_left.ty);
                self.flow_operator_trait_call(
                    span,
                    "LiftComposable",
                    "lift_compose",
                    &receiver_ty,
                    vec![
                        self.resolve_ty(&left_in),
                        self.resolve_ty(&args[0]),
                        self.resolve_ty(&right_out),
                        mapped_ty,
                    ],
                    OperatorTraitOp::LiftCompose,
                    vec![typed_left, typed_right],
                    result_ty,
                    "`>*`",
                )
            }
            _ => Err(TypeError {
                message: "`>*` requires Result, List, or Option on the left-hand side".into(),
                span: span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`>*`",
                    "LHS: (A -> Result<B, E>) or (A -> List<B>) or (A -> Option<B>); RHS: (B -> C); result: (A -> Result<C, E>) or (A -> List<C>) or (A -> Option<C>)",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "The left callable returns {}, so no Result/List/Option container is available at this step. Use `>>` for plain composition; use `|*>` when you already have an evaluated contextual value and want to map a plain RHS over it.",
                        self.ty_name(&left_out)
                    )),
                )),
            }),
        }
    }

    pub(super) fn check_kleisli_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_kleisli_compose_with_expected(span, left, right, None)
    }

    fn check_kleisli_compose_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let expected_parts = self.expected_unary_function_parts(expected);
        let expected_ret = expected_parts.as_ref().map(|(_, ret)| ret.clone());
        let typed_left = if let Some((expected_in, expected_ret)) = &expected_parts {
            let left_context_ret = match self.resolve_ty(expected_ret) {
                Ty::Result(_, err) => Some(Ty::Result(
                    Box::new(self.env.fresh_tyvar()),
                    Box::new(self.resolve_ty(err.as_ref())),
                )),
                Ty::List(_) => Some(Ty::List(Box::new(self.env.fresh_tyvar()))),
                Ty::Enum(name, args)
                    if Self::surface_name(&name) == "Option" && args.len() == 1 =>
                {
                    Some(Ty::Enum(name, vec![self.env.fresh_tyvar()]))
                }
                _ => None,
            };
            if let Some(left_ret) = left_context_ret {
                let contract = self.callable_contract(
                    expected_in,
                    Some(left_ret),
                    ExpectedCallableSlot::Contextual,
                );
                self.check_compose_callable_with_contract(left, &contract, "`>=>`")?
            } else {
                self.check_operator_compose_callable(left, "`>=>`")?
            }
        } else {
            self.check_operator_compose_callable(left, "`>=>`")?
        };
        let (left_in, left_out) =
            self.unary_function_parts(&typed_left.ty, "`>=>`", &typed_left.span)?;
        let rhs_input_hint = match self.resolve_ty(&left_out) {
            Ty::Result(ok, _) => Some(self.resolve_ty(ok.as_ref())),
            Ty::List(item) => Some(self.resolve_ty(item.as_ref())),
            Ty::Enum(name, args) if Self::surface_name(&name) == "Option" && args.len() == 1 => {
                Some(self.resolve_ty(&args[0]))
            }
            _ => None,
        };
        let typed_right = if let Some(rhs_in) = &rhs_input_hint {
            let rhs_ret_expected =
                expected_ret
                    .clone()
                    .or_else(|| match self.resolve_ty(&left_out) {
                        Ty::Result(_, err) => Some(Ty::Result(
                            Box::new(self.env.fresh_tyvar()),
                            Box::new(self.resolve_ty(err.as_ref())),
                        )),
                        Ty::List(_) => Some(Ty::List(Box::new(self.env.fresh_tyvar()))),
                        Ty::Enum(name, args)
                            if Self::surface_name(&name) == "Option" && args.len() == 1 =>
                        {
                            Some(Ty::Enum(name, vec![self.env.fresh_tyvar()]))
                        }
                        _ => None,
                    });
            let contract =
                self.callable_contract(rhs_in, rhs_ret_expected, ExpectedCallableSlot::Contextual);
            self.check_compose_callable_with_contract(right, &contract, "`>=>`")?
        } else {
            self.check_operator_compose_callable(right, "`>=>`")?
        };
        let (right_in, right_out) =
            self.unary_function_parts(&typed_right.ty, "`>=>`", &typed_right.span)?;
        match (self.resolve_ty(&left_out), self.resolve_ty(&right_out)) {
            (Ty::Result(ok, err), Ty::Result(next_ok, next_err)) => {
                if !self.types_compatible(ok.as_ref(), &right_in)
                    || !self.types_compatible(err.as_ref(), next_err.as_ref())
                {
                    return Err(TypeError {
                        message: "`>=>` requires matching Result context on both sides".into(),
                        span: span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`>=>`",
                            "LHS: (A -> Result<B, E>) or (A -> List<B>); RHS: (B -> Result<C, E>) or (B -> List<C>); result: (A -> Result<C, E>) or (A -> List<C>)",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some(format!(
                                "Left output is Result<{}>; right input is {}; error types must match. Use `>*` when the RHS is plain.",
                                self.ty_name(ok.as_ref()),
                                self.ty_name(&right_in)
                            )),
                        )),
                    });
                }
                let chained_ty = Ty::Result(
                    Box::new(self.resolve_ty(next_ok.as_ref())),
                    Box::new(self.resolve_ty(err.as_ref())),
                );
                let result_ty =
                    Ty::Func(vec![self.resolve_ty(&left_in)], Box::new(chained_ty.clone()));
                let receiver_ty = self.resolve_ty(&typed_left.ty);
                self.flow_operator_trait_call(
                    span,
                    "KleisliComposable",
                    "kleisli_compose",
                    &receiver_ty,
                    vec![
                        self.resolve_ty(&left_in),
                        self.resolve_ty(ok.as_ref()),
                        chained_ty,
                    ],
                    OperatorTraitOp::KleisliCompose,
                    vec![typed_left, typed_right],
                    result_ty,
                    "`>=>`",
                )
            }
            (Ty::List(item), Ty::List(next_item)) => {
                if !self.types_compatible(item.as_ref(), &right_in) {
                    return Err(TypeError {
                        message: "`>=>` requires matching List element types across both sides"
                            .into(),
                        span: span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`>=>`",
                            "LHS: (A -> Result<B, E>) or (A -> List<B>); RHS: (B -> Result<C, E>) or (B -> List<C>); result: (A -> Result<C, E>) or (A -> List<C>)",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some(format!(
                                "Left output is List<{}>; right input is {}.",
                                self.ty_name(item.as_ref()),
                                self.ty_name(&right_in)
                            )),
                        )),
                    });
                }
                let chained_ty = Ty::List(Box::new(self.resolve_ty(next_item.as_ref())));
                let result_ty =
                    Ty::Func(vec![self.resolve_ty(&left_in)], Box::new(chained_ty.clone()));
                let receiver_ty = self.resolve_ty(&typed_left.ty);
                self.flow_operator_trait_call(
                    span,
                    "KleisliComposable",
                    "kleisli_compose",
                    &receiver_ty,
                    vec![
                        self.resolve_ty(&left_in),
                        self.resolve_ty(item.as_ref()),
                        chained_ty,
                    ],
                    OperatorTraitOp::KleisliCompose,
                    vec![typed_left, typed_right],
                    result_ty,
                    "`>=>`",
                )
            }
            (Ty::Enum(name, args), Ty::Enum(next_name, next_args))
                if Self::surface_name(&name) == "Option"
                    && Self::surface_name(&next_name) == "Option"
                    && args.len() == 1
                    && next_args.len() == 1 =>
            {
                if !self.types_compatible(&args[0], &right_in) {
                    return Err(TypeError {
                        message: "`>=>` requires matching Option payload types across both sides"
                            .into(),
                        span: span.clone(),
                        hint: Some(self.operator_rule_hint(
                            "`>=>`",
                            "LHS: (A -> Result<B, E>) or (A -> List<B>) or (A -> Option<B>); RHS: (B -> Result<C, E>) or (B -> List<C>) or (B -> Option<C>); result: (A -> Result<C, E>) or (A -> List<C>) or (A -> Option<C>)",
                            &typed_left.ty,
                            &typed_right.ty,
                            Some(format!(
                                "Left output is Option<{}>; right input is {}.",
                                self.ty_name(&args[0]),
                                self.ty_name(&right_in)
                            )),
                        )),
                    });
                }
                let chained_ty = Ty::Enum(name, vec![self.resolve_ty(&next_args[0])]);
                let result_ty =
                    Ty::Func(vec![self.resolve_ty(&left_in)], Box::new(chained_ty.clone()));
                let receiver_ty = self.resolve_ty(&typed_left.ty);
                self.flow_operator_trait_call(
                    span,
                    "KleisliComposable",
                    "kleisli_compose",
                    &receiver_ty,
                    vec![
                        self.resolve_ty(&left_in),
                        self.resolve_ty(&args[0]),
                        chained_ty,
                    ],
                    OperatorTraitOp::KleisliCompose,
                    vec![typed_left, typed_right],
                    result_ty,
                    "`>=>`",
                )
            }
            _ => Err(TypeError {
                message: "`>=>` requires matching Result, List, or Option context on both sides".into(),
                span: span.clone(),
                hint: Some(self.operator_rule_hint(
                    "`>=>`",
                    "LHS: (A -> Result<B, E>) or (A -> List<B>) or (A -> Option<B>); RHS: (B -> Result<C, E>) or (B -> List<C>) or (B -> Option<C>); result: (A -> Result<C, E>) or (A -> List<C>) or (A -> Option<C>)",
                    &typed_left.ty,
                    &typed_right.ty,
                    Some(format!(
                        "Left output is {}; right output is {}. Use `>*` when the left side is contextual but the RHS is plain; use `>>` when both sides are plain.",
                        self.ty_name(&left_out),
                        self.ty_name(&right_out)
                    )),
                )),
            }),
        }
    }

    pub(super) fn match_result_variant_tags(
        &self,
        span: &Span,
    ) -> Result<(u32, u32, u32), TypeError> {
        let variants = self
            .lookup_enum_variants_of("MatchResult")
            .ok_or_else(|| TypeError {
                message: "MatchResult enum is not available in the current environment".into(),
                span: span.clone(),
                hint: None,
            })?;
        let mut success_tag = None;
        let mut no_match_tag = None;
        let mut err_tag = None;
        for variant in variants {
            match variant.short_name.as_str() {
                "Success" => success_tag = Some(variant.tag),
                "NoMatch" => no_match_tag = Some(variant.tag),
                "Err" => err_tag = Some(variant.tag),
                _ => {}
            }
        }
        match (success_tag, no_match_tag, err_tag) {
            (Some(success), Some(no_match), Some(err)) => Ok((success, no_match, err)),
            _ => Err(TypeError {
                message: "MatchResult enum must define Success, NoMatch, and Err variants".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn extractor_contract(
        &mut self,
        extractor_id: &ResolvedId,
        span: &Span,
    ) -> Result<(Ty, Vec<Ty>, u32, u32, u32), TypeError> {
        let extractor_ty = self
            .env
            .lookup_var(extractor_id.unique_id)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Undefined extractor: {}", extractor_id.name),
                span: span.clone(),
                hint: None,
            })?;
        let extractor_ty = self.instantiate_builtin_ty(&extractor_ty);
        let (params, ret) = match &extractor_ty {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "Extractor {} is not callable (got {})",
                        extractor_id.name,
                        self.ty_name(other)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
        };
        if params.len() != 1 {
            return Err(TypeError {
                message: format!(
                    "Extractor {} must accept exactly one input value, got {} parameter(s)",
                    extractor_id.name,
                    params.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }
        let input_ty = params[0].clone();
        let seq_tys =
            self.require_match_result_seq_ty(&self.resolve_ty(&ret), span, &extractor_id.name)?;
        let (success_tag, no_match_tag, err_tag) = self.match_result_variant_tags(span)?;
        Ok((input_ty, seq_tys, success_tag, no_match_tag, err_tag))
    }

    pub(super) fn extractor_callable_ty(
        &mut self,
        extractor_id: &ResolvedId,
        span: &Span,
    ) -> Result<Ty, TypeError> {
        let extractor_ty = self
            .env
            .lookup_var(extractor_id.unique_id)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Undefined extractor: {}", extractor_id.name),
                span: span.clone(),
                hint: None,
            })?;
        Ok(self.instantiate_builtin_ty(&extractor_ty))
    }

    pub(super) fn kernel_uncons_id(&self, span: &Span) -> Result<ResolvedId, TypeError> {
        let id = self
            .function_ids_by_name
            .get("Kernel::uncons")
            .or_else(|| self.function_ids_by_name.get("uncons"))
            .cloned()
            .ok_or_else(|| TypeError {
                message: "Missing helper function: Kernel::uncons".into(),
                span: span.clone(),
                hint: None,
            })?;
        Ok(ResolvedId {
            span: span.clone(),
            ..id
        })
    }

    pub(super) fn uncons_contract_for_input(
        &self,
        observed_ty: &Ty,
        span: &Span,
    ) -> Result<(Ty, Vec<Ty>), TypeError> {
        match self.resolve_ty(observed_ty) {
            Ty::List(inner) => {
                let elem_ty = inner.as_ref().clone();
                let list_ty = Ty::List(Box::new(elem_ty.clone()));
                Ok((list_ty.clone(), vec![elem_ty, list_ty]))
            }
            Ty::Str => Ok((Ty::Str, vec![Ty::Str, Ty::Str])),
            other => Err(TypeError {
                message: format!(
                    "Extractor uncons expects List<...> or String, got {}",
                    self.ty_name(&other)
                ),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn extractor_contract_for_observed_ty(
        &mut self,
        extractor_id: &ResolvedId,
        observed_ty: &Ty,
        span: &Span,
    ) -> Result<(Ty, Ty, Vec<Ty>, u32, u32, u32), TypeError> {
        let extractor_ty = self.extractor_callable_ty(extractor_id, span)?;
        let (input_ty, seq_tys, success_tag, no_match_tag, err_tag) = if matches!(&extractor_ty, Ty::BuiltinFunc { name, .. } if name == "uncons")
        {
            let (input_ty, seq_tys) = self.uncons_contract_for_input(observed_ty, span)?;
            let (success_tag, no_match_tag, err_tag) = self.match_result_variant_tags(span)?;
            (input_ty, seq_tys, success_tag, no_match_tag, err_tag)
        } else {
            self.extractor_contract(extractor_id, span)?
        };
        Ok((
            input_ty,
            self.resolve_ty(&extractor_ty),
            seq_tys,
            success_tag,
            no_match_tag,
            err_tag,
        ))
    }

    pub(super) fn require_match_result_seq_ty(
        &self,
        ty: &Ty,
        span: &Span,
        context: &str,
    ) -> Result<Vec<Ty>, TypeError> {
        match self.resolve_ty(ty) {
            Ty::Enum(name, args) if name == "MatchResult" && args.len() == 1 => match &args[0] {
                Ty::Tuple(items) => Ok(items.clone()),
                other => Ok(vec![other.clone()]),
            },
            other => Err(TypeError {
                message: format!(
                    "{} must return MatchResult<T> or MatchResult<(...)>, got {}",
                    context,
                    self.ty_name(&other)
                ),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn typecheck_user_function_args(
        &mut self,
        span: &Span,
        callee_uid: u32,
        params: &[Ty],
        args: &[ResolvedRecordLitArg],
        callable_hint: Option<&str>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let has_named = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
        let has_positional = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Positional(_)));
        if has_named && has_positional {
            return Err(TypeError {
                message: "Cannot mix positional and named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let param_names = self.user_func_params.get(&callee_uid).cloned();
        let mut typed_args = Vec::with_capacity(params.len());

        if has_named {
            let names = param_names.as_ref().ok_or_else(|| TypeError {
                message: "This function value does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            })?;

            if args.len() != params.len() {
                return Err(TypeError {
                    message: format!(
                        "function expects {} argument(s), got {}",
                        params.len(),
                        args.len()
                    ),
                    span: span.clone(),
                    hint: callable_hint.map(str::to_string),
                });
            }

            let mut reordered: Vec<Option<&Resolved>> = vec![None; params.len()];
            for arg in args {
                let ResolvedRecordLitArg::Named(name, expr) = arg else {
                    unreachable!("validated argument form above")
                };
                let idx = names
                    .iter()
                    .position(|n| n == name)
                    .ok_or_else(|| TypeError {
                        message: format!("Unknown argument name '{}' for function", name),
                        span: span.clone(),
                        hint: None,
                    })?;
                if reordered[idx].is_some() {
                    return Err(TypeError {
                        message: format!("Duplicate argument '{}'", name),
                        span: span.clone(),
                        hint: None,
                    });
                }
                reordered[idx] = Some(expr);
            }

            for (idx, expected_ty) in params.iter().enumerate() {
                let expr = reordered[idx].ok_or_else(|| TypeError {
                    message: format!("Missing argument '{}'", names[idx]),
                    span: span.clone(),
                    hint: None,
                })?;
                let typed = if matches!(self.resolve_ty(expected_ty), Ty::Hole) {
                    self.check_node(expr)?
                } else {
                    self.check_node_with_expected(expr, Some(expected_ty))?
                };
                self.ensure_no_runtime_facet_value(&typed, "Function call arguments")?;
                if !matches!(self.resolve_ty(expected_ty), Ty::Hole)
                    && !self.types_compatible(expected_ty, &typed.ty)
                {
                    return Err(TypeError {
                        message: self.argument_type_mismatch_message(expected_ty, &typed.ty),
                        span: typed.span.clone(),
                        hint: callable_hint.map(str::to_string),
                    });
                }
                typed_args.push(typed);
            }
            return Ok(typed_args);
        }

        if args.len() != params.len() {
            return Err(TypeError {
                message: format!(
                    "function expects {} argument(s), got {}",
                    params.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: callable_hint.map(str::to_string),
            });
        }

        for (expected_ty, arg) in params.iter().zip(args) {
            let ResolvedRecordLitArg::Positional(expr) = arg else {
                unreachable!("validated argument form above")
            };
            let typed = if matches!(self.resolve_ty(expected_ty), Ty::Hole) {
                self.check_node(expr)?
            } else {
                self.check_node_with_expected(expr, Some(expected_ty))?
            };
            self.ensure_no_runtime_facet_value(&typed, "Function call arguments")?;
            if !matches!(self.resolve_ty(expected_ty), Ty::Hole)
                && !self.types_compatible(expected_ty, &typed.ty)
            {
                return Err(TypeError {
                    message: self.argument_type_mismatch_message(expected_ty, &typed.ty),
                    span: typed.span.clone(),
                    hint: callable_hint.map(str::to_string),
                });
            }
            typed_args.push(typed);
        }

        Ok(typed_args)
    }

    fn facet_intrinsic_kind(&self, func: &Resolved) -> Option<&'static str> {
        let Resolved::Var(_, id) = func else {
            return None;
        };
        if let Some(qualified_name) = id.qualified_name.as_deref() {
            return match Self::surface_name(qualified_name) {
                "Facet::view" => Some("view"),
                "Facet::preview" => Some("preview"),
                "Facet::chain" => Some("chain"),
                "Facet::put" => Some("put"),
                "Facet::set" => Some("set"),
                "Facet::over" => Some("over"),
                "Facet::over_result" => Some("over_result"),
                "Facet::case_set" => Some("case_set"),
                "Facet::case_over" => Some("case_over"),
                _ => None,
            };
        }
        None
    }

    fn is_result_chain_auto_import(func: &Resolved) -> bool {
        let Resolved::Var(_, id) = func else {
            return false;
        };
        id.qualified_name
            .as_deref()
            .is_some_and(|qualified| Self::surface_name(qualified) == "Result::chain")
    }

    fn looks_like_facet_path_expr(expr: &Resolved) -> bool {
        match expr {
            Resolved::FieldAccess(_, _, _) | Resolved::FacetSegmentAccess(_, _, _) => true,
            Resolved::Grouped(_, inner) => Self::looks_like_facet_path_expr(inner),
            Resolved::FacetCapture(_, _)
            | Resolved::InferredFacetCapture(_, _)
            | Resolved::BinOp(_, BinOp::Slash, _, _) => true,
            _ => false,
        }
    }

    fn facet_chain_candidate_args(args: &[ResolvedRecordLitArg]) -> bool {
        args.iter()
            .all(|arg| matches!(arg, ResolvedRecordLitArg::Positional(_)))
            && args.iter().any(|arg| match arg {
                ResolvedRecordLitArg::Positional(expr) => Self::looks_like_facet_path_expr(expr),
                ResolvedRecordLitArg::Named(_, _) => false,
            })
    }

    fn pending_segment_from_typed(segment: &TypedFacetSegment) -> PendingFacetSegment {
        Self::pending_field_segment(match segment {
            TypedFacetSegment::Field { field_name, .. } => field_name.clone(),
            TypedFacetSegment::Tuple { field_index, .. } => format!("_{field_index}"),
            TypedFacetSegment::Variant { variant_name, .. } => variant_name.clone(),
            TypedFacetSegment::ListIndex { index, display, .. } => {
                return PendingFacetSegment::Bracket {
                    expr: PendingFacetExpr::Typed(index.clone()),
                    display: display.clone(),
                };
            }
            TypedFacetSegment::ListRange {
                start,
                end,
                display,
                ..
            } => {
                return PendingFacetSegment::RangeBracket {
                    start: PendingFacetExpr::Typed(start.clone()),
                    end: PendingFacetExpr::Typed(end.clone()),
                    display: display.clone(),
                };
            }
            TypedFacetSegment::MapKey { key, display, .. } => {
                return PendingFacetSegment::Bracket {
                    expr: PendingFacetExpr::Typed(key.clone()),
                    display: display.clone(),
                };
            }
        })
    }

    fn facet_path_kind_for_segments(segments: &[TypedFacetSegment]) -> TypedFacetPathKind {
        if segments
            .iter()
            .any(|segment| matches!(segment, TypedFacetSegment::Variant { .. }))
        {
            TypedFacetPathKind::Variant
        } else {
            TypedFacetPathKind::Structural
        }
    }

    fn pending_facet_node(&mut self, span: &Span, path: PendingFacetPath) -> TypedNode {
        let source_tv = self.env.fresh_tyvar();
        let focus_tv = self.env.fresh_tyvar();
        TypedNode {
            ty: Ty::Facet(Box::new(source_tv), Box::new(focus_tv)),
            span: span.clone(),
            node: TypedInner::PendingFacetPath(path),
        }
    }

    fn validate_pending_root_source(
        &mut self,
        root_path_name: &str,
        source_ty: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        match (root_path_name, self.resolve_ty(source_ty)) {
            ("Tuple", Ty::Tuple(_)) => Ok(()),
            ("List", Ty::List(_)) => Ok(()),
            ("HashMap", Ty::Enum(name, args))
                if Self::surface_name(&name) == "HashMap" && args.len() == 1 =>
            {
                Ok(())
            }
            ("Tuple", actual) => Err(TypeError {
                message: format!(
                    "Tuple root Facet path requires tuple source context, got {}",
                    self.ty_name(&actual)
                ),
                span: span.clone(),
                hint: Some("Expected source type like (A, B, ...) for Tuple._N.".into()),
            }),
            ("List", actual) => Err(TypeError {
                message: format!(
                    "List root Facet path requires List<T>, got {}",
                    self.ty_name(&actual)
                ),
                span: span.clone(),
                hint: Some("Use List.[N] with a List source value.".into()),
            }),
            ("HashMap", actual) => Err(TypeError {
                message: format!(
                    "HashMap root Facet path requires HashMap<T>, got {}",
                    self.ty_name(&actual)
                ),
                span: span.clone(),
                hint: Some("Use HashMap.[\"key\"] with a HashMap source value.".into()),
            }),
            _ => Ok(()),
        }
    }

    fn specialize_pending_facet_path(
        &mut self,
        path: PendingFacetPath,
        span: &Span,
        expected_source: Option<&Ty>,
    ) -> Result<TypedFacetPath, TypeError> {
        let mut current_source = if let Some(source_ty_hint) = path.source_ty_hint.clone() {
            if let Some(expected_source) = expected_source {
                if !self.types_compatible(&source_ty_hint, expected_source) {
                    return Err(TypeError {
                        message: format!(
                            "Facet path source type mismatch: path expects {}, got {}",
                            self.ty_name(&source_ty_hint),
                            self.ty_name(expected_source)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            }
            self.resolve_ty(&source_ty_hint)
        } else {
            let Some(expected_source) = expected_source else {
                return Err(TypeError {
                    message:
                        "Tuple._N requires Facet type context (e.g. Facet::view(Tuple._1, source_tuple))"
                            .into(),
                    span: span.clone(),
                    hint: Some(
                        "Use Tuple._N only where a Facet<(...), ...> is expected.".into(),
                    ),
                });
            };
            self.resolve_ty(expected_source)
        };

        let source_ty = current_source.clone();
        if let Some(root_path_name) = &path.root_path_name {
            self.validate_pending_root_source(root_path_name, &source_ty, span)?;
        }
        let mut may_fail = false;
        let mut segments = Vec::with_capacity(path.segments.len());
        for pending_segment in path.segments {
            let (segment, focus_ty, segment_may_fail) = self.resolve_facet_segment_for_source_ty(
                &current_source,
                &pending_segment,
                span,
                true,
            )?;
            segments.push(segment);
            current_source = self.resolve_ty(&focus_ty);
            may_fail |= segment_may_fail;
        }

        Ok(TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: current_source,
            path_kind: Self::facet_path_kind_for_segments(&segments),
            may_fail,
            source_readonly_root: self.ty_is_readonly_root(&source_ty),
            segments,
        })
    }

    fn resolve_facet_path_from_node(
        &mut self,
        typed: TypedNode,
        span: &Span,
        expected_source: Option<&Ty>,
    ) -> Result<TypedFacetPath, TypeError> {
        if !matches!(typed.ty, Ty::Facet(_, _)) {
            return Err(TypeError {
                message: format!("Expected Facet<...> value, got {}", self.ty_name(&typed.ty)),
                span: typed.span.clone(),
                hint: None,
            });
        }
        match typed.node {
            TypedInner::FacetPath(path) => Ok(TypedFacetPath {
                source_ty: self.resolve_ty(&path.source_ty),
                focus_ty: self.resolve_ty(&path.focus_ty),
                path_kind: path.path_kind,
                may_fail: path.may_fail,
                source_readonly_root: path.source_readonly_root,
                segments: path.segments,
            }),
            TypedInner::PendingFacetPath(path) => {
                self.specialize_pending_facet_path(path, span, expected_source)
            }
            _ => Err(TypeError {
                message:
                    "Facet values are compile-time only in Stage1 and cannot be stored or passed around"
                        .into(),
                span: span.clone(),
                hint: Some("Use type-root path expressions inline (e.g. User.name).".into()),
            }),
        }
    }

    fn compose_facet_paths(
        &mut self,
        span: &Span,
        left_path: TypedFacetPath,
        right_expr: &Resolved,
        operator_name: &str,
    ) -> Result<TypedNode, TypeError> {
        let expected_right_focus = self.env.fresh_tyvar();
        let expected_right_ty = Ty::Facet(
            Box::new(self.resolve_ty(&left_path.focus_ty)),
            Box::new(expected_right_focus),
        );
        let right = self.check_node_with_expected(right_expr, Some(&expected_right_ty))?;
        let right_path =
            self.resolve_facet_path_from_node(right, span, Some(&left_path.focus_ty))?;

        if !self.types_compatible(&left_path.focus_ty, &right_path.source_ty) {
            return Err(TypeError {
                message: format!(
                    "{} source/focus mismatch: left focus is {}, right source is {}",
                    operator_name,
                    self.ty_name(&left_path.focus_ty),
                    self.ty_name(&right_path.source_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = self.resolve_ty(&left_path.source_ty);
        let focus_ty = self.resolve_ty(&right_path.focus_ty);
        let mut segments = left_path.segments;
        segments.extend(right_path.segments);
        let path = TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            path_kind: if left_path.path_kind == TypedFacetPathKind::Variant
                || right_path.path_kind == TypedFacetPathKind::Variant
            {
                TypedFacetPathKind::Variant
            } else {
                TypedFacetPathKind::Structural
            },
            may_fail: left_path.may_fail || right_path.may_fail,
            source_readonly_root: left_path.source_readonly_root,
            segments,
        };
        Ok(TypedNode {
            ty: Ty::Facet(Box::new(source_ty), Box::new(focus_ty)),
            span: span.clone(),
            node: TypedInner::FacetPath(path),
        })
    }

    fn compose_pending_facet_paths(
        &mut self,
        span: &Span,
        mut left_path: PendingFacetPath,
        right_expr: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let right = self.check_node(right_expr)?;
        match right.node {
            TypedInner::FacetPath(path) => {
                left_path
                    .segments
                    .extend(path.segments.iter().map(Self::pending_segment_from_typed));
                Ok(self.pending_facet_node(span, left_path))
            }
            TypedInner::PendingFacetPath(path) => {
                left_path.segments.extend(path.segments);
                if left_path.root_path_name.is_none() {
                    left_path.root_path_name = path.root_path_name;
                }
                Ok(self.pending_facet_node(span, left_path))
            }
            _ => Err(TypeError {
                message:
                    "Facet values are compile-time only in Stage1 and cannot be stored or passed around"
                        .into(),
                span: span.clone(),
                hint: Some("Use type-root path expressions inline (e.g. User.name).".into()),
            }),
        }
    }

    fn check_facet_compose_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if args.len() != 2 {
            return Err(TypeError {
                message: format!("Facet::chain expects 2 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: "Facet::chain does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let ResolvedRecordLitArg::Positional(left_expr) = &args[0] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(right_expr) = &args[1] else {
            unreachable!("validated argument form above")
        };

        if let Resolved::FacetCapture(capture_span, expr) = left_expr {
            let (source_expr, pending_path) =
                self.expand_facet_capture_path("Facet::chain", capture_span, expr)?;
            let (_, _, source_value_ty) =
                self.check_facet_source_value("Facet::chain", &source_expr)?;
            let left_path =
                self.specialize_pending_facet_path(pending_path, span, Some(&source_value_ty))?;
            return self.compose_facet_paths(span, left_path, right_expr, "Facet::chain");
        }

        let left = self.check_node(left_expr)?;
        match left.node {
            TypedInner::FacetPath(path) => {
                self.compose_facet_paths(span, path, right_expr, "Facet::chain")
            }
            TypedInner::PendingFacetPath(path) => {
                self.compose_pending_facet_paths(span, path, right_expr)
            }
            _ => Err(TypeError {
                message: format!("Expected Facet<...> value, got {}", self.ty_name(&left.ty)),
                span: left.span.clone(),
                hint: None,
            }),
        }
    }

    fn check_facet_source_value(
        &mut self,
        op_name: &str,
        source_expr: &Resolved,
    ) -> Result<(TypedNode, bool, Ty), TypeError> {
        let typed_source = self.check_node(source_expr)?;
        if matches!(typed_source.ty, Ty::Facet(_, _)) {
            return Err(TypeError {
                message: format!("{} source value cannot be a Facet", op_name),
                span: typed_source.span.clone(),
                hint: None,
            });
        }

        let (source_is_result, source_value_ty) = match self.resolve_ty(&typed_source.ty) {
            Ty::Result(ok, _) => (true, ok.as_ref().clone()),
            other => (false, other),
        };

        Ok((typed_source, source_is_result, source_value_ty))
    }

    fn expand_facet_capture_path(
        &self,
        op_name: &str,
        span: &Span,
        expr: &Resolved,
    ) -> Result<(Resolved, PendingFacetPath), TypeError> {
        let mut segments = Vec::new();
        let mut current = expr.clone();
        loop {
            match current {
                Resolved::FieldAccess(_, inner, field) => {
                    segments.push(Self::pending_field_segment(field));
                    current = *inner;
                }
                Resolved::FacetSegmentAccess(_, inner, segment) => {
                    segments.push(Self::pending_segment_from_syntax(&segment));
                    current = *inner;
                }
                Resolved::Grouped(_, inner) => {
                    current = *inner;
                }
                _ => break,
            }
        }
        if segments.is_empty() {
            return Err(TypeError {
                message: format!("{op_name} shorthand requires a field or tuple path").into(),
                span: span.clone(),
                hint: Some(
                    "Write `~source.field` or `~source._0`. Use canonical `Facet::chain(...)` or a type-root path when you need a standalone Facet path."
                        .into(),
                ),
            });
        }
        segments.reverse();
        Ok((
            current,
            PendingFacetPath {
                root_path_name: None,
                source_ty_hint: None,
                segments,
            },
        ))
    }

    fn check_facet_path_input(
        &mut self,
        span: &Span,
        op_name: &str,
        path_input: FacetPathInput<'_>,
        source_value_ty: &Ty,
        source_input_ty: &Ty,
    ) -> Result<TypedFacetPath, TypeError> {
        match path_input {
            FacetPathInput::Expr(path_expr) => self.check_facet_path_argument(
                span,
                op_name,
                path_expr,
                source_value_ty,
                source_input_ty,
            ),
            FacetPathInput::Capture(path) => {
                let path = self.specialize_pending_facet_path(path, span, Some(source_value_ty))?;
                if !self.types_compatible(&path.source_ty, source_value_ty) {
                    return Err(TypeError {
                        message: format!(
                            "{} source type mismatch: facet expects {}, got {}",
                            op_name,
                            self.ty_name(&path.source_ty),
                            self.ty_name(source_input_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(path)
            }
        }
    }

    fn check_facet_path_argument(
        &mut self,
        span: &Span,
        op_name: &str,
        path_expr: &Resolved,
        source_value_ty: &Ty,
        source_input_ty: &Ty,
    ) -> Result<TypedFacetPath, TypeError> {
        let expected_focus_ty = self.env.fresh_tyvar();
        let expected_path_ty = Ty::Facet(
            Box::new(self.resolve_ty(source_value_ty)),
            Box::new(expected_focus_ty),
        );
        let path_node = self.check_node_with_expected(path_expr, Some(&expected_path_ty))?;
        let path = self.resolve_facet_path_from_node(path_node, span, Some(source_value_ty))?;

        if !self.types_compatible(&path.source_ty, source_value_ty) {
            return Err(TypeError {
                message: format!(
                    "{} source type mismatch: facet expects {}, got {}",
                    op_name,
                    self.ty_name(&path.source_ty),
                    self.ty_name(source_input_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        Ok(path)
    }

    fn parse_facet_read_intrinsic_args<'a>(
        &self,
        span: &Span,
        op_name: &str,
        args: &'a [ResolvedRecordLitArg],
    ) -> Result<(Resolved, FacetPathInput<'a>), TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!("{op_name} does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 1 && args.len() != 2 {
            return Err(TypeError {
                message: format!("{op_name} expects 1 or 2 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }

        match args {
            [ResolvedRecordLitArg::Positional(Resolved::FacetCapture(capture_span, expr))] => {
                let (source_expr, path) =
                    self.expand_facet_capture_path(op_name, capture_span, expr)?;
                Ok((source_expr, FacetPathInput::Capture(path)))
            }
            [ResolvedRecordLitArg::Positional(path_expr), ResolvedRecordLitArg::Positional(source_expr)] => {
                Ok((source_expr.clone(), FacetPathInput::Expr(path_expr)))
            }
            _ => unreachable!("validated argument form above"),
        }
    }

    fn parse_facet_mutating_intrinsic_args<'a>(
        &self,
        span: &Span,
        op_name: &str,
        args: &'a [ResolvedRecordLitArg],
    ) -> Result<(Resolved, FacetPathInput<'a>, &'a Resolved), TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!("{op_name} does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 2 && args.len() != 3 {
            return Err(TypeError {
                message: format!("{op_name} expects 2 or 3 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }

        match args {
            [ResolvedRecordLitArg::Positional(Resolved::FacetCapture(capture_span, expr)), ResolvedRecordLitArg::Positional(value_expr)] =>
            {
                let (source_expr, path) =
                    self.expand_facet_capture_path(op_name, capture_span, expr)?;
                Ok((source_expr, FacetPathInput::Capture(path), value_expr))
            }
            [ResolvedRecordLitArg::Positional(path_expr), ResolvedRecordLitArg::Positional(source_expr), ResolvedRecordLitArg::Positional(value_expr)] => {
                Ok((
                    source_expr.clone(),
                    FacetPathInput::Expr(path_expr),
                    value_expr,
                ))
            }
            _ => unreachable!("validated argument form above"),
        }
    }

    fn prepare_facet_input(
        &mut self,
        span: &Span,
        op_name: &str,
        source_expr: &Resolved,
        path_input: FacetPathInput<'_>,
    ) -> Result<PreparedFacetInput, TypeError> {
        let (typed_source, source_is_result, source_value_ty) =
            self.check_facet_source_value(op_name, source_expr)?;
        let path = self.check_facet_path_input(
            span,
            op_name,
            path_input,
            &source_value_ty,
            &typed_source.ty,
        )?;
        Ok(PreparedFacetInput {
            typed_source,
            source_is_result,
            source_value_ty,
            path,
        })
    }

    fn check_facet_view_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input) =
            self.parse_facet_read_intrinsic_args(span, "Facet::view", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            path,
            ..
        } = self.prepare_facet_input(span, "Facet::view", &source_expr, path_input)?;

        let focus_ty = self.resolve_ty(&path.focus_ty);
        let out_ty = if source_is_result || path.may_fail {
            Ty::Result(Box::new(focus_ty.clone()), Box::new(Ty::Error))
        } else {
            focus_ty
        };

        Ok(TypedNode {
            ty: out_ty,
            span: span.clone(),
            node: TypedInner::FacetView {
                source: Box::new(typed_source),
                path,
                source_is_result,
            },
        })
    }

    fn check_facet_preview_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input) =
            self.parse_facet_read_intrinsic_args(span, "Facet::preview", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            path,
            ..
        } = self.prepare_facet_input(span, "Facet::preview", &source_expr, path_input)?;
        if !path.has_variant_segment() {
            return Err(TypeError {
                message: "Facet::preview requires a variant Facet".into(),
                span: span.clone(),
                hint: Some("Use Facet::view for structural field and tuple paths.".into()),
            });
        }

        let focus_ty = self.resolve_ty(&path.focus_ty);
        Ok(TypedNode {
            ty: Ty::Result(Box::new(focus_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::FacetView {
                source: Box::new(typed_source),
                path,
                source_is_result,
            },
        })
    }

    fn check_facet_set_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, value_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::set", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            source_value_ty,
            path,
        } = self.prepare_facet_input(span, "Facet::set", &source_expr, path_input)?;
        self.check_mutating_facet_path_permissions("Facet::set", &path, span)?;

        let resolved_focus_ty = self.resolve_ty(&path.focus_ty);
        let (typed_value, mode) = if let Ty::Result(ok, _) = &resolved_focus_ty {
            let typed_value = self.check_node(value_expr)?;
            if self.types_compatible(&path.focus_ty, &typed_value.ty) {
                (typed_value, TypedFacetSetMode::Exact)
            } else if self.types_compatible(ok.as_ref(), &typed_value.ty) {
                (typed_value, TypedFacetSetMode::WrapPlainResult)
            } else {
                return Err(TypeError {
                    message: format!(
                        "Facet::set value type mismatch: expected {} or {}, got {}",
                        self.ty_name(&path.focus_ty),
                        self.ty_name(ok.as_ref()),
                        self.ty_name(&typed_value.ty)
                    ),
                    span: typed_value.span.clone(),
                    hint: None,
                });
            }
        } else {
            let typed_value = self.check_node_with_expected(value_expr, Some(&path.focus_ty))?;
            if !self.types_compatible(&path.focus_ty, &typed_value.ty) {
                return Err(TypeError {
                    message: format!(
                        "Facet::set value type mismatch: expected {}, got {}",
                        self.ty_name(&path.focus_ty),
                        self.ty_name(&typed_value.ty)
                    ),
                    span: typed_value.span.clone(),
                    hint: None,
                });
            }
            (typed_value, TypedFacetSetMode::Exact)
        };

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&source_value_ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::FacetSet {
                source: Box::new(typed_source),
                path,
                value: Box::new(typed_value),
                source_is_result,
                mode,
            },
        })
    }

    fn check_facet_put_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, value_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::put", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            source_value_ty,
            path,
        } = self.prepare_facet_input(span, "Facet::put", &source_expr, path_input)?;
        if source_is_result {
            return Err(TypeError {
                message: "Facet::put requires a plain source value".into(),
                span: typed_source.span.clone(),
                hint: Some("Use Facet::set when the source is already Result<T>.".into()),
            });
        }
        if !path.is_infallible_structural() {
            return Err(TypeError {
                message: "Facet::put requires an infallible structural Facet path".into(),
                span: span.clone(),
                hint: Some("Use Facet::set for fallible or variant-sensitive updates.".into()),
            });
        }
        self.check_mutating_facet_path_permissions("Facet::put", &path, span)?;

        let typed_value = self.check_node_with_expected(value_expr, Some(&path.focus_ty))?;
        if !self.types_compatible(&path.focus_ty, &typed_value.ty) {
            return Err(TypeError {
                message: format!(
                    "Facet::put value type mismatch: expected {}, got {}",
                    self.ty_name(&path.focus_ty),
                    self.ty_name(&typed_value.ty)
                ),
                span: typed_value.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: self.resolve_ty(&source_value_ty),
            span: span.clone(),
            node: TypedInner::FacetSet {
                source: Box::new(typed_source),
                path,
                value: Box::new(typed_value),
                source_is_result,
                mode: TypedFacetSetMode::Exact,
            },
        })
    }

    fn check_facet_over_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, update_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::over", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            source_value_ty,
            path,
        } = self.prepare_facet_input(span, "Facet::over", &source_expr, path_input)?;
        self.check_mutating_facet_path_permissions("Facet::over", &path, span)?;

        let typed_update = self.check_node(update_expr)?;
        let mode = self.check_facet_over_callable(
            "Facet::over",
            span,
            &path.focus_ty,
            &typed_update,
            false,
        )?;

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&source_value_ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::FacetOver {
                source: Box::new(typed_source),
                path,
                update_fun: Box::new(typed_update),
                source_is_result,
                mode,
            },
        })
    }

    fn require_enum_facet_path(
        &self,
        op_name: &str,
        path: &TypedFacetPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        if !path.has_variant_segment() {
            return Err(TypeError {
                message: format!("{op_name} requires an enum Facet path"),
                span: span.clone(),
                hint: Some("Use Facet::set/over for structural, list, or map paths.".into()),
            });
        }
        Ok(())
    }

    fn check_facet_case_set_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, value_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::case_set", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            source_value_ty,
            path,
        } = self.prepare_facet_input(span, "Facet::case_set", &source_expr, path_input)?;
        self.require_enum_facet_path("Facet::case_set", &path, span)?;
        if !path.final_segment_is_variant() {
            return Err(TypeError {
                message: "Facet::case_set requires the final Facet segment to be an enum case"
                    .into(),
                span: span.clone(),
                hint: Some(
                    "Use Facet::case_over when updating inside a selected case payload.".into(),
                ),
            });
        }
        self.check_mutating_facet_path_permissions("Facet::case_set", &path, span)?;

        let typed_value = self.check_node_with_expected(value_expr, Some(&path.focus_ty))?;
        if !self.types_compatible(&path.focus_ty, &typed_value.ty) {
            return Err(TypeError {
                message: format!(
                    "Facet::case_set value type mismatch: expected {}, got {}",
                    self.ty_name(&path.focus_ty),
                    self.ty_name(&typed_value.ty)
                ),
                span: typed_value.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&source_value_ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::FacetSet {
                source: Box::new(typed_source),
                path,
                value: Box::new(typed_value),
                source_is_result,
                mode: TypedFacetSetMode::CaseSet,
            },
        })
    }

    fn check_facet_case_over_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, update_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::case_over", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            source_value_ty,
            path,
        } = self.prepare_facet_input(span, "Facet::case_over", &source_expr, path_input)?;
        self.require_enum_facet_path("Facet::case_over", &path, span)?;
        self.check_mutating_facet_path_permissions("Facet::case_over", &path, span)?;

        let typed_update = self.check_node(update_expr)?;
        let mode = match self.check_facet_over_callable(
            "Facet::case_over",
            span,
            &path.focus_ty,
            &typed_update,
            false,
        )? {
            TypedFacetOverMode::FocusValue => TypedFacetOverMode::CaseFocusValue,
            TypedFacetOverMode::FocusResult => TypedFacetOverMode::CaseFocusResult,
            mode @ (TypedFacetOverMode::CaseFocusValue | TypedFacetOverMode::CaseFocusResult) => {
                mode
            }
        };

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&source_value_ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::FacetOver {
                source: Box::new(typed_source),
                path,
                update_fun: Box::new(typed_update),
                source_is_result,
                mode,
            },
        })
    }

    fn check_facet_over_result_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let (source_expr, path_input, update_expr) =
            self.parse_facet_mutating_intrinsic_args(span, "Facet::over_result", args)?;
        let PreparedFacetInput {
            typed_source,
            source_is_result,
            source_value_ty,
            path,
        } = self.prepare_facet_input(span, "Facet::over_result", &source_expr, path_input)?;
        self.check_mutating_facet_path_permissions("Facet::over_result", &path, span)?;

        if !matches!(self.resolve_ty(&path.focus_ty), Ty::Result(_, _)) {
            return Err(TypeError {
                message: format!(
                    "Facet::over_result requires Result focus, got {}",
                    self.ty_name(&path.focus_ty)
                ),
                span: span.clone(),
                hint: Some("Use Facet::over for plain focus updates.".into()),
            });
        }

        let typed_update = self.check_node(update_expr)?;
        let mode = self.check_facet_over_callable(
            "Facet::over_result",
            span,
            &path.focus_ty,
            &typed_update,
            true,
        )?;

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&source_value_ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::FacetOver {
                source: Box::new(typed_source),
                path,
                update_fun: Box::new(typed_update),
                source_is_result,
                mode,
            },
        })
    }

    fn check_facet_over_callable(
        &mut self,
        op_name: &str,
        span: &Span,
        focus_ty: &Ty,
        typed_update: &TypedNode,
        require_result_focus: bool,
    ) -> Result<TypedFacetOverMode, TypeError> {
        let (in_ty, out_ty) = self.unary_function_parts(&typed_update.ty, op_name, span)?;
        let resolved_focus_ty = self.resolve_ty(focus_ty);
        let value_focus_ty = match &resolved_focus_ty {
            Ty::Result(ok, _) => Some(ok.as_ref().clone()),
            _ => None,
        };

        let mode = if require_result_focus {
            TypedFacetOverMode::FocusResult
        } else if let Some(value_focus_ty) = &value_focus_ty {
            if self.types_compatible(value_focus_ty, &in_ty) {
                TypedFacetOverMode::FocusValue
            } else {
                TypedFacetOverMode::FocusResult
            }
        } else {
            TypedFacetOverMode::FocusValue
        };

        let expected_input_ty = match (&mode, &value_focus_ty) {
            (TypedFacetOverMode::FocusValue, Some(value_focus_ty)) => value_focus_ty,
            _ => &resolved_focus_ty,
        };
        if !self.types_compatible(expected_input_ty, &in_ty) {
            return Err(TypeError {
                message: format!(
                    "{op_name} update function input mismatch: expected {}, got {}",
                    self.ty_name(expected_input_ty),
                    self.ty_name(&in_ty)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }

        let (out_ok, out_err) = match self.resolve_ty(&out_ty) {
            Ty::Result(ok, err) => (ok.as_ref().clone(), err.as_ref().clone()),
            _ => {
                return Err(TypeError {
                    message: format!(
                        "{op_name} update function must return Result<...>, got {}",
                        self.ty_name(&out_ty)
                    ),
                    span: typed_update.span.clone(),
                    hint: None,
                });
            }
        };
        let expected_output_ty = match (&mode, &value_focus_ty) {
            (TypedFacetOverMode::FocusValue, Some(value_focus_ty)) => value_focus_ty,
            _ => &resolved_focus_ty,
        };
        if !self.types_compatible(expected_output_ty, &out_ok) {
            return Err(TypeError {
                message: format!(
                    "{op_name} update function output mismatch: expected {}, got {}",
                    self.ty_name(expected_output_ty),
                    self.ty_name(&out_ok)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }
        if !self.types_compatible(&Ty::Error, &out_err) {
            return Err(TypeError {
                message: format!(
                    "{op_name} update function error type must be Error-compatible, got {}",
                    self.ty_name(&out_err)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }

        Ok(mode)
    }

    fn readonly_type_name(ty: &Ty) -> Option<&str> {
        match ty {
            Ty::Struct(name, _) | Ty::Record(name, _) => Some(Self::surface_name(name)),
            _ => None,
        }
    }

    fn check_mutating_facet_path_permissions(
        &self,
        facet_name: &str,
        path: &TypedFacetPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        if path.source_readonly_root {
            let resolved_source_ty = self.resolve_ty(&path.source_ty);
            let type_name = Self::readonly_type_name(&resolved_source_ty).unwrap_or("<anonymous>");
            return Err(TypeError {
                message: format!(
                    "{} cannot mutably traverse readonly type {}",
                    facet_name, type_name
                ),
                span: span.clone(),
                hint: Some(
                    "Use an explicit helper that returns a replacement value instead.".into(),
                ),
            });
        }

        for (index, segment) in path.segments.iter().enumerate() {
            let is_final = index + 1 == path.segments.len();
            match segment {
                TypedFacetSegment::Field {
                    field_name,
                    container_type_name,
                    readonly,
                    focus_readonly_root,
                    focus_type_name,
                    ..
                } => {
                    if *readonly {
                        let owner_can_replace = is_final
                            && self
                                .current_impl_struct_target
                                .as_deref()
                                .map(Self::surface_name)
                                == Some(container_type_name.as_str());
                        if !owner_can_replace {
                            let hint = if is_final {
                                format!(
                                    "Only impl {} may replace the property itself.",
                                    container_type_name
                                )
                            } else {
                                format!(
                                    "Readonly field {}.{} can only be replaced as a whole; it cannot be traversed to update nested state.",
                                    container_type_name, field_name
                                )
                            };
                            return Err(TypeError {
                                message: format!(
                                    "{} cannot mutably traverse readonly field {}.{}; only the owner can replace the property itself",
                                    facet_name, container_type_name, field_name
                                ),
                                span: span.clone(),
                                hint: Some(hint),
                            });
                        }
                    }

                    if *focus_readonly_root && !is_final {
                        let type_name = focus_type_name
                            .as_deref()
                            .unwrap_or(container_type_name.as_str());
                        return Err(TypeError {
                            message: format!(
                                "{} cannot mutably traverse readonly type {}",
                                facet_name, type_name
                            ),
                            span: span.clone(),
                            hint: Some(
                                "Replace the containing property with a freshly computed value instead."
                                    .into(),
                            ),
                        });
                    }
                }
                TypedFacetSegment::Tuple {
                    focus_readonly_root,
                    focus_type_name,
                    ..
                }
                | TypedFacetSegment::Variant {
                    focus_readonly_root,
                    focus_type_name,
                    ..
                }
                | TypedFacetSegment::ListIndex {
                    focus_readonly_root,
                    focus_type_name,
                    ..
                }
                | TypedFacetSegment::ListRange {
                    focus_readonly_root,
                    focus_type_name,
                    ..
                }
                | TypedFacetSegment::MapKey {
                    focus_readonly_root,
                    focus_type_name,
                    ..
                } => {
                    if *focus_readonly_root && !is_final {
                        return Err(TypeError {
                            message: format!(
                                "{} cannot mutably traverse readonly type {}",
                                facet_name,
                                focus_type_name.as_deref().unwrap_or("<anonymous>")
                            ),
                            span: span.clone(),
                            hint: Some(
                                "Replace the containing value with an updated copy instead.".into(),
                            ),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    fn try_check_facet_intrinsic_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        if Self::is_result_chain_auto_import(func) && args.len() == 2 {
            match self.check_facet_compose_intrinsic(span, args) {
                Ok(node) => return Ok(Some(node)),
                Err(err) if Self::facet_chain_candidate_args(args) => return Err(err),
                Err(_) => {}
            }
        }
        match self.facet_intrinsic_kind(func) {
            Some("view") => Ok(Some(self.check_facet_view_intrinsic(span, args)?)),
            Some("preview") => Ok(Some(self.check_facet_preview_intrinsic(span, args)?)),
            Some("chain") => Ok(Some(self.check_facet_compose_intrinsic(span, args)?)),
            Some("put") => Ok(Some(self.check_facet_put_intrinsic(span, args)?)),
            Some("set") => Ok(Some(self.check_facet_set_intrinsic(span, args)?)),
            Some("over") => Ok(Some(self.check_facet_over_intrinsic(span, args)?)),
            Some("over_result") => Ok(Some(self.check_facet_over_result_intrinsic(span, args)?)),
            Some("case_set") => Ok(Some(self.check_facet_case_set_intrinsic(span, args)?)),
            Some("case_over") => Ok(Some(self.check_facet_case_over_intrinsic(span, args)?)),
            _ => Ok(None),
        }
    }

    fn ensure_no_runtime_facet_args(
        &self,
        args: &[TypedNode],
        span: &Span,
        callee: &str,
    ) -> Result<(), TypeError> {
        if args.iter().any(|arg| self.ty_contains_facet(&arg.ty)) {
            return Err(TypeError {
                message: format!(
                    "{} cannot accept Facet values in Stage1 (Facet is compile-time only)",
                    callee
                ),
                span: span.clone(),
                hint: Some("Apply Facet::view(...) before passing the value.".into()),
            });
        }
        Ok(())
    }

    fn ensure_no_runtime_facet_value(
        &self,
        value: &TypedNode,
        context: &str,
    ) -> Result<(), TypeError> {
        if self.ty_contains_facet(&value.ty) {
            return Err(TypeError {
                message: format!(
                    "{} cannot contain Facet values in Stage1 (Facet is compile-time only)",
                    context
                ),
                span: value.span.clone(),
                hint: Some(
                    "Consume Facet with Facet::view/set/over first, then pass the plain value."
                        .into(),
                ),
            });
        }
        Ok(())
    }

    pub(super) fn check_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if let Some(typed) = self.try_check_process_intrinsic_app(span, func, args)? {
            return Ok(typed);
        }

        if let Some(typed) = self.try_check_facet_intrinsic_app(span, func, args)? {
            return Ok(typed);
        }

        if let Some((id, trait_name, method_name)) = self.trait_method_ref(func) {
            let receiver_owner_hint = id
                .name
                .strip_suffix(&format!("::{}", method_name))
                .filter(|owner| *owner == "JsonValue");
            return self.check_trait_method_call(
                span,
                &trait_name,
                &method_name,
                args,
                receiver_owner_hint,
            );
        }

        let typed_func = self.check_node(func)?;
        let func_ty = self.resolve_ty(&typed_func.ty);

        match &func_ty {
            Ty::BuiltinFunc { name, params, ret } => {
                let callable_hint = if let TypedInner::Var(id) = &typed_func.node {
                    Some(self.call_target_signature_hint_for_id(id, params, ret.as_ref()))
                } else {
                    Some(self.call_target_signature_hint(name, params, ret.as_ref(), None))
                };
                let typed_args = self.typecheck_positional_call_args(
                    span,
                    name,
                    params,
                    args,
                    callable_hint.clone(),
                    format!("{} does not accept named arguments", name),
                )?;
                self.ensure_no_runtime_facet_args(&typed_args, span, name)?;

                if name == "__process_self" {
                    let Some(process_name) = self.current_process_name() else {
                        return Err(TypeError {
                            message: "Process::self() is only available inside process handlers"
                                .into(),
                            span: span.clone(),
                            hint: Some(
                                "Call Process::self() inside @init/@get/@set bodies of a defagent."
                                    .into(),
                            ),
                        });
                    };
                    return Ok(TypedNode {
                        ty: Ty::Pid(process_name),
                        span: span.clone(),
                        node: TypedInner::App(Box::new(typed_func), typed_args),
                    });
                }

                if name == "set_exit_code" {
                    match self.runtime_policy.exit_code_policy {
                        ExitCodePolicy::Anywhere => {}
                        ExitCodePolicy::Forbidden => {
                            return Err(TypeError {
                                message: format!(
                                    "set_exit_code is forbidden by source policy ({})",
                                    self.runtime_policy.exit_code_policy.as_str()
                                ),
                                span: span.clone(),
                                hint: Some(
                                    "This source kind does not allow set_exit_code. Use Result-based failure handling instead."
                                        .into(),
                                ),
                            });
                        }
                        ExitCodePolicy::EntryOnly => {
                            let Some(entrypoint) =
                                self.runtime_policy.normalized_entrypoint.as_ref()
                            else {
                                return Err(TypeError {
                                    message:
                                        "set_exit_code requires a normalized entrypoint but none was provided".into(),
                                    span: span.clone(),
                                    hint: Some(
                                        "Configure an entrypoint, or avoid set_exit_code in this compile unit."
                                            .into(),
                                    ),
                                });
                            };
                            if self.current_function_symbol.as_deref() != Some(entrypoint.as_str())
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "set_exit_code is only allowed inside entrypoint `{}` (policy: {})",
                                        entrypoint,
                                        self.runtime_policy.exit_code_policy.as_str()
                                    ),
                                    span: span.clone(),
                                    hint: Some(
                                        "Move set_exit_code into the configured entrypoint function."
                                            .into(),
                                    ),
                                });
                            }
                        }
                    }
                }

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::UserFunc { params, ret, .. } => {
                let has_named = args
                    .iter()
                    .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
                let callee_uid = match func {
                    Resolved::Var(_, id) => id.unique_id,
                    _ if !has_named => u32::MAX,
                    _ => {
                        return Err(TypeError {
                            message: "This function value does not accept named arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                let callable_hint =
                    self.callable_definition_signature_hint(&typed_func, params, ret.as_ref());
                let typed_args = self.typecheck_user_function_args(
                    span,
                    callee_uid,
                    params,
                    args,
                    callable_hint.as_deref(),
                )?;
                let typed_args = typed_args
                    .into_iter()
                    .map(|arg| self.concretize_pending_trait_calls(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                self.ensure_no_runtime_facet_args(&typed_args, span, "Function call")?;

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::Func(params, ret) => {
                let callable_hint =
                    self.callable_signature_hint(&Ty::Func(params.clone(), ret.clone()));
                let typed_args = self.typecheck_positional_call_args(
                    span,
                    "function",
                    params,
                    args,
                    callable_hint.clone(),
                    "Function values do not accept named arguments".into(),
                )?;
                self.ensure_no_runtime_facet_args(&typed_args, span, "Function call")?;

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            _ => Err(TypeError {
                message: format!("Not a function: {}", self.ty_name(&typed_func.ty)),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    fn typecheck_positional_call_args(
        &mut self,
        span: &Span,
        callee_name: &str,
        params: &[Ty],
        args: &[ResolvedRecordLitArg],
        callable_hint: Option<String>,
        named_arg_error: String,
    ) -> Result<Vec<TypedNode>, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: named_arg_error,
                span: span.clone(),
                hint: None,
            });
        }

        if args.len() != params.len() {
            return Err(TypeError {
                message: format!(
                    "{} expects {} argument(s), got {}",
                    callee_name,
                    params.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: callable_hint.clone(),
            });
        }

        let typed_args: Vec<TypedNode> = args
            .iter()
            .zip(params.iter())
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => match self.resolve_ty(expected) {
                    Ty::Hole => self.check_node(expr),
                    _ => self.check_node_with_expected(expr, Some(expected)),
                },
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;

        for (param, arg) in params.iter().zip(&typed_args) {
            if !matches!(self.resolve_ty(param), Ty::Hole) && !self.types_compatible(param, &arg.ty)
            {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(param),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: callable_hint.clone(),
                });
            }
        }

        typed_args
            .into_iter()
            .map(|arg| self.concretize_pending_trait_calls(arg))
            .collect::<Result<Vec<_>, _>>()
    }

    pub(super) fn current_process_name(&self) -> Option<String> {
        let symbol = self.current_function_symbol.as_deref()?;
        let (module, handler) = symbol.rsplit_once("::")?;
        Self::is_process_handler_name(handler).then(|| module.to_string())
    }

    fn try_check_process_intrinsic_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        if let Some(typed) = self.try_check_supervisor_spawn_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_supervisor_adopt_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_supervisor_status_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_supervisor_workers_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_worker_message_template_app(span, func, args)? {
            return Ok(Some(typed));
        }
        self.try_check_singleton_explicit_pid_app(span, func, args)
    }

    fn try_check_supervisor_spawn_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_spawn_target(func) else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "spawn")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!("{supervisor_process}::spawn does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 1 {
            return Err(TypeError {
                message: format!(
                    "{}::spawn expects 1 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: Some(
                    "Pass a worker init route reference like `MyWorker::init(args)`.".into(),
                ),
            });
        }
        if self.in_compiler_generated_supervisor_wrapper(&supervisor_process, "spawn") {
            return Ok(None);
        }
        let ResolvedRecordLitArg::Positional(worker_init) = &args[0] else {
            unreachable!("validated named arguments above")
        };
        if matches!(worker_init, Resolved::Var(_, _)) {
            return Ok(None);
        }
        let (worker_process, typed_init) = self.synthesize_supervisor_worker_init(worker_init)?;
        match self.resolve_ty(&typed_init.ty) {
            Ty::Func(params, _) if params.is_empty() => {}
            other => {
                return Err(TypeError {
                    message: format!(
                        "supervisor spawn expects a zero-argument worker init route, got {}",
                        self.ty_name(&other)
                    ),
                    span: typed_init.span.clone(),
                    hint: Some(
                        "Pass a generated worker init reference like `MyWorker::init(args)`."
                            .into(),
                    ),
                });
            }
        }

        Ok(Some(TypedNode {
            ty: Ty::Result(
                Box::new(Ty::Pid(worker_process.clone())),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init: Box::new(typed_init),
            },
        }))
    }

    fn try_check_supervisor_adopt_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_intrinsic_target(func, "adopt") else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "adopt")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!("{supervisor_process}::adopt does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 1 {
            return Err(TypeError {
                message: format!(
                    "{}::adopt expects 1 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: Some("Pass a worker PID.".into()),
            });
        }
        let ResolvedRecordLitArg::Positional(pid_expr) = &args[0] else {
            unreachable!("validated named arguments above")
        };
        let typed_pid = self.check_node(pid_expr)?;
        let worker_process = match self.resolve_ty(&typed_pid.ty) {
            Ty::Pid(process_name) => process_name,
            other => {
                return Err(TypeError {
                    message: format!(
                        "supervisor adopt expects PID<Worker>, got {}",
                        self.ty_name(&other)
                    ),
                    span: typed_pid.span.clone(),
                    hint: Some("Pass a worker PID returned from a worker init route.".into()),
                });
            }
        };
        let supervisor_spec = self
            .supervisor_spec_by_name(&supervisor_process)
            .ok_or_else(|| TypeError {
                message: format!("unknown supervisor process `{supervisor_process}`"),
                span: span.clone(),
                hint: None,
            })?;
        if !supervisor_spec
            .spec
            .supervisor_policy
            .as_ref()
            .map(|policy| policy.allow_adopt)
            .unwrap_or(false)
        {
            return Err(TypeError {
                message: format!(
                    "{}::adopt is not available because allow_adopt is False",
                    supervisor_process
                ),
                span: span.clone(),
                hint: Some(
                    "Enable `allow_adopt: True` in the supervisor definition or override.".into(),
                ),
            });
        }

        Ok(Some(TypedNode {
            ty: Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: Box::new(typed_pid),
            },
        }))
    }

    fn try_check_supervisor_status_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_intrinsic_target(func, "status") else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "status")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!("{supervisor_process}::status does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if !args.is_empty() {
            return Err(TypeError {
                message: format!(
                    "{}::status expects 0 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }
        let status_ty = self
            .env
            .lookup_type_def("SupervisorStatus")
            .map(|def| Ty::Struct(def.name.clone(), def.fields.clone()))
            .ok_or_else(|| TypeError {
                message: "SupervisorStatus type is not available".into(),
                span: span.clone(),
                hint: None,
            })?;
        Ok(Some(TypedNode {
            ty: Ty::Result(Box::new(status_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::SupervisorStatus { supervisor_process },
        }))
    }

    fn try_check_supervisor_workers_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_intrinsic_target(func, "workers") else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "workers")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!("{supervisor_process}::workers does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 2 {
            return Err(TypeError {
                message: format!(
                    "{}::workers expects 2 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: Some("Pass a worker init route and WorkerStrategy.".into()),
            });
        }
        if self.in_compiler_generated_supervisor_wrapper(&supervisor_process, "workers") {
            return Ok(None);
        }
        if !self.supervisor_workers_allowed_in_current_context() {
            return Err(TypeError {
                message: "supervisor workers can only be called from Singleton GenServer @init"
                    .into(),
                span: span.clone(),
                hint: Some(
                    "Create worker sets in the pool Singleton GenServer @init and keep the handle in state."
                        .into(),
                ),
            });
        }
        let ResolvedRecordLitArg::Positional(worker_init) = &args[0] else {
            unreachable!("validated named arguments above")
        };
        let ResolvedRecordLitArg::Positional(strategy_expr) = &args[1] else {
            unreachable!("validated named arguments above")
        };
        let (worker_process, typed_init) = self.synthesize_supervisor_worker_init(worker_init)?;
        match self.resolve_ty(&typed_init.ty) {
            Ty::Func(params, _) if params.is_empty() => {}
            other => {
                return Err(TypeError {
                    message: format!(
                        "supervisor workers expects a zero-argument worker init route, got {}",
                        self.ty_name(&other)
                    ),
                    span: typed_init.span.clone(),
                    hint: Some(
                        "Pass a generated worker init reference like `MyWorker::init(args)`."
                            .into(),
                    ),
                });
            }
        }
        let strategy_ty = self
            .env
            .lookup_type_def("WorkerStrategy")
            .map(|def| Ty::Struct(def.name.clone(), def.fields.clone()))
            .ok_or_else(|| TypeError {
                message: "WorkerStrategy type is not available".into(),
                span: span.clone(),
                hint: None,
            })?;
        let typed_strategy = self.check_node_with_expected(strategy_expr, Some(&strategy_ty))?;
        if !self.types_compatible(&strategy_ty, &typed_strategy.ty) {
            return Err(TypeError {
                message: format!(
                    "supervisor workers expects WorkerStrategy as worker strategy, got {}",
                    self.ty_name(&typed_strategy.ty)
                ),
                span: typed_strategy.span.clone(),
                hint: None,
            });
        }
        Ok(Some(TypedNode {
            ty: Ty::Result(
                Box::new(Ty::Enum(
                    "Workers".into(),
                    vec![Ty::Pid(worker_process.clone())],
                )),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init: Box::new(typed_init),
                strategy: Box::new(typed_strategy),
            },
        }))
    }

    fn supervisor_workers_allowed_in_current_context(&self) -> bool {
        let Some(spec) = self.current_process_spec() else {
            return false;
        };
        if spec.spec.kind != spire::ast::ProcessKind::GenServer
            || spec.spec.instance != spire::ast::ProcessInstance::Singleton
        {
            return false;
        }
        let Some(symbol) = self.current_function_symbol.as_deref() else {
            return false;
        };
        let Some((_, handler)) = symbol.rsplit_once("::") else {
            return false;
        };
        handler == "__agent_init"
    }

    fn in_compiler_generated_supervisor_wrapper(
        &self,
        supervisor_process: &str,
        method: &str,
    ) -> bool {
        let Some(symbol) = self.current_function_symbol.as_deref() else {
            return false;
        };
        let expected = format!("{}::{method}", Self::surface_name(supervisor_process));
        Self::surface_name(symbol) == expected
    }

    fn ensure_supervisor_registered_for_surface(
        &self,
        span: &Span,
        supervisor_process: &str,
        method: &str,
    ) -> Result<(), TypeError> {
        if self.in_compiler_generated_supervisor_wrapper(supervisor_process, method) {
            return Ok(());
        }
        if Self::surface_name(supervisor_process) == "DynamicSupervisor" {
            return Ok(());
        }
        let registered = self.boot_plan.entries.iter().any(|entry| {
            Self::surface_name(&entry.process_name) == Self::surface_name(supervisor_process)
        }) || self.boot_plan.supervisors.iter().any(|entry| {
            Self::surface_name(&entry.process_name) == Self::surface_name(supervisor_process)
        });
        if registered {
            return Ok(());
        }
        Err(TypeError {
            message: format!(
                "supervisor surface `{}::{method}` is not available in this compile unit; add the supervisor to supervisor_init",
                Self::surface_name(supervisor_process)
            ),
            span: span.clone(),
            hint: Some("Register custom supervisors in supervisor_init before using their generated supervisor surface.".into()),
        })
    }

    fn try_check_worker_message_template_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Ok(None);
        }
        let Resolved::Var(_, id) = func else {
            return Ok(None);
        };
        let qualified = id.qualified_name.as_deref().unwrap_or(&id.name);
        let Some((process_name, _method_name)) = qualified.rsplit_once("::") else {
            return Ok(None);
        };
        let Some(process_name) = self
            .process_specs
            .iter()
            .find(|spec| {
                spec.process_name == process_name
                    && spec.spec.instance == spire::ast::ProcessInstance::Worker
            })
            .map(|spec| spec.process_name.clone())
        else {
            return Ok(None);
        };
        let typed_func = self.check_node(func)?;
        let func_ty = self.resolve_ty(&typed_func.ty);
        let (params, ret) = match &func_ty {
            Ty::UserFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            Ty::BuiltinFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            _ => return Ok(None),
        };
        let Some((first_param, remaining_params)) = params.split_first() else {
            return Ok(None);
        };
        let Ty::Pid(pid_process) = self.resolve_ty(first_param) else {
            return Ok(None);
        };
        if Self::surface_name(&pid_process) != Self::surface_name(&process_name) {
            return Ok(None);
        }
        if args.len() != remaining_params.len() {
            return Ok(None);
        }
        let typed_args: Vec<TypedNode> = args
            .iter()
            .zip(remaining_params.iter())
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (expected, actual) in remaining_params.iter().zip(typed_args.iter()) {
            if !self.types_compatible(expected, &actual.ty) {
                return Ok(None);
            }
        }
        Ok(Some(TypedNode {
            ty: Ty::Func(vec![Ty::Pid(process_name)], ret),
            span: span.clone(),
            node: TypedInner::InjectCall(Box::new(typed_func), typed_args),
        }))
    }

    fn try_check_singleton_explicit_pid_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Ok(None);
        }
        let Resolved::Var(_, id) = func else {
            return Ok(None);
        };
        let qualified = id.qualified_name.as_deref().unwrap_or(&id.name);
        let Some((process_name, method_name)) = qualified.rsplit_once("::") else {
            return Ok(None);
        };
        if method_name == "pid" {
            return Ok(None);
        }
        let Some(process_name) = self
            .process_specs
            .iter()
            .find(|spec| {
                Self::surface_name(&spec.process_name) == Self::surface_name(process_name)
                    && spec.spec.instance == spire::ast::ProcessInstance::Singleton
            })
            .map(|spec| spec.process_name.clone())
        else {
            return Ok(None);
        };

        let typed_func = self.check_node(func)?;
        let func_ty = self.resolve_ty(&typed_func.ty);
        let (params, ret) = match &func_ty {
            Ty::UserFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            Ty::BuiltinFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            _ => return Ok(None),
        };
        if args.len() != params.len() + 1 {
            return Ok(None);
        }

        let pid_ty = Ty::Pid(process_name.clone());
        let typed_pid = match &args[0] {
            ResolvedRecordLitArg::Positional(expr) => {
                self.check_node_with_expected(expr, Some(&pid_ty))?
            }
            ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
        };
        if !self.types_compatible(&pid_ty, &typed_pid.ty) {
            return Ok(None);
        }

        let typed_args: Vec<TypedNode> = args[1..]
            .iter()
            .zip(params.iter())
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (expected, actual) in params.iter().zip(typed_args.iter()) {
            if !self.types_compatible(expected, &actual.ty) {
                return Ok(None);
            }
        }

        let rewritten_call = TypedNode {
            ty: (*ret).clone(),
            span: span.clone(),
            node: TypedInner::App(Box::new(typed_func), typed_args),
        };
        Ok(Some(TypedNode {
            ty: (*ret).clone(),
            span: span.clone(),
            node: TypedInner::Block(vec![typed_pid, rewritten_call]),
        }))
    }

    fn supervisor_spawn_target(&self, func: &Resolved) -> Option<String> {
        self.supervisor_intrinsic_target(func, "spawn")
    }

    fn supervisor_intrinsic_target(&self, func: &Resolved, method: &str) -> Option<String> {
        let Resolved::Var(_, id) = func else {
            return None;
        };
        let qualified = id.qualified_name.as_deref()?;
        let process_name = qualified.strip_suffix(&format!("::{method}"))?;
        if Self::surface_name(process_name) == "Supervisor" {
            return None;
        }
        self.supervisor_spec_by_name(process_name)
            .map(|spec| spec.process_name.clone())
    }

    fn supervisor_spec_by_name(&self, process_name: &str) -> Option<&TypedProcessSpec> {
        self.process_specs.iter().find(|spec| {
            Self::surface_name(&spec.process_name) == Self::surface_name(process_name)
                && matches!(
                    spec.spec.kind,
                    spire::ast::ProcessKind::Supervisor
                        | spire::ast::ProcessKind::DynamicSupervisor
                        | spire::ast::ProcessKind::RuntimeSupervisor
                )
        })
    }

    fn worker_process_spec_for_init_route<'a>(
        &'a self,
        qualified: &str,
    ) -> Option<(
        &'a TypedProcessSpec,
        &'a spire::ast::ProcessRuntimeHandlerSpec,
    )> {
        self.process_specs.iter().find_map(|spec| {
            if spec.spec.instance != spire::ast::ProcessInstance::Worker {
                return None;
            }
            spec.spec
                .handler_specs
                .iter()
                .find(|handler| {
                    handler.kind == spire::ast::ProcessRuntimeHandlerKind::Init
                        && Self::surface_name(qualified)
                            == Self::surface_name(&format!(
                                "{}::{}",
                                spec.process_name, handler.name
                            ))
                })
                .map(|handler| (spec, handler))
        })
    }

    fn worker_process_spec_for_internal_init_uid(&self, uid: u32) -> Option<&TypedProcessSpec> {
        self.process_specs.iter().find(|spec| {
            spec.spec.instance == spire::ast::ProcessInstance::Worker && spec.init_uid == uid
        })
    }

    fn synthesize_supervisor_worker_init(
        &mut self,
        worker_init: &Resolved,
    ) -> Result<(String, TypedNode), TypeError> {
        let span = self.resolved_span(worker_init).clone();
        if let Resolved::Closure(_, params, _, body) = worker_init {
            if params.is_empty() {
                if let Resolved::App(_, func, _) = body.as_ref() {
                    if let Resolved::Var(_, id) = func.as_ref() {
                        if let Some(process_spec) =
                            self.worker_process_spec_for_internal_init_uid(id.unique_id)
                        {
                            let process_name = process_spec.process_name.clone();
                            let typed_init = self.check_node(worker_init)?;
                            return Ok((process_name, typed_init));
                        }
                    }
                }
            }
        }
        let Resolved::App(_, func, args) = worker_init else {
            return Err(TypeError {
                message: "supervisor spawn expects a worker init route reference".into(),
                span,
                hint: Some("Use `MyWorker::init(args)`.".into()),
            });
        };
        let Resolved::Var(_, id) = func.as_ref() else {
            return Err(TypeError {
                message: "supervisor spawn expects a worker init route reference".into(),
                span,
                hint: Some("Use `MyWorker::init(args)`.".into()),
            });
        };
        let qualified = id.qualified_name.as_deref().unwrap_or(&id.name);
        let Some((process_spec, init_handler)) = self.worker_process_spec_for_init_route(qualified)
        else {
            return Err(TypeError {
                message: "supervisor spawn expects a worker init route reference".into(),
                span,
                hint: Some("Use `MyWorker::init(args)`.".into()),
            });
        };
        let process_name = process_spec.process_name.clone();
        let internal_name = init_handler.internal_name.clone();
        let init_uid = process_spec.init_uid;
        let synthetic = Resolved::Closure(
            span.clone(),
            Vec::new(),
            Vec::new(),
            Box::new(Resolved::App(
                span.clone(),
                Box::new(Resolved::Var(
                    span.clone(),
                    ResolvedId {
                        name: internal_name.clone(),
                        qualified_name: Some(format!("{}::{}", process_name, internal_name)),
                        unique_id: init_uid,
                        compiler_generated: true,
                        span: span.clone(),
                    },
                )),
                args.clone(),
            )),
        );
        let typed_init = self.check_node(&synthetic)?;
        Ok((process_name, typed_init))
    }

    fn check_process_context_handler(
        &mut self,
        span: &Span,
        slot: &str,
    ) -> Result<TypedNode, TypeError> {
        let Some(process_name) = self.current_process_name() else {
            return Err(TypeError {
                message: "ctx.<slot> is only available inside process handlers".into(),
                span: span.clone(),
                hint: Some("Use ctx.<slot> inside @init/@get/@set/@call/@cast bodies.".into()),
            });
        };
        let Some(capability) = self
            .process_handler_dependencies
            .get(&process_name)
            .and_then(|slots| slots.get(slot))
            .cloned()
        else {
            return Err(TypeError {
                message: format!(
                    "handler slot `{}` is not declared for process `{}`",
                    slot,
                    Self::surface_name(&process_name)
                ),
                span: span.clone(),
                hint: Some("Declare the slot in meta.handlers before using ctx.<slot>.".into()),
            });
        };
        Ok(TypedNode {
            ty: Ty::Pid(capability),
            span: span.clone(),
            node: TypedInner::ProcessContextHandler {
                process_name,
                slot: slot.to_string(),
            },
        })
    }

    pub(super) fn check_closure(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let saved_function_return_ty = self.function_return_ty.clone();
        let saved_current_function_symbol = self.current_function_symbol.clone();
        let saved_current_impl_struct_target = self.current_impl_struct_target.clone();
        let saved_in_extractor_body = self.in_extractor_body;
        let saved_closure_depth = self.closure_depth;
        let saved_facet_bindings = self.facet_bindings.clone();

        self.env.push_var_scope();
        self.closure_depth = self.closure_depth.saturating_add(1);
        let result = (|| -> Result<TypedNode, TypeError> {
            let mut typed_params = Vec::new();
            let param_tys = match expected {
                Some(Ty::Func(expected_params, _)) => {
                    if expected_params.len() != params.len() {
                        return Err(TypeError {
                            message: format!(
                                "closure expects {} parameter(s), got {}",
                                expected_params.len(),
                                params.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    expected_params.clone()
                }
                Some(other) => {
                    return Err(TypeError {
                        message: format!("Expected function type, got {}", self.ty_name(other)),
                        span: span.clone(),
                        hint: None,
                    });
                }
                None => params
                    .iter()
                    .map(|param| match &param.ty {
                        Some(ast_ty) => {
                            self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())
                        }
                        None => Ok(self.env.fresh_tyvar()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            };

            for (param, param_ty) in params.iter().zip(param_tys.iter()) {
                let param_ty = if let Some(ast_ty) = &param.ty {
                    let annotated =
                        self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                    if !self.types_compatible(param_ty, &annotated) {
                        return Err(TypeError {
                            message: format!(
                                "closure parameter `{}` expected {}, got {}",
                                param.id.name,
                                self.ty_name(param_ty),
                                self.ty_name(&annotated)
                            ),
                            span: param.id.span.clone(),
                            hint: None,
                        });
                    }
                    self.resolve_ty(&annotated)
                } else {
                    self.resolve_ty(param_ty)
                };
                self.env.bind_var(param.id.unique_id, param_ty.clone());
                typed_params.push(TypedClosureParam {
                    id: param.id.clone(),
                    ty: param_ty,
                });
            }

            for capture in captures {
                if let Some(ty) = self.env.lookup_var(capture.unique_id).cloned() {
                    let resolved_ty = self.resolve_ty(&ty);
                    self.env.bind_var(capture.unique_id, resolved_ty);
                }
            }

            if let Some(Ty::Func(_, expected_ret)) = expected {
                self.function_return_ty = Some(expected_ret.as_ref().clone());
            }
            let profile = self.profiler.start();
            let typed_body = self.check_node(body)?;
            self.profiler.finish(ProfileEvent::ClosureBody, profile);
            let typed_body = self.concretize_pending_trait_calls(typed_body)?;
            if expected.is_none() {
                if let Some((method_name, pending_span)) =
                    self.first_pending_trait_helper(&typed_body)
                {
                    return Err(self.pending_trait_helper_error(method_name, pending_span));
                }
            }
            if matches!(typed_body.ty, Ty::Facet(_, _)) {
                return Err(TypeError {
                    message:
                        "Facet is compile-time only in Stage1 and cannot be returned from closures"
                            .into(),
                    span: typed_body.span.clone(),
                    hint: Some("Use Facet::view(...) inside the closure instead.".into()),
                });
            }
            // The closure result type is needed immediately for inference, but
            // the body tree itself is normalized by the enclosing typed node.
            let body_ty = self.resolve_ty(&typed_body.ty);
            if let Some(Ty::Func(_, expected_ret)) = expected {
                let expected_ret = self.resolve_ty(expected_ret);
                if matches!(expected_ret, Ty::Unit)
                    && !self.types_compatible(&expected_ret, &body_ty)
                {
                    let err = TypeError {
                        message: format!(
                            "Argument type mismatch: expected {}, got {}",
                            self.ty_name(&expected_ret),
                            self.ty_name(&body_ty)
                        ),
                        span: typed_body.span.clone(),
                        hint: None,
                    };
                    return Err(err);
                }
            }

            let param_tys = typed_params
                .iter()
                .map(|p| self.resolve_ty(&p.ty))
                .collect::<Vec<_>>();
            Ok(TypedNode {
                ty: Ty::Func(param_tys, Box::new(body_ty)),
                span: span.clone(),
                node: TypedInner::Closure(
                    typed_params
                        .into_iter()
                        .map(|param| TypedClosureParam {
                            id: param.id,
                            ty: self.resolve_ty(&param.ty),
                        })
                        .collect(),
                    captures.to_vec(),
                    Box::new(typed_body),
                ),
            })
        })();

        self.env.pop_var_scope();
        self.function_return_ty = saved_function_return_ty;
        self.current_function_symbol = saved_current_function_symbol;
        self.current_impl_struct_target = saved_current_impl_struct_target;
        self.in_extractor_body = saved_in_extractor_body;
        self.closure_depth = saved_closure_depth;
        self.facet_bindings = saved_facet_bindings;
        result
    }

    pub(super) fn check_capture(
        &mut self,
        span: &Span,
        target: &Resolved,
        args: &[Resolved],
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        if !args.is_empty() {
            return Err(TypeError {
                message: "capture calls with arguments must be lowered before type checking".into(),
                span: span.clone(),
                hint: None,
            });
        }

        if self.trait_method_ref(target).is_some() {
            let Some(expected_ty) = expected else {
                if let Resolved::Var(_, id) = target {
                    return Err(TypeError {
                        message: format!(
                            "Trait helper `{}` needs expected callable type or same-expression inference evidence",
                            id.name
                        ),
                        span: span.clone(),
                        hint: Some(
                            "Add a callable annotation such as `cmp: (Int, Int -> Ordering) = &compare`, or use the capture inside an expression like `&compare `Function::on` _.field`."
                                .into(),
                        ),
                    });
                }
                return Err(TypeError {
                    message:
                        "Trait helper capture needs expected callable type or same-expression inference evidence"
                            .into(),
                    span: span.clone(),
                    hint: Some(
                        "Add a callable annotation or use the capture where the receiver type can be inferred."
                            .into(),
                    ),
                });
            };
            let Ty::Func(params, _) = self.resolve_ty(expected_ty) else {
                return Err(TypeError {
                    message: format!(
                        "Expected function type, got {}",
                        self.ty_name(&self.resolve_ty(expected_ty))
                    ),
                    span: span.clone(),
                    hint: None,
                });
            };
            let mut closure_params = Vec::with_capacity(params.len());
            let mut body_args = Vec::with_capacity(params.len());
            for index in 0..params.len() {
                let param_id = ResolvedId {
                    name: format!("__trait_helper_arg_{}", index),
                    qualified_name: None,
                    unique_id: Self::next_synthetic_range_uid(),
                    compiler_generated: true,
                    span: span.clone(),
                };
                body_args.push(ResolvedRecordLitArg::Positional(Resolved::Var(
                    span.clone(),
                    param_id.clone(),
                )));
                closure_params.push(ResolvedClosureParam {
                    id: param_id,
                    ty: None,
                });
            }
            let synthetic = Resolved::Closure(
                span.clone(),
                closure_params,
                Vec::new(),
                Box::new(Resolved::App(
                    span.clone(),
                    Box::new(target.clone()),
                    body_args,
                )),
            );
            return self.check_node_with_expected(&synthetic, Some(expected_ty));
        }

        if Self::capture_target_is_facet_path(target) {
            if let Some(expected_ty) = expected {
                let expected_ty_resolved = self.resolve_ty(expected_ty);
                if let Ty::Func(params, _) = &expected_ty_resolved {
                    if params.len() == 1 {
                        let view_id = self.runtime_helper_id("Facet::view", span)?;
                        let param_uid = Self::next_synthetic_range_uid();
                        let param_id = ResolvedId {
                            name: "__facet_capture_arg".to_string(),
                            qualified_name: None,
                            unique_id: param_uid,
                            compiler_generated: true,
                            span: span.clone(),
                        };
                        let synthetic = Resolved::Closure(
                            span.clone(),
                            vec![ResolvedClosureParam {
                                id: param_id.clone(),
                                ty: None,
                            }],
                            Vec::new(),
                            Box::new(Resolved::App(
                                span.clone(),
                                Box::new(Resolved::Var(span.clone(), view_id)),
                                vec![
                                    ResolvedRecordLitArg::Positional(target.clone()),
                                    ResolvedRecordLitArg::Positional(Resolved::Var(
                                        span.clone(),
                                        param_id,
                                    )),
                                ],
                            )),
                        );
                        return self.check_node_with_expected(&synthetic, Some(expected_ty));
                    }
                }
            }
        }

        let typed_target = self.check_node(target)?;
        let target_ty = self.resolve_ty(&typed_target.ty);
        if let Ty::Facet(source_ty, focus_ty) = &target_ty {
            let view_id = self.runtime_helper_id("Facet::view", span)?;
            let param_uid = Self::next_synthetic_range_uid();
            let param_name = "__facet_capture_arg".to_string();
            let param_id = ResolvedId {
                name: param_name.clone(),
                qualified_name: None,
                unique_id: param_uid,
                compiler_generated: true,
                span: span.clone(),
            };
            let captures = match target {
                Resolved::Var(_, id) => vec![id.clone()],
                _ => Vec::new(),
            };
            let body = Resolved::App(
                span.clone(),
                Box::new(Resolved::Var(span.clone(), view_id)),
                vec![
                    ResolvedRecordLitArg::Positional(target.clone()),
                    ResolvedRecordLitArg::Positional(Resolved::Var(span.clone(), param_id.clone())),
                ],
            );
            let ret_ty = match &typed_target.node {
                TypedInner::FacetPath(path) if path.may_fail => Ty::Result(
                    Box::new(self.resolve_ty(focus_ty.as_ref())),
                    Box::new(Ty::Error),
                ),
                _ => self.resolve_ty(focus_ty.as_ref()),
            };
            let synthetic = Resolved::Closure(
                span.clone(),
                vec![ResolvedClosureParam {
                    id: param_id,
                    ty: None,
                }],
                captures,
                Box::new(body),
            );
            let expected = Ty::Func(vec![self.resolve_ty(source_ty.as_ref())], Box::new(ret_ty));
            return self
                .check_node_with_expected(&synthetic, Some(&expected))
                .or_else(|_| self.check_node(&synthetic));
        }
        let (params, ret) = match &target_ty {
            Ty::BuiltinFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::UserFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!("Not a function: {}", self.ty_name(other)),
                    span: typed_target.span.clone(),
                    hint: Some(
                        "Capture (`&`) requires a function name, function value, or closure."
                            .into(),
                    ),
                });
            }
        };
        Ok(TypedNode {
            ty: Ty::Func(
                params.into_iter().map(|ty| self.resolve_ty(&ty)).collect(),
                Box::new(self.resolve_ty(&ret)),
            ),
            span: span.clone(),
            node: TypedInner::Capture(Box::new(typed_target), Vec::new()),
        })
    }

    pub(super) fn maybe_call_zero_arg_function(
        &self,
        node: TypedNode,
        _call_span: Span,
    ) -> TypedNode {
        match &node.ty {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret)
                if params.is_empty() =>
            {
                TypedNode {
                    ty: ret.as_ref().clone(),
                    span: node.span.clone(),
                    node: TypedInner::App(Box::new(node), Vec::new()),
                }
            }
            _ => node,
        }
    }

    fn capture_target_is_facet_path(target: &Resolved) -> bool {
        matches!(
            target,
            Resolved::FieldAccess(_, _, _) | Resolved::FacetSegmentAccess(_, _, _)
        )
    }

    pub(super) fn check_binop(
        &mut self,
        span: &Span,
        op: &BinOp,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        if matches!(op, BinOp::Slash) {
            return self.check_slash_compose(span, left, right);
        }

        let typed_left = self.check_node(left)?;
        let typed_right = self.check_node(right)?;
        let lt = self.resolve_ty(&typed_left.ty);
        let rt = self.resolve_ty(&typed_right.ty);
        let compatibility_checkpoint = self.substitutions.clone();
        let compatible = self.types_compatible(&lt, &rt);

        let make_trait_call = |trait_name: String,
                               method_name: &str,
                               receiver_ty: Ty,
                               dispatch: TraitDispatch,
                               result_ty: Ty,
                               origin: TraitCallOrigin,
                               typed_left: TypedNode,
                               typed_right: TypedNode| {
            TypedNode {
                ty: result_ty,
                span: span.clone(),
                node: TypedInner::TraitCall {
                    trait_name,
                    method_name: method_name.into(),
                    receiver_ty,
                    dispatch,
                    origin,
                    args: vec![typed_left, typed_right],
                },
            }
        };

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                let (trait_short_name, method_name, symbol) = match op {
                    BinOp::Add => ("Add", "add", "+"),
                    BinOp::Sub => ("Sub", "sub", "-"),
                    BinOp::Mul => ("Mul", "mul", "*"),
                    _ => unreachable!("validated above"),
                };
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    let summary = self.trait_implementation_summary(trait_short_name);
                    return Err(TypeError {
                        message: format!(
                            "`{}` requires the same type on both sides, but got {} and {}",
                            symbol,
                            self.ty_name(&lt),
                            self.ty_name(&rt),
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(format!(
                            "Operator `{:?}` requires compatible operand types. Left operand is {}, right operand is {}.\n{}",
                            op,
                            self.ty_name(&lt),
                            self.ty_name(&rt),
                            summary
                        )),
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let operator_trait =
                    self.trait_key_by_short_name(trait_short_name)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown trait: {}", trait_short_name),
                            span: span.clone(),
                            hint: None,
                        })?;
                let dispatch = self
                    .trait_dispatch_target(&operator_trait, method_name, &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "`{}` is not defined for {}",
                            symbol,
                            self.ty_name(&receiver_ty)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.trait_implementation_summary(trait_short_name)),
                    })?;
                Ok(make_trait_call(
                    operator_trait,
                    method_name,
                    receiver_ty.clone(),
                    dispatch,
                    receiver_ty,
                    TraitCallOrigin::Explicit,
                    typed_left,
                    typed_right,
                ))
            }
            BinOp::Slash => unreachable!("handled before generic binop path"),
            BinOp::Eq | BinOp::Neq => {
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    let summary = self.trait_implementation_summary(match op {
                        BinOp::Eq => "Eq",
                        BinOp::Neq => "Neq",
                        _ => unreachable!("validated above"),
                    });
                    return Err(TypeError {
                        message: format!(
                            "`{}` requires the same type on both sides, but got {} and {}",
                            match op {
                                BinOp::Eq => "==",
                                BinOp::Neq => "!=",
                                _ => unreachable!("validated above"),
                            },
                            self.ty_name(&lt),
                            self.ty_name(&rt),
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(summary),
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let (trait_short_name, method_name, symbol) = match op {
                    BinOp::Eq => ("Eq", "eq", "=="),
                    BinOp::Neq => ("Neq", "neq", "!="),
                    _ => unreachable!("validated above"),
                };
                let eq_trait = self
                    .trait_key_by_short_name(trait_short_name)
                    .ok_or_else(|| TypeError {
                        message: format!("Unknown trait: {}", trait_short_name),
                        span: span.clone(),
                        hint: None,
                    })?;
                let dispatch = self
                    .trait_dispatch_target(&eq_trait, method_name, &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "`{}` is not defined for {}",
                            symbol,
                            self.ty_name(&receiver_ty)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.trait_implementation_summary(trait_short_name)),
                    })?;
                Ok(make_trait_call(
                    eq_trait,
                    method_name,
                    receiver_ty,
                    dispatch,
                    Ty::Bool,
                    TraitCallOrigin::Explicit,
                    typed_left,
                    typed_right,
                ))
            }
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => {
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    let summary = self.trait_implementation_summary("Compare");
                    return Err(TypeError {
                        message: format!(
                            "`{}` requires the same type on both sides, but got {} and {}",
                            match op {
                                BinOp::Lt => "<",
                                BinOp::Gt => ">",
                                BinOp::Lte => "<=",
                                BinOp::Gte => ">=",
                                _ => unreachable!("validated above"),
                            },
                            self.ty_name(&lt),
                            self.ty_name(&rt),
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(summary),
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let (comparison_op, symbol) = match op {
                    BinOp::Lt => (ComparisonOperator::Lt, "<"),
                    BinOp::Gt => (ComparisonOperator::Gt, ">"),
                    BinOp::Lte => (ComparisonOperator::Lte, "<="),
                    BinOp::Gte => (ComparisonOperator::Gte, ">="),
                    _ => unreachable!("validated above"),
                };
                let method_name = match comparison_op {
                    ComparisonOperator::Lt => "lt",
                    ComparisonOperator::Lte => "lte",
                    ComparisonOperator::Gt => "gt",
                    ComparisonOperator::Gte => "gte",
                };
                let compare_trait =
                    self.trait_key_by_short_name("Compare")
                        .ok_or_else(|| TypeError {
                            message: "Unknown trait: Compare".into(),
                            span: span.clone(),
                            hint: None,
                        })?;
                let dispatch = self
                    .trait_dispatch_target(&compare_trait, method_name, &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "`{}` is not defined for {}",
                            symbol,
                            self.ty_name(&receiver_ty)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.trait_implementation_summary("Compare")),
                    })?;
                Ok(make_trait_call(
                    compare_trait,
                    method_name,
                    receiver_ty,
                    dispatch,
                    Ty::Bool,
                    TraitCallOrigin::Comparison {
                        op: comparison_op,
                        lhs_ty: self.resolve_ty(&lt),
                        rhs_ty: self.resolve_ty(&rt),
                    },
                    typed_left,
                    typed_right,
                ))
            }
            BinOp::Concat => {
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    return Err(TypeError {
                        message: format!(
                            "++ requires (String, String), got ({}, {})",
                            self.ty_name(&lt),
                            self.ty_name(&rt)
                        ),
                        span: typed_right.span.clone(),
                        hint: Some(self.trait_implementation_summary("Concat")),
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let concat_trait =
                    self.trait_key_by_short_name("Concat")
                        .ok_or_else(|| TypeError {
                            message: "Unknown trait: Concat".into(),
                            span: span.clone(),
                            hint: None,
                        })?;
                let dispatch = self
                    .trait_dispatch_target(&concat_trait, "concat", &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!("`++` is not defined for {}", self.ty_name(&receiver_ty)),
                        span: typed_right.span.clone(),
                        hint: Some(self.trait_implementation_summary("Concat")),
                    })?;
                Ok(make_trait_call(
                    concat_trait,
                    "concat",
                    receiver_ty.clone(),
                    dispatch,
                    receiver_ty,
                    TraitCallOrigin::Explicit,
                    typed_left,
                    typed_right,
                ))
            }
        }
    }

    fn check_slash_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        if matches!(typed_left.ty, Ty::Facet(_, _)) {
            return match typed_left.node {
                TypedInner::FacetPath(path) => self.compose_facet_paths(span, path, right, "`/`"),
                TypedInner::PendingFacetPath(path) => {
                    self.compose_pending_facet_paths(span, path, right)
                }
                _ => Err(TypeError {
                    message: format!(
                        "Expected Facet<...> value, got {}",
                        self.ty_name(&typed_left.ty)
                    ),
                    span: typed_left.span.clone(),
                    hint: None,
                }),
            };
        }

        let typed_right = self.check_node(right)?;
        let receiver_ty = self.resolve_ty(&typed_left.ty);
        let rhs_ty = self.resolve_ty(&typed_right.ty);
        let compose_trait = self
            .trait_key_by_short_name("Compose")
            .ok_or_else(|| TypeError {
                message: "Unknown trait: Compose".into(),
                span: span.clone(),
                hint: None,
            })?;
        let result_ty = self.env.fresh_tyvar();
        let Some((dispatch, resolved_trait_args)) = self.operator_trait_dispatch_for_args(
            &compose_trait,
            "compose",
            &receiver_ty,
            &[rhs_ty.clone(), result_ty],
        ) else {
            let hint = if matches!(receiver_ty, Ty::Int | Ty::Float)
                || matches!(rhs_ty, Ty::Int | Ty::Float)
            {
                Some(
                    "Infix `/` is reserved for compose/join. Use `safe_div(...)` for division."
                        .into(),
                )
            } else {
                None
            };
            return Err(TypeError {
                message: format!(
                    "`/` requires Compose implementation on the left, got {} and {}",
                    self.ty_name(&receiver_ty),
                    self.ty_name(&rhs_ty)
                ),
                span: span.clone(),
                hint,
            });
        };
        let result_ty = resolved_trait_args
            .get(1)
            .cloned()
            .unwrap_or_else(|| self.resolve_ty(&typed_left.ty));
        let trait_name = self.trait_instance_key_from_tys(&compose_trait, &resolved_trait_args);
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name,
                method_name: "chain".into(),
                receiver_ty: receiver_ty.clone(),
                dispatch,
                origin: TraitCallOrigin::Operator {
                    op: OperatorTraitOp::SlashCompose,
                    lhs_ty: receiver_ty,
                    rhs_ty,
                },
                args: vec![typed_left, typed_right],
            },
        })
    }

    pub(super) fn check_list_nil(&mut self, span: &Span) -> Result<TypedNode, TypeError> {
        let tv = self.env.fresh_tyvar();
        Ok(TypedNode {
            ty: Ty::List(Box::new(tv)),
            span: span.clone(),
            node: TypedInner::ListNil,
        })
    }

    pub(super) fn check_list_cons(
        &mut self,
        span: &Span,
        head: &Resolved,
        tail: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_head = self.check_node(head)?;
        let typed_tail = self.check_node(tail)?;
        self.ensure_no_runtime_facet_value(&typed_head, "List construction")?;
        self.ensure_no_runtime_facet_value(&typed_tail, "List construction")?;
        let tail_elem_ty = match &typed_tail.ty {
            Ty::List(inner) => inner.as_ref().clone(),
            other => {
                return Err(TypeError {
                    message: format!("list tail must be List<...>, got {}", self.ty_name(other)),
                    span: typed_tail.span.clone(),
                    hint: Some("Use `[head, ..tail]` with a list tail value".into()),
                });
            }
        };

        if !self.types_compatible(&typed_head.ty, &tail_elem_ty) {
            return Err(TypeError {
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(&tail_elem_ty),
                    self.ty_name(&typed_head.ty)
                ),
                span: typed_head.span.clone(),
                hint: Some("List head and tail element types must match".into()),
            });
        }

        let elem_ty = self.resolve_ty(&tail_elem_ty);
        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty.clone())),
            span: span.clone(),
            node: TypedInner::ListCons(Box::new(typed_head), Box::new(typed_tail)),
        })
    }

    pub(super) fn check_list_literal(
        &mut self,
        span: &Span,
        elems: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        if elems.is_empty() {
            return self.check_list_nil(span);
        }

        let typed_elems: Vec<TypedNode> = elems
            .iter()
            .map(|e| self.check_node(e))
            .collect::<Result<Vec<_>, _>>()?;
        for typed in &typed_elems {
            self.ensure_no_runtime_facet_value(typed, "List literal")?;
        }

        let elem_ty = typed_elems[0].ty.clone();
        for te in typed_elems.iter().skip(1) {
            if !self.types_compatible(&elem_ty, &te.ty) {
                return Err(TypeError {
                    message: format!(
                        "expected {}, got {}",
                        self.ty_name(&elem_ty),
                        self.ty_name(&te.ty)
                    ),
                    span: te.span.clone(),
                    hint: Some("All list elements must have the same type".into()),
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty)),
            span: span.clone(),
            node: TypedInner::ListLiteral(typed_elems),
        })
    }

    pub(super) fn check_range_literal(
        &mut self,
        span: &Span,
        start: &Resolved,
        stop: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_start = self.check_node(start)?;
        let typed_stop = self.check_node(stop)?;
        self.ensure_no_runtime_facet_value(&typed_start, "Range literal")?;
        self.ensure_no_runtime_facet_value(&typed_stop, "Range literal")?;

        let start_ty = self.resolve_ty(&typed_start.ty);
        let stop_ty = self.resolve_ty(&typed_stop.ty);
        match (&start_ty, &stop_ty) {
            (Ty::Int, Ty::Int) => {
                if let (Some(start_int), Some(stop_int)) = (
                    Self::typed_int_literal_value(&typed_start),
                    Self::typed_int_literal_value(&typed_stop),
                ) {
                    return Ok(self.fold_int_range_literal(span, start_int, stop_int));
                }
                self.lower_int_range_runtime(span, start.clone(), stop.clone())
            }
            (Ty::Str, Ty::Str) => {
                if let (Some(start_str), Some(stop_str)) = (
                    Self::typed_string_literal_value(&typed_start),
                    Self::typed_string_literal_value(&typed_stop),
                ) {
                    if let Some((start_cp, stop_cp)) =
                        Self::foldable_string_range_endpoints(&start_str, &stop_str)
                    {
                        return Ok(self.fold_string_range_literal(span, start_cp, stop_cp));
                    }
                }
                self.lower_string_range_runtime(span, start.clone(), stop.clone())
            }
            _ => Err(TypeError {
                message: "range literal endpoints must both be Int or both be String".into(),
                span: span.clone(),
                hint: Some("Use `[start..stop]` with matching Int endpoints or matching single-char String endpoints.".into()),
            }),
        }
    }

    fn typed_int_literal_value(node: &TypedNode) -> Option<sindr::primitives::SurtrInt> {
        match &node.node {
            TypedInner::Lit(Lit::Int(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn typed_string_literal_value(node: &TypedNode) -> Option<String> {
        match &node.node {
            TypedInner::Lit(Lit::Str(value)) => Some(value.clone()),
            _ => None,
        }
    }

    fn fold_int_range_literal(
        &mut self,
        span: &Span,
        start: sindr::primitives::SurtrInt,
        stop: sindr::primitives::SurtrInt,
    ) -> TypedNode {
        let mut current = start;
        let mut elems = Vec::new();
        while current <= stop {
            elems.push(TypedNode {
                ty: Ty::Int,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Int(current.clone())),
            });
            current += int(1);
        }

        TypedNode {
            ty: Ty::List(Box::new(Ty::Int)),
            span: span.clone(),
            node: TypedInner::ListLiteral(elems),
        }
    }

    fn fold_string_range_literal(&mut self, span: &Span, start_cp: u8, stop_cp: u8) -> TypedNode {
        let mut elems = Vec::new();
        let mut current = start_cp;
        while current <= stop_cp {
            elems.push(TypedNode {
                ty: Ty::Str,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Str((current as char).to_string())),
            });
            current += 1;
        }

        let list_node = TypedNode {
            ty: Ty::List(Box::new(Ty::Str)),
            span: span.clone(),
            node: TypedInner::ListLiteral(elems),
        };
        TypedNode {
            ty: Ty::Result(Box::new(Ty::List(Box::new(Ty::Str))), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::ConstructorCall(0, vec![list_node]),
        }
    }

    fn foldable_string_range_endpoints(start: &str, stop: &str) -> Option<(u8, u8)> {
        Some((
            Self::single_ascii_range_endpoint_value(start)?,
            Self::single_ascii_range_endpoint_value(stop)?,
        ))
    }

    fn single_ascii_range_endpoint_value(value: &str) -> Option<u8> {
        let mut chars = value.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        if !ch.is_ascii() {
            return None;
        }
        Some(ch as u8)
    }

    fn runtime_helper_id(
        &self,
        qualified_name: &str,
        span: &Span,
    ) -> Result<ResolvedId, TypeError> {
        if let Some(id) = self.function_ids_by_name.get(qualified_name) {
            return Ok(id.clone());
        }
        if let Some(uid) = self.impl_method_uids.get(qualified_name) {
            if let Some(id) = self
                .function_ids_by_name
                .values()
                .find(|id| id.unique_id == *uid)
            {
                return Ok(id.clone());
            }
            let local_name = qualified_name
                .rsplit("::")
                .next()
                .unwrap_or(qualified_name)
                .to_string();
            return Ok(ResolvedId {
                name: local_name,
                qualified_name: Some(qualified_name.to_string()),
                unique_id: *uid,
                compiler_generated: true,
                span: span.clone(),
            });
        }
        Err(TypeError {
            message: format!("internal error: missing std helper `{qualified_name}`"),
            span: span.clone(),
            hint: None,
        })
    }

    fn next_synthetic_range_uid() -> u32 {
        SYNTHETIC_RANGE_UID.fetch_add(1, Ordering::Relaxed)
    }

    fn lower_int_range_runtime(
        &mut self,
        span: &Span,
        start: Resolved,
        stop: Resolved,
    ) -> Result<TypedNode, TypeError> {
        let range_id = self.runtime_helper_id("Generator::range", span)?;
        let to_list_id = self.runtime_helper_id("Generator::to_list", span)?;
        let lowered = Resolved::App(
            span.clone(),
            Box::new(Resolved::Var(span.clone(), to_list_id)),
            vec![ResolvedRecordLitArg::Positional(Resolved::App(
                span.clone(),
                Box::new(Resolved::Var(span.clone(), range_id)),
                vec![
                    ResolvedRecordLitArg::Positional(start),
                    ResolvedRecordLitArg::Positional(stop),
                ],
            ))],
        );
        self.check_node(&lowered)
    }

    fn lower_string_range_runtime(
        &mut self,
        span: &Span,
        start: Resolved,
        stop: Resolved,
    ) -> Result<TypedNode, TypeError> {
        let range_char_id = self.runtime_helper_id("Generator::range_char", span)?;
        let to_list_id = self.runtime_helper_id("Generator::to_list", span)?;
        let gen_uid = Self::next_synthetic_range_uid();
        let gen_id = ResolvedId {
            name: "__range_gen".into(),
            qualified_name: None,
            unique_id: gen_uid,
            compiler_generated: true,
            span: span.clone(),
        };
        let lowered = Resolved::Block(
            span.clone(),
            vec![
                Resolved::SafeBind(
                    span.clone(),
                    ResolvedPattern::Var(gen_id.clone()),
                    Box::new(Resolved::App(
                        span.clone(),
                        Box::new(Resolved::Var(span.clone(), range_char_id)),
                        vec![
                            ResolvedRecordLitArg::Positional(start),
                            ResolvedRecordLitArg::Positional(stop),
                        ],
                    )),
                ),
                Resolved::ConstructorCall(
                    span.clone(),
                    ResolvedId {
                        name: "Ok".into(),
                        qualified_name: None,
                        unique_id: 0,
                        compiler_generated: true,
                        span: span.clone(),
                    },
                    vec![ResolvedRecordLitArg::Positional(Resolved::App(
                        span.clone(),
                        Box::new(Resolved::Var(span.clone(), to_list_id)),
                        vec![ResolvedRecordLitArg::Positional(Resolved::Var(
                            span.clone(),
                            gen_id,
                        ))],
                    ))],
                ),
            ],
        );
        self.check_node(&lowered)
    }

    pub(super) fn check_tuple_literal(
        &mut self,
        span: &Span,
        elems: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        if elems.len() < 2 {
            return Err(TypeError {
                message: "Tuple literals require at least 2 values".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_elems = elems
            .iter()
            .map(|elem| self.check_node(elem))
            .collect::<Result<Vec<_>, _>>()?;
        for typed in &typed_elems {
            self.ensure_no_runtime_facet_value(typed, "Tuple literal")?;
        }
        let item_tys = typed_elems.iter().map(|elem| elem.ty.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Tuple(item_tys),
            span: span.clone(),
            node: TypedInner::TupleLiteral(typed_elems),
        })
    }

    pub(super) fn check_interpolated_str(
        &mut self,
        span: &Span,
        parts: &[ResolvedInterpolatedPart],
    ) -> Result<TypedNode, TypeError> {
        let mut typed_parts = Vec::new();
        for part in parts {
            match part {
                ResolvedInterpolatedPart::Text(s) => {
                    typed_parts.push(TypedInterpolatedPart::Text(s.clone()));
                }
                ResolvedInterpolatedPart::Expr(expr) => {
                    let typed_expr = self.check_node(expr)?;
                    self.ensure_no_runtime_facet_value(&typed_expr, "String interpolation")?;
                    if matches!(typed_expr.ty, Ty::Result(_, _)) {
                        return Err(TypeError {
                            message: "Interpolation does not allow Result type".into(),
                            span: typed_expr.span.clone(),
                            hint: Some(
                                "Unwrap/match the Result first, or convert it to a printable value"
                                    .into(),
                            ),
                        });
                    }
                    typed_parts.push(TypedInterpolatedPart::Expr(Box::new(typed_expr)));
                }
            }
        }

        Ok(TypedNode {
            ty: Ty::Str,
            span: span.clone(),
            node: TypedInner::InterpolatedStr(typed_parts),
        })
    }

    pub(super) fn check_if(
        &mut self,
        span: &Span,
        cond: &Resolved,
        then: &Resolved,
        else_opt: &Option<Box<Resolved>>,
    ) -> Result<TypedNode, TypeError> {
        let typed_cond = self.check_node(cond)?;
        if !self.types_compatible(&Ty::Bool, &typed_cond.ty) {
            return Err(TypeError {
                message: format!(
                    "if condition must be Boolean, got {}",
                    self.ty_name(&typed_cond.ty)
                ),
                span: typed_cond.span.clone(),
                hint: None,
            });
        }

        let raw_then = self.check_node(then)?;
        let typed_then = self.maybe_call_zero_arg_function(raw_then, span.clone());

        match else_opt {
            Some(else_branch) => {
                let raw_else = self.check_node(else_branch)?;
                let typed_else = self.maybe_call_zero_arg_function(raw_else, span.clone());
                if !self.types_compatible(&typed_then.ty, &typed_else.ty) {
                    return Err(TypeError {
                        message: format!(
                            "if branches have different types: {} and {}",
                            self.ty_name(&typed_then.ty),
                            self.ty_name(&typed_else.ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let ty = typed_then.ty.clone();
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::If(
                        Box::new(typed_cond),
                        Box::new(typed_then),
                        Some(Box::new(typed_else)),
                    ),
                })
            }
            None => Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::If(Box::new(typed_cond), Box::new(typed_then), None),
            }),
        }
    }

    pub(super) fn check_dbg(
        &mut self,
        span: &Span,
        args: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        let typed_args = args
            .iter()
            .map(|arg| {
                let expr = self.check_node(arg)?;
                Ok(TypedDbgArg {
                    span: expr.span.clone(),
                    ty_name: self.ty_name(&expr.ty),
                    expr,
                })
            })
            .collect::<Result<Vec<_>, TypeError>>()?;

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Dbg(typed_args),
        })
    }

    pub(super) fn check_assert(
        &mut self,
        span: &Span,
        cond: &Resolved,
        err: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_cond = self.check_node(cond)?;
        if !self.types_compatible(&Ty::Bool, &typed_cond.ty) {
            return Err(TypeError {
                message: format!(
                    "assert condition must be Boolean, got {}",
                    self.ty_name(&typed_cond.ty)
                ),
                span: typed_cond.span.clone(),
                hint: None,
            });
        }

        let raw_err = self.check_node(err)?;
        let typed_err = self.maybe_call_zero_arg_function(raw_err, span.clone());
        self.ensure_guard_error_value(&typed_err, "assert")?;

        Ok(TypedNode {
            ty: Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::Assert(Box::new(typed_cond), Box::new(typed_err)),
        })
    }

    pub(super) fn check_ensure(
        &mut self,
        span: &Span,
        value: &Resolved,
        pred: &Resolved,
        err: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_value = self.check_node(value)?;
        if matches!(pred, Resolved::App(_, _, _)) {
            return Err(TypeError {
                message: "ensure requires a closure or capture predicate".into(),
                span: self.resolved_span(pred).clone(),
                hint: Some("Use `&predicate` or `{|value| predicate(value) }`; call expressions such as `predicate()` are not accepted here.".into()),
            });
        }
        let typed_pred = self.check_compose_callable(pred, "ensure")?;
        let (pred_in, pred_out) =
            self.unary_function_parts(&typed_pred.ty, "ensure", &typed_pred.span)?;
        if !self.types_compatible(&pred_in, &typed_value.ty) {
            return Err(TypeError {
                message: format!(
                    "ensure predicate type mismatch: expected {}, got {}",
                    self.ty_name(&typed_value.ty),
                    self.ty_name(&pred_in)
                ),
                span: typed_pred.span.clone(),
                hint: None,
            });
        }
        if !self.types_compatible(&Ty::Bool, &pred_out) {
            return Err(TypeError {
                message: format!(
                    "ensure predicate must return Boolean, got {}",
                    self.ty_name(&pred_out)
                ),
                span: typed_pred.span.clone(),
                hint: None,
            });
        }

        let raw_err = self.check_node(err)?;
        let typed_err = self.maybe_call_zero_arg_function(raw_err, span.clone());
        self.ensure_guard_error_value(&typed_err, "ensure")?;

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&typed_value.ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::Ensure(
                Box::new(typed_value),
                Box::new(typed_pred),
                Box::new(typed_err),
            ),
        })
    }

    pub(super) fn check_map_err(
        &mut self,
        span: &Span,
        value: &Resolved,
        err: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_value = self.check_result_value(value, "map_err")?;
        let raw_err = self.check_node(err)?;
        let typed_err = self.maybe_call_zero_arg_function(raw_err, span.clone());
        self.ensure_result_error_arg(&typed_err, "map_err")?;

        Ok(TypedNode {
            ty: typed_value.ty.clone(),
            span: span.clone(),
            node: TypedInner::MapErr(Box::new(typed_value), Box::new(typed_err)),
        })
    }

    pub(super) fn check_cause(
        &mut self,
        span: &Span,
        value: &Resolved,
        err: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_value = self.check_result_value(value, "cause")?;
        let raw_err = self.check_node(err)?;
        let typed_err = self.maybe_call_zero_arg_function(raw_err, span.clone());
        self.ensure_result_error_arg(&typed_err, "cause")?;

        Ok(TypedNode {
            ty: typed_value.ty.clone(),
            span: span.clone(),
            node: TypedInner::Cause(Box::new(typed_value), Box::new(typed_err)),
        })
    }

    pub(super) fn check_recover_kind(
        &mut self,
        span: &Span,
        value: &Resolved,
        marker: &Resolved,
        handler: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_value = self.check_result_value(value, "recover_kind")?;
        let value_ty = self.resolve_ty(&typed_value.ty);
        let Ty::Result(ok_ty, _) = &value_ty else {
            unreachable!()
        };
        let ok_ty = ok_ty.as_ref().clone();
        let typed_marker = self.check_node(marker)?;
        let typed_marker = self.maybe_call_zero_arg_function(typed_marker, span.clone());
        self.ensure_recover_kind_marker(&typed_marker)?;
        let expected_handler = Ty::Func(
            vec![Ty::Error],
            Box::new(Ty::Result(Box::new(ok_ty.clone()), Box::new(Ty::Error))),
        );
        let typed_handler = self.check_node_with_expected(handler, Some(&expected_handler))?;
        let (handler_in, handler_out) =
            self.unary_function_parts(&typed_handler.ty, "recover_kind", &typed_handler.span)?;
        if !self.types_compatible(&Ty::Error, &handler_in) {
            return Err(TypeError {
                message: format!(
                    "recover_kind handler must accept Error, got {}",
                    self.ty_name(&handler_in)
                ),
                span: typed_handler.span.clone(),
                hint: None,
            });
        }
        if !self.types_compatible(&expected_handler, &typed_handler.ty) {
            return Err(TypeError {
                message: format!(
                    "recover_kind handler must return Result<{}>, got {}",
                    self.ty_name(&ok_ty),
                    self.ty_name(&handler_out)
                ),
                span: typed_handler.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Result(Box::new(ok_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::RecoverKind(
                Box::new(typed_value),
                Box::new(typed_marker),
                Box::new(typed_handler),
            ),
        })
    }

    fn check_result_value(
        &mut self,
        value: &Resolved,
        form_name: &str,
    ) -> Result<TypedNode, TypeError> {
        let typed_value = self.check_node(value)?;
        let value_ty = self.resolve_ty(&typed_value.ty);
        let expected_result_ty = Ty::Result(Box::new(self.env.fresh_tyvar()), Box::new(Ty::Error));
        if !self.types_compatible(&expected_result_ty, &value_ty) {
            return Err(TypeError {
                message: format!(
                    "{} value must be Result<...>, got {}",
                    form_name,
                    self.ty_name(&value_ty)
                ),
                span: typed_value.span.clone(),
                hint: None,
            });
        }
        Ok(typed_value)
    }

    fn resolve_facet_segment_for_source_ty(
        &mut self,
        source_ty: &Ty,
        segment: &PendingFacetSegment,
        span: &Span,
        _for_capability: bool,
    ) -> Result<(TypedFacetSegment, Ty, bool), TypeError> {
        if let PendingFacetSegment::Bracket { expr, display } = segment {
            let typed_expr = match expr {
                PendingFacetExpr::Resolved(expr) => self.check_node(expr)?,
                PendingFacetExpr::Typed(expr) => self.resolve_typed_node((**expr).clone()),
            };
            return match self.resolve_ty(source_ty) {
                Ty::List(inner) => {
                    let expr_ty = self.resolve_ty(&typed_expr.ty);
                    if let Ty::Result(ok, _) = &expr_ty {
                        if self.types_compatible(ok.as_ref(), &Ty::Int) {
                            return Err(TypeError {
                                message: "Facet bracket expression must be plain Int; unwrap Result<Int> before using it".into(),
                                span: typed_expr.span.clone(),
                                hint: None,
                            });
                        }
                    }
                    if !self.types_compatible(&Ty::Int, &expr_ty) {
                        return Err(TypeError {
                            message: "List Facet index expression must be Int".into(),
                            span: typed_expr.span.clone(),
                            hint: None,
                        });
                    }

                    let literal_index = match &typed_expr.node {
                        TypedInner::Lit(Lit::Int(index)) => Some(index.clone()),
                        _ => None,
                    };
                    let focus_ty = inner.as_ref().clone();
                    Ok((
                        TypedFacetSegment::ListIndex {
                            index: Box::new(typed_expr),
                            display: display.clone(),
                            literal_index,
                            focus_readonly_root: self.ty_is_readonly_root(&focus_ty),
                            focus_type_name: Self::readonly_type_name(&self.resolve_ty(&focus_ty))
                                .map(str::to_string),
                        },
                        focus_ty,
                        true,
                    ))
                }
                Ty::Enum(name, args)
                    if Self::surface_name(&name) == "HashMap" && args.len() == 1 =>
                {
                    let expr_ty = self.resolve_ty(&typed_expr.ty);
                    if let Ty::Result(ok, _) = &expr_ty {
                        if self.types_compatible(ok.as_ref(), &Ty::Str) {
                            return Err(TypeError {
                                message: "Facet bracket expression must be plain String; unwrap Result<String> before using it".into(),
                                span: typed_expr.span.clone(),
                                hint: None,
                            });
                        }
                    }
                    if !self.types_compatible(&Ty::Str, &expr_ty) {
                        return Err(TypeError {
                            message: "HashMap Facet key expression must be String".into(),
                            span: typed_expr.span.clone(),
                            hint: None,
                        });
                    }

                    let literal_key = match &typed_expr.node {
                        TypedInner::Lit(Lit::Str(key)) => Some(key.clone()),
                        _ => None,
                    };
                    let value_ty = args[0].clone();
                    Ok((
                        TypedFacetSegment::MapKey {
                            key: Box::new(typed_expr),
                            display: display.clone(),
                            literal_key,
                            focus_readonly_root: self.ty_is_readonly_root(&value_ty),
                            focus_type_name: Self::readonly_type_name(&self.resolve_ty(&value_ty))
                                .map(str::to_string),
                        },
                        value_ty,
                        true,
                    ))
                }
                other => Err(TypeError {
                    message: format!(
                        "Facet segment {} is not supported for {} yet",
                        Self::pending_segment_display(segment),
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            };
        }
        if let PendingFacetSegment::RangeBracket {
            start,
            end,
            display,
        } = segment
        {
            let typed_start = match start {
                PendingFacetExpr::Resolved(expr) => self.check_node(expr)?,
                PendingFacetExpr::Typed(expr) => self.resolve_typed_node((**expr).clone()),
            };
            let typed_end = match end {
                PendingFacetExpr::Resolved(expr) => self.check_node(expr)?,
                PendingFacetExpr::Typed(expr) => self.resolve_typed_node((**expr).clone()),
            };
            return match self.resolve_ty(source_ty) {
                Ty::List(inner) => {
                    for typed_expr in [&typed_start, &typed_end] {
                        let expr_ty = self.resolve_ty(&typed_expr.ty);
                        if let Ty::Result(ok, _) = &expr_ty {
                            if self.types_compatible(ok.as_ref(), &Ty::Int) {
                                return Err(TypeError {
                                    message: "Facet bracket expression must be plain Int; unwrap Result<Int> before using it".into(),
                                    span: typed_expr.span.clone(),
                                    hint: None,
                                });
                            }
                        }
                        if !self.types_compatible(&Ty::Int, &expr_ty) {
                            return Err(TypeError {
                                message: "List Facet index expression must be Int".into(),
                                span: typed_expr.span.clone(),
                                hint: None,
                            });
                        }
                    }

                    let literal_start = match &typed_start.node {
                        TypedInner::Lit(Lit::Int(index)) => Some(index.clone()),
                        _ => None,
                    };
                    let literal_end = match &typed_end.node {
                        TypedInner::Lit(Lit::Int(index)) => Some(index.clone()),
                        _ => None,
                    };
                    let focus_ty = Ty::List(Box::new(inner.as_ref().clone()));
                    Ok((
                        TypedFacetSegment::ListRange {
                            start: Box::new(typed_start),
                            end: Box::new(typed_end),
                            display: display.clone(),
                            literal_start,
                            literal_end,
                            focus_readonly_root: self.ty_is_readonly_root(&focus_ty),
                            focus_type_name: Self::readonly_type_name(&self.resolve_ty(&focus_ty))
                                .map(str::to_string),
                        },
                        focus_ty,
                        true,
                    ))
                }
                other => Err(TypeError {
                    message: format!(
                        "Facet segment {} is not supported for {} yet",
                        Self::pending_segment_display(segment),
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            };
        }
        let PendingFacetSegment::Field {
            name: field,
            optional,
        } = segment
        else {
            return Err(TypeError {
                message: format!(
                    "Facet segment {} is not supported for {} yet",
                    Self::pending_segment_display(segment),
                    self.ty_name(source_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        };
        match self.resolve_ty(source_ty) {
            Ty::Tuple(items) => {
                if *optional {
                    return Err(TypeError {
                        message: "optional Facet segment requires an enum variant".into(),
                        span: span.clone(),
                        hint: Some(
                            "Use `?` only on enum case segments such as Option.Some?.".into(),
                        ),
                    });
                }
                let index = field
                    .strip_prefix('_')
                    .ok_or_else(|| TypeError {
                        message: "Tuple elements are accessed with ._0, ._1, ...".into(),
                        span: span.clone(),
                        hint: None,
                    })?
                    .parse::<usize>()
                    .map_err(|_| TypeError {
                        message: "Tuple elements are accessed with ._0, ._1, ...".into(),
                        span: span.clone(),
                        hint: None,
                    })?;
                let field_ty = items.get(index).cloned().ok_or_else(|| TypeError {
                    message: format!(
                        "Tuple index ._{} is out of bounds for {}",
                        index,
                        self.ty_name(source_ty)
                    ),
                    span: span.clone(),
                    hint: Some(Self::tuple_index_hint(items.len())),
                })?;
                Ok((
                    TypedFacetSegment::Tuple {
                        field_index: index as u32,
                        tuple_len: items.len() as u32,
                        focus_readonly_root: self.ty_is_readonly_root(&field_ty),
                        focus_type_name: Self::readonly_type_name(&self.resolve_ty(&field_ty))
                            .map(str::to_string),
                    },
                    field_ty,
                    false,
                ))
            }
            Ty::Struct(name, fields) | Ty::Record(name, fields) => {
                if *optional {
                    return Err(TypeError {
                        message: "optional Facet segment requires an enum variant".into(),
                        span: span.clone(),
                        hint: Some(
                            "Use `?` only on enum case segments such as Option.Some?.".into(),
                        ),
                    });
                }
                if self.env.is_private_field(&name, field) {
                    let display_name = Self::surface_name(&name);
                    let outside_impl =
                        self.current_impl_struct_target.as_deref() != Some(display_name);
                    if outside_impl {
                        return Err(TypeError {
                            message: format!("Field '{}.{}' is private", display_name, field),
                            span: span.clone(),
                            hint: Some(format!(
                                "Expose the value through a public method on {} instead.",
                                display_name
                            )),
                        });
                    }
                }
                let (field_index, field_ty) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, (field_name, _))| field_name == field)
                    .map(|(i, (_, ty))| (i as u32, ty.clone()))
                    .ok_or_else(|| TypeError {
                        message: format!("No field '{}' on {}", field, self.ty_name(source_ty)),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok((
                    TypedFacetSegment::Field {
                        field_name: field.to_string(),
                        field_index,
                        container_field_count: fields.len() as u32,
                        container_type_name: Self::surface_name(&name).to_string(),
                        readonly: self.env.is_readonly_field(&name, field),
                        focus_readonly_root: self.ty_is_readonly_root(&field_ty),
                        focus_type_name: Self::readonly_type_name(&self.resolve_ty(&field_ty))
                            .map(str::to_string),
                    },
                    field_ty,
                    false,
                ))
            }
            Ty::Enum(enum_name, _) => {
                if self.lookup_enum_variants_of(&enum_name).is_none() {
                    return Err(TypeError {
                        message: format!(
                            "No variants found for enum {}",
                            Self::surface_name(&enum_name)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let Some(variant) = self.lookup_enum_variant_by_short_name(&enum_name, field)
                else {
                    return Err(TypeError {
                        message: format!(
                            "No variant selector '{}' on {} (use PascalCase constructor names)",
                            field,
                            Self::surface_name(&enum_name)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                };
                let variant = self.instantiate_enum_variant(&variant);
                if !self.types_compatible(&variant.enum_ty, source_ty) {
                    return Err(TypeError {
                        message: format!(
                            "Variant selector {}.{} does not match {}",
                            enum_name,
                            field,
                            self.ty_name(source_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let payload_arity = variant.payload.len() as u32;
                let focus_ty = match variant.payload.len() {
                    0 => Ty::Unit,
                    1 => variant.payload[0].clone(),
                    _ => Ty::Tuple(variant.payload.clone()),
                };
                Ok((
                    TypedFacetSegment::Variant {
                        enum_name,
                        variant_name: variant.short_name,
                        variant_tag: variant.tag,
                        discriminant: variant.discriminant,
                        payload_arity,
                        optional: *optional,
                        focus_readonly_root: self.ty_is_readonly_root(&focus_ty),
                        focus_type_name: Self::readonly_type_name(&self.resolve_ty(&focus_ty))
                            .map(str::to_string),
                    },
                    focus_ty,
                    true,
                ))
            }
            other => {
                let message = if field.starts_with('_')
                    && field
                        .strip_prefix('_')
                        .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
                {
                    format!(
                        "Tuple-style access .{} is only available on tuples, got {}",
                        field,
                        self.ty_name(&other)
                    )
                } else {
                    format!("Cannot access field on {}", self.ty_name(&other))
                };
                Err(TypeError {
                    message,
                    span: span.clone(),
                    hint: None,
                })
            }
        }
    }

    fn try_check_tuple_type_root_facet_path(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
        expected: Option<&Ty>,
    ) -> Result<Option<TypedNode>, TypeError> {
        let Resolved::Var(_, id) = expr else {
            return Ok(None);
        };
        if id.name != "Tuple" {
            return Ok(None);
        }
        let Some(index) = Self::parse_tuple_segment_index(field) else {
            return Ok(None);
        };

        let Some(expected_ty) = expected else {
            return Ok(Some(self.pending_facet_node(
                span,
                PendingFacetPath {
                    root_path_name: Some("Tuple".into()),
                    source_ty_hint: None,
                    segments: vec![Self::pending_field_segment(field)],
                },
            )));
        };
        let expected_ty = self.resolve_ty(expected_ty);
        let (expected_source, expected_focus) = match expected_ty {
            Ty::Facet(source, focus) => (source.as_ref().clone(), focus.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "Tuple.{} requires expected Facet<..., ...> context, got {}",
                        field,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some(
                        "Use Tuple._N as a Facet path argument in Facet::view/set/over.".into(),
                    ),
                });
            }
        };

        let tuple_items = match self.resolve_ty(&expected_source) {
            Ty::Tuple(items) => items,
            other => {
                return Err(TypeError {
                    message: format!(
                        "Tuple.{} requires tuple source context, got {}",
                        field,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some("Expected source type like (A, B, ...) for Tuple._N.".into()),
                });
            }
        };

        let focus_ty = tuple_items.get(index).cloned().ok_or_else(|| TypeError {
            message: format!(
                "Tuple index ._{} is out of bounds for ({})",
                index,
                tuple_items
                    .iter()
                    .map(|item| self.ty_name(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            span: span.clone(),
            hint: Some(Self::tuple_index_hint(tuple_items.len())),
        })?;

        if !self.types_compatible(&focus_ty, &expected_focus) {
            return Err(TypeError {
                message: format!(
                    "Tuple.{} focus type mismatch: expected {}, got {}",
                    field,
                    self.ty_name(&expected_focus),
                    self.ty_name(&focus_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = Ty::Tuple(tuple_items);
        let path = TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            path_kind: TypedFacetPathKind::Structural,
            may_fail: false,
            source_readonly_root: self.ty_is_readonly_root(&source_ty),
            segments: vec![TypedFacetSegment::Tuple {
                field_index: index as u32,
                tuple_len: match &source_ty {
                    Ty::Tuple(items) => items.len() as u32,
                    _ => unreachable!("source_ty is always Tuple here"),
                },
                focus_readonly_root: self.ty_is_readonly_root(&focus_ty),
                focus_type_name: Self::readonly_type_name(&self.resolve_ty(&focus_ty))
                    .map(str::to_string),
            }],
        };

        Ok(Some(TypedNode {
            ty: Ty::Facet(Box::new(source_ty), Box::new(focus_ty)),
            span: span.clone(),
            node: TypedInner::FacetPath(path),
        }))
    }

    fn try_check_container_type_root_facet_path(
        &mut self,
        span: &Span,
        expr: &Resolved,
        segment: &PendingFacetSegment,
        expected: Option<&Ty>,
    ) -> Result<Option<TypedNode>, TypeError> {
        let Resolved::Var(_, id) = expr else {
            return Ok(None);
        };
        let root_path_name = match (id.name.as_str(), segment) {
            (
                "List",
                PendingFacetSegment::Bracket { .. } | PendingFacetSegment::RangeBracket { .. },
            ) => "List",
            ("HashMap", PendingFacetSegment::Bracket { .. }) => "HashMap",
            _ => return Ok(None),
        };

        let Some(expected_ty) = expected else {
            return Ok(Some(self.pending_facet_node(
                span,
                PendingFacetPath {
                    root_path_name: Some(root_path_name.into()),
                    source_ty_hint: None,
                    segments: vec![segment.clone()],
                },
            )));
        };
        let expected_ty = self.resolve_ty(expected_ty);
        let (expected_source, expected_focus) = match expected_ty {
            Ty::Facet(source, focus) => (source.as_ref().clone(), focus.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "{root_path_name} root Facet path requires expected Facet<..., ...> context, got {}",
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "Use {root_path_name} root paths as Facet path arguments in Facet::view/set/over."
                    )),
                });
            }
        };

        self.validate_pending_root_source(root_path_name, &expected_source, span)?;
        let (typed_segment, focus_ty, may_fail) =
            self.resolve_facet_segment_for_source_ty(&expected_source, segment, span, true)?;
        let focus_ty = self.resolve_ty(&focus_ty);
        if !self.types_compatible(&focus_ty, &expected_focus) {
            return Err(TypeError {
                message: format!(
                    "{root_path_name} root Facet path focus type mismatch: expected {}, got {}",
                    self.ty_name(&expected_focus),
                    self.ty_name(&focus_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = self.resolve_ty(&expected_source);
        let path = TypedFacetPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            path_kind: Self::facet_path_kind_for_segments(std::slice::from_ref(&typed_segment)),
            may_fail,
            source_readonly_root: self.ty_is_readonly_root(&source_ty),
            segments: vec![typed_segment],
        };

        Ok(Some(TypedNode {
            ty: Ty::Facet(Box::new(source_ty), Box::new(focus_ty)),
            span: span.clone(),
            node: TypedInner::FacetPath(path),
        }))
    }

    fn check_field_access_with_expected(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let segment = ResolvedFacetPathSegment::Field {
            name: field.to_string(),
            optional: false,
        };
        self.check_facet_segment_access_with_expected(span, expr, &segment, expected)
    }

    fn check_facet_segment_access_with_expected(
        &mut self,
        span: &Span,
        expr: &Resolved,
        syntax_segment: &ResolvedFacetPathSegment,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let pending_segment = Self::pending_segment_from_syntax(syntax_segment);
        let field = match syntax_segment {
            ResolvedFacetPathSegment::Field {
                name,
                optional: false,
            } => Some(name.as_str()),
            _ => None,
        };

        if let Some(container_root_path) =
            self.try_check_container_type_root_facet_path(span, expr, &pending_segment, expected)?
        {
            return Ok(container_root_path);
        }
        if let Some(field) = field {
            if let Some(tuple_root_path) =
                self.try_check_tuple_type_root_facet_path(span, expr, field, expected)?
            {
                return Ok(tuple_root_path);
            }
        }
        let typed_expr = self.check_node(expr)?;

        if matches!(typed_expr.ty, Ty::Facet(_, _)) {
            let path = self.resolve_facet_path_from_node(typed_expr, span, None)?;
            let (segment, focus_ty, may_fail) = self.resolve_facet_segment_for_source_ty(
                &path.focus_ty,
                &pending_segment,
                span,
                true,
            )?;
            let source_ty = self.resolve_ty(&path.source_ty);
            let focus_ty = self.resolve_ty(&focus_ty);
            let mut segments = path.segments;
            segments.push(segment);
            let combined = TypedFacetPath {
                source_ty: source_ty.clone(),
                focus_ty: focus_ty.clone(),
                path_kind: Self::facet_path_kind_for_segments(&segments),
                may_fail: path.may_fail || may_fail,
                source_readonly_root: path.source_readonly_root,
                segments,
            };
            return Ok(TypedNode {
                ty: Ty::Facet(Box::new(source_ty), Box::new(focus_ty)),
                span: span.clone(),
                node: TypedInner::FacetPath(combined),
            });
        }

        if let TypedInner::Var(id) = &typed_expr.node {
            if self.env.is_type_constructor_id(id.unique_id) {
                let (source_ty, expected_focus_ty) = match expected.map(|ty| self.resolve_ty(ty)) {
                    Some(Ty::Facet(source, focus)) => {
                        (source.as_ref().clone(), Some(focus.as_ref().clone()))
                    }
                    _ => (self.resolve_ty(&typed_expr.ty), None),
                };
                let (segment, focus_ty, may_fail) = self.resolve_facet_segment_for_source_ty(
                    &source_ty,
                    &pending_segment,
                    span,
                    true,
                )?;
                let focus_ty = self.resolve_ty(&focus_ty);
                if let Some(expected_focus_ty) = expected_focus_ty {
                    if !self.types_compatible(&focus_ty, &expected_focus_ty) {
                        return Err(TypeError {
                            message: format!(
                                "Facet path focus type mismatch: expected {}, got {}",
                                self.ty_name(&expected_focus_ty),
                                self.ty_name(&focus_ty)
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                }
                let path = TypedFacetPath {
                    source_ty: source_ty.clone(),
                    focus_ty: focus_ty.clone(),
                    path_kind: Self::facet_path_kind_for_segments(std::slice::from_ref(&segment)),
                    may_fail,
                    source_readonly_root: self.ty_is_readonly_root(&source_ty),
                    segments: vec![segment],
                };
                return Ok(TypedNode {
                    ty: Ty::Facet(Box::new(source_ty), Box::new(focus_ty)),
                    span: span.clone(),
                    node: TypedInner::FacetPath(path),
                });
            }
        }

        let (source_is_result, source_focus_ty) = match self.resolve_ty(&typed_expr.ty) {
            Ty::Result(ok, _) => (true, ok.as_ref().clone()),
            other => (false, other),
        };
        let (segment, focus_ty, may_fail) = self.resolve_facet_segment_for_source_ty(
            &source_focus_ty,
            &pending_segment,
            span,
            false,
        )?;
        let focus_ty = self.resolve_ty(&focus_ty);
        let path = TypedFacetPath {
            source_ty: source_focus_ty,
            focus_ty: focus_ty.clone(),
            path_kind: Self::facet_path_kind_for_segments(std::slice::from_ref(&segment)),
            may_fail,
            source_readonly_root: false,
            segments: vec![segment],
        };
        let out_ty = if source_is_result || path.may_fail {
            Ty::Result(Box::new(focus_ty), Box::new(Ty::Error))
        } else {
            focus_ty
        };

        Ok(TypedNode {
            ty: out_ty,
            span: span.clone(),
            node: TypedInner::FacetView {
                source: Box::new(typed_expr),
                path,
                source_is_result,
            },
        })
    }

    pub(super) fn check_field_access(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
    ) -> Result<TypedNode, TypeError> {
        self.check_field_access_with_expected(span, expr, field, None)
    }

    fn ty_is_readonly_root(&self, ty: &Ty) -> bool {
        match self.resolve_ty(ty) {
            Ty::Struct(name, _) | Ty::Record(name, _) => self.env.is_readonly_root(&name),
            _ => false,
        }
    }
}

fn trait_impl_signature_display(
    qualified_name: &str,
    param_list: &str,
    ret: &str,
) -> Option<String> {
    let (_, rest) = qualified_name.split_once("::__traitimpl__::")?;
    let mut parts = rest.rsplitn(4, "::").collect::<Vec<_>>();
    if parts.len() != 4 {
        return None;
    }
    parts.reverse();
    let trait_name = parts[0];
    let target = parts[1];
    let method = parts[2];
    Some(format!(
        "impl {} for {} {{ def {}({}) -> {} }}",
        trait_name, target, method, param_list, ret
    ))
}

fn callable_definition_display_name(qualified_name: &str, local_name: &str) -> String {
    let local_tail = local_name.rsplit("::").next().unwrap_or(local_name);
    if let Some((_prefix, rest)) = qualified_name.split_once("::__traitimpl__::") {
        let mut parts = rest.rsplitn(4, "::").collect::<Vec<_>>();
        if parts.len() == 4 {
            parts.reverse();
            if parts[2] == local_tail {
                let display_name = local_name
                    .strip_prefix("Global::")
                    .unwrap_or(local_name)
                    .replace("::Global::", "::");
                return trim_script_qualified_display_name(&display_name);
            }
        }
        return Checker::surface_name(local_name).to_string();
    }

    let display_name = if qualified_name
        .rsplit("::")
        .next()
        .is_some_and(|tail| tail == local_tail)
    {
        Checker::surface_name(qualified_name).to_string()
    } else {
        Checker::surface_name(local_name).to_string()
    };
    trim_script_qualified_display_name(&display_name)
}

fn trim_script_qualified_display_name(qualified_name: &str) -> String {
    let Some(rest) = qualified_name.strip_prefix("__Script::") else {
        return qualified_name.to_string();
    };
    let segments = rest.split("::").collect::<Vec<_>>();
    if segments.len() < 2 {
        return qualified_name.to_string();
    }

    let name = segments[segments.len() - 1];
    let parent = segments[segments.len() - 2];
    if parent
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        format!("{}::{}", parent, name)
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::env::TypeKind;
    use crate::typed::{TypedFacetPath, TypedFacetPathKind, TypedFacetSegment};

    fn test_span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn setup_type(
        checker: &mut Checker,
        name: &str,
        fields: Vec<(&str, Ty)>,
        readonly_fields: &[&str],
        readonly_root: bool,
    ) {
        checker
            .env
            .predeclare_type_def(name.into(), TypeKind::Struct, Vec::new());
        checker.env.resolve_type_def_signature(
            name,
            fields
                .into_iter()
                .map(|(field, ty)| (field.into(), ty))
                .collect(),
            Vec::new(),
            HashSet::new(),
            readonly_fields
                .iter()
                .map(|field| (*field).into())
                .collect(),
            readonly_root,
        );
    }

    fn field_segment(
        container_type_name: &str,
        field_name: &str,
        field_index: u32,
        readonly: bool,
        focus_readonly_root: bool,
        focus_type_name: Option<&str>,
    ) -> TypedFacetSegment {
        TypedFacetSegment::Field {
            field_name: field_name.into(),
            field_index,
            container_field_count: 1,
            container_type_name: container_type_name.into(),
            readonly,
            focus_readonly_root,
            focus_type_name: focus_type_name.map(str::to_string),
        }
    }

    #[test]
    fn mutating_facet_rejects_deep_traversal_through_readonly_field_even_for_owner() {
        let mut checker = Checker::new(TypecheckContext::default());
        setup_type(&mut checker, "Profile", vec![("name", Ty::Str)], &[], false);
        setup_type(
            &mut checker,
            "User",
            vec![(
                "profile",
                Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
            )],
            &["profile"],
            false,
        );
        checker.current_impl_struct_target = Some("User".into());

        let path = TypedFacetPath {
            source_ty: Ty::Struct(
                "User".into(),
                vec![(
                    "profile".into(),
                    Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
                )],
            ),
            focus_ty: Ty::Str,
            path_kind: TypedFacetPathKind::Structural,
            may_fail: false,
            source_readonly_root: false,
            segments: vec![
                field_segment("User", "profile", 0, true, false, Some("Profile")),
                field_segment("Profile", "name", 0, false, false, Some("String")),
            ],
        };

        let err = checker
            .check_mutating_facet_path_permissions("Facet::set", &path, &test_span())
            .expect_err("deep traversal through readonly field should fail");
        assert!(err.message.contains("readonly field User.profile"));
        assert!(err.message.contains("replace the property itself"));
    }

    #[test]
    fn mutating_facet_allows_owner_to_replace_readonly_field_itself() {
        let mut checker = Checker::new(TypecheckContext::default());
        setup_type(&mut checker, "Profile", vec![("name", Ty::Str)], &[], false);
        setup_type(
            &mut checker,
            "User",
            vec![(
                "profile",
                Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
            )],
            &["profile"],
            false,
        );
        checker.current_impl_struct_target = Some("User".into());

        let path = TypedFacetPath {
            source_ty: Ty::Struct(
                "User".into(),
                vec![(
                    "profile".into(),
                    Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
                )],
            ),
            focus_ty: Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
            path_kind: TypedFacetPathKind::Structural,
            may_fail: false,
            source_readonly_root: false,
            segments: vec![field_segment(
                "User",
                "profile",
                0,
                true,
                false,
                Some("Profile"),
            )],
        };

        checker
            .check_mutating_facet_path_permissions("Facet::set", &path, &test_span())
            .expect("owner replacement of readonly field should succeed");
    }

    #[test]
    fn mutating_facet_rejects_readonly_root_and_nested_readonly_type_boundaries() {
        let mut checker = Checker::new(TypecheckContext::default());
        setup_type(&mut checker, "Profile", vec![("name", Ty::Str)], &[], true);
        setup_type(
            &mut checker,
            "User",
            vec![(
                "profile",
                Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
            )],
            &[],
            false,
        );
        checker.current_impl_struct_target = Some("Profile".into());

        let readonly_root_path = TypedFacetPath {
            source_ty: Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
            focus_ty: Ty::Str,
            path_kind: TypedFacetPathKind::Structural,
            may_fail: false,
            source_readonly_root: true,
            segments: vec![field_segment(
                "Profile",
                "name",
                0,
                false,
                false,
                Some("String"),
            )],
        };
        let err = checker
            .check_mutating_facet_path_permissions("Facet::over", &readonly_root_path, &test_span())
            .expect_err("readonly root should fail");
        assert!(err.message.contains("readonly type Profile"));

        let nested_readonly_path = TypedFacetPath {
            source_ty: Ty::Struct(
                "User".into(),
                vec![(
                    "profile".into(),
                    Ty::Struct("Profile".into(), vec![("name".into(), Ty::Str)]),
                )],
            ),
            focus_ty: Ty::Str,
            path_kind: TypedFacetPathKind::Structural,
            may_fail: false,
            source_readonly_root: false,
            segments: vec![
                field_segment("User", "profile", 0, false, true, Some("Profile")),
                field_segment("Profile", "name", 0, false, false, Some("String")),
            ],
        };
        let err = checker
            .check_mutating_facet_path_permissions(
                "Facet::over_result",
                &nested_readonly_path,
                &test_span(),
            )
            .expect_err("nested readonly type boundary should fail");
        assert!(err.message.contains("readonly type Profile"));
    }
}
