use super::*;
use sindr::names::{builtin_type_name, TypeName};

#[derive(Clone, Copy)]
enum SignatureTyMode<'a> {
    Normal,
    Trait { self_ty: &'a Ty },
    Builtin,
}

impl<'a> SignatureTyMode<'a> {
    fn self_ty(self) -> Option<&'a Ty> {
        match self {
            SignatureTyMode::Trait { self_ty } => Some(self_ty),
            SignatureTyMode::Normal | SignatureTyMode::Builtin => None,
        }
    }

    fn allows_lazy(self) -> bool {
        matches!(self, SignatureTyMode::Builtin)
    }

    fn allows_user_generic_fallback(self) -> bool {
        !matches!(self, SignatureTyMode::Builtin)
    }

    fn rejects_impl_trait_in_error_marker(self) -> bool {
        matches!(self, SignatureTyMode::Normal)
    }
}

impl Checker {
    fn canonical_user_type_name(name: &str) -> String {
        if name.contains("::") {
            name.to_string()
        } else {
            format!("Global::{name}")
        }
    }

    pub(super) fn is_duration_ty(ty: &Ty) -> bool {
        matches!(ty, Ty::Struct(name, _) if Self::surface_name(name) == "Duration")
    }

    fn match_result_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message: "MatchResult is extractor-only and can only be used in extractor definitions"
                .into(),
            span: span.clone(),
            hint: Some(
                "Use MatchResult only as a defextractor / @builtin defextractor return type or inside an extractor body. Ordinary APIs should return Result, Option, or a user-defined enum explicitly."
                    .into(),
            ),
        }
    }

    fn seq_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message: "Seq is not a surface type in this version of Surtr".into(),
            span: span.clone(),
            hint: Some(
                "Use tuple payloads for extractor success values, such as MatchResult<(A, B), Error>."
                    .into(),
            ),
        }
    }

    pub(super) fn error_function_param_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message: "Error cannot be used as a user-defined function parameter type".into(),
            span: span.clone(),
            hint: Some(
                "Keep Error inside Err(...), and inspect it only from an Err(...) match arm."
                    .into(),
            ),
        }
    }

    fn lazy_type_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message: "Lazy<T> is reserved for std-module special-form declarations".into(),
            span: span.clone(),
            hint: Some(
                "Use ordinary expression syntax at call sites; the compiler applies lazy special-form evaluation automatically."
                    .into(),
            ),
        }
    }

    pub(super) fn ty_exposes_error_value(ty: &Ty) -> bool {
        match ty {
            Ty::Error => true,
            Ty::Result(ok, _) => Self::ty_exposes_error_value(ok),
            Ty::List(inner) | Ty::TypeRef(inner) | Ty::Lazy(inner) => {
                Self::ty_exposes_error_value(inner)
            }
            Ty::Facet(source, focus) => {
                Self::ty_exposes_error_value(source) || Self::ty_exposes_error_value(focus)
            }
            Ty::Tuple(items) | Ty::Enum(_, items) => items.iter().any(Self::ty_exposes_error_value),
            Ty::Func(params, ret) => {
                params.iter().any(Self::ty_exposes_error_value) || Self::ty_exposes_error_value(ret)
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params.iter().any(Self::ty_exposes_error_value) || Self::ty_exposes_error_value(ret)
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
                .iter()
                .any(|(_, field_ty)| Self::ty_exposes_error_value(field_ty)),
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Hole
            | Ty::Var(_)
            | Ty::Pid(_) => false,
        }
    }

    pub(super) fn allows_std_error_function_param_exception(id: &ResolvedId) -> bool {
        matches!(
            Self::surface_qualified_name(id.qualified_name.as_deref()),
            Some("Result::tap_err") | Some("Result::_tap_err_value") | Some("Test::_finish_it_err")
        )
    }

    fn match_result_type_allowed(&self, context: TypeSyntaxContext) -> bool {
        matches!(
            context,
            TypeSyntaxContext::ExtractorReturn | TypeSyntaxContext::ExtractorBody
        ) || self.in_extractor_body
    }

    pub(super) fn local_type_syntax_context(&self) -> TypeSyntaxContext {
        if self.in_extractor_body {
            TypeSyntaxContext::ExtractorBody
        } else {
            TypeSyntaxContext::BindingAnnotation
        }
    }

    fn type_ref_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message:
                "TypeRef<$T> is only allowed as a trait method parameter tied to a trait head type parameter."
                    .into(),
            span: span.clone(),
            hint: Some(
                "Use TypeRef<$T> only in deftrait / impl Trait method parameters, not in ordinary signatures or return types."
                    .into(),
            ),
        }
    }

    fn resolve_pid_surface_ty(&self, span: &Span, args: &[AstTy]) -> Result<Ty, TypeError> {
        if args.len() != 1 {
            return Err(TypeError {
                message: "PID<T> requires exactly 1 type argument".into(),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(Ty::Pid(self.pid_marker_from_ast(&args[0])?))
    }

    fn resolve_worker_handle_surface_ty(
        &self,
        span: &Span,
        args: &[AstTy],
        handle_name: &str,
    ) -> Result<Ty, TypeError> {
        if args.len() != 1 {
            return Err(TypeError {
                message: format!("{handle_name}<Worker> requires exactly 1 type argument"),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(Ty::Enum(
            handle_name.to_string(),
            vec![Ty::Pid(self.pid_marker_from_ast(&args[0])?)],
        ))
    }

    fn resolve_task_handle_surface_ty(&self, span: &Span, args: &[AstTy]) -> Result<Ty, TypeError> {
        if args.len() != 1 {
            return Err(TypeError {
                message: "TaskHandle<T> requires exactly 1 type argument".into(),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(Ty::Enum(
            "TaskHandle".to_string(),
            vec![self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?],
        ))
    }

    fn ast_ty_is_none_error_marker(ast_ty: &AstTy) -> bool {
        match ast_ty {
            AstTy::Named(_, name) | AstTy::Generic(_, name, _) => {
                Self::surface_name(name) == "NoneError"
            }
            _ => false,
        }
    }

    fn pid_marker_from_ast(&self, ast_ty: &AstTy) -> Result<String, TypeError> {
        match ast_ty {
            AstTy::Named(_, name) => Ok(name.clone()),
            other => Err(TypeError {
                message: "PID<T> expects a process marker such as PID<Counter>".into(),
                span: Self::ast_ty_span(other).clone(),
                hint: Some(
                    "Use the generated process surface marker name, for example PID<Counter>."
                        .into(),
                ),
            }),
        }
    }

    fn clause_block_type_not_allowed_error(&self, span: &Span, surface_name: &str) -> TypeError {
        let (rendered, special_form, hint) = match surface_name {
            "MatchArms" => (
                "MatchArms<$Scrutinee, $Result>",
                "`match`",
                "Use the dedicated `match value { pattern => expr, ... }` surface instead of passing or storing its clause block as a value type.",
            ),
            "CondClauses" => (
                "CondClauses<$Result>",
                "`cond`",
                "Use the dedicated `cond { cond1 => expr1, ..., True => exprN }` surface instead of passing or storing its clause block as a value type.",
            ),
            "BulkUpdateEntries" => (
                "BulkUpdateEntries<$State>",
                "`Facet::bulk_update`",
                "Use the dedicated `Facet::bulk_update(source) { ... }` surface instead of passing or storing its update block as a value type.",
            ),
            other => (
                other,
                "special form",
                "Do not use this compiler-reserved clause-block type in ordinary user signatures.",
            ),
        };
        TypeError {
            message: format!("{rendered} is reserved for the {special_form} special form"),
            span: span.clone(),
            hint: Some(hint.into()),
        }
    }

    fn hole_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message: "`_` is only allowed as an ignored-input marker inside callable types used by variable annotations or function return signatures.".into(),
            span: span.clone(),
            hint: Some(
                "Use `_` only in signatures such as `(_ -> Int)` or `(... -> (_ -> Int))`; it cannot appear as a plain value type, function parameter type, or container element."
                    .into(),
            ),
        }
    }

    fn resolve_hole_surface_ty(
        &self,
        span: &Span,
        context: TypeSyntaxContext,
    ) -> Result<Ty, TypeError> {
        match context {
            TypeSyntaxContext::HoleClosureParam => Ok(Ty::Hole),
            _ => Err(self.hole_not_allowed_error(span)),
        }
    }

    pub(super) fn resolve_trait_type_ref_param_ty(
        &mut self,
        ast_ty: &AstTy,
        trait_head_bindings: &HashMap<String, Ty>,
        allow_concrete_inner: bool,
        self_ty: &Ty,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Option<Ty>, TypeError> {
        let AstTy::Generic(span, name, args) = ast_ty else {
            return Ok(None);
        };
        if name != "TypeRef" {
            return Ok(None);
        }
        if args.len() != 1 {
            return Err(TypeError {
                message: "TypeRef<T> requires exactly 1 type argument".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let inner = match &args[0] {
            AstTy::Named(_, name) if name.starts_with('$') => {
                trait_head_bindings
                    .get(name)
                    .cloned()
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "TypeRef<{}> must refer to a type parameter declared on the surrounding trait head",
                            name
                        ),
                        span: Self::ast_ty_span(&args[0]).clone(),
                        hint: None,
                    })?
            }
            other if allow_concrete_inner => self.resolve_trait_signature_ast_ty_in_context(
                other,
                TypeSyntaxContext::General,
                self_ty,
                tyvars,
            )?,
            _ => {
                return Err(TypeError {
                    message:
                        "TypeRef<$T> in trait declarations must point at a trait head type parameter."
                            .into(),
                    span: Self::ast_ty_span(&args[0]).clone(),
                    hint: None,
                });
            }
        };

        if matches!(inner, Ty::TypeRef(_)) {
            return Err(TypeError {
                message: "Nested TypeRef is not supported".into(),
                span: span.clone(),
                hint: None,
            });
        }

        Ok(Some(Ty::TypeRef(Box::new(inner))))
    }

    pub(super) fn register_tyvar_bound(&mut self, var: u32, trait_name: &str) {
        let bounds = self.tyvar_bounds.entry(var).or_default();
        if !bounds.iter().any(|bound| bound == trait_name) {
            bounds.push(trait_name.to_string());
            bounds.sort();
        }
    }

    pub(super) fn register_tyvar_bounds(&mut self, var: u32, bounds: &[String]) {
        for bound in bounds {
            self.register_tyvar_bound(var, bound);
        }
    }

    pub(super) fn tyvar_bound_names(&self, var: u32) -> Vec<String> {
        self.tyvar_bounds.get(&var).cloned().unwrap_or_default()
    }

    pub(super) fn tyvar_has_bound(&self, var: u32, trait_name: &str) -> bool {
        self.tyvar_bounds
            .get(&var)
            .is_some_and(|bounds| bounds.iter().any(|bound| bound == trait_name))
    }

    pub(super) fn lit_type(&self, lit: &Lit) -> Ty {
        match lit {
            Lit::Int(_) => Ty::Int,
            Lit::Float(_) => Ty::Float,
            Lit::Str(_) => Ty::Str,
            Lit::Bool(_) => Ty::Bool,
            Lit::Unit => Ty::Unit,
        }
    }

    pub(super) fn ast_ty_span(ast_ty: &AstTy) -> &Span {
        match ast_ty {
            AstTy::Named(span, _)
            | AstTy::Generic(span, _, _)
            | AstTy::Tuple(span, _)
            | AstTy::Func(span, _, _)
            | AstTy::ImplTrait(span, _) => span,
        }
    }

    pub(super) fn collect_type_ref_names(ast_ty: &AstTy, out: &mut Vec<String>) {
        match ast_ty {
            AstTy::Named(_, name) => {
                if !name.starts_with('$') {
                    out.push(Self::canonical_user_type_name(name));
                }
            }
            AstTy::Generic(_, _, args) => {
                for arg in args {
                    Self::collect_type_ref_names(arg, out);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_type_ref_names(item, out);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_type_ref_names(param, out);
                }
                Self::collect_type_ref_names(ret, out);
            }
            AstTy::ImplTrait(_, name) => out.push(Self::canonical_user_type_name(name)),
        }
    }

    fn require_type_arg_count<'a>(
        &self,
        span: &Span,
        args: &'a [AstTy],
        expected: usize,
        message: &'static str,
    ) -> Result<&'a [AstTy], TypeError> {
        if args.len() != expected {
            return Err(TypeError {
                message: message.into(),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(args)
    }

    pub(super) fn resolve_ast_ty_in_context(
        &self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
    ) -> Result<Ty, TypeError> {
        if context == TypeSyntaxContext::ErrorMarker {
            return self.resolve_error_marker_type(ast_ty);
        }

        match ast_ty {
            AstTy::Named(span, name) => {
                match Self::surface_name(name) {
                    generic_name if generic_name.starts_with('$') => self
                        .local_annotation_tyvars
                        .get(generic_name)
                        .cloned()
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type: {}", name),
                            span: span.clone(),
                            hint: Some(
                                "Local type annotations may only reference type parameters declared on the surrounding function or method signature."
                                    .into(),
                            ),
                        }),
                    "_" | "Hole" => self.resolve_hole_surface_ty(span, context),
                    "MatchArms" | "CondClauses" | "BulkUpdateEntries" => Err(self
                        .clause_block_type_not_allowed_error(span, Self::surface_name(name))),
                    "Seq" => Err(self.seq_not_allowed_error(span)),
                    builtin_name => {
                        if let Some(def) = self.env.lookup_type_def(name) {
                            match &def.kind {
                                crate::env::TypeKind::Struct => {
                                    if !def.type_params.is_empty() {
                                        return Err(TypeError {
                                            message: format!(
                                                "Type {} requires {} type argument(s)",
                                                name,
                                                def.type_params.len()
                                            ),
                                            span: span.clone(),
                                            hint: None,
                                        });
                                    }
                                    return Ok(Ty::Struct(def.name.clone(), def.fields.clone()));
                                }
                                crate::env::TypeKind::Record => {
                                    return Ok(Ty::Record(def.name.clone(), def.fields.clone()));
                                }
                                crate::env::TypeKind::ConcreteError => return Ok(Ty::Error),
                                crate::env::TypeKind::Enum => {
                                    if def.type_params.is_empty() {
                                        return Ok(Ty::Enum(def.name.clone(), Vec::new()));
                                    }
                                    return Err(TypeError {
                                        message: format!(
                                            "Type {} requires {} type argument(s)",
                                            name,
                                            def.type_params.len()
                                        ),
                                        span: span.clone(),
                                        hint: None,
                                    });
                                }
                            }
                        }
                        match builtin_type_name(builtin_name) {
                            Some(TypeName::Int) => Ok(Ty::Int),
                            Some(TypeName::Float) => Ok(Ty::Float),
                            Some(TypeName::String) => Ok(Ty::Str),
                            Some(TypeName::Boolean) => Ok(Ty::Bool),
                            Some(TypeName::Unit) => Ok(Ty::Unit),
                            Some(TypeName::Error) => Ok(Ty::Error),
                            Some(TypeName::Regex) => {
                                Ok(Ty::Enum(TypeName::Regex.as_str().into(), Vec::new()))
                            }
                            Some(TypeName::RegexCaptures) => Ok(Ty::Enum(
                                TypeName::RegexCaptures.as_str().into(),
                                Vec::new(),
                            )),
                            Some(TypeName::RegexMatch) => {
                                Ok(Ty::Enum(TypeName::RegexMatch.as_str().into(), Vec::new()))
                            }
                            Some(TypeName::RandomGenerator) => Ok(Ty::Enum(
                                TypeName::RandomGenerator.as_str().into(),
                                Vec::new(),
                            )),
                            Some(TypeName::FileHandle) => Ok(Ty::Enum(
                                TypeName::FileHandle.as_str().into(),
                                Vec::new(),
                            )),
                            Some(
                                TypeName::List
                                | TypeName::HashMap
                                | TypeName::Generator
                                | TypeName::Result
                                | TypeName::Duration
                                | TypeName::ProcessInit
                                | TypeName::Lazy
                                | TypeName::TypeRef
                                | TypeName::Hole
                                | TypeName::Closure
                                | TypeName::MatchArms
                                | TypeName::CondClauses
                                | TypeName::BulkUpdateEntries
                                | TypeName::Facet
                                | TypeName::Pid
                                | TypeName::Workers
                                | TypeName::WorkerLease
                                | TypeName::TaskHandle,
                            )
                            | None => Err(TypeError {
                                message: format!("Unknown type: {}", name),
                                span: span.clone(),
                                hint: None,
                            }),
                        }
                    }
                }
            }
            AstTy::Generic(span, name, _) if Self::surface_name(name) == "TypeRef" => {
                Err(self.type_ref_not_allowed_error(span))
            }
            AstTy::Generic(span, name, _)
                if matches!(Self::surface_name(name), "MatchArms" | "CondClauses" | "BulkUpdateEntries") =>
            {
                Err(self.clause_block_type_not_allowed_error(span, Self::surface_name(name)))
            }
            AstTy::Generic(span, name, _) if Self::surface_name(name) == "Lazy" => {
                Err(self.lazy_type_not_allowed_error(span))
            }
            AstTy::Generic(span, name, _) if Self::surface_name(name) == "Seq" => {
                Err(self.seq_not_allowed_error(span))
            }
            AstTy::Generic(span, name, args) => match Self::surface_name(name) {
                "MatchResult" => {
                    if !self.match_result_type_allowed(context) {
                        return Err(self.match_result_not_allowed_error(span));
                    }
                    if args.is_empty() || args.len() > 2 {
                        return Err(TypeError {
                            message: "MatchResult<$Value> or MatchResult<$Value, Error> requires 1 or 2 type arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let value =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    if args.len() == 2 {
                        let err =
                            self.resolve_ast_ty_in_context(&args[1], TypeSyntaxContext::General)?;
                        if !matches!(err, Ty::Error) {
                            return Err(TypeError {
                                message: "MatchResult<$Value, Error> requires Error as the second argument".into(),
                                span: span.clone(),
                                hint: None,
                            });
                        }
                    }
                    Ok(Ty::Enum("MatchResult".into(), vec![value]))
                }
                "List" => {
                    let args =
                        self.require_type_arg_count(span, args, 1, "List<T> requires exactly 1 type argument")?;
                    let inner_ty =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::List(Box::new(inner_ty)))
                }
                "HashMap" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "HashMap<V> requires exactly 1 type argument",
                    )?;
                    let value_ty =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::Enum("HashMap".into(), vec![value_ty]))
                }
                "Generator" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        2,
                        "Generator<State, Item> requires exactly 2 type arguments",
                    )?;
                    let state_ty =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    let item_ty =
                        self.resolve_ast_ty_in_context(&args[1], TypeSyntaxContext::General)?;
                    Ok(Ty::Enum("Generator".into(), vec![state_ty, item_ty]))
                }
                "ProcessInit" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "ProcessInit<T> requires exactly 1 type argument",
                    )?;
                    let inner_ty =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::Enum("ProcessInit".into(), vec![inner_ty]))
                }
                "Facet" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        2,
                        "Facet<S, A> requires exactly 2 type arguments",
                    )?;
                    let source =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    let focus =
                        self.resolve_ast_ty_in_context(&args[1], TypeSyntaxContext::General)?;
                    Ok(Ty::Facet(Box::new(source), Box::new(focus)))
                }
                "PID" => self.resolve_pid_surface_ty(span, args),
                "Workers" => self.resolve_worker_handle_surface_ty(span, args, "Workers"),
                "WorkerLease" => self.resolve_worker_handle_surface_ty(span, args, "WorkerLease"),
                "TaskHandle" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "TaskHandle<T> requires exactly 1 type argument",
                    )?;
                    let inner =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::Enum("TaskHandle".into(), vec![inner]))
                }
                "Result" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(TypeError {
                            message: "Result<T> or Result<T, E> requires 1 or 2 type arguments"
                                .into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let ok =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    let err = if args.len() == 2 {
                        let allow_none_error_surface = context != TypeSyntaxContext::FunctionReturn
                            && Self::ast_ty_is_none_error_marker(&args[1]);
                        if context != TypeSyntaxContext::FunctionReturn && !allow_none_error_surface
                        {
                            return Err(TypeError {
                                message:
                                    "Result<T, E> is only allowed in function return signatures."
                                        .into(),
                                span: span.clone(),
                                hint: Some(
                                    "Use Result<T> in local code, or Option<T> / T? for optional-style values."
                                        .into(),
                                ),
                            });
                        }
                        self.resolve_ast_ty_in_context(&args[1], TypeSyntaxContext::ErrorMarker)?
                    } else {
                        Ty::Error
                    };
                    Ok(Ty::Result(Box::new(ok), Box::new(err)))
                }
                _ => {
                    if Self::surface_name(name) == "Workers" {
                        return self.resolve_worker_handle_surface_ty(span, args, "Workers");
                    }
                    if Self::surface_name(name) == "WorkerLease" {
                        return self.resolve_worker_handle_surface_ty(span, args, "WorkerLease");
                    }
                    if Self::surface_name(name) == "TaskHandle" {
                        return self.resolve_task_handle_surface_ty(span, args);
                    }
                    let def = self.env.lookup_type_def(name).ok_or_else(|| TypeError {
                        message: format!("Unknown generic type: {}", name),
                        span: span.clone(),
                        hint: None,
                    })?;
                    if def.type_params.len() != args.len() {
                        return Err(TypeError {
                            message: format!(
                                "Type {} requires {} type argument(s), got {}",
                                name,
                                def.type_params.len(),
                                args.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let resolved_args = args
                        .iter()
                        .map(|arg| self.resolve_ast_ty_in_context(arg, TypeSyntaxContext::General))
                        .collect::<Result<Vec<_>, _>>()?;
                    match def.kind {
                        crate::env::TypeKind::Struct => Ok(Ty::Struct(
                            def.name.clone(),
                            self.instantiate_type_def_fields(def, &resolved_args),
                        )),
                        crate::env::TypeKind::Enum => Ok(Ty::Enum(def.name.clone(), resolved_args)),
                        _ => Err(TypeError {
                            message: format!(
                                "Generic type {} is not supported in this context",
                                name
                            ),
                            span: span.clone(),
                            hint: None,
                        }),
                    }
                }
            },
            AstTy::Tuple(span, items) => {
                if items.len() < 2 {
                    return Err(TypeError {
                        message: "Tuple types require at least 2 item types".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let items = items
                    .iter()
                    .map(|item| self.resolve_ast_ty_in_context(item, TypeSyntaxContext::General))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Ty::Tuple(items))
            }
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|p| {
                        self.resolve_ast_ty_in_context(
                            p,
                            match context {
                                TypeSyntaxContext::BindingAnnotation
                                | TypeSyntaxContext::FunctionReturn
                                | TypeSyntaxContext::HoleClosureParam => {
                                    TypeSyntaxContext::HoleClosureParam
                                }
                                _ => TypeSyntaxContext::General,
                            },
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_ast_ty_in_context(ret, context)?;
                Ok(Ty::Func(params, Box::new(ret)))
            }
            AstTy::ImplTrait(span, name) => Err(TypeError {
                message: format!(
                    "`impl {}` is only supported in function and extractor parameters",
                    name
                ),
                span: span.clone(),
                hint: Some("Name the type parameter explicitly, e.g. `<$N: Trait>`.".into()),
            }),
        }
    }

    pub(super) fn resolve_builtin_ast_ty(
        &mut self,
        ast_ty: &AstTy,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_builtin_ast_ty_in_context(ast_ty, TypeSyntaxContext::General, tyvars)
    }

    pub(super) fn resolve_signature_ast_ty_in_context(
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_signature_like_ast_ty_in_context(
            ast_ty,
            context,
            tyvars,
            SignatureTyMode::Normal,
        )
    }

    pub(super) fn seed_signature_type_params(
        &mut self,
        type_params: &[ResolvedTypeParam],
        tyvars: &mut HashMap<String, Ty>,
    ) {
        for param in type_params {
            let fresh = self.env.fresh_tyvar();
            if let Ty::Var(var) = fresh {
                if let Some(bound) = &param.bound {
                    self.register_tyvar_bound(var, bound);
                }
            }
            tyvars.insert(param.name.clone(), fresh);
        }
    }

    pub(super) fn resolve_trait_signature_ast_ty_in_context(
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        self_ty: &Ty,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_signature_like_ast_ty_in_context(
            ast_ty,
            context,
            tyvars,
            SignatureTyMode::Trait { self_ty },
        )
    }

    pub(super) fn resolve_builtin_ast_ty_in_context(
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_signature_like_ast_ty_in_context(
            ast_ty,
            context,
            tyvars,
            SignatureTyMode::Builtin,
        )
    }

    fn signature_like_param_context(context: TypeSyntaxContext) -> TypeSyntaxContext {
        match context {
            TypeSyntaxContext::BindingAnnotation
            | TypeSyntaxContext::FunctionReturn
            | TypeSyntaxContext::HoleClosureParam => TypeSyntaxContext::HoleClosureParam,
            _ => TypeSyntaxContext::General,
        }
    }

    fn resolve_signature_like_ast_ty_in_context(
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
        mode: SignatureTyMode<'_>,
    ) -> Result<Ty, TypeError> {
        match ast_ty {
            AstTy::Named(_, name) if name == "Self" => {
                if let Some(self_ty) = mode.self_ty() {
                    Ok(self_ty.clone())
                } else {
                    self.resolve_ast_ty_in_context(ast_ty, context)
                }
            }
            AstTy::Named(_, name) if name.starts_with('$') => {
                if context == TypeSyntaxContext::ErrorMarker {
                    return Err(TypeError {
                        message:
                            "The error marker E in Result<T, E> must be a deferror-defined type."
                                .into(),
                        span: Self::ast_ty_span(ast_ty).clone(),
                        hint: None,
                    });
                }
                if let Some(existing) = tyvars.get(name) {
                    return Ok(existing.clone());
                }
                let fresh = self.env.fresh_tyvar();
                tyvars.insert(name.clone(), fresh.clone());
                Ok(fresh)
            }
            AstTy::Named(span, name) if matches!(Self::surface_name(name), "_" | "Hole") => {
                self.resolve_hole_surface_ty(span, context)
            }
            AstTy::Named(span, name)
                if matches!(
                    Self::surface_name(name),
                    "MatchArms" | "CondClauses" | "BulkUpdateEntries"
                ) =>
            {
                Err(self.clause_block_type_not_allowed_error(span, Self::surface_name(name)))
            }
            AstTy::Named(span, name) if Self::surface_name(name) == "Seq" => {
                Err(self.seq_not_allowed_error(span))
            }
            AstTy::ImplTrait(_, trait_name) => {
                if context == TypeSyntaxContext::ErrorMarker
                    && mode.rejects_impl_trait_in_error_marker()
                {
                    return Err(TypeError {
                        message:
                            "The error marker E in Result<T, E> must be a deferror-defined type."
                                .into(),
                        span: Self::ast_ty_span(ast_ty).clone(),
                        hint: None,
                    });
                }
                if matches!(mode, SignatureTyMode::Builtin) {
                    return self.resolve_ast_ty_in_context(ast_ty, context);
                }
                let fresh = self.env.fresh_tyvar();
                if let Ty::Var(var) = fresh {
                    self.register_tyvar_bound(var, trait_name);
                }
                Ok(fresh)
            }
            AstTy::Generic(span, name, _) if Self::surface_name(name) == "TypeRef" => {
                Err(self.type_ref_not_allowed_error(span))
            }
            AstTy::Generic(span, name, _)
                if matches!(
                    Self::surface_name(name),
                    "MatchArms" | "CondClauses" | "BulkUpdateEntries"
                ) =>
            {
                Err(self.clause_block_type_not_allowed_error(span, Self::surface_name(name)))
            }
            AstTy::Generic(span, name, args)
                if Self::surface_name(name) == "Lazy" && mode.allows_lazy() =>
            {
                let args = self.require_type_arg_count(
                    span,
                    args,
                    1,
                    "Lazy<T> requires exactly 1 type argument",
                )?;
                let inner = self.resolve_signature_like_ast_ty_in_context(
                    &args[0],
                    TypeSyntaxContext::General,
                    tyvars,
                    mode,
                )?;
                Ok(Ty::Lazy(Box::new(inner)))
            }
            AstTy::Generic(span, name, _) if Self::surface_name(name) == "Seq" => {
                Err(self.seq_not_allowed_error(span))
            }
            AstTy::Generic(span, name, args) => match Self::surface_name(name) {
                "MatchResult" => {
                    if !self.match_result_type_allowed(context) {
                        return Err(self.match_result_not_allowed_error(span));
                    }
                    if args.is_empty() || args.len() > 2 {
                        return Err(TypeError {
                            message: "MatchResult<$Value> or MatchResult<$Value, Error> requires 1 or 2 type arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let value = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    if args.len() == 2 {
                        let err = self.resolve_signature_like_ast_ty_in_context(
                            &args[1],
                            TypeSyntaxContext::General,
                            tyvars,
                            mode,
                        )?;
                        if !matches!(err, Ty::Error) {
                            return Err(TypeError {
                                message: "MatchResult<$Value, Error> requires Error as the second argument".into(),
                                span: span.clone(),
                                hint: None,
                            });
                        }
                    }
                    Ok(Ty::Enum("MatchResult".into(), vec![value]))
                }
                "List" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "List<T> requires exactly 1 type argument",
                    )?;
                    let inner_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    Ok(Ty::List(Box::new(inner_ty)))
                }
                "HashMap" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "HashMap<V> requires exactly 1 type argument",
                    )?;
                    let value_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    Ok(Ty::Enum("HashMap".into(), vec![value_ty]))
                }
                "Generator" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        2,
                        "Generator<State, Item> requires exactly 2 type arguments",
                    )?;
                    let state_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    let item_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[1],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    Ok(Ty::Enum("Generator".into(), vec![state_ty, item_ty]))
                }
                "ProcessInit" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "ProcessInit<T> requires exactly 1 type argument",
                    )?;
                    let inner_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    Ok(Ty::Enum("ProcessInit".into(), vec![inner_ty]))
                }
                "Facet" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        2,
                        "Facet<S, A> requires exactly 2 type arguments",
                    )?;
                    let source = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    let focus = self.resolve_signature_like_ast_ty_in_context(
                        &args[1],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    Ok(Ty::Facet(Box::new(source), Box::new(focus)))
                }
                "PID" => self.resolve_pid_surface_ty(span, args),
                "Workers" => self.resolve_worker_handle_surface_ty(span, args, "Workers"),
                "WorkerLease" => self.resolve_worker_handle_surface_ty(span, args, "WorkerLease"),
                "TaskHandle" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "TaskHandle<T> requires exactly 1 type argument",
                    )?;
                    let inner = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    Ok(Ty::Enum("TaskHandle".into(), vec![inner]))
                }
                "Result" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(TypeError {
                            message: "Result<T> or Result<T, E> requires 1 or 2 type arguments"
                                .into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let ok = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    let err = if args.len() == 2 {
                        if context != TypeSyntaxContext::FunctionReturn {
                            return Err(TypeError {
                                message:
                                    "Result<T, E> is only allowed in function return signatures."
                                        .into(),
                                span: span.clone(),
                                hint: Some("Use Result<T> in local code.".into()),
                            });
                        }
                        self.resolve_signature_like_ast_ty_in_context(
                            &args[1],
                            TypeSyntaxContext::ErrorMarker,
                            tyvars,
                            mode,
                        )?
                    } else {
                        Ty::Error
                    };
                    Ok(Ty::Result(Box::new(ok), Box::new(err)))
                }
                _ if mode.allows_user_generic_fallback() => {
                    let def = self
                        .env
                        .lookup_type_def(name)
                        .cloned()
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown generic type: {}", name),
                            span: span.clone(),
                            hint: None,
                        })?;
                    if def.type_params.len() != args.len() {
                        return Err(TypeError {
                            message: format!(
                                "Type {} requires {} type argument(s), got {}",
                                name,
                                def.type_params.len(),
                                args.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let resolved_args = args
                        .iter()
                        .map(|arg| {
                            self.resolve_signature_like_ast_ty_in_context(
                                arg,
                                TypeSyntaxContext::General,
                                tyvars,
                                mode,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    match def.kind {
                        crate::env::TypeKind::Struct => Ok(Ty::Struct(
                            def.name.clone(),
                            self.instantiate_type_def_fields(&def, &resolved_args),
                        )),
                        crate::env::TypeKind::Enum => Ok(Ty::Enum(def.name.clone(), resolved_args)),
                        _ => Err(TypeError {
                            message: format!(
                                "Generic type {} is not supported in this context",
                                name
                            ),
                            span: span.clone(),
                            hint: None,
                        }),
                    }
                }
                _ => self.resolve_ast_ty_in_context(ast_ty, context),
            },
            AstTy::Tuple(span, items) => {
                if items.len() < 2 {
                    return Err(TypeError {
                        message: "Tuple types require at least 2 item types".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let items = items
                    .iter()
                    .map(|item| {
                        self.resolve_signature_like_ast_ty_in_context(
                            item,
                            TypeSyntaxContext::General,
                            tyvars,
                            mode,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Ty::Tuple(items))
            }
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|p| {
                        self.resolve_signature_like_ast_ty_in_context(
                            p,
                            Self::signature_like_param_context(context),
                            tyvars,
                            mode,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let ret =
                    self.resolve_signature_like_ast_ty_in_context(ret, context, tyvars, mode)?;
                Ok(Ty::Func(params, Box::new(ret)))
            }
            _ => self.resolve_ast_ty_in_context(ast_ty, context),
        }
    }

    pub(super) fn resolve_error_marker_type(&self, ast_ty: &AstTy) -> Result<Ty, TypeError> {
        let span = Self::ast_ty_span(ast_ty).clone();
        let AstTy::Named(_, name) = ast_ty else {
            return Err(TypeError {
                message: "The error marker E in Result<T, E> must be a deferror-defined type."
                    .into(),
                span,
                hint: None,
            });
        };

        if Self::surface_name(name) == "Error" {
            return Ok(Ty::Error);
        }

        let def = self.env.lookup_type_def(name).ok_or_else(|| TypeError {
            message: "The error marker E in Result<T, E> must be a deferror-defined type.".into(),
            span: span.clone(),
            hint: None,
        });

        if let Ok(def) = def {
            if def.kind != crate::env::TypeKind::ConcreteError {
                return Err(TypeError {
                    message: "The error marker E in Result<T, E> must be a deferror-defined type."
                        .into(),
                    span,
                    hint: None,
                });
            }
            return Ok(Ty::Error);
        }

        if !self.env.is_declared_error_type_name(name) {
            return Err(TypeError {
                message: "The error marker E in Result<T, E> must be a deferror-defined type."
                    .into(),
                span,
                hint: None,
            });
        }

        Ok(Ty::Error)
    }

    pub(super) fn types_compatible(&mut self, expected: &Ty, got: &Ty) -> bool {
        let profile = self.profiler.start();
        let expected = self.resolve_ty(expected);
        let got = self.resolve_ty(got);
        let result = match (&expected, &got) {
            (Ty::Hole, Ty::Hole) => true,
            (Ty::Var(var), ty) | (ty, Ty::Var(var)) => self.bind_tyvar(*var, ty),
            (Ty::Int, Ty::Int)
            | (Ty::Float, Ty::Float)
            | (Ty::Str, Ty::Str)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Unit, Ty::Unit)
            | (Ty::Error, Ty::Error) => true,
            (Ty::List(a), Ty::List(b)) => self.types_compatible(a, b),
            (Ty::TypeRef(a), Ty::TypeRef(b)) | (Ty::Lazy(a), Ty::Lazy(b)) => {
                self.types_compatible(a, b)
            }
            (Ty::Pid(a), Ty::Pid(b)) => {
                Self::canonical_user_type_name(a) == Self::canonical_user_type_name(b)
                    || a.starts_with('$')
                    || b.starts_with('$')
            }
            (Ty::Pid(expected_process), Ty::Enum(name, args))
                if name == "WorkerLease" && args.len() == 1 =>
            {
                match args.first() {
                    Some(Ty::Pid(actual_process)) => {
                        Self::canonical_user_type_name(expected_process)
                            == Self::canonical_user_type_name(actual_process)
                            || expected_process.starts_with('$')
                            || actual_process.starts_with('$')
                    }
                    _ => false,
                }
            }
            (Ty::Facet(src_a, focus_a), Ty::Facet(src_b, focus_b)) => {
                self.types_compatible(src_a, src_b) && self.types_compatible(focus_a, focus_b)
            }
            (Ty::Tuple(a), Ty::Tuple(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|(left, right)| self.types_compatible(left, right))
            }
            (Ty::Func(a_params, a_ret), Ty::Func(b_params, b_ret)) => {
                a_params.len() == b_params.len()
                    && a_params
                        .iter()
                        .zip(b_params.iter())
                        .all(|(a, b)| self.types_compatible(a, b))
                    && self.types_compatible(a_ret, b_ret)
            }
            (Ty::Result(ok1, err1), Ty::Result(ok2, err2)) => {
                self.types_compatible(ok1, ok2) && self.types_compatible(err1, err2)
            }
            (Ty::Struct(n1, _), Ty::Struct(n2, _)) => {
                Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
            }
            (Ty::Record(n1, _), Ty::Record(n2, _)) => {
                Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
            }
            (Ty::Enum(n1, args1), Ty::Enum(n2, args2)) => {
                Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
                    && args1.len() == args2.len()
                    && args1
                        .iter()
                        .zip(args2.iter())
                        .all(|(left, right)| self.types_compatible(left, right))
            }
            _ => false,
        };
        self.profiler.finish(ProfileEvent::TypesCompatible, profile);
        result
    }

    pub(super) fn bind_tyvar(&mut self, var: u32, ty: &Ty) -> bool {
        let profile = self.profiler.start();
        let ty = self.resolve_ty(ty);
        let result = if ty == Ty::Var(var) {
            true
        } else if self.ty_contains_var(&ty, var) {
            false
        } else {
            let var_bounds = self.tyvar_bound_names(var);
            match &ty {
                Ty::Var(other) => {
                    let mut combined = var_bounds;
                    for bound in self.tyvar_bound_names(*other) {
                        if !combined.iter().any(|existing| existing == &bound) {
                            combined.push(bound);
                        }
                    }
                    combined.sort();
                    self.tyvar_bounds.insert(var, combined.clone());
                    self.tyvar_bounds.insert(*other, combined);
                }
                _ => {
                    if !self.ty_satisfies_bounds(&ty, &var_bounds) {
                        self.profiler.finish(ProfileEvent::BindTyVar, profile);
                        return false;
                    }
                }
            }
            self.substitutions.insert(var, ty);
            true
        };
        self.profiler.finish(ProfileEvent::BindTyVar, profile);
        result
    }

    pub(super) fn ty_satisfies_bounds(&mut self, ty: &Ty, bounds: &[String]) -> bool {
        if bounds.is_empty() {
            return true;
        }

        match self.resolve_ty(ty) {
            Ty::Var(var) => bounds.iter().all(|bound| self.tyvar_has_bound(var, bound)),
            concrete => bounds
                .iter()
                .all(|bound| self.trait_impl_exists(bound, &concrete)),
        }
    }

    pub(super) fn ty_contains_var(&self, ty: &Ty, needle: u32) -> bool {
        match self.resolve_ty(ty) {
            Ty::Var(var) => var == needle,
            Ty::Hole => false,
            Ty::List(inner) => self.ty_contains_var(&inner, needle),
            Ty::TypeRef(inner) | Ty::Lazy(inner) => self.ty_contains_var(&inner, needle),
            Ty::Pid(_) => false,
            Ty::Facet(source, focus) => {
                self.ty_contains_var(&source, needle) || self.ty_contains_var(&focus, needle)
            }
            Ty::Tuple(items) => items.iter().any(|item| self.ty_contains_var(item, needle)),
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .any(|param| self.ty_contains_var(param, needle))
                    || self.ty_contains_var(&ret, needle)
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.ty_contains_var(param, needle))
                    || self.ty_contains_var(&ret, needle)
            }
            Ty::Result(ok, err) => {
                self.ty_contains_var(&ok, needle) || self.ty_contains_var(&err, needle)
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
                .iter()
                .any(|(_, field_ty)| self.ty_contains_var(field_ty, needle)),
            Ty::Enum(_, args) => args.iter().any(|arg| self.ty_contains_var(arg, needle)),
            _ => false,
        }
    }

    pub(super) fn resolve_ty(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(var) => match self.substitutions.get(var) {
                Some(bound) => self.resolve_ty(bound),
                None => Ty::Var(*var),
            },
            Ty::List(inner) => Ty::List(Box::new(self.resolve_ty(inner))),
            Ty::Hole => Ty::Hole,
            Ty::TypeRef(inner) => Ty::TypeRef(Box::new(self.resolve_ty(inner))),
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.resolve_ty(inner))),
            Ty::Pid(name) => Ty::Pid(name.clone()),
            Ty::Facet(source, focus) => Ty::Facet(
                Box::new(self.resolve_ty(source)),
                Box::new(self.resolve_ty(focus)),
            ),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|item| self.resolve_ty(item)).collect()),
            Ty::Func(params, ret) => Ty::Func(
                params.iter().map(|param| self.resolve_ty(param)).collect(),
                Box::new(self.resolve_ty(ret)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params.iter().map(|param| self.resolve_ty(param)).collect(),
                ret: Box::new(self.resolve_ty(ret)),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                params: params.iter().map(|param| self.resolve_ty(param)).collect(),
                ret: Box::new(self.resolve_ty(ret)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| (field.clone(), self.resolve_ty(field_ty)))
                    .collect(),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| (field.clone(), self.resolve_ty(field_ty)))
                    .collect(),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter().map(|arg| self.resolve_ty(arg)).collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.resolve_ty(ok)),
                Box::new(self.resolve_ty(err)),
            ),
            other => other.clone(),
        }
    }

    pub(super) fn instantiate_ty_with_fresh(
        &mut self,
        ty: &Ty,
        fresh: &mut HashMap<u32, Ty>,
    ) -> Ty {
        let profile = self.profiler.start();
        let result = match ty {
            Ty::Var(var) => fresh
                .entry(*var)
                .or_insert_with(|| {
                    let fresh = self.env.fresh_tyvar();
                    if let Ty::Var(new_var) = fresh {
                        let bounds = self.tyvar_bound_names(*var);
                        self.register_tyvar_bounds(new_var, &bounds);
                    }
                    fresh
                })
                .clone(),
            Ty::List(inner) => Ty::List(Box::new(self.instantiate_ty_with_fresh(inner, fresh))),
            Ty::Hole => Ty::Hole,
            Ty::TypeRef(inner) => {
                Ty::TypeRef(Box::new(self.instantiate_ty_with_fresh(inner, fresh)))
            }
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.instantiate_ty_with_fresh(inner, fresh))),
            Ty::Pid(name) => Ty::Pid(name.clone()),
            Ty::Facet(source, focus) => Ty::Facet(
                Box::new(self.instantiate_ty_with_fresh(source, fresh)),
                Box::new(self.instantiate_ty_with_fresh(focus, fresh)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.instantiate_ty_with_fresh(item, fresh))
                    .collect(),
            ),
            Ty::Func(params, ret) => Ty::Func(
                params
                    .iter()
                    .map(|param| self.instantiate_ty_with_fresh(param, fresh))
                    .collect(),
                Box::new(self.instantiate_ty_with_fresh(ret, fresh)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|param| self.instantiate_ty_with_fresh(param, fresh))
                    .collect(),
                ret: Box::new(self.instantiate_ty_with_fresh(ret, fresh)),
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
                    .map(|param| self.instantiate_ty_with_fresh(param, fresh))
                    .collect(),
                ret: Box::new(self.instantiate_ty_with_fresh(ret, fresh)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| {
                        (
                            field.clone(),
                            self.instantiate_ty_with_fresh(field_ty, fresh),
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
                            self.instantiate_ty_with_fresh(field_ty, fresh),
                        )
                    })
                    .collect(),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter()
                    .map(|arg| self.instantiate_ty_with_fresh(arg, fresh))
                    .collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.instantiate_ty_with_fresh(ok, fresh)),
                Box::new(self.instantiate_ty_with_fresh(err, fresh)),
            ),
            other => other.clone(),
        };
        self.profiler
            .finish(ProfileEvent::InstantiateTyWithFresh, profile);
        result
    }

    pub(super) fn instantiate_builtin_ty(&mut self, ty: &Ty) -> Ty {
        let mut fresh = HashMap::new();
        self.instantiate_ty_with_fresh(ty, &mut fresh)
    }

    fn substitute_type_def_ty(&self, ty: &Ty, bindings: &HashMap<u32, Ty>) -> Ty {
        match ty {
            Ty::Var(var) => bindings.get(var).cloned().unwrap_or(Ty::Var(*var)),
            Ty::List(inner) => Ty::List(Box::new(self.substitute_type_def_ty(inner, bindings))),
            Ty::Hole => Ty::Hole,
            Ty::TypeRef(inner) => {
                Ty::TypeRef(Box::new(self.substitute_type_def_ty(inner, bindings)))
            }
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.substitute_type_def_ty(inner, bindings))),
            Ty::Pid(name) => Ty::Pid(name.clone()),
            Ty::Facet(source, focus) => Ty::Facet(
                Box::new(self.substitute_type_def_ty(source, bindings)),
                Box::new(self.substitute_type_def_ty(focus, bindings)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.substitute_type_def_ty(item, bindings))
                    .collect(),
            ),
            Ty::Func(params, ret) => Ty::Func(
                params
                    .iter()
                    .map(|param| self.substitute_type_def_ty(param, bindings))
                    .collect(),
                Box::new(self.substitute_type_def_ty(ret, bindings)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|param| self.substitute_type_def_ty(param, bindings))
                    .collect(),
                ret: Box::new(self.substitute_type_def_ty(ret, bindings)),
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
                    .map(|param| self.substitute_type_def_ty(param, bindings))
                    .collect(),
                ret: Box::new(self.substitute_type_def_ty(ret, bindings)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| {
                        (
                            field.clone(),
                            self.substitute_type_def_ty(field_ty, bindings),
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
                            self.substitute_type_def_ty(field_ty, bindings),
                        )
                    })
                    .collect(),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter()
                    .map(|arg| self.substitute_type_def_ty(arg, bindings))
                    .collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.substitute_type_def_ty(ok, bindings)),
                Box::new(self.substitute_type_def_ty(err, bindings)),
            ),
            other => other.clone(),
        }
    }

    pub(super) fn instantiate_type_def_fields(
        &self,
        def: &crate::env::TypeDefInfo,
        args: &[Ty],
    ) -> Vec<(String, Ty)> {
        let bindings = def
            .type_param_vars
            .iter()
            .copied()
            .zip(args.iter().cloned())
            .collect::<HashMap<_, _>>();
        def.fields
            .iter()
            .map(|(field, field_ty)| {
                (
                    field.clone(),
                    self.substitute_type_def_ty(field_ty, &bindings),
                )
            })
            .collect()
    }

    pub(super) fn instantiate_enum_variant(
        &mut self,
        variant: &crate::env::EnumVariantInfo,
    ) -> crate::env::EnumVariantInfo {
        let profile = self.profiler.start();
        let mut fresh = HashMap::new();
        let instantiated = crate::env::EnumVariantInfo {
            constructor_name: variant.constructor_name.clone(),
            short_name: variant.short_name.clone(),
            enum_name: variant.enum_name.clone(),
            enum_ty: self.instantiate_ty_with_fresh(&variant.enum_ty, &mut fresh),
            tag: variant.tag,
            payload: variant
                .payload
                .iter()
                .map(|ty| self.instantiate_ty_with_fresh(ty, &mut fresh))
                .collect(),
            discriminant: variant.discriminant.clone(),
        };
        self.profiler
            .finish(ProfileEvent::InstantiateEnumVariant, profile);
        instantiated
    }

    fn diagnostic_tyvar_name(index: usize) -> String {
        let mut value = index;
        let mut name = String::new();
        loop {
            let rem = value % 26;
            name.push((b'A' + rem as u8) as char);
            if value < 26 {
                break;
            }
            value = (value / 26) - 1;
        }
        format!("${}", name.chars().rev().collect::<String>())
    }

    fn diagnostic_ty_name_with_state(
        &self,
        ty: &Ty,
        tyvars: &mut HashMap<u32, String>,
        next_tyvar_index: &mut usize,
    ) -> String {
        match ty {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Boolean".into(),
            Ty::Unit => "Unit".into(),
            Ty::Error => "Error".into(),
            Ty::Hole => "_".into(),
            Ty::List(inner) => format!(
                "List<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::Lazy(inner) => format!(
                "Lazy<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::TypeRef(inner) => format!(
                "TypeRef<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::Pid(name) => format!("PID<{}>", Self::surface_name(name)),
            Ty::Facet(source, focus) => format!(
                "Facet<{}, {}>",
                self.diagnostic_ty_name_with_state(source, tyvars, next_tyvar_index),
                self.diagnostic_ty_name_with_state(focus, tyvars, next_tyvar_index)
            ),
            Ty::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|item| {
                        self.diagnostic_ty_name_with_state(item, tyvars, next_tyvar_index)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Result(ok, _) => format!(
                "Result<{}>",
                self.diagnostic_ty_name_with_state(ok, tyvars, next_tyvar_index)
            ),
            Ty::Var(var) => tyvars
                .entry(*var)
                .or_insert_with(|| {
                    let name = Self::diagnostic_tyvar_name(*next_tyvar_index);
                    *next_tyvar_index += 1;
                    name
                })
                .clone(),
            Ty::Struct(name, _) | Ty::Record(name, _) => Self::surface_name(name).to_string(),
            Ty::Enum(name, args) => {
                if args.is_empty() {
                    Self::surface_name(name).to_string()
                } else {
                    format!(
                        "{}<{}>",
                        Self::surface_name(name),
                        args.iter()
                            .map(|arg| {
                                self.diagnostic_ty_name_with_state(arg, tyvars, next_tyvar_index)
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Ty::Func(params, ret) => {
                let param_str = params
                    .iter()
                    .map(|ty| self.diagnostic_ty_name_with_state(ty, tyvars, next_tyvar_index))
                    .collect::<Vec<_>>()
                    .join(", ");
                if param_str.is_empty() {
                    format!(
                        "(-> {})",
                        self.diagnostic_ty_name_with_state(ret, tyvars, next_tyvar_index)
                    )
                } else {
                    format!(
                        "({} -> {})",
                        param_str,
                        self.diagnostic_ty_name_with_state(ret, tyvars, next_tyvar_index)
                    )
                }
            }
            Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
            Ty::UserFunc { .. } => "UserFunc".into(),
        }
    }

    pub(super) fn diagnostic_ty_names(&self, tys: &[&Ty]) -> Vec<String> {
        let mut tyvars = HashMap::new();
        let mut next_tyvar_index = 0usize;
        tys.iter()
            .map(|ty| self.diagnostic_ty_name_with_state(ty, &mut tyvars, &mut next_tyvar_index))
            .collect()
    }

    pub(super) fn diagnostic_ty_name(&self, ty: &Ty) -> String {
        self.diagnostic_ty_names(&[ty])
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    pub(super) fn ty_name(&self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Boolean".into(),
            Ty::Unit => "Unit".into(),
            Ty::Error => "Error".into(),
            Ty::Hole => "_".into(),
            Ty::List(inner) => format!("List<{}>", self.ty_name(inner)),
            Ty::Lazy(inner) => format!("Lazy<{}>", self.ty_name(inner)),
            Ty::TypeRef(inner) => format!("TypeRef<{}>", self.ty_name(inner)),
            Ty::Pid(name) => format!("PID<{}>", Self::surface_name(name)),
            Ty::Facet(source, focus) => {
                format!("Facet<{}, {}>", self.ty_name(source), self.ty_name(focus))
            }
            Ty::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|item| self.ty_name(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Result(ok, _) => format!("Result<{}>", self.ty_name(ok)),
            Ty::Var(n) => format!("${}", n),
            Ty::Struct(name, _) | Ty::Record(name, _) => Self::surface_name(name).to_string(),
            Ty::Enum(name, args) => {
                if args.is_empty() {
                    Self::surface_name(name).to_string()
                } else {
                    format!(
                        "{}<{}>",
                        Self::surface_name(name),
                        args.iter()
                            .map(|arg| self.ty_name(arg))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Ty::Func(params, ret) => {
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
            Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
            Ty::UserFunc { .. } => "UserFunc".into(),
        }
    }

    pub(super) fn ty_contains_facet(&self, ty: &Ty) -> bool {
        match self.resolve_ty(ty) {
            Ty::Facet(_, _) => true,
            Ty::List(inner) | Ty::TypeRef(inner) | Ty::Lazy(inner) => {
                self.ty_contains_facet(inner.as_ref())
            }
            Ty::Tuple(items) => items.iter().any(|item| self.ty_contains_facet(item)),
            Ty::Func(params, ret) => {
                params.iter().any(|param| self.ty_contains_facet(param))
                    || self.ty_contains_facet(ret.as_ref())
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params.iter().any(|param| self.ty_contains_facet(param))
                    || self.ty_contains_facet(ret.as_ref())
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
                .iter()
                .any(|(_, field_ty)| self.ty_contains_facet(field_ty)),
            Ty::Enum(_, args) => args.iter().any(|arg| self.ty_contains_facet(arg)),
            Ty::Result(ok, err) => {
                self.ty_contains_facet(ok.as_ref()) || self.ty_contains_facet(err.as_ref())
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Var(_)
            | Ty::Error
            | Ty::Hole
            | Ty::Pid(_) => false,
        }
    }

    fn resolve_typed_facet_path(&self, path: TypedFacetPath) -> TypedFacetPath {
        TypedFacetPath {
            source_ty: self.resolve_ty(&path.source_ty),
            focus_ty: self.resolve_ty(&path.focus_ty),
            path_kind: path.path_kind,
            may_fail: path.may_fail,
            source_readonly_root: path.source_readonly_root,
            segments: path.segments,
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

    pub(super) fn resolve_typed_node(&self, node: TypedNode) -> TypedNode {
        let span = node.span.clone();
        let ty = self.resolve_ty(&node.ty);
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
                init: Box::new(self.resolve_typed_node(*init)),
            },
            TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid,
            } => TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: Box::new(self.resolve_typed_node(*pid)),
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
                init: Box::new(self.resolve_typed_node(*init)),
                strategy: Box::new(self.resolve_typed_node(*strategy)),
            },
            TypedInner::App(func, args) => TypedInner::App(
                Box::new(self.resolve_typed_node(*func)),
                args.into_iter()
                    .map(|arg| self.resolve_typed_node(arg))
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
                receiver_ty: self.resolve_ty(&receiver_ty),
                dispatch,
                origin: self.resolve_trait_call_origin(origin),
                args: args
                    .into_iter()
                    .map(|arg| self.resolve_typed_node(arg))
                    .collect(),
            },
            TypedInner::InjectCall(func, args) => TypedInner::InjectCall(
                Box::new(self.resolve_typed_node(*func)),
                args.into_iter()
                    .map(|arg| self.resolve_typed_node(arg))
                    .collect(),
            ),
            TypedInner::Block(stmts) => TypedInner::Block(
                stmts
                    .into_iter()
                    .map(|stmt| self.resolve_typed_node(stmt))
                    .collect(),
            ),
            TypedInner::Bind(pattern, rhs) => TypedInner::Bind(
                self.resolve_typed_pattern(pattern),
                Box::new(self.resolve_typed_node(*rhs)),
            ),
            TypedInner::SafeBind(pattern, rhs) => TypedInner::SafeBind(
                self.resolve_typed_pattern(pattern),
                Box::new(self.resolve_typed_node(*rhs)),
            ),
            TypedInner::BinOp(op, left, right) => TypedInner::BinOp(
                op,
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::Pipe(left, right) => TypedInner::Pipe(
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::Compose(flavor, left, right) => TypedInner::Compose(
                flavor,
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::ListNil => TypedInner::ListNil,
            TypedInner::ListCons(head, tail) => TypedInner::ListCons(
                Box::new(self.resolve_typed_node(*head)),
                Box::new(self.resolve_typed_node(*tail)),
            ),
            TypedInner::ListLiteral(elems) => TypedInner::ListLiteral(
                elems
                    .into_iter()
                    .map(|elem| self.resolve_typed_node(elem))
                    .collect(),
            ),
            TypedInner::HashMapLiteral(entries) => TypedInner::HashMapLiteral(
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        (self.resolve_typed_node(key), self.resolve_typed_node(value))
                    })
                    .collect(),
            ),
            TypedInner::TupleLiteral(elems) => TypedInner::TupleLiteral(
                elems
                    .into_iter()
                    .map(|elem| self.resolve_typed_node(elem))
                    .collect(),
            ),
            TypedInner::InterpolatedStr(parts) => TypedInner::InterpolatedStr(
                parts
                    .into_iter()
                    .map(|part| match part {
                        TypedInterpolatedPart::Text(text) => TypedInterpolatedPart::Text(text),
                        TypedInterpolatedPart::Expr(expr) => {
                            TypedInterpolatedPart::Expr(Box::new(self.resolve_typed_node(*expr)))
                        }
                    })
                    .collect(),
            ),
            TypedInner::Dbg(args) => TypedInner::Dbg(
                args.into_iter()
                    .map(|arg| TypedDbgArg {
                        span: arg.span,
                        ty_name: arg.ty_name,
                        expr: self.resolve_typed_node(arg.expr),
                    })
                    .collect(),
            ),
            TypedInner::If(cond, then, else_opt) => TypedInner::If(
                Box::new(self.resolve_typed_node(*cond)),
                Box::new(self.resolve_typed_node(*then)),
                else_opt.map(|node| Box::new(self.resolve_typed_node(*node))),
            ),
            TypedInner::Assert(cond, err) => TypedInner::Assert(
                Box::new(self.resolve_typed_node(*cond)),
                Box::new(self.resolve_typed_node(*err)),
            ),
            TypedInner::Ensure(value, pred, err) => TypedInner::Ensure(
                Box::new(self.resolve_typed_node(*value)),
                Box::new(self.resolve_typed_node(*pred)),
                Box::new(self.resolve_typed_node(*err)),
            ),
            TypedInner::MapErr(value, err) => TypedInner::MapErr(
                Box::new(self.resolve_typed_node(*value)),
                Box::new(self.resolve_typed_node(*err)),
            ),
            TypedInner::Cause(value, err) => TypedInner::Cause(
                Box::new(self.resolve_typed_node(*value)),
                Box::new(self.resolve_typed_node(*err)),
            ),
            TypedInner::RecoverKind(value, marker, handler) => TypedInner::RecoverKind(
                Box::new(self.resolve_typed_node(*value)),
                Box::new(self.resolve_typed_node(*marker)),
                Box::new(self.resolve_typed_node(*handler)),
            ),
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                Box::new(self.resolve_typed_node(*scrutinee)),
                arms.into_iter()
                    .map(|arm| TypedMatchArm {
                        pattern: self.resolve_typed_match_pattern(arm.pattern),
                        guard: arm.guard.map(|guard| self.resolve_typed_node(guard)),
                        body: self.resolve_typed_node(arm.body),
                    })
                    .collect(),
            ),
            TypedInner::FieldAccess(expr, idx) => {
                TypedInner::FieldAccess(Box::new(self.resolve_typed_node(*expr)), idx)
            }
            TypedInner::ProcessContextHandler { process_name, slot } => {
                TypedInner::ProcessContextHandler { process_name, slot }
            }
            TypedInner::FacetPath(path) => {
                TypedInner::FacetPath(self.resolve_typed_facet_path(path))
            }
            TypedInner::PendingFacetPath(path) => TypedInner::PendingFacetPath(PendingFacetPath {
                root_path_name: path.root_path_name,
                source_ty_hint: path.source_ty_hint.map(|ty| self.resolve_ty(&ty)),
                segments: path.segments,
            }),
            TypedInner::FacetView {
                source,
                path,
                source_is_result,
            } => TypedInner::FacetView {
                source: Box::new(self.resolve_typed_node(*source)),
                path: self.resolve_typed_facet_path(path),
                source_is_result,
            },
            TypedInner::FacetSet {
                source,
                path,
                value,
                source_is_result,
                mode,
            } => TypedInner::FacetSet {
                source: Box::new(self.resolve_typed_node(*source)),
                path: self.resolve_typed_facet_path(path),
                value: Box::new(self.resolve_typed_node(*value)),
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
                source: Box::new(self.resolve_typed_node(*source)),
                path: self.resolve_typed_facet_path(path),
                update_fun: Box::new(self.resolve_typed_node(*update_fun)),
                source_is_result,
                mode,
            },
            TypedInner::StructLit(tag, fields) => TypedInner::StructLit(
                tag,
                fields
                    .into_iter()
                    .map(|field| self.resolve_typed_node(field))
                    .collect(),
            ),
            TypedInner::ConstructorCall(tag, fields) => TypedInner::ConstructorCall(
                tag,
                fields
                    .into_iter()
                    .map(|field| self.resolve_typed_node(field))
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
                        ty: self.resolve_ty(&param.ty),
                    })
                    .collect(),
                Box::new(self.resolve_typed_node(*show)),
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
                            ty: self.resolve_ty(&param.ty),
                        })
                        .collect(),
                    self.resolve_ty(&ret_ty),
                    Box::new(self.resolve_typed_node(*body)),
                    visibility,
                )
            }
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
                    TypedFunParam {
                        id: param.id,
                        ty: self.resolve_ty(&param.ty),
                    },
                    self.resolve_ty(&ret_ty),
                    Box::new(self.resolve_typed_node(*body)),
                    visibility,
                )
            }
            TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty) => {
                TypedInner::BuiltinExtractorDecl(
                    id,
                    self.resolve_ty(&param_ty),
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
                Box::new(self.resolve_typed_node(*body)),
            ),
            TypedInner::Capture(target, args) => TypedInner::Capture(
                Box::new(self.resolve_typed_node(*target)),
                args.into_iter()
                    .map(|arg| self.resolve_typed_node(arg))
                    .collect(),
            ),
            TypedInner::StructDef(tag, name, field_names, field_policies, readonly_root) => {
                TypedInner::StructDef(tag, name, field_names, field_policies, readonly_root)
            }
            TypedInner::RecordDef(tag, name, field_names, field_policies, readonly_root) => {
                TypedInner::RecordDef(tag, name, field_names, field_policies, readonly_root)
            }
            TypedInner::EnumDef(name, variants) => TypedInner::EnumDef(name, variants),
            TypedInner::TraitDef(name, methods) => TypedInner::TraitDef(name, methods),
            TypedInner::TraitImplDef(trait_name, target_name) => {
                TypedInner::TraitImplDef(trait_name, target_name)
            }
            TypedInner::Semi(inner) => TypedInner::Semi(Box::new(self.resolve_typed_node(*inner))),
        };

        TypedNode { ty, span, node }
    }

    pub(super) fn resolve_typed_pattern(&self, pattern: TypedPattern) -> TypedPattern {
        match pattern {
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
            TypedPattern::ResultOk(ty, inner) => TypedPattern::ResultOk(
                self.resolve_ty(&ty),
                Box::new(self.resolve_typed_pattern(*inner)),
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
                input_ty: self.resolve_ty(&input_ty),
                extractor,
                extractor_ty: self.resolve_ty(&extractor_ty),
                success_tag,
                no_match_tag,
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
            TypedMatchPattern::Tuple(items) => TypedMatchPattern::Tuple(
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
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys,
                items,
            } => TypedMatchPattern::Extractor {
                input_ty: self.resolve_ty(&input_ty),
                extractor,
                extractor_ty: self.resolve_ty(&extractor_ty),
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys: seq_tys.into_iter().map(|ty| self.resolve_ty(&ty)).collect(),
                items: items
                    .into_iter()
                    .map(|item| self.resolve_typed_match_pattern(item))
                    .collect(),
            },
        }
    }

    pub(super) fn format_signature(&self, name: &str, params: &[Ty], ret: &Ty) -> String {
        format!(
            "{}: ({}) -> {}",
            name,
            params
                .iter()
                .map(|ty| self.ty_name(ty))
                .collect::<Vec<_>>()
                .join(", "),
            self.ty_name(ret)
        )
    }

    pub(super) fn find_tail_print_call<'a>(&self, node: &'a TypedNode) -> Option<&'a TypedNode> {
        match &node.node {
            TypedInner::Block(stmts) => stmts
                .last()
                .and_then(|last| self.find_tail_print_call(last)),
            TypedInner::Semi(inner) => self.find_tail_print_call(inner),
            TypedInner::App(func, _) => match &func.ty {
                Ty::BuiltinFunc { name, .. } if name == "print" => Some(node),
                _ => None,
            },
            _ => None,
        }
    }

    pub(super) fn describe_unit_return_hint(&self, body: &TypedNode) -> Option<String> {
        let call = self.find_tail_print_call(body)?;
        if let TypedInner::App(func, _) = &call.node {
            if let Ty::BuiltinFunc { name, params, ret } = &func.ty {
                return Some(format!(
                    "The function body ends with `print(...)`, which returns Unit.\n{}\nUse `print(...)` as a statement and end the function with an Int expression.",
                    self.format_signature(name, params, ret)
                ));
            }
        }
        None
    }

    pub(super) fn return_mismatch_span(&self, body: &TypedNode) -> Span {
        self.tail_expr_span(body)
            .unwrap_or_else(|| body.span.clone())
    }

    pub(super) fn tail_expr_span(&self, node: &TypedNode) -> Option<Span> {
        match &node.node {
            TypedInner::Block(stmts) => stmts.last().map(|last| {
                self.tail_expr_span(last)
                    .unwrap_or_else(|| last.span.clone())
            }),
            TypedInner::Semi(inner) => Some(
                self.tail_expr_span(inner)
                    .unwrap_or_else(|| inner.span.clone()),
            ),
            _ => Some(node.span.clone()),
        }
    }
}
