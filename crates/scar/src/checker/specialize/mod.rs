use super::*;
use diagnostics::{
    DiagnosticData, DiagnosticOrigin, EnumConstructorConstraintStatus,
    EnumConstructorTypeArgumentData, Remediation, ReturnTypeArgumentData, SourceFact, SourceId,
    SourceRole, StructuredDiagnostic, TypeDiagnosticReason,
};

enum ResolvedDeferredDoFailure {
    Result(MonadFailTarget),
    Alternative(TypedNode),
}

#[derive(Clone)]
struct CallableSpecializationRequirements {
    tyvars: Vec<u32>,
    requires_concrete: bool,
}

#[derive(Default)]
struct SpecializationWork {
    #[cfg(test)]
    edge_visits: usize,
}

impl SpecializationWork {
    fn visit_edge(&mut self) {
        #[cfg(test)]
        {
            self.edge_visits += 1;
        }
    }
}

struct SpecializationContext<'a> {
    defs_by_fun_idx: &'a SpecializableDefinitions,
    bound_tyvars_by_fun_idx: &'a HashMap<u32, CallableSpecializationRequirements>,
    needs_specialization: &'a HashSet<u32>,
    specialization_fun_idxs: &'a mut HashMap<CallableInstantiationKey, u32>,
    generated_defs: &'a mut Vec<TypedNode>,
}

mod patterns;
mod rewrite;
mod substitute;
mod summary;
use super::audit::UnresolvedExecutableTypeArgument;

impl Checker {
    fn specialization_type_is_concrete(ty: &Ty) -> bool {
        let mut variables = Vec::new();
        Self::collect_ty_vars(ty, &mut variables);
        variables.is_empty()
    }

    fn resolve_deferred_do_failure(
        &mut self,
        deferred: DeferredDoFailureTarget,
    ) -> Result<ResolvedDeferredDoFailure, Box<TypeError>> {
        let target = self.resolve_pattern_failure_target(
            &deferred.carrier_ty,
            &deferred.propagated_error_tys,
            &deferred.failure_span,
            Some((
                &deferred.alternative_trait_key,
                &deferred.alternative_method_name,
                &deferred.do_keyword_span,
            )),
            false,
        )?;
        match target {
            SafeBindFailureTarget::DoMonadFail(target) => {
                Ok(ResolvedDeferredDoFailure::Result(*target))
            }
            SafeBindFailureTarget::DoAlternative { empty } => {
                Ok(ResolvedDeferredDoFailure::Alternative(*empty))
            }
            _ => unreachable!("resolved do failure context"),
        }
    }

    pub(in crate::checker) fn specialize_program(
        &mut self,
        stmts: Vec<TypedNode>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut defs_by_fun_idx = self.specializable_defs.clone();
        for stmt in &stmts {
            if let Some(fun_idx) = Self::def_fun_idx(stmt) {
                defs_by_fun_idx.insert(fun_idx, Arc::new(stmt.clone()));
            }
        }

        let mut needs_specialization = HashSet::new();
        let mut bound_tyvars_by_fun_idx = HashMap::new();
        // Method identities are unchanged while classifying this compile unit.
        // Build their membership index once instead of scanning every impl per definition.
        let trait_impl_method_uids = self
            .trait_impls
            .values()
            .flat_map(|info| {
                info.methods
                    .values()
                    .map(|method| method.function_id.unique_id)
            })
            .collect::<HashSet<_>>();
        let summaries = self.callable_summaries(&defs_by_fun_idx, &trait_impl_method_uids)?;
        let reverse = Self::reverse_callable_edges(&summaries);
        self.direct_pattern_requirement_tyvars =
            self.collect_direct_pattern_requirements(&summaries, &reverse);
        for (fun_idx, summary) in &summaries {
            let mut bound_tyvars = summary.bound_tyvars.clone();
            if let Some(required) = self.direct_pattern_requirement_tyvars.get(fun_idx) {
                for variable in required {
                    if !bound_tyvars.contains(variable) {
                        bound_tyvars.push(*variable);
                    }
                }
            }
            let requires_concrete = summary.requires_concrete
                || self.direct_pattern_requirement_tyvars.contains_key(fun_idx);
            bound_tyvars_by_fun_idx.insert(
                *fun_idx,
                CallableSpecializationRequirements {
                    tyvars: bound_tyvars,
                    requires_concrete,
                },
            );
        }
        self.propagate_callable_specialization_requirements(
            &summaries,
            &reverse,
            &mut bound_tyvars_by_fun_idx,
        )?;
        for (fun_idx, requirements) in &bound_tyvars_by_fun_idx {
            if requirements.requires_concrete || !requirements.tyvars.is_empty() {
                needs_specialization.insert(*fun_idx);
            }
        }

        let mut rewritten = Vec::new();
        let mut generated_defs = Vec::new();
        let mut specialization_fun_idxs = self.specialization_fun_idxs.clone();
        let mut unresolved_definition = None;

        for stmt in stmts {
            if let Some(fun_idx) = Self::def_fun_idx(&stmt) {
                if needs_specialization.contains(&fun_idx) {
                    if unresolved_definition.is_none() {
                        unresolved_definition = self.first_unresolved_executable_type_argument(
                            &stmt,
                            &HashSet::new(),
                            &HashSet::new(),
                        );
                    }
                    self.specializable_defs.insert(fun_idx, Arc::new(stmt));
                    continue;
                }
            }
            let rewritten_stmt = self
                .rewrite_specializations_in_node(
                    stmt,
                    &defs_by_fun_idx,
                    &bound_tyvars_by_fun_idx,
                    &needs_specialization,
                    &mut specialization_fun_idxs,
                    &mut generated_defs,
                )
                .map(|node| *node)
                .map_err(|error| *error)?;
            rewritten.push(rewritten_stmt);
        }

        rewritten.extend(generated_defs);
        if let Some((trait_name, method, subject, span)) = rewritten
            .iter()
            .find_map(|node| self.first_pending_trait_helper(node))
        {
            return Err(self.pending_trait_helper_error(trait_name, method, subject, span));
        }
        if let Some(unresolved) = unresolved_definition.or_else(|| {
            rewritten.iter().find_map(|node| {
                self.first_unresolved_executable_type_argument(
                    node,
                    &HashSet::new(),
                    &HashSet::new(),
                )
            })
        }) {
            return Err(match unresolved {
                UnresolvedExecutableTypeArgument::ReturnTypeArgument {
                    callable,
                    ordinal,
                    unresolved,
                    span,
                } => {
                    let message =
                        format!("return type arguments for `{callable}` cannot be inferred");
                    let help = format!("Provide an expected result type for `{callable}`.");
                    TypeError {
                        message,
                        span: span.clone(),
                        hint: Some(help.clone()),
                        structured: Some(StructuredDiagnostic {
                            reason: TypeDiagnosticReason::AmbiguousReturnTypeArgument.into(),
                            origin: DiagnosticOrigin::ReturnTypeArgument { ordinal },
                            data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
                                declared_origin: None,
                                value_parameter_origin: None,
                                return_origin: None,
                                left_origin: None,
                                right_origin: Some(SourceFact::typed(
                                    SourceRole::ReturnTypeArgument,
                                    SourceId(0),
                                    span.clone(),
                                    unresolved.clone(),
                                )),
                                required_trait: None,
                                expected_count: None,
                                actual_count: None,
                                callable,
                                ordinal: Some(ordinal),
                                expected_type: Some("concrete return type argument".into()),
                                actual_type: Some(unresolved.clone()),
                            }),
                            primary: SourceFact::typed(
                                SourceRole::ReturnTypeArgument,
                                SourceId(0),
                                span,
                                unresolved,
                            ),
                            related: Vec::new(),
                            remediation: Some(Remediation::Help { text: help }),
                        }),
                    }
                }
                UnresolvedExecutableTypeArgument::EnumConstructor {
                    enum_name,
                    constructor,
                    ordinal,
                    span,
                } => {
                    let message = format!(
                        "type argument {ordinal} for enum constructor `{constructor}` cannot be inferred"
                    );
                    let help = format!(
                        "Add an expected `{enum_name}<...>` type or specify every constructor type argument."
                    );
                    TypeError {
                        message,
                        span: span.clone(),
                        hint: Some(help.clone()),
                        structured: Some(StructuredDiagnostic {
                            reason: TypeDiagnosticReason::UnresolvedEnumConstructorTypeArgument
                                .into(),
                            origin: DiagnosticOrigin::EnumConstructor { ordinal },
                            data: DiagnosticData::EnumConstructorTypeArgument(
                                EnumConstructorTypeArgumentData {
                                    enum_name,
                                    constructor,
                                    ordinal,
                                    constraint_status:
                                        EnumConstructorConstraintStatus::Insufficient,
                                },
                            ),
                            primary: SourceFact::untyped(SourceRole::Value, SourceId(0), span),
                            related: Vec::new(),
                            remediation: Some(Remediation::Help { text: help }),
                        }),
                    }
                }
            });
        }
        // Generic templates are not executable function-table entries. Validate
        // after all enclosing calls have supplied their substitutions, so an
        // unresolved template reference cannot escape to Forge.
        let executable_functions = rewritten
            .iter()
            .filter_map(Self::def_fun_idx)
            .collect::<HashSet<_>>();
        for executable in &rewritten {
            if let Some(error) = self.find_typed_node(executable, &|checker, node| {
                // A definition's type describes its declaration; only value
                // references carry an executable function index.
                if matches!(
                    node.node,
                    TypedInner::Def(..) | TypedInner::ExtractorDef(..)
                ) {
                    return None;
                }
                let mut types = Self::pattern_callable_types(node);
                types.push((&node.ty, &node.span));
                for (ty, reference_span) in types {
                    let Ty::UserFunc {
                        fun_idx,
                        call_substitution,
                        ..
                    } = ty
                    else {
                        continue;
                    };
                    if !needs_specialization.contains(fun_idx)
                        || executable_functions.contains(fun_idx)
                    {
                        continue;
                    }
                    let definition = defs_by_fun_idx
                        .get(fun_idx)
                        .expect("specialization template has a definition");
                    let pending =
                        checker.find_typed_node(definition, &|_, call| match &call.node {
                            TypedInner::TraitCall {
                                trait_name,
                                method_name,
                                receiver_ty,
                                dispatch,
                                ..
                            } if matches!(dispatch, TraitDispatch::Pending)
                                || matches!(dispatch, TraitDispatch::Selected(instantiation)
                                if Self::selected_instantiation_has_pending_input(instantiation)) =>
                            {
                                Some((trait_name, method_name, receiver_ty))
                            }
                            _ => None,
                        });
                    if let Some((trait_name, method, subject)) = pending {
                        let mapping = call_substitution.iter().cloned().collect();
                        let subject = checker.substitute_ty_with_mapping(subject, &mapping);
                        return Some(checker.pending_trait_helper_error(
                            trait_name,
                            method,
                            &subject,
                            reference_span,
                        ));
                    }
                    return Some(
                        TypeError::new(
                            "Callable specialization requires concrete type arguments",
                            reference_span.clone(),
                        )
                        .with_hint("Add parameter or result type annotations at this call."),
                    );
                }
                None
            }) {
                return Err(error);
            }
        }
        self.specialization_fun_idxs = specialization_fun_idxs;
        Ok(rewritten)
    }

    fn ensure_specialized_def(
        &mut self,
        original_fun_idx: u32,
        concrete_tys: &[Ty],
        mapping: &HashMap<u32, Ty>,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<u32, TypeError> {
        let original_def = defs_by_fun_idx
            .get(&original_fun_idx)
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!(
                    "Missing generic definition for fun_idx {}",
                    original_fun_idx
                ),
                span: Span { start: 0, end: 0 },
                hint: None,
            })?;
        let key = self.specialization_key_for_def(original_def, concrete_tys, mapping)?;
        if let Some(existing) = specialization_fun_idxs.get(&key).copied() {
            let is_available = defs_by_fun_idx.contains_key(&existing)
                || generated_defs
                    .iter()
                    .any(|def| Self::def_fun_idx(def) == Some(existing));
            if is_available {
                return Ok(existing);
            }
            // Persistent sessions can retain a specialization key after the
            // corresponding generated definition was discarded with an older
            // compilation unit.  Never emit a stale function index.
            specialization_fun_idxs.remove(&key);
        }

        let specialized_fun_idx = self.env.next_fun_idx;
        self.env.next_fun_idx += 1;
        specialization_fun_idxs.insert(key, specialized_fun_idx);

        let substituted_def = self.substitute_specialized_def(
            original_def.as_ref().clone(),
            specialized_fun_idx,
            mapping,
        )?;
        // Reserve the definition before rewriting its body. A specialization
        // can refer to itself while its body is being rewritten; without the
        // reservation that recursive lookup mistakes the fresh cache entry
        // for a stale persistent-session entry and allocates a second index.
        let generated_index = generated_defs.len();
        generated_defs.push(substituted_def.clone());
        let rewritten_def = self
            .rewrite_specializations_in_node(
                substituted_def,
                defs_by_fun_idx,
                bound_tyvars_by_fun_idx,
                needs_specialization,
                specialization_fun_idxs,
                generated_defs,
            )
            .map(|node| *node)
            .map_err(|error| *error)?;
        self.specializable_defs
            .insert(specialized_fun_idx, Arc::new(rewritten_def.clone()));
        generated_defs[generated_index] = rewritten_def;
        Ok(specialized_fun_idx)
    }

    fn specialization_key_for_def(
        &self,
        def: &TypedNode,
        concrete_tys: &[Ty],
        substitution: &CanonicalSubstitution,
    ) -> Result<CallableInstantiationKey, TypeError> {
        let callable = match &def.node {
            TypedInner::Def(_, id, ..) | TypedInner::ExtractorDef(_, id, ..) => {
                Some(MethodDeclarationId(id.unique_id))
            }
            _ => None,
        }
        .ok_or_else(|| TypeError {
            structured: None,
            message: "Expected def/extractor for specialization key".into(),
            span: def.span.clone(),
            hint: None,
        })?;
        let mut implementation = None;
        let mut trait_inputs = Vec::new();
        if let Some((info, method)) = self.trait_impls.values().find_map(|info| {
            info.methods
                .values()
                .find(|method| method.function_id.unique_id == callable.0)
                .map(|method| (info, method))
        }) {
            implementation = Some(self.trait_implementation_identity(info, method)?);
            let contract = method.instantiation_contract.as_ref().ok_or_else(|| {
                self.trait_dispatch_failure(
                    TypeDiagnosticReason::UnresolvedTraitMethodInstantiation,
                    &info.trait_id.name,
                    &method.function_id.name,
                    Some(&info.target_ty),
                    &def.span,
                )
            })?;
            let constructor_bounds = self.constructor_bounds_for_contract(contract);
            for entry in contract
                .head
                .entries
                .iter()
                .chain(&contract.signature.entries)
            {
                let ty = self.instantiate_contract_canonical_type(
                    &entry.ty,
                    substitution,
                    &constructor_bounds,
                )?;
                let ty = self.normalize_executable_contract_type(&entry.ty, ty)?;
                if ty.has_pending_instantiation() {
                    return Err(self.trait_dispatch_failure(
                        TypeDiagnosticReason::UnresolvedTraitMethodInstantiation,
                        &info.trait_id.name,
                        &method.function_id.name,
                        Some(&info.target_ty),
                        &def.span,
                    ));
                }
                trait_inputs.push((entry.role, entry.ordinal, ty));
            }
            trait_inputs.sort_by_key(|(role, ordinal, _)| (*role, *ordinal));
        }
        Ok(CallableInstantiationKey {
            callable,
            implementation,
            trait_inputs,
            type_args: concrete_tys
                .iter()
                .map(|ty| self.canonical_ty_key(ty))
                .collect(),
        })
    }

    pub(in crate::checker) fn canonical_ty_key(&self, ty: &Ty) -> CanonicalTyKey {
        match self.resolve_ty(ty) {
            Ty::Int => CanonicalTyKey::Int,
            Ty::Float => CanonicalTyKey::Float,
            Ty::Str => CanonicalTyKey::String,
            Ty::Bool => CanonicalTyKey::Boolean,
            Ty::Unit => CanonicalTyKey::Unit,
            Ty::Error => CanonicalTyKey::Error,
            Ty::Hole => CanonicalTyKey::Hole,
            Ty::Var(var) => CanonicalTyKey::Var(var),
            Ty::SelfApp(args) => {
                CanonicalTyKey::SelfApp(args.iter().map(|arg| self.canonical_ty_key(arg)).collect())
            }
            Ty::ExtractorClosure(inner) => {
                CanonicalTyKey::ExtractorClosure(Box::new(self.canonical_ty_key(&inner)))
            }
            Ty::MatchResult(inner) => {
                CanonicalTyKey::MatchResult(Box::new(self.canonical_ty_key(&inner)))
            }
            Ty::List(inner) => CanonicalTyKey::List(Box::new(self.canonical_ty_key(&inner))),
            Ty::Tuple(items) => CanonicalTyKey::Tuple(
                items
                    .iter()
                    .map(|item| self.canonical_ty_key(item))
                    .collect(),
            ),
            Ty::Func(params, ret) => CanonicalTyKey::Func {
                params: params
                    .iter()
                    .map(|param| self.canonical_ty_key(param))
                    .collect(),
                ret: Box::new(self.canonical_ty_key(&ret)),
            },
            Ty::Lazy(inner) => CanonicalTyKey::Lazy(Box::new(self.canonical_ty_key(&inner))),
            Ty::Facet(kind, source, focus, update_source, update_focus) => CanonicalTyKey::Facet {
                kind,
                source: Box::new(self.canonical_ty_key(&source)),
                focus: Box::new(self.canonical_ty_key(&focus)),
                update_source: Box::new(self.canonical_ty_key(&update_source)),
                update_focus: Box::new(self.canonical_ty_key(&update_focus)),
            },
            Ty::Pid(marker) => CanonicalTyKey::Pid(Box::new(self.canonical_ty_key(&marker))),
            Ty::ProcessMarker(name) => CanonicalTyKey::ProcessMarker(name),
            Ty::BuiltinFunc { name, params, ret } => CanonicalTyKey::BuiltinFunc {
                name,
                params: params
                    .iter()
                    .map(|param| self.canonical_ty_key(param))
                    .collect(),
                ret: Box::new(self.canonical_ty_key(&ret)),
            },
            Ty::UserFunc {
                type_params,
                params,
                ret,
                ..
            } => CanonicalTyKey::UserFunc {
                type_params,
                params: params
                    .iter()
                    .map(|param| self.canonical_ty_key(param))
                    .collect(),
                ret: Box::new(self.canonical_ty_key(&ret)),
            },
            Ty::Struct(name, fields) => CanonicalTyKey::Struct {
                name: Self::canonical_specialization_name(&name),
                args: fields
                    .arguments
                    .iter()
                    .map(|argument| self.canonical_ty_key(argument))
                    .collect(),
            },
            Ty::Record(name, fields) => CanonicalTyKey::Record {
                name: Self::canonical_specialization_name(&name),
                args: fields
                    .arguments
                    .iter()
                    .map(|argument| self.canonical_ty_key(argument))
                    .collect(),
            },
            Ty::Enum(name, args) => CanonicalTyKey::Enum {
                name: Self::canonical_specialization_name(&name),
                args: args.iter().map(|arg| self.canonical_ty_key(arg)).collect(),
            },
            Ty::Result(ok, err) => CanonicalTyKey::Result {
                ok: Box::new(self.canonical_ty_key(&ok)),
                err: Box::new(self.canonical_ty_key(&err)),
            },
        }
    }

    fn canonical_specialization_name(name: &str) -> String {
        if name.starts_with('$') || name.contains("::") {
            name.to_string()
        } else {
            format!("Global::{name}")
        }
    }

    fn infer_specialization_mapping(
        &mut self,
        def: &TypedNode,
        args: &[TypedNode],
        instantiated_callable: Option<&Ty>,
        allow_omitted_args: bool,
        bound_tyvars: &[u32],
    ) -> Result<HashMap<u32, Ty>, TypeError> {
        let mut mapping = HashMap::new();
        if let Some(Ty::UserFunc {
            call_substitution, ..
        }) = instantiated_callable
        {
            if !call_substitution.is_empty() {
                for (var, ty) in call_substitution {
                    if bound_tyvars.contains(var) {
                        mapping.insert(*var, self.resolve_ty(ty));
                    }
                }
            }
        }
        let param_tys = match &def.node {
            TypedInner::Def(_, _, _, params, _, _, _, _) => params
                .iter()
                .map(|param| param.ty.clone())
                .collect::<Vec<_>>(),
            TypedInner::ExtractorDef(_, _, _, param, _, _, _) => {
                param.iter().map(|param| param.ty.clone()).collect()
            }
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!("Expected def/extractor for specialization, got {:?}", other),
                    span: def.span.clone(),
                    hint: None,
                });
            }
        };

        if param_tys.len() != args.len() && !(allow_omitted_args && args.is_empty()) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Cannot specialize call: expected {} argument(s), got {}",
                    param_tys.len(),
                    args.len()
                ),
                span: def.span.clone(),
                hint: None,
            });
        }

        if !matches!(instantiated_callable, Some(Ty::UserFunc { .. }))
            || bound_tyvars.iter().any(|var| !mapping.contains_key(var))
        {
            return Err(TypeError {
                structured: None,
                message: "Missing call substitution for ordinary specialization".into(),
                span: def.span.clone(),
                hint: None,
            });
        }
        Ok(mapping)
    }

    fn trait_specialization_arguments(
        &self,
        def: &TypedNode,
        bound_tyvars: &[u32],
        mapping: &HashMap<u32, Ty>,
    ) -> Result<Vec<Ty>, TypeError> {
        bound_tyvars
            .iter()
            .map(|var| {
                let concrete = mapping.get(var).map(|ty| self.resolve_ty(ty));
                if let Some(ty) = concrete {
                    let mut unresolved = Vec::new();
                    Self::collect_ty_vars(&ty, &mut unresolved);
                    if unresolved.is_empty() {
                        return Ok(ty);
                    }
                }
                Err(TypeError {
                    structured: None,
                    message: format!(
                        "Incomplete trait method substitution for specialization variable {}",
                        var
                    ),
                    span: def.span.clone(),
                    hint: None,
                })
            })
            .collect()
    }

    fn collect_bound_tyvars_for_def(
        &self,
        def: &TypedNode,
        trait_impl_method_uids: &HashSet<u32>,
        signature_vars: &HashSet<u32>,
        pending_variables: &[u32],
    ) -> Vec<u32> {
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        match &def.node {
            TypedInner::Def(_, id, return_type_arguments, params, ret_ty, _, _body, _) => {
                let is_trait_impl_method = trait_impl_method_uids.contains(&id.unique_id);
                for argument in return_type_arguments {
                    let mut declared = Vec::new();
                    Self::collect_ty_vars(&argument.ty, &mut declared);
                    for var in declared {
                        if seen.insert(var) {
                            ordered.push(var);
                        }
                    }
                }
                for param in params {
                    if is_trait_impl_method {
                        let mut declared = Vec::new();
                        Self::collect_ty_vars(&param.ty, &mut declared);
                        for var in declared {
                            if seen.insert(var) {
                                ordered.push(var);
                            }
                        }
                    } else {
                        self.collect_bound_tyvars_in_ty(&param.ty, &mut ordered, &mut seen);
                    }
                }
                if is_trait_impl_method {
                    let mut declared = Vec::new();
                    Self::collect_ty_vars(ret_ty, &mut declared);
                    for var in declared {
                        if seen.insert(var) {
                            ordered.push(var);
                        }
                    }
                } else {
                    self.collect_bound_tyvars_in_ty(ret_ty, &mut ordered, &mut seen);
                }
                for var in pending_variables {
                    if signature_vars.contains(var) && seen.insert(*var) {
                        ordered.push(*var);
                    }
                }
                // Function-local inference variables can carry trait bounds,
                // but callers cannot infer them from a call site. Only the
                // declared signature determines a valid specialization key.
            }
            TypedInner::ExtractorDef(_, _, type_params, param, ret_ty, _body, _) => {
                for type_param in type_params {
                    if type_param.bound.is_some() && seen.insert(type_param.ty_var) {
                        ordered.push(type_param.ty_var);
                    }
                }
                for param in param {
                    self.collect_bound_tyvars_in_ty(&param.ty, &mut ordered, &mut seen);
                }
                self.collect_bound_tyvars_in_ty(ret_ty, &mut ordered, &mut seen);
                for var in pending_variables {
                    if (signature_vars.contains(var)
                        || type_params.iter().any(|parameter| parameter.ty_var == *var))
                        && seen.insert(*var)
                    {
                        ordered.push(*var);
                    }
                }
                // See `Def` above: exclude function-local inference variables.
            }
            _ => {}
        }
        ordered
    }

    /// A generic trait instance may be pending in a definition body even when
    /// the current surface syntax cannot express that fully-instantiated
    /// bound in the enclosing signature. It still has to participate in the
    /// definition's specialization key, otherwise Forge sees a Pending call.
    fn collect_pending_trait_receiver_tyvars(node: &TypedNode, ordered: &mut Vec<u32>) {
        match &node.node {
            TypedInner::TraitCall {
                dispatch,
                receiver_ty,
                obligation,
                args,
                ..
            } => {
                let mut vars = Vec::new();
                match dispatch {
                    TraitDispatch::Pending => {
                        Self::collect_ty_vars(receiver_ty, &mut vars);
                        Self::collect_ty_vars(&obligation.receiver, &mut vars);
                        for ty in &obligation.trait_args {
                            Self::collect_ty_vars(ty, &mut vars);
                        }
                        for arg in args {
                            Self::collect_ty_vars(&arg.ty, &mut vars);
                        }
                        Self::collect_ty_vars(&node.ty, &mut vars);
                    }
                    TraitDispatch::Selected(instantiation) => {
                        Self::collect_ty_vars(receiver_ty, &mut vars);
                        for ty in instantiation
                            .substitution
                            .values()
                            .chain(instantiation.caller_substitution.values())
                        {
                            Self::collect_ty_vars(ty, &mut vars);
                        }
                        let signature = &instantiation.callable_signature;
                        for ty in signature
                            .return_type_arguments
                            .iter()
                            .map(|argument| &argument.ty)
                            .chain(
                                signature
                                    .value_parameters
                                    .iter()
                                    .map(|parameter| &parameter.ty),
                            )
                            .chain(std::iter::once(&signature.return_type.ty))
                            .chain(
                                signature
                                    .where_constraints
                                    .constraints
                                    .iter()
                                    .map(|constraint| &constraint.subject),
                            )
                        {
                            Self::collect_canonical_tyvars(ty, &mut vars);
                        }
                    }
                    TraitDispatch::Static(_) => {}
                }
                for var in vars {
                    if !ordered.contains(&var) {
                        ordered.push(var);
                    }
                }
            }
            TypedInner::DoSafeBind(control) => {
                if let SafeBindFailureTarget::Deferred(deferred) = &control.failure_target {
                    Self::collect_ty_vars(&deferred.carrier_ty, ordered);
                    for ty in &deferred.propagated_error_tys {
                        Self::collect_ty_vars(ty, ordered);
                    }
                }
            }
            _ => {}
        }
    }

    fn collect_canonical_tyvars(ty: &CanonicalTy, out: &mut Vec<u32>) {
        if let CanonicalTypeHead::Variable(var) = ty.head {
            if !out.contains(&var) {
                out.push(var);
            }
        }
        for argument in &ty.arguments {
            Self::collect_canonical_tyvars(argument, out);
        }
    }

    #[allow(dead_code)]
    fn collect_bound_tyvars_in_node(
        &self,
        node: &TypedNode,
        ordered: &mut Vec<u32>,
        seen: &mut HashSet<u32>,
    ) {
        if let Some(call) = node.monad_fail_call() {
            self.collect_bound_tyvars_in_node(call, ordered, seen);
        }
        for expr in Self::pattern_expression_nodes(node) {
            self.collect_bound_tyvars_in_node(expr, ordered, seen);
        }
        self.collect_bound_tyvars_in_ty(&node.ty, ordered, seen);
        match &node.node {
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.collect_bound_tyvars_in_node(func, ordered, seen);
                for arg in args {
                    self.collect_bound_tyvars_in_node(arg, ordered, seen);
                }
            }
            TypedInner::TraitCall { args, .. } => {
                for arg in args {
                    self.collect_bound_tyvars_in_node(arg, ordered, seen);
                }
            }
            TypedInner::Block(stmts) => {
                for stmt in stmts {
                    self.collect_bound_tyvars_in_node(stmt, ordered, seen);
                }
            }
            TypedInner::Bind(_, rhs)
            | TypedInner::ApplyPattern { value: rhs, .. }
            | TypedInner::SafeBind(_, rhs, _, _)
            | TypedInner::Semi(rhs) => self.collect_bound_tyvars_in_node(rhs, ordered, seen),
            TypedInner::DoSafeBind(control) => {
                self.collect_bound_tyvars_in_node(&control.rhs, ordered, seen);
                if let SafeBindFailureTarget::DoAlternative { empty } = &control.failure_target {
                    self.collect_bound_tyvars_in_node(empty, ordered, seen);
                }
                self.collect_bound_tyvars_in_node(&control.continuation, ordered, seen);
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right) => {
                self.collect_bound_tyvars_in_node(left, ordered, seen);
                self.collect_bound_tyvars_in_node(right, ordered, seen);
            }
            TypedInner::ListCons(head, tail) => {
                self.collect_bound_tyvars_in_node(head, ordered, seen);
                self.collect_bound_tyvars_in_node(tail, ordered, seen);
            }
            TypedInner::ErrorConstruct {
                message, payload, ..
            } => {
                self.collect_bound_tyvars_in_node(message, ordered, seen);
                for value in payload {
                    self.collect_bound_tyvars_in_node(value, ordered, seen);
                }
            }
            TypedInner::ListLiteral(items) | TypedInner::TupleLiteral(items) => {
                for item in items {
                    self.collect_bound_tyvars_in_node(item, ordered, seen);
                }
            }
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    self.collect_bound_tyvars_in_node(key, ordered, seen);
                    self.collect_bound_tyvars_in_node(value, ordered, seen);
                }
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        self.collect_bound_tyvars_in_node(expr, ordered, seen);
                    }
                }
            }
            TypedInner::Dbg(args) => {
                for arg in args {
                    self.collect_bound_tyvars_in_node(&arg.expr, ordered, seen);
                }
            }
            TypedInner::EagerBoundary(inner) => {
                self.collect_bound_tyvars_in_node(inner, ordered, seen)
            }
            TypedInner::If(cond, then_branch, else_branch) => {
                self.collect_bound_tyvars_in_node(cond, ordered, seen);
                self.collect_bound_tyvars_in_node(then_branch, ordered, seen);
                if let Some(else_branch) = else_branch {
                    self.collect_bound_tyvars_in_node(else_branch, ordered, seen);
                }
            }
            TypedInner::Cause(value, err) => {
                self.collect_bound_tyvars_in_node(value, ordered, seen);
                self.collect_bound_tyvars_in_node(err, ordered, seen);
            }
            TypedInner::Match(scrutinee, arms) => {
                self.collect_bound_tyvars_in_node(scrutinee, ordered, seen);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_bound_tyvars_in_node(guard, ordered, seen);
                    }
                    self.collect_bound_tyvars_in_node(&arm.body, ordered, seen);
                }
            }
            TypedInner::FieldAccess(expr, _) => {
                self.collect_bound_tyvars_in_node(expr, ordered, seen);
            }
            TypedInner::ProcessContextHandler { .. } => {}
            TypedInner::SupervisorSpawn { init, .. } => {
                self.collect_bound_tyvars_in_node(init, ordered, seen);
            }
            TypedInner::SupervisorAdopt { pid, .. } => {
                self.collect_bound_tyvars_in_node(pid, ordered, seen);
            }
            TypedInner::SupervisorStatus { .. } => {}
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                self.collect_bound_tyvars_in_node(init, ordered, seen);
                self.collect_bound_tyvars_in_node(strategy, ordered, seen);
            }
            TypedInner::FacetPath(path) => {
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
                for ty in &path.segment_focus_tys {
                    self.collect_bound_tyvars_in_ty(ty, ordered, seen);
                }
            }
            TypedInner::PendingFacetPath(path) => {
                if let Some(source_ty_hint) = &path.source_ty_hint {
                    self.collect_bound_tyvars_in_ty(source_ty_hint, ordered, seen);
                }
            }
            TypedInner::FacetView { source, path, .. } => {
                self.collect_bound_tyvars_in_node(source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
                for ty in &path.segment_focus_tys {
                    self.collect_bound_tyvars_in_ty(ty, ordered, seen);
                }
            }
            TypedInner::FacetSet {
                source,
                path,
                value,
                ..
            } => {
                self.collect_bound_tyvars_in_node(source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
                for ty in &path.segment_focus_tys {
                    self.collect_bound_tyvars_in_ty(ty, ordered, seen);
                }
                self.collect_bound_tyvars_in_node(value, ordered, seen);
            }
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                ..
            } => {
                self.collect_bound_tyvars_in_node(source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.source_ty, ordered, seen);
                self.collect_bound_tyvars_in_ty(&path.focus_ty, ordered, seen);
                for ty in &path.segment_focus_tys {
                    self.collect_bound_tyvars_in_ty(ty, ordered, seen);
                }
                self.collect_bound_tyvars_in_node(update_fun, ordered, seen);
            }
            TypedInner::StructLit(_, fields) | TypedInner::ConstructorCall(_, fields) => {
                for field in fields {
                    self.collect_bound_tyvars_in_node(field, ordered, seen);
                }
            }
            TypedInner::DeferrorDef(_, _, _, _, show) => {
                self.collect_bound_tyvars_in_node(show, ordered, seen);
            }
            TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body)
            | TypedInner::ExtractorClosure(_, _, body)
            | TypedInner::CaptureClosure(_, _, body) => {
                self.collect_bound_tyvars_in_node(body, ordered, seen);
            }
            TypedInner::CaptureConstructorClosure(_, _, _, body) => {
                self.collect_bound_tyvars_in_node(body, ordered, seen);
            }
            TypedInner::Lit(_)
            | TypedInner::ErrorKind(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..) => {}
        }
    }

    fn collect_bound_tyvars_in_ty(&self, ty: &Ty, ordered: &mut Vec<u32>, seen: &mut HashSet<u32>) {
        match self.resolve_ty(ty) {
            Ty::Var(var) => {
                if !self.tyvar_bound_names(var).is_empty() && seen.insert(var) {
                    ordered.push(var);
                }
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.collect_bound_tyvars_in_ty(&inner, ordered, seen),
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.collect_bound_tyvars_in_ty(&source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&focus, ordered, seen);
                self.collect_bound_tyvars_in_ty(&update_source, ordered, seen);
                self.collect_bound_tyvars_in_ty(&update_focus, ordered, seen);
            }
            Ty::Tuple(items) => {
                for item in items {
                    self.collect_bound_tyvars_in_ty(&item, ordered, seen);
                }
            }
            Ty::SelfApp(items) => {
                if let Some((witness, slots)) = Self::constructor_application_parts(&items) {
                    if let Ty::Var(var) = self.resolve_ty(witness) {
                        if seen.insert(var) {
                            ordered.push(var);
                        }
                    }
                    for slot in slots {
                        self.collect_bound_tyvars_in_ty(slot, ordered, seen);
                    }
                } else {
                    for item in items {
                        self.collect_bound_tyvars_in_ty(&item, ordered, seen);
                    }
                }
            }
            Ty::Func(params, ret) => {
                for param in params {
                    self.collect_bound_tyvars_in_ty(&param, ordered, seen);
                }
                self.collect_bound_tyvars_in_ty(&ret, ordered, seen);
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                for param in params {
                    self.collect_bound_tyvars_in_ty(&param, ordered, seen);
                }
                self.collect_bound_tyvars_in_ty(&ret, ordered, seen);
            }
            Ty::Result(ok, err) => {
                self.collect_bound_tyvars_in_ty(&ok, ordered, seen);
                self.collect_bound_tyvars_in_ty(&err, ordered, seen);
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for argument in &nominal.arguments {
                    self.collect_bound_tyvars_in_ty(argument, ordered, seen);
                }
            }
            Ty::Enum(_, args) => {
                for arg in args {
                    self.collect_bound_tyvars_in_ty(&arg, ordered, seen);
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

    fn def_fun_idx(node: &TypedNode) -> Option<u32> {
        match &node.node {
            TypedInner::Def(fun_idx, ..) | TypedInner::ExtractorDef(fun_idx, ..) => Some(*fun_idx),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spire::ast::Visibility;

    fn test_span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn resolved_id(name: &str, qualified_name: Option<&str>, unique_id: u32) -> ResolvedId {
        ResolvedId {
            name: name.to_string(),
            qualified_name: qualified_name.map(str::to_string),
            symbol_info: None,
            unique_id,
            compiler_generated: false,
            span: test_span(),
        }
    }

    fn generic_identity_ty(fun_idx: u32, ty_var: u32) -> Ty {
        Ty::UserFunc {
            fun_idx,
            type_params: vec![ty_var],
            call_substitution: Vec::new(),
            params: vec![Ty::Var(ty_var)],
            ret: Box::new(Ty::Var(ty_var)),
        }
    }

    fn generic_identity_def(
        fun_idx: u32,
        id: ResolvedId,
        param_id: ResolvedId,
        ty_var: u32,
    ) -> TypedNode {
        TypedNode {
            ty: generic_identity_ty(fun_idx, ty_var),
            span: test_span(),
            node: TypedInner::Def(
                fun_idx,
                id,
                vec![TypedReturnTypeArgument {
                    ordinal: 0,
                    ty: Ty::Var(ty_var),
                    span: test_span(),
                }],
                vec![TypedValueParameter {
                    id: param_id.clone(),
                    mode: spire::ast::ValueParameterMode::PositionalOrNamed,
                    ty: Ty::Var(ty_var),
                    span: test_span(),
                }],
                Ty::Var(ty_var),
                None,
                Box::new(TypedNode {
                    ty: Ty::Var(ty_var),
                    span: test_span(),
                    node: TypedInner::Var(param_id),
                }),
                Visibility::Public,
            ),
        }
    }

    fn typed_arg(unique_id: u32, ty: Ty) -> TypedNode {
        TypedNode {
            ty,
            span: test_span(),
            node: TypedInner::Var(resolved_id("arg", None, unique_id)),
        }
    }

    fn call_generic(id: ResolvedId, fun_idx: u32, ty_var: u32, arg: TypedNode) -> TypedNode {
        let ret_ty = arg.ty.clone();
        let mut callable_ty = generic_identity_ty(fun_idx, ty_var);
        if let Ty::UserFunc {
            call_substitution, ..
        } = &mut callable_ty
        {
            *call_substitution = vec![(ty_var, ret_ty.clone())];
        }
        TypedNode {
            ty: ret_ty,
            span: test_span(),
            node: TypedInner::App(
                Box::new(TypedNode {
                    ty: callable_ty,
                    span: test_span(),
                    node: TypedInner::Var(id),
                }),
                vec![arg],
            ),
        }
    }

    fn generated_def_fun_idxs(nodes: &[TypedNode], qualified_name: &str) -> Vec<u32> {
        nodes
            .iter()
            .filter_map(|node| match &node.node {
                TypedInner::Def(fun_idx, id, ..)
                    if id.qualified_name.as_deref() == Some(qualified_name) =>
                {
                    Some(*fun_idx)
                }
                _ => None,
            })
            .collect()
    }

    fn app_fun_idxs(nodes: &[TypedNode]) -> Vec<u32> {
        nodes
            .iter()
            .filter_map(|node| match &node.node {
                TypedInner::App(func, _) => match &func.ty {
                    Ty::UserFunc { fun_idx, .. } => Some(*fun_idx),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn callable_requirement_chain_visits_edges_linearly_in_both_orders() {
        for depth in [16, 32, 64] {
            for reverse in [false, true] {
                let checker = Checker::new(TypecheckContext::default());
                let mut definitions = HashMap::new();
                let mut requirements = HashMap::new();
                for level in 0..depth {
                    let index = if reverse { depth - level } else { level + 1 };
                    let variable = index + 100;
                    let id = resolved_id("relay", None, index);
                    let mut definition = generic_identity_def(
                        index,
                        id,
                        resolved_id("value", None, index + 200),
                        variable,
                    );
                    if level + 1 < depth {
                        let callee = if reverse { index - 1 } else { index + 1 };
                        let TypedInner::Def(_, _, _, _, _, _, body, _) = &mut definition.node
                        else {
                            unreachable!()
                        };
                        *body = Box::new(call_generic(
                            resolved_id("relay", None, callee),
                            callee,
                            callee + 100,
                            typed_arg(index + 200, Ty::Var(variable)),
                        ));
                    }
                    definitions.insert(index, Arc::new(definition));
                    requirements.insert(
                        index,
                        CallableSpecializationRequirements {
                            tyvars: if level + 1 == depth {
                                vec![variable]
                            } else {
                                Vec::new()
                            },
                            requires_concrete: level + 1 == depth,
                        },
                    );
                }
                let summaries = checker
                    .callable_summaries(&definitions, &HashSet::new())
                    .unwrap();
                let reverse_calls = Checker::reverse_callable_edges(&summaries);
                let work = checker
                    .propagate_callable_specialization_requirements(
                        &summaries,
                        &reverse_calls,
                        &mut requirements,
                    )
                    .unwrap();
                for (index, required) in &requirements {
                    assert_eq!(required.tyvars, vec![index + 100]);
                    assert!(required.requires_concrete);
                }
                eprintln!(
                    "P2a depth={depth} reverse={reverse} callable_edge_visits={}",
                    work.edge_visits
                );
                assert!(
                    work.edge_visits <= 2 * (depth as usize - 1),
                    "requirements must visit the dependency chain linearly: {}",
                    work.edge_visits
                );
            }
        }
    }

    fn forwarding_def(fun_idx: u32, variable: u32, callee: u32, slot: u32, input: Ty) -> TypedNode {
        let mut definition = generic_identity_def(
            fun_idx,
            resolved_id("relay", None, fun_idx),
            resolved_id("value", None, fun_idx + 1000),
            variable,
        );
        let TypedInner::Def(_, _, _, _, _, _, body, _) = &mut definition.node else {
            unreachable!()
        };
        *body = Box::new(call_generic(
            resolved_id("callee", None, callee),
            callee,
            slot,
            typed_arg(fun_idx + 1000, input),
        ));
        definition
    }

    #[test]
    fn summary_keeps_direct_pattern_and_ordinary_forwarding_distinct() {
        let mut checker = Checker::new(TypecheckContext::default());
        checker
            .substitutions
            .insert(9, Ty::List(Box::new(Ty::Var(4))));
        let mut leaf = generic_identity_def(
            20,
            resolved_id("leaf", None, 20),
            resolved_id("value", None, 1020),
            1,
        );
        let TypedInner::Def(_, _, _, _, _, _, body, _) = &mut leaf.node else {
            unreachable!()
        };
        *body = Box::new(TypedNode {
            ty: Ty::Var(1),
            span: test_span(),
            node: TypedInner::Match(
                Box::new(typed_arg(2000, Ty::Int)),
                vec![TypedMatchArm {
                    direct_expression: true,
                    pattern: TypedMatchPattern::Wildcard,
                    guard: None,
                    body: typed_arg(2001, Ty::Var(1)),
                }],
            ),
        });
        let definitions = HashMap::from([
            (20, Arc::new(leaf)),
            (
                21,
                Arc::new(forwarding_def(21, 2, 20, 1, Ty::List(Box::new(Ty::Var(2))))),
            ),
            (22, Arc::new(forwarding_def(22, 3, 20, 1, Ty::Var(3)))),
            (23, Arc::new(forwarding_def(23, 4, 20, 1, Ty::Var(9)))),
        ]);
        let summaries = checker
            .callable_summaries(&definitions, &HashSet::new())
            .unwrap();
        let reverse = Checker::reverse_callable_edges(&summaries);
        let direct = checker.collect_direct_pattern_requirements(&summaries, &reverse);
        assert_eq!(direct, HashMap::from([(20, vec![1]), (22, vec![3])]));
        let mut ordinary = (20..=23)
            .map(|index| {
                (
                    index,
                    CallableSpecializationRequirements {
                        tyvars: if index == 20 { vec![1] } else { Vec::new() },
                        requires_concrete: index == 20,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        checker
            .propagate_callable_specialization_requirements(&summaries, &reverse, &mut ordinary)
            .unwrap();
        for (index, variable) in [(20, 1), (21, 2), (22, 3), (23, 4)] {
            assert_eq!(ordinary[&index].tyvars, vec![variable]);
            assert!(ordinary[&index].requires_concrete);
        }
    }

    #[test]
    fn callable_requirement_cycle_converges_and_sorts_seed_slots() {
        let checker = Checker::new(TypecheckContext::default());
        let definitions = HashMap::from([
            (20, Arc::new(forwarding_def(20, 1, 21, 2, Ty::Var(1)))),
            (21, Arc::new(forwarding_def(21, 2, 20, 1, Ty::Var(2)))),
            (
                22,
                Arc::new(generic_identity_def(
                    22,
                    resolved_id("seed", None, 22),
                    resolved_id("value", None, 1022),
                    3,
                )),
            ),
        ]);
        let summaries = checker
            .callable_summaries(&definitions, &HashSet::new())
            .unwrap();
        let reverse = Checker::reverse_callable_edges(&summaries);
        let mut requirements = HashMap::from([
            (
                20,
                CallableSpecializationRequirements {
                    tyvars: vec![1],
                    requires_concrete: true,
                },
            ),
            (
                21,
                CallableSpecializationRequirements {
                    tyvars: vec![],
                    requires_concrete: false,
                },
            ),
            (
                22,
                CallableSpecializationRequirements {
                    tyvars: vec![5, 3, 4],
                    requires_concrete: false,
                },
            ),
        ]);
        let work = checker
            .propagate_callable_specialization_requirements(&summaries, &reverse, &mut requirements)
            .unwrap();
        assert_eq!(requirements[&20].tyvars, vec![1]);
        assert_eq!(requirements[&21].tyvars, vec![2]);
        assert!(requirements[&21].requires_concrete);
        assert_eq!(requirements[&22].tyvars, vec![3, 4, 5]);
        assert_eq!(work.edge_visits, 2);
    }

    #[test]
    fn callable_requirement_missing_slot_uses_original_call_span() {
        let checker = Checker::new(TypecheckContext::default());
        let mut caller = forwarding_def(20, 1, 21, 2, Ty::Var(1));
        let TypedInner::Def(_, _, _, _, _, _, body, _) = &mut caller.node else {
            unreachable!()
        };
        let TypedInner::App(call, _) = &mut body.node else {
            unreachable!()
        };
        call.span = Span { start: 42, end: 57 };
        let Ty::UserFunc {
            call_substitution, ..
        } = &mut call.ty
        else {
            unreachable!()
        };
        call_substitution.clear();
        let definitions = HashMap::from([
            (20, Arc::new(caller)),
            (
                21,
                Arc::new(generic_identity_def(
                    21,
                    resolved_id("leaf", None, 21),
                    resolved_id("value", None, 1021),
                    2,
                )),
            ),
        ]);
        let summaries = checker
            .callable_summaries(&definitions, &HashSet::new())
            .unwrap();
        let reverse = Checker::reverse_callable_edges(&summaries);
        let mut requirements = HashMap::from([
            (
                20,
                CallableSpecializationRequirements {
                    tyvars: vec![],
                    requires_concrete: false,
                },
            ),
            (
                21,
                CallableSpecializationRequirements {
                    tyvars: vec![2],
                    requires_concrete: true,
                },
            ),
        ]);
        let error = checker
            .propagate_callable_specialization_requirements(&summaries, &reverse, &mut requirements)
            .err()
            .expect("required missing substitution must fail");
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::TypecheckInvariantViolation)
        );
        assert!(
            matches!(error.structured.as_ref().map(|diagnostic| &diagnostic.data),
            Some(DiagnosticData::Policy(policy)) if policy.subject.as_deref() == Some("Missing call substitution for ordinary specialization"))
        );
        assert_eq!(error.span, Span { start: 42, end: 57 });
    }

    #[test]
    fn failed_specialization_then_checkpoint_restore_matches_sibling_at_another_type() {
        let mut prefix = Checker::new(TypecheckContext::default());
        prefix.env.next_fun_idx = 100;
        let id = resolved_id("id", Some("Global::id"), 10);
        prefix
            .specialize_program(vec![generic_identity_def(
                20,
                id.clone(),
                resolved_id("value", None, 11),
                1,
            )])
            .unwrap();
        let checkpoint = prefix
            .persistent_state_with_env(prefix.env.clone())
            .checkpoint(Vec::new());
        let mut failed =
            Checker::with_persistent_state(checkpoint.clone().into(), TypecheckContext::default());
        let mut bad = call_generic(id.clone(), 20, 1, typed_arg(2000, Ty::Int));
        let TypedInner::App(call, _) = &mut bad.node else {
            unreachable!()
        };
        let Ty::UserFunc {
            call_substitution, ..
        } = &mut call.ty
        else {
            unreachable!()
        };
        call_substitution.clear();
        failed
            .specialize_program(vec![
                call_generic(id.clone(), 20, 1, typed_arg(2001, Ty::Int)),
                bad,
            ])
            .expect_err("failure follows one materialized specialization");
        assert!(failed.env.next_fun_idx > 100);
        let mut restored =
            Checker::with_persistent_state(checkpoint.clone().into(), TypecheckContext::default());
        let mut sibling =
            Checker::with_persistent_state(checkpoint.into(), TypecheckContext::default());
        let call = call_generic(id, 20, 1, typed_arg(2002, Ty::Str));
        let retried = restored.specialize_program(vec![call.clone()]).unwrap();
        let clean = sibling.specialize_program(vec![call]).unwrap();
        assert_eq!(retried, clean);
        assert_eq!(
            restored.specialization_fun_idxs,
            sibling.specialization_fun_idxs
        );
        assert_eq!(restored.env.next_fun_idx, sibling.env.next_fun_idx);
        assert_eq!(generated_def_fun_idxs(&retried, "Global::id"), vec![100]);
    }

    #[test]
    fn do_safebind_specializes_pattern_against_projected_payload() {
        let mut checker = Checker::new(TypecheckContext::default());
        let result_ty = Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error));
        let control = Box::new(TypedDoSafeBind {
            pattern: TypedPattern::Located(
                Span { start: 11, end: 14 },
                Box::new(TypedPattern::Wildcard(Ty::Var(91_200))),
            ),
            rhs: Box::new(TypedNode {
                ty: result_ty.clone(),
                span: test_span(),
                node: TypedInner::Lit(Lit::Unit),
            }),
            projection: SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty: Ty::Int,
                error_ty: Ty::Error,
            },
            failure_target: SafeBindFailureTarget::DoMonadFail(Box::new(MonadFailTarget {
                carrier_ty: result_ty.clone(),
                error_id: sigil::resolved::ResolvedId {
                    name: "error".into(),
                    qualified_name: None,
                    unique_id: 91999,
                    compiler_generated: true,
                    symbol_info: None,
                    span: spire::ast::Span { start: 0, end: 0 },
                },
                call: Box::new(TypedNode {
                    ty: Ty::Error,
                    span: spire::ast::Span { start: 0, end: 0 },
                    node: TypedInner::ConstructorCall(
                        1,
                        vec![TypedNode {
                            ty: Ty::Error,
                            span: spire::ast::Span { start: 0, end: 0 },
                            node: TypedInner::Var(sigil::resolved::ResolvedId {
                                name: "error".into(),
                                qualified_name: None,
                                unique_id: 91999,
                                compiler_generated: true,
                                symbol_info: None,
                                span: spire::ast::Span { start: 0, end: 0 },
                            }),
                        }],
                    ),
                }),
            })),
            continuation: Box::new(TypedNode {
                ty: result_ty,
                span: test_span(),
                node: TypedInner::Lit(Lit::Unit),
            }),
            origins: DoSafeBindOrigins {
                do_span: test_span(),
                operator_span: test_span(),
                pattern_span: test_span(),
                rhs_span: test_span(),
                result_span: test_span(),
            },
        });

        let rewritten = checker
            .rewrite_do_safebind_specializations(
                control,
                &test_span(),
                &HashMap::new(),
                &HashMap::new(),
                &HashSet::new(),
                &mut HashMap::new(),
                &mut Vec::new(),
            )
            .expect("do SafeBind specialization should succeed");

        assert!(
            matches!(&rewritten.pattern, TypedPattern::Located(source, _) if *source == Span { start: 11, end: 14 })
        );
        assert!(matches!(
            rewritten.pattern.unlocated(),
            TypedPattern::Wildcard(Ty::Int)
        ));
    }

    #[test]
    fn injected_callable_specialization_materializes_target_with_bound_arguments() {
        for bound_arg_count in [0, 1] {
            let mut checker = Checker::new(TypecheckContext::default());
            checker.env.next_fun_idx = 100;
            let function_id = resolved_id("first", Some("Global::first"), 10);
            let mut definition =
                generic_identity_def(20, function_id.clone(), resolved_id("value", None, 11), 1);
            if bound_arg_count == 1 {
                let TypedInner::Def(_, _, _, parameters, _, _, _, _) = &mut definition.node else {
                    unreachable!()
                };
                parameters.push(TypedValueParameter {
                    id: resolved_id("other", None, 12),
                    mode: spire::ast::ValueParameterMode::PositionalOrNamed,
                    ty: Ty::Var(1),
                    span: test_span(),
                });
                let Ty::UserFunc { params, .. } = &mut definition.ty else {
                    unreachable!()
                };
                params.push(Ty::Var(1));
            }
            let injected = TypedNode {
                ty: Ty::Func(vec![Ty::Int], Box::new(Ty::Int)),
                span: test_span(),
                node: TypedInner::InjectCall(
                    Box::new(TypedNode {
                        ty: Ty::UserFunc {
                            fun_idx: 20,
                            type_params: vec![1],
                            call_substitution: vec![(1, Ty::Int)],
                            params: vec![Ty::Int; bound_arg_count + 1],
                            ret: Box::new(Ty::Int),
                        },
                        span: test_span(),
                        node: TypedInner::Var(function_id),
                    }),
                    (0..bound_arg_count)
                        .map(|_| typed_arg(13, Ty::Int))
                        .collect(),
                ),
            };
            let nodes = checker
                .specialize_program(vec![definition, injected])
                .expect("injected callable specialization should succeed");
            let generated = generated_def_fun_idxs(&nodes, "Global::first");
            assert_eq!(
                generated.len(),
                1,
                "injected callable must materialize a definition"
            );
            let injected = nodes
                .iter()
                .find(|node| matches!(node.node, TypedInner::InjectCall(..)))
                .expect("injected callable remains in the program");
            assert_eq!(injected.ty, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
            let TypedInner::InjectCall(target, args) = &injected.node else {
                unreachable!()
            };
            assert_eq!(args.len(), bound_arg_count);
            assert!(
                matches!(&target.ty, Ty::UserFunc { fun_idx, call_substitution, .. }
                if *fun_idx == generated[0] && call_substitution.is_empty())
            );
        }
    }

    #[test]
    fn specializations_reuse_identity_across_incremental_runs() {
        let mut checker = Checker::new(TypecheckContext::default());
        checker.env.next_fun_idx = 100;
        let ty_var = 1;
        let original_fun_idx = 20;
        let function_id = resolved_id("id", Some("Global::id"), 10);
        let param_id = resolved_id("x", None, 11);
        let original_def =
            generic_identity_def(original_fun_idx, function_id.clone(), param_id, ty_var);

        let first = checker
            .specialize_program(vec![
                original_def,
                call_generic(
                    function_id.clone(),
                    original_fun_idx,
                    ty_var,
                    typed_arg(1000, Ty::Int),
                ),
            ])
            .expect("first specialization should succeed");
        let first_generated = generated_def_fun_idxs(&first, "Global::id");
        assert_eq!(first_generated.len(), 1);

        let second = checker
            .specialize_program(vec![call_generic(
                function_id,
                original_fun_idx,
                ty_var,
                typed_arg(1001, Ty::Int),
            )])
            .expect("second specialization should succeed");

        assert!(
            generated_def_fun_idxs(&second, "Global::id").is_empty(),
            "incremental reuse should not emit another specialization def"
        );
        assert_eq!(app_fun_idxs(&second), first_generated);
    }

    #[test]
    fn ordinary_specialization_rejects_missing_call_substitution() {
        let mut checker = Checker::new(TypecheckContext::default());
        let def = generic_identity_def(
            20,
            resolved_id("id", Some("Global::id"), 10),
            resolved_id("x", None, 11),
            1,
        );
        let callable = generic_identity_ty(20, 1);
        let error = checker
            .infer_specialization_mapping(
                &def,
                &[typed_arg(12, Ty::Int)],
                Some(&callable),
                false,
                &[1],
            )
            .expect_err("ordinary calls must retain the solver substitution");
        assert!(error.message.contains("Missing call substitution"));
    }

    #[test]
    fn named_extractor_specialization_rejects_missing_saved_substitution() {
        let mut checker = Checker::new(TypecheckContext::default());
        let id = resolved_id("relay", Some("Int::relay"), 10);
        let definitions = HashMap::from([(
            20,
            Arc::new(generic_identity_def(
                20,
                id.clone(),
                resolved_id("source", None, 11),
                1,
            )),
        )]);
        let requirements = HashMap::from([(
            20,
            CallableSpecializationRequirements {
                tyvars: vec![1],
                requires_concrete: true,
            },
        )]);
        let needs = HashSet::from([20]);
        let mut instances = HashMap::new();
        let mut generated = Vec::new();
        let mut context = SpecializationContext {
            defs_by_fun_idx: &definitions,
            bound_tyvars_by_fun_idx: &requirements,
            needs_specialization: &needs,
            specialization_fun_idxs: &mut instances,
            generated_defs: &mut generated,
        };
        let error = checker
            .specialize_extractor_head(&id, generic_identity_ty(20, 1), &mut context)
            .expect_err("named heads must retain their solver substitution");
        assert!(error.message.contains("Missing call substitution"));
        assert!(generated.is_empty());
    }

    #[test]
    fn specialization_mapping_rejects_argument_count_mismatch_before_zip() {
        let mut checker = Checker::new(TypecheckContext::default());
        let def = generic_identity_def(
            20,
            resolved_id("id", Some("Global::id"), 10),
            resolved_id("x", None, 11),
            1,
        );

        let err = checker
            .infer_specialization_mapping(&def, &[], None, false, &[1])
            .expect_err("specialization must not infer from a partial argument zip");

        assert_eq!(
            err.message,
            "Cannot specialize call: expected 1 argument(s), got 0"
        );
    }

    #[test]
    fn trait_specialization_requires_complete_concrete_solver_substitution() {
        let checker = Checker::new(TypecheckContext::default());
        let def = generic_identity_def(
            20,
            resolved_id("id", Some("Global::id"), 10),
            resolved_id("x", None, 11),
            1,
        );
        for mapping in [
            HashMap::new(),
            HashMap::from([(1, Ty::List(Box::new(Ty::Var(2))))]),
        ] {
            let error = checker
                .trait_specialization_arguments(&def, &[1], &mapping)
                .expect_err("missing or nested unresolved solver substitutions must fail");
            assert!(error
                .message
                .contains("Incomplete trait method substitution"));
        }
        let mapping = HashMap::from([(1, Ty::List(Box::new(Ty::Int))), (2, Ty::Bool)]);
        assert_eq!(
            checker
                .trait_specialization_arguments(&def, &[1], &mapping)
                .unwrap(),
            vec![Ty::List(Box::new(Ty::Int))],
            "impl head variables outside the body binders must not invalidate the substitution",
        );
    }

    #[test]
    fn callable_instantiations_with_equal_display_names_remain_distinct() {
        let checker = Checker::new(TypecheckContext::default());
        let first = generic_identity_def(
            20,
            resolved_id("same", Some("Global::same"), 10),
            resolved_id("x", None, 11),
            1,
        );
        let second = generic_identity_def(
            21,
            resolved_id("same", Some("Global::same"), 12),
            resolved_id("x", None, 13),
            2,
        );
        assert_ne!(
            checker
                .specialization_key_for_def(&first, &[Ty::Int], &HashMap::from([(1, Ty::Int)]))
                .unwrap(),
            checker
                .specialization_key_for_def(&second, &[Ty::Int], &HashMap::from([(2, Ty::Int)]))
                .unwrap(),
            "canonical declaration identity must distinguish callables"
        );
    }

    #[test]
    fn specialization_keys_distinguish_structural_type_arguments() {
        let mut checker = Checker::new(TypecheckContext::default());
        checker.env.next_fun_idx = 100;
        let ty_var = 1;
        let original_fun_idx = 20;
        let function_id = resolved_id("id", Some("Global::id"), 10);
        let param_id = resolved_id("x", None, 11);
        let original_def =
            generic_identity_def(original_fun_idx, function_id.clone(), param_id, ty_var);

        let box_int = Ty::Struct("Global::Box".to_string(), NominalType::new(vec![Ty::Int]));
        let box_string = Ty::Struct("Global::Box".to_string(), NominalType::new(vec![Ty::Str]));
        let typed = checker
            .specialize_program(vec![
                original_def,
                call_generic(
                    function_id.clone(),
                    original_fun_idx,
                    ty_var,
                    typed_arg(1000, box_int),
                ),
                call_generic(
                    function_id,
                    original_fun_idx,
                    ty_var,
                    typed_arg(1001, box_string),
                ),
            ])
            .expect("specialization should succeed");

        let generated = generated_def_fun_idxs(&typed, "Global::id");
        assert_eq!(generated.len(), 2);
        let app_fun_idxs = app_fun_idxs(&typed);
        assert_eq!(app_fun_idxs.len(), 2);
        assert_ne!(app_fun_idxs[0], app_fun_idxs[1]);
    }
}
