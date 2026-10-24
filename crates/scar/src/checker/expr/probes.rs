use super::*;
use diagnostics::SourceFact;

#[derive(Clone)]
pub(in crate::checker) struct CandidateProbeCheckpoint {
    lazy_capture_bindings: HashMap<u32, LazyCaptureDiagnostic>,
    active_lazy_capture: Option<ActiveLazyCapture>,
    env: TypeEnv,
    substitutions: HashMap<u32, Ty>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
    pending_trait_obligations: HashMap<u32, Vec<PendingTraitObligation>>,
    active_capabilities: Vec<CapabilityUse>,
    constructor_capabilities: HashMap<u32, ConstructorCapabilityProvenance>,
    explicit_closure_parameters: HashMap<u32, Ty>,
    constructor_witness_traits: HashMap<u32, String>,
    process_marker_tyvars: HashSet<u32>,
    error_slot_tyvars: HashSet<u32>,
    failure_error_slot_tyvars: HashSet<u32>,
    safe_operator_results: HashMap<(usize, usize), SourceFact>,
    warnings: WarningBuffer,
    pattern_binding_aliases: HashMap<u32, Option<ResolvedId>>,
}

impl Checker {
    pub(in crate::checker) fn candidate_probe_checkpoint(&self) -> CandidateProbeCheckpoint {
        #[cfg(test)]
        self.candidate_probe_checkpoint_count
            .set(self.candidate_probe_checkpoint_count.get() + 1);
        CandidateProbeCheckpoint {
            lazy_capture_bindings: self.lazy_capture_bindings.clone(),
            active_lazy_capture: self.active_lazy_capture.clone(),
            env: self.env.clone(),
            substitutions: self.substitutions.clone(),
            tyvar_bounds: self.tyvar_bounds.clone(),
            pending_trait_obligations: self.pending_trait_obligations.clone(),
            active_capabilities: self.active_capabilities.clone(),
            constructor_capabilities: self.constructor_capabilities.clone(),
            explicit_closure_parameters: self.explicit_closure_parameters.clone(),
            constructor_witness_traits: self.constructor_witness_traits.clone(),
            process_marker_tyvars: self.process_marker_tyvars.clone(),
            error_slot_tyvars: self.error_slot_tyvars.clone(),
            failure_error_slot_tyvars: self.failure_error_slot_tyvars.clone(),
            safe_operator_results: self.safe_operator_results.clone(),
            warnings: self.warnings.clone(),
            pattern_binding_aliases: self.pattern_binding_aliases.clone(),
        }
    }

    pub(in crate::checker) fn rollback_candidate_probe(
        &mut self,
        checkpoint: CandidateProbeCheckpoint,
    ) {
        self.lazy_capture_bindings = checkpoint.lazy_capture_bindings;
        self.active_lazy_capture = checkpoint.active_lazy_capture;
        self.env = checkpoint.env;
        self.substitutions = checkpoint.substitutions;
        self.tyvar_bounds = checkpoint.tyvar_bounds;
        self.pending_trait_obligations = checkpoint.pending_trait_obligations;
        self.active_capabilities = checkpoint.active_capabilities;
        self.constructor_capabilities = checkpoint.constructor_capabilities;
        self.explicit_closure_parameters = checkpoint.explicit_closure_parameters;
        self.constructor_witness_traits = checkpoint.constructor_witness_traits;
        self.process_marker_tyvars = checkpoint.process_marker_tyvars;
        self.error_slot_tyvars = checkpoint.error_slot_tyvars;
        self.failure_error_slot_tyvars = checkpoint.failure_error_slot_tyvars;
        self.safe_operator_results = checkpoint.safe_operator_results;
        self.warnings = checkpoint.warnings;
        self.pattern_binding_aliases = checkpoint.pattern_binding_aliases;
    }
}
