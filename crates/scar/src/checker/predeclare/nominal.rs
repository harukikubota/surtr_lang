use super::*;
use sindr::builtin::builtin_type_meta_by_name;

impl Checker {
    pub(in crate::checker) fn predeclare_error_types(&mut self, stmts: &[Resolved]) {
        for stmt in stmts {
            if let Resolved::DeferrorDef(_, id, _, _) = stmt {
                self.env.declare_error_type_name(id.name.clone());
            }
        }
    }

    /// Resolve the finite graph of declaration parameter roles before reading
    /// schemas. Only marker roles propagate; ordinary field types are not expanded.
    fn predeclare_pid_parameter_roles(&mut self, stmts: &[Resolved]) {
        type Parameter = (String, usize);
        let mut dependents = HashMap::<Parameter, Vec<Parameter>>::new();
        let mut markers = HashSet::<Parameter>::new();
        let mut roots = Vec::new();
        for definition in self.env.type_defs.values() {
            for (index, variable) in definition.type_param_vars.iter().enumerate() {
                if self.process_marker_tyvars.contains(variable) {
                    markers.insert((Self::canonical_user_type_name(&definition.name), index));
                }
            }
        }
        for (name, alias) in &self.signature_aliases {
            roots.push((
                name.clone(),
                alias
                    .params
                    .iter()
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>(),
                vec![&alias.rhs],
            ));
        }
        for statement in stmts {
            let (id, params, inputs) = match statement {
                Resolved::StructDef(_, id, params, fields, _) => {
                    (id, params, fields.iter().map(|f| &f.ty).collect::<Vec<_>>())
                }
                Resolved::EnumDef(_, id, params, variants, _) => (
                    id,
                    params,
                    variants.iter().flat_map(|v| &v.payload).collect::<Vec<_>>(),
                ),
                _ => continue,
            };
            roots.push((
                id.name.clone(),
                params.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
                inputs,
            ));
        }
        for (owner, parameters, mut pending) in roots {
            let own_parameters = parameters
                .into_iter()
                .enumerate()
                .map(|(index, name)| (name, (Self::canonical_user_type_name(&owner), index)))
                .collect::<HashMap<_, _>>();
            while let Some(ty) = pending.pop() {
                match ty {
                    AstTy::Generic(_, name, arguments) => {
                        let marker_head = matches!(
                            sindr::names::builtin_type_name(Self::surface_name(name)),
                            Some(
                                sindr::names::TypeName::Pid
                                    | sindr::names::TypeName::Workers
                                    | sindr::names::TypeName::WorkerLease
                            )
                        );
                        if marker_head {
                            if let [AstTy::Named(_, parameter)] = arguments.as_slice() {
                                if let Some(node) = own_parameters.get(parameter) {
                                    markers.insert(node.clone());
                                }
                            }
                        } else {
                            let target = self
                                .env
                                .lookup_type_def(name)
                                .map(|d| {
                                    (Self::canonical_user_type_name(&d.name), d.type_params.len())
                                })
                                .or_else(|| {
                                    self.signature_aliases.get(name).map(|a| {
                                        (Self::canonical_user_type_name(name), a.params.len())
                                    })
                                });
                            if let Some((target, arity)) = target {
                                for (index, argument) in arguments.iter().take(arity).enumerate() {
                                    if let AstTy::Named(_, parameter) = argument {
                                        if let Some(node) = own_parameters.get(parameter) {
                                            dependents
                                                .entry((target.clone(), index))
                                                .or_default()
                                                .push(node.clone());
                                        }
                                    }
                                }
                            }
                        }
                        pending.extend(arguments);
                    }
                    AstTy::Tuple(_, items) => pending.extend(items),
                    AstTy::Func(_, parameters, result) => {
                        pending.extend(parameters);
                        pending.push(result);
                    }
                    AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => {}
                }
            }
        }
        let mut pending = markers.iter().cloned().collect::<Vec<_>>();
        while let Some(marker) = pending.pop() {
            if let Some(users) = dependents.get(&marker) {
                for user in users {
                    if markers.insert(user.clone()) {
                        pending.push(user.clone());
                    }
                }
            }
        }
        for definition in self.env.type_defs.values() {
            let name = Self::canonical_user_type_name(&definition.name);
            for (index, variable) in definition.type_param_vars.iter().enumerate() {
                if markers.contains(&(name.clone(), index)) {
                    self.process_marker_tyvars.insert(*variable);
                }
            }
        }
    }

    pub(in crate::checker) fn predeclare_type_signatures(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut seen_type_spans: HashMap<String, Span> = HashMap::new();
        // Pass 1: reserve deterministic tags for all user-defined types.
        for stmt in stmts {
            let maybe_decl = match stmt {
                Resolved::StructDef(_, id, type_params, _, _) => Some((
                    &id.name,
                    &id.span,
                    TypeKind::Struct,
                    type_params
                        .iter()
                        .map(|param| param.name.clone())
                        .collect::<Vec<_>>(),
                    type_params
                        .iter()
                        .map(|param| param.bound.clone())
                        .collect::<Vec<_>>(),
                    false,
                )),
                Resolved::RecordDef(_, id, _, _) => Some((
                    &id.name,
                    &id.span,
                    TypeKind::Record,
                    Vec::new(),
                    Vec::new(),
                    false,
                )),
                Resolved::DeferrorDef(_, id, _, _) => Some((
                    &id.name,
                    &id.span,
                    TypeKind::ConcreteError,
                    Vec::new(),
                    Vec::new(),
                    false,
                )),
                Resolved::EnumDef(_, id, type_params, _, attrs) => Some((
                    &id.name,
                    &id.span,
                    TypeKind::Enum,
                    type_params
                        .iter()
                        .map(|param| param.name.clone())
                        .collect::<Vec<_>>(),
                    type_params
                        .iter()
                        .map(|param| param.bound.clone())
                        .collect::<Vec<_>>(),
                    attrs.builtin,
                )),
                _ => None,
            };

            let Some((
                name,
                span,
                kind,
                type_params,
                type_param_bounds,
                allow_builtin_reserved_name,
            )) = maybe_decl
            else {
                continue;
            };

            if !allow_builtin_reserved_name
                && builtin_type_meta_by_name(Self::surface_name(name)).is_some()
            {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Type name `{}` is reserved by a canonical builtin type declaration",
                        Self::surface_name(name)
                    ),
                    span: span.clone(),
                    hint: Some("Builtin and canonical type names cannot be redefined.".into()),
                });
            }

            if let Some(first_span) = seen_type_spans.get(name) {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Internal consistency error: duplicate type owner `{}` reached Scar after resolution",
                        name
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "Sigil must reject duplicate owners before type checking; the first malformed resolved declaration was at {}..{}.",
                        first_span.start, first_span.end
                    )),
                });
            }
            seen_type_spans.insert(name.clone(), span.clone());

            let formal_variables = (0..type_params.len())
                .map(|_| match self.env.fresh_tyvar() {
                    Ty::Var(variable) => variable,
                    _ => unreachable!("fresh type variable must be a variable"),
                })
                .collect();
            self.env
                .predeclare_type_def(name.clone(), kind, type_params, type_param_bounds);
            self.env
                .lookup_type_def_mut(name)
                .expect("reserved nominal identity remains registered")
                .type_param_vars = formal_variables;
        }

        self.ensure_no_type_cycles(stmts)?;
        self.predeclare_trait_headers(stmts)?;
        for statement in stmts {
            let (Resolved::StructDef(_, id, _, _, _) | Resolved::EnumDef(_, id, _, _, _)) =
                statement
            else {
                continue;
            };
            let definition = self
                .env
                .lookup_type_def(&id.name)
                .expect("nominal identities have been reserved")
                .clone();
            for (variable, bound) in definition
                .type_param_vars
                .iter()
                .zip(&definition.type_param_bounds)
            {
                if let Some(bound) = bound {
                    self.register_tyvar_bound(*variable, bound);
                    if let Some(trait_key) = self.declaration_constructor_trait_key(bound) {
                        self.constructor_witness_traits.insert(*variable, trait_key);
                    }
                }
            }
        }
        self.predeclare_pid_parameter_roles(stmts);
        self.pending_nominal_arguments = Some(Vec::new());

        // Pass 2: finalize field signatures and constructor-like bindings.
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, type_params, fields, _) => {
                    let definition = self
                        .env
                        .lookup_type_def(&id.name)
                        .expect("struct identity has been reserved");
                    let mut tyvars = type_params
                        .iter()
                        .zip(&definition.type_param_vars)
                        .map(|(parameter, variable)| (parameter.name.clone(), Ty::Var(*variable)))
                        .collect::<HashMap<_, _>>();
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            let field_ty = self.resolve_signature_ast_ty_in_context(
                                &f.ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            )?;
                            if self.ty_contains_process_init(&field_ty) {
                                return Err(TypeError {
                                    structured: None,
                                    message:
                                        "StandbyInit<T> is only allowed as Standby @init return type"
                                            .into(),
                                    span: f.span.clone(),
                                    hint: None,
                                });
                            }
                            Ok((f.name.clone(), field_ty))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    let private_fields = fields
                        .iter()
                        .filter(|field| field.visibility == spire::ast::Visibility::Private)
                        .map(|field| field.name.clone())
                        .collect::<HashSet<_>>();
                    let readonly_fields = fields
                        .iter()
                        .filter(|field| field.readonly)
                        .map(|field| field.name.clone())
                        .collect::<HashSet<_>>();
                    let type_param_vars = type_params
                        .iter()
                        .filter_map(|param| match tyvars.get(&param.name) {
                            Some(Ty::Var(var)) => Some(*var),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    self.env
                        .resolve_type_def_signature(
                            &id.name,
                            ty_fields.clone(),
                            type_param_vars.clone(),
                            private_fields,
                            readonly_fields,
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env
                        .lookup_type_def_mut(&id.name)
                        .expect("resolved struct remains registered")
                        .field_type_spans = fields
                        .iter()
                        .map(|field| Self::ast_ty_span(&field.ty).clone())
                        .collect();
                    self.env.register_type_constructor_id(id.unique_id);
                    self.env.bind_var(
                        id.unique_id,
                        Ty::Struct(
                            id.name.clone(),
                            NominalType::new(
                                type_param_vars.iter().copied().map(Ty::Var).collect(),
                            ),
                        ),
                    );
                }
                Resolved::RecordDef(_, id, fields, _) => {
                    if let Some(field) = fields.iter().find(|field| {
                        field.visibility != spire::ast::Visibility::Public || field.readonly
                    }) {
                        return Err(TypeError {
                            structured: None,
                            message:
                                "record fields do not support visibility or readonly modifiers"
                                    .into(),
                            span: field.span.clone(),
                            hint: None,
                        });
                    }
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            let field_ty =
                                self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?;
                            if self.ty_contains_process_init(&field_ty) {
                                return Err(TypeError {
                                    structured: None,
                                    message:
                                        "StandbyInit<T> is only allowed as Standby @init return type"
                                            .into(),
                                    span: f.span.clone(),
                                    hint: None,
                                });
                            }
                            Ok((f.name.clone(), field_ty))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    self.env
                        .resolve_type_def_signature(
                            &id.name,
                            ty_fields.clone(),
                            Vec::new(),
                            HashSet::new(),
                            HashSet::new(),
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env
                        .lookup_type_def_mut(&id.name)
                        .expect("resolved record remains registered")
                        .field_type_spans = fields
                        .iter()
                        .map(|field| Self::ast_ty_span(&field.ty).clone())
                        .collect();
                    self.env.register_type_constructor_id(id.unique_id);
                    self.env.bind_var(
                        id.unique_id,
                        Ty::Record(id.name.clone(), NominalType::monomorphic()),
                    );
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
                        .resolve_type_def_signature(
                            &id.name,
                            ty_fields,
                            Vec::new(),
                            private_fields,
                            HashSet::new(),
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env
                        .lookup_type_def_mut(&id.name)
                        .expect("resolved data type remains registered")
                        .field_type_spans = fields
                        .iter()
                        .map(|field| Self::ast_ty_span(&field.ty).clone())
                        .collect();
                }
                Resolved::EnumDef(span, id, type_params, variants, attrs) => {
                    self.validate_builtin_enum_shape(span, id, type_params, variants, attrs)?;
                    let definition = self
                        .env
                        .lookup_type_def(&id.name)
                        .expect("enum identity has been reserved");
                    let mut sig_tyvars = type_params
                        .iter()
                        .zip(&definition.type_param_vars)
                        .map(|(parameter, variable)| (parameter.name.clone(), Ty::Var(*variable)))
                        .collect::<HashMap<_, _>>();
                    let enum_ty_args = type_params
                        .iter()
                        .map(|param| {
                            sig_tyvars
                                .get(&param.name)
                                .cloned()
                                .expect("every enum type parameter must be seeded")
                        })
                        .collect::<Vec<_>>();
                    let type_param_vars = enum_ty_args
                        .iter()
                        .map(|ty| match ty {
                            Ty::Var(variable) => *variable,
                            _ => unreachable!("seeded enum type parameter must be a variable"),
                        })
                        .collect::<Vec<_>>();
                    let _ = self
                        .env
                        .resolve_type_def_signature(
                            &id.name,
                            Vec::new(),
                            type_param_vars,
                            HashSet::new(),
                            HashSet::new(),
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;

                    let enum_surface_name = Self::surface_name(&id.name);
                    let builtin_result_enum = attrs.builtin && enum_surface_name == "Result";
                    let builtin_match_result_enum =
                        attrs.builtin && enum_surface_name == "MatchResult";
                    let builtin_boolean_enum = attrs.builtin && enum_surface_name == "Boolean";
                    let enum_ty = if builtin_result_enum {
                        let ok_ty = enum_ty_args
                            .first()
                            .expect("validated canonical Result has exactly one success slot")
                            .clone();
                        Ty::Result(Box::new(ok_ty), Box::new(Ty::Error))
                    } else if builtin_match_result_enum {
                        Ty::MatchResult(Box::new(enum_ty_args[0].clone()))
                    } else if builtin_boolean_enum {
                        Ty::Bool
                    } else {
                        Ty::Enum(id.name.clone(), enum_ty_args.clone())
                    };

                    self.env.bind_var(id.unique_id, enum_ty.clone());
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
                                structured: None,
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
                                let payload_ty = self.resolve_signature_ast_ty_in_context(
                                    ty,
                                    TypeSyntaxContext::General,
                                    &mut sig_tyvars,
                                )?;
                                if self.ty_contains_process_init(&payload_ty) {
                                    return Err(TypeError {
                                        structured: None,
                                        message:
                                            "StandbyInit<T> is only allowed as Standby @init return type"
                                                .into(),
                                        span: Self::ast_ty_span(ty).clone(),
                                        hint: None,
                                    });
                                }
                                Ok(payload_ty)
                            })
                            .collect::<Result<Vec<_>, _>>()?;

                        let short_name = variant
                            .id
                            .name
                            .rsplit("::")
                            .next()
                            .unwrap_or(variant.id.name.as_str())
                            .to_string();
                        let special_variant = if attrs.builtin {
                            sindr::names::special_enum_variant_meta(&variant.id.name)
                                .filter(|meta| meta.owner == enum_surface_name)
                                .map(|meta| meta.lowering)
                        } else {
                            None
                        };
                        let tag = match special_variant {
                            Some(sindr::names::SpecialEnumVariantLowering::ResultOk) => 0,
                            Some(sindr::names::SpecialEnumVariantLowering::ResultErr) => 1,
                            _ => self.env.reserve_tag(),
                        };
                        let info = crate::env::EnumVariantInfo {
                            special_variant,
                            constructor_name: variant.id.name.clone(),
                            short_name,
                            enum_name: id.name.clone(),
                            enum_ty: enum_ty.clone(),
                            tag,
                            payload: payload.clone(),
                            payload_type_spans: variant
                                .payload
                                .iter()
                                .map(|ty| Self::ast_ty_span(ty).clone())
                                .collect(),
                            discriminant: discriminant.clone(),
                        };
                        self.env
                            .register_enum_variant(variant.id.unique_id, info.clone())
                            .map_err(|message| TypeError {
                                structured: None,
                                message,
                                span: variant.span.clone(),
                                hint: None,
                            })?;
                        enum_variants.push(info);
                    }

                    self.env
                        .enum_variants_by_enum
                        .insert(id.name.clone(), Arc::new(enum_variants));
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub(in crate::checker) fn ensure_no_type_cycles(
        &self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut decl_spans: HashMap<String, Span> = HashMap::new();
        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();

        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, _, fields, _)
                | Resolved::RecordDef(_, id, fields, _)
                | Resolved::DeferrorDef(_, id, fields, _) => {
                    decl_spans.insert(id.name.clone(), id.span.clone());
                    edges.entry(id.name.clone()).or_default();
                    for field in fields {
                        let mut refs = Vec::new();
                        Self::collect_type_dependency_names(&field.ty, &mut refs);
                        for ref_name in refs {
                            edges.entry(id.name.clone()).or_default().insert(ref_name);
                        }
                    }
                }
                Resolved::EnumDef(_, id, _, variants, _) => {
                    decl_spans.insert(id.name.clone(), id.span.clone());
                    edges.entry(id.name.clone()).or_default();
                    let mut common_refs: Option<HashSet<String>> = None;
                    for variant in variants {
                        let mut variant_refs = HashSet::new();
                        for payload_ty in &variant.payload {
                            let mut refs = Vec::new();
                            Self::collect_type_dependency_names(payload_ty, &mut refs);
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

        fn find_cycle<'a>(
            node: &'a str,
            edges: &'a HashMap<String, HashSet<String>>,
            states: &mut HashMap<String, Visit>,
        ) -> Option<Vec<String>> {
            if states.get(node) == Some(&Visit::Done) {
                return None;
            }
            states.insert(node.to_string(), Visit::Visiting);
            let mut stack = vec![(node, edges[node].iter())];
            // Each frame retains its iterator so returning from a child resumes
            // the same dependency order as the recursive traversal.
            while let Some((_, nexts)) = stack.last_mut() {
                if let Some(next) = nexts.next() {
                    match states.get(next) {
                        Some(Visit::Done) => {}
                        Some(Visit::Visiting) => {
                            let start = stack
                                .iter()
                                .position(|(name, _)| *name == next)
                                .expect("visiting dependency remains on the traversal stack");
                            let mut cycle = stack[start..]
                                .iter()
                                .map(|(name, _)| (*name).to_string())
                                .collect::<Vec<_>>();
                            cycle.push(next.clone());
                            return Some(cycle);
                        }
                        None => {
                            states.insert(next.clone(), Visit::Visiting);
                            stack.push((next.as_str(), edges[next].iter()));
                        }
                    }
                } else {
                    let (finished, _) = stack.pop().expect("traversal has an active frame");
                    states.insert(finished.to_string(), Visit::Done);
                }
            }
            None
        }

        let mut states: HashMap<String, Visit> = HashMap::new();
        for name in decl_spans.keys() {
            if let Some(cycle) = find_cycle(name, &edges, &mut states) {
                return Err(TypeError {
                    structured: None,
                    message: format!("Cyclic type definition detected: {}", cycle.join(" -> ")),
                    span: decl_spans[&cycle[0]].clone(),
                    hint: None,
                });
            }
        }

        Ok(())
    }

    fn validate_nominal_declaration_constructor_applications(
        &self,
        ty: &Ty,
        parameters: &HashMap<u32, (String, String)>,
        owner: &str,
        span: &Span,
    ) -> Result<(), TypeError> {
        match ty {
            Ty::SelfApp(items) => {
                if let Some((witness, slots)) = Self::constructor_application_parts(items) {
                    let Ty::Var(variable) = self.resolve_ty(witness) else {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Type constructor application in {} has no declaration parameter witness",
                                Self::surface_name(owner)
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    let Some((parameter, bound)) = parameters.get(&variable) else {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Type constructor application in {} does not refer to one of its declaration parameters",
                                Self::surface_name(owner)
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    let Some(trait_key) = self.constructor_witness_traits.get(&variable) else {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Type constructor parameter {} on {} requires a TypeCtorTrait declaration constraint; {} is not a constructor trait",
                                parameter,
                                Self::surface_name(owner),
                                self.trait_display_name(bound)
                            ),
                            span: span.clone(),
                            hint: Some(
                                "Use a declaration constraint whose Trait defines `Self: Type<...>`."
                                    .into(),
                            ),
                        });
                    };
                    let Some(trait_info) = self.traits.get(trait_key) else {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Unknown constructor Trait {} for {}",
                                self.trait_display_name(trait_key),
                                parameter
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    let expected = trait_info.constructor_slots.len();
                    if slots.len() != expected {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Type constructor parameter {} requires {} slot argument(s), got {}",
                                parameter,
                                expected,
                                slots.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                }
                for item in items {
                    self.validate_nominal_declaration_constructor_applications(
                        item, parameters, owner, span,
                    )?;
                }
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.validate_nominal_declaration_constructor_applications(
                inner, parameters, owner, span,
            )?,
            Ty::Tuple(items) => {
                for item in items {
                    self.validate_nominal_declaration_constructor_applications(
                        item, parameters, owner, span,
                    )?;
                }
            }
            Ty::Func(params, ret)
            | Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. } => {
                for param in params {
                    self.validate_nominal_declaration_constructor_applications(
                        param, parameters, owner, span,
                    )?;
                }
                self.validate_nominal_declaration_constructor_applications(
                    ret, parameters, owner, span,
                )?;
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                for item in [source, focus, update_source, update_focus] {
                    self.validate_nominal_declaration_constructor_applications(
                        item, parameters, owner, span,
                    )?;
                }
            }
            Ty::Result(ok, err) => {
                self.validate_nominal_declaration_constructor_applications(
                    ok, parameters, owner, span,
                )?;
                self.validate_nominal_declaration_constructor_applications(
                    err, parameters, owner, span,
                )?;
            }
            Ty::Enum(_, arguments) => {
                for argument in arguments {
                    self.validate_nominal_declaration_constructor_applications(
                        argument, parameters, owner, span,
                    )?;
                }
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for argument in &nominal.arguments {
                    self.validate_nominal_declaration_constructor_applications(
                        argument, parameters, owner, span,
                    )?;
                }
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::Hole
            | Ty::Var(_)
            | Ty::ProcessMarker(_) => {}
        }
        Ok(())
    }

    pub(in crate::checker) fn activate_nominal_declaration_bounds(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let pending = self.pending_nominal_arguments.take().ok_or_else(|| {
            self.typecheck_invariant_error(
                "nominal schema validation was not initialized",
                &Span { start: 0, end: 0 },
            )
        })?;
        for (name, arguments, span, defer_unresolved) in pending {
            let definition = self.env.lookup_type_def(&name).cloned().ok_or_else(|| {
                self.typecheck_invariant_error("deferred nominal declaration was lost", &span)
            })?;
            self.validate_nominal_type_arguments(&definition, &arguments, &span, defer_unresolved)?;
        }
        let declarations = stmts
            .iter()
            .filter_map(|stmt| match stmt {
                Resolved::StructDef(span, id, _, _, _) | Resolved::EnumDef(span, id, _, _, _) => {
                    Some((id.name.clone(), span.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        for (name, span) in declarations {
            let def = self.env.lookup_type_def(&name).cloned().ok_or_else(|| {
                self.typecheck_invariant_error(
                    "nominal declaration missing during bound validation",
                    &span,
                )
            })?;
            let parameters = def
                .type_param_vars
                .iter()
                .copied()
                .zip(def.type_params.iter())
                .zip(def.type_param_bounds.iter())
                .filter_map(|((variable, parameter), bound)| {
                    bound
                        .as_ref()
                        .map(|bound| (variable, (parameter.clone(), bound.clone())))
                })
                .collect::<HashMap<_, _>>();
            for (variable, (_, bound)) in &parameters {
                if let Some(trait_key) = self.declaration_constructor_trait_key(bound) {
                    self.constructor_witness_traits.insert(*variable, trait_key);
                }
            }
            for (_, field) in &def.fields {
                self.validate_nominal_declaration_constructor_applications(
                    field,
                    &parameters,
                    &name,
                    &span,
                )?;
                self.validate_nominal_type_well_formed(field, &span, false)?;
            }
            if matches!(def.kind, TypeKind::Enum) {
                if let Some(variants) = self.lookup_enum_variants_of(&name).cloned() {
                    for variant in variants {
                        for payload in &variant.payload {
                            self.validate_nominal_declaration_constructor_applications(
                                payload,
                                &parameters,
                                &name,
                                &span,
                            )?;
                            self.validate_nominal_type_well_formed(payload, &span, false)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
