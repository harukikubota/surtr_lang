use super::*;

pub(super) struct Checker {
    pub(super) test_script: bool,
    pub(super) preserve_match_pattern_locations: bool,
    pub(super) active_lazy_capture: Option<ActiveLazyCapture>,
    pub(super) env: TypeEnv,
    pub(super) function_return_ty: Option<Ty>,
    pub(super) error_definition_uid: Option<u32>,
    pub(super) is_capture_closure: bool,
    pub(super) function_return_origin: Option<Span>,
    pub(super) local_annotation_tyvars: HashMap<String, Ty>,
    /// Active constructor probes retain errors produced while resolving source
    /// type syntax. These failures do not depend on a carrier candidate.
    pub(super) type_syntax_probe_error: Option<Rc<RefCell<Option<TypeError>>>>,
    /// Declaration-owned generic variables are rigid while their body is
    /// checked. Inference variables may bind to them, but they never bind to a
    /// concrete type (or to a different signature generic) at the definition
    /// site.
    pub(super) rigid_tyvars: HashSet<u32>,
    pub(super) current_function_symbol: Option<String>,
    pub(super) current_impl_struct_target: Option<String>,
    /// Private field authority comes from inherent methods or type-owned derive bodies.
    pub(super) current_private_field_owner: Option<String>,
    pub(super) callable_context: CallableContext,
    pub(super) closure_depth: usize,
    pub(super) facet_bindings: HashMap<u32, StoredFacetPath>,
    pub(super) lazy_capture_bindings: HashMap<u32, LazyCaptureDiagnostic>,
    pub(super) consts: HashMap<u32, ConstMeta>,
    pub(super) user_func_params: HashMap<u32, Vec<String>>,
    pub(super) callable_signatures: CallableSignatures,
    pub(super) lazy_function_parameters: HashMap<u32, Vec<usize>>,
    pub(super) impl_method_uids: HashMap<String, u32>,
    pub(super) function_ids_by_name: HashMap<String, ResolvedId>,
    pub(super) specializable_defs: SpecializableDefinitions,
    /// Derived anew for each specialization pass; never persisted across sessions.
    pub(super) direct_pattern_requirement_tyvars: HashMap<u32, Vec<u32>>,
    pub(super) specialization_fun_idxs: HashMap<CallableInstantiationKey, u32>,
    pub(super) substitutions: HashMap<u32, Ty>,
    pub(super) tyvar_bounds: HashMap<u32, Vec<String>>,
    pub(super) error_slot_tyvars: HashSet<u32>,
    pub(super) failure_error_slot_tyvars: HashSet<u32>,
    /// Deferred schema validation is completed after every impl head is registered.
    pub(super) pending_nominal_arguments: Option<Vec<(String, Vec<Ty>, Span, bool)>>,
    /// Obligations discovered while an inference variable is still unbound.
    /// These are deliberately separate from signature bounds: solving an
    /// expression must never strengthen a declaration-owned generic.
    pub(super) pending_trait_obligations: HashMap<u32, Vec<PendingTraitObligation>>,
    /// Bare declaration capabilities currently in scope. They become used
    /// only when checking an expression requests the matching full trait
    /// family obligation (or forwards that proof to a generic callee).
    pub(super) active_capabilities: Vec<CapabilityUse>,
    /// Static constructor-trait provenance attached to abstract bindings.
    /// These are intentionally binding-local: constructor roots may share a
    /// witness while different bindings expose different guaranteed capability
    /// sets. An empty set remains constrained and is not an ordinary concrete
    /// value.
    pub(super) constructor_capabilities: HashMap<u32, ConstructorCapabilityProvenance>,
    pub(super) explicit_closure_parameters: HashMap<u32, Ty>,
    /// Constructor-trait identity for each signature-position witness.
    pub(super) constructor_witness_traits: HashMap<u32, String>,
    pub(super) process_marker_tyvars: HashSet<u32>,
    pub(super) signature_aliases: HashMap<String, SignatureAliasInfo>,
    pub(super) pattern_binding_aliases: HashMap<u32, Option<ResolvedId>>,
    pub(super) alias_expansion_stack: Vec<String>,
    pub(super) runtime_policy: RuntimeSourcePolicy,
    pub(super) enforce_builtin_type_contracts: bool,
    pub(super) allow_private_facet_inspection: bool,
    pub(super) seen_builtin_type_decls: HashMap<String, (Vec<String>, Span)>,
    pub(super) facet_path_kind_decls: HashMap<String, Vec<String>>,
    pub(super) traits: TraitDefinitions,
    /// Derived only when canonical proof needs UID lookup; never serialized.
    pub(super) canonical_traits_by_uid: OnceLock<HashMap<u32, String>>,
    pub(super) trait_impls: TraitImplementations,
    pub(super) constructor_mapping_resolution: ConstructorMappingResolution,
    pub(super) trait_impl_index_by_base_trait: TraitImplIndex,
    pub(super) trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    pub(super) profiler: TypecheckProfiler,
    pub(super) process_handler_dependencies: HashMap<String, HashMap<String, String>>,
    pub(super) process_specs: Vec<TypedProcessSpec>,
    pub(super) boot_plan: spire::ast::SupervisorInitSpec,
    /// Exact expression-result provenance; never inferred from source text or type shape.
    pub(super) safe_operator_results: HashMap<(usize, usize), diagnostics::SourceFact>,
    pub(super) warnings: WarningBuffer,
    #[cfg(test)]
    pub(super) candidate_probe_checkpoint_count: Cell<usize>,
    #[cfg(test)]
    pub(super) trait_identity_entries_visited: Cell<usize>,
}

impl Checker {
    pub(super) fn new(context: TypecheckContext) -> Self {
        Self::with_persistent_state(PersistentCheckerState::new(), context)
    }

    pub(super) fn with_persistent_state(
        state: PersistentCheckerState,
        context: TypecheckContext,
    ) -> Self {
        Self {
            test_script: context.test_script,
            preserve_match_pattern_locations: false,
            env: state.env,
            function_return_ty: None,
            error_definition_uid: None,
            is_capture_closure: false,
            function_return_origin: None,
            local_annotation_tyvars: HashMap::new(),
            type_syntax_probe_error: None,
            rigid_tyvars: HashSet::new(),
            current_function_symbol: None,
            current_impl_struct_target: None,
            current_private_field_owner: None,
            callable_context: CallableContext::Function,
            closure_depth: 0,
            facet_bindings: state.facet_bindings,
            lazy_capture_bindings: state.lazy_capture_bindings,
            active_lazy_capture: None,
            consts: state.consts,
            user_func_params: state.user_func_params,
            callable_signatures: state.callable_signatures,
            lazy_function_parameters: state.lazy_function_parameters,
            impl_method_uids: state.impl_method_uids,
            function_ids_by_name: state.function_ids_by_name,
            specializable_defs: state.specializable_defs,
            specialization_fun_idxs: state.specialization_fun_idxs,
            direct_pattern_requirement_tyvars: HashMap::new(),
            substitutions: HashMap::new(),
            tyvar_bounds: state.tyvar_bounds,
            error_slot_tyvars: state.error_slot_tyvars,
            failure_error_slot_tyvars: state.failure_error_slot_tyvars,
            pending_nominal_arguments: None,
            pending_trait_obligations: HashMap::new(),
            active_capabilities: Vec::new(),
            constructor_capabilities: state.constructor_capabilities,
            explicit_closure_parameters: state.explicit_closure_parameters,
            constructor_witness_traits: state.constructor_witness_traits,
            process_marker_tyvars: state.process_marker_tyvars,
            signature_aliases: state.signature_aliases,
            pattern_binding_aliases: state.pattern_binding_aliases,
            alias_expansion_stack: Vec::new(),
            runtime_policy: context.runtime_policy,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            allow_private_facet_inspection: context.allow_private_facet_inspection,
            seen_builtin_type_decls: HashMap::new(),
            facet_path_kind_decls: HashMap::new(),
            traits: state.traits,
            canonical_traits_by_uid: OnceLock::new(),
            trait_impls: state.trait_impls,
            constructor_mapping_resolution: ConstructorMappingResolution::default(),
            trait_impl_index_by_base_trait: state.trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name: state.trait_methods_by_qualified_name,
            profiler: TypecheckProfiler::new_from_env(),
            process_handler_dependencies: HashMap::new(),
            process_specs: Vec::new(),
            boot_plan: spire::ast::SupervisorInitSpec::default(),
            safe_operator_results: HashMap::new(),
            warnings: WarningBuffer::default(),
            #[cfg(test)]
            candidate_probe_checkpoint_count: Cell::new(0),
            #[cfg(test)]
            trait_identity_entries_visited: Cell::new(0),
        }
    }

    pub(super) fn spawn_child_checker(&self, env: TypeEnv) -> Self {
        let profile = self.profiler.start();
        let state = self.persistent_state_with_env(env);
        let mut checker = Checker::with_persistent_state(
            state,
            TypecheckContext {
                runtime_policy: self.runtime_policy.clone(),
                enforce_builtin_type_contracts: self.enforce_builtin_type_contracts,
                allow_private_facet_inspection: self.allow_private_facet_inspection,
                test_script: self.test_script,
            },
        );
        checker.function_return_ty = self.function_return_ty.clone();
        checker.error_definition_uid = self.error_definition_uid;
        checker.is_capture_closure = self.is_capture_closure;
        checker.function_return_origin = self.function_return_origin.clone();
        checker.local_annotation_tyvars = self.local_annotation_tyvars.clone();
        checker.type_syntax_probe_error = self.type_syntax_probe_error.clone();
        checker.rigid_tyvars = self.rigid_tyvars.clone();
        checker.current_function_symbol = self.current_function_symbol.clone();
        checker.current_impl_struct_target = self.current_impl_struct_target.clone();
        checker.current_private_field_owner = self.current_private_field_owner.clone();
        checker.callable_context = self.callable_context;
        checker.closure_depth = self.closure_depth;
        checker.safe_operator_results = self.safe_operator_results.clone();
        checker.active_lazy_capture = self.active_lazy_capture.clone();
        checker.substitutions = self.substitutions.clone();
        checker.pending_trait_obligations = self.pending_trait_obligations.clone();
        checker.active_capabilities = self.active_capabilities.clone();
        checker.seen_builtin_type_decls = self.seen_builtin_type_decls.clone();
        checker.facet_path_kind_decls = self.facet_path_kind_decls.clone();
        checker.process_handler_dependencies = self.process_handler_dependencies.clone();
        checker.process_specs = self.process_specs.clone();
        checker.profiler = self.profiler.clone();
        self.profiler
            .finish(ProfileEvent::ChildCheckerSpawn, profile);
        checker
    }

    pub(super) fn persistent_state_with_env(&self, env: TypeEnv) -> PersistentCheckerState {
        PersistentCheckerState {
            env,
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
        }
    }

    pub(super) fn into_persistent_state(self) -> PersistentCheckerState {
        PersistentCheckerState {
            env: self.env,
            consts: self.consts,
            facet_bindings: self.facet_bindings,
            lazy_capture_bindings: self.lazy_capture_bindings,
            user_func_params: self.user_func_params,
            callable_signatures: self.callable_signatures,
            lazy_function_parameters: self.lazy_function_parameters,
            impl_method_uids: self.impl_method_uids,
            function_ids_by_name: self.function_ids_by_name,
            specializable_defs: self.specializable_defs,
            specialization_fun_idxs: self.specialization_fun_idxs,
            traits: self.traits,
            trait_impls: self.trait_impls,
            trait_impl_index_by_base_trait: self.trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name,
            tyvar_bounds: self.tyvar_bounds,
            error_slot_tyvars: self.error_slot_tyvars,
            failure_error_slot_tyvars: self.failure_error_slot_tyvars,
            constructor_witness_traits: self.constructor_witness_traits,
            process_marker_tyvars: self.process_marker_tyvars,
            constructor_capabilities: self.constructor_capabilities,
            explicit_closure_parameters: self.explicit_closure_parameters,
            signature_aliases: self.signature_aliases,
            pattern_binding_aliases: self.pattern_binding_aliases,
        }
    }
}
