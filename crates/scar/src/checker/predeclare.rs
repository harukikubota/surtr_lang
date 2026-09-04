use super::*;
use sindr::builtin::{builtin_type_meta_by_name, builtin_type_supports_inherent_impl};
use sindr::names::builtin_type_usage_policy;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

static SYNTHETIC_DEFAULT_METHOD_UID: AtomicU32 = AtomicU32::new(0x6000_0000);

impl Checker {
    fn validate_type_shape_clause(
        clause: Option<&ResolvedWhereClause>,
        trait_definition: bool,
        enclosing_impl_trait: Option<&ResolvedId>,
        constructor_trait_ids: &HashSet<u32>,
    ) -> Result<(), TypeError> {
        let Some(clause) = clause else {
            return Ok(());
        };
        let mut shape_seen = false;
        for constraint in &clause.constraints {
            for bound in &constraint.bounds {
                if let ResolvedWhereConstraintRhs::TraitSlot { trait_id, span, .. } = bound {
                    let Some(enclosing_trait) = enclosing_impl_trait else {
                        return Err(TypeError {
                            structured: None,
                            message: "`Trait.$Slot` mappings are only allowed in a trait implementation where clause".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !constructor_trait_ids.contains(&enclosing_trait.unique_id) {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Trait {} is not a TypeConstructor trait and cannot own a slot mapping",
                                enclosing_trait.name
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    if trait_id.unique_id != enclosing_trait.unique_id {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "`Trait.$Slot` mapping owner {} must be the same trait as enclosing impl {}",
                                trait_id.name, enclosing_trait.name
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    continue;
                }
                let ResolvedWhereConstraintRhs::TypeConstructor { span, .. } = bound else {
                    continue;
                };
                if !trait_definition
                    || !matches!(&constraint.subject, AstTy::Named(_, name) if name == "Self")
                {
                    return Err(TypeError {
                        structured: None,
                        message:
                            "`Type<...>` is only allowed as `Self: Type<...>` in a trait definition where clause"
                                .into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                if shape_seen {
                    return Err(TypeError {
                        structured: None,
                        message: "A trait definition cannot declare more than one Self type-constructor constraint"
                            .into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                shape_seen = true;
            }
        }
        Ok(())
    }

    pub(super) fn validate_declaration_where_well_formedness(
        &self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut constructor_trait_ids = self
            .traits
            .values()
            .filter(|info| !info.constructor_slots.is_empty())
            .map(|info| info.id.unique_id)
            .collect::<HashSet<_>>();
        constructor_trait_ids.extend(stmts.iter().filter_map(|stmt| {
            let Resolved::TraitDef(_, id, _, Some(clause), _, _) = stmt else {
                return None;
            };
            clause
                .constraints
                .iter()
                .any(|constraint| {
                    matches!(&constraint.subject, AstTy::Named(_, name) if name == "Self")
                        && constraint.bounds.iter().any(|bound| {
                            matches!(bound, ResolvedWhereConstraintRhs::TypeConstructor { .. })
                        })
                })
                .then_some(id.unique_id)
        }));
        // A trait may inherit its contextual shape through a parent (for
        // example `Applicative: Functor`).  This validation runs before the
        // trait table has been predeclared, so reproduce the same closure
        // over the resolved declarations here rather than mistaking those
        // inherited constructor traits for ordinary traits.
        loop {
            let mut changed = false;
            for stmt in stmts {
                let Resolved::TraitDef(_, id, _, Some(clause), _, _) = stmt else {
                    continue;
                };
                if constructor_trait_ids.contains(&id.unique_id) {
                    continue;
                }
                let inherits_constructor_shape = clause.constraints.iter().any(|constraint| {
                    matches!(&constraint.subject, AstTy::Named(_, name) if name == "Self")
                        && constraint.bounds.iter().any(|bound| {
                            matches!(bound, ResolvedWhereConstraintRhs::Trait { trait_id }
                                if constructor_trait_ids.contains(&trait_id.unique_id))
                        })
                });
                if inherits_constructor_shape {
                    constructor_trait_ids.insert(id.unique_id);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for stmt in stmts {
            match stmt {
                Resolved::Def(_, _, _, _, _, clause, _, _)
                | Resolved::BuiltinDecl(_, _, _, _, _, clause, _) => {
                    Self::validate_type_shape_clause(
                        clause.as_ref(),
                        false,
                        None,
                        &constructor_trait_ids,
                    )?;
                }
                Resolved::TraitDef(_, _, _, clause, methods, _) => {
                    Self::validate_type_shape_clause(
                        clause.as_ref(),
                        true,
                        None,
                        &constructor_trait_ids,
                    )?;
                    for method in methods {
                        Self::validate_type_shape_clause(
                            method.where_clause.as_ref(),
                            false,
                            None,
                            &constructor_trait_ids,
                        )?;
                    }
                }
                Resolved::TraitImplDef(_, trait_id, _, _, clause, methods) => {
                    Self::validate_type_shape_clause(
                        clause.as_ref(),
                        false,
                        Some(trait_id),
                        &constructor_trait_ids,
                    )?;
                    for method in methods {
                        Self::validate_type_shape_clause(
                            method.where_clause.as_ref(),
                            false,
                            None,
                            &constructor_trait_ids,
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn predeclare_signature_aliases(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        for stmt in stmts {
            let Resolved::TypeAlias(span, name, params, rhs, _) = stmt else {
                continue;
            };
            if self.signature_aliases.contains_key(name) || self.env.lookup_type_def(name).is_some()
            {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Internal consistency error: duplicate type owner `{}` reached Scar after resolution",
                        name
                    ),
                    span: span.clone(),
                    hint: Some(
                        "Sigil must reject duplicate owners before type checking.".into(),
                    ),
                });
            }
            let mut used = HashSet::new();
            Self::collect_ast_ty_type_params(rhs, &mut used);
            if let Some(param) = params.iter().find(|param| !used.contains(&param.name)) {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Type alias {} has an unused type parameter {}",
                        name, param.name
                    ),
                    span: param.span.clone(),
                    hint: Some(
                        "Every alias type parameter must appear in its function signature.".into(),
                    ),
                });
            }
            self.signature_aliases.insert(
                name.clone(),
                SignatureAliasInfo {
                    params: params.clone(),
                    rhs: rhs.clone(),
                    span: span.clone(),
                },
            );
        }
        Ok(())
    }

    fn where_constraint_subject_ty(
        &self,
        subject: &AstTy,
        tyvars: &HashMap<String, Ty>,
        self_ty: Option<&Ty>,
        span: &Span,
    ) -> Result<Ty, TypeError> {
        let AstTy::Named(_, name) = subject else {
            return Err(TypeError {
                structured: None,
                message: "where constraint subjects must be `Self` or a signature type variable"
                    .into(),
                span: span.clone(),
                hint: None,
            });
        };
        if name == "Self" {
            return self_ty.cloned().ok_or_else(|| TypeError {
                structured: None,
                message: "`Self` is only available in trait and trait impl where clauses".into(),
                span: span.clone(),
                hint: None,
            });
        }
        if !name.starts_with('$') {
            return Err(TypeError {
                structured: None,
                message: "where constraint subjects must be `Self` or a signature type variable"
                    .into(),
                span: span.clone(),
                hint: None,
            });
        }
        tyvars.get(name).cloned().ok_or_else(|| TypeError {
            structured: None,
            message: format!(
                "where clause constraint `{name}` does not appear in the declaration signature"
            ),
            span: span.clone(),
            hint: Some("where clauses add constraints; they do not declare type variables".into()),
        })
    }

    fn apply_where_trait_bound(
        &mut self,
        subject: &Ty,
        trait_id: &ResolvedId,
    ) -> Result<(), TypeError> {
        let trait_key = self.trait_key(trait_id);
        self.traits.get(&trait_key).ok_or_else(|| TypeError {
            structured: None,
            message: format!("Unknown trait: {}", trait_id.name),
            span: trait_id.span.clone(),
            hint: None,
        })?;
        match self.resolve_ty(subject) {
            Ty::Var(var) => {
                self.register_tyvar_bound(var, &trait_key);
                Ok(())
            }
            // A concrete `Self` is already guarded by trait dispatch/impl
            // validation. Signature where clauses primarily add bounds to
            // variables that must survive instantiation at call sites.
            _ => Ok(()),
        }
    }

    pub(super) fn apply_resolved_where_trait_bounds(
        &mut self,
        where_clause: Option<&ResolvedWhereClause>,
        tyvars: &HashMap<String, Ty>,
        self_ty: Option<&Ty>,
    ) -> Result<(), TypeError> {
        let Some(where_clause) = where_clause else {
            return Ok(());
        };
        for constraint in &where_clause.constraints {
            let subject = self.where_constraint_subject_ty(
                &constraint.subject,
                tyvars,
                self_ty,
                &constraint.span,
            )?;
            for bound in &constraint.bounds {
                if let ResolvedWhereConstraintRhs::Trait { trait_id } = bound {
                    self.apply_where_trait_bound(&subject, trait_id)?;
                }
            }
        }
        Ok(())
    }

    fn apply_typed_where_trait_bounds(
        &mut self,
        where_clause: Option<&TypedWhereClause>,
        tyvars: &HashMap<String, Ty>,
        self_ty: Option<&Ty>,
    ) -> Result<(), TypeError> {
        let Some(where_clause) = where_clause else {
            return Ok(());
        };
        for constraint in &where_clause.constraints {
            let subject = self.where_constraint_subject_ty(
                &constraint.subject,
                tyvars,
                self_ty,
                &constraint.span,
            )?;
            for bound in &constraint.bounds {
                if let TypedWhereConstraintRhs::Trait { trait_id } = bound {
                    self.apply_where_trait_bound(&subject, trait_id)?;
                }
            }
        }
        Ok(())
    }

    fn next_synthetic_default_method_uid() -> u32 {
        SYNTHETIC_DEFAULT_METHOD_UID.fetch_add(1, AtomicOrdering::Relaxed)
    }

    fn synthetic_default_method_symbol(
        trait_instance_key: &str,
        target_name: &str,
        method_name: &str,
    ) -> String {
        fn sanitize(segment: &str) -> String {
            segment
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect()
        }

        format!(
            "__default__{}__{}__{}",
            sanitize(trait_instance_key),
            sanitize(target_name),
            sanitize(method_name)
        )
    }

    fn trait_method_display_name(trait_id: &ResolvedId, method_name: &str) -> String {
        format!("{}::{}", Self::surface_name(&trait_id.name), method_name)
    }

    fn synthesized_default_method_id(
        &self,
        trait_instance_key: &str,
        _trait_id: &ResolvedId,
        target_name: &str,
        method_name: &str,
        span: &Span,
    ) -> ResolvedId {
        let qualified_name =
            Self::synthetic_default_method_symbol(trait_instance_key, target_name, method_name);
        ResolvedId {
            name: method_name.to_string(),
            qualified_name: Some(qualified_name),
            symbol_info: None,
            unique_id: Self::next_synthetic_default_method_uid(),
            compiler_generated: true,
            span: span.clone(),
        }
    }

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
            structured: None,
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

    fn const_facet_segment_is_allowed(&self, segment: &ResolvedFacetPathSegment) -> bool {
        match segment {
            ResolvedFacetPathSegment::Field { .. } => true,
            ResolvedFacetPathSegment::Bracket(expr) => match expr.expr.as_ref() {
                Resolved::Lit(_, Lit::Int(_) | Lit::Str(_)) => true,
                Resolved::RangeLiteral(_, start, end) => matches!(
                    (start.as_ref(), end.as_ref()),
                    (Resolved::Lit(_, Lit::Int(_)), Resolved::Lit(_, Lit::Int(_)))
                ),
                _ => false,
            },
        }
    }

    fn const_has_dynamic_bracket_segment(&self, value: &Resolved) -> bool {
        match value {
            Resolved::FieldAccess(_, inner, _) => self.const_has_dynamic_bracket_segment(inner),
            Resolved::FacetSegmentAccess(_, inner, segment) => {
                self.const_has_dynamic_bracket_segment(inner)
                    || !self.const_facet_segment_is_allowed(segment)
            }
            Resolved::BinOp(_, BinOp::Slash, left, right) => {
                self.const_has_dynamic_bracket_segment(left)
                    || self.const_has_dynamic_bracket_segment(right)
            }
            _ => false,
        }
    }

    fn const_surface_is_allowed(&self, value: &Resolved) -> bool {
        match value {
            Resolved::Lit(_, _) => true,
            Resolved::Var(_, id) => self
                .consts
                .get(&id.unique_id)
                .is_none_or(|meta| matches!(meta.kind, ConstKind::FacetPath)),
            Resolved::FieldAccess(_, inner, _) => self.const_surface_is_allowed(inner),
            Resolved::FacetSegmentAccess(_, inner, segment) => {
                self.const_surface_is_allowed(inner) && self.const_facet_segment_is_allowed(segment)
            }
            Resolved::InferredFacetCapture(_, _) => false,
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
                if self.const_has_dynamic_bracket_segment(value) {
                    return Err(TypeError {
                        structured: None,
                        message:
                            "const Facet path bracket segments must use literal Int or String values"
                                .into(),
                        span: span.clone(),
                        hint: Some(
                            "Use literal container keys in const Facet paths and keep dynamic bracket expressions in local bindings or Facet API calls.".into(),
                        ),
                    });
                }
                return Err(TypeError {
                    structured: None,
                    message: "const value must be a primitive literal or a facet path".into(),
                    span: span.clone(),
                    hint: Some(
                        "V1 const supports literal values, facet paths, Facet const refs, and `/` composition of those facet values only.".into(),
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
                TypedInner::FacetPath(path) => (
                    ConstKind::FacetPath,
                    StoredConstValue::FacetPath(path.clone()),
                ),
                _ => {
                    return Err(TypeError {
                        structured: None,
                        message: "const value must be a primitive literal or a facet path".into(),
                        span: span.clone(),
                        hint: Some(
                            "Use `const NAME = 1`, `const NAME = User.profile`, or compose Facet consts with `/`.".into(),
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
                Resolved::StructDef(_, id, type_params, _, _) => Some((
                    &id.name,
                    &id.span,
                    TypeKind::Struct,
                    type_params
                        .iter()
                        .map(|param| param.name.clone())
                        .collect::<Vec<_>>(),
                    false,
                )),
                Resolved::RecordDef(_, id, _, _) => {
                    Some((&id.name, &id.span, TypeKind::Record, Vec::new(), false))
                }
                Resolved::DeferrorDef(_, id, _, _) => Some((
                    &id.name,
                    &id.span,
                    TypeKind::ConcreteError,
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
                    attrs.builtin,
                )),
                _ => None,
            };

            let Some((name, span, kind, type_params, allow_builtin_reserved_name)) = maybe_decl
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

            self.env
                .predeclare_type_def(name.clone(), kind, type_params);
        }

        self.ensure_no_type_cycles(stmts)?;

        // Pass 2: finalize field signatures and constructor-like bindings.
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, type_params, fields, attrs) => {
                    let mut tyvars = HashMap::new();
                    self.seed_signature_type_params(type_params, &mut tyvars);
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
                            type_param_vars,
                            private_fields,
                            readonly_fields,
                            attrs.readonly,
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env.register_type_constructor_id(id.unique_id);
                    self.env
                        .bind_var(id.unique_id, Ty::Struct(id.name.clone(), ty_fields));
                }
                Resolved::RecordDef(_, id, fields, _) => {
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
                    let private_fields = fields
                        .iter()
                        .filter(|field| field.visibility == spire::ast::Visibility::Private)
                        .map(|field| field.name.clone())
                        .collect::<HashSet<_>>();
                    self.env
                        .resolve_type_def_signature(
                            &id.name,
                            ty_fields.clone(),
                            Vec::new(),
                            private_fields,
                            HashSet::new(),
                            false,
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
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
                        .resolve_type_def_signature(
                            &id.name,
                            ty_fields,
                            Vec::new(),
                            private_fields,
                            HashSet::new(),
                            false,
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                }
                Resolved::EnumDef(_, id, type_params, variants, attrs) => {
                    let _ = self
                        .env
                        .resolve_type_def_signature(
                            &id.name,
                            Vec::new(),
                            Vec::new(),
                            HashSet::new(),
                            HashSet::new(),
                            false,
                        )
                        .ok_or_else(|| TypeError {
                            structured: None,
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

                    let enum_surface_name = Self::surface_name(&id.name);
                    let builtin_result_enum = attrs.builtin && enum_surface_name == "Result";
                    let builtin_boolean_enum = attrs.builtin && enum_surface_name == "Boolean";
                    let enum_ty = if builtin_result_enum {
                        let ok_ty = enum_ty_args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| self.env.fresh_tyvar());
                        Ty::Result(Box::new(ok_ty), Box::new(Ty::Error))
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
                        let tag = if builtin_result_enum {
                            match short_name.as_str() {
                                "Ok" => 0,
                                "Err" => 1,
                                _ => self.env.reserve_tag(),
                            }
                        } else {
                            self.env.reserve_tag()
                        };
                        let info = crate::env::EnumVariantInfo {
                            constructor_name: variant.id.name.clone(),
                            short_name,
                            enum_name: id.name.clone(),
                            enum_ty: enum_ty.clone(),
                            tag,
                            payload: payload.clone(),
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
                    structured: None,
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

    pub(super) fn resolve_def_signature_ast_ty_in_context(
        &mut self,
        id: &ResolvedId,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        let Some((target, _)) = Self::split_impl_method_id(id) else {
            return self.resolve_signature_ast_ty_in_context(ast_ty, context, tyvars);
        };
        if self.env.lookup_type_def(&target).is_none()
            && !builtin_type_supports_inherent_impl(Self::surface_name(&target))
        {
            return self.resolve_signature_ast_ty_in_context(ast_ty, context, tyvars);
        }
        let rewritten = Self::rewrite_inherent_self_apps(ast_ty, &target);
        self.resolve_signature_ast_ty_in_context(&rewritten, context, tyvars)
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
                            structured: None,
                            message: "`self` can only be rebound inside impl methods".to_string(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !self.types_compatible(expected, bind_ty) {
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
                    if !self.types_compatible(expected, alias_ty) {
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
            | TypedPattern::Pin(_, _, _)
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
            if let Resolved::StructDef(_, id, type_params, fields, _attrs) = stmt {
                let mut tyvars = HashMap::new();
                self.seed_signature_type_params(type_params, &mut tyvars);
                let expected_self_ty = Ty::Struct(
                    id.name.clone(),
                    fields
                        .iter()
                        .map(|field| {
                            Ok((
                                field.name.clone(),
                                self.resolve_signature_ast_ty_in_context(
                                    &field.ty,
                                    TypeSyntaxContext::General,
                                    &mut tyvars,
                                )?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?,
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
                    if !self.struct_new_return_allowed(expected_self_ty, &ret_ty) {
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

    pub(super) fn register_function_id(&mut self, id: &ResolvedId) {
        let key = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        self.function_ids_by_name.insert(key.clone(), id.clone());
        if let Some(surface_key) = key.strip_prefix("Global::") {
            self.function_ids_by_name
                .insert(surface_key.to_string(), id.clone());
        }
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

    /// Storage identity retains the complete impl head.  The base trait stays
    /// in the first key slot so candidate lookup remains inexpensive, while
    /// concrete specializations such as `List<Int>` and `List<String>` no
    /// longer overwrite each other merely because they share a nominal head.
    fn trait_impl_storage_key(
        &self,
        trait_id: &ResolvedId,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
    ) -> TraitImplKey {
        (
            self.trait_key(trait_id),
            format!(
                "{} for {}",
                trait_args
                    .iter()
                    .map(Self::ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::ast_ty_key(target_ast_ty)
            ),
        )
    }

    /// Coherence is deliberately independent of dispatch ordering.  Impl
    /// head variables are allocated independently when their heads are
    /// resolved, so unifying both argument lists and targets in one temporary
    /// substitution environment is an alpha-renaming-safe overlap test.
    fn trait_impl_patterns_overlap(
        &mut self,
        left_args: &[Ty],
        left_target: &Ty,
        right_args: &[Ty],
        right_target: &Ty,
    ) -> bool {
        if left_args.len() != right_args.len() {
            return false;
        }
        let before = self.substitutions.clone();
        self.substitutions.clear();
        let overlap = left_args
            .iter()
            .zip(right_args)
            .all(|(left, right)| self.types_compatible(left, right))
            && self.types_compatible(left_target, right_target);
        self.substitutions = before;
        overlap
    }

    pub(super) fn trait_impl_for_head(
        &self,
        trait_id: &ResolvedId,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
    ) -> Option<TraitImplInfo> {
        let storage_key = self.trait_impl_storage_key(trait_id, trait_args, target_ast_ty);
        self.trait_impls.get(&storage_key).cloned()
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

    pub(super) fn collect_ty_vars(ty: &Ty, out: &mut Vec<u32>) {
        match ty {
            Ty::Var(var) => {
                if !out.contains(var) {
                    out.push(*var);
                }
            }
            Ty::List(inner) | Ty::Lazy(inner) => Self::collect_ty_vars(inner, out),
            Ty::Result(source, focus) => {
                Self::collect_ty_vars(source, out);
                Self::collect_ty_vars(focus, out);
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                Self::collect_ty_vars(source, out);
                Self::collect_ty_vars(focus, out);
                Self::collect_ty_vars(update_source, out);
                Self::collect_ty_vars(update_focus, out);
            }
            Ty::Tuple(items) | Ty::SelfApp(items) | Ty::Enum(_, items) => {
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

    fn signature_type_param_vars(
        declared: &[ResolvedTypeParam],
        tyvars: &HashMap<String, Ty>,
        params: &[Ty],
        ret: &Ty,
    ) -> Vec<u32> {
        let mut vars = Vec::new();
        for param in declared {
            if let Some(ty) = tyvars.get(&param.name) {
                Self::collect_ty_vars(ty, &mut vars);
            }
        }
        for param in params {
            Self::collect_ty_vars(param, &mut vars);
        }
        Self::collect_ty_vars(ret, &mut vars);
        vars
    }

    fn trait_constructor_slots(
        &self,
        trait_id: &ResolvedId,
        where_clause: Option<&ResolvedWhereClause>,
    ) -> Result<Vec<String>, TypeError> {
        let mut slots = None;
        let Some(clause) = where_clause else {
            return Ok(Vec::new());
        };
        for constraint in &clause.constraints {
            if !matches!(&constraint.subject, AstTy::Named(_, name) if name == "Self") {
                continue;
            }
            for bound in &constraint.bounds {
                let ResolvedWhereConstraintRhs::TypeConstructor {
                    span,
                    slots: declared_slots,
                } = bound
                else {
                    continue;
                };
                if slots.is_some() {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait {} declares more than one Self type-constructor constraint",
                            trait_id.name
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                if declared_slots.is_empty() {
                    return Err(TypeError {
                        structured: None,
                        message: "Type constructor constraints require at least one slot".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let mut names = Vec::with_capacity(declared_slots.len());
                let mut seen = HashSet::new();
                for slot in declared_slots {
                    let AstTy::Named(slot_span, name) = slot else {
                        return Err(TypeError {
                            structured: None,
                            message: "Type constructor slots must be type variables such as `$A`"
                                .into(),
                            span: Self::ast_ty_span(slot).clone(),
                            hint: None,
                        });
                    };
                    if !name.starts_with('$') {
                        return Err(TypeError {
                            structured: None,
                            message: "Type constructor slots must be type variables such as `$A`"
                                .into(),
                            span: slot_span.clone(),
                            hint: None,
                        });
                    }
                    if !seen.insert(name.clone()) {
                        return Err(TypeError {
                            structured: None,
                            message: format!("Duplicate type constructor slot: {}", name),
                            span: slot_span.clone(),
                            hint: None,
                        });
                    }
                    names.push(name.clone());
                }
                slots = Some(names);
            }
        }
        Ok(slots.unwrap_or_default())
    }

    fn trait_parents(where_clause: Option<&ResolvedWhereClause>) -> Vec<TraitParent> {
        let Some(clause) = where_clause else {
            return Vec::new();
        };
        clause
            .constraints
            .iter()
            .filter(|constraint| {
                matches!(&constraint.subject, AstTy::Named(_, subject) if subject == "Self")
            })
            .flat_map(|constraint| constraint.bounds.iter())
            .filter_map(|bound| match bound {
                ResolvedWhereConstraintRhs::Trait { trait_id } => Some(TraitParent {
                    trait_id: trait_id.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn resolve_trait_constraint_closure(&mut self) -> Result<(), TypeError> {
        fn visit(
            key: &str,
            traits: &HashMap<String, TraitInfo>,
            visiting: &mut Vec<String>,
            resolved: &mut HashMap<String, Vec<String>>,
        ) -> Result<Vec<String>, TypeError> {
            if let Some(slots) = resolved.get(key) {
                return Ok(slots.clone());
            }
            if let Some(start) = visiting.iter().position(|item| item == key) {
                let mut cycle = visiting[start..].to_vec();
                cycle.push(key.to_string());
                let info = traits.get(key).expect("visited trait must exist");
                return Err(TypeError {
                    structured: None,
                    message: format!("Parent trait constraint cycle: {}", cycle.join(" -> ")),
                    span: info.id.span.clone(),
                    hint: None,
                });
            }
            let info = traits.get(key).ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown parent trait: {key}"),
                span: Span { start: 0, end: 0 },
                hint: None,
            })?;
            visiting.push(key.to_string());
            let mut slots = info.constructor_slots.clone();
            for parent in &info.parents {
                let parent_key = parent
                    .trait_id
                    .qualified_name
                    .clone()
                    .unwrap_or_else(|| parent.trait_id.name.clone());
                let parent_slots = visit(&parent_key, traits, visiting, resolved)?;
                if slots.is_empty() {
                    slots = parent_slots;
                } else if !parent_slots.is_empty() && slots.len() != parent_slots.len() {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait {} exposes {} constructor slot(s), but parent {} exposes {}",
                            info.id.name,
                            slots.len(),
                            parent.trait_id.name,
                            parent_slots.len()
                        ),
                        span: info.id.span.clone(),
                        hint: None,
                    });
                }
            }
            visiting.pop();
            resolved.insert(key.to_string(), slots.clone());
            Ok(slots)
        }

        let keys = self.traits.keys().cloned().collect::<Vec<_>>();
        let snapshot = self.traits.clone();
        let mut resolved = HashMap::new();
        for key in &keys {
            visit(key, &snapshot, &mut Vec::new(), &mut resolved)?;
        }
        for (key, slots) in resolved {
            if let Some(info) = self.traits.get_mut(&key) {
                info.constructor_slots = slots;
            }
        }
        for info in self.traits.values() {
            if !info.constructor_slots.is_empty() && !info.type_params.is_empty() {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Constructor trait {} cannot declare trait type parameter(s)",
                        info.id.name
                    ),
                    span: info.id.span.clone(),
                    hint: Some(
                        "Put element slots in `Self: Type<$A, ...>` and introduce them through method inputs."
                            .into(),
                    ),
                });
            }
        }
        Ok(())
    }

    fn target_top_level_type_params(target_ast_ty: &AstTy) -> Vec<String> {
        let AstTy::Generic(_, _, args) = target_ast_ty else {
            return Vec::new();
        };
        args.iter()
            .filter_map(|arg| match arg {
                AstTy::Named(_, name) if name.starts_with('$') => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn constructor_slot_vars_for_impl(
        &self,
        trait_info: &TraitInfo,
        target_ast_ty: &AstTy,
        where_clause: Option<&ResolvedWhereClause>,
        target_param_vars: &HashMap<String, u32>,
        span: &Span,
    ) -> Result<(Vec<u32>, Vec<usize>), TypeError> {
        let target_params = Self::target_top_level_type_params(target_ast_ty);
        let mut mapped_slots = vec![None; trait_info.constructor_slots.len()];
        let mut mapped_params = HashSet::new();
        if let Some(clause) = where_clause {
            for constraint in &clause.constraints {
                let AstTy::Named(subject_span, subject) = &constraint.subject else {
                    continue;
                };
                for bound in &constraint.bounds {
                    let ResolvedWhereConstraintRhs::TraitSlot {
                        trait_id,
                        slot_name,
                        slot_ordinal,
                        span: bound_span,
                    } = bound
                    else {
                        continue;
                    };
                    if trait_id.unique_id != trait_info.id.unique_id {
                        continue;
                    }
                    if !target_params.contains(subject) {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "{} is not a top-level type parameter of the impl target",
                                subject
                            ),
                            span: subject_span.clone(),
                            hint: None,
                        });
                    }
                    let slot_ordinal = *slot_ordinal as usize;
                    if slot_ordinal >= trait_info.constructor_slots.len() {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Trait {} has no constructor slot {}",
                                trait_info.id.name, slot_name
                            ),
                            span: bound_span.clone(),
                            hint: None,
                        });
                    }
                    if mapped_slots[slot_ordinal].is_some() {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Trait constructor slot {} is mapped more than once",
                                slot_name
                            ),
                            span: bound_span.clone(),
                            hint: None,
                        });
                    }
                    if !mapped_params.insert(subject.clone()) {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Impl target parameter {} is mapped to more than one constructor slot",
                                subject
                            ),
                            span: bound_span.clone(),
                            hint: None,
                        });
                    }
                    mapped_slots[slot_ordinal] = target_param_vars.get(subject).copied();
                }
            }
        }

        if mapped_slots.iter().all(Option::is_none)
            && trait_info.constructor_slots.len() == 1
            && target_params.len() == 1
        {
            mapped_slots[0] = target_param_vars.get(&target_params[0]).copied();
        }

        if mapped_slots.iter().any(Option::is_none) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} does not satisfy Type<{}>: map every constructor slot in the impl where clause",
                    Self::surface_ast_ty_key(target_ast_ty),
                    trait_info.constructor_slots.join(", ")
                ),
                span: span.clone(),
                hint: Some(format!(
                    "Use `where $T: {}.{}` for each public constructor slot.",
                    trait_info.id.name,
                    trait_info.constructor_slots[0]
                )),
            });
        }

        let vars = mapped_slots.into_iter().flatten().collect::<Vec<_>>();
        let positions = vars
            .iter()
            .map(|var| {
                target_params
                    .iter()
                    .position(|param| target_param_vars.get(param) == Some(var))
                    .expect("mapped constructor variable must belong to target parameters")
            })
            .collect();
        Ok((vars, positions))
    }

    pub(super) fn resolve_trait_impl_head_tys(
        &mut self,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
    ) -> Result<(Vec<Ty>, Ty, Vec<u32>, HashMap<String, u32>), TypeError> {
        let mut tyvars = HashMap::new();
        let target_ty = self.resolve_signature_ast_ty_in_context(
            target_ast_ty,
            TypeSyntaxContext::General,
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
        let target_param_vars = tyvars
            .into_iter()
            .filter_map(|(name, ty)| match ty {
                Ty::Var(var) if name.starts_with('$') => Some((name, var)),
                _ => None,
            })
            .collect();
        Ok((trait_arg_tys, target_ty, type_param_vars, target_param_vars))
    }

    fn compiler_trait_target_names(&self, trait_name: &str) -> &'static [&'static str] {
        if self.trait_matches_short_name(trait_name, "Add")
            || self.trait_matches_short_name(trait_name, "Compare")
        {
            return &["Float", "Int"];
        }
        if self.trait_matches_short_name(trait_name, "Sub")
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
        if self.trait_matches_short_name(trait_name, "Show") {
            return &["Boolean", "Error", "Float", "Int", "String", "Unit"];
        }
        &[]
    }

    fn public_trait_target_display(info: &TraitImplInfo) -> Option<String> {
        let display = Self::surface_ast_ty_key(&info.target_ast_ty);
        let base = display.split('<').next().unwrap_or(display.as_str());
        if Self::builtin_type_has_public_trait_target_surface(base) {
            Some(display)
        } else {
            None
        }
    }

    fn builtin_type_has_public_trait_target_surface(base: &str) -> bool {
        if base == "Self" {
            return false;
        }
        builtin_type_usage_policy(base).map_or(true, |policy| policy.type_annotation_allowed)
    }

    fn surface_ast_ty_key(ty: &AstTy) -> String {
        match ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => Self::surface_name(name).into(),
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                Self::surface_name(name),
                args.iter()
                    .map(Self::surface_ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::surface_ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => format!(
                "({} -> {})",
                params
                    .iter()
                    .map(Self::surface_ast_ty_key)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::surface_ast_ty_key(ret)
            ),
        }
    }

    pub(super) fn trait_implementation_targets(&self, trait_name: &str) -> Vec<String> {
        let mut targets = std::collections::BTreeSet::new();
        for target in self.compiler_trait_target_names(trait_name) {
            targets.insert((*target).to_string());
        }
        let match_exact = trait_name.contains('<');
        for info in self.trait_impls.values() {
            let matches = if match_exact {
                self.trait_instance_key(&info.trait_id, &info.trait_args) == trait_name
            } else {
                self.trait_matches_short_name(&self.trait_key(&info.trait_id), trait_name)
            };
            if matches {
                if let Some(display) = Self::public_trait_target_display(info) {
                    targets.insert(display);
                }
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
                Self::format_trait_implementation_targets(&targets)
            )
        }
    }

    fn format_trait_implementation_targets(targets: &[String]) -> String {
        let tuple_arities = targets
            .iter()
            .map(|target| Self::generic_tuple_arity(target))
            .collect::<Vec<_>>();
        let Some(first_tuple_index) = tuple_arities.iter().position(Option::is_some) else {
            return targets.join(", ");
        };

        let arities = tuple_arities
            .iter()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();
        let ranges = Self::format_generic_tuple_arity_ranges(&arities);
        let tuple_summary = format!("Tuple(len={ranges})");
        let mut formatted = Vec::with_capacity(targets.len());
        for (index, target) in targets.iter().enumerate() {
            if index == first_tuple_index {
                formatted.push(tuple_summary.clone());
            }
            if tuple_arities[index].is_none() {
                formatted.push(target.clone());
            }
        }

        formatted.join(", ")
    }

    fn format_generic_tuple_arity_ranges(arities: &BTreeSet<usize>) -> String {
        let mut ranges = Vec::new();
        let mut iter = arities.iter().copied();
        let Some(first) = iter.next() else {
            return String::new();
        };

        let mut start = first;
        let mut end = first;
        for arity in iter {
            if arity == end.saturating_add(1) {
                end = arity;
            } else {
                ranges.push(Self::format_generic_tuple_arity_range(start, end));
                start = arity;
                end = arity;
            }
        }
        ranges.push(Self::format_generic_tuple_arity_range(start, end));
        ranges.join(", ")
    }

    fn format_generic_tuple_arity_range(start: usize, end: usize) -> String {
        if start == end {
            start.to_string()
        } else {
            format!("[{start}..{end}]")
        }
    }

    fn generic_tuple_arity(target: &str) -> Option<usize> {
        let target = target.trim();
        let inner = target.strip_prefix('(')?.strip_suffix(')')?;
        let elements = Self::split_type_list(inner)?;
        if elements.len() < 2
            || elements.iter().any(|element| {
                let element = element.trim();
                let Some(name) = element.strip_prefix('$') else {
                    return true;
                };
                name.is_empty()
                    || !name
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
            })
        {
            return None;
        }
        Some(elements.len())
    }

    fn split_type_list(source: &str) -> Option<Vec<&str>> {
        let mut elements = Vec::new();
        let mut start = 0;
        let mut depth = 0usize;

        for (index, character) in source.char_indices() {
            match character {
                '(' | '[' | '{' | '<' => depth += 1,
                ')' | ']' | '}' | '>' => {
                    depth = depth.checked_sub(1)?;
                }
                ',' if depth == 0 => {
                    elements.push(source.get(start..index)?.trim());
                    start = index + character.len_utf8();
                }
                _ => {}
            }
        }

        if depth != 0 {
            return None;
        }
        elements.push(source.get(start..)?.trim());
        Some(elements)
    }

    pub(super) fn tyvar_satisfies_compiler_trait(&self, _var: u32, _trait_name: &str) -> bool {
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
            Ty::Facet(..) => Some("Facet".into()),
            Ty::Tuple(items) if items.len() >= 2 => Some(format!("Tuple{}", items.len())),
            Ty::Func(_, _) => Some("Function".into()),
            Ty::Struct(name, _) | Ty::Record(name, _) => Some(name),
            Ty::Enum(name, _) => Some(name),
            _ => None,
        }
    }

    pub(super) fn trait_impl_exists(&mut self, trait_name: &str, ty: &Ty) -> bool {
        self.trait_impl_exists_for_args(trait_name, &[], ty)
    }

    pub(super) fn trait_impl_exists_for_args(
        &mut self,
        trait_name: &str,
        trait_args: &[Ty],
        ty: &Ty,
    ) -> bool {
        self.trait_obligation_satisfied_with_args(trait_name, trait_args, ty, &mut HashSet::new())
    }

    /// Solver identity is the resolved trait base plus its argument vector;
    /// formatted instance names are diagnostics only.
    fn trait_obligation_satisfied_with_args(
        &mut self,
        trait_name: &str,
        trait_args: &[Ty],
        ty: &Ty,
        visiting: &mut HashSet<ObligationKey>,
    ) -> bool {
        let receiver_ty = self.resolve_ty(ty);
        if let Ty::Var(var) = receiver_ty {
            let requested = self.trait_instance_key_from_tys(trait_name, trait_args);
            if self.rigid_tyvars.contains(&var) {
                return self.rigid_tyvar_entails_trait(var, &requested, &mut HashSet::new());
            }
            // An unbound inference variable is deliberately deferred.  This
            // must not manufacture a new bound; a later binding will run the
            // same solver against its concrete type.
            let pending = self.pending_trait_obligations.entry(var).or_default();
            let obligation = PendingTraitObligation {
                trait_id: trait_name.to_string(),
                args: trait_args.to_vec(),
                receiver: Ty::Var(var),
            };
            if !pending.contains(&obligation) {
                pending.push(obligation);
            }
            return true;
        }

        let key = ObligationKey {
            trait_name: trait_name.to_string(),
            trait_args: trait_args
                .iter()
                .map(|arg| self.canonical_ty_key(arg))
                .collect(),
            target: self.canonical_ty_key(&receiver_ty),
        };
        if !visiting.insert(key.clone()) {
            self.trait_obligation_cycle = Some(format!(
                "CyclicTraitObligation: {} for {}",
                self.trait_display_name(&self.trait_instance_key_from_tys(trait_name, trait_args)),
                self.ty_name(&receiver_ty)
            ));
            return false;
        }
        let result = (|| {
            for impl_key in self.trait_impl_candidate_keys(trait_name) {
                let Some(impl_info) = self.trait_impls.get(&impl_key).cloned() else {
                    continue;
                };
                let mut fresh = HashMap::new();
                let impl_target = self.instantiate_ty_with_fresh(&impl_info.target_ty, &mut fresh);
                let impl_trait_args = impl_info
                    .trait_arg_tys
                    .iter()
                    .map(|arg| self.instantiate_ty_with_fresh(arg, &mut fresh))
                    .collect::<Vec<_>>();
                // Older signature-bound storage still carries a rendered
                // instance key. Keep this as a boundary adapter only; all
                // new solver callers pass the base trait plus `trait_args`.
                let legacy_instance_key = trait_args.is_empty() && trait_name.contains('<');
                let args_match = if legacy_instance_key {
                    self.trait_display_name(&self.trait_instance_key_from_tys(
                        &self.trait_key(&impl_info.trait_id),
                        &impl_trait_args,
                    )) == self.trait_display_name(trait_name)
                } else {
                    impl_trait_args.len() == trait_args.len()
                        && impl_trait_args
                            .iter()
                            .zip(trait_args)
                            .all(|(candidate, requested)| {
                                self.types_compatible(candidate, requested)
                            })
                };
                if !args_match {
                    continue;
                }
                let before = self.substitutions.clone();
                let target_matches = self.types_compatible(&impl_target, &receiver_ty);
                let applicable =
                    target_matches && self.impl_body_obligations_hold(&impl_info, &fresh, visiting);
                self.substitutions = before;
                if applicable {
                    return true;
                }
            }
            self.compiler_trait_impl_exists(trait_name, &receiver_ty)
        })();
        visiting.remove(&key);
        result
    }

    /// Check declared bounds (including trait inheritance) without adding a
    /// constraint to the signature variable. This is also used by parent
    /// coverage, where the child impl's where clause is temporarily supplied
    /// as a proof environment.
    fn rigid_tyvar_entails_trait(
        &self,
        var: u32,
        requested: &str,
        visiting: &mut HashSet<String>,
    ) -> bool {
        self.tyvar_bound_names(var)
            .iter()
            .any(|bound| self.trait_bound_entails(bound, requested, visiting))
    }

    pub(super) fn trait_bound_entails(
        &self,
        bound: &str,
        requested: &str,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if bound == requested {
            return true;
        }
        if !visiting.insert(bound.to_string()) {
            return false;
        }
        let entails = self.traits.get(bound).is_some_and(|trait_info| {
            trait_info.parents.iter().any(|parent| {
                self.trait_bound_entails(&self.trait_key(&parent.trait_id), requested, visiting)
            })
        });
        visiting.remove(bound);
        entails
    }

    fn impl_body_obligations_hold(
        &mut self,
        impl_info: &TraitImplInfo,
        fresh: &HashMap<u32, Ty>,
        visiting: &mut HashSet<ObligationKey>,
    ) -> bool {
        for method in impl_info.methods.values() {
            for obligation in &method.body_obligations {
                let consumes_capability = impl_info.where_clause.as_ref().is_some_and(|clause| {
                    clause.constraints.iter().any(|constraint| {
                        let subject = match &constraint.subject {
                            AstTy::Named(_, name) if name == "Self" => self.resolve_ty(
                                &self.substitute_ty_with_mapping(&impl_info.target_ty, fresh),
                            ),
                            AstTy::Named(_, name) => impl_info
                                .type_param_vars_by_name
                                .get(name)
                                .and_then(|var| fresh.get(var))
                                .map(|ty| self.resolve_ty(ty))
                                .unwrap_or(Ty::Hole),
                            _ => Ty::Hole,
                        };
                        let obligation_receiver = self.resolve_ty(
                            &self.substitute_ty_with_mapping(&obligation.receiver, fresh),
                        );
                        let subject_matches = constraint.bounds.iter().any(|bound| {
                            let TypedWhereConstraintRhs::Trait { trait_id } = bound else {
                                return false;
                            };
                            self.capability_receiver_matches(
                                &self.trait_key(trait_id),
                                &subject,
                                &obligation_receiver,
                            )
                        });
                        subject_matches
                            && constraint.bounds.iter().any(|bound| {
                                let TypedWhereConstraintRhs::Trait { trait_id } = bound else {
                                    return false;
                                };
                                let capability = self.trait_key(trait_id);
                                let requested = Self::base_trait_key(&obligation.trait_id);
                                let capability = Self::base_trait_key(&capability);
                                capability == requested
                                    || self.trait_bound_entails(
                                        capability,
                                        requested,
                                        &mut HashSet::new(),
                                    )
                            })
                    })
                });
                if !consumes_capability {
                    continue;
                }
                let receiver =
                    self.resolve_ty(&self.substitute_ty_with_mapping(&obligation.receiver, fresh));
                let args = obligation
                    .trait_args
                    .iter()
                    .map(|arg| self.resolve_ty(&self.substitute_ty_with_mapping(arg, fresh)))
                    .collect::<Vec<_>>();
                if !self.trait_obligation_satisfied_with_args(
                    &obligation.trait_id,
                    &args,
                    &receiver,
                    visiting,
                ) {
                    return false;
                }
            }
        }
        true
    }

    pub(super) fn trait_dispatch_override(
        &self,
        trait_name: &str,
        method_name: &str,
        target_name: &str,
    ) -> Option<TraitDispatchTarget> {
        let target_name = Self::surface_name(target_name);
        if matches!(target_name, "Int" | "Float") {
            let op = if self.trait_matches_short_name(trait_name, "Add") && method_name == "add" {
                Some(BinOp::Add)
            } else if self.trait_matches_short_name(trait_name, "Sub") && method_name == "sub" {
                Some(BinOp::Sub)
            } else if self.trait_matches_short_name(trait_name, "Mul") && method_name == "mul" {
                Some(BinOp::Mul)
            } else {
                None
            };
            if let Some(op) = op {
                return Some(TraitDispatchTarget::BinOp(op));
            }
        }
        if self.trait_matches_short_name(trait_name, "Compare") && method_name == "compare" {
            return match target_name {
                "Int" => Some(TraitDispatchTarget::Builtin("__compare_int".into())),
                "Float" => Some(TraitDispatchTarget::Builtin("__compare_float".into())),
                _ => None,
            };
        }
        if self.trait_matches_short_name(trait_name, "Compare")
            && matches!(target_name, "Int" | "Float")
        {
            let op = match method_name {
                "lt" => Some(BinOp::Lt),
                "lte" => Some(BinOp::Lte),
                "gt" => Some(BinOp::Gt),
                "gte" => Some(BinOp::Gte),
                _ => None,
            };
            if let Some(op) = op {
                return Some(TraitDispatchTarget::BinOp(op));
            }
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
        if self.trait_matches_short_name(trait_name, "Eq")
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
        if self.trait_matches_short_name(trait_name, "Eq") {
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
    ) -> Result<(Vec<Ty>, Ty, Vec<Ty>, Vec<Ty>), TypeError> {
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
            .value_parameters
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
        self.apply_typed_where_trait_bounds(method.where_clause.as_ref(), &tyvars, Some(self_ty))?;
        let trait_args = trait_info
            .type_params
            .iter()
            .filter_map(|param| trait_head_bindings.get(&param.name).cloned())
            .collect::<Vec<_>>();
        let return_type_arguments = method
            .return_type_arguments
            .iter()
            .map(|argument| {
                self.resolve_trait_signature_ast_ty_in_context(
                    &argument.ty,
                    TypeSyntaxContext::General,
                    self_ty,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((params, ret, trait_args, return_type_arguments))
    }

    fn expand_trait_self_apps(
        &self,
        ty: Ty,
        target_ty: &Ty,
        constructor_slot_vars: &[u32],
    ) -> Result<Ty, TypeError> {
        Ok(match ty {
            Ty::SelfApp(args) if Self::constructor_application_parts(&args).is_some() => {
                Ty::SelfApp(
                    args.into_iter()
                        .map(|arg| {
                            self.expand_trait_self_apps(arg, target_ty, constructor_slot_vars)
                        })
                        .collect::<Result<_, _>>()?,
                )
            }
            Ty::SelfApp(args) => {
                if args.len() != constructor_slot_vars.len() {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Self requires {} constructor argument(s), got {}",
                            constructor_slot_vars.len(),
                            args.len()
                        ),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    });
                }
                let mapping = constructor_slot_vars
                    .iter()
                    .copied()
                    .zip(args)
                    .collect::<HashMap<_, _>>();
                self.substitute_ty_with_mapping(target_ty, &mapping)
            }
            Ty::List(inner) => Ty::List(Box::new(self.expand_trait_self_apps(
                *inner,
                target_ty,
                constructor_slot_vars,
            )?)),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .into_iter()
                    .map(|item| self.expand_trait_self_apps(item, target_ty, constructor_slot_vars))
                    .collect::<Result<_, _>>()?,
            ),
            Ty::Func(params, ret) => Ty::Func(
                params
                    .into_iter()
                    .map(|param| {
                        self.expand_trait_self_apps(param, target_ty, constructor_slot_vars)
                    })
                    .collect::<Result<_, _>>()?,
                Box::new(self.expand_trait_self_apps(*ret, target_ty, constructor_slot_vars)?),
            ),
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.expand_trait_self_apps(
                *inner,
                target_ty,
                constructor_slot_vars,
            )?)),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                kind,
                Box::new(self.expand_trait_self_apps(*source, target_ty, constructor_slot_vars)?),
                Box::new(self.expand_trait_self_apps(*focus, target_ty, constructor_slot_vars)?),
                Box::new(self.expand_trait_self_apps(
                    *update_source,
                    target_ty,
                    constructor_slot_vars,
                )?),
                Box::new(self.expand_trait_self_apps(
                    *update_focus,
                    target_ty,
                    constructor_slot_vars,
                )?),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name,
                params: params
                    .into_iter()
                    .map(|param| {
                        self.expand_trait_self_apps(param, target_ty, constructor_slot_vars)
                    })
                    .collect::<Result<_, _>>()?,
                ret: Box::new(self.expand_trait_self_apps(
                    *ret,
                    target_ty,
                    constructor_slot_vars,
                )?),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx,
                type_params,
                params: params
                    .into_iter()
                    .map(|param| {
                        self.expand_trait_self_apps(param, target_ty, constructor_slot_vars)
                    })
                    .collect::<Result<_, _>>()?,
                ret: Box::new(self.expand_trait_self_apps(
                    *ret,
                    target_ty,
                    constructor_slot_vars,
                )?),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name,
                fields
                    .into_iter()
                    .map(|(name, field)| {
                        Ok((
                            name,
                            self.expand_trait_self_apps(field, target_ty, constructor_slot_vars)?,
                        ))
                    })
                    .collect::<Result<_, TypeError>>()?,
            ),
            Ty::Record(name, fields) => Ty::Record(
                name,
                fields
                    .into_iter()
                    .map(|(name, field)| {
                        Ok((
                            name,
                            self.expand_trait_self_apps(field, target_ty, constructor_slot_vars)?,
                        ))
                    })
                    .collect::<Result<_, TypeError>>()?,
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name,
                args.into_iter()
                    .map(|arg| self.expand_trait_self_apps(arg, target_ty, constructor_slot_vars))
                    .collect::<Result<_, _>>()?,
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.expand_trait_self_apps(*ok, target_ty, constructor_slot_vars)?),
                Box::new(self.expand_trait_self_apps(*err, target_ty, constructor_slot_vars)?),
            ),
            other => other,
        })
    }

    fn alpha_normalized_signature(
        &self,
        return_type_arguments: &[Ty],
        params: &[Ty],
        ret: &Ty,
    ) -> (Vec<Ty>, Vec<Ty>, Ty) {
        let mut vars = Vec::new();
        for return_type_argument in return_type_arguments {
            Self::collect_ty_vars(return_type_argument, &mut vars);
        }
        for param in params {
            Self::collect_ty_vars(param, &mut vars);
        }
        Self::collect_ty_vars(ret, &mut vars);
        let mapping = vars
            .into_iter()
            .enumerate()
            .map(|(ordinal, var)| (var, Ty::Var(ordinal as u32)))
            .collect::<HashMap<_, _>>();
        (
            return_type_arguments
                .iter()
                .map(|param| self.substitute_ty_with_mapping(param, &mapping))
                .collect(),
            params
                .iter()
                .map(|param| self.substitute_ty_with_mapping(param, &mapping))
                .collect(),
            self.substitute_ty_with_mapping(ret, &mapping),
        )
    }

    pub(super) fn resolve_trait_impl_method_signature(
        &mut self,
        trait_info: &TraitInfo,
        trait_args: &[AstTy],
        method: &TraitImplMethodInfo,
        target_ast_ty: &AstTy,
        fallback_ret_ty: &AstTy,
        impl_where_clause: Option<&TypedWhereClause>,
    ) -> Result<(Vec<Ty>, Ty, Vec<u32>, Vec<Ty>), TypeError> {
        if trait_info.type_params.len() != trait_args.len() {
            return Err(TypeError {
                structured: None,
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
            .value_parameters
            .iter()
            .map(|param| {
                self.resolve_trait_signature_ast_ty_in_context(
                    &param.ty,
                    TypeSyntaxContext::General,
                    &self_ty,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let return_type_arguments = method
            .return_type_arguments
            .iter()
            .map(|argument| {
                self.resolve_trait_signature_ast_ty_in_context(
                    &argument.ty,
                    TypeSyntaxContext::General,
                    &self_ty,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret_source = method.ret_ty.as_ref().unwrap_or(fallback_ret_ty);
        let ret = self.resolve_trait_signature_ast_ty_in_context(
            ret_source,
            TypeSyntaxContext::FunctionReturn,
            &self_ty,
            &mut tyvars,
        )?;
        self.apply_typed_where_trait_bounds(method.where_clause.as_ref(), &tyvars, Some(&self_ty))?;
        self.apply_typed_where_trait_bounds(impl_where_clause, &tyvars, Some(&self_ty))?;
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
        Ok((params, ret, type_params, return_type_arguments))
    }

    pub(super) fn predeclare_traits(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        for stmt in stmts {
            let Resolved::TraitDef(span, id, type_params, where_clause, methods, _) = stmt else {
                continue;
            };
            let trait_key = self.trait_key(id);
            let constructor_slots = self.trait_constructor_slots(id, where_clause.as_ref())?;
            let parents = Self::trait_parents(where_clause.as_ref());
            let mut direct_parents = HashSet::new();
            for parent in &parents {
                let key = parent
                    .trait_id
                    .qualified_name
                    .as_deref()
                    .unwrap_or(parent.trait_id.name.as_str());
                if !direct_parents.insert(key.to_string()) {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait {} declares parent {} more than once",
                            id.name, parent.trait_id.name
                        ),
                        span: parent.trait_id.span.clone(),
                        hint: None,
                    });
                }
            }
            if !constructor_slots.is_empty() {
                for method in methods {
                    let mut return_type_argument_slots = HashSet::new();
                    for return_type_argument in &method.return_type_arguments {
                        Self::collect_constructor_signature_slots(
                            &return_type_argument.ty,
                            &mut return_type_argument_slots,
                        );
                    }
                    let mut value_param_slots = HashSet::new();
                    for param in &method.value_parameters {
                        Self::collect_constructor_signature_slots(
                            &param.ty,
                            &mut value_param_slots,
                        );
                    }
                    if let Some(slot) = return_type_argument_slots
                        .intersection(&value_param_slots)
                        .next()
                    {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Constructor trait method {} introduces {} through both ReturnTypeArguments and value arguments",
                                method.id.name, slot
                            ),
                            span: method.span.clone(),
                            hint: Some(
                                "Introduce each type variable through exactly one input channel."
                                    .into(),
                            ),
                        });
                    }
                    let mut input_slots = return_type_argument_slots;
                    input_slots.extend(value_param_slots);
                    let mut return_slots = HashSet::new();
                    Self::collect_constructor_signature_slots(&method.ret_ty, &mut return_slots);
                    if let Some(slot) = return_slots.difference(&input_slots).next() {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Constructor trait method {} has {} only in its return type",
                                method.id.name, slot
                            ),
                            span: method.span.clone(),
                            hint: Some(
                                "Introduce the type variable through ReturnTypeArguments or a value argument."
                                    .into(),
                            ),
                        });
                    }
                }
            }
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
                        return_type_arguments: method.return_type_arguments.clone(),
                        type_params: method.type_params.clone(),
                        value_parameters: method.value_parameters.clone(),
                        ret_ty: method.ret_ty.clone(),
                        where_clause: method.where_clause.as_ref().map(TypedWhereClause::from),
                        attrs: method.attrs.clone(),
                        body: method.body.clone(),
                        span: method.span.clone(),
                    },
                );
            }
            self.traits.insert(
                trait_key.clone(),
                TraitInfo {
                    id: id.clone(),
                    type_params: type_params.clone(),
                    where_clause: where_clause.as_ref().map(TypedWhereClause::from),
                    constructor_root: (!constructor_slots.is_empty()).then_some(trait_key),
                    constructor_slots,
                    parents,
                    methods: method_map,
                },
            );
            let _ = span;
        }

        self.resolve_trait_constraint_closure()?;

        for stmt in stmts {
            let Resolved::TraitImplDef(
                span,
                trait_id,
                trait_args,
                target_ast_ty,
                where_clause,
                methods,
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
            if trait_info.type_params.len() != trait_args.len() {
                return Err(TypeError {
                    structured: None,
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
            let (trait_arg_tys, target_ty, type_param_vars, target_param_vars) =
                self.resolve_trait_impl_head_tys(trait_args, target_ast_ty)?;
            let target_name = self.trait_target_name(&target_ty).ok_or_else(|| TypeError {
                structured: None,
                message: "trait impl target must be a concrete named type, tuple type, or function type".into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: Some("Use `impl Trait for Int` / `impl Trait for UserType` / `impl Trait for (Int, String)` / `impl Trait for ($A -> $B)`.".into()),
            })?;
            let (constructor_slot_vars, constructor_slot_positions) = self
                .constructor_slot_vars_for_impl(
                    &trait_info,
                    target_ast_ty,
                    where_clause.as_ref(),
                    &target_param_vars,
                    span,
                )?;

            let mut method_map = HashMap::new();
            for method in methods {
                method_map.insert(
                    method.method_name.clone(),
                    TraitImplMethodInfo {
                        method_name: method.method_name.clone(),
                        function_id: method.function_id.clone(),
                        return_type_arguments: method.return_type_arguments.clone(),
                        type_params: method.type_params.clone(),
                        value_parameters: method.value_parameters.clone(),
                        ret_ty: method.ret_ty.clone(),
                        where_clause: method.where_clause.as_ref().map(TypedWhereClause::from),
                        body: method.body.clone(),
                        attrs: method.attrs.clone(),
                        span: method.span.clone(),
                        display_name_override: None,
                        dispatch_override: self.trait_dispatch_override(
                            &trait_instance_key,
                            &method.method_name,
                            &target_name,
                        ),
                        is_builtin: method.is_builtin,
                        body_obligations: Vec::new(),
                    },
                );
            }

            for (required_method, trait_method) in &trait_info.methods {
                if method_map.contains_key(required_method) {
                    continue;
                }
                let Some(default_body) = trait_method.body.clone() else {
                    if trait_method.attrs.visibility == spire::ast::Visibility::Private {
                        continue;
                    }
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait impl {} for {} is missing method `{}`",
                            trait_id.name, target_name, required_method
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                };
                method_map.insert(
                    required_method.clone(),
                    TraitImplMethodInfo {
                        method_name: required_method.clone(),
                        function_id: self.synthesized_default_method_id(
                            &trait_instance_key,
                            &trait_info.id,
                            &target_name,
                            required_method,
                            &trait_method.span,
                        ),
                        return_type_arguments: trait_method.return_type_arguments.clone(),
                        type_params: trait_method.type_params.clone(),
                        value_parameters: trait_method.value_parameters.clone(),
                        ret_ty: None,
                        where_clause: trait_method.where_clause.clone(),
                        body: default_body,
                        attrs: trait_method.attrs.clone(),
                        span: trait_method.span.clone(),
                        display_name_override: Some(Self::trait_method_display_name(
                            &trait_info.id,
                            required_method,
                        )),
                        dispatch_override: self.trait_dispatch_override(
                            &trait_instance_key,
                            required_method,
                            &target_name,
                        ),
                        is_builtin: false,
                        body_obligations: Vec::new(),
                    },
                );
            }

            for method_name in method_map.keys() {
                if !trait_info.methods.contains_key(method_name) {
                    return Err(TypeError {
                        structured: None,
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
                let trait_method =
                    trait_info
                        .methods
                        .get(method_name)
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!(
                                "Trait impl {} for {} defines unknown method `{}`",
                                trait_id.name, target_name, method_name
                            ),
                            span: impl_method.span.clone(),
                            hint: None,
                        })?;
                if trait_method.type_params.len() != impl_method.type_params.len() {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait impl method {}::{} has incompatible type parameter arity",
                            trait_id.name, method_name
                        ),
                        span: impl_method.span.clone(),
                        hint: None,
                    });
                }

                if trait_method.attrs.visibility != impl_method.attrs.visibility {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait impl method {}::{} has incompatible visibility",
                            trait_id.name, method_name
                        ),
                        span: impl_method.span.clone(),
                        hint: Some("A trait private helper must be implemented with `defp`, and a public trait method with `def`".into()),
                    });
                }

                let (trait_params, trait_ret, trait_head_vars, trait_return_type_arguments) =
                    self.resolve_trait_method_signature(&trait_info, trait_method, &target_ty)?;
                let trait_head_mapping = trait_head_vars
                    .into_iter()
                    .zip(trait_arg_tys.iter().cloned())
                    .filter_map(|(from, to)| match from {
                        Ty::Var(var) => Some((var, to)),
                        _ => None,
                    })
                    .collect::<HashMap<_, _>>();
                let trait_params = trait_params
                    .into_iter()
                    .map(|param| {
                        let param = self.substitute_ty_with_mapping(&param, &trait_head_mapping);
                        self.expand_trait_self_apps(param, &target_ty, &constructor_slot_vars)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let trait_ret = self.substitute_ty_with_mapping(&trait_ret, &trait_head_mapping);
                let trait_ret =
                    self.expand_trait_self_apps(trait_ret, &target_ty, &constructor_slot_vars)?;
                let trait_return_type_arguments = trait_return_type_arguments
                    .into_iter()
                    .map(|param| {
                        let param = self.substitute_ty_with_mapping(&param, &trait_head_mapping);
                        self.expand_trait_self_apps(param, &target_ty, &constructor_slot_vars)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let (impl_params, impl_ret, _, impl_return_type_arguments) = self
                    .resolve_trait_impl_method_signature(
                        &trait_info,
                        trait_args,
                        impl_method,
                        target_ast_ty,
                        &trait_method.ret_ty,
                        where_clause.as_ref().map(TypedWhereClause::from).as_ref(),
                    )?;

                if trait_params.len() != impl_params.len() {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait impl method {}::{} has incompatible arity",
                            trait_id.name, method_name
                        ),
                        span: impl_method.span.clone(),
                        hint: None,
                    });
                }

                let expected_signature = self.alpha_normalized_signature(
                    &trait_return_type_arguments,
                    &trait_params,
                    &trait_ret,
                );
                let impl_signature = self.alpha_normalized_signature(
                    &impl_return_type_arguments,
                    &impl_params,
                    &impl_ret,
                );
                if expected_signature != impl_signature {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait impl method {}::{} has an incompatible signature",
                            trait_id.name, method_name
                        ),
                        span: impl_method.span.clone(),
                        hint: Some(format!(
                            "Expected ({}) -> {}, got ({}) -> {}",
                            trait_params
                                .iter()
                                .map(|ty| self.ty_name(ty))
                                .collect::<Vec<_>>()
                                .join(", "),
                            self.ty_name(&trait_ret),
                            impl_params
                                .iter()
                                .map(|ty| self.ty_name(ty))
                                .collect::<Vec<_>>()
                                .join(", "),
                            self.ty_name(&impl_ret)
                        )),
                    });
                }

                if self.canonical_where_clause_key(
                    trait_method.where_clause.as_ref(),
                    &Self::method_constraint_vars(
                        &trait_method.value_parameters,
                        &trait_method.ret_ty,
                        &trait_method.return_type_arguments,
                        &trait_method.type_params,
                    ),
                ) != self.canonical_where_clause_key(
                    impl_method.where_clause.as_ref(),
                    &Self::method_constraint_vars(
                        &impl_method.value_parameters,
                        impl_method.ret_ty.as_ref().unwrap_or(&trait_method.ret_ty),
                        &impl_method.return_type_arguments,
                        &impl_method.type_params,
                    ),
                ) {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Trait impl method {}::{} has incompatible trait constraints",
                            trait_id.name, method_name
                        ),
                        span: impl_method.span.clone(),
                        hint: Some("The impl method must use the same where constraints as the trait method".into()),
                    });
                }
            }

            let existing_impls = self.trait_impls.values().cloned().collect::<Vec<_>>();
            for existing in existing_impls {
                if self.trait_key(&existing.trait_id) != trait_key {
                    continue;
                }
                if self.trait_impl_patterns_overlap(
                    &trait_arg_tys,
                    &target_ty,
                    &existing.trait_arg_tys,
                    &existing.target_ty,
                ) {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Overlapping trait impls for {}: {} and {}",
                            trait_id.name,
                            Self::surface_ast_ty_key(target_ast_ty),
                            Self::surface_ast_ty_key(&existing.target_ast_ty)
                        ),
                        span: span.clone(),
                        hint: Some("Trait impl patterns must be structurally disjoint; Surtr does not use specialization or declaration-order dispatch.".into()),
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
                let peers = self
                    .trait_impls
                    .values()
                    .filter(|existing| self.trait_key(&existing.trait_id) == peer_trait_key)
                    .cloned()
                    .collect::<Vec<_>>();
                let peer = peers.into_iter().find(|existing| {
                    self.trait_impl_patterns_overlap(
                        &trait_arg_tys,
                        &target_ty,
                        &existing.trait_arg_tys,
                        &existing.target_ty,
                    )
                });
                if let Some(existing) = peer {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "{} and {} cannot both be implemented for {} -> {}",
                            trait_id.name,
                            existing
                                .trait_id
                                .name
                                .rsplit("::")
                                .next()
                                .unwrap_or(&existing.trait_id.name),
                            Self::surface_ast_ty_key(target_ast_ty),
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

            let impl_key = self.trait_impl_storage_key(trait_id, trait_args, target_ast_ty);
            self.trait_impls.insert(
                impl_key.clone(),
                TraitImplInfo {
                    trait_id: trait_id.clone(),
                    trait_args: trait_args.clone(),
                    trait_arg_tys,
                    target_name,
                    target_ast_ty: target_ast_ty.clone(),
                    target_ty,
                    where_clause: where_clause.as_ref().map(TypedWhereClause::from),
                    type_param_vars,
                    type_param_vars_by_name: target_param_vars,
                    constructor_slot_vars,
                    constructor_slot_positions,
                    methods: method_map,
                },
            );
            self.index_trait_impl(impl_key);
        }

        let impls = self.trait_impls.values().cloned().collect::<Vec<_>>();
        for child_impl in &impls {
            self.validate_parent_impl_chain(child_impl, &mut HashSet::new())?;
        }

        Ok(())
    }

    /// Constraint equality for trait methods is alpha-equivalence, not a
    /// comparison of source generic spellings or declaration order.
    fn canonical_where_clause_key(
        &self,
        clause: Option<&TypedWhereClause>,
        vars: &HashMap<String, usize>,
    ) -> Option<String> {
        let canonical_ty = |ty: &AstTy| Self::canonical_constraint_ty_key(ty, &vars);
        clause.map(|clause| {
            let mut constraints = clause
                .constraints
                .iter()
                .map(|constraint| {
                    let mut bounds = constraint
                        .bounds
                        .iter()
                        .map(|bound| match bound {
                            TypedWhereConstraintRhs::Trait { trait_id: id, .. } => format!(
                                "trait:{}",
                                id.qualified_name.as_deref().unwrap_or(&id.name)
                            ),
                            TypedWhereConstraintRhs::TypeConstructor { slots, .. } => format!(
                                "type:{}",
                                slots
                                    .iter()
                                    .map(&canonical_ty)
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                            TypedWhereConstraintRhs::TraitSlot {
                                trait_id,
                                slot_name,
                                slot_ordinal,
                                ..
                            } => format!(
                                "slot:{}:{}:{}",
                                trait_id.qualified_name.as_deref().unwrap_or(&trait_id.name),
                                slot_name,
                                slot_ordinal
                            ),
                        })
                        .collect::<Vec<_>>();
                    bounds.sort();
                    bounds.dedup();
                    format!("{}:{}", canonical_ty(&constraint.subject), bounds.join("+"))
                })
                .collect::<Vec<_>>();
            constraints.sort();
            constraints.dedup();
            constraints.join(";")
        })
    }

    fn method_constraint_vars(
        params: &[ResolvedValueParameter],
        ret: &AstTy,
        return_type_arguments: &[ResolvedReturnTypeArgument],
        type_params: &[ResolvedTypeParam],
    ) -> HashMap<String, usize> {
        let mut names = type_params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();
        for ty in params
            .iter()
            .map(|param| &param.ty)
            .chain(std::iter::once(ret))
            .chain(return_type_arguments.iter().map(|argument| &argument.ty))
        {
            Self::collect_constraint_var_names(ty, &mut names);
        }
        names
            .into_iter()
            .filter(|name| name.starts_with('$'))
            .enumerate()
            .map(|(ordinal, name)| (name, ordinal))
            .collect()
    }

    fn collect_constraint_var_names(ty: &AstTy, names: &mut Vec<String>) {
        match ty {
            AstTy::Named(_, name) => {
                if name.starts_with('$') && !names.iter().any(|known| known == name) {
                    names.push(name.clone());
                }
            }
            AstTy::Generic(_, _, args) | AstTy::Tuple(_, args) => {
                for arg in args {
                    Self::collect_constraint_var_names(arg, names);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_constraint_var_names(param, names);
                }
                Self::collect_constraint_var_names(ret, names);
            }
            AstTy::ImplTrait(..) => {}
        }
    }

    fn canonical_constraint_ty_key(ty: &AstTy, vars: &HashMap<String, usize>) -> String {
        match ty {
            AstTy::Named(_, name) => vars
                .get(name)
                .map(|ordinal| format!("Var({ordinal})"))
                .unwrap_or_else(|| format!("Named({})", Self::surface_name(name))),
            AstTy::ImplTrait(_, name) => format!("Impl({})", Self::surface_name(name)),
            AstTy::Generic(_, name, args) => format!(
                "Generic({};{})",
                Self::surface_name(name),
                args.iter()
                    .map(|arg| Self::canonical_constraint_ty_key(arg, vars))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            AstTy::Tuple(_, items) => format!(
                "Tuple({})",
                items
                    .iter()
                    .map(|item| Self::canonical_constraint_ty_key(item, vars))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            AstTy::Func(_, params, ret) => format!(
                "Func({}->{})",
                params
                    .iter()
                    .map(|param| Self::canonical_constraint_ty_key(param, vars))
                    .collect::<Vec<_>>()
                    .join(","),
                Self::canonical_constraint_ty_key(ret, vars)
            ),
        }
    }

    fn validate_parent_impl_chain(
        &mut self,
        child_impl: &TraitImplInfo,
        visiting: &mut HashSet<(String, String)>,
    ) -> Result<(), TypeError> {
        let child_trait_key = self.trait_key(&child_impl.trait_id);
        let visit_key = (child_trait_key.clone(), child_impl.target_name.clone());
        if !visiting.insert(visit_key.clone()) {
            return Ok(());
        }
        let Some(child_trait) = self.traits.get(&child_trait_key).cloned() else {
            return Ok(());
        };
        for parent in &child_trait.parents {
            let parent_key = self.trait_key(&parent.trait_id);
            self.traits.get(&parent_key).ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown parent trait: {}", parent.trait_id.name),
                span: parent.trait_id.span.clone(),
                hint: None,
            })?;
            let parent_candidates = self
                .trait_impls
                .values()
                .filter(|impl_info| self.trait_key(&impl_info.trait_id) == parent_key)
                .cloned()
                .collect::<Vec<_>>();
            let parent_impl = parent_candidates
                .into_iter()
                .find(|impl_info| self.parent_impl_covers_child(impl_info, child_impl))
                .ok_or_else(|| TypeError {
                    structured: None,
                    message: format!(
                        "Trait impl {} for {} requires parent impl {} for the same target",
                        child_impl.trait_id.name, child_impl.target_name, parent.trait_id.name
                    ),
                    span: child_impl.trait_id.span.clone(),
                    hint: None,
                })?;
            if child_impl.constructor_slot_positions != parent_impl.constructor_slot_positions {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Trait impl {} for {} must use the same constructor slot mapping as parent {}",
                        child_impl.trait_id.name, child_impl.target_name, parent.trait_id.name
                    ),
                    span: child_impl.trait_id.span.clone(),
                    hint: None,
                });
            }
            self.validate_parent_impl_chain(&parent_impl, visiting)?;
        }
        visiting.remove(&visit_key);
        Ok(())
    }

    /// Parent-trait validation is universal: every instance described by the
    /// child head (under the child's declared bounds) must be described by a
    /// single parent impl.  It is not the existential overlap test used for
    /// coherence.
    fn parent_impl_covers_child(
        &mut self,
        parent_impl: &TraitImplInfo,
        child_impl: &TraitImplInfo,
    ) -> bool {
        let before_substitutions = self.substitutions.clone();
        let before_rigid = self.rigid_tyvars.clone();
        let mut child_vars = Vec::new();
        Self::collect_ty_vars(&child_impl.target_ty, &mut child_vars);
        for arg in &child_impl.trait_arg_tys {
            Self::collect_ty_vars(arg, &mut child_vars);
        }
        self.rigid_tyvars.extend(child_vars);
        // Coverage is quantified over the child impl's instances. Its where
        // clause is therefore an assumption available while proving the
        // parent's requirements, but must not escape into the checker-wide
        // declaration environment.
        let mut fresh = HashMap::new();
        let parent_target = self.instantiate_ty_with_fresh(&parent_impl.target_ty, &mut fresh);
        // A declaration bound `Self: Parent` names the Parent capability
        // family, not a zero-argument Parent instance. Instantiate the full
        // parent head so its variables remain available to its where clause,
        // but coverage is determined by the target and declared obligations.
        for arg in &parent_impl.trait_arg_tys {
            self.instantiate_ty_with_fresh(arg, &mut fresh);
        }
        let head_covers = self.types_compatible(&parent_target, &child_impl.target_ty);
        let obligations_hold =
            head_covers && self.parent_where_is_entailed_by_child(parent_impl, child_impl, &fresh);

        self.substitutions = before_substitutions;
        self.rigid_tyvars = before_rigid;
        obligations_hold
    }

    /// Prove a parent's `where` requirements from the child's declared
    /// assumptions.  This is deliberately a local proof environment: unlike
    /// the old implementation it never writes assumptions into
    /// `tyvar_bounds`, where they could accidentally affect another impl.
    fn parent_where_is_entailed_by_child(
        &mut self,
        parent_impl: &TraitImplInfo,
        child_impl: &TraitImplInfo,
        fresh: &HashMap<u32, Ty>,
    ) -> bool {
        let Some(parent_where) = &parent_impl.where_clause else {
            return true;
        };

        parent_where.constraints.iter().all(|constraint| {
            let parent_subject =
                match &constraint.subject {
                    AstTy::Named(_, subject) if subject == "Self" => Some(self.resolve_ty(
                        &self.substitute_ty_with_mapping(&parent_impl.target_ty, fresh),
                    )),
                    AstTy::Named(_, subject) => parent_impl
                        .type_param_vars_by_name
                        .get(subject)
                        .and_then(|var| fresh.get(var))
                        .map(|ty| self.resolve_ty(ty)),
                    _ => None,
                };
            let Some(parent_subject) = parent_subject else {
                return false;
            };

            constraint.bounds.iter().all(|bound| {
                let TypedWhereConstraintRhs::Trait { trait_id } = bound else {
                    return true;
                };
                let required = self.trait_key(trait_id);
                self.child_where_entails(child_impl, &parent_subject, &required)
                    || self.trait_obligation_satisfied_with_args(
                        &required,
                        &[],
                        &parent_subject,
                        &mut HashSet::new(),
                    )
            })
        })
    }

    fn child_where_entails(
        &mut self,
        child_impl: &TraitImplInfo,
        subject: &Ty,
        required: &str,
    ) -> bool {
        let Some(child_where) = &child_impl.where_clause else {
            return false;
        };
        child_where.constraints.iter().any(|constraint| {
            let child_subject = match &constraint.subject {
                AstTy::Named(_, name) if name == "Self" => child_impl.target_ty.clone(),
                AstTy::Named(_, name) => match child_impl.type_param_vars_by_name.get(name) {
                    Some(var) => Ty::Var(*var),
                    None => return false,
                },
                _ => return false,
            };
            self.resolve_ty(&child_subject) == self.resolve_ty(subject)
                && constraint.bounds.iter().any(|bound| match bound {
                    TypedWhereConstraintRhs::Trait { trait_id } => {
                        self.trait_key(trait_id) == required
                    }
                    _ => false,
                })
        })
    }

    pub(super) fn predeclare_functions(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut fun_idx = self.env.next_fun_idx;
        let mut trait_impl_keys_in_stmts = HashSet::new();

        for stmt in stmts {
            let Resolved::TraitImplDef(_, trait_id, trait_args, target_ast_ty, _, _) = stmt else {
                continue;
            };
            let (_, target_ty, _, _) =
                self.resolve_trait_impl_head_tys(trait_args, target_ast_ty)?;
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
            trait_impl_keys_in_stmts.insert(self.trait_impl_storage_key(
                trait_id,
                trait_args,
                target_ast_ty,
            ));
        }

        for stmt in stmts {
            match stmt {
                Resolved::BuiltinDecl(_, id, _, params, ret_ty, where_clause, _) => {
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
                    if let Some(clause) = where_clause {
                        self.builtin_contracts.insert(
                            id.unique_id,
                            BuiltinContract {
                                where_clause: TypedWhereClause::from(clause),
                                type_vars: tyvars.clone(),
                                param_tys: param_tys.clone(),
                            },
                        );
                    }
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
                Resolved::Def(
                    _,
                    id,
                    return_type_arguments,
                    params,
                    ret_ty,
                    where_clause,
                    _,
                    attrs,
                ) => {
                    self.register_function_id(id);
                    if !attrs.builtin
                        && Self::split_impl_method_name(
                            id.qualified_name.as_deref().unwrap_or(&id.name),
                        )
                        .is_none()
                    {
                        Self::reject_return_only_signature_slots(
                            params,
                            ret_ty.as_ref(),
                            &id.span,
                        )?;
                    }
                    let mut tyvars = HashMap::new();
                    for argument in return_type_arguments {
                        self.resolve_def_signature_ast_ty_in_context(
                            id,
                            &argument.ty,
                            TypeSyntaxContext::General,
                            &mut tyvars,
                        )?;
                    }
                    let param_tys = params
                        .iter()
                        .map(|param| {
                            let param_ty = self.resolve_def_signature_ast_ty_in_context(
                                id,
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
                        Some(ty) => self.resolve_def_signature_ast_ty_in_context(
                            id,
                            ty,
                            TypeSyntaxContext::FunctionReturn,
                            &mut tyvars,
                        )?,
                        None => Ty::Unit,
                    };
                    self.apply_resolved_where_trait_bounds(where_clause.as_ref(), &tyvars, None)?;
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
                                .map(Self::ast_ty_span)
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
                    let type_params = Self::signature_type_param_vars(
                        type_params,
                        &tyvars,
                        std::slice::from_ref(&param_ty),
                        &ret,
                    );
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
            let left_key =
                self.trait_impl_storage_key(&left.trait_id, &left.trait_args, &left.target_ast_ty);
            let right_key = self.trait_impl_storage_key(
                &right.trait_id,
                &right.trait_args,
                &right.target_ast_ty,
            );
            left_key.cmp(&right_key)
        });

        for trait_impl in trait_impls {
            let impl_key = self.trait_impl_storage_key(
                &trait_impl.trait_id,
                &trait_impl.trait_args,
                &trait_impl.target_ast_ty,
            );
            if !trait_impl_keys_in_stmts.contains(&impl_key) {
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
            methods.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));

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
                let (param_tys, ret, type_params, _) = self.resolve_trait_impl_method_signature(
                    &trait_info,
                    &trait_impl.trait_args,
                    method,
                    &trait_impl.target_ast_ty,
                    &trait_method.ret_ty,
                    trait_impl.where_clause.as_ref(),
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

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn generic_tuple_targets_are_compressed_into_an_arity_range() {
        let targets = (2..=8)
            .map(|arity| {
                let elements = (0..arity)
                    .map(|index| format!("$A{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({elements})")
            })
            .chain(["Duration".into(), "Float".into(), "Int".into()])
            .collect::<Vec<_>>();

        assert_eq!(
            Checker::format_trait_implementation_targets(&targets),
            "Tuple(len=[2..8]), Duration, Float, Int"
        );
    }

    #[test]
    fn concrete_and_mixed_tuple_targets_are_not_compressed() {
        let targets = vec![
            "(String, Int)".into(),
            "($A0, Int)".into(),
            "($A0, $A1)".into(),
            "($A0, $A1, $A2)".into(),
        ];

        assert_eq!(
            Checker::format_trait_implementation_targets(&targets),
            "(String, Int), ($A0, Int), Tuple(len=[2..3])"
        );
    }

    #[test]
    fn generic_tuple_ranges_are_derived_from_the_full_arity_set() {
        let targets = vec![
            "Duration".into(),
            "($A0, $A1)".into(),
            "Int".into(),
            "($A0, $A1, $A2, $A3)".into(),
            "($A0, $A1, $A2)".into(),
            "($A0, $A1, $A2, $A3, $A4, $A5)".into(),
            "($A0, $A1, $A2, $A3, $A4, $A5, $A6)".into(),
            "($A0, $A1, $A2, $A3, $A4, $A5, $A6, $A7)".into(),
        ];

        assert_eq!(
            Checker::format_trait_implementation_targets(&targets),
            "Duration, Tuple(len=[2..4], [6..8]), Int"
        );
    }

    #[test]
    fn singleton_generic_tuple_arities_share_one_summary_entry() {
        let targets = vec![
            "Int".into(),
            "($A0, $A1)".into(),
            "Duration".into(),
            "($A0, $A1, $A2, $A3)".into(),
            "($A0, $A1, $A2, $A3, $A4)".into(),
            "($A0, $A1, $A2, $A3, $A4, $A5)".into(),
            "($A0, $A1, $A2, $A3, $A4, $A5, $A6)".into(),
            "($A0, $A1, $A2, $A3, $A4, $A5, $A6, $A7)".into(),
        ];

        assert_eq!(
            Checker::format_trait_implementation_targets(&targets),
            "Int, Tuple(len=2, [4..8]), Duration"
        );
    }

    #[test]
    fn public_trait_target_surface_uses_builtin_type_usage_policy() {
        assert!(Checker::builtin_type_has_public_trait_target_surface(
            "String"
        ));
        assert!(Checker::builtin_type_has_public_trait_target_surface(
            "User"
        ));
        assert!(!Checker::builtin_type_has_public_trait_target_surface(
            "Self"
        ));
        assert!(!Checker::builtin_type_has_public_trait_target_surface(
            "StandbyInit"
        ));
    }
}
