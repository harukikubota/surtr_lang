use super::*;
use sindr::names::TraitImplPolicy;

const SYNTHETIC_DEFAULT_METHOD_UID_BASE: u32 = 0x6000_0000;

#[derive(Default, Debug)]
struct PredeclareTraitWork {
    trait_setup: std::time::Duration,
    impl_registration: std::time::Duration,
    constructor_mappings: std::time::Duration,
    method_signatures: std::time::Duration,
    added_impls: usize,
    coherence_entries: usize,
    peer_entries: usize,
    method_lookup_entries: usize,
    mapping_declarations: usize,
}

enum ParentImplCoverage {
    HeadMismatch,
    ConstraintMismatch,
    Covers,
}

fn is_generated_derive_impl(methods: &[ResolvedTraitImplMethod]) -> bool {
    !methods.is_empty()
        && methods
            .iter()
            .all(|method| method.function_id.compiler_generated)
}

impl Checker {
    pub(super) fn validate_type_shape_clause(
        clause: Option<&ResolvedWhereClause>,
        trait_definition: bool,
        enclosing_impl_trait: Option<&ResolvedId>,
        constructor_trait_ids: &HashSet<u32>,
        trait_names: &HashSet<String>,
    ) -> Result<(), TypeError> {
        let Some(clause) = clause else {
            return Ok(());
        };
        let mut shape_seen = false;
        for constraint in &clause.constraints {
            if let AstTy::Named(span, subject) = &constraint.subject {
                if trait_names.contains(Self::surface_name(subject))
                    && constraint
                        .bounds
                        .iter()
                        .any(|bound| matches!(bound, ResolvedWhereConstraintRhs::Trait { .. }))
                {
                    return Err(
                        crate::checker::signatures::invalid_trait_constraint_subject_error(
                            Self::surface_name(subject),
                            span.clone(),
                        ),
                    );
                }
            }
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

    pub(in crate::checker) fn where_constraint_subject_ty(
        &mut self,
        subject: &AstTy,
        tyvars: &HashMap<String, Ty>,
        self_ty: Option<&Ty>,
        span: &Span,
        generated_derive: bool,
    ) -> Result<Ty, TypeError> {
        if generated_derive && !matches!(subject, AstTy::Named(_, _)) {
            let self_ty = self_ty.ok_or_else(|| {
                TypeError::new("Generated derive requires an impl target", span.clone())
            })?;
            let mut bindings = tyvars.clone();
            let resolved = self.resolve_trait_signature_ast_ty_in_context(
                subject,
                TypeSyntaxContext::General,
                self_ty,
                &mut bindings,
            )?;
            if bindings.keys().any(|name| !tyvars.contains_key(name)) {
                return Err(TypeError::new(
                    "Generated derive constraint references a type variable outside its impl target",
                    span.clone(),
                ));
            }
            return Ok(resolved);
        }
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
        let constructor_trait = self
            .traits
            .get(&trait_key)
            .map(|info| !info.constructor_slots.is_empty())
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown trait: {}", trait_id.name),
                span: trait_id.span.clone(),
                hint: None,
            })?;
        match self.resolve_ty(subject) {
            Ty::Var(var) => {
                self.register_tyvar_bound(var, &trait_key);
                if constructor_trait {
                    self.constructor_witness_traits.insert(var, trait_key);
                }
                Ok(())
            }
            // A concrete `Self` is already guarded by trait dispatch/impl
            // validation. Signature where clauses primarily add bounds to
            // variables that must survive instantiation at call sites.
            _ => Ok(()),
        }
    }

    pub(in crate::checker) fn apply_resolved_where_trait_bounds(
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
                false,
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
        generated_derive: bool,
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
                generated_derive,
            )?;
            for bound in &constraint.bounds {
                if let TypedWhereConstraintRhs::Trait { trait_id } = bound {
                    self.apply_where_trait_bound(&subject, trait_id)?;
                }
            }
        }
        Ok(())
    }

    fn apply_trait_head_where_bounds(
        &mut self,
        where_clause: Option<&TypedWhereClause>,
        trait_head_bindings: &HashMap<String, Ty>,
    ) -> Result<(), TypeError> {
        let Some(where_clause) = where_clause else {
            return Ok(());
        };
        for constraint in &where_clause.constraints {
            let AstTy::Named(_, subject) = &constraint.subject else {
                continue;
            };
            let Some(subject_ty) = trait_head_bindings.get(subject) else {
                continue;
            };
            for bound in &constraint.bounds {
                if let TypedWhereConstraintRhs::Trait { trait_id } = bound {
                    self.apply_where_trait_bound(subject_ty, trait_id)?;
                }
            }
        }
        Ok(())
    }

    fn synthetic_default_method_symbol(impl_declaration_id: u32, trait_method_uid: u32) -> String {
        format!("__default_impl_{impl_declaration_id}_trait_method_{trait_method_uid}")
    }

    fn synthetic_default_method_uid(
        &self,
        qualified_name: &str,
        locally_reserved: &HashSet<u32>,
    ) -> u32 {
        if let Some(existing) = self.function_ids_by_name.get(qualified_name) {
            return existing.unique_id;
        }
        let mut occupied = self
            .function_ids_by_name
            .values()
            .map(|id| id.unique_id)
            .chain(self.trait_impls.values().flat_map(|implementation| {
                implementation
                    .methods
                    .values()
                    .map(|method| method.function_id.unique_id)
            }))
            .chain(locally_reserved.iter().copied())
            .collect::<HashSet<_>>();
        let mut candidate = SYNTHETIC_DEFAULT_METHOD_UID_BASE;
        while !occupied.insert(candidate) {
            candidate = candidate
                .checked_add(1)
                .expect("synthetic default method UID namespace exhausted");
        }
        candidate
    }

    fn trait_method_display_name(trait_id: &ResolvedId, method_name: &str) -> String {
        format!("{}::{}", Self::surface_name(&trait_id.name), method_name)
    }

    fn synthesized_default_method_id(
        &self,
        impl_declaration_id: u32,
        trait_method: &ResolvedId,
        span: &Span,
        locally_reserved: &HashSet<u32>,
    ) -> ResolvedId {
        let qualified_name =
            Self::synthetic_default_method_symbol(impl_declaration_id, trait_method.unique_id);
        let unique_id = self.synthetic_default_method_uid(&qualified_name, locally_reserved);
        ResolvedId {
            name: trait_method.name.clone(),
            qualified_name: Some(qualified_name),
            symbol_info: None,
            unique_id,
            compiler_generated: true,
            span: span.clone(),
        }
    }

    pub(super) fn struct_new_contract_error(
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

    pub(super) fn struct_new_return_allowed(
        &mut self,
        expected_self_ty: &Ty,
        ret_ty: &Ty,
    ) -> Result<bool, TypeError> {
        let resolved_ret = self.resolve_ty(ret_ty);
        if self.types_compatible(expected_self_ty, &resolved_ret)? {
            return Ok(true);
        }
        Ok(match resolved_ret {
            Ty::Result(ok, _) => self.types_compatible(expected_self_ty, ok.as_ref())?,
            _ => false,
        })
    }

    pub(in crate::checker) fn trait_key(&self, id: &ResolvedId) -> String {
        id.qualified_name.clone().unwrap_or_else(|| id.name.clone())
    }

    pub(in crate::checker) fn ast_ty_key(ast_ty: &AstTy) -> String {
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

    pub(in crate::checker) fn trait_instance_key(
        &self,
        trait_id: &ResolvedId,
        trait_args: &[AstTy],
    ) -> String {
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

    pub(in crate::checker) fn trait_impl_for_declaration(
        &self,
        declaration_id: u32,
    ) -> Option<Arc<TraitImplInfo>> {
        self.trait_impls
            .values()
            .find(|info| info.declaration_key.declaration_id == declaration_id)
            .cloned()
    }

    pub(in crate::checker) fn trait_display_name(&self, trait_name: &str) -> String {
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

    pub(in crate::checker) fn trait_key_by_short_name(&self, short_name: &str) -> Option<String> {
        let mut candidates = self
            .traits
            .values()
            .filter(|info| info.id.name == short_name)
            .map(|info| self.trait_key(&info.id))
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        (candidates.len() == 1).then(|| candidates.remove(0))
    }

    pub(in crate::checker) fn trait_matches_short_name(
        &self,
        trait_name: &str,
        short_name: &str,
    ) -> bool {
        self.trait_key_by_short_name(short_name)
            .as_deref()
            .is_some_and(|key| key == trait_name)
    }

    pub(in crate::checker) fn trait_instance_key_from_tys(
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

    pub(in crate::checker) fn collect_ty_vars(ty: &Ty, out: &mut Vec<u32>) {
        match ty {
            Ty::Var(var) => {
                if !out.contains(var) {
                    out.push(*var);
                }
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => Self::collect_ty_vars(inner, out),
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
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for argument in &nominal.arguments {
                    Self::collect_ty_vars(argument, out);
                }
                for (_, field_ty) in nominal {
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
            | Ty::ProcessMarker(_) => {}
        }
    }

    pub(super) fn signature_type_param_vars(
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

    pub(super) fn resolve_trait_constraint_closure(&mut self) -> Result<(), TypeError> {
        fn visit(
            key: &str,
            traits: &TraitDefinitions,
            visiting: &mut Vec<String>,
            resolved: &mut HashMap<String, Vec<String>>,
        ) -> Result<Vec<String>, TypeError> {
            if let Some(result) = resolved.get(key) {
                return Ok(result.clone());
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
            let result = slots;
            resolved.insert(key.to_string(), result.clone());
            Ok(result)
        }

        let keys = self.traits.keys().cloned().collect::<Vec<_>>();
        let mut resolved = HashMap::new();
        for key in &keys {
            visit(key, &self.traits, &mut Vec::new(), &mut resolved)?;
        }
        for (key, slots) in resolved {
            if let Some(info) = self.traits.get_mut(&key) {
                if info.constructor_slots != slots {
                    Arc::make_mut(info).constructor_slots = slots;
                }
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

    fn explicit_constructor_slot_vars_for_impl(
        &self,
        trait_info: &TraitInfo,
        target_ast_ty: &AstTy,
        where_clause: Option<&TypedWhereClause>,
        target_param_vars: &HashMap<String, u32>,
        span: &Span,
        root: bool,
    ) -> Result<(Vec<Option<u32>>, Vec<Option<usize>>), TypeError> {
        let target_params = Self::target_top_level_type_params(target_ast_ty);
        let mut mapped_slots = vec![None; trait_info.constructor_slots.len()];
        let mut mapped_params = HashSet::new();
        if let Some(clause) = where_clause {
            for constraint in &clause.constraints {
                let AstTy::Named(subject_span, subject) = &constraint.subject else {
                    continue;
                };
                for bound in &constraint.bounds {
                    let TypedWhereConstraintRhs::TraitSlot {
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

        if root
            && mapped_slots.iter().all(Option::is_none)
            && trait_info.constructor_slots.len() == 1
            && target_params.len() == 1
        {
            mapped_slots[0] = target_param_vars.get(&target_params[0]).copied();
        }

        if root && mapped_slots.iter().any(Option::is_none) {
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

        let positions = mapped_slots.iter().map(|var| var.map(|var| {
            let AstTy::Generic(_, _, arguments) = target_ast_ty else {
                unreachable!("mapped constructor target is an application")
            };
            arguments.iter().position(|argument| matches!(argument, AstTy::Named(_, name) if target_param_vars.get(name) == Some(&var)))
                .expect("mapped constructor variable belongs to target parameters")
        })).collect();
        Ok((mapped_slots, positions))
    }

    fn root_constructor_slot_vars_for_impl(
        &self,
        trait_info: &TraitInfo,
        target_ast_ty: &AstTy,
        where_clause: Option<&TypedWhereClause>,
        target_param_vars: &HashMap<String, u32>,
        span: &Span,
    ) -> Result<(Vec<u32>, Vec<usize>), TypeError> {
        let (vars, positions) = self.explicit_constructor_slot_vars_for_impl(
            trait_info,
            target_ast_ty,
            where_clause,
            target_param_vars,
            span,
            true,
        )?;
        Ok((
            vars.into_iter().map(Option::unwrap).collect(),
            positions.into_iter().map(Option::unwrap).collect(),
        ))
    }

    pub(in crate::checker) fn trait_head_parameter_constructor_bound(
        &self,
        trait_info: &TraitInfo,
        parameter_name: &str,
    ) -> Option<String> {
        trait_info
            .where_clause
            .as_ref()?
            .constraints
            .iter()
            .find_map(|constraint| {
                matches!(&constraint.subject, AstTy::Named(_, name) if name == parameter_name)
                    .then(|| {
                        constraint.bounds.iter().find_map(|bound| match bound {
                            TypedWhereConstraintRhs::Trait { trait_id }
                                if self.traits.get(&self.trait_key(trait_id)).is_some_and(
                                    |bound_info| !bound_info.constructor_slots.is_empty(),
                                ) =>
                            {
                                Some(self.trait_key(trait_id))
                            }
                            _ => None,
                        })
                    })
                    .flatten()
            })
    }

    pub(in crate::checker) fn resolve_trait_impl_head_tys(
        &mut self,
        trait_info: &TraitInfo,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
    ) -> Result<(Vec<Ty>, Ty, Vec<u32>, HashMap<String, u32>), TypeError> {
        let mut tyvars = HashMap::new();
        let target_ty = self.resolve_signature_like_ast_ty_in_context(
            target_ast_ty,
            TypeSyntaxContext::General,
            &mut tyvars,
            crate::checker::type_syntax::SignatureTyMode::ImplHead { self_ty: None },
        )?;
        let trait_arg_tys = trait_info
            .type_params
            .iter()
            .zip(trait_args)
            .map(|(parameter, arg)| {
                if self
                    .trait_head_parameter_constructor_bound(trait_info, &parameter.name)
                    .is_some()
                    && !matches!(arg, AstTy::Named(_, name) if name.starts_with('$'))
                {
                    self.resolve_type_constructor_head(arg)
                } else {
                    self.resolve_signature_like_ast_ty_in_context(
                        arg,
                        TypeSyntaxContext::General,
                        &mut tyvars,
                        crate::checker::type_syntax::SignatureTyMode::ImplHead {
                            self_ty: Some(&target_ty),
                        },
                    )
                }
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

    pub(in crate::checker) fn resolve_trait_method_signature(
        &mut self,
        trait_info: &TraitInfo,
        method: &TraitMethodInfo,
        self_ty: &Ty,
    ) -> Result<(Vec<Ty>, Ty, Vec<Ty>, Vec<Ty>, MethodTypeEnvironment), TypeError> {
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
        self.apply_trait_head_where_bounds(trait_info.where_clause.as_ref(), &trait_head_bindings)?;
        let mut tyvars = trait_head_bindings.clone();
        self.seed_signature_type_params(&method.type_params, &mut tyvars);
        let mut direct_constructor_inputs =
            crate::checker::signatures::DirectConstructorInputs::default();
        let mut return_type_arguments = Vec::new();
        for argument in &method.return_type_arguments {
            let ty = self.resolve_trait_signature_ty_in_context(
                &argument.ty,
                TypeSyntaxContext::General,
                self_ty,
                &mut tyvars,
            )?;
            crate::checker::signatures::remember_direct_constructor_input(
                self,
                &argument.ty,
                &ty,
                &mut direct_constructor_inputs,
            );
            return_type_arguments.push(ty);
        }
        let params = method
            .value_parameters
            .iter()
            .map(|param| {
                let ty = self.resolve_trait_parameter_contract(
                    &param.ty,
                    self_ty,
                    &mut tyvars,
                    trait_info.standard_lazy_contract,
                )?;
                crate::checker::signatures::remember_direct_constructor_input(
                    self,
                    &param.ty,
                    &ty,
                    &mut direct_constructor_inputs,
                );
                Ok(
                    crate::checker::signatures::coalesce_direct_constructor_inputs(
                        self,
                        ty,
                        &direct_constructor_inputs,
                    ),
                )
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let ret = self.resolve_trait_signature_ty_in_context(
            &method.ret_ty,
            TypeSyntaxContext::FunctionReturn,
            self_ty,
            &mut tyvars,
        )?;
        crate::checker::signatures::remember_direct_constructor_input(
            self,
            &method.ret_ty,
            &ret,
            &mut direct_constructor_inputs,
        );
        let ret = crate::checker::signatures::coalesce_direct_constructor_inputs(
            self,
            ret,
            &direct_constructor_inputs,
        );
        self.apply_typed_where_trait_bounds(
            method.where_clause.as_ref(),
            &tyvars,
            Some(self_ty),
            false,
        )?;
        for ty in &params {
            self.validate_nominal_type_well_formed(ty, &method.span, false)?;
        }
        self.validate_nominal_type_well_formed(&ret, &method.span, false)?;
        for ty in &return_type_arguments {
            self.validate_nominal_type_well_formed(ty, &method.span, false)?;
        }
        let trait_args = trait_info
            .type_params
            .iter()
            .filter_map(|param| trait_head_bindings.get(&param.name).cloned())
            .collect::<Vec<_>>();
        Ok((
            params,
            ret,
            trait_args.clone(),
            return_type_arguments,
            MethodTypeEnvironment {
                head_bindings: trait_head_bindings,
                bindings: tyvars,
                trait_arguments: trait_args,
                self_ty: self_ty.clone(),
                direct_inputs: direct_constructor_inputs,
            },
        ))
    }

    pub(in crate::checker) fn expand_trait_self_apps(
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
            Ty::ExtractorClosure(inner) => Ty::ExtractorClosure(Box::new(
                self.expand_trait_self_apps(*inner, target_ty, constructor_slot_vars)?,
            )),
            Ty::MatchResult(inner) => Ty::MatchResult(Box::new(self.expand_trait_self_apps(
                *inner,
                target_ty,
                constructor_slot_vars,
            )?)),
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
            Ty::Pid(inner) => Ty::Pid(Box::new(self.expand_trait_self_apps(
                *inner,
                target_ty,
                constructor_slot_vars,
            )?)),
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
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
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
                NominalType::new(
                    fields
                        .arguments
                        .into_iter()
                        .map(|argument| {
                            self.expand_trait_self_apps(argument, target_ty, constructor_slot_vars)
                        })
                        .collect::<Result<_, TypeError>>()?,
                    fields
                        .fields
                        .into_iter()
                        .map(|(name, field)| {
                            Ok((
                                name,
                                self.expand_trait_self_apps(
                                    field,
                                    target_ty,
                                    constructor_slot_vars,
                                )?,
                            ))
                        })
                        .collect::<Result<_, TypeError>>()?,
                ),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name,
                NominalType::new(
                    fields
                        .arguments
                        .into_iter()
                        .map(|argument| {
                            self.expand_trait_self_apps(argument, target_ty, constructor_slot_vars)
                        })
                        .collect::<Result<_, TypeError>>()?,
                    fields
                        .fields
                        .into_iter()
                        .map(|(name, field)| {
                            Ok((
                                name,
                                self.expand_trait_self_apps(
                                    field,
                                    target_ty,
                                    constructor_slot_vars,
                                )?,
                            ))
                        })
                        .collect::<Result<_, TypeError>>()?,
                ),
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

    pub(in crate::checker) fn resolve_trait_impl_method_signature(
        &mut self,
        trait_info: &TraitInfo,
        trait_args: &[AstTy],
        method: &TraitImplMethodInfo,
        target_ast_ty: &AstTy,
        fallback_ret_ty: &sigil::resolved::ResolvedSignatureTy,
        impl_where_clause: Option<&TypedWhereClause>,
        generated_derive: bool,
        constructor_slot_positions: &[usize],
    ) -> Result<(Vec<Ty>, Ty, Vec<u32>, Vec<Ty>, MethodTypeEnvironment), TypeError> {
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
            let resolved = if self
                .trait_head_parameter_constructor_bound(trait_info, &param.name)
                .is_some()
                && !matches!(arg, AstTy::Named(_, name) if name.starts_with('$'))
            {
                self.resolve_type_constructor_head(arg)?
            } else {
                self.resolve_trait_signature_ast_ty_in_context(
                    arg,
                    TypeSyntaxContext::General,
                    &self_ty,
                    &mut tyvars,
                )?
            };
            trait_head_bindings.insert(param.name.clone(), resolved);
        }
        self.apply_trait_head_where_bounds(trait_info.where_clause.as_ref(), &trait_head_bindings)?;
        self.apply_typed_where_trait_bounds(
            impl_where_clause,
            &tyvars,
            Some(&self_ty),
            generated_derive,
        )?;

        let head_bindings = tyvars.clone();
        // User impl source has its own namespace. Only synthesized default
        // bodies still originate in the contract's source namespace.
        if method.display_name_override.is_some() {
            tyvars.extend(trait_head_bindings.clone());
        }
        self.seed_signature_type_params(&method.type_params, &mut tyvars);
        let mut direct_constructor_inputs =
            crate::checker::signatures::DirectConstructorInputs::default();
        let mut return_type_arguments = Vec::new();
        for argument in &method.return_type_arguments {
            let ty = self.resolve_trait_signature_ty_in_context(
                &argument.ty,
                TypeSyntaxContext::General,
                &self_ty,
                &mut tyvars,
            )?;
            crate::checker::signatures::remember_direct_constructor_input(
                self,
                &argument.ty,
                &ty,
                &mut direct_constructor_inputs,
            );
            return_type_arguments.push(ty);
        }
        let params = method
            .value_parameters
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let allows_lazy = trait_info.standard_lazy_contract
                    && trait_info
                        .methods
                        .get(&method.method_name)
                        .and_then(|declaration| declaration.value_parameters.get(index))
                        .is_some_and(|parameter| {
                            Self::lazy_parameter_inner(parameter.ty.syntax()).is_some()
                        });
                let ty = self.resolve_trait_parameter_contract(
                    &param.ty,
                    &self_ty,
                    &mut tyvars,
                    allows_lazy,
                )?;
                crate::checker::signatures::remember_direct_constructor_input(
                    self,
                    &param.ty,
                    &ty,
                    &mut direct_constructor_inputs,
                );
                Ok(
                    crate::checker::signatures::coalesce_direct_constructor_inputs(
                        self,
                        ty,
                        &direct_constructor_inputs,
                    ),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret_signature_source = method.ret_ty.as_ref().unwrap_or(fallback_ret_ty);
        let mut return_bindings = tyvars.clone();
        if method.ret_ty.is_none() {
            return_bindings.extend(trait_head_bindings.clone());
        }
        let ret = self.resolve_trait_signature_ty_in_context(
            ret_signature_source,
            TypeSyntaxContext::FunctionReturn,
            &self_ty,
            &mut return_bindings,
        )?;
        crate::checker::signatures::remember_direct_constructor_input(
            self,
            ret_signature_source,
            &ret,
            &mut direct_constructor_inputs,
        );
        let ret = crate::checker::signatures::coalesce_direct_constructor_inputs(
            self,
            ret,
            &direct_constructor_inputs,
        );
        self.apply_typed_where_trait_bounds(
            method.where_clause.as_ref(),
            &tyvars,
            Some(&self_ty),
            false,
        )?;
        self.apply_typed_where_trait_bounds(
            impl_where_clause,
            &tyvars,
            Some(&self_ty),
            generated_derive,
        )?;
        let slots = self.slot_vars_at_positions(
            target_ast_ty,
            &head_bindings,
            constructor_slot_positions,
            &method.span,
        )?;
        let params = params
            .into_iter()
            .map(|ty| self.expand_trait_self_apps(ty, &self_ty, &slots))
            .collect::<Result<Vec<_>, _>>()?;
        let ret = self.expand_trait_self_apps(ret, &self_ty, &slots)?;
        let return_type_arguments = return_type_arguments
            .into_iter()
            .map(|ty| self.expand_trait_self_apps(ty, &self_ty, &slots))
            .collect::<Result<Vec<_>, _>>()?;
        self.validate_nominal_type_well_formed(&self_ty, &method.span, false)?;
        for ty in &params {
            self.validate_nominal_type_well_formed(ty, &method.span, false)?;
        }
        self.validate_nominal_type_well_formed(&ret, &method.span, false)?;
        for ty in &return_type_arguments {
            self.validate_nominal_type_well_formed(ty, &method.span, false)?;
        }
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
        Ok((
            params,
            ret,
            type_params,
            return_type_arguments,
            MethodTypeEnvironment {
                bindings: tyvars,
                head_bindings,
                trait_arguments: trait_info
                    .type_params
                    .iter()
                    .filter_map(|param| trait_head_bindings.get(&param.name).cloned())
                    .collect(),
                self_ty,
                direct_inputs: direct_constructor_inputs,
            },
        ))
    }

    pub(in crate::checker) fn predeclare_traits(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        let mut work = PredeclareTraitWork::default();
        let result = self.predeclare_traits_with_work(stmts, &mut work);
        if self.profiler.enabled() {
            eprintln!(
                "scar-predeclare trait_setup={:.3}ms impl_registration={:.3}ms constructor_mappings={:.3}ms method_signatures={:.3}ms added_impls={} coherence_entries={} peer_entries={} method_lookup_entries={} mapping_declarations={}",
                work.trait_setup.as_secs_f64() * 1000.0,
                work.impl_registration.as_secs_f64() * 1000.0,
                work.constructor_mappings.as_secs_f64() * 1000.0,
                work.method_signatures.as_secs_f64() * 1000.0,
                work.added_impls,
                work.coherence_entries,
                work.peer_entries,
                work.method_lookup_entries,
                work.mapping_declarations,
            );
        }
        result
    }

    fn predeclare_traits_with_work(
        &mut self,
        stmts: &[Resolved],
        work: &mut PredeclareTraitWork,
    ) -> Result<(), TypeError> {
        let phase_start = self.profiler.start();
        let mut declared_trait = false;
        // Keep suffix declaration lookup local to this compile. Retained impl
        // metadata and the checkpoint representation need no additional index.
        let mut declared_impl_keys = HashMap::new();
        for stmt in stmts {
            let Resolved::TraitDef(span, id, type_params, where_clause, methods, _) = stmt else {
                continue;
            };
            declared_trait = true;
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
            let trait_info = TraitInfo {
                id: id.clone(),
                compiler_owned_failure: self.enforce_builtin_type_contracts
                    && trait_key == "MonadFail",
                compiler_owned_extraction: self.enforce_builtin_type_contracts
                    && trait_key == "MonadExtractor",
                compiler_owned_equality: self.enforce_builtin_type_contracts
                    && id.name == "Eq"
                    && trait_key == "Eq",
                standard_lazy_contract: self.enforce_builtin_type_contracts,
                type_params: type_params.clone(),
                where_clause: where_clause.as_ref().map(TypedWhereClause::from),
                constructor_slots,
                parents,
                methods: method_map,
            };
            self.traits.insert(trait_key, Arc::new(trait_info));
            self.canonical_traits_by_uid.take();

            let _ = span;
        }

        if declared_trait {
            self.resolve_trait_constraint_closure()?;
            for statement in stmts {
                if let Resolved::TraitDef(span, id, ..) = statement {
                    if self
                        .traits
                        .get(&self.trait_key(id))
                        .is_some_and(|info| info.compiler_owned_extraction)
                    {
                        self.canonical_monad_extractor_key(span)?;
                    }
                }
            }

            // Trait helpers participate in the same canonical callable
            // registry as user functions and non-intrinsic builtins. Resolve
            // their signatures only after inherited constructor slots are
            // closed so parameter bounds such as `$M: Monad` retain their
            // constructor witness metadata.
            for stmt in stmts {
                let Resolved::TraitDef(_, id, ..) = stmt else {
                    continue;
                };
                let trait_key = self.trait_key(id);
                let trait_info = self
                    .traits
                    .get(&trait_key)
                    .cloned()
                    .expect("predeclared Trait must remain registered");
                for method in trait_info.methods.values() {
                    let self_ty = self.env.fresh_tyvar();
                    let (param_tys, ret, _, return_type_argument_tys, _) =
                        self.resolve_trait_method_signature(&trait_info, method, &self_ty)?;
                    self.callable_signatures.insert(
                        method.id.unique_id,
                        Arc::new(crate::checker::signatures::canonical_callable_signature(
                            self,
                            &method.id,
                            &method.return_type_arguments,
                            &method.value_parameters,
                            return_type_argument_tys.as_slice(),
                            param_tys.as_slice(),
                            ret,
                            sindr::signature::CanonicalConstraintSet::default(),
                            sindr::signature::RuntimeTarget::TraitDispatch(method.id.unique_id),
                            sindr::signature::CallableDeclarationKind::TraitMethod,
                        )?),
                    );
                }
            }
        }

        if let Some(start) = phase_start {
            work.trait_setup = start.elapsed();
        }
        let phase_start = self.profiler.start();
        for stmt in stmts {
            let Resolved::TraitImplDef(
                span,
                declaration_id,
                trait_id,
                trait_args,
                target_ast_ty,
                where_clause,
                methods,
            ) = stmt
            else {
                continue;
            };
            work.added_impls += 1;
            let generated_derive = is_generated_derive_impl(methods);

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
            let (trait_arg_tys, target_ty, type_param_vars, target_param_vars) =
                self.resolve_trait_impl_head_tys(&trait_info, trait_args, target_ast_ty)?;
            let policy = self.trait_impl_policy_for_ty(&target_ty);
            if policy != TraitImplPolicy::Open {
                let target = self.ty_name(&target_ty);
                let reason = match policy {
                    TraitImplPolicy::CompilerOwned => "reserved for compiler-owned capabilities",
                    TraitImplPolicy::Forbidden => "forbidden by the type's trait policy",
                    TraitImplPolicy::Open => unreachable!(),
                };
                return Err(TypeError::from_structured(diagnostics::StructuredDiagnostic {
                    reason: diagnostics::TypeDiagnosticReason::TraitImplementationForbidden.into(),
                    origin: diagnostics::DiagnosticOrigin::Declaration,
                    data: diagnostics::DiagnosticData::Policy(diagnostics::PolicyData {
                        policy: diagnostics::TypePolicy::TraitImplementation,
                        subject: Some(target),
                        expected_type: Some(trait_id.name.clone()),
                        actual_type: Some(reason.into()),
                        stage: Some("declaration".into()),
                        entrypoint: None,
                    }),
                    primary: diagnostics::SourceFact::untyped(
                        diagnostics::SourceRole::Impl,
                        diagnostics::SourceId(0),
                        Self::ast_ty_span(target_ast_ty).clone(),
                    ),
                    related: vec![diagnostics::SourceFact::untyped(
                        diagnostics::SourceRole::Trait,
                        diagnostics::SourceId(0),
                        trait_id.span.clone(),
                    )],
                    remediation: Some(diagnostics::Remediation::Help {
                        text: "Use a user-defined outer type for this trait implementation when appropriate.".into(),
                    }),
                }));
            }
            let typed_impl_clause = where_clause.as_ref().map(TypedWhereClause::from);
            let impl_head_bindings = target_param_vars
                .iter()
                .map(|(name, var)| (name.clone(), Ty::Var(*var)))
                .collect::<HashMap<_, _>>();
            self.apply_typed_where_trait_bounds(
                typed_impl_clause.as_ref(),
                &impl_head_bindings,
                Some(&target_ty),
                generated_derive,
            )?;
            let target_name = self.trait_target_name(&target_ty).ok_or_else(|| TypeError {
                structured: None,
                message: "trait impl target must be a concrete named type, tuple type, or function type".into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: Some("Use `impl Trait for Int` / `impl Trait for UserType` / `impl Trait for (Int, String)` / `impl Trait for ($A -> $B)`.".into()),
            })?;
            let has_constructor_parent = trait_info.parents.iter().any(|parent| {
                !self.traits[&self.trait_key(&parent.trait_id)]
                    .constructor_slots
                    .is_empty()
            });
            let (constructor_slot_vars, constructor_slot_positions) = if has_constructor_parent {
                (Vec::new(), Vec::new())
            } else {
                self.root_constructor_slot_vars_for_impl(
                    &trait_info,
                    target_ast_ty,
                    typed_impl_clause.as_ref(),
                    &target_param_vars,
                    span,
                )?
            };

            let head_environment = MethodTypeEnvironment {
                head_bindings: target_param_vars
                    .iter()
                    .map(|(name, var)| (name.clone(), Ty::Var(*var)))
                    .collect(),
                bindings: target_param_vars
                    .iter()
                    .map(|(name, var)| (name.clone(), Ty::Var(*var)))
                    .collect(),
                trait_arguments: trait_arg_tys.clone(),
                self_ty: target_ty.clone(),
                direct_inputs: crate::checker::signatures::DirectConstructorInputs::default(),
            };
            let (head_type_list, canonical_environment) =
                self.canonical_impl_head(trait_args, target_ast_ty, &head_environment, span)?;
            let impl_constraints = self
                .canonical_method_list(
                    &[],
                    &[],
                    &Ty::Unit,
                    &[],
                    &[],
                    &AstTy::Named(span.clone(), "Unit".into()),
                    typed_impl_clause.as_ref(),
                    &head_environment,
                    &canonical_environment,
                    None,
                    false,
                )?
                .where_constraints;
            let impl_key =
                CanonicalTraitImplPatternKey::from_head(trait_id.unique_id, &head_type_list);
            // Coherence only compares patterns of the same base trait. The
            // existing index includes both retained and earlier suffix impls.
            for existing_key in self.trait_impl_candidate_keys(&trait_key) {
                work.coherence_entries += 1;
                let existing = &self.trait_impls[&existing_key];
                if Self::canonical_patterns_overlap(&impl_key, &existing.declaration_key.pattern) {
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

            // Only the two conversion traits can have an exclusive peer.
            // Other impls do not need a whole-trait short-name uniqueness scan.
            let exclusive_peer = if trait_info.id.name == "Convert"
                && self.trait_matches_short_name(&trait_key, "Convert")
            {
                self.trait_key_by_short_name("TryConvert")
            } else if trait_info.id.name == "TryConvert"
                && self.trait_matches_short_name(&trait_key, "TryConvert")
            {
                self.trait_key_by_short_name("Convert")
            } else {
                None
            };
            if let Some(peer_trait_key) = exclusive_peer {
                let peer = self
                    .trait_impl_candidate_keys(&peer_trait_key)
                    .into_iter()
                    .inspect(|_| work.peer_entries += 1)
                    .map(|key| &self.trait_impls[&key])
                    .find(|existing| {
                        Self::canonical_patterns_overlap(
                            &impl_key,
                            &existing.declaration_key.pattern,
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

            self.trait_impls.insert(
                impl_key.clone(),
                Arc::new(TraitImplInfo {
                    declaration_key: TraitImplDeclarationKey {
                        pattern: impl_key.clone(),
                        declaration_id: *declaration_id,
                    },
                    head_type_list,
                    impl_constraints,
                    method_signature_lists: HashMap::new(),
                    trait_id: trait_id.clone(),
                    trait_args: trait_args.clone(),
                    trait_arg_tys,
                    target_name,
                    target_ast_ty: target_ast_ty.clone(),
                    target_ty,
                    where_clause: where_clause.as_ref().map(TypedWhereClause::from),
                    generated_derive,
                    type_param_vars,
                    type_param_vars_by_name: target_param_vars,
                    constructor_slot_vars,
                    constructor_slot_positions,
                    methods: HashMap::new(),
                }),
            );
            self.constructor_mapping_resolution
                .pending
                .insert(impl_key.clone());
            self.index_trait_impl(impl_key.clone());
            declared_impl_keys.insert(*declaration_id, impl_key);
        }

        if let Some(start) = phase_start {
            work.impl_registration = start.elapsed();
        }
        let phase_start = self.profiler.start();
        if !declared_impl_keys.is_empty() {
            work.mapping_declarations = self.constructor_mapping_resolution.pending.len();
            self.resolve_impl_constructor_mappings()?;
        }
        if let Some(start) = phase_start {
            work.constructor_mappings = start.elapsed();
        }
        let phase_start = self.profiler.start();

        // Methods consume the declaration's resolved mapping, never re-infer it.
        for stmt in stmts {
            let Resolved::TraitImplDef(
                span,
                declaration_id,
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
            let trait_info = self.traits[&trait_key].clone();
            work.method_lookup_entries += 1;
            let impl_key = declared_impl_keys
                .get(declaration_id)
                .expect("collected suffix impl head");
            let impl_info = self.trait_impls[impl_key].clone();
            let generated_derive = impl_info.generated_derive;
            let target_ty = impl_info.target_ty.clone();
            let target_name = impl_info.target_name.clone();
            let trait_arg_tys = impl_info.trait_arg_tys.clone();
            let constructor_slot_vars = impl_info.constructor_slot_vars.clone();
            let constructor_slot_positions = impl_info.constructor_slot_positions.clone();
            let typed_impl_clause = impl_info.where_clause.clone();
            let head_type_list = impl_info.head_type_list.clone();
            self.validate_nominal_type_well_formed(&target_ty, span, false)?;
            for argument in &trait_arg_tys {
                self.validate_nominal_type_well_formed(argument, span, false)?;
            }
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
                        dispatch_override: None,
                        is_builtin: method.is_builtin,
                        body_obligations: Vec::new(),
                        instantiation_contract: None,
                        resolved_signature: None,
                    },
                );
            }

            let mut required_methods = trait_info.methods.iter().collect::<Vec<_>>();
            required_methods.sort_by_key(|(_, method)| method.id.unique_id);
            let mut locally_reserved_default_uids = HashSet::new();
            for (required_method, trait_method) in required_methods {
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
                            *declaration_id,
                            &trait_method.id,
                            &trait_method.span,
                            &locally_reserved_default_uids,
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
                        dispatch_override: None,
                        is_builtin: false,
                        body_obligations: Vec::new(),
                        instantiation_contract: None,
                        resolved_signature: None,
                    },
                );
                locally_reserved_default_uids
                    .insert(method_map[required_method].function_id.unique_id);
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

            let mut method_signature_lists = HashMap::new();
            let mut instantiation_contracts = HashMap::new();
            let mut resolved_signatures = HashMap::new();
            let mut dispatch_overrides = HashMap::new();
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

                let (
                    trait_params,
                    trait_ret,
                    trait_head_vars,
                    trait_return_type_arguments,
                    mut contract_env,
                ) = self.resolve_trait_method_signature(&trait_info, trait_method, &target_ty)?;
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
                let (impl_params, impl_ret, impl_type_params, impl_return_type_arguments, impl_env) =
                    self.resolve_trait_impl_method_signature(
                        &trait_info,
                        trait_args,
                        impl_method,
                        target_ast_ty,
                        &trait_method.ret_ty,
                        where_clause.as_ref().map(TypedWhereClause::from).as_ref(),
                        generated_derive,
                        &constructor_slot_positions,
                    )?;
                let generated_boolean_equality = generated_derive
                    && trait_info.compiler_owned_equality
                    && matches!(
                        self.canonical_request(&target_ty)?.head,
                        CanonicalTypeHead::Builtin(sindr::names::TypeName::Boolean)
                    );
                if impl_method.is_builtin || generated_boolean_equality {
                    dispatch_overrides.insert(
                        method_name.clone(),
                        self.trait_dispatch_override(
                            &trait_info.id,
                            method_name,
                            &target_ty,
                            &impl_method.value_parameters,
                            &impl_params,
                            &impl_ret,
                            &impl_method.span,
                        )?,
                    );
                }

                for ty in contract_env.bindings.values_mut() {
                    *ty = self.substitute_ty_with_mapping(ty, &trait_head_mapping);
                    *ty = self.expand_trait_self_apps(
                        ty.clone(),
                        &target_ty,
                        &constructor_slot_vars,
                    )?;
                }
                let expected_env = self.canonical_contract_environment(
                    &contract_env,
                    &trait_info,
                    &head_type_list,
                    &constructor_slot_positions,
                )?;
                let expected = self.canonical_method_list(
                    &trait_return_type_arguments,
                    &trait_params,
                    &trait_ret,
                    &trait_method.return_type_arguments,
                    &trait_method.value_parameters,
                    &trait_method.ret_ty,
                    trait_method.where_clause.as_ref(),
                    &contract_env,
                    &expected_env,
                    None,
                    true,
                )?;
                let (actual_head, mut actual_env) =
                    self.canonical_impl_head(trait_args, target_ast_ty, &impl_env, span)?;
                actual_env.slot_positions = constructor_slot_positions.clone();
                if impl_method.display_name_override.is_some() {
                    actual_env = self.canonical_contract_environment(
                        &impl_env,
                        &trait_info,
                        &actual_head,
                        &constructor_slot_positions,
                    )?;
                }
                let return_env = if impl_method.ret_ty.is_none() {
                    Some(self.canonical_contract_environment(
                        &impl_env,
                        &trait_info,
                        &actual_head,
                        &constructor_slot_positions,
                    )?)
                } else {
                    None
                };
                let mut actual = self.canonical_method_list(
                    &impl_return_type_arguments,
                    &impl_params,
                    &impl_ret,
                    &impl_method.return_type_arguments,
                    &impl_method.value_parameters,
                    impl_method.ret_ty.as_ref().unwrap_or(&trait_method.ret_ty),
                    impl_method.where_clause.as_ref(),
                    &impl_env,
                    &actual_env,
                    return_env.as_ref(),
                    true,
                )?;
                let instantiated_impl_constraints = self
                    .canonical_method_list(
                        &[],
                        &[],
                        &Ty::Unit,
                        &[],
                        &[],
                        &AstTy::Named(span.clone(), "Unit".into()),
                        typed_impl_clause.as_ref(),
                        &impl_env,
                        &actual_env,
                        None,
                        false,
                    )?
                    .where_constraints;
                instantiation_contracts.insert(
                    method_name.clone(),
                    ImplMethodInstantiationContract {
                        head: actual_head.clone(),
                        signature: actual.clone(),
                        impl_constraints: instantiated_impl_constraints,
                    },
                );
                resolved_signatures.insert(
                    method_name.clone(),
                    ResolvedImplMethodSignature {
                        params: impl_params,
                        result: impl_ret,
                        type_params: impl_type_params,
                        return_type_arguments: impl_return_type_arguments,
                        environment: impl_env,
                    },
                );
                let mapping = self
                    .validate_trait_method_contract(
                        &head_type_list,
                        &actual_head,
                        &expected,
                        &actual,
                        method_name,
                        &trait_method.span,
                        &impl_method.span,
                        trait_method.where_clause.as_ref(),
                        impl_method.where_clause.as_ref(),
                    )
                    .map_err(|mut error| {
                        let names = self.diagnostic_ty_names(
                            &std::iter::once(&target_ty)
                                .chain(trait_arg_tys.iter())
                                .collect::<Vec<_>>(),
                        );
                        let identity = diagnostics::TraitDiagnosticIdentity {
                            trait_id: trait_key.clone(),
                            trait_arguments: names[1..].to_vec(),
                            subject_type: names[0].clone(),
                        };
                        let diagnostic = error
                            .structured
                            .as_mut()
                            .expect("contract failures are structured");
                        match &mut diagnostic.data {
                            diagnostics::DiagnosticData::TraitMethodTypeList(data) => {
                                data.identity = Some(identity)
                            }
                            diagnostics::DiagnosticData::TraitMethodConstraint(data) => {
                                data.identity = Some(identity)
                            }
                            _ => unreachable!("contract validation returns a contract diagnostic"),
                        }
                        let declaration_identity = diagnostics::DeclarationIdentity {
                            owner: trait_key.clone(),
                            name: method_name.clone(),
                        };
                        let impl_declaration = match &mut diagnostic.data {
                            diagnostics::DiagnosticData::TraitMethodTypeList(data) => {
                                &mut data.impl_declaration
                            }
                            diagnostics::DiagnosticData::TraitMethodConstraint(data) => {
                                &mut data.impl_declaration
                            }
                            _ => unreachable!("contract diagnostic"),
                        };
                        impl_declaration
                            .as_mut()
                            .expect("contract impl origin")
                            .declaration_identity = Some(declaration_identity.clone());
                        diagnostic.primary.declaration_identity = Some(declaration_identity);
                        error
                    })?;
                // Store method variables in the same namespace as the impl
                // head; do not rebuild a receiver-only mapping at a later use.
                for entry in &mut actual.entries {
                    entry.ty = entry.ty.substitute(&mapping);
                }
                for constraint in &mut actual.where_constraints.constraints {
                    constraint.subject = constraint.subject.substitute(&mapping);
                    if let CanonicalMethodBound::TypeConstructor(slots) = &mut constraint.bound {
                        for slot in slots {
                            *slot = slot.substitute(&mapping);
                        }
                    }
                }
                method_signature_lists.insert(method_name.clone(), actual);
            }

            for (name, signature) in resolved_signatures {
                method_map
                    .get_mut(&name)
                    .expect("declared method")
                    .resolved_signature = Some(signature);
            }
            for (name, contract) in instantiation_contracts {
                method_map
                    .get_mut(&name)
                    .expect("declared method")
                    .instantiation_contract = Some(contract);
            }
            for (name, dispatch) in dispatch_overrides {
                method_map
                    .get_mut(&name)
                    .expect("declared method")
                    .dispatch_override = Some(dispatch);
            }
            let impl_info = Arc::make_mut(
                self.trait_impls
                    .get_mut(&impl_info.declaration_key.pattern)
                    .expect("registered impl head"),
            );
            impl_info.methods = method_map;
            impl_info.method_signature_lists = method_signature_lists;
        }

        if let Some(start) = phase_start {
            work.method_signatures = start.elapsed();
        }
        Ok(())
    }

    fn slot_vars_at_positions(
        &self,
        target: &AstTy,
        bindings: &HashMap<String, Ty>,
        positions: &[usize],
        span: &Span,
    ) -> Result<Vec<u32>, TypeError> {
        let mut vars = Vec::new();
        for position in positions {
            let argument = match target {
                AstTy::Generic(_, _, args) => args.get(*position),
                _ => None,
            };
            let Some(AstTy::Named(_, name)) = argument else {
                return Err(TypeError::new(
                    "Inherited constructor slot must remain a top-level type parameter of the impl target",
                    span.clone(),
                ));
            };
            let Some(Ty::Var(var)) = bindings.get(name) else {
                return Err(TypeError::new(
                    "Inherited constructor slot must remain a top-level type parameter of the impl target",
                    span.clone(),
                ));
            };
            if vars.contains(var) {
                return Err(TypeError::new(
                    format!(
                        "Impl target parameter {name} is mapped to more than one constructor slot"
                    ),
                    span.clone(),
                ));
            }
            vars.push(*var);
        }
        Ok(vars)
    }

    pub(super) fn resolve_impl_constructor_mappings(&mut self) -> Result<(), TypeError> {
        let mut keys = self
            .constructor_mapping_resolution
            .pending
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort_by_key(|key| self.trait_impls[key].declaration_key.declaration_id);
        let result = (|| {
            for key in keys {
                self.resolve_impl_constructor_mapping(&key)?;
            }
            if !self.constructor_mapping_resolution.pending.is_empty()
                || self
                    .constructor_mapping_resolution
                    .requests
                    .borrow()
                    .is_some()
            {
                return Err(TypeError::new(
                    "Internal error: unfinished impl mapping resolution",
                    Span { start: 0, end: 0 },
                ));
            }
            Ok(())
        })();
        // Declaration dependency state never escapes into body checking or a checkpoint.
        self.constructor_mapping_resolution = ConstructorMappingResolution::default();
        result
    }

    fn parent_coverage_with_resolved_mappings(
        &mut self,
        parent: &TraitImplInfo,
        child: &TraitImplInfo,
    ) -> Result<ParentImplCoverage, TypeError> {
        loop {
            assert!(self
                .constructor_mapping_resolution
                .requests
                .borrow()
                .is_none());
            *self.constructor_mapping_resolution.requests.borrow_mut() = Some(HashSet::new());
            let result = self.parent_impl_covers_child(parent, child);
            let requests = self
                .constructor_mapping_resolution
                .requests
                .borrow_mut()
                .take()
                .expect("declaration proof frame");
            if requests.is_empty() {
                return result;
            }
            // Even a successful trial may have used a pending projection during
            // canonicalization. Discard every trial result which requested metadata.
            let mut requests = requests.into_iter().collect::<Vec<_>>();
            requests.sort_by_key(|key| self.trait_impls[key].declaration_key.declaration_id);
            for key in requests {
                self.resolve_impl_constructor_mapping(&key)?;
            }
        }
    }

    fn resolve_impl_constructor_mapping(&mut self, key: &TraitImplKey) -> Result<(), TypeError> {
        if !self.constructor_mapping_resolution.pending.contains(key) {
            return Ok(());
        }
        let child = self.trait_impls[key].clone();
        if !self
            .constructor_mapping_resolution
            .resolving
            .insert(key.clone())
        {
            return Err(TypeError::new(
                format!(
                    "Constructor slot mapping dependency cycle for {}",
                    child.trait_id.name
                ),
                child.trait_id.span.clone(),
            ));
        }
        let trait_info = self.traits[&self.trait_key(&child.trait_id)].clone();
        let mut inherited: Option<Vec<usize>> = None;
        for parent in &trait_info.parents {
            let parent_key = self.trait_key(&parent.trait_id);
            let parent_info = self
                .traits
                .get(&parent_key)
                .ok_or_else(|| {
                    TypeError::new(
                        format!("Unknown parent trait: {}", parent.trait_id.name),
                        parent.trait_id.span.clone(),
                    )
                })?
                .clone();
            let mut candidates = self.trait_impl_candidate_keys(&parent_key);
            candidates.sort_by_key(|key| self.trait_impls[key].declaration_key.declaration_id);
            let mut covered = false;
            let mut head_covered = false;
            for candidate_key in candidates {
                let candidate = self.trait_impls[&candidate_key].clone();
                match self.parent_coverage_with_resolved_mappings(&candidate, &child)? {
                    ParentImplCoverage::HeadMismatch => continue,
                    ParentImplCoverage::ConstraintMismatch => {
                        head_covered = true;
                        continue;
                    }
                    ParentImplCoverage::Covers => {}
                }
                covered = true;
                self.resolve_impl_constructor_mapping(&candidate_key)?;
                if parent_info.constructor_slots.is_empty() {
                    continue;
                }
                let positions = self.trait_impls[&candidate_key]
                    .constructor_slot_positions
                    .clone();
                if let Some(previous) = &inherited {
                    if previous != &positions {
                        return Err(TypeError::new(
                            format!("Trait impl {} for {} has conflicting inherited constructor slot mappings from parent {}",
                                child.trait_id.name, child.target_name, parent.trait_id.name),
                            child.trait_id.span.clone(),
                        ));
                    }
                } else {
                    inherited = Some(positions);
                }
            }
            if !covered {
                let message = if head_covered {
                    format!(
                        "Trait impl {} for {} cannot prove parent constraint(s) of {}",
                        child.trait_id.name, child.target_name, parent.trait_id.name
                    )
                } else {
                    format!(
                        "Trait impl {} for {} requires parent impl {} for the same target",
                        child.trait_id.name, child.target_name, parent.trait_id.name
                    )
                };
                return Err(TypeError::new(message, child.trait_id.span.clone()));
            }
        }
        if let Some(positions) = inherited {
            let bindings = child
                .type_param_vars_by_name
                .iter()
                .map(|(name, var)| (name.clone(), Ty::Var(*var)))
                .collect();
            let vars = self.slot_vars_at_positions(
                &child.target_ast_ty,
                &bindings,
                &positions,
                &child.trait_id.span,
            )?;
            let (_, confirmations) = self.explicit_constructor_slot_vars_for_impl(
                &trait_info,
                &child.target_ast_ty,
                child.where_clause.as_ref(),
                &child.type_param_vars_by_name,
                &child.trait_id.span,
                false,
            )?;
            if confirmations
                .iter()
                .zip(&positions)
                .any(|(explicit, inherited)| {
                    explicit.is_some_and(|explicit| explicit != *inherited)
                })
            {
                return Err(TypeError::new(
                    format!("Trait impl {} for {} must use the same constructor slot mapping as its parent",
                        child.trait_id.name, child.target_name),
                    child.trait_id.span.clone(),
                ));
            }
            let info = Arc::make_mut(self.trait_impls.get_mut(key).expect("collected impl"));
            info.constructor_slot_vars = vars;
            info.constructor_slot_positions = positions;
        }
        self.constructor_mapping_resolution.resolving.remove(key);
        self.constructor_mapping_resolution.pending.remove(key);
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
    ) -> Result<ParentImplCoverage, TypeError> {
        let before_substitutions = self.substitutions.clone();
        let before_rigid = self.rigid_tyvars.clone();
        let before_bounds = self.tyvar_bounds.clone();
        let before_witnesses = self.constructor_witness_traits.clone();
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
        // A declaration constraint `Self: Parent` names the Parent capability
        // family, not a zero-argument Parent instance. Instantiate the full
        // parent head so its variables remain available to its where clause,
        // but coverage is determined by the target and declared obligations.
        for arg in &parent_impl.trait_arg_tys {
            self.instantiate_ty_with_fresh(arg, &mut fresh);
        }
        let obligations_hold = (|| {
            // Head coverage is structural. Bounds are proved separately below,
            // rather than making a missing bound look like a head mismatch.
            let Some(mapping) =
                self.parent_head_substitution(&parent_target, &child_impl.target_ty, &fresh)?
            else {
                return Ok(ParentImplCoverage::HeadMismatch);
            };
            self.substitutions.extend(mapping);
            if self.parent_where_is_entailed_by_child(parent_impl, child_impl, &fresh)? {
                Ok(ParentImplCoverage::Covers)
            } else {
                Ok(ParentImplCoverage::ConstraintMismatch)
            }
        })();

        self.substitutions = before_substitutions;
        self.rigid_tyvars = before_rigid;
        self.tyvar_bounds = before_bounds;
        self.constructor_witness_traits = before_witnesses;
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
    ) -> Result<bool, TypeError> {
        let Some(parent_where) = &parent_impl.where_clause else {
            return Ok(true);
        };

        for constraint in &parent_where.constraints {
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
                return Ok(false);
            };

            for bound in &constraint.bounds {
                let TypedWhereConstraintRhs::Trait { trait_id } = bound else {
                    continue;
                };
                let required = self.trait_key(trait_id);
                if !self.child_where_entails(child_impl, &parent_subject, &required)
                    && !matches!(
                        self.probe_trait_head(&required, &[], &parent_subject)?,
                        ApplicabilityProof::Satisfied(_)
                    )
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
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
}

#[cfg(test)]
mod incremental_predeclare_tests {
    use super::*;

    fn resolved(source: &str) -> Vec<Resolved> {
        let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
        crate::test_support::resolve_ast_with_builtin_prelude(ast).unwrap()
    }

    #[test]
    fn suffix_predeclare_work_does_not_visit_unrelated_retained_impls() {
        let prefix = "deftrait RetainedP2 {}\ndeftrait AddedP2 {}\nimpl RetainedP2 for Int {}";
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        let prefix_stmts = resolved(prefix);
        let prefix_len = prefix_stmts.len();
        checker.check_program(prefix_stmts).unwrap();
        let retained = checker.trait_impls.clone();
        let suffix = resolved(&format!("{prefix}\nimpl AddedP2 for Int {{}}"))
            .into_iter()
            .skip(prefix_len)
            .collect::<Vec<_>>();
        let mut work = PredeclareTraitWork::default();
        checker
            .predeclare_traits_with_work(&suffix, &mut work)
            .unwrap();
        assert_eq!(work.added_impls, 1);
        assert_eq!(work.mapping_declarations, 1);
        assert_eq!(
            work.coherence_entries, 0,
            "unrelated traits are not coherence candidates"
        );
        assert_eq!(
            work.method_lookup_entries, 1,
            "the added declaration has one direct lookup"
        );
        for (key, original) in retained {
            assert!(Arc::ptr_eq(&original, &checker.trait_impls[&key]));
        }
    }

    #[test]
    fn suffix_overlap_keeps_diagnostic_and_failed_session_isolation() {
        let prefix = "deftrait SessionP2 {}\ndefenum SessionBoxP2<$A> { Box($A), }\nimpl SessionP2 for SessionBoxP2<$A> {}";
        let mut session = crate::test_support::session_from_cached_std_prelude();
        let prefix_stmts = resolved(prefix);
        let prefix_len = prefix_stmts.len();
        session.typecheck(prefix_stmts).unwrap();
        let saved = session.checkpoint();
        let before = bincode::serialize(&saved).unwrap();
        let suffix = |declaration: &str| {
            resolved(&format!("{prefix}\n{declaration}"))
                .into_iter()
                .skip(prefix_len)
                .collect::<Vec<_>>()
        };
        let error = session
            .typecheck(suffix("impl SessionP2 for SessionBoxP2<Int> {}"))
            .expect_err("same trait and target must overlap across the suffix boundary");
        assert!(error.message.contains("Overlapping trait impls"), "{error}");
        assert_eq!(error.span.start, prefix.len() + 1);
        assert!(error
            .hint
            .as_deref()
            .unwrap()
            .contains("structurally disjoint"));
        assert_eq!(before, bincode::serialize(&session.checkpoint()).unwrap());
        session
            .typecheck(suffix("impl SessionP2 for String {}"))
            .unwrap();
        let mut sibling = ScarSession::new();
        sibling.rollback(saved);
        sibling
            .typecheck(suffix("impl SessionP2 for String {}"))
            .unwrap();
    }
}
