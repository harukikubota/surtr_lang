use super::*;
use sindr::builtin::builtin_type_supports_inherent_impl;

impl Checker {
    pub(in crate::checker) fn split_impl_method_name(name: &str) -> Option<(String, String)> {
        let (target, method) = name.rsplit_once("::")?;
        if target.is_empty() || method.is_empty() {
            None
        } else {
            Some((target.to_string(), method.to_string()))
        }
    }

    pub(in crate::checker) fn split_impl_method_id(id: &ResolvedId) -> Option<(String, String)> {
        if let Some(qualified) = id.qualified_name.as_deref() {
            if let Some(split) = Self::split_impl_method_name(qualified) {
                return Some(split);
            }
        }

        if let Some(split) = Self::split_impl_method_name(&id.name) {
            return Some(split);
        }

        let qualified = id.qualified_name.as_deref()?;
        let mut parts = qualified.rsplitn(3, "::");
        let method = parts.next()?;
        let target = parts.next()?;
        if target.is_empty() || method.is_empty() {
            None
        } else {
            Some((target.to_string(), method.to_string()))
        }
    }

    fn rewrite_inherent_self_apps(ast_ty: &AstTy, target: &str) -> AstTy {
        match ast_ty {
            AstTy::Named(span, name) => AstTy::Named(span.clone(), name.clone()),
            AstTy::ImplTrait(span, name) => AstTy::ImplTrait(span.clone(), name.clone()),
            AstTy::Generic(span, name, args) => AstTy::Generic(
                span.clone(),
                if name == "Self" {
                    target.to_string()
                } else {
                    name.clone()
                },
                args.iter()
                    .map(|arg| Self::rewrite_inherent_self_apps(arg, target))
                    .collect(),
            ),
            AstTy::Tuple(span, items) => AstTy::Tuple(
                span.clone(),
                items
                    .iter()
                    .map(|item| Self::rewrite_inherent_self_apps(item, target))
                    .collect(),
            ),
            AstTy::Func(span, params, ret) => AstTy::Func(
                span.clone(),
                params
                    .iter()
                    .map(|param| Self::rewrite_inherent_self_apps(param, target))
                    .collect(),
                Box::new(Self::rewrite_inherent_self_apps(ret, target)),
            ),
        }
    }

    pub(super) fn rewrite_inherent_signature_ast_ty(
        &self,
        id: &ResolvedId,
        ast_ty: &AstTy,
    ) -> AstTy {
        let Some((target, _)) = Self::split_impl_method_id(id) else {
            return ast_ty.clone();
        };
        let target_surface = Self::surface_name(&target).to_string();
        if !target.starts_with("Global::")
            && self.env.lookup_type_def(&target).is_none()
            && self.env.lookup_type_def(&target_surface).is_none()
            && !builtin_type_supports_inherent_impl(&target_surface)
        {
            return ast_ty.clone();
        }
        Self::rewrite_inherent_self_apps(ast_ty, &target_surface)
    }

    pub(in crate::checker) fn resolve_def_parameter_contract(
        &mut self,
        id: &ResolvedId,
        signature: &sigil::resolved::ResolvedSignatureTy,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        if let Some(inner) = Self::lazy_parameter_inner(signature.syntax()) {
            if !self.enforce_builtin_type_contracts
                && !self.lazy_function_parameters.contains_key(&id.unique_id)
            {
                return Err(self.lazy_type_not_allowed_error(Self::ast_ty_span(signature.syntax())));
            }
            let inner_signature = sigil::resolved::ResolvedSignatureTy {
                syntax: inner.clone(),
                direct_constructor_trait: None,
            };
            let result = self.resolve_def_signature_ast_ty_in_context(
                id,
                &inner_signature,
                TypeSyntaxContext::General,
                tyvars,
            )?;
            return Ok(Ty::Func(vec![], Box::new(result)));
        }
        self.resolve_def_signature_ast_ty_in_context(
            id,
            signature,
            TypeSyntaxContext::General,
            tyvars,
        )
    }

    pub(in crate::checker) fn resolve_def_signature_ast_ty_in_context(
        &mut self,
        id: &ResolvedId,
        signature_ty: &sigil::resolved::ResolvedSignatureTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        let Some((target, _)) = Self::split_impl_method_id(id) else {
            return self.resolve_resolved_signature_ty_in_context(signature_ty, context, tyvars);
        };
        let target_surface = Self::surface_name(&target).to_string();
        if !target.starts_with("Global::")
            && self.env.lookup_type_def(&target).is_none()
            && self.env.lookup_type_def(&target_surface).is_none()
            && !builtin_type_supports_inherent_impl(&target_surface)
        {
            return self.resolve_resolved_signature_ty_in_context(signature_ty, context, tyvars);
        }
        let rewritten = sigil::resolved::ResolvedSignatureTy {
            syntax: Self::rewrite_inherent_self_apps(signature_ty.syntax(), &target_surface),
            direct_constructor_trait: signature_ty.direct_constructor_trait.clone(),
        };
        self.resolve_resolved_signature_ty_in_context(&rewritten, context, tyvars)
    }

    pub(in crate::checker) fn current_impl_self_ty(&self) -> Option<Ty> {
        let symbol = self.current_function_symbol.as_deref()?;
        let mut parts = symbol.split("::").collect::<Vec<_>>();
        if parts.len() < 2 {
            return None;
        }
        let target = parts.pop()?;
        let _method = target;
        let type_name = parts.pop()?;
        let def = self.env.lookup_type_def(type_name)?;
        match def.kind {
            TypeKind::Struct => Some(Ty::Struct(
                def.name.clone(),
                NominalType::new(def.type_param_vars.iter().copied().map(Ty::Var).collect()),
            )),
            TypeKind::Enum => Some(Ty::Enum(def.name.clone(), Vec::new())),
            TypeKind::Record | TypeKind::ConcreteError => None,
        }
    }

    pub(in crate::checker) fn ensure_self_rebinding_types(
        &mut self,
        pattern: &TypedPattern,
        span: &Span,
    ) -> Result<(), TypeError> {
        let expected_self = self.current_impl_self_ty();
        self.ensure_self_rebinding_types_inner(pattern, span, expected_self.as_ref())
    }

    pub(in crate::checker) fn ensure_self_rebinding_types_inner(
        &mut self,
        pattern: &TypedPattern,
        span: &Span,
        expected_self: Option<&Ty>,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedPattern::Located(_, inner) => {
                self.ensure_self_rebinding_types_inner(inner, span, expected_self)
            }
            TypedPattern::HashMap(_, entries) => {
                for entry in entries {
                    self.ensure_self_rebinding_types_inner(&entry.pattern, span, expected_self)?;
                }
                Ok(())
            }
            TypedPattern::Var(bind_ty, id) => {
                if id.name == "self" {
                    let Some(expected) = expected_self else {
                        return Err(TypeError {
                            structured: None,
                            message: "`self` can only be rebound inside impl methods".to_string(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !self.types_compatible(expected, bind_ty)? {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "`self` rebinding requires Self type ({}), got {}",
                                self.ty_name(expected),
                                self.ty_name(bind_ty)
                            ),
                            span: id.span.clone(),
                            hint: None,
                        });
                    }
                }
                Ok(())
            }
            TypedPattern::As(alias_ty, inner, id) => {
                if id.name == "self" {
                    let Some(expected) = expected_self else {
                        return Err(TypeError {
                            structured: None,
                            message: "`self` can only be rebound inside impl methods".to_string(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !self.types_compatible(expected, alias_ty)? {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "`self` rebinding requires Self type ({}), got {}",
                                self.ty_name(expected),
                                self.ty_name(alias_ty)
                            ),
                            span: id.span.clone(),
                            hint: None,
                        });
                    }
                }
                self.ensure_self_rebinding_types_inner(inner, span, expected_self)
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.ensure_self_rebinding_types_inner(head, span, expected_self)?;
                self.ensure_self_rebinding_types_inner(tail, span, expected_self)
            }
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    self.ensure_self_rebinding_types_inner(item, span, expected_self)?;
                }
                Ok(())
            }
            TypedPattern::Constructor { fields, .. } => {
                for field in fields {
                    self.ensure_self_rebinding_types_inner(field, span, expected_self)?;
                }
                Ok(())
            }
            TypedPattern::Extractor { items, .. } => {
                for item in items {
                    self.ensure_self_rebinding_types_inner(item, span, expected_self)?;
                }
                Ok(())
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::Pin(_, _, _)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => Ok(()),
        }
    }

    pub(in crate::checker) fn ensure_struct_impl_new_contract(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut struct_defs: HashMap<String, (Span, Ty)> = HashMap::new();
        let mut structs_with_new: HashSet<String> = HashSet::new();

        for stmt in stmts {
            if let Resolved::StructDef(_, id, type_params, _fields, _attrs) = stmt {
                let mut tyvars = HashMap::new();
                self.seed_signature_type_params(type_params, &mut tyvars);
                let expected_self_ty = Ty::Struct(
                    id.name.clone(),
                    NominalType::new(
                        type_params
                            .iter()
                            .map(|param| {
                                tyvars
                                    .get(&param.name)
                                    .cloned()
                                    .expect("seeded struct type parameter")
                            })
                            .collect(),
                    ),
                );
                struct_defs.insert(id.name.clone(), (id.span.clone(), expected_self_ty.clone()));
                if let Some(surface_name) = id.name.strip_prefix("Global::") {
                    struct_defs.insert(
                        surface_name.to_string(),
                        (id.span.clone(), expected_self_ty),
                    );
                }
            }
        }

        for stmt in stmts {
            let Resolved::Def(_, id, _, _, _, _, _, _) = stmt else {
                continue;
            };
            if let Some((target, method)) = Self::split_impl_method_id(id) {
                if method == "new" {
                    structs_with_new.insert(target.clone());
                    if !target.contains("::") {
                        structs_with_new.insert(format!("Global::{}", target));
                    }
                    if let Some(surface_name) = target.strip_prefix("Global::") {
                        structs_with_new.insert(surface_name.to_string());
                    }
                    let Some((span, expected_self_ty)) = struct_defs.get(&target) else {
                        continue;
                    };
                    let Some(method_ty) = self.env.lookup_var(id.unique_id) else {
                        continue;
                    };
                    let ret_ty = match self.resolve_ty(method_ty) {
                        Ty::UserFunc { ret, .. }
                        | Ty::BuiltinFunc { ret, .. }
                        | Ty::Func(_, ret) => *ret,
                        other => {
                            return Err(self.struct_new_contract_error(&target, span, Some(&other)))
                        }
                    };
                    if !self.struct_new_return_allowed(expected_self_ty, &ret_ty)? {
                        return Err(self.struct_new_contract_error(&target, span, Some(&ret_ty)));
                    }
                }
            }
        }

        for (struct_name, (span, _)) in struct_defs {
            if !structs_with_new.contains(&struct_name) {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Struct `{}` must define `new` in its impl block (e.g. `impl {} {{ def new(...) -> Self {{ ... }} }}` or `impl {} {{ def new(...) -> Result<Self, Error> {{ ... }} }}`)",
                        struct_name, struct_name
                        , struct_name
                    ),
                    span,
                    hint: None,
                });
            }
        }

        Ok(())
    }

    pub(in crate::checker) fn register_function_id(&mut self, id: &ResolvedId) {
        let key = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        self.function_ids_by_name.insert(key.clone(), id.clone());
        if let Some(surface_key) = key.strip_prefix("Global::") {
            self.function_ids_by_name
                .insert(surface_key.to_string(), id.clone());
        }
    }

    pub(in crate::checker) fn predeclare_functions(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut fun_idx = self.env.next_fun_idx;
        let mut trait_impl_keys_in_stmts = HashSet::new();

        for stmt in stmts {
            let Resolved::TraitImplDef(
                span,
                declaration_id,
                trait_id,
                trait_args,
                target_ast_ty,
                _,
                _,
            ) = stmt
            else {
                continue;
            };
            let trait_key = self.trait_key(trait_id);
            let trait_info = self
                .traits
                .get(&trait_key)
                .cloned()
                .ok_or_else(|| TypeError {
                    structured: None,
                    message: format!("Unknown trait: {}", trait_id.name),
                    span: span.clone(),
                    hint: None,
                })?;
            let (_, target_ty, _, _) =
                self.resolve_trait_impl_head_tys(&trait_info, trait_args, target_ast_ty)?;
            self.trait_target_name(&target_ty)
                .ok_or_else(|| {
                    TypeError {
                structured: None,
                message:
                    "trait impl target must be a concrete named type, tuple type, or function type"
                        .into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: None,
            }
                })?;
            trait_impl_keys_in_stmts.insert(*declaration_id);
        }

        for stmt in stmts {
            match stmt {
                Resolved::BuiltinDecl(
                    _,
                    id,
                    return_type_arguments,
                    params,
                    ret_ty,
                    where_clause,
                    _,
                ) => {
                    if Self::is_compiler_flow_operator_decl(id) {
                        if !crate::checker::signatures::builtin_surface_matches(
                            id,
                            return_type_arguments,
                            params,
                            ret_ty.as_ref(),
                            where_clause.as_ref(),
                        ) {
                            return Err(TypeError::new(
                                format!(
                                    "Builtin {} does not match its canonical surface signature",
                                    id.name
                                ),
                                id.span.clone(),
                            ));
                        }
                        continue;
                    }
                    let declared_name = id.name.rsplit("::").next().unwrap_or(&id.name);
                    let runtime_name = sindr::builtin::builtin_runtime_name(
                        declared_name,
                        id.qualified_name.as_deref(),
                    );
                    let meta = sindr::builtin::builtin_meta_by_runtime_name(runtime_name);
                    let is_special_form = Self::is_special_form_builtin_decl_name(declared_name);
                    if !is_special_form
                        && (meta.is_none()
                            || crate::checker::signatures::builtin_surface_signature(id).is_none())
                    {
                        return Err(TypeError {
                            structured: None,
                            message: format!("Unknown builtin declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: Some(
                                "Declare the builtin owner and runtime mapping in BUILTIN_METAS before using @builtin."
                                    .into(),
                            ),
                        });
                    }
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    let return_type_argument_tys = return_type_arguments
                        .iter()
                        .map(|argument| {
                            self.resolve_builtin_signature_ty_in_context(
                                &argument.ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let param_tys = params
                        .iter()
                        .enumerate()
                        .map(|(_index, param)| {
                            self.resolve_builtin_signature_ty_in_context(
                                &param.ty,
                                TypeSyntaxContext::StdBuiltinParameter,
                                &mut tyvars,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let ret = match ret_ty {
                        Some(ty) => self.resolve_builtin_signature_ty_in_context(
                            ty,
                            TypeSyntaxContext::FunctionReturn,
                            &mut tyvars,
                        )?,
                        None => Ty::Unit,
                    };
                    let canonical_where_constraints =
                        crate::checker::signatures::canonical_where_constraints(
                            self,
                            where_clause.as_ref(),
                            &mut tyvars,
                        )?;
                    crate::checker::signatures::validate_boolean_function_suffix(self, id, &ret)?;
                    if let Some(meta) = meta {
                        self.callable_signatures.insert(
                            id.unique_id,
                            Arc::new(crate::checker::signatures::canonical_callable_signature(
                                self,
                                id,
                                return_type_arguments,
                                params,
                                return_type_argument_tys.as_slice(),
                                param_tys.as_slice(),
                                ret.clone(),
                                canonical_where_constraints,
                                sindr::signature::RuntimeTarget::Builtin(meta.builtin_id()),
                                sindr::signature::CallableDeclarationKind::Builtin,
                            )?),
                        );
                    }
                    self.env.bind_var(
                        id.unique_id,
                        Ty::BuiltinFunc {
                            name: sindr::builtin::builtin_runtime_name(
                                declared_name,
                                id.qualified_name.as_deref(),
                            )
                            .into(),
                            params: param_tys,
                            ret: Box::new(ret),
                        },
                    );
                }
                Resolved::BuiltinExtractorDecl(_, id, param, ret_ty, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    let param_tys = param
                        .iter()
                        .map(|param| match &param.ty {
                            Some(ty) => self.resolve_builtin_ast_ty_in_context(
                                ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            ),
                            None => Ok(self.env.fresh_tyvar()),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let target_param = param.last().ok_or_else(|| {
                        TypeError::new("Extractor requires at least one input", id.span.clone())
                    })?;
                    let target_span = target_param
                        .ty
                        .as_ref()
                        .map(Self::ast_ty_span)
                        .unwrap_or(&target_param.id.span);
                    self.require_concrete_extractor_target_head(
                        param_tys
                            .last()
                            .expect("non-empty Extractor parameter types"),
                        target_span,
                        &id.name,
                    )?;
                    let ret = self.resolve_builtin_ast_ty_in_context(
                        ret_ty,
                        TypeSyntaxContext::ExtractorReturn,
                        &mut tyvars,
                    )?;
                    self.env.bind_var(
                        id.unique_id,
                        Ty::BuiltinFunc {
                            name: id.name.clone(),
                            params: param_tys,
                            ret: Box::new(ret),
                        },
                    );
                }
                Resolved::Def(
                    _,
                    id,
                    return_type_arguments,
                    params,
                    ret_ty,
                    where_clause,
                    _,
                    _attrs,
                ) => {
                    self.validate_standard_lazy_function_decl(&id.span, id, params, ret_ty)?;
                    self.register_function_id(id);
                    let lazy_parameters = params
                        .iter()
                        .enumerate()
                        .filter_map(|(ordinal, parameter)| {
                            Self::lazy_parameter_inner(parameter.ty.syntax())
                                .is_some()
                                .then_some(ordinal)
                        })
                        .collect::<Vec<_>>();
                    if self.enforce_builtin_type_contracts && !lazy_parameters.is_empty() {
                        self.lazy_function_parameters
                            .insert(id.unique_id, lazy_parameters);
                    }
                    let mut tyvars = HashMap::new();
                    let mut direct_constructor_inputs =
                        crate::checker::signatures::DirectConstructorInputs::default();
                    let mut return_type_argument_tys = Vec::new();
                    for argument in return_type_arguments {
                        let ty = self.resolve_def_signature_ast_ty_in_context(
                            id,
                            &argument.ty,
                            TypeSyntaxContext::General,
                            &mut tyvars,
                        )?;
                        crate::checker::signatures::remember_direct_constructor_input(
                            self,
                            &argument.ty,
                            &ty,
                            &mut direct_constructor_inputs,
                        );
                        return_type_argument_tys.push(ty);
                    }
                    let param_tys = params
                        .iter()
                        .map(|param| {
                            let param_ty =
                                self.resolve_def_parameter_contract(id, &param.ty, &mut tyvars)?;
                            crate::checker::signatures::remember_direct_constructor_input(
                                self,
                                &param.ty,
                                &param_ty,
                                &mut direct_constructor_inputs,
                            );
                            let param_ty =
                                crate::checker::signatures::coalesce_direct_constructor_inputs(
                                    self,
                                    param_ty,
                                    &direct_constructor_inputs,
                                );
                            Ok(param_ty)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let param_names = params
                        .iter()
                        .map(|param| param.id.name.clone())
                        .collect::<Vec<_>>();
                    let ret = match ret_ty {
                        Some(ty) => self.resolve_def_signature_ast_ty_in_context(
                            id,
                            ty,
                            TypeSyntaxContext::FunctionReturn,
                            &mut tyvars,
                        )?,
                        None => Ty::Unit,
                    };
                    if let Some(ret_ty) = ret_ty {
                        crate::checker::signatures::remember_direct_constructor_input(
                            self,
                            ret_ty,
                            &ret,
                            &mut direct_constructor_inputs,
                        );
                    }
                    let ret = crate::checker::signatures::coalesce_direct_constructor_inputs(
                        self,
                        ret,
                        &direct_constructor_inputs,
                    );
                    self.apply_resolved_where_trait_bounds(where_clause.as_ref(), &tyvars, None)?;
                    for (argument, ty) in
                        return_type_arguments.iter().zip(&return_type_argument_tys)
                    {
                        self.validate_nominal_type_well_formed(
                            ty,
                            Self::ast_ty_span(argument.ty.syntax()),
                            false,
                        )?;
                    }
                    for (parameter, ty) in params.iter().zip(&param_tys) {
                        self.validate_nominal_type_well_formed(
                            ty,
                            Self::ast_ty_span(parameter.ty.syntax()),
                            false,
                        )?;
                    }
                    if let Some(return_annotation) = ret_ty {
                        self.validate_nominal_type_well_formed(
                            &ret,
                            Self::ast_ty_span(return_annotation.syntax()),
                            false,
                        )?;
                    }
                    let canonical_where_constraints =
                        crate::checker::signatures::canonical_where_constraints(
                            self,
                            where_clause.as_ref(),
                            &mut tyvars,
                        )?;
                    let function_symbol = id.qualified_name.as_deref().unwrap_or(&id.name);
                    if self.ty_contains_process_init(&ret)
                        && !self.is_lazy_init_function_symbol(function_symbol)
                    {
                        return Err(TypeError {
                            structured: None,
                            message: "StandbyInit<T> is only allowed as Standby @init return type"
                                .into(),
                            span: ret_ty
                                .as_ref()
                                .map(|ty| Self::ast_ty_span(ty.syntax()))
                                .cloned()
                                .unwrap_or_else(|| id.span.clone()),
                            hint: None,
                        });
                    }
                    let type_params =
                        Self::signature_type_param_vars(&[], &tyvars, &param_tys, &ret);
                    self.env.bind_var(
                        id.unique_id,
                        Ty::UserFunc {
                            fun_idx,
                            type_params,
                            call_substitution: Vec::new(),
                            params: param_tys.clone(),
                            ret: Box::new(ret.clone()),
                        },
                    );
                    self.callable_signatures.insert(
                        id.unique_id,
                        Arc::new(crate::checker::signatures::canonical_callable_signature(
                            self,
                            id,
                            return_type_arguments,
                            params,
                            return_type_argument_tys.as_slice(),
                            param_tys.as_slice(),
                            ret,
                            canonical_where_constraints,
                            sindr::signature::RuntimeTarget::UserFunction(fun_idx),
                            sindr::signature::CallableDeclarationKind::Function,
                        )?),
                    );
                    self.user_func_params.insert(id.unique_id, param_names);
                    if let Some(qualified_name) = id.qualified_name.as_ref() {
                        if Self::split_impl_method_name(qualified_name).is_some() {
                            self.impl_method_uids
                                .insert(qualified_name.clone(), id.unique_id);
                        }
                    }
                    fun_idx += 1;
                }
                Resolved::ExtractorDef(span, id, type_params, param, ret_ty, _, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    self.seed_signature_type_params(type_params, &mut tyvars);
                    let param_tys = param
                        .iter()
                        .map(|param| match &param.ty {
                            Some(ty) => self.resolve_signature_ast_ty_in_context(
                                ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            ),
                            None => Ok(self.env.fresh_tyvar()),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let target_param = param.last().ok_or_else(|| {
                        TypeError::new("Extractor requires at least one input", span.clone())
                    })?;
                    let target_span = target_param
                        .ty
                        .as_ref()
                        .map(Self::ast_ty_span)
                        .unwrap_or(&target_param.id.span);
                    self.require_concrete_extractor_target_head(
                        param_tys
                            .last()
                            .expect("non-empty Extractor parameter types"),
                        target_span,
                        &id.name,
                    )?;
                    let ret = self.resolve_signature_ast_ty_in_context(
                        ret_ty,
                        TypeSyntaxContext::ExtractorReturn,
                        &mut tyvars,
                    )?;
                    for param_ty in &param_tys {
                        self.validate_nominal_type_well_formed(param_ty, span, false)?;
                    }
                    self.validate_nominal_type_well_formed(&ret, span, false)?;
                    let type_params =
                        Self::signature_type_param_vars(type_params, &tyvars, &param_tys, &ret);
                    self.env.bind_var(
                        id.unique_id,
                        Ty::UserFunc {
                            fun_idx,
                            type_params,
                            call_substitution: Vec::new(),
                            params: param_tys,
                            ret: Box::new(ret),
                        },
                    );
                    fun_idx += 1;
                }
                Resolved::DeferrorDef(span, id, _, body) => {
                    self.register_function_id(id);
                    let Resolved::Closure(_, params, _, _) = body.as_ref() else {
                        return Err(TypeError::new(
                            "deferror body must be a constructor block",
                            span.clone(),
                        ));
                    };
                    let inputs = params
                        .iter()
                        .map(|param| {
                            let ty = param.ty.as_ref().ok_or_else(|| {
                                TypeError::new(
                                    "deferror constructor inputs require type annotations",
                                    param.id.span.clone(),
                                )
                            })?;
                            Ok((
                                param.id.name.clone(),
                                self.resolve_error_definition_ty(
                                    ty,
                                    ErrorDefinitionTypePosition::MessageConstructorInput,
                                )?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    let param_tys = inputs.iter().map(|(_, ty)| ty.clone()).collect();
                    self.env
                        .error_constructor_inputs
                        .insert(id.unique_id, inputs);

                    self.env.bind_var(
                        id.unique_id,
                        Ty::UserFunc {
                            fun_idx,
                            type_params: Vec::new(),
                            call_substitution: Vec::new(),
                            params: param_tys,
                            ret: Box::new(Ty::Error),
                        },
                    );
                    self.env.register_error_constructor(id.unique_id);
                    fun_idx += 1;
                }
                Resolved::BuiltinTypeDecl(_, _, _, _) => {}
                _ => {}
            }
        }

        if trait_impl_keys_in_stmts.is_empty() {
            self.env.next_fun_idx = fun_idx;
            return Ok(());
        }
        let mut trait_impls = self.trait_impls.values().cloned().collect::<Vec<_>>();
        trait_impls.sort_by_key(|info| info.declaration_key.declaration_id);

        for trait_impl in trait_impls {
            if !trait_impl_keys_in_stmts.contains(&trait_impl.declaration_key.declaration_id) {
                continue;
            }
            let trait_key = self.trait_key(&trait_impl.trait_id);
            let trait_info = self
                .traits
                .get(&trait_key)
                .cloned()
                .ok_or_else(|| TypeError {
                    structured: None,
                    message: format!("Unknown trait: {}", trait_impl.trait_id.name),
                    span: trait_impl.trait_id.span.clone(),
                    hint: None,
                })?;
            let mut methods = trait_impl.methods.iter().collect::<Vec<_>>();
            methods.sort_by_key(|(_, method)| method.function_id.unique_id);

            for (method_name, method) in methods {
                if method.is_builtin {
                    continue;
                }
                self.register_function_id(&method.function_id);
                let trait_method =
                    trait_info
                        .methods
                        .get(method_name)
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!(
                                "Unknown trait method: {}::{}",
                                trait_impl.trait_id.name, method_name
                            ),
                            span: method.span.clone(),
                            hint: None,
                        })?;
                let (param_tys, ret, type_params, return_type_argument_tys, _) = self
                    .resolve_trait_impl_method_signature(
                        &trait_info,
                        &trait_impl.trait_args,
                        method,
                        &trait_impl.target_ast_ty,
                        &trait_method.ret_ty,
                        trait_impl.where_clause.as_ref(),
                        trait_impl.generated_derive,
                        &trait_impl.constructor_slot_positions,
                    )?;
                let param_names = method
                    .value_parameters
                    .iter()
                    .map(|param| param.id.name.clone())
                    .collect::<Vec<_>>();
                self.env.bind_var(
                    method.function_id.unique_id,
                    Ty::UserFunc {
                        fun_idx,
                        type_params,
                        call_substitution: Vec::new(),
                        params: param_tys.clone(),
                        ret: Box::new(ret.clone()),
                    },
                );
                self.user_func_params
                    .insert(method.function_id.unique_id, param_names);
                self.callable_signatures.insert(
                    method.function_id.unique_id,
                    Arc::new(crate::checker::signatures::canonical_callable_signature(
                        self,
                        &method.function_id,
                        &method.return_type_arguments,
                        &method.value_parameters,
                        return_type_argument_tys.as_slice(),
                        param_tys.as_slice(),
                        ret.clone(),
                        sindr::signature::CanonicalConstraintSet::default(),
                        sindr::signature::RuntimeTarget::TraitDispatch(
                            method.function_id.unique_id,
                        ),
                        sindr::signature::CallableDeclarationKind::TraitMethod,
                    )?),
                );
                if let Some(qualified_name) = method.function_id.qualified_name.as_ref() {
                    if Self::split_impl_method_name(qualified_name).is_some() {
                        self.impl_method_uids
                            .insert(qualified_name.clone(), method.function_id.unique_id);
                    }
                }
                fun_idx += 1;
            }
        }

        self.env.next_fun_idx = fun_idx;
        Ok(())
    }
}
