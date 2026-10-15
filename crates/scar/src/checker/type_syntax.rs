use super::*;
use sindr::intrinsic::IntrinsicId;
use sindr::names::{builtin_type_name, builtin_type_usage_policy, BuiltinTypeUsage, TypeName};

#[derive(Clone, Copy)]
pub(super) enum SignatureTyMode<'a> {
    Normal,
    ImplHead { self_ty: Option<&'a Ty> },
    Trait { self_ty: &'a Ty },
    Builtin,
    ResolvedNormal,
    ResolvedTrait { self_ty: &'a Ty },
    ResolvedBuiltin,
}

impl<'a> SignatureTyMode<'a> {
    fn self_ty(self) -> Option<&'a Ty> {
        match self {
            SignatureTyMode::Trait { self_ty } | SignatureTyMode::ResolvedTrait { self_ty } => {
                Some(self_ty)
            }
            SignatureTyMode::ImplHead { self_ty } => self_ty,
            SignatureTyMode::Normal
            | SignatureTyMode::Builtin
            | SignatureTyMode::ResolvedNormal
            | SignatureTyMode::ResolvedBuiltin => None,
        }
    }

    fn allows_lazy(self) -> bool {
        matches!(
            self,
            SignatureTyMode::Builtin | SignatureTyMode::ResolvedBuiltin
        )
    }

    fn allows_user_generic_fallback(self) -> bool {
        !matches!(
            self,
            SignatureTyMode::Builtin | SignatureTyMode::ResolvedBuiltin
        )
    }

    fn without_direct_constructor_name_fallback(self) -> Self {
        match self {
            SignatureTyMode::ImplHead { self_ty } => SignatureTyMode::ImplHead { self_ty },
            SignatureTyMode::Normal | SignatureTyMode::ResolvedNormal => {
                SignatureTyMode::ResolvedNormal
            }
            SignatureTyMode::Trait { self_ty } | SignatureTyMode::ResolvedTrait { self_ty } => {
                SignatureTyMode::ResolvedTrait { self_ty }
            }
            SignatureTyMode::Builtin | SignatureTyMode::ResolvedBuiltin => {
                SignatureTyMode::ResolvedBuiltin
            }
        }
    }

    fn allows_direct_constructor_name_fallback(self) -> bool {
        matches!(
            self,
            SignatureTyMode::ImplHead { .. }
                | SignatureTyMode::Normal
                | SignatureTyMode::Trait { .. }
                | SignatureTyMode::Builtin
        )
    }
}

impl Checker {
    pub(super) fn constructor_trait_key_for_signature_ty(
        &self,
        signature_ty: &sigil::resolved::ResolvedSignatureTy,
    ) -> Option<String> {
        let trait_id = signature_ty.direct_constructor_trait.as_ref()?;
        let key = self.trait_key(trait_id);
        self.traits
            .get(&key)
            .filter(|info| {
                info.id.unique_id == trait_id.unique_id && !info.constructor_slots.is_empty()
            })
            .map(|_| key)
    }

    pub(super) fn constructor_trait_key_for_ast_ty(&self, ast_ty: &AstTy) -> Option<String> {
        let name = match ast_ty {
            AstTy::Named(_, name) | AstTy::Generic(_, name, _) => Self::surface_name(name),
            _ => return None,
        };
        self.unique_constructor_trait_key(name)
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

    pub(super) fn declaration_constructor_trait_key(&self, bound: &str) -> Option<String> {
        if let Some(info) = self.traits.get(bound) {
            return (!info.constructor_slots.is_empty()).then(|| bound.to_string());
        }
        self.unique_constructor_trait_key(Self::surface_name(bound))
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

    pub(super) fn bare_constructor_occurrence(ty: &Ty) -> Option<u32> {
        let Ty::SelfApp(items) = ty else {
            return None;
        };
        match Self::constructor_application_parts(items) {
            Some((Ty::Var(occurrence), [])) => Some(*occurrence),
            _ => None,
        }
    }

    pub(super) fn is_concrete_constructor_shape(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::List(_) | Ty::Result(_, _) | Ty::Enum(_, _) | Ty::Struct(_, _) | Ty::Record(_, _)
        )
    }

    pub(super) fn constructor_application_slots_for_trait(
        &mut self,
        trait_key: &str,
        concrete_ty: &Ty,
    ) -> ConstructorSlotsOutcome {
        match self.constructor_projection(trait_key, concrete_ty) {
            ConstructorProjectionOutcome::Applicable { info, mapping } => {
                let mut slots = Vec::with_capacity(info.constructor_slot_vars.len());
                for variable in info.constructor_slot_vars.iter().copied() {
                    let Some(slot) = mapping.get(&variable).cloned() else {
                        return ConstructorSlotsOutcome::Rejected {
                            failures: vec![
                                ConstructorProjectionFailure::MissingConstructorSlotMapping {
                                    variable,
                                },
                            ],
                        };
                    };
                    slots.push(slot);
                }
                ConstructorSlotsOutcome::Projected(slots)
            }
            ConstructorProjectionOutcome::Deferred { waiting_on } => {
                ConstructorSlotsOutcome::Deferred { waiting_on }
            }
            ConstructorProjectionOutcome::Rejected { failures } => {
                ConstructorSlotsOutcome::Rejected { failures }
            }
        }
    }

    pub(super) fn constructor_application_slots_for_witness(
        &mut self,
        witness: &Ty,
        _expected_slot_count: usize,
        concrete_ty: &Ty,
    ) -> ConstructorSlotsOutcome {
        if let Ty::Var(var) = witness {
            if let Some(trait_key) = self.constructor_witness_traits.get(var).cloned() {
                return self.constructor_application_slots_for_trait(&trait_key, concrete_ty);
            }
            return ConstructorSlotsOutcome::Deferred {
                waiting_on: vec![*var],
            };
        }
        ConstructorSlotsOutcome::Rejected {
            failures: vec![ConstructorProjectionFailure::MissingWitnessTrait],
        }
    }

    pub(super) fn apply_constructor_application(
        &self,
        witness_source: &Ty,
        witness: &Ty,
        slots: &[Ty],
    ) -> ConstructorApplicationOutcome {
        match witness {
            Ty::Var(variable) => ConstructorApplicationOutcome::Deferred {
                waiting_on: vec![*variable],
            },
            Ty::List(_) if slots.len() == 1 => {
                ConstructorApplicationOutcome::Applied(Ty::List(Box::new(slots[0].clone())))
            }
            Ty::Result(_, err) if slots.len() == 1 => ConstructorApplicationOutcome::Applied(
                Ty::Result(Box::new(slots[0].clone()), err.clone()),
            ),
            Ty::Enum(name, args) => {
                let positions =
                    match self.constructor_positions_for_application(witness_source, witness) {
                        Ok(positions) => positions,
                        Err(outcome) => return outcome,
                    };
                if positions.len() != slots.len() {
                    return ConstructorApplicationOutcome::Rejected {
                        failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                            expected: positions.len(),
                            actual: slots.len(),
                        }],
                    };
                }
                let mut applied = args.clone();
                for (position, slot) in positions.into_iter().zip(slots) {
                    let Some(argument) = applied.get_mut(position) else {
                        return ConstructorApplicationOutcome::Rejected {
                            failures: vec![ConstructorProjectionFailure::InvalidSlotPosition {
                                position,
                            }],
                        };
                    };
                    *argument = slot.clone();
                }
                ConstructorApplicationOutcome::Applied(Ty::Enum(name.clone(), applied))
            }
            Ty::Struct(name, _) | Ty::Record(name, _) => {
                let positions =
                    match self.constructor_positions_for_application(witness_source, witness) {
                        Ok(positions) => positions,
                        Err(outcome) => return outcome,
                    };
                if positions.len() != slots.len() {
                    return ConstructorApplicationOutcome::Rejected {
                        failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                            expected: positions.len(),
                            actual: slots.len(),
                        }],
                    };
                }
                let Some(mut args) = self.resolved_named_type_args(name, witness) else {
                    return ConstructorApplicationOutcome::Rejected {
                        failures: vec![ConstructorProjectionFailure::MissingNominalArguments],
                    };
                };
                for (position, slot) in positions.into_iter().zip(slots) {
                    let Some(argument) = args.get_mut(position) else {
                        return ConstructorApplicationOutcome::Rejected {
                            failures: vec![ConstructorProjectionFailure::InvalidSlotPosition {
                                position,
                            }],
                        };
                    };
                    *argument = slot.clone();
                }
                let Some(def) = self.env.lookup_type_def(name) else {
                    return ConstructorApplicationOutcome::Rejected {
                        failures: vec![ConstructorProjectionFailure::MissingNominalArguments],
                    };
                };
                let fields = self.instantiate_type_def_fields(def, &args);
                match witness {
                    Ty::Struct(_, _) => ConstructorApplicationOutcome::Applied(Ty::Struct(
                        name.clone(),
                        NominalType::new(args, fields),
                    )),
                    Ty::Record(_, _) => ConstructorApplicationOutcome::Applied(Ty::Record(
                        name.clone(),
                        NominalType::new(args, fields),
                    )),
                    _ => unreachable!("matched nominal record kind"),
                }
            }
            Ty::List(_) | Ty::Result(_, _) => ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                    expected: 1,
                    actual: slots.len(),
                }],
            },
            _ => ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::UnsupportedConstructor],
            },
        }
    }

    fn constructor_positions_for_application(
        &self,
        witness_source: &Ty,
        witness: &Ty,
    ) -> Result<Vec<usize>, ConstructorApplicationOutcome> {
        let Ty::Var(variable) = witness_source else {
            return Err(ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::MissingWitnessTrait],
            });
        };
        let Some(trait_key) = self.constructor_witness_traits.get(variable) else {
            return Err(ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::MissingWitnessTrait],
            });
        };
        match self.constructor_projection(trait_key, witness) {
            ConstructorProjectionOutcome::Applicable { info, .. } => {
                Ok(info.constructor_slot_positions.clone())
            }
            ConstructorProjectionOutcome::Deferred { waiting_on } => {
                Err(ConstructorApplicationOutcome::Deferred { waiting_on })
            }
            ConstructorProjectionOutcome::Rejected { failures } => {
                Err(ConstructorApplicationOutcome::Rejected { failures })
            }
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
            // Generic kind variables occur only in builtin compose signatures.
            // They are a constraint placeholder until a concrete path is built.
            return Ok(crate::types::FacetKind::ReadablePath);
        }
        Ok(
            crate::types::FacetKind::from_surface_name(Self::surface_name(name))
                .expect("validated Facet kind name"),
        )
    }

    pub(super) fn canonical_user_type_name(name: &str) -> String {
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

    fn builtin_type_intrinsic_signature_only(name: &str) -> Option<IntrinsicId> {
        match builtin_type_usage_policy(Self::surface_name(name)).map(|policy| policy.usage) {
            Some(BuiltinTypeUsage::IntrinsicSignatureOnly(intrinsic)) => Some(intrinsic),
            _ => None,
        }
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
                "Use tuple payloads for extractor success values, such as MatchResult<(A, B)>."
                    .into(),
            ),
        }
    }

    pub(super) fn lazy_type_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: "Lazy<T> is reserved for standard callable parameter declarations".into(),
            span: span.clone(),
            hint: Some(
                "Use a zero-argument function type in user declarations; standard Lazy parameters normalize their inputs at call sites."
                    .into(),
            ),
        }
    }

    pub(super) fn ensure_no_match_result_value(
        &self,
        ty: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        fn contains(ty: &Ty) -> bool {
            match ty {
                Ty::MatchResult(_) => true,
                Ty::List(inner) | Ty::Lazy(inner) | Ty::Pid(inner) => contains(inner),
                Ty::Tuple(items) | Ty::Enum(_, items) | Ty::SelfApp(items) => {
                    items.iter().any(contains)
                }
                Ty::Result(ok, err) => contains(ok) || contains(err),
                Ty::Func(params, ret)
                | Ty::UserFunc { params, ret, .. }
                | Ty::BuiltinFunc { params, ret, .. } => {
                    params.iter().any(contains) || contains(ret)
                }
                Ty::Struct(_, n) | Ty::Record(_, n) => {
                    n.arguments.iter().any(contains) || n.iter().any(|(_, ty)| contains(ty))
                }
                Ty::Facet(_, a, b, c, d) => [a, b, c, d].iter().any(|ty| contains(ty)),
                _ => false,
            }
        }
        if contains(&self.resolve_ty(ty)) {
            Err(TypeError::new(
                "MatchResult cannot be held or passed as an ordinary value",
                span.clone(),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn local_type_syntax_context(&self) -> TypeSyntaxContext {
        if matches!(
            self.callable_context,
            CallableContext::Extractor | CallableContext::ExtractorClosure
        ) {
            TypeSyntaxContext::ExtractorBody
        } else {
            TypeSyntaxContext::BindingAnnotation
        }
    }

    fn resolve_pid_surface_ty(&mut self, span: &Span, args: &[AstTy]) -> Result<Ty, TypeError> {
        if args.len() != 1 {
            return Err(TypeError {
                structured: None,
                message: "PID<T> requires exactly 1 type argument".into(),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(Ty::Pid(Box::new(self.pid_marker_from_ast(&args[0])?)))
    }

    fn resolve_worker_handle_surface_ty(
        &mut self,
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
            vec![Ty::Pid(Box::new(self.pid_marker_from_ast(&args[0])?))],
        ))
    }

    fn resolve_task_handle_surface_ty(
        &mut self,
        span: &Span,
        args: &[AstTy],
    ) -> Result<Ty, TypeError> {
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

    fn concrete_pid_marker(&self, ast_ty: &AstTy) -> Result<Ty, TypeError> {
        if let AstTy::Named(_, name) = ast_ty {
            let canonical = Self::canonical_user_type_name(name);
            if self
                .process_specs
                .iter()
                .any(|spec| spec.process_name == canonical)
                || matches!(
                    canonical.as_str(),
                    "Global::OutHandler" | "Global::InHandler"
                )
            {
                return Ok(Ty::ProcessMarker(canonical));
            }
            return Err(TypeError::new(
                format!("Unknown PID process marker `{name}`"),
                Self::ast_ty_span(ast_ty).clone(),
            ));
        }
        Err(TypeError::new(
            "PID<T> expects a declared process marker or a type variable",
            Self::ast_ty_span(ast_ty).clone(),
        ))
    }

    pub(super) fn constrain_pid_marker_type(
        &mut self,
        marker: &Ty,
        span: &Span,
        declaration: bool,
    ) -> Result<(), TypeError> {
        match self.resolve_ty(marker) {
            Ty::ProcessMarker(_) => Ok(()),
            Ty::Var(var) => {
                if !declaration
                    && self.rigid_tyvars.contains(&var)
                    && !self.process_marker_tyvars.contains(&var)
                {
                    return Err(TypeError::new(
                        "PID marker annotation cannot constrain an ordinary signature type variable",
                        span.clone(),
                    ));
                }
                self.process_marker_tyvars.insert(var);
                Ok(())
            }
            other => Err(TypeError::new(
                format!(
                    "PID<T> requires a process marker, got {}",
                    self.ty_name(&other)
                ),
                span.clone(),
            )),
        }
    }

    fn pid_marker_from_ast(&mut self, ast_ty: &AstTy) -> Result<Ty, TypeError> {
        let marker = match ast_ty {
            AstTy::Named(_, name) if name.starts_with('$') => {
                self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?
            }
            _ => self.concrete_pid_marker(ast_ty)?,
        };
        self.constrain_pid_marker_type(&marker, Self::ast_ty_span(ast_ty), false)?;
        Ok(marker)
    }

    fn resolve_signature_pid_surface_ty(
        &mut self,
        span: &Span,
        args: &[AstTy],
        handle_name: &str,
        tyvars: &mut HashMap<String, Ty>,
        mode: SignatureTyMode<'_>,
    ) -> Result<Ty, TypeError> {
        let [marker] = args else {
            return Err(TypeError::new(
                format!("{handle_name}<T> requires exactly 1 type argument"),
                span.clone(),
            ));
        };
        let marker = match marker {
            AstTy::Named(_, name) if name.starts_with('$') => self
                .resolve_signature_like_ast_ty_in_context(
                    marker,
                    TypeSyntaxContext::General,
                    tyvars,
                    mode,
                )?,
            _ => self.concrete_pid_marker(marker)?,
        };
        self.constrain_pid_marker_type(&marker, span, true)?;
        let pid = Ty::Pid(Box::new(marker));
        Ok(if handle_name == "PID" {
            pid
        } else {
            Ty::Enum(handle_name.to_string(), vec![pid])
        })
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

    fn intrinsic_marker_type_not_allowed_error(
        &self,
        span: &Span,
        intrinsic: IntrinsicId,
    ) -> TypeError {
        let (marker, surface) = match intrinsic {
            IntrinsicId::Do => (TypeName::DoBlock.as_str(), intrinsic.surface_name()),
        };
        TypeError::from_structured(diagnostics::StructuredDiagnostic {
            reason: diagnostics::TypeDiagnosticReason::ReservedIntrinsicMarkerUsage.into(),
            origin: diagnostics::DiagnosticOrigin::Intrinsic,
            data: diagnostics::DiagnosticData::Policy(diagnostics::PolicyData {
                policy: diagnostics::TypePolicy::IntrinsicMarkerUsage,
                subject: Some(marker.to_string()),
                expected_type: None,
                actual_type: Some(marker.to_string()),
                stage: None,
                entrypoint: Some(surface.to_string()),
            }),
            primary: diagnostics::SourceFact::untyped(
                diagnostics::SourceRole::Annotation,
                diagnostics::SourceId(0),
                span.clone(),
            ),
            related: Vec::new(),
            remediation: Some(diagnostics::Remediation::Help {
                text: format!(
                    "Remove `{marker}`; write a `{surface} {{ ... }}` expression to sequence Monad values."
                ),
            }),
        })
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
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
    ) -> Result<Ty, TypeError> {
        let result = self.resolve_ast_ty_in_context_inner(ast_ty, context);
        if let (Err(error), Some(probe)) = (&result, &self.type_syntax_probe_error) {
            let mut retained = probe.borrow_mut();
            if retained.is_none() {
                *retained = Some(error.clone());
            }
        }
        result
    }

    fn resolve_ast_ty_in_context_inner(
        &mut self,
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
                    builtin_name
                        if Self::builtin_type_intrinsic_signature_only(builtin_name)
                            .is_some() =>
                    {
                        Err(self.intrinsic_marker_type_not_allowed_error(
                            span,
                            Self::builtin_type_intrinsic_signature_only(builtin_name)
                                .expect("guarded intrinsic marker"),
                        ))
                    }
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
                                    return Ok(Ty::Struct(
                                        def.name.clone(),
                                        NominalType::monomorphic(def.fields.clone()),
                                    ));
                                }
                                crate::env::TypeKind::Record => {
                                    return Ok(Ty::Record(
                                        def.name.clone(),
                                        NominalType::monomorphic(def.fields.clone()),
                                    ));
                                }
                                crate::env::TypeKind::ConcreteError => {
                                    if context == TypeSyntaxContext::ConcreteErrorLocal {
                                        return Ok(Ty::Error);
                                    }
                                    return Err(TypeError::new("Concrete Error types are only valid for an already narrowed local binding", span.clone()));
                                },
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
                            Some(TypeName::ErrorKind) => Ok(Ty::Enum("ErrorKind".into(), Vec::new())),
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
                                | TypeName::InfiniteGenerator
                                | TypeName::Result
                                | TypeName::ExtractorClosure | TypeName::MatchResult
                                | TypeName::Duration
                                | TypeName::StandbyInit
                                | TypeName::Lazy
                                | TypeName::Hole
                                | TypeName::Closure
                                | TypeName::MatchArms
                                | TypeName::CondClauses
                                | TypeName::DoBlock
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
                if Self::builtin_type_intrinsic_signature_only(name).is_some() =>
            {
                Err(self.intrinsic_marker_type_not_allowed_error(
                    span,
                    Self::builtin_type_intrinsic_signature_only(name)
                        .expect("guarded intrinsic marker"),
                ))
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
                name @ ("Generator" | "InfiniteGenerator") => {
                    let args = self.require_type_arg_count(
                        span, args, 1,
                        if name == "Generator" { "Generator<Item> requires exactly 1 type argument" } else { "InfiniteGenerator<Item> requires exactly 1 type argument" },
                    )?;
                    let item_ty = self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::Enum(name.into(), vec![item_ty]))
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
                "ExtractorClosure" => {
                    let [AstTy::Func(_, params, ret)] = args.as_slice() else { return Err(TypeError::new("ExtractorClosure requires exactly one function signature", span.clone())); };
                    if params.is_empty() { return Err(TypeError::new("ExtractorClosure requires at least one input", span.clone())); }
                    let params = params.iter().map(|ty| self.resolve_ast_ty_in_context(ty, TypeSyntaxContext::General)).collect::<Result<Vec<_>, _>>()?;
                    let ret = self.resolve_ast_ty_in_context(
                        ret,
                        TypeSyntaxContext::ExtractorClosureReturn,
                    )?;
                    if !matches!(ret, Ty::MatchResult(_)) { return Err(TypeError::new("ExtractorClosure must return MatchResult", span.clone())); }
                    Ok(Ty::ExtractorClosure(Box::new(Ty::Func(params, Box::new(ret)))))
                }
                "MatchResult" => {
                    if context != TypeSyntaxContext::ExtractorReturn
                        && context != TypeSyntaxContext::ExtractorClosureReturn
                    {
                        return Err(TypeError::new("MatchResult is only allowed as an Extractor return type", span.clone()));
                    }
                    if !(1..=2).contains(&args.len()) {
                        return Err(TypeError::new("MatchResult<P, Error> requires 1 or 2 type arguments", span.clone()));
                    }
                    if args.len() == 2 && context == TypeSyntaxContext::ExtractorClosureReturn {
                        return Err(TypeError::new(
                            "MatchResult<P, Error> is only allowed in defextractor return signatures",
                            Self::ast_ty_span(&args[1]).clone(),
                        ));
                    }
                    if args.len() == 2 && !matches!(&args[1], AstTy::Named(_, name) if name == "Error") {
                        return Err(TypeError::new("MatchResult second type argument must be canonical Error", Self::ast_ty_span(&args[1]).clone()));
                    }
                    let payload = self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::MatchResult(Box::new(payload)))
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
                        if context != TypeSyntaxContext::FunctionReturn {
                            return Err(TypeError {
                                structured: None,
                                message:
                                    "Result<T, E> is only allowed in function return signatures."
                                        .into(),
                                span: span.clone(),
                                hint: Some(
                                    "Use Result<T> in local code."
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
                    let def = self.env.lookup_type_def(name).cloned().ok_or_else(|| TypeError {
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
                    let mut resolved_args = Vec::with_capacity(args.len());
                    for (index, (argument, bound)) in args.iter().zip(&def.type_param_bounds).enumerate() {
                        let constructor_trait = bound
                            .as_deref()
                            .and_then(|bound| self.declaration_constructor_trait_key(bound));
                        let marker_slot = def.type_param_vars.get(index)
                            .is_some_and(|var| self.process_marker_tyvars.contains(var));
                        let resolved = if marker_slot {
                            self.pid_marker_from_ast(argument)?
                        } else if let Some(trait_key) = constructor_trait.filter(|_| {
                            !matches!(argument, AstTy::Named(_, variable) if variable.starts_with('$'))
                        }) {
                            let head = self.resolve_type_constructor_head(argument)?;
                            self.normalize_inferred_constructor_identity(&trait_key, &head)
                                .unwrap_or(head)
                        } else {
                            self.resolve_ast_ty_in_context(
                                argument,
                                TypeSyntaxContext::General,
                            )?
                        };
                        resolved_args.push(resolved);
                    }
                    self.validate_nominal_type_arguments(&def, &resolved_args, span, false)?;
                    match def.kind {
                        crate::env::TypeKind::Struct => Ok(Ty::Struct(
                            def.name.clone(),
                            NominalType::new(
                                resolved_args.clone(),
                                self.instantiate_type_def_fields(&def, &resolved_args),
                            ),
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
                let ret = self.resolve_ast_ty_in_context(ret, Self::function_type_return_context(context))?;
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
    /// TypeCtorTrait slot accepts a bare head, a complete carrier, or a carrier
    /// application containing RTA-local inference holes.
    pub(super) fn resolve_call_site_return_type_argument(
        &mut self,
        slot_ty: &Ty,
        ast_ty: &AstTy,
    ) -> Result<Ty, TypeError> {
        if matches!(ast_ty, AstTy::Named(_, name) if Self::surface_name(name) == "_") {
            return Ok(self.env.fresh_tyvar());
        }
        if matches!(slot_ty, Ty::Var(var) if self.process_marker_tyvars.contains(var)) {
            return self.pid_marker_from_ast(ast_ty);
        }
        let is_constructor_slot = match slot_ty {
            Ty::SelfApp(items) => Self::constructor_application_parts(items).is_some(),
            Ty::Var(variable) => self.constructor_witness_traits.contains_key(variable),
            _ => false,
        };
        if !is_constructor_slot {
            return self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General);
        }

        self.resolve_type_constructor_return_type_argument(ast_ty)
    }

    /// Trait implementations can relate an ordinary return-only type input to
    /// the receiver and value parameters.  A bare generic head constrains that
    /// input's declaration identity while the canonical impl solver determines
    /// its arguments from those shared variables.
    pub(super) fn resolve_trait_call_site_return_type_argument(
        &mut self,
        slot_ty: &Ty,
        ast_ty: &AstTy,
    ) -> Result<Ty, TypeError> {
        let is_constructor_slot = matches!(
            slot_ty,
            Ty::SelfApp(items) if Self::constructor_application_parts(items).is_some()
        );
        if !is_constructor_slot
            && matches!(ast_ty, AstTy::Named(_, name) if self.type_constructor_arity(name).is_some())
        {
            return self.resolve_type_constructor_head(ast_ty);
        }
        self.resolve_call_site_return_type_argument(slot_ty, ast_ty)
    }

    pub(super) fn resolve_type_constructor_return_type_argument(
        &mut self,
        ast_ty: &AstTy,
    ) -> Result<Ty, TypeError> {
        let outer_name = match ast_ty {
            AstTy::Named(_, name) | AstTy::Generic(_, name, _) if name.starts_with('$') => {
                Some(name)
            }
            _ => None,
        };
        if let Some(name) = outer_name {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Return type argument cannot use constructor variable `{name}` as its carrier"
                ),
                span: Self::ast_ty_span(ast_ty).clone(),
                hint: Some(
                    "Use a concrete carrier head, a complete carrier application, or `_`.".into(),
                ),
            });
        }
        self.resolve_type_constructor_return_type_argument_part(ast_ty)
    }

    fn resolve_type_constructor_return_type_argument_part(
        &mut self,
        ast_ty: &AstTy,
    ) -> Result<Ty, TypeError> {
        match ast_ty {
            AstTy::Named(_, name) if Self::surface_name(name) == "_" => Ok(self.env.fresh_tyvar()),
            AstTy::Named(_, name) if name.starts_with('$') => {
                self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)
            }
            AstTy::Named(_, name) if self.type_constructor_arity(name).is_some() => {
                self.resolve_type_constructor_head(ast_ty)
            }
            AstTy::Named(_, _) => {
                self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)
            }
            AstTy::Generic(span, name, arguments) => {
                let resolved_arguments = arguments
                    .iter()
                    .map(|argument| {
                        self.resolve_type_constructor_return_type_argument_part(argument)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match Self::surface_name(name) {
                    "List" => {
                        let [inner] = resolved_arguments.as_slice() else {
                            return Err(TypeError::new(
                                "List<T> requires exactly 1 type argument",
                                span.clone(),
                            ));
                        };
                        Ok(Ty::List(Box::new(inner.clone())))
                    }
                    "HashMap" => {
                        let [value] = resolved_arguments.as_slice() else {
                            return Err(TypeError::new(
                                "HashMap<V> requires exactly 1 type argument",
                                span.clone(),
                            ));
                        };
                        Ok(Ty::Enum("HashMap".into(), vec![value.clone()]))
                    }
                    name @ ("Generator" | "InfiniteGenerator") => {
                        let [item] = resolved_arguments.as_slice() else {
                            return Err(TypeError::new(
                                format!("{}<Item> requires exactly 1 type argument", name),
                                span.clone(),
                            ));
                        };
                        Ok(Ty::Enum(name.into(), vec![item.clone()]))
                    }
                    "Result" => match resolved_arguments.as_slice() {
                        [ok] => Ok(Ty::Result(Box::new(ok.clone()), Box::new(Ty::Error))),
                        [ok, error] => {
                            Ok(Ty::Result(Box::new(ok.clone()), Box::new(error.clone())))
                        }
                        _ => Err(TypeError::new(
                            "Result<T, E> requires 1 or 2 type arguments",
                            span.clone(),
                        )),
                    },
                    "Lazy" => {
                        let [inner] = resolved_arguments.as_slice() else {
                            return Err(TypeError::new(
                                "Lazy<T> requires exactly 1 type argument",
                                span.clone(),
                            ));
                        };
                        Ok(Ty::Lazy(Box::new(inner.clone())))
                    }
                    _ => {
                        let definition =
                            self.env
                                .lookup_type_def(name)
                                .cloned()
                                .ok_or_else(|| TypeError {
                                    structured: None,
                                    message: format!("Unknown generic type: {name}"),
                                    span: span.clone(),
                                    hint: None,
                                })?;
                        if definition.type_params.len() != resolved_arguments.len() {
                            return Err(TypeError {
                                structured: None,
                                message: format!(
                                    "Type {} requires {} type argument(s), got {}",
                                    name,
                                    definition.type_params.len(),
                                    resolved_arguments.len()
                                ),
                                span: span.clone(),
                                hint: None,
                            });
                        }
                        self.constrain_rta_nominal_arguments(
                            &definition,
                            &resolved_arguments,
                            span,
                        )?;
                        match definition.kind {
                            crate::env::TypeKind::Struct => Ok(Ty::Struct(
                                definition.name.clone(),
                                NominalType::new(
                                    resolved_arguments.clone(),
                                    self.instantiate_type_def_fields(
                                        &definition,
                                        &resolved_arguments,
                                    ),
                                ),
                            )),
                            crate::env::TypeKind::Record => Ok(Ty::Record(
                                definition.name.clone(),
                                NominalType::new(
                                    resolved_arguments.clone(),
                                    self.instantiate_type_def_fields(
                                        &definition,
                                        &resolved_arguments,
                                    ),
                                ),
                            )),
                            crate::env::TypeKind::Enum => Ok(Self::builtin_special_enum_ty(
                                &definition.name,
                                &resolved_arguments,
                            )
                            .unwrap_or_else(|| {
                                Ty::Enum(definition.name.clone(), resolved_arguments)
                            })),
                            crate::env::TypeKind::ConcreteError => Err(TypeError::new(
                                format!("Error type {name} is not a carrier"),
                                span.clone(),
                            )),
                        }
                    }
                }
            }
            AstTy::Tuple(_, items) => Ok(Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.resolve_type_constructor_return_type_argument_part(item))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            AstTy::Func(_, parameters, return_type) => Ok(Ty::Func(
                parameters
                    .iter()
                    .map(|parameter| {
                        self.resolve_type_constructor_return_type_argument_part(parameter)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                Box::new(self.resolve_type_constructor_return_type_argument_part(return_type)?),
            )),
            AstTy::ImplTrait(span, name) => Err(TypeError::new(
                format!("`impl {name}` is not a carrier ReturnTypeArgument"),
                span.clone(),
            )),
        }
    }

    pub(super) fn resolve_type_constructor_head(
        &mut self,
        ast_ty: &AstTy,
    ) -> Result<Ty, TypeError> {
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
            return Err(self.hole_not_allowed_error(span));
        }

        // A bare head constrains only the declaration identity. Every
        // argument remains an invocation variable, including captured ones;
        // implementation patterns are not sources of inferred inputs.
        let Some(def) = self.env.lookup_type_def(name).cloned() else {
            let mut fresh = || self.env.fresh_tyvar();
            let builtin_head = match builtin_type_name(Self::surface_name(name)) {
                Some(TypeName::List) => Some(Ty::List(Box::new(fresh()))),
                Some(TypeName::HashMap) => Some(Ty::Enum("HashMap".into(), vec![fresh()])),
                Some(name @ (TypeName::Generator | TypeName::InfiniteGenerator)) => {
                    Some(Ty::Enum(name.as_str().into(), vec![fresh()]))
                }
                Some(TypeName::Result) => Some(Ty::Result(Box::new(fresh()), Box::new(fresh()))),
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
        self.constrain_rta_nominal_arguments(&def, &args, span)?;
        let ty = match def.kind {
            crate::env::TypeKind::Struct => Ty::Struct(
                def.name.clone(),
                NominalType::new(args.clone(), self.instantiate_type_def_fields(&def, &args)),
            ),
            crate::env::TypeKind::Record => Ty::Record(
                def.name.clone(),
                NominalType::new(args.clone(), self.instantiate_type_def_fields(&def, &args)),
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

    fn type_constructor_arity(&self, name: &str) -> Option<usize> {
        if let Some(definition) = self.env.lookup_type_def(name) {
            return (!definition.type_params.is_empty()).then_some(definition.type_params.len());
        }
        match builtin_type_name(Self::surface_name(name)) {
            Some(
                TypeName::List
                | TypeName::HashMap
                | TypeName::StandbyInit
                | TypeName::Lazy
                | TypeName::TaskHandle
                | TypeName::Generator
                | TypeName::InfiniteGenerator,
            ) => Some(1),
            Some(TypeName::Result) => Some(2),
            _ => None,
        }
    }

    fn constrain_rta_nominal_arguments(
        &mut self,
        definition: &crate::env::TypeDefInfo,
        arguments: &[Ty],
        span: &Span,
    ) -> Result<(), TypeError> {
        for (argument, bound) in arguments.iter().zip(&definition.type_param_bounds) {
            let (Ty::Var(variable), Some(bound)) = (self.resolve_ty(argument), bound) else {
                continue;
            };
            self.register_tyvar_bound(variable, bound);
            if let Some(trait_key) = self.declaration_constructor_trait_key(bound) {
                self.constructor_witness_traits.insert(variable, trait_key);
            }
        }
        self.validate_nominal_type_arguments(definition, arguments, span, true)
    }

    pub(super) fn resolve_builtin_ast_ty(
        &mut self,
        ast_ty: &AstTy,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_builtin_ast_ty_in_context(
            ast_ty,
            TypeSyntaxContext::StdBuiltinParameter,
            tyvars,
        )
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

    pub(super) fn resolve_resolved_signature_ty_in_context(
        &mut self,
        signature_ty: &sigil::resolved::ResolvedSignatureTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_resolved_signature_ty_with_mode(
            signature_ty,
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
                    if let Some(trait_key) = self.declaration_constructor_trait_key(bound) {
                        self.constructor_witness_traits.insert(var, trait_key);
                    }
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

    pub(super) fn lazy_parameter_inner(ast: &AstTy) -> Option<&AstTy> {
        match ast {
            AstTy::Generic(_, name, arguments)
                if Self::builtin_type_is_lazy_signature_surface_only(name)
                    && arguments.len() == 1 =>
            {
                Some(&arguments[0])
            }
            _ => None,
        }
    }

    pub(super) fn resolve_trait_parameter_contract(
        &mut self,
        signature: &sigil::resolved::ResolvedSignatureTy,
        self_ty: &Ty,
        tyvars: &mut HashMap<String, Ty>,
        allows_lazy: bool,
    ) -> Result<Ty, TypeError> {
        if let Some(inner) = Self::lazy_parameter_inner(signature.syntax()) {
            if !allows_lazy {
                return Err(self.lazy_type_not_allowed_error(Self::ast_ty_span(signature.syntax())));
            }
            let result = self.resolve_trait_signature_ast_ty_in_context(
                inner,
                TypeSyntaxContext::General,
                self_ty,
                tyvars,
            )?;
            return Ok(Ty::Func(vec![], Box::new(result)));
        }
        self.resolve_trait_signature_ty_in_context(
            signature,
            TypeSyntaxContext::General,
            self_ty,
            tyvars,
        )
    }

    pub(super) fn resolve_trait_signature_ty_in_context(
        &mut self,
        signature_ty: &sigil::resolved::ResolvedSignatureTy,
        context: TypeSyntaxContext,
        self_ty: &Ty,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_resolved_signature_ty_with_mode(
            signature_ty,
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

    pub(super) fn resolve_builtin_signature_ty_in_context(
        &mut self,
        signature_ty: &sigil::resolved::ResolvedSignatureTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_resolved_signature_ty_with_mode(
            signature_ty,
            context,
            tyvars,
            SignatureTyMode::Builtin,
        )
    }

    fn resolve_resolved_signature_ty_with_mode(
        &mut self,
        signature_ty: &sigil::resolved::ResolvedSignatureTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
        mode: SignatureTyMode<'_>,
    ) -> Result<Ty, TypeError> {
        let context = Self::std_parameter_type_context(context);
        let Some(trait_id) = signature_ty.direct_constructor_trait.as_ref() else {
            return self.resolve_signature_like_ast_ty_in_context(
                signature_ty.syntax(),
                context,
                tyvars,
                mode.without_direct_constructor_name_fallback(),
            );
        };
        let trait_key = self.trait_key(trait_id);
        let Some(trait_info) = self.traits.get(&trait_key) else {
            return Err(TypeError::new(
                format!(
                    "MissingCanonicalTypeConstructorTrait: resolved Trait id {} ({}) is not registered",
                    trait_id.unique_id, trait_key
                ),
                trait_id.span.clone(),
            ));
        };
        if trait_info.id.unique_id != trait_id.unique_id || trait_info.constructor_slots.is_empty()
        {
            return Err(TypeError::new(
                format!(
                    "InvalidCanonicalTypeConstructorTrait: resolved Trait id {} does not name a TypeCtorTrait",
                    trait_id.unique_id
                ),
                trait_id.span.clone(),
            ));
        }
        if !trait_info.type_params.is_empty() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Direct signature use of parameterized constructor trait {} is not supported",
                    trait_id.name
                ),
                span: trait_id.span.clone(),
                hint: Some(
                    "Introduce a named constructor parameter and constrain it in a where clause."
                        .into(),
                ),
            });
        }
        let slot_count = trait_info.constructor_slots.len();
        match signature_ty.syntax() {
            AstTy::Named(span, _) => {
                if !matches!(context, TypeSyntaxContext::General) {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "`{}` requires constructor slot arguments in this signature position",
                            Self::surface_ast_ty(signature_ty.syntax())
                        ),
                        span: span.clone(),
                        hint: Some("Write the direct Trait application with all declared constructor slots.".into()),
                    });
                }
                let witness = self.env.fresh_tyvar();
                let Ty::Var(witness_var) = witness else {
                    unreachable!("fresh constructor witness")
                };
                self.constructor_witness_traits.insert(witness_var, trait_key);
                Ok(Ty::SelfApp(vec![Ty::Hole, Ty::Var(witness_var)]))
            }
            AstTy::Generic(span, name, arguments) => {
                if arguments.len() != slot_count {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Type constructor Trait {} requires {} slot argument(s), got {}",
                            trait_id.name,
                            slot_count,
                            arguments.len()
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let witness = if name.starts_with('$') {
                    tyvars
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| self.env.fresh_tyvar())
                } else {
                    self.env.fresh_tyvar()
                };
                let Ty::Var(witness_var) = witness else {
                    unreachable!("fresh constructor witness")
                };
                self.constructor_witness_traits.insert(witness_var, trait_key);
                let mut application = vec![Ty::Hole, Ty::Var(witness_var)];
                for argument in arguments {
                    application.push(self.resolve_signature_like_ast_ty_in_context(
                        argument,
                        TypeSyntaxContext::General,
                        tyvars,
                        mode.without_direct_constructor_name_fallback(),
                    )?);
                }
                Ok(Ty::SelfApp(application))
            }
            AstTy::ImplTrait(_, _) | AstTy::Tuple(_, _) | AstTy::Func(_, _, _) => Err(
                TypeError::new(
                    "InvalidCanonicalTypeConstructorTrait: canonical identity is attached to a non-direct type",
                    trait_id.span.clone(),
                ),
            ),
        }
    }

    fn std_parameter_type_context(context: TypeSyntaxContext) -> TypeSyntaxContext {
        if context == TypeSyntaxContext::StdBuiltinParameter {
            TypeSyntaxContext::General
        } else {
            context
        }
    }

    fn function_type_return_context(context: TypeSyntaxContext) -> TypeSyntaxContext {
        match context {
            // Retain nested ignored-input callable syntax, without propagating
            // the error-contract permission of a definition's direct return.
            TypeSyntaxContext::FunctionReturn => TypeSyntaxContext::BindingAnnotation,
            TypeSyntaxContext::ExtractorReturn | TypeSyntaxContext::ExtractorClosureReturn => {
                TypeSyntaxContext::General
            }
            context => context,
        }
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
        let context = Self::std_parameter_type_context(context);
        match ast_ty {
            AstTy::Named(span, name) => {
                if let Some(alias) =
                    self.resolve_signature_alias(span, name, &[], context, tyvars, mode)?
                {
                    return Ok(alias);
                }
                if mode.allows_direct_constructor_name_fallback() {
                    if let Some(trait_key) =
                        self.unique_constructor_trait_key(Self::surface_name(name))
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
                        // Allocate per occurrence here; signature normalization
                        // later coalesces occurrences of the same direct Trait
                        // name while leaving different names independent.
                        let witness = self.env.fresh_tyvar();
                        let Ty::Var(witness_var) = witness else {
                            unreachable!("fresh constructor witness must be a type variable")
                        };
                        self.constructor_witness_traits
                            .insert(witness_var, trait_key);
                        return Ok(Ty::SelfApp(vec![Ty::Hole, Ty::Var(witness_var)]));
                    }
                }
            }
            AstTy::Generic(span, name, args) => {
                if let Some(alias) =
                    self.resolve_signature_alias(span, name, args, context, tyvars, mode)?
                {
                    return Ok(alias);
                }
                if mode.allows_direct_constructor_name_fallback() {
                    if let Some(trait_key) =
                        self.unique_constructor_trait_key(Self::surface_name(name))
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
                        // Return positions also start with a fresh witness here.
                        // Signature normalization reconnects it only to the same
                        // direct Trait name, never by family/root identity.
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
            AstTy::Named(span, name)
                if Self::builtin_type_intrinsic_signature_only(name).is_some() =>
            {
                Err(self.intrinsic_marker_type_not_allowed_error(
                    span,
                    Self::builtin_type_intrinsic_signature_only(name)
                        .expect("guarded intrinsic marker"),
                ))
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
            AstTy::Generic(span, name, args) if name.starts_with('$') => {
                let witness = if let Some(existing) = tyvars.get(name) {
                    existing.clone()
                } else {
                    let fresh = self.env.fresh_tyvar();
                    tyvars.insert(name.clone(), fresh.clone());
                    fresh
                };
                if let Ty::Var(variable) = &witness {
                    if let Some(trait_key) = self.constructor_witness_traits.get(variable) {
                        let expected = self
                            .traits
                            .get(trait_key)
                            .expect("constructor witness Trait must remain registered")
                            .constructor_slots
                            .len();
                        if args.len() != expected {
                            return Err(TypeError {
                                structured: None,
                                message: format!(
                                    "Type constructor parameter {} requires {} slot argument(s), got {}",
                                    name,
                                    expected,
                                    args.len()
                                ),
                                span: span.clone(),
                                hint: None,
                            });
                        }
                    }
                }
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
                if Self::builtin_type_intrinsic_signature_only(name).is_some() =>
            {
                Err(self.intrinsic_marker_type_not_allowed_error(
                    span,
                    Self::builtin_type_intrinsic_signature_only(name)
                        .expect("guarded intrinsic marker"),
                ))
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
                name @ ("Generator" | "InfiniteGenerator") => {
                    let args = self.require_type_arg_count(
                        span, args, 1,
                        if name == "Generator" { "Generator<Item> requires exactly 1 type argument" } else { "InfiniteGenerator<Item> requires exactly 1 type argument" },
                    )?;
                    let item_ty = self.resolve_signature_like_ast_ty_in_context(
                        &args[0], TypeSyntaxContext::General, tyvars, mode,
                    )?;
                    Ok(Ty::Enum(name.into(), vec![item_ty]))
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
                        matches!(
                            mode,
                            SignatureTyMode::Builtin | SignatureTyMode::ResolvedBuiltin
                        ),
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
                "PID" => self.resolve_signature_pid_surface_ty(span, args, "PID", tyvars, mode),
                "Workers" => self.resolve_signature_pid_surface_ty(span, args, "Workers", tyvars, mode),
                "WorkerLease" => self.resolve_signature_pid_surface_ty(span, args, "WorkerLease", tyvars, mode),
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
                "ExtractorClosure" => {
                    let [AstTy::Func(_, params, ret)] = args.as_slice() else { return Err(TypeError::new("ExtractorClosure requires exactly one function signature", span.clone())); };
                    if params.is_empty() { return Err(TypeError::new("ExtractorClosure requires at least one input", span.clone())); }
                    let params = params.iter().map(|ty| self.resolve_signature_like_ast_ty_in_context(ty, TypeSyntaxContext::General, tyvars, mode)).collect::<Result<Vec<_>, _>>()?;
                    let ret = self.resolve_signature_like_ast_ty_in_context(ret, TypeSyntaxContext::ExtractorClosureReturn, tyvars, mode)?;
                    if !matches!(ret, Ty::MatchResult(_)) { return Err(TypeError::new("ExtractorClosure must return MatchResult", span.clone())); }
                    Ok(Ty::ExtractorClosure(Box::new(Ty::Func(params, Box::new(ret)))))
                }
                "MatchResult" => {
                    if context != TypeSyntaxContext::ExtractorReturn
                        && context != TypeSyntaxContext::ExtractorClosureReturn
                    {
                        return Err(TypeError::new("MatchResult is only allowed as an Extractor return type", span.clone()));
                    }
                    if !(1..=2).contains(&args.len()) {
                        return Err(TypeError::new("MatchResult<P, Error> requires 1 or 2 type arguments", span.clone()));
                    }
                    if args.len() == 2 && context == TypeSyntaxContext::ExtractorClosureReturn {
                        return Err(TypeError::new(
                            "MatchResult<P, Error> is only allowed in defextractor return signatures",
                            Self::ast_ty_span(&args[1]).clone(),
                        ));
                    }
                    if args.len() == 2 && !matches!(&args[1], AstTy::Named(_, name) if name == "Error") {
                        return Err(TypeError::new("MatchResult second type argument must be canonical Error", Self::ast_ty_span(&args[1]).clone()));
                    }
                    let payload = self.resolve_signature_like_ast_ty_in_context(&args[0], TypeSyntaxContext::General, tyvars, mode)?;
                    Ok(Ty::MatchResult(Box::new(payload)))
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
                            message: if matches!(
                                mode,
                                SignatureTyMode::ResolvedNormal
                                    | SignatureTyMode::ResolvedTrait { .. }
                                    | SignatureTyMode::ResolvedBuiltin
                            ) {
                                format!("Unknown type: {}", name)
                            } else {
                                format!("Unknown generic type: {}", name)
                            },
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
                    let mut resolved_args = Vec::with_capacity(args.len());
                    for (index, (argument, bound)) in args.iter().zip(&def.type_param_bounds).enumerate() {
                        let constructor_trait = bound
                            .as_deref()
                            .and_then(|bound| self.declaration_constructor_trait_key(bound));
                        let marker_slot = def.type_param_vars.get(index)
                            .is_some_and(|var| self.process_marker_tyvars.contains(var));
                        let resolved = if marker_slot {
                            let marker = match argument {
                                AstTy::Named(_, name) if name.starts_with('$') =>
                                    self.resolve_signature_like_ast_ty_in_context(
                                        argument, TypeSyntaxContext::General, tyvars, mode,
                                    )?,
                                _ => self.concrete_pid_marker(argument)?,
                            };
                            self.constrain_pid_marker_type(&marker, Self::ast_ty_span(argument), true)?;
                            marker
                        } else if let Some(trait_key) = constructor_trait.filter(|_| {
                            !matches!(argument, AstTy::Named(_, variable) if variable.starts_with('$'))
                        }) {
                            let head = self.resolve_type_constructor_head(argument)?;
                            if matches!(mode, SignatureTyMode::ImplHead { .. }) {
                                head
                            } else {
                                self.normalize_inferred_constructor_identity(&trait_key, &head)
                                    .unwrap_or(head)
                            }
                        } else {
                            self.resolve_signature_like_ast_ty_in_context(
                                argument,
                                TypeSyntaxContext::General,
                                tyvars,
                                mode,
                            )?
                        };
                        resolved_args.push(resolved);
                    }
                    if !matches!(mode, SignatureTyMode::ImplHead { .. }) {
                        self.validate_nominal_type_arguments(&def, &resolved_args, span, true)?;
                    }
                    match def.kind {
                        crate::env::TypeKind::Struct => Ok(Ty::Struct(
                            def.name.clone(),
                            NominalType::new(
                                resolved_args.clone(),
                                self.instantiate_type_def_fields(&def, &resolved_args),
                            ),
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
                    self.resolve_signature_like_ast_ty_in_context(ret, Self::function_type_return_context(context), tyvars, mode)?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constructor_trait(name: &str, qualified_name: &str, unique_id: u32) -> Arc<TraitInfo> {
        Arc::new(TraitInfo {
            id: ResolvedId {
                name: name.into(),
                qualified_name: Some(qualified_name.into()),
                unique_id,
                compiler_generated: false,
                symbol_info: None,
                span: Span { start: 0, end: 1 },
            },
            compiler_owned_equality: false,
            compiler_owned_failure: false,
            standard_lazy_contract: false,
            type_params: Vec::new(),
            where_clause: None,
            constructor_slots: vec!["$A".into()],
            parents: Vec::new(),
            methods: HashMap::new(),
        })
    }

    #[test]
    fn pid_annotation_rejects_an_already_bound_ordinary_type_variable() {
        let mut checker = Checker::new(TypecheckContext::default());
        let marker = checker.env.fresh_tyvar();
        let Ty::Var(variable) = marker else {
            unreachable!()
        };
        checker.local_annotation_tyvars.insert("$P".into(), marker);
        checker.substitutions.insert(variable, Ty::Int);
        let span = Span { start: 0, end: 7 };
        let annotation = AstTy::Generic(
            span.clone(),
            "PID".into(),
            vec![AstTy::Named(span, "$P".into())],
        );
        let error = checker
            .resolve_ast_ty_in_context(&annotation, TypeSyntaxContext::BindingAnnotation)
            .expect_err("an existing Int binding cannot acquire process marker identity");
        assert!(error.message.contains("process marker"));
        assert!(error.message.contains("Int"));
        assert!(!checker.process_marker_tyvars.contains(&variable));
    }

    #[test]
    fn type_relation_and_callable_context_preserve_bare_return_origin() {
        let mut checker = Checker::with_persistent_state(
            crate::test_support::session_from_cached_std_prelude().state,
            TypecheckContext::default(),
        );
        let Ty::Var(occurrence) = checker.env.fresh_tyvar() else {
            unreachable!()
        };
        let trait_key = checker
            .declaration_constructor_trait_key("Functor")
            .unwrap();
        checker
            .constructor_witness_traits
            .insert(occurrence, trait_key);
        checker
            .substitutions
            .insert(occurrence, Ty::List(Box::new(Ty::Hole)));
        let bare_return = Ty::SelfApp(vec![Ty::Hole, Ty::Var(occurrence)]);
        let expected = Ty::Func(vec![Ty::Int], Box::new(bare_return));
        let actual = Ty::Func(vec![Ty::Int], Box::new(Ty::List(Box::new(Ty::Bool))));

        assert_eq!(
            checker.contextual_callable_expected(Some(&expected)),
            Some(expected.clone())
        );
        assert!(
            checker.types_compatible(&expected, &actual).unwrap(),
            "bare return compares carrier identity without equating its mapped payload"
        );
        let ignored_input = Ty::Func(vec![Ty::Hole], Box::new(Ty::List(Box::new(Ty::Bool))));
        assert!(
            !checker.types_compatible(&expected, &ignored_input).unwrap(),
            "preserving bare return identity must not relax callable input equality"
        );
    }

    #[test]
    fn do_safebind_tail_span_uses_the_final_expression_origin() {
        let final_span = Span { start: 30, end: 42 };
        let safe_bind = TypedNode {
            ty: Ty::Unit,
            span: Span { start: 5, end: 50 },
            node: TypedInner::DoSafeBind(Box::new(TypedDoSafeBind {
                pattern: TypedPattern::Wildcard(Ty::Int),
                rhs: Box::new(TypedNode {
                    ty: Ty::Int,
                    span: Span { start: 15, end: 20 },
                    node: TypedInner::Lit(Lit::Int(1.into())),
                }),
                projection: SafeBindRhsProjection::PatternInput {
                    pattern_input_ty: Ty::Int,
                },
                failure_target: SafeBindFailureTarget::TopLevel,
                continuation: Box::new(TypedNode {
                    ty: Ty::Unit,
                    span: final_span.clone(),
                    node: TypedInner::Lit(Lit::Unit),
                }),
                origins: DoSafeBindOrigins {
                    do_span: Span { start: 5, end: 50 },
                    operator_span: Span { start: 10, end: 12 },
                    pattern_span: Span { start: 13, end: 14 },
                    rhs_span: Span { start: 15, end: 20 },
                    result_span: final_span.clone(),
                },
            })),
        };
        let checker = Checker::new(TypecheckContext::default());

        assert_eq!(checker.tail_expr_span(&safe_bind), Some(final_span));
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
    fn exact_ordinary_trait_bound_does_not_fall_back_to_same_surface_constructor_trait() {
        let mut checker = Checker::new(TypecheckContext::default());
        let mut ordinary = constructor_trait("Context", "Left::Context", 71_003);
        Arc::make_mut(&mut ordinary).constructor_slots.clear();
        checker.traits.insert("Left::Context".into(), ordinary);
        checker.traits.insert(
            "Right::Context".into(),
            constructor_trait("Context", "Right::Context", 71_004),
        );

        assert_eq!(
            checker.declaration_constructor_trait_key("Left::Context"),
            None
        );
        assert_eq!(
            checker.declaration_constructor_trait_key("Right::Context"),
            Some("Right::Context".into())
        );
    }

    #[test]
    fn direct_signature_constructor_trait_without_canonical_identity_fails_closed() {
        let mut checker = Checker::new(TypecheckContext::default());
        checker.traits.insert(
            "Global::Context".into(),
            constructor_trait("Context", "Global::Context", 71_010),
        );
        let signature_ty = sigil::resolved::ResolvedSignatureTy {
            syntax: AstTy::Generic(
                Span { start: 0, end: 8 },
                "Context".into(),
                vec![AstTy::Named(Span { start: 9, end: 12 }, "Int".into())],
            ),
            direct_constructor_trait: None,
        };

        let error = checker
            .resolve_resolved_signature_ty_in_context(
                &signature_ty,
                TypeSyntaxContext::General,
                &mut HashMap::new(),
            )
            .expect_err("Scar must not rediscover a direct signature Trait by display name");
        assert!(error.message.contains("Unknown type: Context"), "{error:?}");
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
    fn error_kind_is_a_runtime_value_type() {
        let mut checker = Checker::new(TypecheckContext::default());
        let span = Span { start: 0, end: 9 };
        let marker = AstTy::Named(span.clone(), "ErrorKind".into());
        let int = AstTy::Named(span.clone(), "Int".into());
        let mut tyvars = HashMap::new();
        assert!(checker.resolve_builtin_ast_ty(&marker, &mut tyvars).is_ok());
        for nested in [
            AstTy::Generic(span.clone(), "List".into(), vec![marker.clone()]),
            AstTy::Func(span.clone(), vec![marker.clone()], Box::new(int.clone())),
            AstTy::Func(span.clone(), vec![int.clone()], Box::new(marker.clone())),
            AstTy::Tuple(span.clone(), vec![marker.clone(), int]),
        ] {
            checker
                .resolve_builtin_ast_ty(&nested, &mut tyvars)
                .expect("nested ErrorKind is an ordinary value type");
        }
        assert!(checker
            .resolve_builtin_ast_ty_in_context(
                &marker,
                TypeSyntaxContext::FunctionReturn,
                &mut tyvars
            )
            .is_ok());
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

impl Checker {
    /// Trait type slots have exactly one input introduction channel: either
    /// explicit `::<...>` ReturnTypeArguments or value parameters. A ValueParameter slot must
    /// also flow into the return type, so explicit specialization is observable
    /// in the method contract.
    pub(super) fn validate_trait_method_input_slots(
        method: &ResolvedTraitMethodSig,
    ) -> Result<(), TypeError> {
        let mut return_type_argument_slots = HashSet::new();
        for return_type_argument in &method.return_type_arguments {
            Self::collect_trait_method_slots(
                &return_type_argument.ty,
                &mut return_type_argument_slots,
            );
        }
        let mut value_param_slots = HashSet::new();
        for param in &method.value_parameters {
            Self::collect_trait_method_slots(&param.ty, &mut value_param_slots);
        }
        if let Some(slot) = return_type_argument_slots
            .intersection(&value_param_slots)
            .next()
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Trait method {} introduces {slot} through both ReturnTypeArguments and value arguments",
                    method.id.name
                ),
                span: method.span.clone(),
                hint: Some(
                    "Introduce each type variable through exactly one input channel.".into(),
                ),
            });
        }

        let mut return_slots = HashSet::new();
        Self::collect_trait_method_slots(&method.ret_ty, &mut return_slots);
        if let Some(slot) = return_type_argument_slots.difference(&return_slots).next() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Trait method {} declares {slot} in ReturnTypeArguments but does not use it in its return type",
                    method.id.name
                ),
                span: method.span.clone(),
                hint: Some(
                    "ReturnTypeArguments type variables must appear in the return type.".into(),
                ),
            });
        }

        let mut input_slots = return_type_argument_slots;
        input_slots.extend(value_param_slots);
        if let Some(slot) = return_slots.difference(&input_slots).next() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Trait method {} has {slot} only in its return type",
                    method.id.name
                ),
                span: method.span.clone(),
                hint: Some(
                    "Introduce the type variable through ReturnTypeArguments or a value argument."
                        .into(),
                ),
            });
        }
        Ok(())
    }

    pub(super) fn collect_trait_method_slots(ty: &AstTy, used: &mut HashSet<String>) {
        match ty {
            AstTy::Named(_, name) if name == "Self" || name.starts_with('$') => {
                used.insert(name.clone());
            }
            AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => {}
            AstTy::Generic(_, name, args) => {
                if name == "Self" || name.starts_with('$') {
                    used.insert(name.clone());
                }
                for arg in args {
                    Self::collect_trait_method_slots(arg, used);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_trait_method_slots(item, used);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_trait_method_slots(param, used);
                }
                Self::collect_trait_method_slots(ret, used);
            }
        }
    }

    /// Collect the input/output slots relevant to constructor-trait methods.
    /// `Self` is a hidden constructor witness in these signatures, while `$A`
    /// names an element slot.  Ordinary named types are intentionally ignored.
    pub(super) fn collect_constructor_signature_slots(ty: &AstTy, used: &mut HashSet<String>) {
        match ty {
            AstTy::Named(_, name) => {
                if name == "Self" || name.starts_with('$') {
                    used.insert(name.clone());
                }
            }
            AstTy::ImplTrait(_, _) => {}
            AstTy::Generic(_, name, args) => {
                if name == "Self" || name.starts_with('$') {
                    used.insert(name.clone());
                }
                for arg in args {
                    Self::collect_constructor_signature_slots(arg, used);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_constructor_signature_slots(item, used);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_constructor_signature_slots(param, used);
                }
                Self::collect_constructor_signature_slots(ret, used);
            }
        }
    }
}
