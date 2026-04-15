use super::captures::collect_captures;
use super::declarations::trait_instance_key;
use super::scope_init::{
    initialize_scope, is_doc_only_builtin_decl, is_runtime_builtin_decl,
    is_special_form_builtin_decl, resolve_decl_attrs,
};
use super::special_forms::{IfKind, LogicKind};
use super::*;

const TUPLE_TYPE_ROOT_UID: u32 = u32::MAX - 7;

impl Resolver {
    fn partial_pipeline_special_form_arity(name: &str) -> Option<usize> {
        match name {
            "if" => Some(3),
            "if_then" => Some(2),
            "assert" => Some(2),
            "ensure" => Some(3),
            "and" | "or" => Some(2),
            _ => None,
        }
    }

    fn desugar_pipeline_rhs_special_form_partial(&self, rhs: Ast) -> Ast {
        let Ast::App(span, func, args) = rhs else {
            return rhs;
        };

        let Ast::Var(_, ref name) = *func else {
            return Ast::App(span, func, args);
        };
        let Some(expected_arity) = Self::partial_pipeline_special_form_arity(name) else {
            return Ast::App(span, func, args);
        };
        if args.len() + 1 != expected_arity {
            return Ast::App(span, func, args);
        }

        let param_name = format!("__pipe_injected_{}_{}", span.start, span.end);
        let param_span = span.clone();
        let mut injected_args = Vec::with_capacity(args.len() + 1);
        injected_args.push(RecordLitArg::Positional(Ast::Var(
            param_span.clone(),
            param_name.clone(),
        )));
        injected_args.extend(args);

        let call = Ast::App(span.clone(), func, injected_args);
        Ast::Closure(
            span.clone(),
            vec![ClosureParam {
                name: param_name,
                ty: None,
                span: param_span,
            }],
            Box::new(call),
        )
    }

    fn conversion_call_head(func: &Ast) -> Option<&'static str> {
        match func {
            Ast::Var(_, name) if name == "from" => Some("from"),
            Ast::Var(_, name) if name == "try_from" => Some("try_from"),
            Ast::Path(_, path) if path.segments.len() >= 2 => {
                let method = path.segments.last()?;
                let owner = path.segments.get(path.segments.len() - 2)?;
                match (owner.as_str(), method.as_str()) {
                    ("From", "from") => Some("from"),
                    ("TryFrom", "try_from") => Some("try_from"),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn type_witness_from_expr(expr: Ast) -> Result<AstTy, ResolveError> {
        match expr {
            Ast::ConstructorCall(span, name, args) => {
                if args.is_empty() {
                    return Ok(AstTy::Named(span, name));
                }
                let inner = args
                    .into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => Self::type_witness_from_expr(expr),
                        RecordLitArg::Named(_, expr) => Err(ResolveError {
                            message: "type witness arguments do not accept named type parameters"
                                .into(),
                            span: expr.span().clone(),
                        }),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(AstTy::Generic(span, name, inner))
            }
            Ast::Path(span, path) => Ok(AstTy::Named(span, path.segments.join("::"))),
            Ast::Var(span, name) if name == "Unit" => Ok(AstTy::Named(span, name)),
            Ast::Var(span, name) if name.chars().next().is_some_and(|ch| ch.is_uppercase()) => {
                Ok(AstTy::Named(span, name))
            }
            other => Err(ResolveError {
                message: "conversion target must be a bare type name such as String or Result<Int>"
                    .into(),
                span: other.span().clone(),
            }),
        }
    }

    fn type_witness_span(ast_ty: &AstTy) -> &Span {
        match ast_ty {
            AstTy::Named(span, _)
            | AstTy::Generic(span, _, _)
            | AstTy::Tuple(span, _)
            | AstTy::Func(span, _, _)
            | AstTy::ImplTrait(span, _) => span,
        }
    }

    pub(super) fn new() -> Self {
        Self {
            scope: initialize_scope(),
            predeclared_ids: HashMap::new(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            current_module_path: None,
            allow_top_level_shadowing: false,
        }
    }

    pub(super) fn with_scope(scope: Scope) -> Self {
        Self {
            scope,
            predeclared_ids: HashMap::new(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            current_module_path: None,
            allow_top_level_shadowing: false,
        }
    }

    pub(super) fn into_scope(self) -> Scope {
        self.scope
    }

    pub(super) fn qualify_current_declaration_name(&self, name: &str) -> String {
        match self.current_module_path.as_deref() {
            Some(module_path) if !module_path.is_empty() => format!("{}::{}", module_path, name),
            _ => name.to_string(),
        }
    }

    pub(super) fn with_child_scope<T>(
        &mut self,
        f: impl FnOnce(&mut Resolver) -> Result<T, ResolveError>,
    ) -> Result<T, ResolveError> {
        let mut child = Resolver::with_scope(self.scope.clone());
        child.declaration_uids = self.declaration_uids.clone();
        child.declaration_uid_kinds = self.declaration_uid_kinds.clone();
        child.current_module_path = self.current_module_path.clone();
        child.allow_top_level_shadowing = self.allow_top_level_shadowing;
        let out = f(&mut child)?;
        self.scope.advance_next_id_to(child.scope.next_id());
        Ok(out)
    }

    pub(super) fn is_constructor_style_head(name: &str) -> bool {
        name.rsplit("::")
            .next()
            .and_then(|segment| segment.chars().next())
            .is_some_and(|ch| ch.is_uppercase())
    }

    pub(super) fn declaration_fq_name_for_uid(&self, uid: u32) -> Option<String> {
        self.declaration_uids
            .iter()
            .find_map(|(fq_name, entry_uid)| (*entry_uid == uid).then(|| fq_name.clone()))
    }

    pub(super) fn attached_extractor_for_struct(
        &self,
        struct_uid: u32,
        surface_head: &str,
    ) -> Option<(Option<String>, u32, DeclarationKind)> {
        // Struct heads are resolved by declaration name, not by `import`.
        // In MatchBlock, `User(...)` is sugar for `User::deconstruct(...)`.
        let surface_extractor_name = format!("{}::deconstruct", surface_head);
        if let Some(extractor_uid) = self.scope.lookup(&surface_extractor_name) {
            let extractor_kind = self.declaration_uid_kinds.get(&extractor_uid).cloned()?;
            let qualified_name = self.declaration_fq_name_for_uid(extractor_uid);
            return Some((qualified_name, extractor_uid, extractor_kind));
        }

        let struct_fq_name = self.declaration_fq_name_for_uid(struct_uid)?;
        let extractor_fq_name = format!("{}::deconstruct", struct_fq_name);
        let extractor_uid = *self.declaration_uids.get(&extractor_fq_name)?;
        let extractor_kind = self.declaration_uid_kinds.get(&extractor_uid).cloned()?;
        Some((Some(extractor_fq_name), extractor_uid, extractor_kind))
    }

    pub(super) fn resolve_program(
        &mut self,
        stmts: Vec<Ast>,
    ) -> Result<Vec<Resolved>, ResolveError> {
        let stmts = self.lower_impl_defs(stmts)?;
        self.validate_auto_import_conflicts(&stmts)?;
        self.predeclare_functions(&stmts)?;
        let mut resolved = Vec::new();
        for stmt in stmts {
            if matches!(stmt, Ast::Import(_, _, _))
                || matches!(&stmt, Ast::BuiltinDecl(_, name, _, _, _) if is_doc_only_builtin_decl(name))
            {
                // `import` declarations are consumed by resolver-side module/import handling.
                // Until full module resolution lands, they are intentionally no-op here.
                continue;
            }
            resolved.push(self.resolve_node(stmt)?);
        }
        self.validate_trait_impl_pairs(&resolved)?;
        self.predeclared_ids.clear();
        Ok(resolved)
    }
}

impl Resolver {
    pub(super) fn resolve_node(&mut self, node: Ast) -> Result<Resolved, ResolveError> {
        match node {
            Ast::Lit(span, lit) => Ok(Resolved::Lit(span, lit)),

            Ast::Var(span, name) => {
                let uid = self
                    .scope
                    .lookup(&name)
                    .or_else(|| {
                        if name == "Tuple" {
                            Some(TUPLE_TYPE_ROOT_UID)
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| ResolveError {
                        message: format!("Undefined variable: {}", name),
                        span: span.clone(),
                    })?;
                let qualified_name = (uid != TUPLE_TYPE_ROOT_UID)
                    .then(|| self.declaration_fq_name_for_uid(uid))
                    .flatten();
                Ok(Resolved::Var(
                    span.clone(),
                    ResolvedId {
                        name,
                        qualified_name,
                        unique_id: uid,
                        span,
                    },
                ))
            }
            Ast::Path(span, path) => {
                let name = path.segments.join("::");
                let uid = self
                    .scope
                    .lookup(&name)
                    .or_else(|| {
                        if name == "Tuple" {
                            Some(TUPLE_TYPE_ROOT_UID)
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| ResolveError {
                        message: format!("Undefined variable: {}", name),
                        span: span.clone(),
                    })?;
                let qualified_name = if uid == TUPLE_TYPE_ROOT_UID {
                    None
                } else {
                    Some(
                        self.declaration_fq_name_for_uid(uid)
                            .unwrap_or_else(|| name.clone()),
                    )
                };
                Ok(Resolved::Var(
                    span.clone(),
                    ResolvedId {
                        qualified_name,
                        name,
                        unique_id: uid,
                        span,
                    },
                ))
            }

            Ast::App(span, func, args) => {
                // Check for special forms
                if let Ast::Var(_, ref name) = *func {
                    if name == "if" {
                        return self.resolve_if(span, args, IfKind::If3);
                    }
                    if name == "if_then" {
                        return self.resolve_if(span, args, IfKind::IfThen2);
                    }
                    if name == "assert" {
                        return self.resolve_assert(span, args);
                    }
                    if name == "ensure" {
                        return self.resolve_ensure(span, args);
                    }
                    if name == "and" {
                        return self.resolve_logic_call(span, args, LogicKind::And);
                    }
                    if name == "or" {
                        return self.resolve_logic_call(span, args, LogicKind::Or);
                    }
                }

                if Self::conversion_call_head(&func).is_some() {
                    let resolved_func = self.resolve_node(*func)?;
                    if args.len() != 2 {
                        return Err(ResolveError {
                            message: "from/try_from expects exactly 2 positional arguments".into(),
                            span,
                        });
                    }
                    let mut resolved_args = Vec::with_capacity(2);
                    for (idx, arg) in args.into_iter().enumerate() {
                        match arg {
                            RecordLitArg::Positional(expr) if idx == 1 => {
                                let witness_ty = Self::type_witness_from_expr(expr)?;
                                resolved_args.push(ResolvedRecordLitArg::Positional(
                                    Resolved::TypeRefWitness(
                                        Self::type_witness_span(&witness_ty).clone(),
                                        witness_ty,
                                    ),
                                ));
                            }
                            RecordLitArg::Positional(expr) => resolved_args
                                .push(ResolvedRecordLitArg::Positional(self.resolve_node(expr)?)),
                            RecordLitArg::Named(_, expr) => {
                                return Err(ResolveError {
                                    message: "from/try_from does not accept named arguments".into(),
                                    span: expr.span().clone(),
                                });
                            }
                        }
                    }
                    return Ok(Resolved::App(span, Box::new(resolved_func), resolved_args));
                }

                let resolved_func = self.resolve_node(*func)?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(expr)?))
                        }
                        RecordLitArg::Named(name, expr) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(expr)?))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::App(span, Box::new(resolved_func), resolved_args))
            }

            Ast::Bind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::Bind(span, resolved_pat, Box::new(resolved_rhs)))
            }

            Ast::SafeBind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::SafeBind(
                    span,
                    resolved_pat,
                    Box::new(resolved_rhs),
                ))
            }

            Ast::BinOp(span, op, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::BinOp(span, op, Box::new(l), Box::new(r)))
            }

            Ast::Pipe(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.desugar_pipeline_rhs_special_form_partial(*right);
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::Pipe(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextMap(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.desugar_pipeline_rhs_special_form_partial(*right);
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::ContextMap(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextBind(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.desugar_pipeline_rhs_special_form_partial(*right);
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::ContextBind(span, Box::new(l), Box::new(r)))
            }

            Ast::Compose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::Compose(span, Box::new(l), Box::new(r)))
            }

            Ast::KleisliCompose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::KleisliCompose(span, Box::new(l), Box::new(r)))
            }

            Ast::ListNil(span) => Ok(Resolved::ListNil(span)),

            Ast::ListCons(span, head, tail) => {
                let head = self.resolve_node(*head)?;
                let tail = self.resolve_node(*tail)?;
                Ok(Resolved::ListCons(span, Box::new(head), Box::new(tail)))
            }

            Ast::ListLiteral(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::ListLiteral(span, resolved))
            }

            Ast::TupleLiteral(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::TupleLiteral(span, resolved))
            }

            Ast::InterpolatedStr(span, parts) => {
                let mut resolved_parts = Vec::new();
                for part in parts {
                    match part {
                        spire::ast::InterpolatedPart::Text(s) => {
                            resolved_parts.push(ResolvedInterpolatedPart::Text(s));
                        }
                        spire::ast::InterpolatedPart::Expr(expr) => {
                            let resolved_expr = self.resolve_node(*expr)?;
                            resolved_parts
                                .push(ResolvedInterpolatedPart::Expr(Box::new(resolved_expr)));
                        }
                    }
                }
                Ok(Resolved::InterpolatedStr(span, resolved_parts))
            }

            Ast::FieldAccess(span, expr, field) => {
                let resolved_expr = self.resolve_node(*expr)?;
                Ok(Resolved::FieldAccess(span, Box::new(resolved_expr), field))
            }

            Ast::Block(span, stmts) => {
                let resolved = self.with_child_scope(|child| {
                    stmts
                        .into_iter()
                        .map(|s| child.resolve_node(s))
                        .collect::<Result<Vec<_>, _>>()
                })?;
                Ok(Resolved::Block(span, resolved))
            }

            Ast::Semi(span, inner) => {
                let resolved = self.resolve_node(*inner)?;
                Ok(Resolved::Semi(span, Box::new(resolved)))
            }

            // Struct/Record/Deferror definitions — reuse predeclared IDs
            Ast::StructDef(span, name, fields) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| {
                        Ok(ResolvedField {
                            id: None,
                            name: f.name,
                            ty: self.resolve_type_annotation(f.ty)?,
                            span: f.span,
                            visibility: f.visibility,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructDef(span, rid, rfields))
            }

            Ast::RecordDef(span, name, fields) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| {
                        Ok(ResolvedField {
                            id: None,
                            name: f.name,
                            ty: self.resolve_type_annotation(f.ty)?,
                            span: f.span,
                            visibility: f.visibility,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::RecordDef(span, rid, rfields))
            }

            Ast::DeferrorDef(span, name, fields, show_expr, _) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let mut error_scope = self.scope.clone();
                let mut rfields = Vec::new();
                for f in fields {
                    let uid = error_scope.define(&f.name, f.span.clone());
                    rfields.push(ResolvedField {
                        id: Some(ResolvedId {
                            name: f.name.clone(),
                            qualified_name: None,
                            unique_id: uid,
                            span: f.span.clone(),
                        }),
                        name: f.name,
                        ty: self.resolve_type_annotation(f.ty)?,
                        span: f.span,
                        visibility: f.visibility,
                    });
                }
                let mut show_resolver = Resolver::with_scope(error_scope);
                let resolved_show = show_resolver.resolve_node(*show_expr)?;
                self.scope.advance_next_id_to(show_resolver.scope.next_id());
                Ok(Resolved::DeferrorDef(
                    span,
                    rid,
                    rfields,
                    Box::new(resolved_show),
                ))
            }

            Ast::EnumDef(span, name, type_params, variants, _) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name: name.clone(),
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_type_params = type_params
                    .into_iter()
                    .map(|param| self.resolve_type_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;

                let mut resolved_variants = Vec::new();
                for variant in variants {
                    let ctor_name = format!("{}::{}", name, variant.name);
                    let ctor_uid = self
                        .take_predeclared_id(&ctor_name)
                        .or_else(|| self.scope.lookup(&ctor_name))
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.scope.define_with_id(&ctor_name, ctor_uid);
                    let qualified_ctor_name = self.qualify_current_declaration_name(&ctor_name);
                    resolved_variants.push(ResolvedEnumVariant {
                        id: ResolvedId {
                            name: ctor_name,
                            qualified_name: Some(qualified_ctor_name),
                            unique_id: ctor_uid,
                            span: variant.span.clone(),
                        },
                        payload: variant
                            .payload
                            .into_iter()
                            .map(|ty| self.resolve_type_annotation(ty))
                            .collect::<Result<Vec<_>, ResolveError>>()?,
                        discriminant: variant.discriminant,
                        span: variant.span,
                    });
                }

                Ok(Resolved::EnumDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_variants,
                ))
            }

            Ast::Def(span, name, type_params, params, ret_ty, body, attrs) => {
                let fun_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_scope = self.scope.clone();
                // Ensure self-recursion inside this definition binds to this declaration,
                // not to a newer same-name declaration predeclared later in the chunk.
                body_scope.define_with_id(&name, fun_uid);
                let mut body_resolver = Resolver::with_scope(body_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let resolved_params = params
                    .into_iter()
                    .map(|param| body_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: fun_uid,
                    span: span.clone(),
                };

                Ok(Resolved::Def(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_params,
                    ret_ty
                        .map(|ty| self.resolve_type_annotation(ty))
                        .transpose()?,
                    Box::new(resolved_body),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ExtractorDef(span, name, type_params, param, ret_ty, body, attrs) => {
                let fun_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_scope = self.scope.clone();
                body_scope.define_with_id(&name, fun_uid);
                let mut body_resolver = Resolver::with_scope(body_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let resolved_param = body_resolver.resolve_extractor_param(param)?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: fun_uid,
                    span: span.clone(),
                };

                Ok(Resolved::ExtractorDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_param,
                    self.resolve_type_annotation(ret_ty)?,
                    Box::new(resolved_body),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::TraitDef(span, name, type_params, methods, attrs) => {
                let qualified_trait_name = self.qualify_current_declaration_name(&name);
                let trait_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, trait_uid);
                let rid = ResolvedId {
                    name: name.clone(),
                    qualified_name: Some(qualified_trait_name.clone()),
                    unique_id: trait_uid,
                    span: span.clone(),
                };
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let mut resolved_methods = Vec::new();
                for method in methods {
                    let spire::ast::TraitMethodSig {
                        name: method_name,
                        type_params,
                        params,
                        ret_ty,
                        span: method_span,
                    } = method;
                    let method_alias = trait_method_qualified_name(&name, &method_name);
                    let qualified_method =
                        trait_method_qualified_name(&qualified_trait_name, &method_name);
                    let method_uid = self
                        .take_predeclared_id(&method_alias)
                        .or_else(|| self.scope.lookup(&method_alias))
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.scope.define_with_id(&method_alias, method_uid);

                    let mut method_resolver = Resolver::with_scope(self.scope.clone());
                    method_resolver.declaration_uids = self.declaration_uids.clone();
                    method_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                    method_resolver.current_module_path = self.current_module_path.clone();
                    method_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                    let resolved_params = params
                        .into_iter()
                        .map(|param| method_resolver.resolve_fun_param(param))
                        .collect::<Result<Vec<_>, ResolveError>>()?;
                    self.scope
                        .advance_next_id_to(method_resolver.scope.next_id());
                    resolved_methods.push(ResolvedTraitMethodSig {
                        id: ResolvedId {
                            name: method_name,
                            qualified_name: Some(qualified_method),
                            unique_id: method_uid,
                            span: method_span.clone(),
                        },
                        type_params: self.resolve_type_params(type_params)?,
                        params: resolved_params,
                        ret_ty: self.resolve_type_annotation(ret_ty)?,
                        span: method_span,
                    });
                }
                Ok(Resolved::TraitDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_methods,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::TraitImplDef(span, trait_name, trait_args, target_ty, methods) => {
                let (trait_uid, qualified_trait_name) =
                    self.resolve_trait_reference(&trait_name, &span)?;
                let trait_id = ResolvedId {
                    name: trait_name.clone(),
                    qualified_name: Some(qualified_trait_name.clone()),
                    unique_id: trait_uid,
                    span: span.clone(),
                };
                let resolved_target_ty = self.resolve_type_annotation(target_ty)?;
                let target_key = match &resolved_target_ty {
                    AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
                    AstTy::Generic(_, name, args) => format!(
                        "{}<{}>",
                        name,
                        args.iter()
                            .map(Self::ast_ty_symbol_key)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    AstTy::Tuple(_, items) => format!(
                        "({})",
                        items
                            .iter()
                            .map(Self::ast_ty_symbol_key)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    AstTy::Func(_, params, ret) => format!(
                        "({} -> {})",
                        params
                            .iter()
                            .map(Self::ast_ty_symbol_key)
                            .collect::<Vec<_>>()
                            .join(", "),
                        Self::ast_ty_symbol_key(ret)
                    ),
                };
                let mut resolved_methods = Vec::new();
                for method in methods {
                    let Ast::Def(
                        method_span,
                        method_name,
                        type_params,
                        params,
                        ret_ty,
                        body,
                        attrs,
                    ) = method
                    else {
                        return Err(ResolveError {
                            message: "trait impl body may only contain `def` declarations"
                                .to_string(),
                            span: span.clone(),
                        });
                    };

                    let qualified_function_name = trait_impl_method_qualified_name(
                        self.current_module_path.as_deref(),
                        &trait_name,
                        &trait_args,
                        &resolved_target_ty,
                        &method_name,
                        method_span.start,
                    );
                    let method_uid = self
                        .declaration_uids
                        .get(&qualified_function_name)
                        .copied()
                        .unwrap_or_else(|| self.scope.reserve_id());
                    let mut body_scope = self.scope.clone();
                    body_scope.define_with_id(&method_name, method_uid);
                    let mut body_resolver = Resolver::with_scope(body_scope);
                    body_resolver.declaration_uids = self.declaration_uids.clone();
                    body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                    body_resolver.current_module_path = self.current_module_path.clone();
                    body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                    let resolved_params = params
                        .into_iter()
                        .map(|param| body_resolver.resolve_fun_param(param))
                        .collect::<Result<Vec<_>, ResolveError>>()?;
                    let resolved_body = body_resolver.resolve_node(*body)?;
                    self.scope.advance_next_id_to(body_resolver.scope.next_id());
                    let local_function_name = if trait_args.is_empty() {
                        format!("{}::{}", target_key, method_name)
                    } else {
                        format!(
                            "{}::{}::{}",
                            trait_instance_key(&qualified_trait_name, &trait_args),
                            target_key,
                            method_name
                        )
                    };

                    resolved_methods.push(ResolvedTraitImplMethod {
                        method_name: method_name.clone(),
                        function_id: ResolvedId {
                            name: local_function_name,
                            qualified_name: Some(qualified_function_name),
                            unique_id: method_uid,
                            span: method_span.clone(),
                        },
                        type_params: self.resolve_type_params(type_params)?,
                        params: resolved_params,
                        ret_ty: ret_ty
                            .map(|ty| self.resolve_type_annotation(ty))
                            .transpose()?,
                        body: Box::new(resolved_body),
                        attrs: resolve_decl_attrs(&attrs),
                        span: method_span,
                    });
                }

                Ok(Resolved::TraitImplDef(
                    span,
                    trait_id,
                    trait_args
                        .into_iter()
                        .map(|arg| self.resolve_type_annotation(arg))
                        .collect::<Result<Vec<_>, _>>()?,
                    resolved_target_ty,
                    resolved_methods,
                ))
            }

            Ast::BuiltinDecl(span, name, params, ret_ty, attrs) => {
                if !is_runtime_builtin_decl(&name) && !is_special_form_builtin_decl(&name) {
                    return Err(ResolveError {
                        message: format!("Unknown builtin declaration: {}", name),
                        span,
                    });
                }

                let builtin_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut decl_resolver = Resolver::with_scope(self.scope.clone());
                let resolved_params = params
                    .into_iter()
                    .map(|param| decl_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                self.scope.advance_next_id_to(decl_resolver.scope.next_id());
                self.scope.define_with_id(&name, builtin_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_uid,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinDecl(
                    span,
                    rid,
                    resolved_params,
                    ret_ty
                        .map(|ty| self.resolve_type_annotation(ty))
                        .transpose()?,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::BuiltinExtractorDecl(span, name, param, ret_ty, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_param = self.resolve_extractor_param(param)?;
                Ok(Resolved::BuiltinExtractorDecl(
                    span,
                    rid,
                    resolved_param,
                    self.resolve_type_annotation(ret_ty)?,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::BuiltinTypeDecl(span, head, attrs) => {
                let builtin_type_uid = self
                    .take_predeclared_id(&head.name)
                    .unwrap_or_else(|| self.scope.reserve_id());
                let qualified_name = self.qualify_current_declaration_name(&head.name);
                let rid = ResolvedId {
                    name: head.name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_type_uid,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinTypeDecl(
                    span,
                    rid,
                    head.params,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ResultCtorDecl(span, name, param_ty, ret_ty, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                Ok(Resolved::ResultCtorDecl(
                    span,
                    rid,
                    param_ty,
                    ret_ty,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::Defmod(span, name, _, _) => Err(ResolveError {
                message: format!("Module resolution is not implemented yet: {}", name),
                span,
            }),
            Ast::Import(span, _, _) => Err(ResolveError {
                message: "Import resolution is not implemented yet".to_string(),
                span,
            }),
            Ast::Include(span, _) => Err(ResolveError {
                message: "include directives must be resolved before name resolution".to_string(),
                span,
            }),
            Ast::ImplDef(span, target, _) => Err(ResolveError {
                message: format!("impl lowering failed for target `{}`", target),
                span,
            }),

            Ast::Closure(span, params, body) => {
                let mut closure_scope = self.scope.clone();
                let mut resolved_params = Vec::new();
                for param in params {
                    let uid = closure_scope.define(&param.name, param.span.clone());
                    resolved_params.push(ResolvedClosureParam {
                        id: ResolvedId {
                            name: param.name,
                            qualified_name: None,
                            unique_id: uid,
                            span: param.span,
                        },
                        ty: param.ty,
                    });
                }

                let mut body_resolver = Resolver::with_scope(closure_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_body = body_resolver.resolve_node(*body)?;
                self.scope.advance_next_id_to(body_resolver.scope.next_id());

                let captures = collect_captures(&resolved_body, &resolved_params);

                Ok(Resolved::Closure(
                    span,
                    resolved_params,
                    captures,
                    Box::new(resolved_body),
                ))
            }

            Ast::Capture(span, target, args) => {
                let resolved_target = self.resolve_node(*target)?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| self.resolve_node(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::Capture(
                    span,
                    Box::new(resolved_target),
                    resolved_args,
                ))
            }

            Ast::StructLit(span, type_name, field_vals) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                })?;
                let rid = ResolvedId {
                    name: type_name,
                    qualified_name: None,
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_fields = field_vals
                    .into_iter()
                    .map(|(name, expr)| Ok((name, self.resolve_node(expr)?)))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructLit(span, rid, resolved_fields))
            }

            Ast::ConstructorCall(span, type_name, args) => {
                let normalized_name = {
                    // In ExprBlock, a struct head like `User(...)` dispatches to
                    // `User::new(...)` when that constructor exists.
                    let sugared = format!("{}::new", type_name);
                    if self.scope.lookup(&sugared).is_some() {
                        sugared
                    } else {
                        type_name
                    }
                };
                let uid = self
                    .scope
                    .lookup(&normalized_name)
                    .ok_or_else(|| ResolveError {
                        message: format!("Undefined type: {}", normalized_name),
                        span: span.clone(),
                    })?;
                let rid = ResolvedId {
                    name: normalized_name,
                    qualified_name: None,
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        spire::ast::RecordLitArg::Positional(e) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(e)?))
                        }
                        spire::ast::RecordLitArg::Named(name, e) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(e)?))
                        }
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::ConstructorCall(span, rid, resolved_args))
            }

            Ast::Match(span, scrutinee, arms) => {
                let resolved_scrut = self.resolve_node(*scrutinee)?;
                let resolved_arms = arms
                    .into_iter()
                    .map(|arm| self.resolve_match_arm(arm))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::Match(
                    span,
                    Box::new(resolved_scrut),
                    resolved_arms,
                ))
            }
        }
    }

    pub(super) fn resolve_fun_param(
        &mut self,
        param: FunParam,
    ) -> Result<ResolvedFunParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedFunParam {
            id: ResolvedId {
                name: param.name,
                qualified_name: None,
                unique_id: uid,
                span: param.span,
            },
            ty: self.resolve_type_annotation(param.ty)?,
        })
    }

    pub(super) fn resolve_type_params(
        &self,
        type_params: Vec<spire::ast::TypeParam>,
    ) -> Result<Vec<ResolvedTypeParam>, ResolveError> {
        type_params
            .into_iter()
            .map(|param| self.resolve_type_param(param))
            .collect()
    }

    pub(super) fn resolve_extractor_param(
        &mut self,
        param: ExtractorParam,
    ) -> Result<ResolvedExtractorParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedExtractorParam {
            id: ResolvedId {
                name: param.name,
                qualified_name: None,
                unique_id: uid,
                span: param.span,
            },
            ty: param
                .ty
                .map(|ty| self.resolve_type_annotation(ty))
                .transpose()?,
        })
    }

    fn resolve_type_param(
        &self,
        param: spire::ast::TypeParam,
    ) -> Result<ResolvedTypeParam, ResolveError> {
        Ok(ResolvedTypeParam {
            name: param.name,
            bound: param
                .bound
                .map(|bound| self.resolve_trait_bound_name(&bound, &param.span))
                .transpose()?,
            span: param.span,
        })
    }

    fn resolve_type_annotation(&self, ty: AstTy) -> Result<AstTy, ResolveError> {
        match ty {
            AstTy::Named(span, name) => Ok(AstTy::Named(span, name)),
            AstTy::ImplTrait(span, name) => Ok(AstTy::ImplTrait(
                span.clone(),
                self.resolve_trait_bound_name(&name, &span)?,
            )),
            AstTy::Generic(span, name, args) => Ok(AstTy::Generic(
                span,
                name,
                args.into_iter()
                    .map(|arg| self.resolve_type_annotation(arg))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            AstTy::Tuple(span, items) => Ok(AstTy::Tuple(
                span,
                items
                    .into_iter()
                    .map(|item| self.resolve_type_annotation(item))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            AstTy::Func(span, params, ret) => Ok(AstTy::Func(
                span,
                params
                    .into_iter()
                    .map(|param| self.resolve_type_annotation(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
                Box::new(self.resolve_type_annotation(*ret)?),
            )),
        }
    }

    fn resolve_trait_reference(
        &self,
        trait_name: &str,
        span: &Span,
    ) -> Result<(u32, String), ResolveError> {
        let trait_uid = self.scope.lookup(trait_name).ok_or_else(|| ResolveError {
            message: format!("Undefined trait: {}", trait_name),
            span: span.clone(),
        })?;
        match self.declaration_uid_kinds.get(&trait_uid) {
            Some(DeclarationKind::Trait) => {}
            _ => {
                return Err(ResolveError {
                    message: format!("{} is not a trait", trait_name),
                    span: span.clone(),
                });
            }
        }
        let qualified_name = self
            .declaration_fq_name_for_uid(trait_uid)
            .unwrap_or_else(|| trait_name.to_string());
        Ok((trait_uid, qualified_name))
    }

    fn resolve_trait_bound_name(
        &self,
        trait_name: &str,
        span: &Span,
    ) -> Result<String, ResolveError> {
        self.resolve_trait_reference(trait_name, span)
            .map(|(_, qualified_name)| qualified_name)
    }

    fn ast_ty_symbol_key(ty: &AstTy) -> String {
        match ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                name,
                args.iter()
                    .map(Self::ast_ty_symbol_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::ast_ty_symbol_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => format!(
                "({} -> {})",
                params
                    .iter()
                    .map(Self::ast_ty_symbol_key)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::ast_ty_symbol_key(ret)
            ),
        }
    }

    fn validate_trait_impl_pairs(&self, resolved: &[Resolved]) -> Result<(), ResolveError> {
        let mut seen_pairs = HashSet::new();
        for node in resolved {
            let Resolved::TraitImplDef(span, trait_id, trait_args, target_ty, _) = node else {
                continue;
            };
            let trait_name = trait_instance_key(
                trait_id.qualified_name.as_deref().unwrap_or(&trait_id.name),
                trait_args,
            );
            let pair_key = format!("{} for {}", trait_name, Self::ast_ty_symbol_key(target_ty));
            if !seen_pairs.insert(pair_key.clone()) {
                return Err(ResolveError {
                    message: format!(
                        "Multiple trait impl blocks for `{}` are not allowed",
                        pair_key
                    ),
                    span: span.clone(),
                });
            }
        }
        Ok(())
    }
}
