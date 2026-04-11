use super::*;

impl Checker {
    pub(super) fn predeclare_error_types(&mut self, stmts: &[Resolved]) {
        for stmt in stmts {
            if let Resolved::DeferrorDef(_, id, _, _) = stmt {
                self.env.declare_error_type_name(id.name.clone());
            }
        }
    }

    pub(super) fn predeclare_type_signatures(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        // Pass 1: reserve deterministic tags for all user-defined types.
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, _) => {
                    self.env
                        .predeclare_type_def(id.name.clone(), TypeKind::Struct, Vec::new());
                }
                Resolved::RecordDef(_, id, _) => {
                    self.env
                        .predeclare_type_def(id.name.clone(), TypeKind::Record, Vec::new());
                }
                Resolved::DeferrorDef(_, id, _, _) => {
                    self.env
                        .predeclare_type_def(id.name.clone(), TypeKind::Error, Vec::new());
                }
                Resolved::EnumDef(_, id, type_params, _) => {
                    self.env.predeclare_type_def(
                        id.name.clone(),
                        TypeKind::Enum,
                        type_params.iter().map(|param| param.name.clone()).collect(),
                    );
                }
                _ => {}
            }
        }

        self.ensure_no_type_cycles(stmts)?;

        // Pass 2: finalize field signatures and constructor-like bindings.
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, fields) => {
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            Ok((
                                f.name.clone(),
                                self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields.clone())
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env
                        .bind_var(id.unique_id, Ty::Struct(id.name.clone(), ty_fields));
                }
                Resolved::RecordDef(_, id, fields) => {
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            Ok((
                                f.name.clone(),
                                self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields.clone())
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env
                        .bind_var(id.unique_id, Ty::Record(id.name.clone(), ty_fields));
                }
                Resolved::DeferrorDef(_, id, fields, _) => {
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            Ok((
                                f.name.clone(),
                                self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                }
                Resolved::EnumDef(_, id, type_params, variants) => {
                    let _ = self
                        .env
                        .resolve_type_def_signature(&id.name, Vec::new())
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    let mut sig_tyvars = HashMap::new();
                    let mut enum_ty_args = Vec::new();
                    for param in type_params {
                        let ty = self.env.fresh_tyvar();
                        sig_tyvars.insert(param.name.clone(), ty.clone());
                        enum_ty_args.push(ty);
                    }

                    self.env.bind_var(
                        id.unique_id,
                        Ty::Enum(id.name.clone(), enum_ty_args.clone()),
                    );

                    let mut next_discriminant = sindr::primitives::int(0);
                    let mut seen_discriminants: HashSet<sindr::primitives::SurtrInt> =
                        HashSet::new();
                    let mut enum_variants = Vec::new();

                    for variant in variants {
                        let discriminant = if let Some(explicit) = &variant.discriminant {
                            explicit.clone()
                        } else {
                            next_discriminant.clone()
                        };
                        if seen_discriminants.contains(&discriminant) {
                            return Err(TypeError {
                                message: format!(
                                    "Duplicate enum discriminant {} in {}",
                                    discriminant, id.name
                                ),
                                span: variant.span.clone(),
                                hint: None,
                            });
                        }
                        seen_discriminants.insert(discriminant.clone());
                        next_discriminant = discriminant.clone() + sindr::primitives::int(1);

                        let payload = variant
                            .payload
                            .iter()
                            .map(|ty| {
                                self.resolve_signature_ast_ty_in_context(
                                    ty,
                                    TypeSyntaxContext::General,
                                    &mut sig_tyvars,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;

                        let tag = self.env.reserve_tag();
                        let short_name = variant
                            .id
                            .name
                            .rsplit("::")
                            .next()
                            .unwrap_or(variant.id.name.as_str())
                            .to_string();
                        let info = crate::env::EnumVariantInfo {
                            constructor_name: variant.id.name.clone(),
                            short_name,
                            enum_name: id.name.clone(),
                            enum_ty: Ty::Enum(id.name.clone(), enum_ty_args.clone()),
                            tag,
                            payload: payload.clone(),
                            discriminant: discriminant.clone(),
                        };
                        self.env
                            .register_enum_variant(variant.id.unique_id, info.clone())
                            .map_err(|message| TypeError {
                                message,
                                span: variant.span.clone(),
                                hint: None,
                            })?;
                        enum_variants.push(info);
                    }

                    self.env
                        .enum_variants_by_enum
                        .insert(id.name.clone(), enum_variants);
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub(super) fn ensure_no_type_cycles(&self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut decl_spans: HashMap<String, Span> = HashMap::new();
        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();

        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, fields)
                | Resolved::RecordDef(_, id, fields)
                | Resolved::DeferrorDef(_, id, fields, _) => {
                    decl_spans.insert(id.name.clone(), id.span.clone());
                    edges.entry(id.name.clone()).or_default();
                    for field in fields {
                        let mut refs = Vec::new();
                        Self::collect_type_ref_names(&field.ty, &mut refs);
                        for ref_name in refs {
                            edges.entry(id.name.clone()).or_default().insert(ref_name);
                        }
                    }
                }
                Resolved::EnumDef(_, id, _, variants) => {
                    decl_spans.insert(id.name.clone(), id.span.clone());
                    edges.entry(id.name.clone()).or_default();
                    let mut common_refs: Option<HashSet<String>> = None;
                    for variant in variants {
                        let mut variant_refs = HashSet::new();
                        for payload_ty in &variant.payload {
                            let mut refs = Vec::new();
                            Self::collect_type_ref_names(payload_ty, &mut refs);
                            for ref_name in refs {
                                variant_refs.insert(ref_name);
                            }
                        }
                        common_refs = Some(match common_refs {
                            Some(existing) => existing
                                .intersection(&variant_refs)
                                .cloned()
                                .collect::<HashSet<_>>(),
                            None => variant_refs,
                        });
                    }
                    for ref_name in common_refs.unwrap_or_default() {
                        edges.entry(id.name.clone()).or_default().insert(ref_name);
                    }
                }
                _ => {}
            }
        }

        for refs in edges.values_mut() {
            refs.retain(|name| decl_spans.contains_key(name));
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Visit {
            Visiting,
            Done,
        }

        fn dfs(
            node: &str,
            edges: &HashMap<String, HashSet<String>>,
            states: &mut HashMap<String, Visit>,
            stack: &mut Vec<String>,
        ) -> Option<Vec<String>> {
            if let Some(state) = states.get(node) {
                if *state == Visit::Visiting {
                    let start = stack.iter().position(|name| name == node).unwrap_or(0);
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(node.to_string());
                    return Some(cycle);
                }
                if *state == Visit::Done {
                    return None;
                }
            }

            states.insert(node.to_string(), Visit::Visiting);
            stack.push(node.to_string());

            if let Some(nexts) = edges.get(node) {
                for next in nexts {
                    if let Some(cycle) = dfs(next, edges, states, stack) {
                        return Some(cycle);
                    }
                }
            }

            stack.pop();
            states.insert(node.to_string(), Visit::Done);
            None
        }

        let mut states: HashMap<String, Visit> = HashMap::new();
        let mut stack = Vec::new();
        for name in decl_spans.keys() {
            if let Some(cycle) = dfs(name, &edges, &mut states, &mut stack) {
                let head = cycle.first().cloned().unwrap_or_else(|| name.clone());
                return Err(TypeError {
                    message: format!("Cyclic type definition detected: {}", cycle.join(" -> ")),
                    span: decl_spans
                        .get(&head)
                        .cloned()
                        .unwrap_or(Span { start: 0, end: 0 }),
                    hint: None,
                });
            }
        }

        Ok(())
    }

    pub(super) fn split_impl_method_name(name: &str) -> Option<(String, String)> {
        let (target, method) = name.rsplit_once("::")?;
        if target.is_empty() || method.is_empty() {
            None
        } else {
            Some((target.to_string(), method.to_string()))
        }
    }

    pub(super) fn current_impl_self_ty(&self) -> Option<Ty> {
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
            TypeKind::Struct => Some(Ty::Struct(def.name.clone(), def.fields.clone())),
            TypeKind::Enum => Some(Ty::Enum(def.name.clone(), Vec::new())),
            TypeKind::Record | TypeKind::Error => None,
        }
    }

    pub(super) fn ensure_self_rebinding_types(
        &mut self,
        pattern: &TypedPattern,
        span: &Span,
    ) -> Result<(), TypeError> {
        let expected_self = self.current_impl_self_ty();
        self.ensure_self_rebinding_types_inner(pattern, span, expected_self.as_ref())
    }

    pub(super) fn ensure_self_rebinding_types_inner(
        &mut self,
        pattern: &TypedPattern,
        span: &Span,
        expected_self: Option<&Ty>,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedPattern::Var(bind_ty, id) => {
                if id.name == "self" {
                    let Some(expected) = expected_self else {
                        return Err(TypeError {
                            message: "`self` can only be rebound inside impl methods".to_string(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !self.types_compatible(expected, bind_ty) {
                        return Err(TypeError {
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
                            message: "`self` can only be rebound inside impl methods".to_string(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !self.types_compatible(expected, alias_ty) {
                        return Err(TypeError {
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
            TypedPattern::ResultOk(_, inner) => {
                self.ensure_self_rebinding_types_inner(inner, span, expected_self)
            }
            TypedPattern::Extractor { items, .. } => {
                for item in items {
                    self.ensure_self_rebinding_types_inner(item, span, expected_self)?;
                }
                Ok(())
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _) => Ok(()),
        }
    }

    pub(super) fn ensure_struct_impl_new_contract(
        &self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut struct_decl_spans: HashMap<String, Span> = HashMap::new();
        let mut structs_with_new: HashSet<String> = HashSet::new();

        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, _) => {
                    struct_decl_spans.insert(id.name.clone(), id.span.clone());
                }
                Resolved::Def(_, id, _, _, _, _) => {
                    if let Some((target, method)) = Self::split_impl_method_name(&id.name) {
                        if method == "new" {
                            structs_with_new.insert(target);
                        }
                    }
                }
                _ => {}
            }
        }

        for (struct_name, span) in struct_decl_spans {
            if !structs_with_new.contains(&struct_name) {
                return Err(TypeError {
                    message: format!(
                        "Struct `{}` must define `new` in its impl block (e.g. `impl {} {{ def new(...) -> Self {{ ... }} }}`)",
                        struct_name, struct_name
                    ),
                    span,
                    hint: None,
                });
            }
        }

        Ok(())
    }

    pub(super) fn register_function_id(&mut self, id: &ResolvedId) {
        let key = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        self.function_ids_by_name.insert(key, id.clone());
    }

    pub(super) fn predeclare_functions(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut fun_idx = self.env.next_fun_idx;

        for stmt in stmts {
            match stmt {
                Resolved::BuiltinDecl(_, id, params, ret_ty, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    let param_tys = params
                        .iter()
                        .map(|param| {
                            self.resolve_builtin_ast_ty_in_context(
                                &param.ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let ret = match ret_ty {
                        Some(ty) => self.resolve_builtin_ast_ty_in_context(
                            ty,
                            TypeSyntaxContext::FunctionReturn,
                            &mut tyvars,
                        )?,
                        None => Ty::Unit,
                    };
                    self.env.bind_var(
                        id.unique_id,
                        Ty::BuiltinFunc {
                            name: id.name.clone(),
                            params: param_tys,
                            ret: Box::new(ret),
                        },
                    );
                }
                Resolved::BuiltinExtractorDecl(_, id, param, ret_ty, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    let param_ty = match &param.ty {
                        Some(ty) => self.resolve_builtin_ast_ty_in_context(
                            ty,
                            TypeSyntaxContext::General,
                            &mut tyvars,
                        )?,
                        None => self.env.fresh_tyvar(),
                    };
                    let ret = self.resolve_builtin_ast_ty_in_context(
                        ret_ty,
                        TypeSyntaxContext::FunctionReturn,
                        &mut tyvars,
                    )?;
                    self.env.bind_var(
                        id.unique_id,
                        Ty::BuiltinFunc {
                            name: id.name.clone(),
                            params: vec![param_ty],
                            ret: Box::new(ret),
                        },
                    );
                }
                Resolved::Def(_, id, params, ret_ty, _, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    let param_tys = params
                        .iter()
                        .map(|param| {
                            self.resolve_signature_ast_ty_in_context(
                                &param.ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let param_names = params
                        .iter()
                        .map(|param| param.id.name.clone())
                        .collect::<Vec<_>>();
                    let ret = match ret_ty {
                        Some(ty) => self.resolve_signature_ast_ty_in_context(
                            ty,
                            TypeSyntaxContext::FunctionReturn,
                            &mut tyvars,
                        )?,
                        None => Ty::Unit,
                    };
                    let type_params = tyvars
                        .values()
                        .filter_map(|ty| match ty {
                            Ty::Var(var) => Some(*var),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    self.env.bind_var(
                        id.unique_id,
                        Ty::UserFunc {
                            fun_idx,
                            type_params,
                            params: param_tys,
                            ret: Box::new(ret),
                        },
                    );
                    self.user_func_params.insert(id.unique_id, param_names);
                    if Self::split_impl_method_name(&id.name).is_some() {
                        self.impl_method_uids.insert(id.name.clone(), id.unique_id);
                    }
                    fun_idx += 1;
                }
                Resolved::ExtractorDef(_, id, param, ret_ty, _, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    let param_ty = match &param.ty {
                        Some(ty) => self.resolve_signature_ast_ty_in_context(
                            ty,
                            TypeSyntaxContext::General,
                            &mut tyvars,
                        )?,
                        None => self.env.fresh_tyvar(),
                    };
                    let ret = self.resolve_signature_ast_ty_in_context(
                        ret_ty,
                        TypeSyntaxContext::FunctionReturn,
                        &mut tyvars,
                    )?;
                    let type_params = tyvars
                        .values()
                        .filter_map(|ty| match ty {
                            Ty::Var(var) => Some(*var),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    self.env.bind_var(
                        id.unique_id,
                        Ty::UserFunc {
                            fun_idx,
                            type_params,
                            params: vec![param_ty],
                            ret: Box::new(ret),
                        },
                    );
                    fun_idx += 1;
                }
                Resolved::DeferrorDef(_, id, fields, _) => {
                    self.register_function_id(id);
                    let param_tys = fields
                        .iter()
                        .map(|field| {
                            self.resolve_ast_ty_in_context(&field.ty, TypeSyntaxContext::General)
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                    self.env.bind_var(
                        id.unique_id,
                        Ty::UserFunc {
                            fun_idx,
                            type_params: Vec::new(),
                            params: param_tys,
                            ret: Box::new(Ty::Error),
                        },
                    );
                    self.env.register_error_constructor(id.unique_id);
                    fun_idx += 1;
                }
                Resolved::BuiltinTypeDecl(_, _, _, _) => {}
                Resolved::ResultCtorDecl(_, _, _, _, _) => {}
                _ => {}
            }
        }

        self.env.next_fun_idx = fun_idx;
        Ok(())
    }
}
