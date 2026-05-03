use super::*;
use sindr::builtin::builtin_type_meta_by_name;

impl Checker {
    fn struct_new_contract_error(
        &self,
        struct_name: &str,
        span: &Span,
        actual: Option<&Ty>,
    ) -> TypeError {
        let actual_suffix = actual
            .map(|ty| format!("; got {}", self.ty_name(ty)))
            .unwrap_or_default();
        TypeError {
            message: format!(
                "Struct `{}::{}` `new` must return Self or Result<Self, E>{}",
                struct_name, "new", actual_suffix
            ),
            span: span.clone(),
            hint: Some(format!(
                "Define `impl {} {{ def new(...) -> Self {{ ... }} }}` or `impl {} {{ def new(...) -> Result<Self, Error> {{ ... }} }}`.",
                struct_name, struct_name
            )),
        }
    }

    fn struct_new_return_allowed(&mut self, expected_self_ty: &Ty, ret_ty: &Ty) -> bool {
        let resolved_ret = self.resolve_ty(ret_ty);
        if self.types_compatible(expected_self_ty, &resolved_ret) {
            return true;
        }
        match resolved_ret {
            Ty::Result(ok, _) => self.types_compatible(expected_self_ty, ok.as_ref()),
            _ => false,
        }
    }

    pub(super) fn predeclare_error_types(&mut self, stmts: &[Resolved]) {
        for stmt in stmts {
            if let Resolved::DeferrorDef(_, id, _, _) = stmt {
                self.env.declare_error_type_name(id.name.clone());
            }
        }
    }

    fn const_surface_is_allowed(&self, value: &Resolved) -> bool {
        match value {
            Resolved::Lit(_, _) => true,
            Resolved::Var(_, id) => self
                .consts
                .get(&id.unique_id)
                .is_none_or(|meta| matches!(meta.kind, ConstKind::LensPath)),
            Resolved::FieldAccess(_, inner, _) => self.const_surface_is_allowed(inner),
            Resolved::BinOp(_, BinOp::Slash, left, right) => {
                self.const_surface_is_allowed(left) && self.const_surface_is_allowed(right)
            }
            _ => false,
        }
    }

    pub(super) fn predeclare_consts(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        for stmt in stmts {
            let Resolved::ConstDef(span, id, ast_ty, value, attrs) = stmt else {
                continue;
            };

            if !self.const_surface_is_allowed(value) {
                return Err(TypeError {
                    message: "const value must be a primitive literal or a lens path".into(),
                    span: span.clone(),
                    hint: Some(
                        "V1 const supports literal values, lens paths, Lens const refs, and `/` composition of those lens values only.".into(),
                    ),
                });
            }

            let expected_ty = ast_ty
                .as_ref()
                .map(|ty| self.resolve_ast_ty_in_context(ty, TypeSyntaxContext::BindingAnnotation))
                .transpose()?;
            let checked = self.check_node_with_expected(value, expected_ty.as_ref())?;
            let typed = self.resolve_typed_node(checked);

            let (kind, stored) = match &typed.node {
                TypedInner::Lit(lit) => (
                    ConstKind::PrimitiveLiteral,
                    StoredConstValue::Literal(lit.clone()),
                ),
                TypedInner::LensPath(path) => (
                    ConstKind::LensPath,
                    StoredConstValue::LensPath(path.clone()),
                ),
                _ => {
                    return Err(TypeError {
                        message: "const value must be a primitive literal or a lens path".into(),
                        span: span.clone(),
                        hint: Some(
                            "Use `const NAME = 1`, `const NAME = User.profile`, or compose Lens consts with `/`.".into(),
                        ),
                    })
                }
            };

            self.env.bind_var(id.unique_id, typed.ty.clone());
            self.consts.insert(
                id.unique_id,
                ConstMeta {
                    name: id.name.clone(),
                    visibility: attrs.visibility,
                    ty: typed.ty.clone(),
                    kind,
                    value: stored,
                    span: span.clone(),
                },
            );
        }
        Ok(())
    }

    pub(super) fn predeclare_type_signatures(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut seen_type_spans: HashMap<String, Span> = HashMap::new();
        // Pass 1: reserve deterministic tags for all user-defined types.
        for stmt in stmts {
            let maybe_decl = match stmt {
                Resolved::StructDef(_, id, _) => {
                    Some((&id.name, &id.span, TypeKind::Struct, Vec::new()))
                }
                Resolved::RecordDef(_, id, _) => {
                    Some((&id.name, &id.span, TypeKind::Record, Vec::new()))
                }
                Resolved::DeferrorDef(_, id, _, _) => {
                    Some((&id.name, &id.span, TypeKind::ConcreteError, Vec::new()))
                }
                Resolved::EnumDef(_, id, type_params, _) => Some((
                    &id.name,
                    &id.span,
                    TypeKind::Enum,
                    type_params
                        .iter()
                        .map(|param| param.name.clone())
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            };

            let Some((name, span, kind, type_params)) = maybe_decl else {
                continue;
            };

            if builtin_type_meta_by_name(name).is_some() {
                return Err(TypeError {
                    message: format!(
                        "Type name `{}` is reserved by a canonical builtin type declaration",
                        name
                    ),
                    span: span.clone(),
                    hint: Some("Builtin and canonical type names cannot be redefined.".into()),
                });
            }

            if let Some(first_span) = seen_type_spans.get(name) {
                return Err(TypeError {
                    message: format!(
                        "Duplicate visible type name `{}` in the flat type namespace",
                        name
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "The first declaration was at {}..{}.",
                        first_span.start, first_span.end
                    )),
                });
            }
            seen_type_spans.insert(name.clone(), span.clone());

            self.env
                .predeclare_type_def(name.clone(), kind, type_params);
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
                    let private_fields = fields
                        .iter()
                        .filter(|field| field.visibility == spire::ast::Visibility::Private)
                        .map(|field| field.name.clone())
                        .collect::<HashSet<_>>();
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields.clone(), private_fields)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env.register_type_constructor_id(id.unique_id);
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
                    let private_fields = fields
                        .iter()
                        .filter(|field| field.visibility == spire::ast::Visibility::Private)
                        .map(|field| field.name.clone())
                        .collect::<HashSet<_>>();
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields.clone(), private_fields)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env.register_type_constructor_id(id.unique_id);
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
                    let private_fields = fields
                        .iter()
                        .filter(|field| field.visibility == spire::ast::Visibility::Private)
                        .map(|field| field.name.clone())
                        .collect::<HashSet<_>>();
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields, private_fields)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                }
                Resolved::EnumDef(_, id, type_params, variants) => {
                    let _ = self
                        .env
                        .resolve_type_def_signature(&id.name, Vec::new(), HashSet::new())
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
                    self.env.register_type_constructor_id(id.unique_id);

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

    pub(super) fn split_impl_method_id(id: &ResolvedId) -> Option<(String, String)> {
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
            TypeKind::Record | TypeKind::ConcreteError => None,
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
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    self.ensure_self_rebinding_types_inner(item, span, expected_self)?;
                }
                Ok(())
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
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => Ok(()),
        }
    }

    pub(super) fn ensure_struct_impl_new_contract(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut struct_defs: HashMap<String, (Span, Ty)> = HashMap::new();
        let mut structs_with_new: HashSet<String> = HashSet::new();

        for stmt in stmts {
            if let Resolved::StructDef(_, id, fields) = stmt {
                let expected_self_ty = Ty::Struct(
                    id.name.clone(),
                    fields
                        .iter()
                        .map(|field| {
                            Ok((
                                field.name.clone(),
                                self.resolve_ast_ty_in_context(
                                    &field.ty,
                                    TypeSyntaxContext::General,
                                )?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?,
                );
                struct_defs.insert(id.name.clone(), (id.span.clone(), expected_self_ty));
            }
        }

        for stmt in stmts {
            let Resolved::Def(_, id, _, _, _, _, _) = stmt else {
                continue;
            };
            if let Some((target, method)) = Self::split_impl_method_id(id) {
                if method == "new" {
                    structs_with_new.insert(target.clone());
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
                    if !self.struct_new_return_allowed(expected_self_ty, &ret_ty) {
                        return Err(self.struct_new_contract_error(&target, span, Some(&ret_ty)));
                    }
                }
            }
        }

        for (struct_name, (span, _)) in struct_defs {
            if !structs_with_new.contains(&struct_name) {
                return Err(TypeError {
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

    pub(super) fn register_function_id(&mut self, id: &ResolvedId) {
        let key = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        self.function_ids_by_name.insert(key, id.clone());
    }

    pub(super) fn trait_key(&self, id: &ResolvedId) -> String {
        id.qualified_name.clone().unwrap_or_else(|| id.name.clone())
    }

    pub(super) fn ast_ty_key(ast_ty: &AstTy) -> String {
        match ast_ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                name,
                args.iter()
                    .map(Self::ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => format!(
                "({} -> {})",
                params
                    .iter()
                    .map(Self::ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::ast_ty_key(ret)
            ),
        }
    }

    pub(super) fn trait_instance_key(&self, trait_id: &ResolvedId, trait_args: &[AstTy]) -> String {
        let base = self.trait_key(trait_id);
        if trait_args.is_empty() {
            base
        } else {
            format!(
                "{}<{}>",
                base,
                trait_args
                    .iter()
                    .map(Self::ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    pub(super) fn trait_display_name(&self, trait_name: &str) -> String {
        let (base, suffix) = trait_name
            .split_once('<')
            .map(|(base, suffix)| (base, Some(suffix)))
            .unwrap_or((trait_name, None));
        let display = self
            .traits
            .get(base)
            .map(|info| info.id.name.clone())
            .unwrap_or_else(|| base.rsplit("::").next().unwrap_or(base).to_string());
        match suffix {
            Some(suffix) => format!("{}<{}", display, suffix),
            None => display,
        }
    }

    pub(super) fn trait_key_by_short_name(&self, short_name: &str) -> Option<String> {
        self.traits
            .values()
            .find(|info| info.id.name == short_name)
            .map(|info| self.trait_key(&info.id))
    }

    pub(super) fn trait_matches_short_name(&self, trait_name: &str, short_name: &str) -> bool {
        let base = trait_name
            .split_once('<')
            .map(|(base, _)| base)
            .unwrap_or(trait_name);
        self.trait_key_by_short_name(short_name)
            .as_deref()
            .is_some_and(|key| key == base)
    }

    pub(super) fn trait_instance_key_from_tys(
        &self,
        trait_name: &str,
        trait_args: &[Ty],
    ) -> String {
        if trait_args.is_empty() {
            trait_name.to_string()
        } else {
            format!(
                "{}<{}>",
                trait_name,
                trait_args
                    .iter()
                    .map(|ty| self.ty_name(&self.resolve_ty(ty)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    fn collect_ty_vars(ty: &Ty, out: &mut Vec<u32>) {
        match ty {
            Ty::Var(var) => {
                if !out.contains(var) {
                    out.push(*var);
                }
            }
            Ty::List(inner) | Ty::TypeRef(inner) => Self::collect_ty_vars(inner, out),
            Ty::Lens(source, focus) | Ty::Result(source, focus) => {
                Self::collect_ty_vars(source, out);
                Self::collect_ty_vars(focus, out);
            }
            Ty::Tuple(items) | Ty::Enum(_, items) => {
                for item in items {
                    Self::collect_ty_vars(item, out);
                }
            }
            Ty::Func(params, ret) => {
                for param in params {
                    Self::collect_ty_vars(param, out);
                }
                Self::collect_ty_vars(ret, out);
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                for param in params {
                    Self::collect_ty_vars(param, out);
                }
                Self::collect_ty_vars(ret, out);
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => {
                for (_, field_ty) in fields {
                    Self::collect_ty_vars(field_ty, out);
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

    pub(super) fn resolve_trait_impl_head_tys(
        &mut self,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
    ) -> Result<(Vec<Ty>, Ty, Vec<u32>), TypeError> {
        let mut tyvars = HashMap::new();
        let placeholder_self = self.env.fresh_tyvar();
        let target_ty = self.resolve_trait_signature_ast_ty_in_context(
            target_ast_ty,
            TypeSyntaxContext::General,
            &placeholder_self,
            &mut tyvars,
        )?;
        let trait_arg_tys = trait_args
            .iter()
            .map(|arg| {
                self.resolve_trait_signature_ast_ty_in_context(
                    arg,
                    TypeSyntaxContext::General,
                    &target_ty,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut type_param_vars = Vec::new();
        for ty in &trait_arg_tys {
            Self::collect_ty_vars(ty, &mut type_param_vars);
        }
        Self::collect_ty_vars(&target_ty, &mut type_param_vars);
        Ok((trait_arg_tys, target_ty, type_param_vars))
    }

    fn compiler_trait_target_names(&self, trait_name: &str) -> &'static [&'static str] {
        if self.trait_matches_short_name(trait_name, "Add")
            || self.trait_matches_short_name(trait_name, "Lt")
            || self.trait_matches_short_name(trait_name, "Lte")
            || self.trait_matches_short_name(trait_name, "Gt")
            || self.trait_matches_short_name(trait_name, "Gte")
        {
            return &["Float", "Int"];
        }
        if self.trait_matches_short_name(trait_name, "Numeric")
            || self.trait_matches_short_name(trait_name, "Sub")
            || self.trait_matches_short_name(trait_name, "Mul")
        {
            return &["Float", "Int"];
        }
        if self.trait_matches_short_name(trait_name, "Concat") {
            return &["String"];
        }
        if self.trait_matches_short_name(trait_name, "Eq") {
            return &["Boolean", "Float", "Int", "String"];
        }
        if self.trait_matches_short_name(trait_name, "Neq") {
            return &["Boolean", "Float", "Int", "String"];
        }
        if self.trait_matches_short_name(trait_name, "Show") {
            return &["Boolean", "Error", "Float", "Int", "String", "Unit"];
        }
        &[]
    }

    pub(super) fn trait_implementation_targets(&self, trait_name: &str) -> Vec<String> {
        let mut targets = std::collections::BTreeSet::new();
        for target in self.compiler_trait_target_names(trait_name) {
            targets.insert((*target).to_string());
        }
        for (impl_trait_name, target_name) in self.trait_impls.keys() {
            if impl_trait_name == trait_name {
                targets.insert(target_name.clone());
            }
        }
        targets.into_iter().collect()
    }

    pub(super) fn trait_implementation_summary(&self, trait_name: &str) -> String {
        let display_name = self.trait_display_name(trait_name);
        let targets = self.trait_implementation_targets(trait_name);
        if targets.is_empty() {
            format!("{} has no visible implementations", display_name)
        } else {
            format!(
                "{} is implemented for: {}",
                display_name,
                targets.join(", ")
            )
        }
    }

    pub(super) fn tyvar_satisfies_compiler_trait(&self, var: u32, trait_name: &str) -> bool {
        if self.trait_matches_short_name(trait_name, "Show") {
            return self
                .trait_key_by_short_name("Numeric")
                .as_deref()
                .is_some_and(|numeric_trait| self.tyvar_has_bound(var, numeric_trait));
        }
        false
    }

    pub(super) fn trait_target_name(&self, ty: &Ty) -> Option<String> {
        match self.resolve_ty(ty) {
            Ty::Int => Some("Int".into()),
            Ty::Float => Some("Float".into()),
            Ty::Str => Some("String".into()),
            Ty::Bool => Some("Boolean".into()),
            Ty::Unit => Some("Unit".into()),
            Ty::Error => Some("Error".into()),
            Ty::Pid(name) => Some(format!("PID<{name}>")),
            Ty::Result(_, _) => Some("Result".into()),
            Ty::List(_) => Some("List".into()),
            Ty::Lens(_, _) => Some("Lens".into()),
            Ty::Func(_, _) => Some("Function".into()),
            Ty::Struct(name, _) | Ty::Record(name, _) => Some(name),
            Ty::Enum(name, _) => Some(name),
            _ => None,
        }
    }

    pub(super) fn trait_impl_exists(&self, trait_name: &str, ty: &Ty) -> bool {
        if self.trait_target_name(ty).is_some_and(|target_name| {
            self.trait_impls
                .contains_key(&(trait_name.into(), target_name))
        }) {
            return true;
        }
        self.compiler_trait_impl_exists(trait_name, ty)
    }

    pub(super) fn trait_dispatch_override(
        &self,
        trait_name: &str,
        method_name: &str,
        target_name: &str,
    ) -> Option<TraitDispatchTarget> {
        if matches!(target_name, "Int" | "Float") {
            let op = if self.trait_matches_short_name(trait_name, "Add") && method_name == "add" {
                Some(BinOp::Add)
            } else if self.trait_matches_short_name(trait_name, "Sub") && method_name == "sub" {
                Some(BinOp::Sub)
            } else if self.trait_matches_short_name(trait_name, "Mul") && method_name == "mul" {
                Some(BinOp::Mul)
            } else if self.trait_matches_short_name(trait_name, "Lt") && method_name == "lt" {
                Some(BinOp::Lt)
            } else if self.trait_matches_short_name(trait_name, "Lte") && method_name == "lte" {
                Some(BinOp::Lte)
            } else if self.trait_matches_short_name(trait_name, "Gt") && method_name == "gt" {
                Some(BinOp::Gt)
            } else if self.trait_matches_short_name(trait_name, "Gte") && method_name == "gte" {
                Some(BinOp::Gte)
            } else {
                None
            };
            if let Some(op) = op {
                return Some(TraitDispatchTarget::BinOp(op));
            }
        }
        if self.trait_matches_short_name(trait_name, "Numeric")
            && matches!(target_name, "Int" | "Float")
            && method_name == "safe_div"
        {
            return Some(TraitDispatchTarget::Builtin("safe_div".into()));
        }
        if self.trait_matches_short_name(trait_name, "Show")
            && matches!(
                target_name,
                "Int" | "Float" | "String" | "Boolean" | "Unit" | "Error"
            )
        {
            return (method_name == "to_string")
                .then(|| TraitDispatchTarget::Builtin("to_string".into()));
        }
        if self.trait_matches_short_name(trait_name, "Eq")
            && matches!(target_name, "Int" | "Float" | "String" | "Boolean")
            && method_name == "eq"
        {
            return Some(TraitDispatchTarget::BinOp(BinOp::Eq));
        }
        if self.trait_matches_short_name(trait_name, "Neq")
            && matches!(target_name, "Int" | "Float" | "String" | "Boolean")
            && method_name == "neq"
        {
            return Some(TraitDispatchTarget::BinOp(BinOp::Neq));
        }
        if self.trait_matches_short_name(trait_name, "Concat") && target_name == "String" {
            return (method_name == "concat").then(|| TraitDispatchTarget::BinOp(BinOp::Concat));
        }
        None
    }

    fn compiler_trait_impl_exists(&self, trait_name: &str, ty: &Ty) -> bool {
        let ty = self.resolve_ty(ty);
        if self.trait_matches_short_name(trait_name, "Show") {
            return !matches!(
                ty,
                Ty::Var(_) | Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Unit | Ty::Error
            );
        }
        if self.trait_matches_short_name(trait_name, "Eq")
            || self.trait_matches_short_name(trait_name, "Neq")
        {
            return matches!(ty, Ty::Enum(_, _));
        }
        false
    }

    pub(super) fn compiler_trait_dispatch_target(
        &self,
        trait_name: &str,
        method_name: &str,
        target_ty: &Ty,
    ) -> Option<TraitDispatchTarget> {
        let target_ty = self.resolve_ty(target_ty);
        if self.trait_matches_short_name(trait_name, "Show") {
            return (method_name == "to_string"
                && !matches!(
                    target_ty,
                    Ty::Var(_) | Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Unit | Ty::Error
                ))
            .then(|| TraitDispatchTarget::Builtin("to_string".into()));
        }
        if self.trait_matches_short_name(trait_name, "Eq") {
            return match (method_name, target_ty) {
                ("eq", Ty::Enum(_, _)) => Some(TraitDispatchTarget::BinOp(BinOp::Eq)),
                _ => None,
            };
        }
        if self.trait_matches_short_name(trait_name, "Neq") {
            return match (method_name, target_ty) {
                ("neq", Ty::Enum(_, _)) => Some(TraitDispatchTarget::BinOp(BinOp::Neq)),
                _ => None,
            };
        }
        None
    }

    pub(super) fn resolve_trait_method_signature(
        &mut self,
        trait_info: &TraitInfo,
        method: &TraitMethodInfo,
        self_ty: &Ty,
    ) -> Result<(Vec<Ty>, Ty, Vec<Ty>), TypeError> {
        let mut trait_head_bindings = HashMap::new();
        for param in &trait_info.type_params {
            let fresh = self.env.fresh_tyvar();
            if let Ty::Var(var) = fresh {
                if let Some(bound) = &param.bound {
                    self.register_tyvar_bound(var, bound);
                }
            }
            trait_head_bindings.insert(param.name.clone(), fresh);
        }
        let mut tyvars = trait_head_bindings.clone();
        self.seed_signature_type_params(&method.type_params, &mut tyvars);
        let params = method
            .params
            .iter()
            .map(|param| {
                if let Some(ty) = self.resolve_trait_type_ref_param_ty(
                    &param.ty,
                    &trait_head_bindings,
                    false,
                    self_ty,
                    &mut tyvars,
                )? {
                    Ok(ty)
                } else {
                    self.resolve_trait_signature_ast_ty_in_context(
                        &param.ty,
                        TypeSyntaxContext::General,
                        self_ty,
                        &mut tyvars,
                    )
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret = self.resolve_trait_signature_ast_ty_in_context(
            &method.ret_ty,
            TypeSyntaxContext::FunctionReturn,
            self_ty,
            &mut tyvars,
        )?;
        let trait_args = trait_info
            .type_params
            .iter()
            .filter_map(|param| trait_head_bindings.get(&param.name).cloned())
            .collect::<Vec<_>>();
        Ok((params, ret, trait_args))
    }

    pub(super) fn resolve_trait_impl_method_signature(
        &mut self,
        trait_info: &TraitInfo,
        trait_args: &[AstTy],
        method: &TraitImplMethodInfo,
        target_ast_ty: &AstTy,
        fallback_ret_ty: &AstTy,
    ) -> Result<(Vec<Ty>, Ty, Vec<u32>), TypeError> {
        if trait_info.type_params.len() != trait_args.len() {
            return Err(TypeError {
                message: format!(
                    "Trait {} requires {} type argument(s), got {}",
                    trait_info.id.name,
                    trait_info.type_params.len(),
                    trait_args.len()
                ),
                span: method.span.clone(),
                hint: None,
            });
        }

        let mut trait_head_bindings = HashMap::new();
        let mut tyvars = HashMap::new();
        let placeholder_self = self.env.fresh_tyvar();
        let self_ty = self.resolve_trait_signature_ast_ty_in_context(
            target_ast_ty,
            TypeSyntaxContext::General,
            &placeholder_self,
            &mut tyvars,
        )?;
        for (param, arg) in trait_info.type_params.iter().zip(trait_args.iter()) {
            let resolved = self.resolve_trait_signature_ast_ty_in_context(
                arg,
                TypeSyntaxContext::General,
                &self_ty,
                &mut tyvars,
            )?;
            trait_head_bindings.insert(param.name.clone(), resolved);
        }

        tyvars.extend(trait_head_bindings.clone());
        self.seed_signature_type_params(&method.type_params, &mut tyvars);
        let params = method
            .params
            .iter()
            .map(|param| {
                if let Some(ty) = self.resolve_trait_type_ref_param_ty(
                    &param.ty,
                    &trait_head_bindings,
                    true,
                    &self_ty,
                    &mut tyvars,
                )? {
                    Ok(ty)
                } else {
                    self.resolve_trait_signature_ast_ty_in_context(
                        &param.ty,
                        TypeSyntaxContext::General,
                        &self_ty,
                        &mut tyvars,
                    )
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret_source = method.ret_ty.as_ref().unwrap_or(fallback_ret_ty);
        let ret = self.resolve_trait_signature_ast_ty_in_context(
            ret_source,
            TypeSyntaxContext::FunctionReturn,
            &self_ty,
            &mut tyvars,
        )?;
        let mut type_params = Vec::new();
        for ty in tyvars.values() {
            Self::collect_ty_vars(ty, &mut type_params);
        }
        for var in method
            .type_params
            .iter()
            .filter_map(|param| match tyvars.get(&param.name) {
                Some(Ty::Var(var)) => Some(*var),
                _ => None,
            })
        {
            if !type_params.contains(&var) {
                type_params.push(var);
            }
        }
        Ok((params, ret, type_params))
    }

    pub(super) fn predeclare_traits(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        for stmt in stmts {
            let Resolved::TraitDef(span, id, type_params, methods, _) = stmt else {
                continue;
            };
            let trait_key = self.trait_key(id);
            let mut method_map = HashMap::new();
            for method in methods {
                if let Some(qualified_name) = &method.id.qualified_name {
                    self.trait_methods_by_qualified_name.insert(
                        qualified_name.clone(),
                        (trait_key.clone(), method.id.name.clone()),
                    );
                }
                method_map.insert(
                    method.id.name.clone(),
                    TraitMethodInfo {
                        id: method.id.clone(),
                        type_params: method.type_params.clone(),
                        params: method.params.clone(),
                        ret_ty: method.ret_ty.clone(),
                        span: method.span.clone(),
                    },
                );
            }
            self.traits.insert(
                trait_key,
                TraitInfo {
                    id: id.clone(),
                    type_params: type_params.clone(),
                    methods: method_map,
                },
            );
            let _ = span;
        }

        for stmt in stmts {
            let Resolved::TraitImplDef(span, trait_id, trait_args, target_ast_ty, methods) = stmt
            else {
                continue;
            };

            let trait_key = self.trait_key(trait_id);
            let trait_info = self
                .traits
                .get(&trait_key)
                .cloned()
                .ok_or_else(|| TypeError {
                    message: format!("Unknown trait: {}", trait_id.name),
                    span: span.clone(),
                    hint: None,
                })?;
            if trait_info.type_params.len() != trait_args.len() {
                return Err(TypeError {
                    message: format!(
                        "Trait {} requires {} type argument(s), got {}",
                        trait_id.name,
                        trait_info.type_params.len(),
                        trait_args.len()
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
            let trait_instance_key = self.trait_instance_key(trait_id, trait_args);
            let (trait_arg_tys, target_ty, type_param_vars) =
                self.resolve_trait_impl_head_tys(trait_args, target_ast_ty)?;
            let target_name = self.trait_target_name(&target_ty).ok_or_else(|| TypeError {
                message: "trait impl target must be a concrete named type or function type".into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: Some("Use `impl Trait for Int` / `impl Trait for Float` / `impl Trait for UserType` / `impl Trait for ($A -> $B)`.".into()),
            })?;

            let mut method_map = HashMap::new();
            for method in methods {
                method_map.insert(
                    method.method_name.clone(),
                    TraitImplMethodInfo {
                        method_name: method.method_name.clone(),
                        function_id: method.function_id.clone(),
                        type_params: method.type_params.clone(),
                        params: method.params.clone(),
                        ret_ty: method.ret_ty.clone(),
                        body: method.body.clone(),
                        attrs: method.attrs.clone(),
                        span: method.span.clone(),
                        dispatch_override: self.trait_dispatch_override(
                            &trait_instance_key,
                            &method.method_name,
                            &target_name,
                        ),
                        is_builtin: method.is_builtin,
                    },
                );
            }

            for required_method in trait_info.methods.keys() {
                if !method_map.contains_key(required_method) {
                    return Err(TypeError {
                        message: format!(
                            "Trait impl {} for {} is missing method `{}`",
                            trait_id.name, target_name, required_method
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            }

            for method_name in method_map.keys() {
                if !trait_info.methods.contains_key(method_name) {
                    return Err(TypeError {
                        message: format!(
                            "Trait impl {} for {} defines unknown method `{}`",
                            trait_id.name, target_name, method_name
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            }

            for (method_name, impl_method) in &method_map {
                let trait_method = trait_info
                    .methods
                    .get(method_name)
                    .expect("validated above");
                if trait_method.type_params.len() != impl_method.type_params.len() {
                    return Err(TypeError {
                        message: format!(
                            "Trait impl method {}::{} has incompatible type parameter arity",
                            trait_id.name, method_name
                        ),
                        span: impl_method.span.clone(),
                        hint: None,
                    });
                }

                let (trait_params, trait_ret, _) =
                    self.resolve_trait_method_signature(&trait_info, trait_method, &target_ty)?;
                let (impl_params, impl_ret, _) = self.resolve_trait_impl_method_signature(
                    &trait_info,
                    trait_args,
                    impl_method,
                    target_ast_ty,
                    &trait_method.ret_ty,
                )?;

                if trait_params.len() != impl_params.len() {
                    return Err(TypeError {
                        message: format!(
                            "Trait impl method {}::{} has incompatible arity",
                            trait_id.name, method_name
                        ),
                        span: impl_method.span.clone(),
                        hint: None,
                    });
                }

                for (expected, got) in trait_params.iter().zip(&impl_params) {
                    let before = self.substitutions.clone();
                    let compatible = self.types_compatible(expected, got);
                    self.substitutions = before;
                    if !compatible {
                        return Err(TypeError {
                            message: format!(
                                "Trait impl method {}::{} has incompatible parameter type: expected {}, got {}",
                                trait_id.name,
                                method_name,
                                self.ty_name(expected),
                                self.ty_name(got)
                            ),
                            span: impl_method.span.clone(),
                            hint: None,
                        });
                    }
                }

                let before = self.substitutions.clone();
                let ret_compatible = self.types_compatible(&trait_ret, &impl_ret);
                self.substitutions = before;
                if !ret_compatible {
                    return Err(TypeError {
                        message: format!(
                            "Trait impl method {}::{} has incompatible return type: expected {}, got {}",
                            trait_id.name,
                            method_name,
                            self.ty_name(&trait_ret),
                            self.ty_name(&impl_ret)
                        ),
                        span: impl_method.span.clone(),
                        hint: None,
                    });
                }
            }

            let exclusive_peer = if self.trait_matches_short_name(&trait_key, "From") {
                self.trait_key_by_short_name("TryFrom")
            } else if self.trait_matches_short_name(&trait_key, "TryFrom") {
                self.trait_key_by_short_name("From")
            } else {
                None
            };
            if let Some(peer_trait_key) = exclusive_peer {
                let peer_instance_key = if trait_args.is_empty() {
                    peer_trait_key.clone()
                } else {
                    format!(
                        "{}<{}>",
                        peer_trait_key,
                        trait_args
                            .iter()
                            .map(Self::ast_ty_key)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                if self
                    .trait_impls
                    .contains_key(&(peer_instance_key.clone(), target_name.clone()))
                {
                    return Err(TypeError {
                        message: format!(
                            "{} and {} cannot both be implemented for {} -> {}",
                            trait_id.name,
                            peer_trait_key
                                .rsplit("::")
                                .next()
                                .unwrap_or(&peer_trait_key),
                            target_name,
                            trait_args
                                .iter()
                                .map(Self::ast_ty_key)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            }

            let impl_key = (trait_instance_key.clone(), target_name.clone());
            self.trait_impls.insert(
                impl_key.clone(),
                TraitImplInfo {
                    trait_id: trait_id.clone(),
                    trait_args: trait_args.clone(),
                    trait_arg_tys,
                    target_name,
                    target_ast_ty: target_ast_ty.clone(),
                    target_ty,
                    type_param_vars,
                    methods: method_map,
                },
            );
            self.index_trait_impl(impl_key);
        }

        Ok(())
    }

    pub(super) fn predeclare_functions(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut fun_idx = self.env.next_fun_idx;
        let trait_impl_method_ids_in_stmts = stmts
            .iter()
            .filter_map(|stmt| match stmt {
                Resolved::TraitImplDef(_, _, _, _, methods) => Some(
                    methods
                        .iter()
                        .map(|method| method.function_id.unique_id)
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect::<HashSet<_>>();

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
                            name: sindr::builtin::builtin_runtime_name(
                                &id.name,
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
                        TypeSyntaxContext::ExtractorReturn,
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
                Resolved::Def(_, id, type_params, params, ret_ty, _, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    self.seed_signature_type_params(type_params, &mut tyvars);
                    let param_tys = params
                        .iter()
                        .map(|param| {
                            let param_ty = self.resolve_signature_ast_ty_in_context(
                                &param.ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            )?;
                            if !self.allow_error_function_params
                                && !Self::allows_std_error_function_param_exception(id)
                                && Self::ty_exposes_error_value(&param_ty)
                            {
                                return Err(self.error_function_param_not_allowed_error(
                                    Self::ast_ty_span(&param.ty),
                                ));
                            }
                            Ok(param_ty)
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
                    if let Some(qualified_name) = id.qualified_name.as_ref() {
                        if Self::split_impl_method_name(qualified_name).is_some() {
                            self.impl_method_uids
                                .insert(qualified_name.clone(), id.unique_id);
                        }
                    }
                    fun_idx += 1;
                }
                Resolved::ExtractorDef(_, id, type_params, param, ret_ty, _, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    self.seed_signature_type_params(type_params, &mut tyvars);
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
                        TypeSyntaxContext::ExtractorReturn,
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

        let mut trait_impls = self.trait_impls.values().cloned().collect::<Vec<_>>();
        trait_impls.sort_by(|left, right| {
            let left_key = (
                self.trait_instance_key(&left.trait_id, &left.trait_args),
                left.target_name.clone(),
            );
            let right_key = (
                self.trait_instance_key(&right.trait_id, &right.trait_args),
                right.target_name.clone(),
            );
            left_key.cmp(&right_key)
        });

        for trait_impl in trait_impls {
            let trait_key = self.trait_key(&trait_impl.trait_id);
            let trait_info = self
                .traits
                .get(&trait_key)
                .cloned()
                .ok_or_else(|| TypeError {
                    message: format!("Unknown trait: {}", trait_impl.trait_id.name),
                    span: trait_impl.trait_id.span.clone(),
                    hint: None,
                })?;
            let mut methods = trait_impl.methods.iter().collect::<Vec<_>>();
            methods.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));

            for (method_name, method) in methods {
                if !trait_impl_method_ids_in_stmts.contains(&method.function_id.unique_id) {
                    continue;
                }
                if method.is_builtin {
                    continue;
                }
                self.register_function_id(&method.function_id);
                let trait_method =
                    trait_info
                        .methods
                        .get(method_name)
                        .ok_or_else(|| TypeError {
                            message: format!(
                                "Unknown trait method: {}::{}",
                                trait_impl.trait_id.name, method_name
                            ),
                            span: method.span.clone(),
                            hint: None,
                        })?;
                let (param_tys, ret, type_params) = self.resolve_trait_impl_method_signature(
                    &trait_info,
                    &trait_impl.trait_args,
                    method,
                    &trait_impl.target_ast_ty,
                    &trait_method.ret_ty,
                )?;
                let param_names = method
                    .params
                    .iter()
                    .map(|param| param.id.name.clone())
                    .collect::<Vec<_>>();
                self.env.bind_var(
                    method.function_id.unique_id,
                    Ty::UserFunc {
                        fun_idx,
                        type_params,
                        params: param_tys,
                        ret: Box::new(ret),
                    },
                );
                self.user_func_params
                    .insert(method.function_id.unique_id, param_names);
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
