use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

impl Checker {
    pub(super) fn bare_return_typevar_result_mismatch(
        &self,
        expected_ret: &Ty,
        actual_ret: &Ty,
        span: &Span,
    ) -> Option<TypeError> {
        match (self.resolve_ty(expected_ret), self.resolve_ty(actual_ret)) {
            (Ty::Var(_), Ty::Result(_, _)) => Some(self.type_relation_error(expected_ret, actual_ret,
                self.type_fact(SourceRole::Expected, span, expected_ret), self.type_fact(SourceRole::Value, span, actual_ret),
                TypeDiagnosticReason::ReturnTypeMismatch, DiagnosticOrigin::Return, "function", 0)
                .with_hint("A plain return type variable cannot be satisfied by Err(...). Declare Result<$T> when the function propagates failures.")),
            _ => None,
        }
    }

    pub(in crate::checker) fn resolved_capability_uses(
        &mut self,
        where_clause: Option<&ResolvedWhereClause>,
        tyvars: &HashMap<String, Ty>,
    ) -> Result<Vec<CapabilityUse>, TypeError> {
        let Some(where_clause) = where_clause else {
            return Ok(Vec::new());
        };
        let mut uses = Vec::new();
        for constraint in &where_clause.constraints {
            let subject = match &constraint.subject {
                AstTy::Named(_, name) | AstTy::Generic(_, name, _) if tyvars.contains_key(name) => {
                    tyvars.get(name).cloned().expect("checked above")
                }
                _ => continue,
            };
            let subject_ty = self.resolve_ty(&subject);
            let subject_name = Self::surface_ast_ty(&constraint.subject);
            for bound in &constraint.bounds {
                if let ResolvedWhereConstraintRhs::Trait { trait_id } = bound {
                    uses.push(CapabilityUse {
                        subject_ty: subject_ty.clone(),
                        subject_name: subject_name.clone(),
                        trait_id: self.trait_key(trait_id),
                        span: trait_id.span.clone(),
                        consumed: false,
                    });
                }
            }
        }
        Ok(uses)
    }

    pub(super) fn generated_derive_capability_uses(
        &mut self,
        where_clause: Option<&ResolvedWhereClause>,
        tyvars: &HashMap<String, Ty>,
        self_ty: &Ty,
    ) -> Result<Vec<CapabilityUse>, TypeError> {
        let Some(where_clause) = where_clause else {
            return Ok(Vec::new());
        };
        let mut uses = Vec::new();
        for constraint in &where_clause.constraints {
            let subject_ty = self.where_constraint_subject_ty(
                &constraint.subject,
                tyvars,
                Some(self_ty),
                &constraint.span,
                true,
            )?;
            let subject_name = Self::surface_ast_ty(&constraint.subject);
            for bound in &constraint.bounds {
                if let ResolvedWhereConstraintRhs::Trait { trait_id } = bound {
                    uses.push(CapabilityUse {
                        subject_ty: self.resolve_ty(&subject_ty),
                        subject_name: subject_name.clone(),
                        trait_id: self.trait_key(trait_id),
                        span: trait_id.span.clone(),
                        consumed: false,
                    });
                }
            }
        }
        Ok(uses)
    }

    fn collect_constructor_application_names(ty: &AstTy, names: &mut HashSet<String>) {
        match ty {
            AstTy::Generic(_, name, arguments) => {
                names.insert(name.clone());
                for argument in arguments {
                    Self::collect_constructor_application_names(argument, names);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_constructor_application_names(item, names);
                }
            }
            AstTy::Func(_, parameters, return_type) => {
                for parameter in parameters {
                    Self::collect_constructor_application_names(parameter, names);
                }
                Self::collect_constructor_application_names(return_type, names);
            }
            AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => {}
        }
    }

    pub(super) fn consume_signature_constructor_capabilities(
        &self,
        capabilities: &mut [CapabilityUse],
        return_type_arguments: &[ResolvedReturnTypeArgument],
        value_parameters: &[ResolvedValueParameter],
        return_type: Option<&sigil::resolved::ResolvedSignatureTy>,
    ) {
        let mut constructor_names = HashSet::new();
        for argument in return_type_arguments {
            Self::collect_constructor_application_names(
                argument.ty.syntax(),
                &mut constructor_names,
            );
        }
        for parameter in value_parameters {
            Self::collect_constructor_application_names(
                parameter.ty.syntax(),
                &mut constructor_names,
            );
        }
        if let Some(return_type) = return_type {
            Self::collect_constructor_application_names(
                return_type.syntax(),
                &mut constructor_names,
            );
        }

        for capability in capabilities {
            if constructor_names.contains(&capability.subject_name)
                && self
                    .traits
                    .get(&capability.trait_id)
                    .is_some_and(|info| !info.constructor_slots.is_empty())
            {
                capability.consumed = true;
            }
        }
    }

    pub(super) fn typed_capability_uses(
        &mut self,
        where_clause: Option<&TypedWhereClause>,
        tyvars: &HashMap<String, Ty>,
    ) -> Vec<CapabilityUse> {
        let Some(where_clause) = where_clause else {
            return Vec::new();
        };
        let mut uses = Vec::new();
        for constraint in &where_clause.constraints {
            let subject = match &constraint.subject {
                AstTy::Named(_, name) | AstTy::Generic(_, name, _) if tyvars.contains_key(name) => {
                    tyvars.get(name).cloned().expect("checked above")
                }
                _ => continue,
            };
            let subject_ty = self.resolve_ty(&subject);
            let subject_name = Self::surface_ast_ty(&constraint.subject);
            for bound in &constraint.bounds {
                if let TypedWhereConstraintRhs::Trait { trait_id } = bound {
                    uses.push(CapabilityUse {
                        subject_ty: subject_ty.clone(),
                        subject_name: subject_name.clone(),
                        trait_id: self.trait_key(trait_id),
                        span: trait_id.span.clone(),
                        consumed: false,
                    });
                }
            }
        }
        uses
    }

    pub(in crate::checker) fn seed_missing_method_type_params(
        &mut self,
        type_params: &[ResolvedTypeParam],
        tyvars: &mut HashMap<String, Ty>,
    ) {
        let missing = type_params
            .iter()
            .filter(|param| !tyvars.contains_key(&param.name))
            .cloned()
            .collect::<Vec<_>>();
        self.seed_signature_type_params(&missing, tyvars);
    }

    pub(in crate::checker) fn resolved_named_type_args(
        &self,
        type_name: &str,
        resolved_ty: &Ty,
    ) -> Option<Vec<Ty>> {
        let def = self.env.lookup_type_def(type_name)?;
        let nominal = match resolved_ty {
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal,
            _ => return None,
        };
        (nominal.arguments.len() == def.type_params.len()).then(|| nominal.arguments.clone())
    }

    pub(in crate::checker) fn collect_signature_ty_bindings(
        &self,
        ast_ty: &AstTy,
        resolved_ty: &Ty,
        bindings: &mut HashMap<String, Ty>,
    ) {
        match (ast_ty, resolved_ty) {
            (AstTy::Named(_, name), ty) if name.starts_with('$') => {
                bindings.insert(name.clone(), ty.clone());
            }
            (AstTy::Generic(_, name, args), Ty::SelfApp(items))
                if name != "Self" && Self::constructor_application_parts(items).is_some() =>
            {
                let (_, slots) = Self::constructor_application_parts(items).expect("checked above");
                for (arg, slot) in args.iter().zip(slots.iter()) {
                    self.collect_signature_ty_bindings(arg, slot, bindings);
                }
            }
            (AstTy::Generic(_, name, args), Ty::Struct(_, _) | Ty::Record(_, _)) => {
                if let Some(resolved_args) = self.resolved_named_type_args(name, resolved_ty) {
                    for (arg, resolved) in args.iter().zip(resolved_args.iter()) {
                        self.collect_signature_ty_bindings(arg, resolved, bindings);
                    }
                }
            }
            (AstTy::Generic(_, _, args), Ty::Pid(marker)) if args.len() == 1 => {
                self.collect_signature_ty_bindings(&args[0], marker, bindings);
            }
            (AstTy::Generic(_, name, args), Ty::Enum(_, resolved_args))
                if matches!(Self::surface_name(name), "Workers" | "WorkerLease")
                    && args.len() == 1 =>
            {
                if let [Ty::Pid(marker)] = resolved_args.as_slice() {
                    self.collect_signature_ty_bindings(&args[0], marker, bindings);
                }
            }
            (AstTy::Generic(_, _, args), Ty::List(inner)) if args.len() == 1 => {
                self.collect_signature_ty_bindings(&args[0], inner, bindings);
            }
            (AstTy::Generic(_, _, args), Ty::Result(ok, err)) => {
                if let Some(arg) = args.first() {
                    self.collect_signature_ty_bindings(arg, ok, bindings);
                }
                if let Some(arg) = args.get(1) {
                    self.collect_signature_ty_bindings(arg, err, bindings);
                }
            }
            (AstTy::Generic(_, _, args), Ty::Enum(_, resolved_args)) => {
                for (arg, resolved) in args.iter().zip(resolved_args.iter()) {
                    self.collect_signature_ty_bindings(arg, resolved, bindings);
                }
            }
            (AstTy::Tuple(_, items), Ty::Tuple(resolved_items)) => {
                for (item, resolved) in items.iter().zip(resolved_items.iter()) {
                    self.collect_signature_ty_bindings(item, resolved, bindings);
                }
            }
            (AstTy::Func(_, params, ret), Ty::Func(resolved_params, resolved_ret)) => {
                for (param, resolved) in params.iter().zip(resolved_params.iter()) {
                    self.collect_signature_ty_bindings(param, resolved, bindings);
                }
                self.collect_signature_ty_bindings(ret, resolved_ret, bindings);
            }
            _ => {}
        }
    }

    pub(in crate::checker) fn resolve_contextual_return_body(
        &mut self,
        return_ast: &sigil::resolved::ResolvedSignatureTy,
        expected_ret: &Ty,
        typed_body: &TypedNode,
        rigid_tyvars: &HashSet<u32>,
    ) -> Result<Option<Ty>, TypeError> {
        let Some(trait_key) = self.constructor_trait_key_for_signature_ty(return_ast) else {
            return Ok(None);
        };
        let actual = self.resolve_ty(&typed_body.ty);
        let has_concrete_constructor_shape = Self::is_concrete_constructor_shape(&actual);
        let expected_parts = match expected_ret {
            Ty::SelfApp(items) => Self::constructor_application_parts(items),
            _ => None,
        };
        let Some((witness, expected_slots)) = expected_parts else {
            unreachable!("constructor result annotation must lower to a contextual application")
        };
        if let Ty::SelfApp(actual_items) = &actual {
            if let Some((actual_witness, _)) = Self::constructor_application_parts(actual_items) {
                let same_witness = self.resolve_ty(witness) == self.resolve_ty(actual_witness);
                if same_witness
                    && self.types_compatible_with_rigid(expected_ret, &actual, rigid_tyvars)?
                {
                    // A definition generic over a direct constructor input
                    // keeps its body abstract. Pending trait calls are
                    // concretized when the ordinary function is specialized.
                    return Ok(None);
                }
            }
        }
        if matches!(actual, Ty::Var(_) | Ty::SelfApp(_)) {
            return Err(self.ambiguous_constructor_result(
                Self::surface_name(&trait_key),
                "return",
                &self.return_mismatch_span(typed_body),
            ));
        }
        if !has_concrete_constructor_shape {
            return Err(self.trait_failure(
                TypeDiagnosticReason::NoApplicableTraitImplementation,
                &trait_key,
                &actual,
                &self.return_mismatch_span(typed_body),
                DiagnosticOrigin::Return,
            ));
        }
        let concrete_slots = match self
            .constructor_application_slots_for_trait(&trait_key, &actual)
            .into_checked()
            .map_err(|error| error.at_span(&self.return_mismatch_span(typed_body)))?
        {
            ConstructorSlotsOutcome::Projected(slots) => slots,
            ConstructorSlotsOutcome::Deferred { waiting_on } => {
                let mut error = self.ambiguous_constructor_result(
                    Self::surface_name(&trait_key),
                    "return",
                    &self.return_mismatch_span(typed_body),
                );
                error.hint = Some(format!(
                    "Resolve all {} constructor input(s) before returning this value.",
                    waiting_on.len()
                ));
                return Err(error);
            }
            ConstructorSlotsOutcome::Rejected { failures } => {
                let span = self.return_mismatch_span(typed_body);
                return if Self::constructor_projection_failures_are_metadata(&failures) {
                    Err(signatures::constructor_signature_metadata_error(
                        &Self::surface_ast_ty(return_ast),
                        &span,
                        Self::constructor_projection_failure_detail(&failures),
                    ))
                } else {
                    Err(self.trait_failure(
                        TypeDiagnosticReason::NoApplicableTraitImplementation,
                        &trait_key,
                        &actual,
                        &span,
                        DiagnosticOrigin::Return,
                    ))
                };
            }
        };
        if expected_slots.len() != concrete_slots.len()
            || !try_all(
                expected_slots.iter().zip(concrete_slots.iter()),
                |(expected, actual)| {
                    self.types_compatible_with_rigid(expected, actual, rigid_tyvars)
                },
            )?
            || !self.bind_tyvar(
                match witness {
                    Ty::Var(var) => *var,
                    _ => unreachable!("fresh constructor witness must be a type variable"),
                },
                &actual,
            )?
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(expected_ret),
                    self.ty_name(&actual)
                ),
                span: self.return_mismatch_span(typed_body),
                hint: None,
            });
        }
        if self
            .trait_dispatch_target_for_args(&trait_key, "", &self.resolve_ty(&actual), &[])?
            .is_none()
        {
            return Err(TypeError::new(
                format!(
                    "{} does not implement constructor trait {}",
                    self.ty_name(&actual),
                    self.trait_display_name(&trait_key)
                ),
                self.return_mismatch_span(typed_body),
            ));
        }
        Ok(Some(actual))
    }

    pub(in crate::checker) fn check_body_in_isolated_scope(
        &mut self,
        local_bindings: &[(u32, Ty)],
        local_capabilities: &[(u32, String)],
        declaration_capabilities: &[CapabilityUse],
        deferred_capabilities: &mut [CapabilityUse],
        local_annotation_tyvars: HashMap<String, Ty>,
        rigid_tyvars: HashSet<u32>,
        function_return_ty: Ty,
        function_return_span: &Span,
        function_return_origin: Option<&Span>,
        function_symbol: String,
        impl_target: Option<String>,
        private_field_owner: Option<String>,
        in_extractor_body: bool,
        body: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let saved_function_return_ty = self.function_return_ty.clone();
        let saved_function_return_origin = self.function_return_origin.clone();
        let saved_local_annotation_tyvars = self.local_annotation_tyvars.clone();
        let saved_rigid_tyvars = self.rigid_tyvars.clone();
        let saved_current_function_symbol = self.current_function_symbol.clone();
        let saved_current_impl_struct_target = self.current_impl_struct_target.clone();
        let saved_current_private_field_owner = self.current_private_field_owner.clone();
        let saved_callable_context = self.callable_context;
        let saved_closure_depth = self.closure_depth;
        let saved_facet_bindings = self.facet_bindings.clone();
        let saved_constructor_capabilities = self.constructor_capabilities.clone();
        let saved_active_capabilities = self.active_capabilities.clone();

        self.env.push_var_scope();
        self.function_return_ty = Some(function_return_ty.clone());
        self.function_return_origin = function_return_origin.cloned();
        self.local_annotation_tyvars = local_annotation_tyvars;
        self.rigid_tyvars = rigid_tyvars;
        // Direct constructor inputs introduce witnesses outside the named
        // type-variable map. They are caller-owned just like named generics.
        for (_, ty) in local_bindings {
            let mut variables = Vec::new();
            Self::collect_ty_vars(ty, &mut variables);
            for variable in variables {
                if self.constructor_witness_traits.contains_key(&variable) {
                    self.rigid_tyvars.insert(variable);
                }
            }
        }
        self.current_function_symbol = Some(function_symbol);
        self.current_impl_struct_target = impl_target;
        self.current_private_field_owner = private_field_owner;
        self.callable_context = if in_extractor_body {
            CallableContext::Extractor
        } else {
            CallableContext::Function
        };
        self.active_capabilities = declaration_capabilities
            .iter()
            .chain(deferred_capabilities.iter())
            .cloned()
            .collect();
        for (unique_id, ty) in local_bindings {
            self.env.bind_var(*unique_id, ty.clone());
        }
        for (unique_id, capability) in local_capabilities {
            self.constructor_capabilities.insert(
                *unique_id,
                ConstructorCapabilityProvenance::constrained(capability.clone()),
            );
        }

        // Function and trait method bodies are embedded in a top-level typed
        // definition. Check the body now, but defer subtree normalization to the
        // single resolve_typed_node pass in check_program.
        let profile = self.profiler.start();
        let result =
            self.check_node_with_return_expected(body, &function_return_ty, function_return_span);
        for (deferred, checked) in deferred_capabilities.iter_mut().zip(
            self.active_capabilities
                .iter()
                .skip(declaration_capabilities.len()),
        ) {
            deferred.consumed |= checked.consumed;
        }
        let unused_capability = result.as_ref().ok().and_then(|_| {
            self.active_capabilities
                .iter()
                .take(declaration_capabilities.len())
                .find(|capability| !capability.consumed)
                .cloned()
        });
        self.profiler
            .finish(ProfileEvent::CheckBodyIsolated, profile);

        self.env.pop_var_scope();
        self.function_return_ty = saved_function_return_ty;
        self.function_return_origin = saved_function_return_origin;
        self.local_annotation_tyvars = saved_local_annotation_tyvars;
        self.rigid_tyvars = saved_rigid_tyvars;
        self.current_function_symbol = saved_current_function_symbol;
        self.current_impl_struct_target = saved_current_impl_struct_target;
        self.current_private_field_owner = saved_current_private_field_owner;
        self.callable_context = saved_callable_context;
        self.closure_depth = saved_closure_depth;
        self.facet_bindings = saved_facet_bindings;
        self.constructor_capabilities = saved_constructor_capabilities;
        self.active_capabilities = saved_active_capabilities;

        if let Some(unused) = unused_capability {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "UnusedTraitConstraint: {}: {} is never consumed",
                    unused.subject_name,
                    Self::surface_name(&unused.trait_id)
                ),
                span: unused.span,
                hint: Some(
                    "Remove this bound, or use it in a trait call or generic proof forwarding expression."
                        .into(),
                ),
            });
        }
        result
    }

    pub(in crate::checker) fn body_tail_is_return_type_argument_call(
        &self,
        body: &Resolved,
    ) -> bool {
        match body {
            Resolved::Do(..) => true,
            Resolved::Block(_, statements) => statements
                .last()
                .is_some_and(|tail| self.body_tail_is_return_type_argument_call(tail)),
            Resolved::Grouped(_, inner) => self.body_tail_is_return_type_argument_call(inner),
            Resolved::Cond(_, clauses) => clauses
                .iter()
                .any(|(_, body)| self.body_tail_is_return_type_argument_call(body)),
            Resolved::LazyApp(_, LazyCaptureKind::If, _, args) => {
                self.body_tail_is_return_type_argument_call(&args[1])
                    || self.body_tail_is_return_type_argument_call(&args[2])
            }
            Resolved::LazyApp(_, LazyCaptureKind::IfThen, _, args) => {
                self.body_tail_is_return_type_argument_call(&args[1])
            }
            Resolved::If(_, _, then_branch, else_branch) => {
                self.body_tail_is_return_type_argument_call(then_branch)
                    || else_branch
                        .as_ref()
                        .is_some_and(|branch| self.body_tail_is_return_type_argument_call(branch))
            }
            Resolved::Match(_, _, arms)
            | Resolved::IsMatch(_, _, arms)
            | Resolved::IfLet(_, _, arms, _) => arms
                .iter()
                .any(|arm| self.body_tail_is_return_type_argument_call(&arm.body)),
            Resolved::App(_, function, _) | Resolved::Capture(_, function, _) => {
                let target = match function.as_ref() {
                    Resolved::ReturnTypeArgumentApply(_, target, _) => target.as_ref(),
                    target => target,
                };
                matches!(target, Resolved::Var(_, id)
                    if self.callable_signatures
                        .get(&id.unique_id)
                        .is_some_and(|signature| !signature.return_type_arguments.is_empty()))
            }
            _ => false,
        }
    }

    pub(in crate::checker) fn check_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        return_type_arguments: &[ResolvedReturnTypeArgument],
        params: &[ResolvedValueParameter],
        ret_ty: &Option<sigil::resolved::ResolvedSignatureTy>,
        where_clause: Option<&ResolvedWhereClause>,
        body: &Resolved,
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let mut typed_params = Vec::new();
        let mut local_bindings = Vec::new();
        let mut local_capabilities = Vec::new();
        let mut tyvars = HashMap::new();
        // Where-clause variables are part of the signature namespace. Seed
        // them before resolving `$M<$A>` applications so a constructor bound
        // can register `$M` as the application witness rather than creating a
        // disconnected fresh witness for each occurrence.
        if let Some(clause) = where_clause {
            for constraint in &clause.constraints {
                if let AstTy::Named(_, name) = &constraint.subject {
                    if name.starts_with('$') {
                        tyvars
                            .entry(name.clone())
                            .or_insert_with(|| self.env.fresh_tyvar());
                    }
                }
            }
            self.apply_resolved_where_trait_bounds(Some(clause), &tyvars, None)?;
        }
        let mut direct_constructor_inputs =
            crate::checker::signatures::DirectConstructorInputs::default();
        let mut typed_return_type_arguments = Vec::new();
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
            typed_return_type_arguments.push(TypedReturnTypeArgument {
                ordinal: argument.ordinal,
                ty,
                span: argument.span.clone(),
            });
        }

        for param in params {
            let param_ty = self.resolve_def_parameter_contract(id, &param.ty, &mut tyvars)?;
            crate::checker::signatures::remember_direct_constructor_input(
                self,
                &param.ty,
                &param_ty,
                &mut direct_constructor_inputs,
            );
            let param_ty = crate::checker::signatures::coalesce_direct_constructor_inputs(
                self,
                param_ty,
                &direct_constructor_inputs,
            );
            if self.ty_contains_process_init(&param_ty) {
                return Err(TypeError {
                    structured: None,
                    message: "StandbyInit<T> is only allowed as Standby @init return type".into(),
                    span: param.id.span.clone(),
                    hint: None,
                });
            }
            if self.ty_contains_facet(&param_ty) {
                return Err(TypeError {
                    structured: None,
                    message:
                        "Facet is compile-time only in Stage1 and cannot appear in function parameter types"
                            .into(),
                    span: param.id.span.clone(),
                    hint: None,
                });
            }
            local_bindings.push((param.id.unique_id, param_ty.clone()));
            if let Some(capability) = self.constructor_trait_key_for_signature_ty(&param.ty) {
                local_capabilities.push((param.id.unique_id, capability));
            }
            typed_params.push(TypedValueParameter {
                id: param.id.clone(),
                mode: param.mode,
                ty: param_ty.clone(),
                span: param.span.clone(),
            });
        }

        let mut expected_ret = match ret_ty {
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
                &expected_ret,
                &mut direct_constructor_inputs,
            );
        }
        expected_ret = crate::checker::signatures::coalesce_direct_constructor_inputs(
            self,
            expected_ret,
            &direct_constructor_inputs,
        );
        // Reuse the predeclared input identities before checking the body.
        // Recursive and forward calls already carry substitutions keyed by
        // these identities; checking a definition must not replace them.
        if let Some(predeclared) = self.callable_signatures.get(&id.unique_id) {
            let checked_types = typed_return_type_arguments
                .iter()
                .map(|slot| slot.ty.clone())
                .chain(typed_params.iter().map(|param| param.ty.clone()))
                .chain(std::iter::once(expected_ret.clone()))
                .collect::<Vec<_>>();
            let declared_types = predeclared
                .return_type_arguments
                .iter()
                .map(|slot| slot.ty.clone())
                .chain(
                    predeclared
                        .value_parameters
                        .iter()
                        .map(|param| param.ty.clone()),
                )
                .chain(std::iter::once(predeclared.return_type.ty.clone()))
                .collect::<Vec<_>>();
            let mut checked_vars = Vec::new();
            let mut declared_vars = Vec::new();
            Self::collect_ty_vars(&Ty::Tuple(checked_types.clone()), &mut checked_vars);
            Self::collect_ty_vars(&Ty::Tuple(declared_types.clone()), &mut declared_vars);
            if checked_vars.len() != declared_vars.len() {
                return Err(
                    crate::checker::signatures::missing_canonical_callable_signature(id, span),
                );
            }
            let mapping = checked_vars
                .into_iter()
                .zip(declared_vars.into_iter().map(Ty::Var))
                .collect::<HashMap<_, _>>();
            if checked_types
                .iter()
                .map(|ty| self.substitute_ty_with_mapping(ty, &mapping))
                .ne(declared_types.iter().cloned())
            {
                return Err(
                    crate::checker::signatures::missing_canonical_callable_signature(id, span),
                );
            }
            for ty in tyvars.values_mut() {
                *ty = self.substitute_ty_with_mapping(ty, &mapping);
            }
            for (_, ty) in &mut local_bindings {
                *ty = self.substitute_ty_with_mapping(ty, &mapping);
            }
            for parameter in &mut typed_params {
                parameter.ty = self.substitute_ty_with_mapping(&parameter.ty, &mapping);
            }
            for argument in &mut typed_return_type_arguments {
                argument.ty = self.substitute_ty_with_mapping(&argument.ty, &mapping);
            }
            expected_ret = self.substitute_ty_with_mapping(&expected_ret, &mapping);
        }
        self.apply_resolved_where_trait_bounds(where_clause, &tyvars, None)?;
        let mut declaration_capabilities = self.resolved_capability_uses(where_clause, &tyvars)?;
        self.consume_signature_constructor_capabilities(
            &mut declaration_capabilities,
            return_type_arguments,
            params,
            ret_ty.as_ref(),
        );
        let signature_types = typed_return_type_arguments
            .iter()
            .map(|argument| &argument.ty)
            .chain(typed_params.iter().map(|parameter| &parameter.ty))
            .chain(std::iter::once(&expected_ret))
            .collect::<Vec<_>>();
        for capability in &mut declaration_capabilities {
            if signature_types.iter().any(|ty| {
                self.nominal_type_uses_capability(ty, &capability.subject_ty, &capability.trait_id)
            }) {
                capability.consumed = true;
            }
        }
        if self.ty_contains_facet(&expected_ret) {
            return Err(TypeError {
                structured: None,
                message:
                    "Facet is compile-time only in Stage1 and cannot appear in function return types"
                        .into(),
                span: span.clone(),
                hint: None,
            });
        }

        let current_symbol = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        if self.process_handler_return_exposes_context_pid(&current_symbol, &expected_ret) {
            return Err(TypeError {
                structured: None,
                message: "handler dependency cannot be returned from process handlers".into(),
                span: span.clone(),
                hint: Some("Keep ctx.<slot> usage inside the process handler body.".into()),
            });
        }
        let is_entrypoint = self
            .runtime_policy
            .normalized_entrypoint
            .as_deref()
            .is_some_and(|entry| entry == current_symbol);
        if is_entrypoint {
            if !params.is_empty() {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "entrypoint `{}` must have signature () -> Result<()>",
                        current_symbol
                    ),
                    span: span.clone(),
                    hint: Some("Remove entrypoint parameters and return Result<()>.".into()),
                });
            }
            if !Self::is_main_result_unit_ty(&expected_ret) {
                let legacy_main = current_symbol == "main"
                    && self
                        .runtime_policy
                        .normalized_entrypoint
                        .as_deref()
                        .is_some_and(|entry| entry == "main");
                return Err(TypeError {
                    structured: None,
                    message: if legacy_main {
                        "main must declare return type Result<()>".into()
                    } else {
                        format!(
                            "entrypoint `{}` must declare return type Result<()>",
                            current_symbol
                        )
                    },
                    span: span.clone(),
                    hint: Some(
                        "Define entrypoint as `def <name>() -> Result<()> { ... }` and return Ok(()) or Err(error)."
                            .into(),
                    ),
                });
            }
        }

        let impl_target = Self::split_impl_method_id(id).and_then(|(impl_target, _method)| {
            self.env
                .lookup_type_def(&impl_target)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
                .then_some(Self::surface_name(&impl_target).to_string())
        });
        let typed_body = self.check_body_in_isolated_scope(
            &local_bindings,
            &local_capabilities,
            &declaration_capabilities,
            &mut [],
            tyvars.clone(),
            Self::signature_tyvar_ids(&tyvars),
            expected_ret.clone(),
            ret_ty
                .as_ref()
                .map(|ty| Self::ast_ty_span(ty.syntax()))
                .unwrap_or(span),
            ret_ty.as_ref().map(|ty| Self::ast_ty_span(ty.syntax())),
            current_symbol,
            impl_target.clone(),
            impl_target,
            false,
            body,
        )?;

        if let Some(return_ast) = ret_ty.as_ref() {
            if let Some(concrete) = self.resolve_contextual_return_body(
                return_ast,
                &expected_ret,
                &typed_body,
                &Self::signature_tyvar_ids(&tyvars),
            )? {
                expected_ret = concrete;
            }
        }
        for parameter in &mut typed_params {
            parameter.ty = self.resolve_ty(&parameter.ty);
        }
        for argument in &mut typed_return_type_arguments {
            argument.ty = self.resolve_ty(&argument.ty);
        }

        let actual_ret = self.resolve_ty(&typed_body.ty);
        if let Some(err) = self.bare_return_typevar_result_mismatch(
            &expected_ret,
            &actual_ret,
            &self.return_mismatch_span(&typed_body),
        ) {
            return Err(err);
        }
        let rigid_tyvars = Self::signature_tyvar_ids(&tyvars);
        let return_constructor_coercion = ret_ty
            .as_ref()
            .and_then(|ty| self.constructor_trait_key_for_signature_ty(ty))
            .map(|trait_key| {
                self.constructor_annotation_compatible(&trait_key, &expected_ret, &typed_body.ty)
                    .map_err(|error| error.at_span(&typed_body.span))
            })
            .transpose()?
            .unwrap_or(false);
        let saved_rigid = std::mem::replace(&mut self.rigid_tyvars, rigid_tyvars.clone());
        let relation = self.assert_type_relation(
            &expected_ret,
            &typed_body.ty,
            self.type_fact(
                SourceRole::Expected,
                ret_ty
                    .as_ref()
                    .map(|ty| Self::ast_ty_span(ty.syntax()))
                    .unwrap_or(span),
                &expected_ret,
            ),
            self.type_fact(
                SourceRole::Value,
                &self.return_mismatch_span(&typed_body),
                &actual_ret,
            ),
            TypeDiagnosticReason::ReturnTypeMismatch,
            DiagnosticOrigin::Return,
            &id.name,
            0,
        );
        self.rigid_tyvars = saved_rigid;
        if let Err(mut error) = relation {
            if !return_constructor_coercion {
                if matches!(actual_ret, Ty::Unit) {
                    if let Some(hint) = self.describe_unit_return_hint(&typed_body) {
                        error = error.with_hint(hint);
                    }
                }
                return Err(error);
            }
        }

        let mut typed_body = *self
            .concretize_pending_trait_calls(typed_body)
            .map_err(|error| *error)?;
        Self::normalize_handler_captures(&mut typed_body);
        let signature_inputs = local_bindings
            .iter()
            .map(|(_, ty)| ty.clone())
            .chain(std::iter::once(expected_ret.clone()))
            .collect::<Vec<_>>();
        self.validate_pending_body_dependencies(&typed_body, &signature_inputs)?;

        let fun_idx = match self.env.lookup_var(id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => {
                return Err(TypeError {
                    structured: None,
                    message: format!("Undefined function: {}", id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        };
        let checked_params = typed_params
            .iter()
            .map(|param| self.resolve_ty(&param.ty))
            .collect::<Vec<_>>();
        let mut checked_type_params = Vec::new();
        for argument in &typed_return_type_arguments {
            Self::collect_ty_vars(&argument.ty, &mut checked_type_params);
        }
        for param in &checked_params {
            Self::collect_ty_vars(param, &mut checked_type_params);
        }
        Self::collect_ty_vars(&expected_ret, &mut checked_type_params);
        self.env.bind_var(
            id.unique_id,
            Ty::UserFunc {
                fun_idx,
                type_params: checked_type_params,
                call_substitution: Vec::new(),
                params: checked_params.clone(),
                ret: Box::new(expected_ret.clone()),
            },
        );
        let canonical_where_constraints = crate::checker::signatures::canonical_where_constraints(
            self,
            where_clause,
            &mut tyvars,
        )?;
        self.callable_signatures.insert(
            id.unique_id,
            Arc::new(crate::checker::signatures::canonical_callable_signature(
                self,
                id,
                return_type_arguments,
                params,
                typed_return_type_arguments
                    .iter()
                    .map(|argument| argument.ty.clone())
                    .collect::<Vec<_>>()
                    .as_slice(),
                checked_params.as_slice(),
                expected_ret.clone(),
                canonical_where_constraints,
                sindr::signature::RuntimeTarget::UserFunction(fun_idx),
                sindr::signature::CallableDeclarationKind::Function,
            )?),
        );
        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Def(
                fun_idx,
                id.clone(),
                typed_return_type_arguments,
                typed_params,
                expected_ret,
                where_clause.map(TypedWhereClause::from),
                Box::new(typed_body),
                attrs.visibility,
            ),
        })
    }

    pub(in crate::checker) fn check_extractor_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_params: &[ResolvedTypeParam],
        params: &[ResolvedExtractorParam],
        ret_ty: &AstTy,
        body: &Resolved,
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let mut tyvars = HashMap::new();
        self.seed_signature_type_params(type_params, &mut tyvars);

        if params.is_empty() {
            return Err(TypeError::new(
                "Extractor requires at least one input",
                span.clone(),
            ));
        }
        let mut typed_params = Vec::with_capacity(params.len());
        for param in params {
            let param_ty = match &param.ty {
                Some(ty) => self.resolve_signature_ast_ty_in_context(
                    ty,
                    TypeSyntaxContext::General,
                    &mut tyvars,
                )?,
                None => self.env.fresh_tyvar(),
            };
            if self.ty_contains_facet(&param_ty) {
                return Err(TypeError::new(
                    "Facet is compile-time only and cannot appear in extractor parameters",
                    param.id.span.clone(),
                ));
            }
            typed_params.push(TypedValueParameter {
                id: param.id.clone(),
                mode: spire::ast::ValueParameterMode::PositionalOrNamed,
                ty: param_ty,
                span: param.id.span.clone(),
            });
        }
        let target_param = params.last().expect("non-empty Extractor parameters");
        let target_span = target_param
            .ty
            .as_ref()
            .map(Self::ast_ty_span)
            .unwrap_or(&target_param.id.span);
        self.require_concrete_extractor_target_head(
            &typed_params
                .last()
                .expect("non-empty Extractor parameter types")
                .ty,
            target_span,
            &id.name,
        )?;
        let mut expected_ret = self.resolve_signature_ast_ty_in_context(
            ret_ty,
            TypeSyntaxContext::ExtractorReturn,
            &mut tyvars,
        )?;
        // Pattern heads may already refer to the predeclared signature. Keep
        // those declaration identities when checking the Extractor body.
        let Some(Ty::UserFunc {
            type_params: declared_vars,
            params: declared_params,
            ret: declared_ret,
            ..
        }) = self.env.lookup_var(id.unique_id).cloned()
        else {
            return Err(self.typecheck_invariant_error(
                "Extractor definition must have a predeclared callable signature",
                span,
            ));
        };
        let checked_types = typed_params
            .iter()
            .map(|parameter| parameter.ty.clone())
            .chain(std::iter::once(expected_ret.clone()))
            .collect::<Vec<_>>();
        let declared_types = declared_params
            .into_iter()
            .chain(std::iter::once(*declared_ret))
            .collect::<Vec<_>>();
        let mut checked_vars = Vec::new();
        for parameter in type_params {
            if let Some(ty) = tyvars.get(&parameter.name) {
                Self::collect_ty_vars(ty, &mut checked_vars);
            }
        }
        Self::collect_ty_vars(&Ty::Tuple(checked_types.clone()), &mut checked_vars);
        if checked_vars.len() != declared_vars.len() {
            return Err(self.typecheck_invariant_error(
                "Extractor signature must retain its predeclared type slots",
                span,
            ));
        }
        let mapping = checked_vars
            .into_iter()
            .zip(declared_vars.into_iter().map(Ty::Var))
            .collect::<HashMap<_, _>>();
        if checked_types
            .iter()
            .map(|ty| self.substitute_ty_with_mapping(ty, &mapping))
            .ne(declared_types)
        {
            return Err(self.typecheck_invariant_error(
                "Extractor signature must match its predeclared callable contract",
                span,
            ));
        }
        for ty in tyvars.values_mut() {
            *ty = self.substitute_ty_with_mapping(ty, &mapping);
        }
        for parameter in &mut typed_params {
            parameter.ty = self.substitute_ty_with_mapping(&parameter.ty, &mapping);
        }
        expected_ret = self.substitute_ty_with_mapping(&expected_ret, &mapping);
        let local_bindings = typed_params
            .iter()
            .map(|param| (param.id.unique_id, param.ty.clone()))
            .collect::<Vec<_>>();
        if self.ty_contains_facet(&expected_ret) {
            return Err(TypeError {
                structured: None,
                message:
                    "Facet is compile-time only in Stage1 and cannot appear in extractor return types"
                        .into(),
                span: span.clone(),
                hint: None,
            });
        }
        self.require_extractor_match_result_payload_ty(
            &expected_ret,
            span,
            &format!("Extractor {}", id.name),
        )?;

        let current_symbol = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        let impl_target = Self::split_impl_method_id(id).and_then(|(impl_target, _method)| {
            self.env
                .lookup_type_def(&impl_target)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
                .then_some(Self::surface_name(&impl_target).to_string())
        });
        let typed_body = self.check_body_in_isolated_scope(
            &local_bindings,
            &[],
            &[],
            &mut [],
            tyvars.clone(),
            Self::signature_tyvar_ids(&tyvars),
            expected_ret.clone(),
            Self::ast_ty_span(ret_ty),
            Some(Self::ast_ty_span(ret_ty)),
            current_symbol,
            impl_target.clone(),
            impl_target,
            true,
            body,
        )?;

        let rigid_tyvars = Self::signature_tyvar_ids(&tyvars);
        if !self.types_compatible_with_rigid(&expected_ret, &typed_body.ty, &rigid_tyvars)? {
            let actual_ret = self.resolve_ty(&typed_body.ty);
            let hint = if matches!(actual_ret, Ty::Unit) {
                self.describe_unit_return_hint(&typed_body)
            } else {
                None
            };
            return Err(TypeError {
                structured: None,
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(&expected_ret),
                    self.ty_name(&actual_ret)
                ),
                span: self.return_mismatch_span(&typed_body),
                hint,
            });
        }

        let typed_body = *self
            .concretize_pending_trait_calls(typed_body)
            .map_err(|error| *error)?;
        let signature_inputs = local_bindings
            .iter()
            .map(|(_, ty)| ty.clone())
            .chain(std::iter::once(expected_ret.clone()))
            .collect::<Vec<_>>();
        self.validate_pending_body_dependencies(&typed_body, &signature_inputs)?;

        let fun_idx = match self.env.lookup_var(id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => {
                return Err(TypeError {
                    structured: None,
                    message: format!("Undefined extractor: {}", id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        };

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::ExtractorDef(
                fun_idx,
                id.clone(),
                type_params
                    .iter()
                    .filter_map(|param| match tyvars.get(&param.name) {
                        Some(Ty::Var(var)) => Some(TypedTypeParam {
                            name: param.name.clone(),
                            ty_var: *var,
                            bound: param.bound.clone(),
                        }),
                        _ => None,
                    })
                    .collect(),
                typed_params
                    .into_iter()
                    .map(|param| TypedValueParameter {
                        ty: self.resolve_ty(&param.ty),
                        ..param
                    })
                    .collect(),
                self.resolve_ty(&expected_ret),
                Box::new(typed_body),
                attrs.visibility,
            ),
        })
    }

    pub(in crate::checker) fn is_main_result_unit_ty(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::Result(ok, err)
                if matches!(ok.as_ref(), Ty::Unit) && matches!(err.as_ref(), Ty::Error)
        )
    }
}
