use super::*;

impl Checker {
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
                    out.push(name.clone());
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
            AstTy::ImplTrait(_, name) => out.push(name.clone()),
        }
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
            AstTy::Named(span, name) => match name.as_str() {
                "Int" => Ok(Ty::Int),
                "Float" => Ok(Ty::Float),
                "String" => Ok(Ty::Str),
                "Boolean" => Ok(Ty::Bool),
                "Unit" => Ok(Ty::Unit),
                "Error" => Ok(Ty::Error),
                other => {
                    if let Some(def) = self.env.lookup_type_def(other) {
                        match &def.kind {
                            crate::env::TypeKind::Struct => {
                                Ok(Ty::Struct(def.name.clone(), def.fields.clone()))
                            }
                            crate::env::TypeKind::Record => {
                                Ok(Ty::Record(def.name.clone(), def.fields.clone()))
                            }
                            crate::env::TypeKind::Error => Ok(Ty::Error),
                            crate::env::TypeKind::Enum => {
                                if def.type_params.is_empty() {
                                    Ok(Ty::Enum(def.name.clone(), Vec::new()))
                                } else {
                                    Err(TypeError {
                                        message: format!(
                                            "Type {} requires {} type argument(s)",
                                            other,
                                            def.type_params.len()
                                        ),
                                        span: span.clone(),
                                        hint: None,
                                    })
                                }
                            }
                        }
                    } else {
                        Err(TypeError {
                            message: format!("Unknown type: {}", other),
                            span: span.clone(),
                            hint: None,
                        })
                    }
                }
            },
            AstTy::Generic(span, name, args) => match name.as_str() {
                "MatchResult" => {
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
                    if args.len() != 1 {
                        return Err(TypeError {
                            message: "List<T> requires exactly 1 type argument".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let inner_ty =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::List(Box::new(inner_ty)))
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
                        if context != TypeSyntaxContext::FunctionReturn {
                            return Err(TypeError {
                                message:
                                    "Result<T, E> is only allowed in function return signatures."
                                        .into(),
                                span: span.clone(),
                                hint: Some("Use Result<T> in local code.".into()),
                            });
                        }
                        self.resolve_ast_ty_in_context(&args[1], TypeSyntaxContext::ErrorMarker)?
                    } else {
                        Ty::Error
                    };
                    Ok(Ty::Result(Box::new(ok), Box::new(err)))
                }
                other => {
                    let def = self.env.lookup_type_def(other).ok_or_else(|| TypeError {
                        message: format!("Unknown generic type: {}", other),
                        span: span.clone(),
                        hint: None,
                    })?;
                    if def.type_params.len() != args.len() {
                        return Err(TypeError {
                            message: format!(
                                "Type {} requires {} type argument(s), got {}",
                                other,
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
                        crate::env::TypeKind::Enum => Ok(Ty::Enum(def.name.clone(), resolved_args)),
                        _ => Err(TypeError {
                            message: format!(
                                "Generic type {} is not supported in this context",
                                other
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
                    .map(|p| self.resolve_ast_ty_in_context(p, TypeSyntaxContext::General))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_ast_ty_in_context(ret, TypeSyntaxContext::General)?;
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
        match ast_ty {
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
            AstTy::ImplTrait(_, trait_name) => {
                if context == TypeSyntaxContext::ErrorMarker {
                    return Err(TypeError {
                        message:
                            "The error marker E in Result<T, E> must be a deferror-defined type."
                                .into(),
                        span: Self::ast_ty_span(ast_ty).clone(),
                        hint: None,
                    });
                }
                let fresh = self.env.fresh_tyvar();
                if let Ty::Var(var) = fresh {
                    self.register_tyvar_bound(var, trait_name);
                }
                Ok(fresh)
            }
            AstTy::Generic(span, name, args) if name == "List" => {
                if args.len() != 1 {
                    return Err(TypeError {
                        message: "List<T> requires exactly 1 type argument".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let inner = self.resolve_signature_ast_ty_in_context(
                    &args[0],
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
                Ok(Ty::List(Box::new(inner)))
            }
            AstTy::Generic(span, name, args) if name == "MatchResult" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(TypeError {
                        message: "MatchResult<$Value> or MatchResult<$Value, Error> requires 1 or 2 type arguments".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let value = self.resolve_signature_ast_ty_in_context(
                    &args[0],
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
                if args.len() == 2 {
                    let err = self.resolve_signature_ast_ty_in_context(
                        &args[1],
                        TypeSyntaxContext::General,
                        tyvars,
                    )?;
                    if !matches!(err, Ty::Error) {
                        return Err(TypeError {
                            message:
                                "MatchResult<$Value, Error> requires Error as the second argument"
                                    .into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                }
                Ok(Ty::Enum("MatchResult".into(), vec![value]))
            }
            AstTy::Generic(span, name, args) if name == "Result" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(TypeError {
                        message: "Result<T> or Result<T, E> requires 1 or 2 type arguments".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let ok = self.resolve_signature_ast_ty_in_context(
                    &args[0],
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
                let err = if args.len() == 2 {
                    if context != TypeSyntaxContext::FunctionReturn {
                        return Err(TypeError {
                            message: "Result<T, E> is only allowed in function return signatures."
                                .into(),
                            span: span.clone(),
                            hint: Some("Use Result<T> in local code.".into()),
                        });
                    }
                    self.resolve_signature_ast_ty_in_context(
                        &args[1],
                        TypeSyntaxContext::ErrorMarker,
                        tyvars,
                    )?
                } else {
                    Ty::Error
                };
                Ok(Ty::Result(Box::new(ok), Box::new(err)))
            }
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
                        self.resolve_signature_ast_ty_in_context(
                            item,
                            TypeSyntaxContext::General,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Ty::Tuple(items))
            }
            AstTy::Generic(span, name, args) => {
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
                        self.resolve_signature_ast_ty_in_context(
                            arg,
                            TypeSyntaxContext::General,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match def.kind {
                    crate::env::TypeKind::Enum => Ok(Ty::Enum(def.name.clone(), resolved_args)),
                    _ => Err(TypeError {
                        message: format!("Generic type {} is not supported in this context", name),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|param| {
                        self.resolve_signature_ast_ty_in_context(
                            param,
                            TypeSyntaxContext::General,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_signature_ast_ty_in_context(
                    ret,
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
                Ok(Ty::Func(params, Box::new(ret)))
            }
            _ => self.resolve_ast_ty_in_context(ast_ty, context),
        }
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
        match ast_ty {
            AstTy::Named(_, name) if name == "Self" => Ok(self_ty.clone()),
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
            AstTy::ImplTrait(span, trait_name) => Err(TypeError {
                message: format!(
                    "`impl {}` is not supported inside trait method signatures",
                    trait_name
                ),
                span: span.clone(),
                hint: None,
            }),
            AstTy::Generic(span, name, args) if name == "List" => {
                if args.len() != 1 {
                    return Err(TypeError {
                        message: "List<T> requires exactly 1 type argument".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let inner = self.resolve_trait_signature_ast_ty_in_context(
                    &args[0],
                    TypeSyntaxContext::General,
                    self_ty,
                    tyvars,
                )?;
                Ok(Ty::List(Box::new(inner)))
            }
            AstTy::Generic(span, name, args) if name == "MatchResult" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(TypeError {
                        message: "MatchResult<$Value> or MatchResult<$Value, Error> requires 1 or 2 type arguments".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let value = self.resolve_trait_signature_ast_ty_in_context(
                    &args[0],
                    TypeSyntaxContext::General,
                    self_ty,
                    tyvars,
                )?;
                if args.len() == 2 {
                    let err = self.resolve_trait_signature_ast_ty_in_context(
                        &args[1],
                        TypeSyntaxContext::General,
                        self_ty,
                        tyvars,
                    )?;
                    if !matches!(err, Ty::Error) {
                        return Err(TypeError {
                            message:
                                "MatchResult<$Value, Error> requires Error as the second argument"
                                    .into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                }
                Ok(Ty::Enum("MatchResult".into(), vec![value]))
            }
            AstTy::Generic(span, name, args) if name == "Result" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(TypeError {
                        message: "Result<T> or Result<T, E> requires 1 or 2 type arguments".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let ok = self.resolve_trait_signature_ast_ty_in_context(
                    &args[0],
                    TypeSyntaxContext::General,
                    self_ty,
                    tyvars,
                )?;
                let err = if args.len() == 2 {
                    if context != TypeSyntaxContext::FunctionReturn {
                        return Err(TypeError {
                            message: "Result<T, E> is only allowed in function return signatures."
                                .into(),
                            span: span.clone(),
                            hint: Some("Use Result<T> in local code.".into()),
                        });
                    }
                    self.resolve_trait_signature_ast_ty_in_context(
                        &args[1],
                        TypeSyntaxContext::ErrorMarker,
                        self_ty,
                        tyvars,
                    )?
                } else {
                    Ty::Error
                };
                Ok(Ty::Result(Box::new(ok), Box::new(err)))
            }
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
                        self.resolve_trait_signature_ast_ty_in_context(
                            item,
                            TypeSyntaxContext::General,
                            self_ty,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Ty::Tuple(items))
            }
            AstTy::Generic(span, name, args) => {
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
                        self.resolve_trait_signature_ast_ty_in_context(
                            arg,
                            TypeSyntaxContext::General,
                            self_ty,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match def.kind {
                    crate::env::TypeKind::Enum => Ok(Ty::Enum(def.name.clone(), resolved_args)),
                    _ => Err(TypeError {
                        message: format!("Generic type {} is not supported in this context", name),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|param| {
                        self.resolve_trait_signature_ast_ty_in_context(
                            param,
                            TypeSyntaxContext::General,
                            self_ty,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_trait_signature_ast_ty_in_context(
                    ret,
                    TypeSyntaxContext::General,
                    self_ty,
                    tyvars,
                )?;
                Ok(Ty::Func(params, Box::new(ret)))
            }
            _ => self.resolve_ast_ty_in_context(ast_ty, context),
        }
    }

    pub(super) fn resolve_builtin_ast_ty_in_context(
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        match ast_ty {
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
            AstTy::Generic(span, name, args) => match name.as_str() {
                "MatchResult" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(TypeError {
                            message: "MatchResult<$Value> or MatchResult<$Value, Error> requires 1 or 2 type arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let value = self.resolve_builtin_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                    )?;
                    if args.len() == 2 {
                        let err = self.resolve_builtin_ast_ty_in_context(
                            &args[1],
                            TypeSyntaxContext::General,
                            tyvars,
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
                    if args.len() != 1 {
                        return Err(TypeError {
                            message: "List<T> requires exactly 1 type argument".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let inner_ty = self.resolve_builtin_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                    )?;
                    Ok(Ty::List(Box::new(inner_ty)))
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
                    let ok = self.resolve_builtin_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
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
                        self.resolve_builtin_ast_ty_in_context(
                            &args[1],
                            TypeSyntaxContext::ErrorMarker,
                            tyvars,
                        )?
                    } else {
                        Ty::Error
                    };
                    Ok(Ty::Result(Box::new(ok), Box::new(err)))
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
                        self.resolve_builtin_ast_ty_in_context(
                            item,
                            TypeSyntaxContext::General,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Ty::Tuple(items))
            }
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|p| {
                        self.resolve_builtin_ast_ty_in_context(
                            p,
                            TypeSyntaxContext::General,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_builtin_ast_ty_in_context(
                    ret,
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
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

        let def = self.env.lookup_type_def(name).ok_or_else(|| TypeError {
            message: "The error marker E in Result<T, E> must be a deferror-defined type.".into(),
            span: span.clone(),
            hint: None,
        });

        if let Ok(def) = def {
            if def.kind != crate::env::TypeKind::Error {
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
        let expected = self.resolve_ty(expected);
        let got = self.resolve_ty(got);
        match (&expected, &got) {
            (Ty::Var(var), ty) | (ty, Ty::Var(var)) => self.bind_tyvar(*var, ty),
            (Ty::Int, Ty::Int)
            | (Ty::Float, Ty::Float)
            | (Ty::Str, Ty::Str)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Unit, Ty::Unit)
            | (Ty::Error, Ty::Error) => true,
            (Ty::List(a), Ty::List(b)) => self.types_compatible(a, b),
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
            (Ty::Struct(n1, _), Ty::Struct(n2, _)) => n1 == n2,
            (Ty::Record(n1, _), Ty::Record(n2, _)) => n1 == n2,
            (Ty::Enum(n1, args1), Ty::Enum(n2, args2)) => {
                n1 == n2
                    && args1.len() == args2.len()
                    && args1
                        .iter()
                        .zip(args2.iter())
                        .all(|(left, right)| self.types_compatible(left, right))
            }
            _ => false,
        }
    }

    pub(super) fn bind_tyvar(&mut self, var: u32, ty: &Ty) -> bool {
        let ty = self.resolve_ty(ty);
        if ty == Ty::Var(var) {
            return true;
        }
        if self.ty_contains_var(&ty, var) {
            return false;
        }
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
                    return false;
                }
            }
        }
        self.substitutions.insert(var, ty);
        true
    }

    pub(super) fn ty_satisfies_bounds(&self, ty: &Ty, bounds: &[String]) -> bool {
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
            Ty::List(inner) => self.ty_contains_var(&inner, needle),
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
        match ty {
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
        }
    }

    pub(super) fn instantiate_builtin_ty(&mut self, ty: &Ty) -> Ty {
        let mut fresh = HashMap::new();
        self.instantiate_ty_with_fresh(ty, &mut fresh)
    }

    pub(super) fn instantiate_enum_variant(
        &mut self,
        variant: &crate::env::EnumVariantInfo,
    ) -> crate::env::EnumVariantInfo {
        let mut fresh = HashMap::new();
        crate::env::EnumVariantInfo {
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
        }
    }

    pub(super) fn ty_name(&self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Boolean".into(),
            Ty::Unit => "Unit".into(),
            Ty::Error => "Error".into(),
            Ty::List(inner) => format!("List<{}>", self.ty_name(inner)),
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
            Ty::Struct(name, _) | Ty::Record(name, _) => name.clone(),
            Ty::Enum(name, args) => {
                if args.is_empty() {
                    name.clone()
                } else {
                    format!(
                        "{}<{}>",
                        name,
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

    pub(super) fn resolve_typed_node(&self, node: TypedNode) -> TypedNode {
        let span = node.span.clone();
        let ty = self.resolve_ty(&node.ty);
        let node = match node.node {
            TypedInner::Lit(lit) => TypedInner::Lit(lit),
            TypedInner::Var(id) => TypedInner::Var(id),
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
                args,
            } => TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty: self.resolve_ty(&receiver_ty),
                dispatch,
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
            TypedInner::ResultMap(left, right) => TypedInner::ResultMap(
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::ResultBind(left, right) => TypedInner::ResultBind(
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
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                Box::new(self.resolve_typed_node(*scrutinee)),
                arms.into_iter()
                    .map(|(pat, body)| {
                        (
                            self.resolve_typed_match_pattern(pat),
                            self.resolve_typed_node(body),
                        )
                    })
                    .collect(),
            ),
            TypedInner::FieldAccess(expr, idx) => {
                TypedInner::FieldAccess(Box::new(self.resolve_typed_node(*expr)), idx)
            }
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
            TypedInner::Def(fun_idx, id, type_params, params, ret_ty, body) => TypedInner::Def(
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
            ),
            TypedInner::ExtractorDef(fun_idx, id, type_params, param, ret_ty, body) => {
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
            TypedInner::StructDef(tag, name, field_names) => {
                TypedInner::StructDef(tag, name, field_names)
            }
            TypedInner::RecordDef(tag, name, field_names) => {
                TypedInner::RecordDef(tag, name, field_names)
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
            TypedMatchPattern::As(inner, id) => {
                TypedMatchPattern::As(Box::new(self.resolve_typed_match_pattern(*inner)), id)
            }
            TypedMatchPattern::Wildcard => TypedMatchPattern::Wildcard,
            TypedMatchPattern::BoolLit(value) => TypedMatchPattern::BoolLit(value),
            TypedMatchPattern::IntLit(value) => TypedMatchPattern::IntLit(value),
            TypedMatchPattern::StrLit(value) => TypedMatchPattern::StrLit(value),
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
