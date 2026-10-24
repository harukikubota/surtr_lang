use super::*;

mod function_indices;
#[cfg(test)]
mod session_contract_tests;
#[cfg(test)]
mod specialization_state_tests;

/// Type-check the resolved AST, producing a fully typed tree.
pub fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
    typecheck_with_context(resolved, TypecheckContext::default())
}

pub fn typecheck_with_warnings(
    resolved: Vec<Resolved>,
) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
    typecheck_with_context_with_warnings(resolved, TypecheckContext::default())
}

pub fn typecheck_staged_program(
    program: sigil::ResolvedStagedProgram,
) -> Result<TypedProgram, TypeError> {
    typecheck_staged_program_with_context(program, TypecheckContext::default())
}

pub fn typecheck_staged_program_with_warnings(
    program: sigil::ResolvedStagedProgram,
) -> Result<PhaseOutput<TypedProgram>, TypeError> {
    typecheck_staged_program_with_context_with_warnings(program, TypecheckContext::default())
}

pub fn typecheck_staged_program_with_context(
    program: sigil::ResolvedStagedProgram,
    context: TypecheckContext,
) -> Result<TypedProgram, TypeError> {
    typecheck_staged_program_with_context_with_warnings(program, context).map(|output| output.value)
}

pub fn typecheck_staged_program_with_context_with_warnings(
    program: sigil::ResolvedStagedProgram,
    context: TypecheckContext,
) -> Result<PhaseOutput<TypedProgram>, TypeError> {
    let process_specs = program.process_specs().cloned().map(Into::into).collect();
    let mut checker = Checker::new(context);
    checker.boot_plan = program.boot_plan.clone();
    let nodes = checker.check_stages(program.stages)?;
    let warnings = checker.warnings.take();
    Ok(PhaseOutput::new(
        TypedProgram {
            nodes,
            enum_definitions: checker.env.typed_enum_definitions(),
            enum_error_definitions: checker.env.typed_enum_error_definitions(),
            nominal_definitions: checker.env.typed_nominal_definitions(),
            process_specs,
            boot_plan: program.boot_plan,
        },
        warnings,
    ))
}

pub fn typecheck_with_context(
    resolved: Vec<Resolved>,
    context: TypecheckContext,
) -> Result<Vec<TypedNode>, TypeError> {
    typecheck_with_context_with_warnings(resolved, context).map(|output| output.value)
}

pub fn typecheck_with_context_with_warnings(
    resolved: Vec<Resolved>,
    context: TypecheckContext,
) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
    let mut checker = Checker::new(context);
    let value = checker.check_program(resolved)?;
    Ok(PhaseOutput::new(value, checker.warnings.take()))
}

pub fn type_contains_unresolved_vars(ty: &Ty) -> bool {
    match ty {
        Ty::Var(_) => true,
        Ty::List(inner)
        | Ty::MatchResult(inner)
        | Ty::ExtractorClosure(inner)
        | Ty::Lazy(inner)
        | Ty::Pid(inner) => type_contains_unresolved_vars(inner),
        Ty::Tuple(items) | Ty::SelfApp(items) | Ty::Enum(_, items) => {
            items.iter().any(type_contains_unresolved_vars)
        }
        Ty::Func(params, ret) => {
            params.iter().any(type_contains_unresolved_vars) || type_contains_unresolved_vars(ret)
        }
        Ty::Result(source, focus) => {
            type_contains_unresolved_vars(source) || type_contains_unresolved_vars(focus)
        }
        Ty::Facet(_, source, focus, update_source, update_focus) => {
            type_contains_unresolved_vars(source)
                || type_contains_unresolved_vars(focus)
                || type_contains_unresolved_vars(update_source)
                || type_contains_unresolved_vars(update_focus)
        }
        Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
            params.iter().any(type_contains_unresolved_vars) || type_contains_unresolved_vars(ret)
        }
        Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
            nominal.arguments.iter().any(type_contains_unresolved_vars)
        }
        Ty::Int
        | Ty::Float
        | Ty::Str
        | Ty::Bool
        | Ty::Unit
        | Ty::ProcessMarker(_)
        | Ty::Hole
        | Ty::Error => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckContext {
    /// Test command scripts have a Unit result and no top-level failure receiver.
    pub test_script: bool,
    pub runtime_policy: RuntimeSourcePolicy,
    pub enforce_builtin_type_contracts: bool,
    /// REPL `:facet` inspects structurally valid paths even when a private
    /// segment is not consumable from the current source scope.
    pub allow_private_facet_inspection: bool,
}

impl Default for TypecheckContext {
    fn default() -> Self {
        Self {
            test_script: false,
            runtime_policy: RuntimeSourcePolicy::script(),
            enforce_builtin_type_contracts: false,
            allow_private_facet_inspection: false,
        }
    }
}

impl TypecheckContext {
    pub fn from_source_policy(policy: sindr::policy::SourcePolicy) -> Self {
        Self {
            test_script: false,
            runtime_policy: policy.runtime_policy,
            enforce_builtin_type_contracts: false,
            allow_private_facet_inspection: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PersistentCheckerState {
    pub(super) env: TypeEnv,
    pub(super) consts: HashMap<u32, ConstMeta>,
    pub(super) facet_bindings: HashMap<u32, StoredFacetPath>,
    pub(super) lazy_capture_bindings: HashMap<u32, LazyCaptureDiagnostic>,
    pub(super) user_func_params: HashMap<u32, Vec<String>>,
    /// Canonical signature registry shared by ordinary and builtin callables.
    pub(super) callable_signatures: CallableSignatures,
    pub(super) lazy_function_parameters: HashMap<u32, Vec<usize>>,
    pub(super) impl_method_uids: HashMap<String, u32>,
    pub(super) function_ids_by_name: HashMap<String, ResolvedId>,
    pub(super) specializable_defs: SpecializableDefinitions,
    pub(super) specialization_fun_idxs: HashMap<CallableInstantiationKey, u32>,
    pub(super) traits: TraitDefinitions,
    pub(super) trait_impls: TraitImplementations,
    pub(super) trait_impl_index_by_base_trait: TraitImplIndex,
    pub(super) trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    pub(super) tyvar_bounds: HashMap<u32, Vec<String>>,
    pub(super) error_slot_tyvars: HashSet<u32>,
    pub(super) failure_error_slot_tyvars: HashSet<u32>,
    pub(super) constructor_witness_traits: HashMap<u32, String>,
    pub(super) process_marker_tyvars: HashSet<u32>,
    pub(super) constructor_capabilities: HashMap<u32, ConstructorCapabilityProvenance>,
    pub(super) explicit_closure_parameters: HashMap<u32, Ty>,
    pub(super) signature_aliases: HashMap<String, SignatureAliasInfo>,
    pub(super) pattern_binding_aliases: HashMap<u32, Option<ResolvedId>>,
}

impl PersistentCheckerState {
    pub(super) fn new() -> Self {
        Self {
            env: initialize_env(),
            consts: HashMap::new(),
            facet_bindings: HashMap::new(),
            lazy_capture_bindings: HashMap::new(),
            user_func_params: HashMap::new(),
            callable_signatures: HashMap::new(),
            lazy_function_parameters: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
            specializable_defs: HashMap::new(),
            specialization_fun_idxs: HashMap::new(),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_impl_index_by_base_trait: HashMap::new(),
            trait_methods_by_qualified_name: HashMap::new(),
            tyvar_bounds: HashMap::new(),
            error_slot_tyvars: HashSet::new(),
            failure_error_slot_tyvars: HashSet::new(),
            constructor_witness_traits: HashMap::new(),
            process_marker_tyvars: HashSet::new(),
            constructor_capabilities: HashMap::new(),
            explicit_closure_parameters: HashMap::new(),
            signature_aliases: HashMap::new(),
            pattern_binding_aliases: HashMap::new(),
        }
    }

    pub(super) fn checkpoint(&self, process_specs: Vec<TypedProcessSpec>) -> ScarCheckpoint {
        ScarCheckpoint {
            env: self.env.clone(),
            consts: self.consts.clone(),
            facet_bindings: self.facet_bindings.clone(),
            lazy_capture_bindings: self.lazy_capture_bindings.clone(),
            user_func_params: self.user_func_params.clone(),
            callable_signatures: self.callable_signatures.clone(),
            lazy_function_parameters: self.lazy_function_parameters.clone(),
            impl_method_uids: self.impl_method_uids.clone(),
            function_ids_by_name: self.function_ids_by_name.clone(),
            specializable_defs: self.specializable_defs.clone(),
            specialization_fun_idxs: self.specialization_fun_idxs.clone(),
            traits: self.traits.clone(),
            trait_impls: self.trait_impls.clone(),
            trait_impl_index_by_base_trait: self.trait_impl_index_by_base_trait.clone(),
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name.clone(),
            tyvar_bounds: self.tyvar_bounds.clone(),
            error_slot_tyvars: self.error_slot_tyvars.clone(),
            failure_error_slot_tyvars: self.failure_error_slot_tyvars.clone(),
            constructor_witness_traits: self.constructor_witness_traits.clone(),
            process_marker_tyvars: self.process_marker_tyvars.clone(),
            constructor_capabilities: self.constructor_capabilities.clone(),
            explicit_closure_parameters: self.explicit_closure_parameters.clone(),
            signature_aliases: self.signature_aliases.clone(),
            pattern_binding_aliases: self.pattern_binding_aliases.clone(),
            process_specs,
        }
    }
}

impl From<ScarCheckpoint> for PersistentCheckerState {
    fn from(checkpoint: ScarCheckpoint) -> Self {
        Self {
            env: checkpoint.env,
            consts: checkpoint.consts,
            facet_bindings: checkpoint.facet_bindings,
            lazy_capture_bindings: checkpoint.lazy_capture_bindings,
            user_func_params: checkpoint.user_func_params,
            callable_signatures: checkpoint.callable_signatures,
            lazy_function_parameters: checkpoint.lazy_function_parameters,
            impl_method_uids: checkpoint.impl_method_uids,
            function_ids_by_name: checkpoint.function_ids_by_name,
            specializable_defs: checkpoint.specializable_defs,
            specialization_fun_idxs: checkpoint.specialization_fun_idxs,
            traits: checkpoint.traits,
            trait_impls: checkpoint.trait_impls,
            trait_impl_index_by_base_trait: checkpoint.trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name: checkpoint.trait_methods_by_qualified_name,
            tyvar_bounds: checkpoint.tyvar_bounds,
            error_slot_tyvars: checkpoint.error_slot_tyvars,
            failure_error_slot_tyvars: checkpoint.failure_error_slot_tyvars,
            constructor_witness_traits: checkpoint.constructor_witness_traits,
            process_marker_tyvars: checkpoint.process_marker_tyvars,
            constructor_capabilities: checkpoint.constructor_capabilities,
            explicit_closure_parameters: checkpoint.explicit_closure_parameters,
            signature_aliases: checkpoint.signature_aliases,
            pattern_binding_aliases: checkpoint.pattern_binding_aliases,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScarCheckpoint {
    env: TypeEnv,
    consts: HashMap<u32, ConstMeta>,
    facet_bindings: HashMap<u32, StoredFacetPath>,
    #[serde(default)]
    lazy_capture_bindings: HashMap<u32, LazyCaptureDiagnostic>,
    #[serde(default)]
    user_func_params: HashMap<u32, Vec<String>>,
    #[serde(default)]
    pub(super) callable_signatures: CallableSignatures,
    lazy_function_parameters: HashMap<u32, Vec<usize>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    specializable_defs: SpecializableDefinitions,
    #[serde(default)]
    specialization_fun_idxs: HashMap<CallableInstantiationKey, u32>,
    pub(super) traits: TraitDefinitions,
    pub(super) trait_impls: TraitImplementations,
    trait_impl_index_by_base_trait: TraitImplIndex,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
    error_slot_tyvars: HashSet<u32>,
    failure_error_slot_tyvars: HashSet<u32>,
    #[serde(default)]
    constructor_witness_traits: HashMap<u32, String>,
    process_marker_tyvars: HashSet<u32>,
    constructor_capabilities: HashMap<u32, ConstructorCapabilityProvenance>,
    explicit_closure_parameters: HashMap<u32, Ty>,
    signature_aliases: HashMap<String, SignatureAliasInfo>,
    pattern_binding_aliases: HashMap<u32, Option<ResolvedId>>,
    process_specs: Vec<TypedProcessSpec>,
}

#[derive(Debug, Clone)]
pub struct ScarSession {
    pub(super) state: PersistentCheckerState,
    process_specs: Vec<TypedProcessSpec>,
}

impl ScarSession {
    pub fn new() -> Self {
        Self {
            state: PersistentCheckerState::new(),
            process_specs: Vec::new(),
        }
    }

    pub fn typecheck(&mut self, resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.typecheck_with_context(resolved, TypecheckContext::default())
    }

    pub fn typecheck_with_warnings(
        &mut self,
        resolved: Vec<Resolved>,
    ) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
        self.typecheck_with_context_with_warnings(resolved, TypecheckContext::default())
    }

    pub fn typecheck_staged_program(
        &mut self,
        program: sigil::ResolvedStagedProgram,
    ) -> Result<TypedProgram, TypeError> {
        self.typecheck_staged_program_with_context(program, TypecheckContext::default())
    }

    pub fn typecheck_staged_program_with_warnings(
        &mut self,
        program: sigil::ResolvedStagedProgram,
    ) -> Result<PhaseOutput<TypedProgram>, TypeError> {
        self.typecheck_staged_program_with_context_with_warnings(
            program,
            TypecheckContext::default(),
        )
    }

    pub fn typecheck_staged_program_with_context(
        &mut self,
        program: sigil::ResolvedStagedProgram,
        context: TypecheckContext,
    ) -> Result<TypedProgram, TypeError> {
        self.typecheck_staged_program_with_context_with_warnings(program, context)
            .map(|output| output.value)
    }

    /// Typechecks a one-shot compile suffix without cloning the persistent
    /// session state. On error the session contains the partially checked
    /// state; incremental callers must use the transactional API above and
    /// roll back their checkpoint instead.
    pub fn typecheck_staged_program_in_place_with_context(
        &mut self,
        program: sigil::ResolvedStagedProgram,
        context: TypecheckContext,
    ) -> Result<TypedProgram, TypeError> {
        let process_specs = program.process_specs().cloned().map(Into::into).collect();
        let state = std::mem::replace(&mut self.state, PersistentCheckerState::new());
        let mut checker = Checker::with_persistent_state(state, context);
        checker.set_process_handler_dependencies(&self.process_specs);
        checker.boot_plan = program.boot_plan.clone();
        let nodes = match checker.check_stages(program.stages) {
            Ok(nodes) => nodes,
            Err(error) => {
                self.process_specs = checker.process_specs.clone();
                self.state = checker.into_persistent_state();
                return Err(error);
            }
        };
        let persisted_process_specs = checker.process_specs.clone();
        let enum_definitions = checker.env.typed_enum_definitions();
        let enum_error_definitions = checker.env.typed_enum_error_definitions();
        let nominal_definitions = checker.env.typed_nominal_definitions();
        self.state = checker.into_persistent_state();
        self.process_specs = persisted_process_specs;
        Ok(TypedProgram {
            nodes,
            enum_definitions,
            enum_error_definitions,
            nominal_definitions,
            process_specs,
            boot_plan: program.boot_plan,
        })
    }

    pub fn typecheck_staged_program_with_context_with_warnings(
        &mut self,
        program: sigil::ResolvedStagedProgram,
        context: TypecheckContext,
    ) -> Result<PhaseOutput<TypedProgram>, TypeError> {
        let process_specs = program.process_specs().cloned().map(Into::into).collect();
        let mut checker = Checker::with_persistent_state(self.state.clone(), context);
        checker.set_process_handler_dependencies(&self.process_specs);
        checker.boot_plan = program.boot_plan.clone();
        let nodes = checker.check_stages(program.stages)?;
        let persisted_process_specs = checker.process_specs.clone();
        let warnings = checker.warnings.take();
        let enum_definitions = checker.env.typed_enum_definitions();
        let enum_error_definitions = checker.env.typed_enum_error_definitions();
        let nominal_definitions = checker.env.typed_nominal_definitions();
        self.state = checker.into_persistent_state();
        self.process_specs = persisted_process_specs;
        Ok(PhaseOutput::new(
            TypedProgram {
                nodes,
                enum_definitions,
                enum_error_definitions,
                nominal_definitions,
                process_specs,
                boot_plan: program.boot_plan,
            },
            warnings,
        ))
    }

    pub fn typecheck_with_context(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<Vec<TypedNode>, TypeError> {
        self.typecheck_with_context_with_warnings(resolved, context)
            .map(|output| output.value)
    }

    /// Typechecks a one-shot compile suffix without cloning the persistent
    /// session state. On error the session contains the partially checked
    /// state; incremental callers must use the transactional API above and
    /// roll back their checkpoint instead.
    pub fn typecheck_in_place_with_context(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let state = std::mem::replace(&mut self.state, PersistentCheckerState::new());
        let mut checker = Checker::with_persistent_state(state, context);
        checker.set_process_handler_dependencies(self.process_specs.as_slice());
        let typed = match checker.check_program(resolved) {
            Ok(typed) => typed,
            Err(error) => {
                self.process_specs = checker.process_specs.clone();
                self.state = checker.into_persistent_state();
                return Err(error);
            }
        };
        let persisted_process_specs = checker.process_specs.clone();
        self.state = checker.into_persistent_state();
        self.process_specs = persisted_process_specs;
        Ok(typed)
    }

    pub fn typecheck_with_context_with_warnings(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
        let mut checker = Checker::with_persistent_state(self.state.clone(), context);
        checker.set_process_handler_dependencies(self.process_specs.as_slice());
        let typed = checker.check_program(resolved)?;
        let persisted_process_specs = checker.process_specs.clone();
        let warnings = checker.warnings.take();
        self.state = checker.into_persistent_state();
        self.process_specs = persisted_process_specs;
        Ok(PhaseOutput::new(typed, warnings))
    }

    pub fn checkpoint(&self) -> ScarCheckpoint {
        self.state.checkpoint(self.process_specs.clone())
    }

    pub fn lookup_type_def(&self, name: &str) -> Option<&crate::env::TypeDefInfo> {
        self.state.env.lookup_type_def(name)
    }

    pub fn enum_variants_of(&self, enum_name: &str) -> Option<&Vec<crate::env::EnumVariantInfo>> {
        self.state.env.enum_variants_of(enum_name)
    }

    pub fn rollback(&mut self, checkpoint: ScarCheckpoint) {
        let process_specs = checkpoint.process_specs.clone();
        self.state = checkpoint.into();
        self.process_specs = process_specs;
    }

    pub fn ensure_next_fun_idx_at_least(&mut self, next_fun_idx: u32) {
        // REPL runtime is the source of truth for currently materialized
        // function indices. Never move Scar backwards because stdlib
        // checkpoints may also reserve indices for delayed specializations.
        self.state.env.next_fun_idx = self.state.env.next_fun_idx.max(next_fun_idx);
    }
}

impl Default for ScarSession {
    fn default() -> Self {
        Self::new()
    }
}
