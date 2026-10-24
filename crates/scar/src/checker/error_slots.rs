use super::*;

impl Checker {
    pub(super) fn is_error_slot_ty(&self, ty: &Ty) -> bool {
        self.is_error_value_ty(ty)
            || matches!(self.resolve_ty(ty), Ty::Var(variable) if self.error_slot_tyvars.contains(&variable))
    }

    pub(super) fn require_error_slot(&mut self, ty: &Ty, span: &Span) -> Result<(), TypeError> {
        match self.resolve_ty(ty) {
            Ty::Var(variable) => {
                self.error_slot_tyvars.insert(variable);
                Ok(())
            }
            ty if self.is_error_value_ty(&ty) => Ok(()),
            _ => Err(TypeError::new(
                "Result Error position must be Error or EnumError",
                span.clone(),
            )),
        }
    }

    pub(super) fn validate_error_slot_binding(
        &mut self,
        variable: u32,
        ty: &Ty,
    ) -> Result<bool, TypeError> {
        let ty = self.resolve_ty(ty);
        if self.error_slot_tyvars.contains(&variable) {
            if let Ty::Var(other) = &ty {
                self.error_slot_tyvars.insert(*other);
            } else if !self.is_error_value_ty(&ty) {
                return Ok(false);
            }
        }
        if self.failure_error_slot_tyvars.contains(&variable) {
            if let Ty::Var(other) = &ty {
                self.failure_error_slot_tyvars.insert(*other);
            } else if !self.error_slot_accepts_common_error(&ty) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn inherit_error_slot_variable(&mut self, source: u32, target: u32) {
        if self.error_slot_tyvars.contains(&source) {
            self.error_slot_tyvars.insert(target);
        }
        if self.failure_error_slot_tyvars.contains(&source) {
            self.failure_error_slot_tyvars.insert(target);
        }
    }

    fn error_slot_accepts_common_error(&self, ty: &Ty) -> bool {
        matches!(self.resolve_ty(ty), Ty::Error)
            || self
                .enum_error_name(ty)
                .is_some_and(|name| self.env.enum_error_other.contains(&name))
    }

    fn carrier_error_slot(&self, carrier: &Ty) -> Option<Ty> {
        match self.resolve_ty(carrier) {
            Ty::Result(_, error) => Some(*error),
            Ty::Struct(name, nominal)
                if Self::canonical_user_type_name(&name) == "Global::ResultT" =>
            {
                nominal.arguments.get(2).cloned()
            }
            _ => None,
        }
    }

    pub(super) fn monad_fail_error_slot_proof(
        &self,
        trait_name: &str,
        carrier: &Ty,
    ) -> Option<ApplicabilityProof> {
        if !self
            .traits
            .get(trait_name)
            .is_some_and(|info| info.compiler_owned_failure)
        {
            return None;
        }
        let slot = self.carrier_error_slot(carrier)?;
        Some(match self.resolve_ty(&slot) {
            Ty::Var(variable) => ApplicabilityProof::Deferred(vec![variable]),
            slot if self.error_slot_accepts_common_error(&slot) => {
                ApplicabilityProof::Satisfied(Vec::new())
            }
            _ => ApplicabilityProof::Unsatisfied,
        })
    }

    pub(super) fn validate_monad_fail_receiver(
        &mut self,
        trait_name: &str,
        carrier: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        match self.monad_fail_error_slot_proof(trait_name, carrier) {
            Some(ApplicabilityProof::Unsatisfied) => Err(self
                .closed_monad_fail_diagnostic(carrier, span)
                .expect("unsatisfied MonadFail error slot is a closed EnumError")),
            Some(ApplicabilityProof::Deferred(variables)) => {
                self.error_slot_tyvars.extend(variables.iter().copied());
                self.failure_error_slot_tyvars.extend(variables);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn validate_error_coercion_node(
        &mut self,
        node: &TypedNode,
    ) -> Result<(), TypeError> {
        let TypedInner::ErrorCoerce {
            value,
            concrete_kind,
        } = &node.node
        else {
            return Ok(());
        };
        if self.validate_aggregate_error_transport(node, value)? {
            return Ok(());
        }
        let target = self.resolve_ty(&node.ty);
        let source = self.resolve_ty(&value.ty);
        self.require_error_slot(&target, &node.span)?;
        self.require_error_slot(&source, &value.span)?;
        if target == source || matches!(target, Ty::Error) {
            return Ok(());
        }
        if let Ty::Var(variable) = target {
            if matches!(source, Ty::Error) && concrete_kind.is_none() {
                self.failure_error_slot_tyvars.insert(variable);
            }
            return Ok(());
        }
        if matches!(source, Ty::Var(_)) {
            return Ok(());
        }
        let Some(name) = self.enum_error_name(&target) else {
            return Err(TypeError::new(
                "Error conversion target must be Error or EnumError",
                node.span.clone(),
            ));
        };
        let has_other = self.env.enum_error_other.contains(&name);
        if matches!(source, Ty::Error) {
            if let Some(kind) = concrete_kind {
                if has_other
                    || self
                        .env
                        .enum_error_members
                        .get(&name)
                        .is_some_and(|members| members.contains(kind))
                {
                    return Ok(());
                }
                return Err(self.enum_error_conversion_error(
                    &name,
                    &source,
                    Some(kind),
                    &node.span,
                ));
            }
        }
        if has_other {
            return Ok(());
        }
        Err(self.enum_error_conversion_error(&name, &source, None, &node.span))
    }

    /// Resolve Error roles over declaration templates, never over instantiated fields.
    /// Each declaration parameter gains its role at most once, including recursive
    /// schemas whose concrete type arguments would grow without bound.
    pub(super) fn refresh_nominal_error_slot_roles(
        &mut self,
        span: &Span,
    ) -> Result<(), TypeError> {
        let mut definitions = self.env.type_defs.values().cloned().collect::<Vec<_>>();
        definitions.sort_by(|left, right| left.name.cmp(&right.name));
        definitions.dedup_by(|left, right| left.name == right.name);
        let mut templates = Vec::new();
        for definition in &definitions {
            if definition.state != crate::env::TypeDefState::SignatureResolved {
                return Err(self.typecheck_invariant_error(
                    "Error-position roles require resolved declaration schemas",
                    span,
                ));
            }
            for (index, (_, field)) in definition.fields.iter().enumerate() {
                templates.push((
                    field.clone(),
                    definition
                        .field_type_spans
                        .get(index)
                        .unwrap_or(span)
                        .clone(),
                ));
            }
            if matches!(definition.kind, TypeKind::Enum | TypeKind::EnumError) {
                let variants = self
                    .lookup_enum_variants_of(&definition.name)
                    .ok_or_else(|| {
                        self.typecheck_invariant_error(
                            "Error-position roles require checked enum variants",
                            span,
                        )
                    })?;
                for variant in variants {
                    for (index, payload) in variant.payload.iter().enumerate() {
                        templates.push((
                            payload.clone(),
                            variant
                                .payload_type_spans
                                .get(index)
                                .unwrap_or(span)
                                .clone(),
                        ));
                    }
                }
            }
        }
        loop {
            let previous = self.error_slot_tyvars.len();
            for (ty, source) in &templates {
                self.validate_error_positions_in_type(ty, source)?;
            }
            if self.error_slot_tyvars.len() == previous {
                return Ok(());
            }
        }
    }

    fn validate_nominal_error_arguments(
        &mut self,
        name: &str,
        arguments: &[Ty],
        span: &Span,
    ) -> Result<(), TypeError> {
        let definition = self.env.lookup_type_def(name).ok_or_else(|| {
            self.typecheck_invariant_error(
                "Error-position nominal application has no declaration",
                span,
            )
        })?;
        if definition.state != crate::env::TypeDefState::SignatureResolved {
            return Err(self.typecheck_invariant_error(
                "Error-position validation requires a resolved declaration schema",
                span,
            ));
        }
        if definition.type_param_vars.len() != arguments.len() {
            return Err(self.typecheck_invariant_error(
                "Error-position nominal application has inconsistent type argument arity",
                span,
            ));
        }
        let required = definition
            .type_param_vars
            .iter()
            .enumerate()
            .filter_map(|(index, variable)| {
                self.error_slot_tyvars.contains(variable).then_some(index)
            })
            .collect::<Vec<_>>();
        for index in required {
            // This obligation applies to the complete argument, not every variable
            // nested inside it: List<E> is not an Error type even if E is one.
            self.require_error_slot(&arguments[index], span)?;
        }
        for argument in arguments {
            self.validate_error_positions_in_type(argument, span)?;
        }
        Ok(())
    }

    pub(super) fn validate_error_positions_in_type(
        &mut self,
        ty: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        let ty = self.resolve_ty(ty);
        match &ty {
            Ty::Result(ok, error) => {
                self.require_error_slot(error, span)?;
                self.validate_error_positions_in_type(ok, span)?;
            }
            Ty::Struct(name, nominal) | Ty::Record(name, nominal) => {
                self.validate_nominal_error_arguments(name, &nominal.arguments, span)?;
                if let Some(error) = self.carrier_error_slot(&ty) {
                    self.require_error_slot(&error, span)?;
                }
            }
            Ty::Enum(name, items) => {
                use sindr::names::TypeName;
                // These primitive/opaque enum representations have no user schema
                // and no Error parameter role. Other enum owners require metadata.
                if matches!(
                    sindr::names::builtin_type_name(name),
                    Some(
                        TypeName::ErrorKind
                            | TypeName::StandbyInit
                            | TypeName::Regex
                            | TypeName::RegexCaptures
                            | TypeName::RegexMatch
                            | TypeName::RandomGenerator
                            | TypeName::FileHandle
                            | TypeName::HashMap
                            | TypeName::Generator
                            | TypeName::InfiniteGenerator
                            | TypeName::Workers
                            | TypeName::WorkerLease
                            | TypeName::TaskHandle
                    )
                ) {
                    for item in items {
                        self.validate_error_positions_in_type(item, span)?;
                    }
                } else {
                    self.validate_nominal_error_arguments(name, items, span)?;
                }
            }
            Ty::Func(arguments, result)
            | Ty::BuiltinFunc {
                params: arguments,
                ret: result,
                ..
            }
            | Ty::UserFunc {
                params: arguments,
                ret: result,
                ..
            } => {
                for argument in arguments {
                    self.validate_error_positions_in_type(argument, span)?;
                }
                self.validate_error_positions_in_type(result, span)?;
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.validate_error_positions_in_type(inner, span)?,
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                for item in items {
                    self.validate_error_positions_in_type(item, span)?;
                }
            }
            Ty::Facet(_, source, focus, target, replacement) => {
                for item in [source, focus, target, replacement] {
                    self.validate_error_positions_in_type(item, span)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) fn enum_error_conversion_error(
        &self,
        name: &str,
        source: &Ty,
        concrete_kind: Option<&str>,
        span: &Span,
    ) -> TypeError {
        self.enum_error_policy_error(
            name,
            source,
            concrete_kind,
            span,
            diagnostics::ErrorCollectionOperation::Conversion,
        )
    }

    pub(super) fn closed_monad_fail_diagnostic(
        &self,
        carrier: &Ty,
        span: &Span,
    ) -> Option<TypeError> {
        let slot = self.carrier_error_slot(carrier)?;
        let name = self.enum_error_name(&slot)?;
        if self.env.enum_error_other.contains(&name) {
            return None;
        }
        Some(self.enum_error_policy_error(
            &name,
            &Ty::Error,
            None,
            span,
            diagnostics::ErrorCollectionOperation::MonadFail,
        ))
    }

    fn enum_error_policy_error(
        &self,
        name: &str,
        source: &Ty,
        concrete_kind: Option<&str>,
        span: &Span,
        operation: diagnostics::ErrorCollectionOperation,
    ) -> TypeError {
        let Some(declaration_span) = self.env.enum_error_declaration_spans.get(name) else {
            return self.typecheck_invariant_error(
                "EnumError diagnosis requires its declaration span",
                span,
            );
        };
        let Some(members) = self.env.enum_error_members.get(name) else {
            return self.typecheck_invariant_error(
                "EnumError diagnosis requires its declared members",
                span,
            );
        };
        let target = Self::surface_name(name).to_string();
        let source_type = self.diagnostic_ty_name(&self.resolve_ty(source));
        let primary_role = match operation {
            diagnostics::ErrorCollectionOperation::Conversion => diagnostics::SourceRole::Value,
            diagnostics::ErrorCollectionOperation::MonadFail => diagnostics::SourceRole::CallTarget,
        };
        let guidance = match (operation, concrete_kind) {
            (diagnostics::ErrorCollectionOperation::Conversion, Some(kind)) => format!("Add {kind} to {target}, or add Other(Error) to accept unlisted Error kinds."),
            _ => format!("Add Other(Error) to {target} to accept a common Error; a closed collection accepts only direct or locally matched declared kinds."),
        };
        TypeError::from_structured(diagnostics::StructuredDiagnostic {
            reason: diagnostics::TypeDiagnosticReason::EnumErrorPolicyViolation.into(),
            origin: diagnostics::DiagnosticOrigin::Intrinsic,
            data: diagnostics::DiagnosticData::ErrorCollection(diagnostics::ErrorCollectionData {
                operation,
                target: target.clone(),
                members: members.clone(),
                source_type: source_type.clone(),
                concrete_kind: concrete_kind.map(str::to_string),
            }),
            primary: diagnostics::SourceFact::typed(
                primary_role,
                diagnostics::SourceId(0),
                span.clone(),
                source_type,
            ),
            related: vec![diagnostics::SourceFact::typed(
                diagnostics::SourceRole::Declaration,
                diagnostics::SourceId(0),
                declaration_span.clone(),
                target,
            )],
            remediation: Some(diagnostics::Remediation::Help { text: guidance }),
        })
    }
}
