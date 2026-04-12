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
                Resolved::Def(_, id, _, _, _, _, _) => {
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

    pub(super) fn trait_key(&self, id: &ResolvedId) -> String {
        id.qualified_name.clone().unwrap_or_else(|| id.name.clone())
    }

    pub(super) fn trait_display_name(&self, trait_name: &str) -> String {
        self.traits
            .get(trait_name)
            .map(|info| info.id.name.clone())
            .unwrap_or_else(|| {
                trait_name
                    .rsplit("::")
                    .next()
                    .unwrap_or(trait_name)
                    .to_string()
            })
    }

    pub(super) fn trait_key_by_short_name(&self, short_name: &str) -> Option<String> {
        self.traits
            .values()
            .find(|info| info.id.name == short_name)
            .map(|info| self.trait_key(&info.id))
    }

    pub(super) fn trait_matches_short_name(&self, trait_name: &str, short_name: &str) -> bool {
        self.trait_key_by_short_name(short_name)
            .as_deref()
            .is_some_and(|key| key == trait_name)
    }

    fn compiler_trait_target_names(&self, trait_name: &str) -> &'static [&'static str] {
        if self.trait_matches_short_name(trait_name, "Numeric") {
            return &["Float", "Int"];
        }
        if self.trait_matches_short_name(trait_name, "Concat") {
            return &["String"];
        }
        if self.trait_matches_short_name(trait_name, "Ord") {
            return &["Float", "Int"];
        }
        if self.trait_matches_short_name(trait_name, "Eq") {
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
            Ty::Struct(name, _) | Ty::Record(name, _) => Some(name),
            Ty::Enum(name, args) if args.is_empty() => Some(name),
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
        let Some(numeric_trait) = self.trait_key_by_short_name("Numeric") else {
            return None;
        };
        if trait_name != numeric_trait || !matches!(target_name, "Int" | "Float") {
            return None;
        }
        match method_name {
            "add" => Some(TraitDispatchTarget::BinOp(BinOp::Add)),
            "sub" => Some(TraitDispatchTarget::BinOp(BinOp::Sub)),
            "mul" => Some(TraitDispatchTarget::BinOp(BinOp::Mul)),
            "safe_div" => Some(TraitDispatchTarget::Builtin("safe_div".into())),
            _ => None,
        }
    }

    fn compiler_trait_impl_exists(&self, trait_name: &str, ty: &Ty) -> bool {
        let ty = self.resolve_ty(ty);
        if self.trait_matches_short_name(trait_name, "Show") {
            return !matches!(ty, Ty::Var(_));
        }
        if self.trait_matches_short_name(trait_name, "Eq") {
            return matches!(
                ty,
                Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Enum(_, _)
            );
        }
        if self.trait_matches_short_name(trait_name, "Ord") {
            return matches!(ty, Ty::Int | Ty::Float);
        }
        if self.trait_matches_short_name(trait_name, "Concat") {
            return matches!(ty, Ty::Str);
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
            return (method_name == "to_string" && !matches!(target_ty, Ty::Var(_)))
                .then(|| TraitDispatchTarget::Builtin("to_string".into()));
        }
        if self.trait_matches_short_name(trait_name, "Eq") {
            return match (method_name, target_ty) {
                ("eq", Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Enum(_, _)) => {
                    Some(TraitDispatchTarget::BinOp(BinOp::Eq))
                }
                ("neq", Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Enum(_, _)) => {
                    Some(TraitDispatchTarget::BinOp(BinOp::Neq))
                }
                _ => None,
            };
        }
        if self.trait_matches_short_name(trait_name, "Ord") {
            return match (method_name, target_ty) {
                ("lt", Ty::Int | Ty::Float) => Some(TraitDispatchTarget::BinOp(BinOp::Lt)),
                ("lte", Ty::Int | Ty::Float) => Some(TraitDispatchTarget::BinOp(BinOp::Lte)),
                ("gt", Ty::Int | Ty::Float) => Some(TraitDispatchTarget::BinOp(BinOp::Gt)),
                ("gte", Ty::Int | Ty::Float) => Some(TraitDispatchTarget::BinOp(BinOp::Gte)),
                _ => None,
            };
        }
        if self.trait_matches_short_name(trait_name, "Concat") {
            return match (method_name, target_ty) {
                ("concat", Ty::Str) => Some(TraitDispatchTarget::BinOp(BinOp::Concat)),
                _ => None,
            };
        }
        None
    }

    pub(super) fn resolve_trait_method_signature(
        &mut self,
        method: &TraitMethodInfo,
        self_ty: &Ty,
    ) -> Result<(Vec<Ty>, Ty), TypeError> {
        let mut tyvars = HashMap::new();
        self.seed_signature_type_params(&method.type_params, &mut tyvars);
        let params = method
            .params
            .iter()
            .map(|param| {
                self.resolve_trait_signature_ast_ty_in_context(
                    &param.ty,
                    TypeSyntaxContext::General,
                    self_ty,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret = self.resolve_trait_signature_ast_ty_in_context(
            &method.ret_ty,
            TypeSyntaxContext::FunctionReturn,
            self_ty,
            &mut tyvars,
        )?;
        Ok((params, ret))
    }

    pub(super) fn resolve_trait_impl_method_signature(
        &mut self,
        method: &TraitImplMethodInfo,
        self_ty: &Ty,
        fallback_ret_ty: &AstTy,
    ) -> Result<(Vec<Ty>, Ty, Vec<u32>), TypeError> {
        let mut tyvars = HashMap::new();
        self.seed_signature_type_params(&method.type_params, &mut tyvars);
        let params = method
            .params
            .iter()
            .map(|param| {
                self.resolve_trait_signature_ast_ty_in_context(
                    &param.ty,
                    TypeSyntaxContext::General,
                    self_ty,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret_source = method.ret_ty.as_ref().unwrap_or(fallback_ret_ty);
        let ret = self.resolve_trait_signature_ast_ty_in_context(
            ret_source,
            TypeSyntaxContext::FunctionReturn,
            self_ty,
            &mut tyvars,
        )?;
        let type_params = method
            .type_params
            .iter()
            .filter_map(|param| match tyvars.get(&param.name) {
                Some(Ty::Var(var)) => Some(*var),
                _ => None,
            })
            .collect::<Vec<_>>();
        Ok((params, ret, type_params))
    }

    pub(super) fn predeclare_traits(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        for stmt in stmts {
            let Resolved::TraitDef(span, id, methods, _) = stmt else {
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
                    methods: method_map,
                },
            );
            let _ = span;
        }

        for stmt in stmts {
            let Resolved::TraitImplDef(span, trait_id, target_ast_ty, methods) = stmt else {
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
            let target_ty =
                self.resolve_ast_ty_in_context(target_ast_ty, TypeSyntaxContext::General)?;
            let target_name = self.trait_target_name(&target_ty).ok_or_else(|| TypeError {
                message: "trait impl target must be a concrete named type".into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: Some("Use `impl Trait for Int` / `impl Trait for Float` / `impl Trait for UserType`.".into()),
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
                            &trait_key,
                            &method.method_name,
                            &target_name,
                        ),
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

                let (trait_params, trait_ret) =
                    self.resolve_trait_method_signature(trait_method, &target_ty)?;
                let (impl_params, impl_ret, _) = self.resolve_trait_impl_method_signature(
                    impl_method,
                    &target_ty,
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

            self.trait_impls.insert(
                (trait_key, target_name.clone()),
                TraitImplInfo {
                    trait_id: trait_id.clone(),
                    target_name,
                    target_ty,
                    methods: method_map,
                },
            );
        }

        Ok(())
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
                Resolved::Def(_, id, type_params, params, ret_ty, _, _) => {
                    self.register_function_id(id);
                    let mut tyvars = HashMap::new();
                    self.seed_signature_type_params(type_params, &mut tyvars);
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

        let mut trait_impls = self.trait_impls.values().cloned().collect::<Vec<_>>();
        trait_impls.sort_by(|left, right| {
            let left_key = (self.trait_key(&left.trait_id), left.target_name.clone());
            let right_key = (self.trait_key(&right.trait_id), right.target_name.clone());
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
                    method,
                    &trait_impl.target_ty,
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
                if Self::split_impl_method_name(&method.function_id.name).is_some() {
                    self.impl_method_uids.insert(
                        method.function_id.name.clone(),
                        method.function_id.unique_id,
                    );
                }
                fun_idx += 1;
            }
        }

        self.env.next_fun_idx = fun_idx;
        Ok(())
    }
}
