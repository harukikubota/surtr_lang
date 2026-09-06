use super::*;
use sindr::names::{builtin_type_name, builtin_type_usage_policy, TypeName};

#[derive(Clone, Copy)]
pub(super) enum SignatureTyMode<'a> {
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
}

impl Checker {
    pub(super) fn constructor_trait_key_for_ast_ty(&self, ast_ty: &AstTy) -> Option<String> {
        let name = match ast_ty {
            AstTy::Named(_, name) | AstTy::Generic(_, name, _) => Self::surface_name(name),
            _ => return None,
        };
        self.unique_constructor_trait_key(name)
    }

    pub(super) fn constructor_family_key(&self, trait_key: &str) -> String {
        self.traits
            .get(trait_key)
            .and_then(|info| info.constructor_root.clone())
            .unwrap_or_else(|| trait_key.to_string())
    }

    fn unique_constructor_trait_key(&self, name: &str) -> Option<String> {
        let mut candidates = self
            .traits
            .iter()
            .filter_map(|(key, info)| {
                (!info.constructor_slots.is_empty()
                    && (Self::surface_name(key) == name
                        || Self::surface_name(&info.id.name) == name))
                    .then_some(key.clone())
            })
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        (candidates.len() == 1).then(|| candidates.remove(0))
    }

    // A constructor-trait application is encoded as `SelfApp(Hole, witness,
    // slots...)`.  Plain `SelfApp(slots...)` remains the trait-metadata form.
    // Keeping both in the existing type variant avoids leaking a runtime type.
    pub(super) fn constructor_application_parts(items: &[Ty]) -> Option<(&Ty, &[Ty])> {
        match items {
            [Ty::Hole, witness, slots @ ..] => Some((witness, slots)),
            _ => None,
        }
    }

    pub(super) fn constructor_application_slots(ty: &Ty) -> Option<Vec<Ty>> {
        match ty {
            Ty::List(inner) => Some(vec![inner.as_ref().clone()]),
            Ty::Result(ok, _) => Some(vec![ok.as_ref().clone()]),
            Ty::Enum(_, args) => Some(args.clone()),
            Ty::Struct(_, fields) | Ty::Record(_, fields) => {
                Some(fields.iter().map(|(_, ty)| ty.clone()).collect())
            }
            _ => None,
        }
    }

    pub(super) fn constructor_application_slots_for_trait(
        &mut self,
        trait_key: &str,
        concrete_ty: &Ty,
    ) -> Option<Vec<Ty>> {
        let (info, mapping) = self.constructor_projection(trait_key, concrete_ty)?;
        info.constructor_slot_vars
            .iter()
            .map(|var| mapping.get(var).cloned())
            .collect()
    }

    pub(super) fn constructor_application_slots_for_witness(
        &mut self,
        witness: &Ty,
        _expected_slot_count: usize,
        concrete_ty: &Ty,
    ) -> Option<Vec<Ty>> {
        if let Ty::Var(var) = witness {
            if let Some(trait_key) = self.constructor_witness_traits.get(var).cloned() {
                return self.constructor_application_slots_for_trait(&trait_key, concrete_ty);
            }
        }
        None
    }

    pub(super) fn apply_constructor_application(
        &self,
        witness_source: &Ty,
        witness: &Ty,
        slots: &[Ty],
    ) -> Option<Ty> {
        let constructor_positions = match witness_source {
            Ty::Var(var) => self
                .constructor_witness_traits
                .get(var)
                .and_then(|trait_key| {
                    self.constructor_projection(trait_key, witness)
                        .map(|(info, _)| info.constructor_slot_positions)
                }),
            _ => None,
        };
        match witness {
            Ty::List(_) if slots.len() == 1 => Some(Ty::List(Box::new(slots[0].clone()))),
            Ty::Result(_, err) if slots.len() == 1 => {
                Some(Ty::Result(Box::new(slots[0].clone()), err.clone()))
            }
            Ty::Enum(name, args) => {
                let positions =
                    constructor_positions.filter(|positions| positions.len() == slots.len())?;
                let mut applied = args.clone();
                for (position, slot) in positions.into_iter().zip(slots) {
                    *applied.get_mut(position)? = slot.clone();
                }
                Some(Ty::Enum(name.clone(), applied))
            }
            Ty::Struct(name, _) | Ty::Record(name, _) => {
                let positions =
                    constructor_positions.filter(|positions| positions.len() == slots.len())?;
                let mut args = self.resolved_named_type_args(name, witness)?;
                for (position, slot) in positions.into_iter().zip(slots) {
                    *args.get_mut(position)? = slot.clone();
                }
                let def = self.env.lookup_type_def(name)?;
                let fields = self.instantiate_type_def_fields(def, &args);
                match witness {
                    Ty::Struct(_, _) => Some(Ty::Struct(name.clone(), fields)),
                    Ty::Record(_, _) => Some(Ty::Record(name.clone(), fields)),
                    _ => unreachable!("matched nominal record kind"),
                }
            }
            _ => None,
        }
    }
    fn resolve_signature_alias(
        &mut self,
        span: &Span,
        name: &str,
        args: &[AstTy],
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
        mode: SignatureTyMode<'_>,
    ) -> Result<Option<Ty>, TypeError> {
        let Some(alias) = self.signature_aliases.get(name).cloned() else {
            return Ok(None);
        };
        if alias.params.len() != args.len() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Type alias {} requires {} type argument(s), got {}",
                    Self::surface_name(name),
                    alias.params.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }
        if self.alias_expansion_stack.iter().any(|item| item == name) {
            let mut cycle = self.alias_expansion_stack.clone();
            cycle.push(name.to_string());
            return Err(TypeError {
                structured: None,
                message: format!("Cyclic type alias: {}", cycle.join(" -> ")),
                span: span.clone(),
                hint: None,
            });
        }
        let mut alias_tyvars = tyvars.clone();
        for (param, arg) in alias.params.iter().zip(args) {
            let actual = self.resolve_signature_like_ast_ty_in_context(
                arg,
                TypeSyntaxContext::General,
                tyvars,
                mode,
            )?;
            alias_tyvars.insert(param.name.clone(), actual);
        }
        self.alias_expansion_stack.push(name.to_string());
        let result = self.resolve_signature_like_ast_ty_in_context(
            &alias.rhs,
            context,
            &mut alias_tyvars,
            mode,
        );
        self.alias_expansion_stack.pop();
        result.map(Some)
    }

    fn validate_facet_kind_annotation(
        &self,
        ast: &AstTy,
        allow_alias: bool,
    ) -> Result<(), TypeError> {
        let AstTy::Named(span, name) = ast else {
            return Err(TypeError {
                structured: None,
                message: "Facet kind slot K must be a compiler-managed path kind name".into(),
                span: Self::ast_ty_span(ast).clone(),
                hint: None,
            });
        };
        let atomic = matches!(
            Self::surface_name(name),
            "InfallibleStructural" | "FallibleStructural" | "VariantPath"
        );
        let alias = matches!(
            Self::surface_name(name),
            "ReadablePath" | "WritablePath" | "PutPath" | "PreviewPath" | "CasePath"
        );
        if atomic || Self::surface_name(name).starts_with('$') || (allow_alias && alias) {
            Ok(())
        } else if alias {
            Err(TypeError {
                structured: None,
                message: "Facet kind-set aliases are only valid in compiler intrinsic constraints, not user Facet annotations".into(),
                span: span.clone(),
                hint: Some("Use the derived atomic kind InfallibleStructural, FallibleStructural, or VariantPath.".into()),
            })
        } else {
            Err(TypeError {
                structured: None,
                message: format!("Unknown Facet path kind `{}`", Self::surface_name(name)),
                span: span.clone(),
                hint: None,
            })
        }
    }

    fn facet_kind_annotation(
        &self,
        ast: &AstTy,
        allow_alias: bool,
    ) -> Result<crate::types::FacetKind, TypeError> {
        self.validate_facet_kind_annotation(ast, allow_alias)?;
        let AstTy::Named(_, name) = ast else {
            unreachable!("validated Facet kind is named")
        };
        if Self::surface_name(name).starts_with('$') {
            // Generic kind variables occur only in builtin chain signatures.
            // They are a constraint placeholder until a concrete path is built.
            return Ok(crate::types::FacetKind::ReadablePath);
        }
        Ok(
            crate::types::FacetKind::from_surface_name(Self::surface_name(name))
                .expect("validated Facet kind name"),
        )
    }

    fn canonical_user_type_name(name: &str) -> String {
        if name.contains("::") {
            name.to_string()
        } else {
            format!("Global::{name}")
        }
    }

    fn builtin_type_is_clause_block_surface_only(name: &str) -> bool {
        builtin_type_usage_policy(Self::surface_name(name))
            .is_some_and(|policy| policy.clause_block_surface_only)
    }

    fn builtin_type_is_lazy_signature_surface_only(name: &str) -> bool {
        builtin_type_usage_policy(Self::surface_name(name))
            .is_some_and(|policy| policy.lazy_signature_surface_only)
    }

    fn builtin_special_enum_ty(name: &str, args: &[Ty]) -> Option<Ty> {
        match Self::surface_name(name) {
            "Boolean" if args.is_empty() => Some(Ty::Bool),
            "Result" => args
                .first()
                .cloned()
                .map(|ok| Ty::Result(Box::new(ok), Box::new(Ty::Error))),
            _ => None,
        }
    }

    pub(super) fn is_duration_ty(ty: &Ty) -> bool {
        matches!(ty, Ty::Struct(name, _) if Self::surface_name(name) == "Duration")
    }

    fn seq_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: "Seq is not a surface type in this version of Surtr".into(),
            span: span.clone(),
            hint: Some(
                "Use tuple payloads for extractor success values, such as Option<(A, B)>.".into(),
            ),
        }
    }

    pub(super) fn error_function_param_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
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
            structured: None,
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
            Ty::List(inner) | Ty::Lazy(inner) => Self::ty_exposes_error_value(inner),
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                Self::ty_exposes_error_value(source)
                    || Self::ty_exposes_error_value(focus)
                    || Self::ty_exposes_error_value(update_source)
                    || Self::ty_exposes_error_value(update_focus)
            }
            Ty::Tuple(items) | Ty::SelfApp(items) | Ty::Enum(_, items) => {
                items.iter().any(Self::ty_exposes_error_value)
            }
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

    pub(super) fn ty_is_error_observer_callable(ty: &Ty) -> bool {
        match ty {
            Ty::Func(params, ret)
            | Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. } => {
                params.iter().any(Self::ty_exposes_error_value) || Self::ty_exposes_error_value(ret)
            }
            _ => false,
        }
    }

    pub(super) fn error_observer_escape_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: "Error observer closure cannot escape its Error-observation call".into(),
            span: span.clone(),
            hint: Some(
                "Pass the closure directly to Result::tap_err or another Error-observation API; do not store, return, or rebind it."
                    .into(),
            ),
        }
    }

    pub(super) fn error_observer_call_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: "Error observer closure can only be passed to Error-observation APIs".into(),
            span: span.clone(),
            hint: Some(
                "Use Result::tap_err(value, handler) instead of calling handler directly.".into(),
            ),
        }
    }

    pub(super) fn allows_std_error_function_param_exception(id: &ResolvedId) -> bool {
        matches!(
            Self::surface_qualified_name(id.qualified_name.as_deref()),
            Some("Result::tap_err") | Some("Result::_tap_err_value") | Some("Test::_finish_it_err")
        )
    }

    pub(super) fn local_type_syntax_context(&self) -> TypeSyntaxContext {
        if self.in_extractor_body {
            TypeSyntaxContext::ExtractorBody
        } else {
            TypeSyntaxContext::BindingAnnotation
        }
    }

    fn resolve_pid_surface_ty(&self, span: &Span, args: &[AstTy]) -> Result<Ty, TypeError> {
        if args.len() != 1 {
            return Err(TypeError {
                structured: None,
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
                structured: None,
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
                structured: None,
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
                structured: None,
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
            structured: None,
            message: format!("{rendered} is reserved for the {special_form} special form"),
            span: span.clone(),
            hint: Some(hint.into()),
        }
    }

    fn hole_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: "`_` is only allowed as an ignored-input marker inside callable types used by variable annotations or function return signatures.".into(),
            span: span.clone(),
            hint: Some(
                "Use `_` only in signatures such as `(_ -> Int)` or `(... -> (_ -> Int))`; it cannot appear as a plain value type, function parameter type, or container element."
                    .into(),
            ),
        }
    }

    fn reserved_hole_type_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: "`Hole` is compiler-reserved; write `_` only in a callable input type or Facet deferred slots.".into(),
            span: span.clone(),
            hint: Some("`Hole` has no user-facing type spelling.".into()),
        }
    }

    fn resolve_hole_surface_ty(
        &self,
        span: &Span,
        context: TypeSyntaxContext,
    ) -> Result<Ty, TypeError> {
        match context {
            TypeSyntaxContext::HoleClosureParam | TypeSyntaxContext::FacetDeferredSlot => {
                Ok(Ty::Hole)
            }
            _ => Err(self.hole_not_allowed_error(span)),
        }
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

    pub(super) fn tyvar_has_bound(&mut self, var: u32, trait_name: &str) -> bool {
        let requested_family = trait_name;
        let matching_bounds = self
            .tyvar_bounds
            .get(&var)
            .into_iter()
            .flatten()
            .filter(|bound| {
                let bound_family = bound.as_str();
                bound_family == requested_family
                    || self.trait_bound_entails(bound_family, requested_family, &mut HashSet::new())
            })
            .cloned()
            .collect::<Vec<_>>();
        if !matching_bounds.is_empty() {
            for capability in &mut self.active_capabilities {
                let capability_family = &capability.trait_id;
                if matches!(&capability.subject_ty, Ty::Var(subject_var) if *subject_var == var)
                    && matching_bounds.iter().any(|bound| {
                        let bound_family = bound.as_str();
                        capability_family == bound_family
                    })
                {
                    capability.consumed = true;
                }
            }
        }
        !matching_bounds.is_empty()
    }

    pub(super) fn consume_matching_capability(&mut self, receiver: &Ty, trait_name: &str) -> bool {
        let receiver = self.resolve_ty(receiver);
        let requested_family = trait_name;
        let matches = self
            .active_capabilities
            .iter()
            .enumerate()
            .filter_map(|(index, capability)| {
                let subject = self.resolve_ty(&capability.subject_ty);
                let capability_family = &capability.trait_id;
                let family_matches = capability_family == requested_family
                    || self.trait_bound_entails(
                        capability_family,
                        requested_family,
                        &mut HashSet::new(),
                    );
                let receiver_matches =
                    self.capability_receiver_matches(&capability.trait_id, &subject, &receiver);
                (receiver_matches && family_matches).then_some(index)
            })
            .collect::<Vec<_>>();
        let matched = !matches.is_empty();
        for index in matches {
            self.active_capabilities[index].consumed = true;
        }
        matched
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

    pub(super) fn collect_type_dependency_names(ast_ty: &AstTy, out: &mut Vec<String>) {
        match ast_ty {
            AstTy::Named(_, name) => {
                if !name.starts_with('$') {
                    out.push(Self::canonical_user_type_name(name));
                }
            }
            AstTy::Generic(_, _, args) => {
                for arg in args {
                    Self::collect_type_dependency_names(arg, out);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_type_dependency_names(item, out);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_type_dependency_names(param, out);
                }
                Self::collect_type_dependency_names(ret, out);
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
                structured: None,
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
                            structured: None,
                            message: format!("Unknown type: {}", name),
                            span: span.clone(),
                            hint: Some(
                                "Local type annotations may only reference type parameters declared on the surrounding function or method signature."
                                    .into(),
                            ),
                        }),
                    "_" => self.resolve_hole_surface_ty(span, context),
                    "Hole" => Err(self.reserved_hole_type_error(span)),
                    builtin_name if Self::builtin_type_is_clause_block_surface_only(builtin_name) => {
                        Err(self.clause_block_type_not_allowed_error(
                            span,
                            Self::surface_name(name),
                        ))
                    }
                    "Seq" => Err(self.seq_not_allowed_error(span)),
                    builtin_name => {
                        if let Some(def) = self.env.lookup_type_def(name) {
                            match &def.kind {
                                crate::env::TypeKind::Struct => {
                                    if !def.type_params.is_empty() {
                                        return Err(TypeError {
                                            structured: None,
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
                                    if let Some(ty) =
                                        Self::builtin_special_enum_ty(&def.name, &[])
                                    {
                                        return Ok(ty);
                                    }
                                    if def.type_params.is_empty() {
                                        return Ok(Ty::Enum(def.name.clone(), Vec::new()));
                                    }
                                    return Err(TypeError {
                                        structured: None,
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
                                | TypeName::StandbyInit
                                | TypeName::Lazy
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
                                structured: None,
                                message: format!("Unknown type: {}", name),
                                span: span.clone(),
                                hint: None,
                            }),
                        }
                    }
                }
            }
            AstTy::Generic(span, name, _)
                if Self::builtin_type_is_clause_block_surface_only(name) =>
            {
                Err(self.clause_block_type_not_allowed_error(span, Self::surface_name(name)))
            }
            AstTy::Generic(span, name, _)
                if Self::builtin_type_is_lazy_signature_surface_only(name) =>
            {
                Err(self.lazy_type_not_allowed_error(span))
            }
            AstTy::Generic(span, name, _) if Self::surface_name(name) == "Seq" => {
                Err(self.seq_not_allowed_error(span))
            }
            AstTy::Generic(span, name, args) => match Self::surface_name(name) {
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
                "StandbyInit" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "StandbyInit<T> requires exactly 1 type argument",
                    )?;
                    let inner_ty =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::Enum("StandbyInit".into(), vec![inner_ty]))
                }
                "Facet" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        5,
                        "Facet<K, S, A, T, B> requires exactly 5 type arguments",
                    )?;
                    let kind = self.facet_kind_annotation(&args[0], false)?;
                    // K is a compile-only declaration name.  The currently
                    // runtime-free capability representation stores S/A; T/B
                    // are validated here and instantiated by the intrinsic.
                    // Kind names are compiler declarations, not ordinary
                    // runtime types. Their validity is checked by Facet path
                    // specialization, so do not resolve them through the
                    // value-type namespace here.
                    let source =
                        self.resolve_ast_ty_in_context(&args[1], TypeSyntaxContext::General)?;
                    let focus =
                        self.resolve_ast_ty_in_context(&args[2], TypeSyntaxContext::General)?;
                    let update_source = self.resolve_ast_ty_in_context(&args[3], TypeSyntaxContext::FacetDeferredSlot)?;
                    let update_focus = self.resolve_ast_ty_in_context(&args[4], TypeSyntaxContext::FacetDeferredSlot)?;
                    if matches!((&update_source, &update_focus), (Ty::Hole, Ty::Hole))
                        || (!matches!(update_source, Ty::Hole) && !matches!(update_focus, Ty::Hole))
                    {
                        Ok(Ty::Facet(
                            kind,
                            Box::new(source),
                            Box::new(focus),
                            Box::new(update_source),
                            Box::new(update_focus),
                        ))
                    } else {
                        Err(TypeError {
                            structured: None,
                            message: "Facet update slots T and B must both be `_` or both be concrete types".into(),
                            span: span.clone(),
                            hint: None,
                        })
                    }
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
                            structured: None,
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
                                structured: None,
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
                        structured: None,
                        message: format!("Unknown generic type: {}", name),
                        span: span.clone(),
                        hint: None,
                    })?;
                    if def.type_params.len() != args.len() {
                        return Err(TypeError {
                            structured: None,
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
                        crate::env::TypeKind::Enum => {
                            if let Some(ty) =
                                Self::builtin_special_enum_ty(&def.name, &resolved_args)
                            {
                                Ok(ty)
                            } else {
                                Ok(Ty::Enum(def.name.clone(), resolved_args))
                            }
                        }
                        _ => Err(TypeError {
                            structured: None,
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
                        structured: None,
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
                structured: None,
                message: format!(
                    "`impl {}` is only supported in function and extractor parameters",
                    name
                ),
                span: span.clone(),
                hint: Some("Name the type parameter explicitly, e.g. `<$N: Trait>`.".into()),
            }),
        }
    }

    /// Resolve a call-site ReturnTypeArgument according to its declared slot.
    /// Ordinary type slots use the normal complete-type grammar. A direct
    /// TypeCtorTrait slot accepts a bare constructor head and gives its
    /// implementation-controlled slots fresh placeholders.
    pub(super) fn resolve_call_site_return_type_argument(
        &mut self,
        slot_ty: &Ty,
        ast_ty: &AstTy,
    ) -> Result<Ty, TypeError> {
        let is_constructor_slot = matches!(
            slot_ty,
            Ty::SelfApp(items) if Self::constructor_application_parts(items).is_some()
        );
        if !is_constructor_slot {
            return self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General);
        }

        let AstTy::Named(span, name) = ast_ty else {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Return type argument expects a bare type constructor head, got {}",
                    Self::surface_ast_ty(ast_ty)
                ),
                span: Self::ast_ty_span(ast_ty).clone(),
                hint: Some("Remove the constructor's payload arguments at this call site.".into()),
            });
        };
        if Self::surface_name(name) == "_" {
            return Ok(self.env.fresh_tyvar());
        }

        let trait_key = match slot_ty {
            Ty::SelfApp(items) => {
                Self::constructor_application_parts(items).and_then(|(witness, _)| match witness {
                    Ty::Var(var) => self.constructor_witness_traits.get(var).cloned(),
                    _ => None,
                })
            }
            _ => None,
        };
        if let Some(trait_key) = trait_key {
            let requested_head = match self.env.lookup_type_def(name) {
                Some(def) => self.canonical_nominal_head(&def.name)?,
                None => builtin_type_name(name)
                    .map(CanonicalTypeHead::Builtin)
                    .ok_or_else(|| {
                        TypeError::new(format!("Unknown type constructor: {}", name), span.clone())
                    })?,
            };
            let mut candidates = self
                .trait_impl_candidate_keys(&trait_key)
                .into_iter()
                .filter_map(|key| self.trait_impls.get(&key))
                .filter(|info| info.declaration_key.pattern.target.head == requested_head)
                .cloned()
                .collect::<Vec<_>>();
            if candidates.len() > 1 {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Type constructor {} has multiple {} implementations",
                        name,
                        self.trait_display_name(&trait_key)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
            if let Some(info) = candidates.pop() {
                let mapping = info
                    .type_param_vars
                    .iter()
                    .map(|var| {
                        let replacement = if info.constructor_slot_vars.contains(var) {
                            Ty::Hole
                        } else {
                            self.env.fresh_tyvar()
                        };
                        (*var, replacement)
                    })
                    .collect::<HashMap<_, _>>();
                return Ok(self.substitute_ty_with_mapping(&info.target_ty, &mapping));
            }
        }

        let Some(def) = self.env.lookup_type_def(name).cloned() else {
            let mut fresh = || self.env.fresh_tyvar();
            let builtin_head = match builtin_type_name(Self::surface_name(name)) {
                Some(TypeName::List) => Some(Ty::List(Box::new(fresh()))),
                Some(TypeName::HashMap) => Some(Ty::Enum("HashMap".into(), vec![fresh()])),
                Some(TypeName::Generator) => {
                    Some(Ty::Enum("Generator".into(), vec![fresh(), fresh()]))
                }
                Some(TypeName::Result) => Some(Ty::Result(Box::new(fresh()), Box::new(Ty::Error))),
                Some(TypeName::StandbyInit) => Some(Ty::Enum("StandbyInit".into(), vec![fresh()])),
                Some(TypeName::Lazy) => Some(Ty::Lazy(Box::new(fresh()))),
                Some(TypeName::TaskHandle) => Some(Ty::Enum("TaskHandle".into(), vec![fresh()])),
                _ => None,
            };
            return builtin_head.ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown type constructor: {}", name),
                span: span.clone(),
                hint: None,
            });
        };
        if def.type_params.is_empty() {
            return Err(TypeError {
                structured: None,
                message: format!("Type {} is not a type constructor", name),
                span: span.clone(),
                hint: None,
            });
        }

        let args = (0..def.type_params.len())
            .map(|_| self.env.fresh_tyvar())
            .collect::<Vec<_>>();
        let ty = match def.kind {
            crate::env::TypeKind::Struct => Ty::Struct(
                def.name.clone(),
                self.instantiate_type_def_fields(&def, &args),
            ),
            crate::env::TypeKind::Record => Ty::Record(
                def.name.clone(),
                self.instantiate_type_def_fields(&def, &args),
            ),
            crate::env::TypeKind::Enum => Self::builtin_special_enum_ty(&def.name, &args)
                .unwrap_or_else(|| Ty::Enum(def.name.clone(), args)),
            crate::env::TypeKind::ConcreteError => {
                return Err(TypeError {
                    structured: None,
                    message: format!("Error type {} is not a type constructor", name),
                    span: span.clone(),
                    hint: None,
                });
            }
        };
        Ok(ty)
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

    pub(super) fn resolve_signature_like_ast_ty_in_context(
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
        mode: SignatureTyMode<'_>,
    ) -> Result<Ty, TypeError> {
        match ast_ty {
            AstTy::Named(span, name) => {
                if let Some(alias) =
                    self.resolve_signature_alias(span, name, &[], context, tyvars, mode)?
                {
                    return Ok(alias);
                }
                if let Some(trait_key) = self.unique_constructor_trait_key(Self::surface_name(name))
                {
                    if !matches!(
                        context,
                        TypeSyntaxContext::General | TypeSyntaxContext::BindingAnnotation
                    ) {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Bare constructor trait {} is only allowed for a value parameter or local binding",
                                Self::surface_name(name)
                            ),
                            span: span.clone(),
                            hint: Some("Use an explicit application such as `Applicative<$A>` in callable signatures.".into()),
                        });
                    }
                    // A constructor witness belongs to this direct signature
                    // position.  Even applications of the same root in two
                    // parameters must remain independently instantiable.
                    let witness = self.env.fresh_tyvar();
                    let Ty::Var(witness_var) = witness else {
                        unreachable!("fresh constructor witness must be a type variable")
                    };
                    self.constructor_witness_traits
                        .insert(witness_var, trait_key);
                    return Ok(Ty::SelfApp(vec![Ty::Hole, Ty::Var(witness_var)]));
                }
            }
            AstTy::Generic(span, name, args) => {
                if let Some(alias) =
                    self.resolve_signature_alias(span, name, args, context, tyvars, mode)?
                {
                    return Ok(alias);
                }
                if let Some(trait_key) = self.unique_constructor_trait_key(Self::surface_name(name))
                {
                    let constructor_slot_count = self
                        .traits
                        .get(&trait_key)
                        .expect("selected constructor trait must remain registered")
                        .constructor_slots
                        .len();
                    if args.len() != constructor_slot_count {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Constructor trait {} requires {} slot argument(s), got {}",
                                Self::surface_name(name),
                                constructor_slot_count,
                                args.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    // Return positions also pass through this path, so they
                    // receive a fresh witness rather than inheriting an input
                    // constructor by root identity.
                    let witness = self.env.fresh_tyvar();
                    let Ty::Var(witness_var) = witness else {
                        unreachable!("fresh constructor witness must be a type variable")
                    };
                    self.constructor_witness_traits
                        .insert(witness_var, trait_key);
                    let mut application = vec![Ty::Hole, Ty::Var(witness_var)];
                    for arg in args {
                        application.push(self.resolve_signature_like_ast_ty_in_context(
                            arg,
                            TypeSyntaxContext::General,
                            tyvars,
                            mode,
                        )?);
                    }
                    return Ok(Ty::SelfApp(application));
                }
            }
            _ => {}
        }
        self.resolve_signature_like_ast_ty_in_context_non_alias(ast_ty, context, tyvars, mode)
    }

    fn resolve_signature_like_ast_ty_in_context_non_alias(
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
                        structured: None,
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
            AstTy::Named(span, name) if Self::surface_name(name) == "_" => {
                self.resolve_hole_surface_ty(span, context)
            }
            AstTy::Named(span, name) if Self::surface_name(name) == "Hole" => {
                Err(self.reserved_hole_type_error(span))
            }
            AstTy::Named(span, name) if Self::builtin_type_is_clause_block_surface_only(name) => {
                Err(self.clause_block_type_not_allowed_error(span, Self::surface_name(name)))
            }
            AstTy::Named(span, name) if Self::surface_name(name) == "Seq" => {
                Err(self.seq_not_allowed_error(span))
            }
            AstTy::ImplTrait(_, _) => Err(TypeError {
                structured: None,
                message: "Anonymous `impl Trait` types are not supported; introduce a named type slot and constrain it with `where`".into(),
                span: Self::ast_ty_span(ast_ty).clone(),
                hint: Some("Use a named `$T` type slot and add `$T: Trait` to the `where` clause.".into()),
            }),
            AstTy::Generic(_, name, args) if name.starts_with('$') => {
                let witness = if let Some(existing) = tyvars.get(name) {
                    existing.clone()
                } else {
                    let fresh = self.env.fresh_tyvar();
                    tyvars.insert(name.clone(), fresh.clone());
                    fresh
                };
                let mut application = vec![Ty::Hole, witness];
                application.extend(
                    args.iter()
                        .map(|argument| {
                            self.resolve_signature_like_ast_ty_in_context(
                                argument,
                                TypeSyntaxContext::General,
                                tyvars,
                                mode,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                );
                Ok(Ty::SelfApp(application))
            }
            AstTy::Generic(_, name, args) if name == "Self" && mode.self_ty().is_some() => {
                let args = args
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
                Ok(Ty::SelfApp(args))
            }
            AstTy::Generic(span, name, _)
                if Self::builtin_type_is_clause_block_surface_only(name) =>
            {
                Err(self.clause_block_type_not_allowed_error(span, Self::surface_name(name)))
            }
            AstTy::Generic(span, name, args)
                if Self::builtin_type_is_lazy_signature_surface_only(name)
                    && mode.allows_lazy() =>
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
                "Option" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "Option<T> requires exactly 1 type argument",
                    )?;
                    let inner_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    let enum_name = self
                        .env
                        .lookup_type_def(name)
                        .map(|def| def.name.clone())
                        .unwrap_or_else(|| name.clone());
                    Ok(Ty::Enum(enum_name, vec![inner_ty]))
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
                "StandbyInit" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        1,
                        "StandbyInit<T> requires exactly 1 type argument",
                    )?;
                    let inner_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    Ok(Ty::Enum("StandbyInit".into(), vec![inner_ty]))
                }
                "Facet" => {
                    let args = self.require_type_arg_count(
                        span,
                        args,
                        5,
                        "Facet<K, S, A, T, B> requires exactly 5 type arguments",
                    )?;
                    let kind = self.facet_kind_annotation(
                        &args[0],
                        matches!(mode, SignatureTyMode::Builtin),
                    )?;
                    // See the surface-type resolver above: K is compile-only.
                    let source = self.resolve_signature_like_ast_ty_in_context(
                        &args[1],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    let focus = self.resolve_signature_like_ast_ty_in_context(
                        &args[2],
                        TypeSyntaxContext::General,
                        tyvars,
                        mode,
                    )?;
                    let update_source = self.resolve_signature_like_ast_ty_in_context(
                        &args[3],
                        TypeSyntaxContext::FacetDeferredSlot,
                        tyvars,
                        mode,
                    )?;
                    let update_focus = self.resolve_signature_like_ast_ty_in_context(
                        &args[4],
                        TypeSyntaxContext::FacetDeferredSlot,
                        tyvars,
                        mode,
                    )?;
                    if !matches!((&update_source, &update_focus), (Ty::Hole, Ty::Hole))
                        && (matches!(update_source, Ty::Hole) || matches!(update_focus, Ty::Hole))
                    {
                        return Err(TypeError {
                            structured: None,
                            message: "Facet update slots T and B must both be `_` or both be concrete types".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    Ok(Ty::Facet(
                        kind,
                        Box::new(source),
                        Box::new(focus),
                        Box::new(update_source),
                        Box::new(update_focus),
                    ))
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
                            structured: None,
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
                                structured: None,
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
                            structured: None,
                            message: format!("Unknown generic type: {}", name),
                            span: span.clone(),
                            hint: None,
                        })?;
                    if def.type_params.len() != args.len() {
                        return Err(TypeError {
                            structured: None,
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
                        crate::env::TypeKind::Enum => {
                            if let Some(ty) =
                                Self::builtin_special_enum_ty(&def.name, &resolved_args)
                            {
                                Ok(ty)
                            } else {
                                Ok(Ty::Enum(def.name.clone(), resolved_args))
                            }
                        }
                        _ => Err(TypeError {
                            structured: None,
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
                        structured: None,
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
                structured: None,
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
            structured: None,
            message: "The error marker E in Result<T, E> must be a deferror-defined type.".into(),
            span: span.clone(),
            hint: None,
        });

        if let Ok(def) = def {
            if def.kind != crate::env::TypeKind::ConcreteError {
                return Err(TypeError {
                    structured: None,
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
                structured: None,
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
        let result =
            match (&expected, &got) {
                (Ty::Hole, Ty::Hole) => true,
                (Ty::Var(left), Ty::Var(right)) => match (
                    self.rigid_tyvars.contains(left),
                    self.rigid_tyvars.contains(right),
                ) {
                    (true, true) => left == right,
                    (true, false) => self.bind_tyvar(*right, &Ty::Var(*left)),
                    (false, true) => self.bind_tyvar(*left, &Ty::Var(*right)),
                    (false, false) => self.bind_tyvar(*left, &Ty::Var(*right)),
                },
                (Ty::Var(var), _) if self.rigid_tyvars.contains(var) => false,
                (_, Ty::Var(var)) if self.rigid_tyvars.contains(var) => false,
                (Ty::Var(var), ty) | (ty, Ty::Var(var)) => self.bind_tyvar(*var, ty),
                (Ty::Int, Ty::Int)
                | (Ty::Float, Ty::Float)
                | (Ty::Str, Ty::Str)
                | (Ty::Bool, Ty::Bool)
                | (Ty::Unit, Ty::Unit)
                | (Ty::Error, Ty::Error) => true,
                (Ty::List(a), Ty::List(b)) => self.types_compatible(a, b),
                (Ty::Lazy(a), Ty::Lazy(b)) => self.types_compatible(a, b),
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
                (
                    Ty::Facet(kind_a, src_a, focus_a, update_src_a, update_focus_a),
                    Ty::Facet(kind_b, src_b, focus_b, update_src_b, update_focus_b),
                ) => {
                    kind_a.accepts(*kind_b)
                        && self.types_compatible(src_a, src_b)
                        && self.types_compatible(focus_a, focus_b)
                        && self.types_compatible(update_src_a, update_src_b)
                        && self.types_compatible(update_focus_a, update_focus_b)
                }
                (Ty::Tuple(a), Ty::Tuple(b)) => {
                    a.len() == b.len()
                        && a.iter()
                            .zip(b.iter())
                            .all(|(left, right)| self.types_compatible(left, right))
                }
                (Ty::SelfApp(a), Ty::SelfApp(b))
                    if Self::constructor_application_parts(a).is_some()
                        && Self::constructor_application_parts(b).is_some() =>
                {
                    let (left_witness, left_slots) =
                        Self::constructor_application_parts(a).expect("checked above");
                    let (right_witness, right_slots) =
                        Self::constructor_application_parts(b).expect("checked above");
                    left_slots.len() == right_slots.len()
                        && self.types_compatible(left_witness, right_witness)
                        && left_slots
                            .iter()
                            .zip(right_slots.iter())
                            .all(|(left, right)| self.types_compatible(left, right))
                }
                (Ty::SelfApp(a), other) if Self::constructor_application_parts(a).is_some() => {
                    let (witness, expected_slots) =
                        Self::constructor_application_parts(a).expect("checked above");
                    if !self.types_compatible(witness, other) {
                        false
                    } else {
                        expected_slots.is_empty()
                            || self
                                .constructor_application_slots_for_witness(
                                    witness,
                                    expected_slots.len(),
                                    other,
                                )
                                .is_some_and(|actual_slots| {
                                    actual_slots.len() == expected_slots.len()
                                        && expected_slots.iter().zip(actual_slots.iter()).all(
                                            |(expected, actual)| {
                                                self.types_compatible(expected, actual)
                                            },
                                        )
                                })
                    }
                }
                (other, Ty::SelfApp(b)) if Self::constructor_application_parts(b).is_some() => {
                    self.types_compatible(&Ty::SelfApp(b.clone()), other)
                }
                (Ty::SelfApp(a), Ty::SelfApp(b)) => {
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
                (Ty::Struct(n1, fields1), Ty::Struct(n2, fields2)) => {
                    Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
                        && (fields1.is_empty()
                            || fields2.is_empty()
                            || (fields1.len() == fields2.len()
                                && fields1.iter().zip(fields2).all(
                                    |((name1, ty1), (name2, ty2))| {
                                        name1 == name2 && self.types_compatible(ty1, ty2)
                                    },
                                )))
                }
                (Ty::Record(n1, fields1), Ty::Record(n2, fields2)) => {
                    Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
                        && (fields1.is_empty()
                            || fields2.is_empty()
                            || (fields1.len() == fields2.len()
                                && fields1.iter().zip(fields2).all(
                                    |((name1, ty1), (name2, ty2))| {
                                        name1 == name2 && self.types_compatible(ty1, ty2)
                                    },
                                )))
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

    /// A contextual trait capability applies to every value in its declared
    /// constructor slot(s), while captured nominal arguments must still agree.
    /// This is stricter than nominal-head comparison but permits a Factory
    /// capability on Boxed<$A> to prove a receiverless Factory::make() result
    /// of Boxed<Int>.
    pub(super) fn capability_receiver_matches(
        &self,
        capability_trait: &str,
        subject: &Ty,
        receiver: &Ty,
    ) -> bool {
        if self.resolve_ty(subject) == self.resolve_ty(receiver) {
            return true;
        }
        let Ok(canonical_subject) = self.canonical_request(subject) else {
            return false;
        };
        let Ok(canonical_receiver) = self.canonical_request(receiver) else {
            return false;
        };
        if canonical_subject.head != canonical_receiver.head
            || canonical_subject.arguments.len() != canonical_receiver.arguments.len()
        {
            return false;
        }
        let Some((info, _)) = self.constructor_projection(capability_trait, subject) else {
            return false;
        };
        canonical_subject
            .arguments
            .iter()
            .zip(&canonical_receiver.arguments)
            .enumerate()
            .filter(|(index, _)| !info.constructor_slot_positions.contains(index))
            .all(|(_, (subject, receiver))| subject == receiver)
    }

    pub(super) fn signature_tyvar_ids(tyvars: &HashMap<String, Ty>) -> HashSet<u32> {
        tyvars
            .values()
            .filter_map(|ty| match ty {
                Ty::Var(var) => Some(*var),
                _ => None,
            })
            .collect()
    }

    pub(super) fn types_compatible_with_rigid(
        &mut self,
        expected: &Ty,
        got: &Ty,
        rigid_tyvars: &HashSet<u32>,
    ) -> bool {
        let saved = std::mem::replace(&mut self.rigid_tyvars, rigid_tyvars.clone());
        let compatible = self.types_compatible(expected, got);
        self.rigid_tyvars = saved;
        compatible
    }

    /// Checks whether a callable parameter can consume an argument. `Hole` is
    /// an ignored-input marker, not a wildcard type: it is accepted only at
    /// this callable-application boundary and never participates in general
    /// type compatibility or unification.
    pub(super) fn callable_accepts_input(&mut self, param: &Ty, argument: &Ty) -> bool {
        matches!(self.resolve_ty(param), Ty::Hole) || self.types_compatible(param, argument)
    }

    /// Flow operators encode their callable input in their trait arguments.
    /// Preserve the ignored-input meaning there without treating `Hole` as a
    /// generally compatible type or binding it to the implementation generic.
    pub(super) fn operator_trait_arg_compatible(&mut self, expected: &Ty, actual: &Ty) -> bool {
        matches!(self.resolve_ty(actual), Ty::Hole) || self.types_compatible(expected, actual)
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
            let pending_obligations = self
                .pending_trait_obligations
                .get(&var)
                .cloned()
                .unwrap_or_default();
            match &ty {
                Ty::Var(other) => {
                    if self.rigid_tyvars.contains(other)
                        && !var_bounds
                            .iter()
                            .all(|bound| self.tyvar_has_bound(*other, bound))
                    {
                        self.profiler.finish(ProfileEvent::BindTyVar, profile);
                        return false;
                    }
                    let mut combined = var_bounds;
                    for bound in self.tyvar_bound_names(*other) {
                        if !combined.iter().any(|existing| existing == &bound) {
                            combined.push(bound);
                        }
                    }
                    combined.sort();
                    self.tyvar_bounds.insert(var, combined.clone());
                    self.tyvar_bounds.insert(*other, combined);
                    let binding = HashMap::from([(var, Ty::Var(*other))]);
                    for mut obligation in pending_obligations {
                        obligation.receiver = self.substitute_ty_with_mapping(
                            &self.resolve_ty(&obligation.receiver),
                            &binding,
                        );
                        obligation.args = obligation
                            .args
                            .iter()
                            .map(|arg| {
                                self.substitute_ty_with_mapping(&self.resolve_ty(arg), &binding)
                            })
                            .collect();
                        let pending = self.pending_trait_obligations.entry(*other).or_default();
                        if !pending.contains(&obligation) {
                            pending.push(obligation);
                        }
                    }
                }
                _ => {
                    if !self.ty_satisfies_bounds(&ty, &var_bounds) {
                        self.profiler.finish(ProfileEvent::BindTyVar, profile);
                        return false;
                    }
                    if !pending_obligations.iter().all(|obligation| {
                        let binding = HashMap::from([(var, ty.clone())]);
                        let receiver = self.substitute_ty_with_mapping(
                            &self.resolve_ty(&obligation.receiver),
                            &binding,
                        );
                        let arguments = obligation
                            .args
                            .iter()
                            .map(|arg| {
                                self.substitute_ty_with_mapping(&self.resolve_ty(arg), &binding)
                            })
                            .collect::<Vec<_>>();
                        self.trait_impl_exists_for_args(&obligation.trait_id, &arguments, &receiver)
                    }) {
                        self.profiler.finish(ProfileEvent::BindTyVar, profile);
                        return false;
                    }
                }
            }
            self.substitutions.insert(var, ty);
            self.pending_trait_obligations.remove(&var);
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
            concrete => bounds.iter().all(|bound| {
                matches!(
                    self.prove_trait_capability(bound, &concrete),
                    Ok(ApplicabilityProof::Satisfied(_))
                )
            }),
        }
    }

    pub(super) fn ty_contains_var(&self, ty: &Ty, needle: u32) -> bool {
        match self.resolve_ty(ty) {
            Ty::Var(var) => var == needle,
            Ty::Hole => false,
            Ty::List(inner) => self.ty_contains_var(&inner, needle),
            Ty::Lazy(inner) => self.ty_contains_var(&inner, needle),
            Ty::Pid(_) => false,
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.ty_contains_var(&source, needle)
                    || self.ty_contains_var(&focus, needle)
                    || self.ty_contains_var(&update_source, needle)
                    || self.ty_contains_var(&update_focus, needle)
            }
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                items.iter().any(|item| self.ty_contains_var(item, needle))
            }
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
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.resolve_ty(inner))),
            Ty::Pid(name) => Ty::Pid(name.clone()),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                *kind,
                Box::new(self.resolve_ty(source)),
                Box::new(self.resolve_ty(focus)),
                Box::new(self.resolve_ty(update_source)),
                Box::new(self.resolve_ty(update_focus)),
            ),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|item| self.resolve_ty(item)).collect()),
            Ty::SelfApp(items) => {
                let source_witness =
                    Self::constructor_application_parts(items).map(|(witness, _)| witness.clone());
                let resolved = items
                    .iter()
                    .map(|item| self.resolve_ty(item))
                    .collect::<Vec<_>>();
                if let Some((witness, slots)) = Self::constructor_application_parts(&resolved) {
                    if let Some(applied) = source_witness.as_ref().and_then(|source| {
                        self.apply_constructor_application(source, witness, slots)
                    }) {
                        return self.resolve_ty(&applied);
                    }
                }
                Ty::SelfApp(resolved)
            }
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
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.resolve_ty(ty)))
                    .collect(),
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
            Ty::Var(var) => {
                if let Some(instantiated) = fresh.get(var) {
                    instantiated.clone()
                } else {
                    let instantiated = self.env.fresh_tyvar();
                    if let Ty::Var(new_var) = instantiated {
                        let bounds = self.tyvar_bound_names(*var);
                        self.register_tyvar_bounds(new_var, &bounds);
                        if let Some(trait_key) = self.constructor_witness_traits.get(var).cloned() {
                            self.constructor_witness_traits.insert(new_var, trait_key);
                        }
                    }
                    fresh.insert(*var, instantiated.clone());
                    instantiated
                }
            }
            Ty::List(inner) => Ty::List(Box::new(self.instantiate_ty_with_fresh(inner, fresh))),
            Ty::Hole => Ty::Hole,
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.instantiate_ty_with_fresh(inner, fresh))),
            Ty::Pid(name) => Ty::Pid(name.clone()),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                *kind,
                Box::new(self.instantiate_ty_with_fresh(source, fresh)),
                Box::new(self.instantiate_ty_with_fresh(focus, fresh)),
                Box::new(self.instantiate_ty_with_fresh(update_source, fresh)),
                Box::new(self.instantiate_ty_with_fresh(update_focus, fresh)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.instantiate_ty_with_fresh(item, fresh))
                    .collect(),
            ),
            Ty::SelfApp(items) => Ty::SelfApp(
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
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.instantiate_ty_with_fresh(ty, fresh)))
                    .collect(),
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

    /// Instantiates declaration-owned callable generics for one use site.
    /// Every lookup gets an independent fresh map so substitutions cannot
    /// leak between calls to the same function.
    pub(super) fn instantiate_callable_ty(&mut self, ty: &Ty) -> Ty {
        let mut fresh = HashMap::new();
        self.instantiate_ty_with_fresh(ty, &mut fresh)
    }

    fn substitute_type_def_ty(&self, ty: &Ty, bindings: &HashMap<u32, Ty>) -> Ty {
        match ty {
            Ty::Var(var) => bindings.get(var).cloned().unwrap_or(Ty::Var(*var)),
            Ty::List(inner) => Ty::List(Box::new(self.substitute_type_def_ty(inner, bindings))),
            Ty::Hole => Ty::Hole,
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.substitute_type_def_ty(inner, bindings))),
            Ty::Pid(name) => Ty::Pid(name.clone()),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                *kind,
                Box::new(self.substitute_type_def_ty(source, bindings)),
                Box::new(self.substitute_type_def_ty(focus, bindings)),
                Box::new(self.substitute_type_def_ty(update_source, bindings)),
                Box::new(self.substitute_type_def_ty(update_focus, bindings)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.substitute_type_def_ty(item, bindings))
                    .collect(),
            ),
            Ty::SelfApp(items) => Ty::SelfApp(
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
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.substitute_type_def_ty(ty, bindings)))
                    .collect(),
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

    pub(super) fn diagnostic_tyvar_name(index: usize) -> String {
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
            Ty::SelfApp(args) => format!(
                "Self<{}>",
                args.iter()
                    .map(|arg| self.diagnostic_ty_name_with_state(arg, tyvars, next_tyvar_index))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::List(inner) => format!(
                "List<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::Lazy(inner) => format!(
                "Lazy<{}>",
                self.diagnostic_ty_name_with_state(inner, tyvars, next_tyvar_index)
            ),
            Ty::Pid(name) => format!("PID<{}>", Self::surface_name(name)),
            Ty::Facet(kind, source, focus, update_source, update_focus) => format!(
                "Facet<{}, {}, {}, {}, {}>",
                kind.as_str(),
                self.diagnostic_ty_name_with_state(source, tyvars, next_tyvar_index),
                self.diagnostic_ty_name_with_state(focus, tyvars, next_tyvar_index),
                self.diagnostic_ty_name_with_state(update_source, tyvars, next_tyvar_index),
                self.diagnostic_ty_name_with_state(update_focus, tyvars, next_tyvar_index)
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
            Ty::SelfApp(args) => format!(
                "Self<{}>",
                args.iter()
                    .map(|arg| self.ty_name(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::List(inner) => format!("List<{}>", self.ty_name(inner)),
            Ty::Lazy(inner) => format!("Lazy<{}>", self.ty_name(inner)),
            Ty::Pid(name) => format!("PID<{}>", Self::surface_name(name)),
            Ty::Facet(kind, source, focus, update_source, update_focus) => {
                format!(
                    "Facet<{}, {}, {}, {}, {}>",
                    kind.as_str(),
                    self.ty_name(source),
                    self.ty_name(focus),
                    self.ty_name(update_source),
                    self.ty_name(update_focus)
                )
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
            Ty::Var(n) => self
                .local_annotation_tyvars
                .iter()
                .find_map(|(name, ty)| matches!(ty, Ty::Var(var) if var == n).then(|| name.clone()))
                .unwrap_or_else(|| "the inferred argument type".into()),
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
            Ty::Facet(..) => true,
            Ty::List(inner) | Ty::Lazy(inner) => self.ty_contains_facet(inner.as_ref()),
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                items.iter().any(|item| self.ty_contains_facet(item))
            }
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
            update_source_ty: self.resolve_ty(&path.update_source_ty),
            update_focus_ty: self.resolve_ty(&path.update_focus_ty),
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
                obligation,
                dispatch,
                origin,
                args,
            } => TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty: self.resolve_ty(&receiver_ty),
                obligation: TraitObligation {
                    trait_id: obligation.trait_id,
                    trait_args: obligation
                        .trait_args
                        .iter()
                        .map(|arg| self.resolve_ty(arg))
                        .collect(),
                    receiver: self.resolve_ty(&obligation.receiver),
                },
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
            TypedInner::EagerBoundary(inner) => {
                TypedInner::EagerBoundary(Box::new(self.resolve_typed_node(*inner)))
            }
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
                    .map(|param| TypedValueParameter {
                        id: param.id,
                        mode: param.mode,
                        ty: self.resolve_ty(&param.ty),
                        span: param.span,
                    })
                    .collect(),
                Box::new(self.resolve_typed_node(*show)),
            ),
            TypedInner::Def(
                fun_idx,
                id,
                type_params,
                params,
                ret_ty,
                where_clause,
                body,
                visibility,
            ) => TypedInner::Def(
                fun_idx,
                id,
                type_params
                    .into_iter()
                    .map(|argument| TypedReturnTypeArgument {
                        ordinal: argument.ordinal,
                        ty: self.resolve_ty(&argument.ty),
                        span: argument.span,
                    })
                    .collect(),
                params
                    .into_iter()
                    .map(|param| TypedValueParameter {
                        id: param.id,
                        mode: param.mode,
                        ty: self.resolve_ty(&param.ty),
                        span: param.span,
                    })
                    .collect(),
                self.resolve_ty(&ret_ty),
                where_clause,
                Box::new(self.resolve_typed_node(*body)),
                visibility,
            ),
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
                    TypedValueParameter {
                        id: param.id,
                        mode: param.mode,
                        ty: self.resolve_ty(&param.ty),
                        span: param.span,
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
            TypedInner::TraitDef(name, where_clause, methods) => {
                TypedInner::TraitDef(name, where_clause, methods)
            }
            TypedInner::TraitImplDef(trait_name, target_name, where_clause) => {
                TypedInner::TraitImplDef(trait_name, target_name, where_clause)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn constructor_trait(name: &str, qualified_name: &str, unique_id: u32) -> TraitInfo {
        TraitInfo {
            id: ResolvedId {
                name: name.into(),
                qualified_name: Some(qualified_name.into()),
                unique_id,
                compiler_generated: false,
                symbol_info: None,
                span: Span { start: 0, end: 1 },
            },
            type_params: Vec::new(),
            where_clause: None,
            constructor_slots: vec!["$A".into()],
            constructor_root: Some(qualified_name.into()),
            parents: Vec::new(),
            methods: HashMap::new(),
        }
    }

    #[test]
    fn duplicate_constructor_trait_short_names_are_not_selected_by_registration_order() {
        for reversed in [false, true] {
            let mut checker = Checker::new(TypecheckContext::default());
            let entries = [
                (
                    "Left::Functor",
                    constructor_trait("Functor", "Left::Functor", 71_001),
                ),
                (
                    "Right::Functor",
                    constructor_trait("Functor", "Right::Functor", 71_002),
                ),
            ];
            let order: &[usize] = if reversed { &[1, 0] } else { &[0, 1] };
            for index in order {
                let (key, info) = &entries[*index];
                checker.traits.insert((*key).into(), info.clone());
            }

            assert!(checker.unique_constructor_trait_key("Functor").is_none());
            assert_eq!(
                checker.unique_constructor_trait_key("Left::Functor"),
                Some("Left::Functor".into())
            );
            assert!(checker.trait_key_by_short_name("Functor").is_none());
        }
    }

    #[test]
    fn clause_block_surface_type_query_uses_builtin_type_usage_policy() {
        assert!(Checker::builtin_type_is_clause_block_surface_only(
            "MatchArms"
        ));
        assert!(Checker::builtin_type_is_clause_block_surface_only(
            "Global::CondClauses"
        ));
        assert!(Checker::builtin_type_is_clause_block_surface_only(
            "BulkUpdateEntries"
        ));
        assert!(!Checker::builtin_type_is_clause_block_surface_only(
            "String"
        ));
        assert!(!Checker::builtin_type_is_clause_block_surface_only(
            "StandbyInit"
        ));
        assert!(!Checker::builtin_type_is_clause_block_surface_only("Lazy"));
    }

    #[test]
    fn lazy_signature_surface_type_query_uses_builtin_type_usage_policy() {
        assert!(Checker::builtin_type_is_lazy_signature_surface_only("Lazy"));
        assert!(Checker::builtin_type_is_lazy_signature_surface_only(
            "Global::Lazy"
        ));
        assert!(!Checker::builtin_type_is_lazy_signature_surface_only(
            "MatchArms"
        ));
        assert!(!Checker::builtin_type_is_lazy_signature_surface_only(
            "String"
        ));
    }
}
