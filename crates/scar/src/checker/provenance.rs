//! Compile-time value views follow source projections, independently of nominal type resolution.
use super::*;
use diagnostics::{DiagnosticOrigin, TypeDiagnosticReason};

type Source = (ConstructorCapabilityProvenance, Ty);
type Bindings = HashMap<u32, Source>;
// Scoped to one immutable AST analysis; no pointer identities escape this walk.
type ProvenanceAnalysisCache = HashMap<usize, Vec<(Bindings, Provenance)>>;
use ConstructorCapabilityProvenance as Provenance;

enum CallbackProvenanceTarget {
    Inputs,
    Result,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) enum Projection {
    Field { index: usize, tag: Option<u32> },
    Element,
    TypeArgument(usize),
    ConstructorSlot { capability: String, index: usize },
}

impl Checker {
    pub(super) fn constructor_capability_for_node(&self, node: &TypedNode) -> Provenance {
        if let Some(capability) = self.constructor_capability_for_type(&node.ty) {
            return Provenance::constrained(capability);
        }
        self.value_provenance(node, &Bindings::new())
    }

    fn source_provenance(&self, node: &TypedNode, bindings: &Bindings) -> Source {
        (self.value_provenance(node, bindings), node.ty.clone())
    }

    fn value_provenance(&self, node: &TypedNode, bindings: &Bindings) -> Provenance {
        self.cached_value_provenance(node, bindings, &mut ProvenanceAnalysisCache::new())
    }

    fn cached_source_provenance(
        &self,
        node: &TypedNode,
        bindings: &Bindings,
        cache: &mut ProvenanceAnalysisCache,
    ) -> Source {
        (
            self.cached_value_provenance(node, bindings, cache),
            node.ty.clone(),
        )
    }

    fn cached_value_provenance(
        &self,
        node: &TypedNode,
        bindings: &Bindings,
        cache: &mut ProvenanceAnalysisCache,
    ) -> Provenance {
        let identity = node as *const TypedNode as usize;
        if let Some((_, provenance)) = cache
            .get(&identity)
            .and_then(|entries| entries.iter().find(|(known, _)| known == bindings))
        {
            return provenance.clone();
        }
        let provenance = self.analyze_value_provenance(node, bindings, cache);
        cache
            .entry(identity)
            .or_default()
            .push((bindings.clone(), provenance.clone()));
        provenance
    }

    fn analyze_value_provenance(
        &self,
        node: &TypedNode,
        bindings: &Bindings,
        cache: &mut ProvenanceAnalysisCache,
    ) -> Provenance {
        match &node.node {
            TypedInner::Var(id) => bindings
                .get(&id.unique_id)
                .map(|source| source.0.clone())
                .or_else(|| self.constructor_capabilities.get(&id.unique_id).cloned())
                .unwrap_or_else(|| {
                    if self.callable_signatures.contains_key(&id.unique_id) {
                        Provenance::DeclaredCallable(id.unique_id)
                    } else {
                        Provenance::RequiresProof
                    }
                }),
            TypedInner::TupleLiteral(items) | TypedInner::StructLit(_, items) => {
                Provenance::Fields(
                    items
                        .iter()
                        .map(|item| self.cached_source_provenance(item, bindings, cache))
                        .collect(),
                )
            }
            TypedInner::ConstructorCall(tag, items) => Provenance::Variants(vec![(
                *tag,
                items
                    .iter()
                    .map(|item| self.cached_source_provenance(item, bindings, cache))
                    .collect(),
            )]),
            TypedInner::ListNil => Provenance::Sequence(Vec::new()),
            TypedInner::ListLiteral(items) => Provenance::Sequence(
                items
                    .iter()
                    .map(|item| self.cached_source_provenance(item, bindings, cache))
                    .collect(),
            ),
            TypedInner::HashMapLiteral(items) => Provenance::Sequence(
                items
                    .iter()
                    .map(|(_, item)| self.cached_source_provenance(item, bindings, cache))
                    .collect(),
            ),
            TypedInner::ListCons(head, tail) => {
                let mut elements = vec![self.cached_source_provenance(head, bindings, cache)];
                let tail_source = self.cached_source_provenance(tail, bindings, cache);
                if !matches!(&tail_source.0, Provenance::Sequence(items) if items.is_empty()) {
                    elements.push(self.project_provenance(
                        &tail_source,
                        &Projection::Element,
                        &head.ty,
                    ));
                }
                Provenance::Sequence(elements)
            }
            TypedInner::FieldAccess(value, index) => {
                self.project_provenance(
                    &self.cached_source_provenance(value, bindings, cache),
                    &Projection::Field {
                        index: *index as usize,
                        tag: None,
                    },
                    &node.ty,
                )
                .0
            }
            TypedInner::FacetView {
                api,
                source,
                path,
                source_is_result,
            } => {
                let mut source = self.cached_source_provenance(source, bindings, cache);
                if *source_is_result {
                    source = self.project_provenance(
                        &source,
                        &Projection::Field {
                            index: 0,
                            tag: Some(0),
                        },
                        &path.source_ty,
                    );
                }
                for segment in &path.segments {
                    let projection = match segment {
                        TypedFacetSegment::Field { field_index, .. }
                        | TypedFacetSegment::Tuple { field_index, .. } => Projection::Field {
                            index: *field_index as usize,
                            tag: None,
                        },
                        TypedFacetSegment::ListIndex { .. } | TypedFacetSegment::MapKey { .. } => {
                            Projection::Element
                        }
                        TypedFacetSegment::Variant {
                            variant_tag,
                            payload_arity: 1,
                            ..
                        } => Projection::Field {
                            index: 1,
                            tag: Some(*variant_tag),
                        },
                        TypedFacetSegment::ListRange { .. } => continue,
                        _ => {
                            source = (Provenance::Intersection(Vec::new()), path.focus_ty.clone());
                            break;
                        }
                    };
                    source = self.project_provenance(&source, &projection, &path.focus_ty);
                }
                // These reads produce a new Result, independently of the
                // focus's constructor capabilities. Preserve the focus view
                // under Ok so extracting it does not strengthen its proof.
                if matches!(api, TypedFacetApi::Preview) || *source_is_result || path.may_fail {
                    Provenance::Variants(vec![
                        (0, vec![source]),
                        (1, vec![(Provenance::RequiresProof, Ty::Error)]),
                    ])
                } else {
                    source.0
                }
            }
            TypedInner::Closure(parameters, captures, body)
            | TypedInner::ExtractorClosure(parameters, captures, body)
            | TypedInner::CaptureClosure(parameters, captures, body) => {
                let mut local = bindings.clone();
                for parameter in parameters {
                    local.insert(
                        parameter.id.unique_id,
                        (
                            Provenance::Parameter(parameter.id.unique_id),
                            parameter.ty.clone(),
                        ),
                    );
                }
                Provenance::Callable {
                    parameters: parameters
                        .iter()
                        .map(|parameter| parameter.id.unique_id)
                        .collect(),
                    result: Box::new(self.cached_source_provenance(body, &local, cache)),
                    calls: {
                        let mut call_bindings = local.clone();
                        for parameter in parameters {
                            if let Some(declared) = self
                                .explicit_closure_parameters
                                .get(&parameter.id.unique_id)
                            {
                                call_bindings.insert(
                                    parameter.id.unique_id,
                                    self.template_provenance(
                                        declared,
                                        &parameter.ty,
                                        &Bindings::new(),
                                    ),
                                );
                            }
                        }
                        let mut calls = Vec::new();
                        // Only symbolic inputs can acquire a different value
                        // view when this callable is invoked. A closed body has
                        // already had all of its calls checked. Rebuilding its
                        // call recipes through nested zero-argument closures
                        // would duplicate the same subtree exponentially.
                        let inputs = parameters
                            .iter()
                            .map(|parameter| parameter.id.unique_id)
                            .chain(captures.iter().map(|id| id.unique_id))
                            .collect();
                        if Self::finalized_closure_captures(body, &inputs)
                            .iter()
                            .filter_map(|id| call_bindings.get(&id.unique_id))
                            .any(|source| Self::provenance_has_parameter(&source.0))
                        {
                            self.capture_call_provenance(body, &call_bindings, &mut calls, cache);
                        }
                        calls
                    },
                }
            }
            TypedInner::App(function, arguments) => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.cached_source_provenance(argument, bindings, cache))
                    .collect::<Vec<_>>();
                let function_source = self.cached_source_provenance(function, bindings, cache);
                self.invoke_provenance(&function_source, &arguments, &node.ty)
                    .0
            }
            TypedInner::Pipe(value, function) => {
                let argument = self.cached_source_provenance(value, bindings, cache);
                let function_source = self.cached_source_provenance(function, bindings, cache);
                self.invoke_provenance(&function_source, &[argument], &node.ty)
                    .0
            }
            TypedInner::InjectCall(function, arguments) => Provenance::Injected {
                function: Box::new(self.cached_source_provenance(function, bindings, cache)),
                arguments: arguments
                    .iter()
                    .map(|argument| self.cached_source_provenance(argument, bindings, cache))
                    .collect(),
            },
            TypedInner::MapErr(value, error) => {
                let source = self.cached_source_provenance(value, bindings, cache);
                let success = self.project_provenance(
                    &source,
                    &Projection::Field {
                        index: 0,
                        tag: Some(0),
                    },
                    &Ty::Hole,
                );
                Provenance::Variants(vec![
                    (0, vec![success]),
                    (
                        1,
                        vec![self.cached_source_provenance(error, bindings, cache)],
                    ),
                ])
            }
            TypedInner::Capture(function, arguments) if arguments.is_empty() => {
                self.cached_value_provenance(function, bindings, cache)
            }
            TypedInner::TraitCall {
                obligation,
                method_name,
                dispatch,
                args,
                ..
            } => {
                let Some(method) = self
                    .traits
                    .get(&obligation.trait_id)
                    .and_then(|info| info.methods.get(method_name))
                else {
                    return Provenance::Intersection(Vec::new());
                };
                let arguments = args
                    .iter()
                    .map(|argument| self.cached_source_provenance(argument, bindings, cache))
                    .collect::<Vec<_>>();
                // A TypeCtorTrait method that returns its declared `Self` RTA
                // carries the selected capability even without a value input.
                // Keep that proof on a later binding instead of attempting to
                // reconstruct it from the result representation.
                let returns_declared_self = !self.traits[&obligation.trait_id]
                    .constructor_slots
                    .is_empty()
                    && match method.ret_ty.syntax() {
                        AstTy::Named(_, name) => Self::surface_name(name) == "Self",
                        AstTy::Generic(_, name, _) => Self::surface_name(name) == "Self",
                        _ => false,
                    }
                    && method.return_type_arguments.iter().any(|argument| {
                        matches!(
                            argument.ty.syntax(),
                            AstTy::Named(_, name) if Self::surface_name(name) == "Self"
                        )
                    });
                fn mentions_self(ty: &AstTy) -> bool {
                    match ty {
                        AstTy::Named(_, name) => name == "Self",
                        AstTy::Generic(_, name, arguments) => {
                            name == "Self" || arguments.iter().any(mentions_self)
                        }
                        AstTy::Tuple(_, items) => items.iter().any(mentions_self),
                        AstTy::Func(_, parameters, result) => {
                            parameters.iter().any(mentions_self) || mentions_self(result)
                        }
                        AstTy::ImplTrait(_, _) => false,
                    }
                }
                let fresh_declared_self = returns_declared_self
                    && method
                        .value_parameters
                        .iter()
                        .all(|parameter| !mentions_self(parameter.ty.syntax()));
                if self.traits[&obligation.trait_id]
                    .constructor_slots
                    .is_empty()
                {
                    if let TraitDispatch::Selected(selected) = dispatch {
                        if let TraitImplementationId::Declared { declaration, .. } =
                            &selected.implementation
                        {
                            let Some(signature) = self
                                .trait_impls
                                .values()
                                .find(|info| &info.declaration_key == declaration)
                                .and_then(|info| info.method_signature_lists.get(method_name))
                            else {
                                return Provenance::Intersection(Vec::new());
                            };
                            let mut parameters = Vec::new();
                            let mut result = None;
                            for entry in &signature.entries {
                                let Ok(ty) = self.canonical_to_ty(&entry.ty) else {
                                    return Provenance::Intersection(Vec::new());
                                };
                                match entry.role {
                                    TypeListRole::ValueParameter => parameters.push(ty),
                                    TypeListRole::ReturnType => result = Some(ty),
                                    _ => {}
                                }
                            }
                            let Some(result) = result else {
                                return Provenance::Intersection(Vec::new());
                            };
                            let mut variables = HashMap::new();
                            if let Some(outcome) = self.collect_callable_provenance_variables(
                                &parameters,
                                &arguments,
                                &mut variables,
                                CallbackProvenanceTarget::Result,
                            ) {
                                return Provenance::ConstructorApplication(outcome);
                            }
                            return self
                                .template_provenance(
                                    &result,
                                    &node.ty,
                                    &self.merged_provenance_variables(&variables),
                                )
                                .0;
                        }
                    }
                }
                let inferred = self
                    .invoke_provenance(
                        &(Provenance::DeclaredCallable(method.id.unique_id), Ty::Hole),
                        &arguments,
                        &node.ty,
                    )
                    .0;
                if returns_declared_self {
                    let mut capabilities = BTreeSet::from([obligation.trait_id.clone()]);
                    let result_ty = self.resolve_ty(&node.ty);
                    let concrete_return_witness = Self::is_concrete_constructor_shape(&result_ty)
                        || matches!(
                            &result_ty,
                            Ty::SelfApp(items)
                                if Self::constructor_application_parts(items)
                                    .is_some_and(|(witness, _)| !matches!(self.resolve_ty(witness), Ty::Var(_) | Ty::Hole))
                        );
                    if fresh_declared_self
                        && concrete_return_witness
                        && matches!(dispatch, TraitDispatch::Selected(_))
                    {
                        for (trait_key, info) in &self.traits {
                            if !info.constructor_slots.is_empty()
                                && matches!(
                                    // This enumerates optional capabilities; only required proofs propagate errors.
                                    self.constructor_projection(trait_key, &result_ty),
                                    ConstructorProjectionOutcome::Applicable { .. }
                                )
                            {
                                capabilities.insert(trait_key.clone());
                            }
                        }
                    }
                    Provenance::ConstrainedTemplate {
                        capabilities,
                        source: Box::new((inferred, node.ty.clone())),
                    }
                } else {
                    inferred
                }
            }
            TypedInner::EagerBoundary(inner) => {
                self.cached_value_provenance(inner, bindings, cache)
            }
            TypedInner::If(_, then_branch, Some(else_branch)) => self
                .common_constructor_provenance(&[
                    self.cached_source_provenance(then_branch, bindings, cache),
                    self.cached_source_provenance(else_branch, bindings, cache),
                ]),
            TypedInner::ApplyPattern {
                value,
                pattern,
                projections,
            } => {
                let mut local = bindings.clone();
                let source = self.cached_source_provenance(value, bindings, cache);
                self.pattern_provenance_bindings(pattern, &source, &mut local);
                let slots = projections
                    .iter()
                    .map(|(id, ty)| {
                        local
                            .get(&id.unique_id)
                            .cloned()
                            .unwrap_or_else(|| (Provenance::RequiresProof, ty.clone()))
                    })
                    .collect::<Vec<_>>();
                let payload = match slots.as_slice() {
                    [] => (Provenance::Fields(Vec::new()), Ty::Unit),
                    [slot] => slot.clone(),
                    _ => (
                        Provenance::Fields(slots.clone()),
                        Ty::Tuple(slots.iter().map(|(_, ty)| ty.clone()).collect()),
                    ),
                };
                Provenance::Variants(vec![
                    (0, vec![payload]),
                    (1, vec![(Provenance::RequiresProof, Ty::Error)]),
                ])
            }
            TypedInner::Match(scrutinee, arms) => {
                let source = self.cached_source_provenance(scrutinee, bindings, cache);
                self.common_constructor_provenance(
                    &arms
                        .iter()
                        .map(|arm| {
                            let mut local = bindings.clone();
                            self.match_provenance_bindings(&arm.pattern, &source, &mut local);
                            self.cached_source_provenance(&arm.body, &local, cache)
                        })
                        .collect::<Vec<_>>(),
                )
            }
            TypedInner::Block(statements) => {
                let mut local = bindings.clone();
                let mut result = Provenance::RequiresProof;
                for statement in statements {
                    result = self.cached_value_provenance(statement, &local, cache);
                    let inner = match &statement.node {
                        TypedInner::Semi(inner) => inner.as_ref(),
                        _ => statement,
                    };
                    match &inner.node {
                        TypedInner::Bind(pattern, value) => {
                            let source = self.cached_source_provenance(value, &local, cache);
                            self.pattern_provenance_bindings(pattern, &source, &mut local);
                        }
                        TypedInner::SafeBind(pattern, value, _, _) => {
                            let source = self.cached_source_provenance(value, &local, cache);
                            let success = self.safebind_source_provenance(&source);
                            self.pattern_provenance_bindings(pattern, &success, &mut local);
                        }
                        _ => {}
                    }
                }
                result
            }
            TypedInner::DoSafeBind(control) => {
                let mut local = bindings.clone();
                let source = self.cached_source_provenance(&control.rhs, bindings, cache);
                let success = self.safebind_source_provenance(&source);
                self.pattern_provenance_bindings(&control.pattern, &success, &mut local);
                self.cached_value_provenance(&control.continuation, &local, cache)
            }
            _ => Provenance::RequiresProof,
        }
    }

    fn provenance_has_parameter(provenance: &Provenance) -> bool {
        let has_source = |source: &Source| Self::provenance_has_parameter(&source.0);
        match provenance {
            Provenance::Parameter(_) => true,
            Provenance::ConstrainedTemplate { source, .. }
            | Provenance::Projection { source, .. } => has_source(source),
            Provenance::Intersection(sources)
            | Provenance::Fields(sources)
            | Provenance::Sequence(sources) => sources.iter().any(has_source),
            Provenance::Variants(variants) => variants
                .iter()
                .any(|(_, sources)| sources.iter().any(has_source)),
            Provenance::Callable { result, calls, .. } => {
                has_source(result)
                    || calls.iter().any(|(function, arguments)| {
                        has_source(function) || arguments.iter().any(has_source)
                    })
            }
            Provenance::Injected {
                function,
                arguments,
            }
            | Provenance::Call {
                function,
                arguments,
            } => has_source(function) || arguments.iter().any(has_source),
            Provenance::Template { variables, .. } => variables.values().any(has_source),
            _ => false,
        }
    }

    fn capture_call_provenance(
        &self,
        node: &TypedNode,
        bindings: &Bindings,
        calls: &mut Vec<(Source, Vec<Source>)>,
        cache: &mut ProvenanceAnalysisCache,
    ) {
        match &node.node {
            TypedInner::Block(statements) => {
                let mut local = bindings.clone();
                for statement in statements {
                    self.capture_call_provenance(statement, &local, calls, cache);
                    let inner = match &statement.node {
                        TypedInner::Semi(inner) => inner.as_ref(),
                        _ => statement,
                    };
                    match &inner.node {
                        TypedInner::Bind(pattern, value) => {
                            self.pattern_provenance_bindings(
                                pattern,
                                &self.cached_source_provenance(value, &local, cache),
                                &mut local,
                            );
                        }
                        TypedInner::SafeBind(pattern, value, _, _) => {
                            let success = self.safebind_source_provenance(
                                &self.cached_source_provenance(value, &local, cache),
                            );
                            self.pattern_provenance_bindings(pattern, &success, &mut local);
                        }
                        _ => {}
                    }
                }
                return;
            }
            TypedInner::Match(value, arms) => {
                self.capture_call_provenance(value, bindings, calls, cache);
                let source = self.cached_source_provenance(value, bindings, cache);
                for arm in arms {
                    let mut local = bindings.clone();
                    self.match_provenance_bindings(&arm.pattern, &source, &mut local);
                    if let Some(guard) = &arm.guard {
                        self.capture_call_provenance(guard, &local, calls, cache);
                    }
                    self.capture_call_provenance(&arm.body, &local, calls, cache);
                }
                return;
            }
            TypedInner::DoSafeBind(control) => {
                self.capture_call_provenance(&control.rhs, bindings, calls, cache);
                let mut local = bindings.clone();
                let success = self.safebind_source_provenance(&self.cached_source_provenance(
                    &control.rhs,
                    bindings,
                    cache,
                ));
                self.pattern_provenance_bindings(&control.pattern, &success, &mut local);
                self.capture_call_provenance(&control.continuation, &local, calls, cache);
                return;
            }

            TypedInner::App(function, arguments) => {
                calls.push((
                    self.cached_source_provenance(function, bindings, cache),
                    arguments
                        .iter()
                        .map(|argument| self.cached_source_provenance(argument, bindings, cache))
                        .collect(),
                ));
            }
            TypedInner::TraitCall {
                obligation,
                method_name,
                args,
                ..
            } => {
                if let Some(method) = self
                    .traits
                    .get(&obligation.trait_id)
                    .and_then(|info| info.methods.get(method_name))
                {
                    calls.push((
                        (Provenance::DeclaredCallable(method.id.unique_id), Ty::Hole),
                        args.iter()
                            .map(|argument| {
                                self.cached_source_provenance(argument, bindings, cache)
                            })
                            .collect(),
                    ));
                }
            }
            _ => {}
        }
        for child in Self::capture_expression_children(node) {
            self.capture_call_provenance(child, bindings, calls, cache);
        }
    }

    fn trait_provenance_template(
        &self,
        ty: &Ty,
        context: Option<(&str, &Ty)>,
    ) -> Result<Ty, ConstructorApplicationOutcome> {
        let Some((capability, actual)) = context else {
            return Ok(ty.clone());
        };
        let recurse = |ty: &Ty| self.trait_provenance_template(ty, context);
        match ty {
            Ty::SelfApp(arguments) if Self::constructor_application_parts(arguments).is_none() => {
                match self.expand_constructor_template(capability, actual, arguments) {
                    ConstructorApplicationOutcome::Applied(expanded) => Ok(expanded),
                    outcome @ (ConstructorApplicationOutcome::Deferred { .. }
                    | ConstructorApplicationOutcome::Rejected { .. }) => Err(outcome),
                }
            }
            Ty::Func(parameters, result) => Ok(Ty::Func(
                parameters.iter().map(recurse).collect::<Result<_, _>>()?,
                Box::new(recurse(result)?),
            )),
            Ty::ExtractorClosure(element) => Ok(Ty::ExtractorClosure(Box::new(recurse(element)?))),
            Ty::MatchResult(element) => Ok(Ty::MatchResult(Box::new(recurse(element)?))),
            Ty::List(element) => Ok(Ty::List(Box::new(recurse(element)?))),
            Ty::Tuple(items) => Ok(Ty::Tuple(
                items.iter().map(recurse).collect::<Result<_, _>>()?,
            )),
            Ty::Result(ok, error) => Ok(Ty::Result(
                Box::new(recurse(ok)?),
                Box::new(recurse(error)?),
            )),
            Ty::Enum(name, arguments) => Ok(Ty::Enum(
                name.clone(),
                arguments.iter().map(recurse).collect::<Result<_, _>>()?,
            )),
            Ty::Struct(name, fields) => {
                Ok(Ty::Struct(name.clone(), fields.try_map_types(recurse)?))
            }
            Ty::Record(name, fields) => {
                Ok(Ty::Record(name.clone(), fields.try_map_types(recurse)?))
            }
            _ => Ok(ty.clone()),
        }
    }

    fn merged_provenance_variables(&self, variables: &HashMap<u32, Vec<Source>>) -> Bindings {
        variables
            .iter()
            .map(|(variable, sources)| {
                (
                    *variable,
                    (
                        self.common_constructor_provenance(sources),
                        sources
                            .first()
                            .map(|source| source.1.clone())
                            .unwrap_or(Ty::Hole),
                    ),
                )
            })
            .collect()
    }

    fn collect_callable_provenance_variables(
        &self,
        parameters: &[Ty],
        arguments: &[Source],
        variables: &mut HashMap<u32, Vec<Source>>,
        target: CallbackProvenanceTarget,
    ) -> Option<ConstructorApplicationOutcome> {
        for (parameter, argument) in parameters.iter().zip(arguments) {
            if !matches!(parameter, Ty::Func(..)) {
                if let Some(outcome) =
                    self.collect_provenance_variables(parameter, argument, variables)
                {
                    return Some(outcome);
                }
            }
        }
        let producers = parameters
            .iter()
            .zip(arguments)
            .filter_map(|(parameter, argument)| match parameter {
                Ty::Func(inputs, output) => {
                    Some((inputs.as_slice(), output.as_ref(), argument.clone()))
                }
                _ => None,
            })
            .collect();
        let value_inputs = if matches!(target, CallbackProvenanceTarget::Inputs) {
            variables.keys().copied().collect()
        } else {
            HashSet::new()
        };
        self.collect_callback_output_provenance(producers, variables, &value_inputs)
    }

    /// Evaluate declared slot dependencies in the same order for callback
    /// inputs, callback contracts, and the enclosing call's result view.
    fn collect_callback_output_provenance(
        &self,
        mut producers: Vec<(&[Ty], &Ty, Source)>,
        variables: &mut HashMap<u32, Vec<Source>>,
        value_inputs: &HashSet<u32>,
    ) -> Option<ConstructorApplicationOutcome> {
        let input_variables = |inputs: &[Ty]| {
            let mut variables = Vec::new();
            for input in inputs {
                Self::collect_ty_vars(input, &mut variables);
            }
            variables
        };
        let initial_inputs = variables.keys().copied().collect::<HashSet<_>>();
        while !producers.is_empty() {
            let ready = producers.iter().position(|(inputs, _, _)| {
                input_variables(inputs).iter().all(|dependency| {
                    initial_inputs.contains(dependency)
                        || !producers.iter().any(|(inputs, output, _)| {
                            let mut outputs = Vec::new();
                            Self::collect_ty_vars(output, &mut outputs);
                            // A T -> T slot retains the same symbolic source; it
                            // does not establish a new source for T by itself.
                            outputs.contains(dependency)
                                && !input_variables(inputs).contains(dependency)
                        })
                })
            });
            let Some(index) = ready else {
                let mut waiting_on = producers
                    .iter()
                    .flat_map(|(inputs, _, _)| input_variables(inputs))
                    .collect::<Vec<_>>();
                waiting_on.sort_unstable();
                waiting_on.dedup();
                return Some(ConstructorApplicationOutcome::Deferred { waiting_on });
            };
            let (inputs, output, source) = producers.remove(index);
            let known = self.merged_provenance_variables(variables);
            let inputs = inputs
                .iter()
                .map(|ty| self.template_provenance(ty, &self.resolve_ty(ty), &known))
                .collect::<Vec<_>>();
            let result = self.invoke_provenance(&source, &inputs, output);
            let mut produced = HashMap::new();
            if let Some(outcome) = self.collect_provenance_variables(output, &result, &mut produced)
            {
                return Some(outcome);
            }
            for (variable, sources) in produced {
                // Callback results contribute to a return view. They do not
                // replace the original value supplied to independent callbacks.
                if !value_inputs.contains(&variable) {
                    variables.entry(variable).or_default().extend(sources);
                }
            }
        }
        None
    }

    /// Derive callback inputs from declaration slots and the checked value arguments.
    /// Keep templates unresolved: nominal unification does not identify value views.
    pub(super) fn callback_input_provenance(
        &self,
        parameters: &[Ty],
        arguments: &[Option<TypedNode>],
        callback_index: usize,
        span: &Span,
    ) -> Result<Option<Vec<Source>>, TypeError> {
        let Some(Ty::Func(inputs, _)) = parameters.get(callback_index) else {
            return Ok(Some(Vec::new()));
        };
        let mut value_variables = Vec::new();
        for parameter in parameters {
            if !matches!(parameter, Ty::Func(..)) {
                Self::collect_ty_vars(parameter, &mut value_variables);
            }
        }
        let mut required = Vec::new();
        for input in inputs {
            Self::collect_ty_vars(input, &mut required);
        }
        // Follow producer callback slots transitively before choosing the
        // argument-check order. The graph is declaration identity based.
        loop {
            let before = required.len();
            for (index, parameter) in parameters.iter().enumerate() {
                if index == callback_index {
                    continue;
                }
                if let Ty::Func(producer_inputs, output) = parameter {
                    let mut outputs = Vec::new();
                    Self::collect_ty_vars(output, &mut outputs);
                    if outputs
                        .iter()
                        .any(|id| required.contains(id) && !value_variables.contains(id))
                    {
                        let mut dependencies = Vec::new();
                        for input in producer_inputs {
                            Self::collect_ty_vars(input, &mut dependencies);
                        }
                        for dependency in dependencies {
                            if !required.contains(&dependency) {
                                required.push(dependency);
                            }
                        }
                    }
                }
            }
            if required.len() == before {
                break;
            }
        }
        let mut variables = HashMap::<u32, Vec<Source>>::new();
        let mut producers = Vec::new();
        for (index, (parameter, argument)) in parameters.iter().zip(arguments).enumerate() {
            if index == callback_index {
                continue;
            }
            let contribution = match parameter {
                Ty::Func(_, output) => output.as_ref(),
                other => other,
            };
            let mut contributed = Vec::new();
            Self::collect_ty_vars(contribution, &mut contributed);
            let needed = contributed.iter().any(|id| {
                required.contains(id)
                    && (!matches!(parameter, Ty::Func(..)) || !value_variables.contains(id))
            });
            if !needed {
                continue;
            }
            let Some(argument) = argument else {
                if needed {
                    return Ok(None);
                }
                continue;
            };
            let source = self.source_provenance(argument, &Bindings::new());
            if let Ty::Func(producer_inputs, output) = parameter {
                if needed {
                    producers.push((producer_inputs.as_slice(), output.as_ref(), source));
                }
            } else if let Some(outcome) =
                self.collect_provenance_variables(parameter, &source, &mut variables)
            {
                return Err(self.callback_provenance_projection_error(outcome, span));
            }
        }
        let value_inputs = variables.keys().copied().collect();
        if let Some(outcome) =
            self.collect_callback_output_provenance(producers, &mut variables, &value_inputs)
        {
            return Err(self.callback_provenance_projection_error(outcome, span));
        }
        let known = self.merged_provenance_variables(&variables);
        Ok(Some(
            inputs
                .iter()
                .map(|ty| self.template_provenance(ty, &self.resolve_ty(ty), &known))
                .collect(),
        ))
    }

    fn callback_provenance_projection_error(
        &self,
        outcome: ConstructorApplicationOutcome,
        span: &Span,
    ) -> TypeError {
        let outcome = match outcome.into_checked() {
            Ok(outcome) => outcome,
            Err(error) => return error.at_span(span),
        };
        let detail = match outcome {
            ConstructorApplicationOutcome::Deferred { .. } => {
                "Callback input provenance remains unresolved".to_string()
            }
            ConstructorApplicationOutcome::Rejected { failures } => {
                if Self::constructor_projection_failures_are_metadata(&failures) {
                    return signatures::constructor_signature_metadata_error(
                        "callback input",
                        span,
                        Self::constructor_projection_failure_detail(&failures),
                    );
                }
                "Callback input provenance does not satisfy the declared constructor".to_string()
            }
            ConstructorApplicationOutcome::Applied(_) => {
                "Unexpected successful callback input projection".to_string()
            }
        };
        TypeError {
            structured: None,
            message: detail,
            span: span.clone(),
            hint: None,
        }
    }

    pub(super) fn unresolved_callback_provenance_error(&self, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: "Callback input provenance has unresolved argument dependencies".into(),
            span: span.clone(),
            hint: Some(
                "Provide an explicit callback parameter type to establish its input contract."
                    .into(),
            ),
        }
    }

    pub(super) fn check_parameter_constructor_provenance(
        &self,
        parameter: &Ty,
        argument: &TypedNode,
        callable: &str,
    ) -> Result<(), TypeError> {
        self.check_parameter_constructor_source(
            parameter,
            &self.source_provenance(argument, &Bindings::new()),
            &argument.span,
            callable,
        )
    }

    fn check_source_constructor_capability(
        &self,
        required: &str,
        source: &Source,
        span: &Span,
        callable: &str,
    ) -> Result<(), TypeError> {
        let actual = self
            .constructor_capability_for_type(&source.1)
            .map(Provenance::constrained)
            .unwrap_or_else(|| source.0.clone());
        if let Some(outcome) = self.constructor_provenance_application_outcome(&actual) {
            self.require_constructor_projection_type(outcome, required, &source.1, span, callable)?;
        }
        if !self
            .constructor_provenance_allows(&actual, required, &source.1)
            .map_err(|error| error.at_span(span))?
        {
            return Err(self.trait_failure(
                TypeDiagnosticReason::MissingTypeConstructorCapability,
                required,
                &source.1,
                span,
                DiagnosticOrigin::Call,
            ));
        }
        Ok(())
    }

    fn check_parameter_constructor_source(
        &self,
        parameter: &Ty,
        source: &Source,
        span: &Span,
        callable: &str,
    ) -> Result<(), TypeError> {
        if let Some(required) = self.constructor_capability_for_type(parameter) {
            self.check_source_constructor_capability(&required, source, span, callable)?;
        }
        match parameter {
            Ty::SelfApp(items) => {
                if let Some((_, inputs)) = Self::constructor_application_parts(items) {
                    if let Some(capability) = self.constructor_capability_for_type(parameter) {
                        for (index, input) in inputs.iter().enumerate() {
                            self.check_parameter_constructor_source(
                                input,
                                &self.project_constructor_slot(source, &capability, index, input),
                                span,
                                callable,
                            )?;
                        }
                    }
                }
            }
            Ty::List(item) => self.check_parameter_constructor_source(
                item,
                &self.project_provenance(source, &Projection::Element, item),
                span,
                callable,
            )?,
            Ty::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.check_parameter_constructor_source(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            item,
                        ),
                        span,
                        callable,
                    )?;
                }
            }
            Ty::Result(ok, error) => {
                for (tag, item) in [(0, ok.as_ref()), (1, error.as_ref())] {
                    self.check_parameter_constructor_source(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field {
                                index: 0,
                                tag: Some(tag),
                            },
                            item,
                        ),
                        span,
                        callable,
                    )?;
                }
            }
            Ty::Enum(_, arguments) => {
                for (index, argument) in arguments.iter().enumerate() {
                    self.check_parameter_constructor_source(
                        argument,
                        &self.project_provenance(
                            source,
                            &Projection::TypeArgument(index),
                            argument,
                        ),
                        span,
                        callable,
                    )?;
                }
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for (index, (_, field)) in nominal.iter().enumerate() {
                    self.check_parameter_constructor_source(
                        field,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            field,
                        ),
                        span,
                        callable,
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A function value retains the capability requirements of its declaration
    /// even after its visible function type has been instantiated to a nominal type.
    pub(super) fn check_callable_input_provenance(
        &self,
        function: &TypedNode,
        arguments: &[TypedNode],
    ) -> Result<(), TypeError> {
        self.check_callable_source_inputs(
            &self.source_provenance(function, &Bindings::new()),
            &arguments
                .iter()
                .map(|arg| self.source_provenance(arg, &Bindings::new()))
                .collect::<Vec<_>>(),
            &function.span,
        )
    }

    fn check_callable_source_inputs(
        &self,
        function: &Source,
        arguments: &[Source],
        span: &Span,
    ) -> Result<(), TypeError> {
        match &function.0 {
            Provenance::DeclaredCallable(id) => {
                let signature = self.callable_signatures.get(id).ok_or_else(|| {
                    self.typecheck_invariant_error("Callable provenance has no declaration", span)
                })?;
                let trait_context = self
                    .traits
                    .iter()
                    .find(|(_, info)| {
                        !info.constructor_slots.is_empty()
                            && info
                                .methods
                                .values()
                                .any(|method| method.id.unique_id == *id)
                    })
                    .map(|(name, _)| name.as_str());
                let mut receiver = None;
                for (parameter, argument) in signature.value_parameters.iter().zip(arguments) {
                    self.check_parameter_constructor_source(
                        &parameter.ty,
                        argument,
                        span,
                        &signature.identity.name,
                    )?;
                    if let (Some(capability), Ty::SelfApp(items)) = (trait_context, &parameter.ty) {
                        if Self::constructor_application_parts(items).is_none() {
                            self.check_source_constructor_capability(
                                capability,
                                argument,
                                span,
                                &signature.identity.name,
                            )?;
                            receiver.get_or_insert(&argument.1);
                        }
                    }
                }
                let context = trait_context.zip(receiver);
                let parameters = signature
                    .value_parameters
                    .iter()
                    .map(|parameter| self.trait_provenance_template(&parameter.ty, context))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|outcome| self.callback_provenance_projection_error(outcome, span))?;
                self.check_callback_sources(&parameters, arguments, span)?;
                Ok(())
            }
            Provenance::Callable {
                parameters, calls, ..
            } => {
                let bindings = parameters
                    .iter()
                    .copied()
                    .zip(arguments.iter().cloned())
                    .collect();
                for (function, inputs) in calls {
                    self.check_callable_source_inputs(
                        &self.substitute_provenance(function, &bindings),
                        &inputs
                            .iter()
                            .map(|input| self.substitute_provenance(input, &bindings))
                            .collect::<Vec<_>>(),
                        span,
                    )?;
                }
                Ok(())
            }
            Provenance::Intersection(functions) => {
                for function in functions {
                    self.check_callable_source_inputs(function, arguments, span)?;
                }
                Ok(())
            }
            Provenance::Injected {
                function,
                arguments: tail,
            } => {
                let mut arguments = arguments.to_vec();
                arguments.extend(tail.iter().cloned());
                self.check_callable_source_inputs(function, &arguments, span)
            }
            Provenance::ConstrainedTemplate { source, .. } => {
                self.check_callable_source_inputs(source, arguments, span)
            }
            _ => Ok(()),
        }
    }

    fn has_callback_slot(template: &Ty) -> bool {
        match template {
            Ty::Func(..) | Ty::UserFunc { .. } | Ty::BuiltinFunc { .. } => true,
            Ty::Tuple(items) | Ty::Enum(_, items) | Ty::SelfApp(items) => {
                items.iter().any(Self::has_callback_slot)
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal
                .iter()
                .any(|(_, field)| Self::has_callback_slot(field)),
            Ty::List(item)
            | Ty::Lazy(item)
            | Ty::MatchResult(item)
            | Ty::ExtractorClosure(item) => Self::has_callback_slot(item),
            Ty::Result(ok, error) => Self::has_callback_slot(ok) || Self::has_callback_slot(error),
            _ => false,
        }
    }

    /// Check function-valued slots through the same structural projections used
    /// for return provenance. Captured and mapped slots retain distinct sources.
    pub(super) fn check_callback_argument_provenance(
        &self,
        parameters: &[Ty],
        arguments: &[TypedNode],
    ) -> Result<(), TypeError> {
        if !parameters.iter().any(Self::has_callback_slot) {
            return Ok(());
        }
        let sources = arguments
            .iter()
            .map(|arg| self.source_provenance(arg, &Bindings::new()))
            .collect::<Vec<_>>();
        let span = &arguments
            .first()
            .expect("a callback has a value argument")
            .span;
        self.check_callback_sources(parameters, &sources, span)
    }

    fn check_callback_sources(
        &self,
        parameters: &[Ty],
        sources: &[Source],
        span: &Span,
    ) -> Result<(), TypeError> {
        if !parameters.iter().any(Self::has_callback_slot) {
            return Ok(());
        }
        let mut variables = HashMap::new();
        if let Some(outcome) = self.collect_callable_provenance_variables(
            parameters,
            sources,
            &mut variables,
            CallbackProvenanceTarget::Inputs,
        ) {
            return Err(self.callback_provenance_projection_error(outcome, span));
        }
        let known = self.merged_provenance_variables(&variables);
        for (parameter, source) in parameters.iter().zip(sources) {
            self.check_callback_slot_provenance(parameter, source, &known, span)?;
        }
        Ok(())
    }

    fn check_callback_slot_provenance(
        &self,
        template: &Ty,
        source: &Source,
        variables: &Bindings,
        span: &Span,
    ) -> Result<(), TypeError> {
        if !Self::has_callback_slot(template) {
            return Ok(());
        }
        match template {
            Ty::Func(inputs, _) => {
                let inputs = inputs
                    .iter()
                    .map(|ty| self.template_provenance(ty, &self.resolve_ty(ty), variables))
                    .collect::<Vec<_>>();
                self.check_callable_source_inputs(source, &inputs, span)
            }
            Ty::SelfApp(items) => {
                if let Some((_, slots)) = Self::constructor_application_parts(items) {
                    let capability =
                        self.constructor_capability_for_type(template)
                            .ok_or_else(|| {
                                self.typecheck_invariant_error(
                                    "Constructor callback slot lacks a declared capability",
                                    span,
                                )
                            })?;
                    for (index, slot) in slots.iter().enumerate() {
                        self.check_callback_slot_provenance(
                            slot,
                            &self.project_constructor_slot(source, &capability, index, slot),
                            variables,
                            span,
                        )?;
                    }
                    Ok(())
                } else {
                    Err(self.typecheck_invariant_error(
                        "Constructor callback slot lacks its receiver context",
                        span,
                    ))
                }
            }
            Ty::Result(ok, error) => {
                for (tag, item) in [(0, ok.as_ref()), (1, error.as_ref())] {
                    let projected = self.project_provenance(
                        source,
                        &Projection::Field {
                            index: 0,
                            tag: Some(tag),
                        },
                        item,
                    );
                    self.check_callback_slot_provenance(item, &projected, variables, span)?;
                }
                Ok(())
            }
            Ty::List(item) => self.check_callback_slot_provenance(
                item,
                &self.project_provenance(source, &Projection::Element, item),
                variables,
                span,
            ),
            Ty::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.check_callback_slot_provenance(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            item,
                        ),
                        variables,
                        span,
                    )?;
                }
                Ok(())
            }
            Ty::Enum(_, items) => {
                for (index, item) in items.iter().enumerate() {
                    self.check_callback_slot_provenance(
                        item,
                        &self.project_provenance(source, &Projection::TypeArgument(index), item),
                        variables,
                        span,
                    )?;
                }
                Ok(())
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for (index, (_, item)) in nominal.iter().enumerate() {
                    self.check_callback_slot_provenance(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            item,
                        ),
                        variables,
                        span,
                    )?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn invoke_provenance(&self, function: &Source, arguments: &[Source], actual: &Ty) -> Source {
        match &function.0 {
            Provenance::Injected {
                function,
                arguments: tail,
            } => {
                if arguments.len() != 1 {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                }
                let mut arguments = arguments.to_vec();
                arguments.extend(tail.iter().cloned());
                self.invoke_provenance(function, &arguments, actual)
            }
            Provenance::Intersection(functions) => {
                let results = functions
                    .iter()
                    .map(|function| self.invoke_provenance(function, arguments, actual))
                    .collect::<Vec<_>>();
                (self.common_constructor_provenance(&results), actual.clone())
            }
            Provenance::Callable {
                parameters, result, ..
            } => {
                if parameters.len() != arguments.len() {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                }
                self.substitute_provenance(
                    result,
                    &parameters
                        .iter()
                        .copied()
                        .zip(arguments.iter().cloned())
                        .collect(),
                )
            }
            Provenance::DeclaredCallable(id) => {
                let Some(signature) = self.callable_signatures.get(id) else {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                };
                let context = self
                    .traits
                    .iter()
                    .find(|(_, info)| {
                        info.methods
                            .values()
                            .any(|method| method.id.unique_id == *id)
                    })
                    .and_then(|(name, _)| {
                        let receiver = signature.value_parameters.iter().zip(arguments).find_map(
                            |(parameter, source)| {
                                let is_declared_self_application = matches!(
                                    &parameter.ty,
                                    Ty::SelfApp(items)
                                        if Self::constructor_application_parts(items).is_none()
                                );
                                let is_matching_constructor_application = self
                                    .constructor_capability_for_type(&parameter.ty)
                                    .is_some_and(|capability| {
                                        self.constructor_capability_allows(
                                            &capability,
                                            name,
                                            &mut HashSet::new(),
                                        )
                                    });
                                (is_declared_self_application
                                    || is_matching_constructor_application)
                                    .then_some(&source.1)
                            },
                        );
                        Some((name.as_str(), receiver.unwrap_or(actual)))
                    });
                let parameters = signature
                    .value_parameters
                    .iter()
                    .map(|parameter| self.trait_provenance_template(&parameter.ty, context))
                    .collect::<Result<Vec<_>, _>>();
                let parameters = match parameters {
                    Ok(parameters) => parameters,
                    Err(outcome) => {
                        return (Provenance::ConstructorApplication(outcome), actual.clone());
                    }
                };
                let mut return_type =
                    match self.trait_provenance_template(&signature.return_type.ty, context) {
                        Ok(return_type) => return_type,
                        Err(outcome) => {
                            return (Provenance::ConstructorApplication(outcome), actual.clone());
                        }
                    };
                let mut variables = HashMap::<u32, Vec<Source>>::new();
                if let Some(outcome) = self.collect_callable_provenance_variables(
                    &parameters,
                    arguments,
                    &mut variables,
                    CallbackProvenanceTarget::Result,
                ) {
                    return (Provenance::ConstructorApplication(outcome), actual.clone());
                }
                // Value arguments remain the authoritative provenance source.
                // Return-only inputs have no value position to contribute one,
                // so retain their instantiated call type without replacing any
                // source view already collected above.
                if let Ty::UserFunc {
                    call_substitution, ..
                } = &function.1
                {
                    let mut value_inputs = Vec::new();
                    for parameter in &signature.value_parameters {
                        Self::collect_ty_vars(&parameter.ty, &mut value_inputs);
                    }
                    // Only constructor identities supplied without a value
                    // source are concrete here. Payload variables and returned
                    // callable inputs must retain their source projections.
                    let return_only_mapping: HashMap<_, _> = call_substitution
                        .iter()
                        .filter(|(variable, _)| {
                            self.constructor_witness_traits.contains_key(variable)
                                && !value_inputs.contains(variable)
                        })
                        .cloned()
                        .collect();
                    if !return_only_mapping.is_empty() {
                        return_type =
                            self.substitute_type_def_ty(&return_type, &return_only_mapping);
                    }
                    for (variable, ty) in call_substitution {
                        variables.entry(*variable).or_insert_with(|| {
                            vec![(
                                self.constructor_witness_traits
                                    .get(variable)
                                    .cloned()
                                    .map(Provenance::constrained)
                                    .unwrap_or(Provenance::RequiresProof),
                                self.resolve_ty(ty),
                            )]
                        });
                    }
                }
                self.template_provenance(
                    &return_type,
                    actual,
                    &self.merged_provenance_variables(&variables),
                )
            }
            Provenance::Template { ty, variables } => {
                let Some((parameters, result)) = self.function_parts(ty) else {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                };
                if parameters.len() != arguments.len() {
                    return (Provenance::Intersection(Vec::new()), actual.clone());
                }
                let mut sources = variables
                    .iter()
                    .map(|(variable, source)| (*variable, vec![source.clone()]))
                    .collect::<HashMap<_, _>>();
                if let Some(outcome) = self.collect_callable_provenance_variables(
                    parameters,
                    arguments,
                    &mut sources,
                    CallbackProvenanceTarget::Result,
                ) {
                    return (Provenance::ConstructorApplication(outcome), actual.clone());
                }
                self.template_provenance(
                    result,
                    actual,
                    &self.merged_provenance_variables(&sources),
                )
            }
            Provenance::Parameter(_) | Provenance::Projection { .. } => (
                Provenance::Call {
                    function: Box::new(function.clone()),
                    arguments: arguments.to_vec(),
                },
                actual.clone(),
            ),
            Provenance::RequiresProof => self
                .function_parts(&function.1)
                .map(|(_, result)| self.template_provenance(result, actual, &Bindings::new()))
                .unwrap_or_else(|| (Provenance::Intersection(Vec::new()), actual.clone())),
            _ => (Provenance::Intersection(Vec::new()), actual.clone()),
        }
    }

    pub(super) fn constructor_capability_for_type(&self, ty: &Ty) -> Option<String> {
        let Ty::SelfApp(items) = ty else {
            return None;
        };
        let (Ty::Var(variable), _) = Self::constructor_application_parts(items)? else {
            return None;
        };
        self.constructor_witness_traits.get(variable).cloned()
    }

    pub(super) fn common_constructor_provenance(&self, provenances: &[Source]) -> Provenance {
        let mut sources = Vec::new();
        for (provenance, ty) in provenances {
            let leaves = match provenance {
                Provenance::Intersection(nested) => nested.clone(),
                _ => vec![(provenance.clone(), ty.clone())],
            };
            for source in leaves {
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
        }
        Provenance::Intersection(sources)
    }

    pub(super) fn constructor_provenance_application_outcome(
        &self,
        provenance: &Provenance,
    ) -> Option<ConstructorApplicationOutcome> {
        let from_source =
            |source: &Source| self.constructor_provenance_application_outcome(&source.0);
        match provenance {
            Provenance::ConstructorApplication(outcome) => Some(outcome.clone()),
            Provenance::Intersection(sources)
            | Provenance::Fields(sources)
            | Provenance::Sequence(sources) => sources.iter().find_map(from_source),
            Provenance::Variants(variants) => variants
                .iter()
                .flat_map(|(_, sources)| sources)
                .find_map(from_source),
            Provenance::Callable { result, .. } => from_source(result),
            Provenance::Injected {
                function,
                arguments,
            }
            | Provenance::Call {
                function,
                arguments,
            } => from_source(function).or_else(|| arguments.iter().find_map(from_source)),
            Provenance::Projection { source, .. } => from_source(source),
            Provenance::Template { variables, .. } => variables.values().find_map(from_source),
            Provenance::RequiresProof
            | Provenance::Constrained(_)
            | Provenance::ConstrainedTemplate { .. }
            | Provenance::Parameter(_)
            | Provenance::DeclaredCallable(_) => None,
        }
    }

    pub(super) fn constructor_provenance_allows(
        &self,
        provenance: &Provenance,
        required: &str,
        actual_ty: &Ty,
    ) -> Result<bool, TypeError> {
        Ok(match provenance {
            Provenance::Constrained(capabilities) => capabilities.iter().any(|actual| {
                self.constructor_capability_allows(actual, required, &mut HashSet::new())
            }),
            Provenance::ConstrainedTemplate { capabilities, .. } => {
                capabilities.iter().any(|actual| {
                    self.constructor_capability_allows(actual, required, &mut HashSet::new())
                })
            }
            Provenance::Intersection(sources) => {
                !sources.is_empty()
                    && try_all(sources.iter(), |(source, ty)| {
                        self.constructor_provenance_allows(source, required, ty)
                    })?
            }
            Provenance::Template { ty, .. }
                if self.constructor_capability_for_type(ty).is_some() =>
            {
                let capability = self
                    .constructor_capability_for_type(ty)
                    .expect("checked constructor template");
                self.constructor_capability_allows(&capability, required, &mut HashSet::new())
            }
            Provenance::Template { ty, variables } => {
                let mapping = variables
                    .iter()
                    .map(|(variable, source)| (*variable, source.1.clone()))
                    .collect();
                let declared = self.substitute_ty_with_mapping(ty, &mapping);
                matches!(
                    self.constructor_projection(required, &declared)
                        .into_checked()?,
                    ConstructorProjectionOutcome::Applicable { .. }
                )
            }
            Provenance::Parameter(_) | Provenance::Projection { .. } | Provenance::Call { .. } => {
                false
            }
            Provenance::ConstructorApplication(outcome) => {
                outcome.clone().into_checked()?;
                false
            }
            _ => {
                if let Some(capability) = self.constructor_capability_for_type(actual_ty) {
                    self.constructor_capability_allows(&capability, required, &mut HashSet::new())
                } else {
                    matches!(
                        self.constructor_projection(required, actual_ty)
                            .into_checked()?,
                        ConstructorProjectionOutcome::Applicable { .. }
                    )
                }
            }
        })
    }

    fn template_provenance(&self, template: &Ty, actual: &Ty, variables: &Bindings) -> Source {
        if let Ty::Var(variable) = template {
            if let Some(source) = variables.get(variable) {
                return source.clone();
            }
        }
        (
            Provenance::Template {
                ty: template.clone(),
                variables: variables.clone(),
            },
            actual.clone(),
        )
    }

    fn substitute_provenance(&self, source: &Source, bindings: &Bindings) -> Source {
        let substitute = |source: &Source| self.substitute_provenance(source, bindings);
        let provenance = match &source.0 {
            Provenance::Injected {
                function,
                arguments,
            } => Provenance::Injected {
                function: Box::new(substitute(function)),
                arguments: arguments.iter().map(substitute).collect(),
            },
            Provenance::Call {
                function,
                arguments,
            } => {
                return self.invoke_provenance(
                    &substitute(function),
                    &arguments.iter().map(substitute).collect::<Vec<_>>(),
                    &source.1,
                );
            }
            Provenance::Parameter(variable) => {
                return bindings
                    .get(variable)
                    .cloned()
                    .unwrap_or_else(|| source.clone());
            }
            Provenance::Projection {
                source: projected,
                projection,
            } => return self.project_provenance(&substitute(projected), projection, &source.1),
            Provenance::Intersection(sources) => self
                .common_constructor_provenance(&sources.iter().map(substitute).collect::<Vec<_>>()),
            Provenance::Fields(fields) => {
                Provenance::Fields(fields.iter().map(substitute).collect())
            }
            Provenance::Sequence(elements) => {
                Provenance::Sequence(elements.iter().map(substitute).collect())
            }
            Provenance::Variants(variants) => Provenance::Variants(
                variants
                    .iter()
                    .map(|(tag, fields)| (*tag, fields.iter().map(substitute).collect()))
                    .collect(),
            ),
            Provenance::Callable {
                parameters,
                result,
                calls,
            } => Provenance::Callable {
                parameters: parameters.clone(),
                result: Box::new(substitute(result)),
                calls: calls
                    .iter()
                    .map(|(function, arguments)| {
                        (
                            substitute(function),
                            arguments.iter().map(substitute).collect(),
                        )
                    })
                    .collect(),
            },
            Provenance::Template { ty, variables } => Provenance::Template {
                ty: ty.clone(),
                variables: variables
                    .iter()
                    .map(|(variable, source)| (*variable, substitute(source)))
                    .collect(),
            },
            Provenance::ConstrainedTemplate {
                capabilities,
                source: projected,
            } => Provenance::ConstrainedTemplate {
                capabilities: capabilities.clone(),
                source: Box::new(substitute(projected)),
            },
            other => other.clone(),
        };
        (provenance, source.1.clone())
    }

    fn project_constructor_slot(
        &self,
        source: &Source,
        capability: &str,
        index: usize,
        expected: &Ty,
    ) -> Source {
        if let Provenance::ConstrainedTemplate { source, .. } = &source.0 {
            return self.project_constructor_slot(source, capability, index, expected);
        }
        if let Provenance::Intersection(sources) = &source.0 {
            let projected = sources
                .iter()
                .map(|source| self.project_constructor_slot(source, capability, index, expected))
                .collect::<Vec<_>>();
            return (
                self.common_constructor_provenance(&projected),
                self.resolve_ty(expected),
            );
        }
        if let Ty::SelfApp(items) = self.resolve_ty(&source.1) {
            if let Some((_, slots)) = Self::constructor_application_parts(&items) {
                if let Some(actual) = slots.get(index) {
                    if let Provenance::Template {
                        ty: Ty::SelfApp(template),
                        variables,
                    } = &source.0
                    {
                        if let Some((_, template_slots)) =
                            Self::constructor_application_parts(template)
                        {
                            if let Some(template) = template_slots.get(index) {
                                return self.template_provenance(template, actual, variables);
                            }
                        }
                    }
                    return match source.0 {
                        Provenance::Parameter(_) | Provenance::Projection { .. } => (
                            Provenance::Projection {
                                source: Box::new(source.clone()),
                                projection: Projection::ConstructorSlot {
                                    capability: capability.into(),
                                    index,
                                },
                            },
                            actual.clone(),
                        ),
                        _ => (Provenance::RequiresProof, actual.clone()),
                    };
                }
            }
        }
        let (implementation, _) =
            match self.constructor_projection(capability, &self.resolve_ty(&source.1)) {
                ConstructorProjectionOutcome::Applicable { info, mapping } => (info, mapping),
                ConstructorProjectionOutcome::Deferred { waiting_on } => {
                    return (
                        Provenance::ConstructorApplication(
                            ConstructorApplicationOutcome::Deferred { waiting_on },
                        ),
                        expected.clone(),
                    );
                }
                ConstructorProjectionOutcome::Rejected { failures } => {
                    return (
                        Provenance::ConstructorApplication(
                            ConstructorApplicationOutcome::Rejected { failures },
                        ),
                        expected.clone(),
                    );
                }
            };
        let Some(variable) = implementation.constructor_slot_vars.get(index) else {
            return (
                Provenance::ConstructorApplication(ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                        expected: index + 1,
                        actual: implementation.constructor_slot_vars.len(),
                    }],
                }),
                expected.clone(),
            );
        };
        let mut variables = HashMap::new();
        if let Some(outcome) =
            self.collect_provenance_variables(&implementation.target_ty, source, &mut variables)
        {
            return (
                Provenance::ConstructorApplication(outcome),
                expected.clone(),
            );
        }
        self.merged_provenance_variables(&variables)
            .remove(variable)
            .unwrap_or_else(|| (Provenance::RequiresProof, self.resolve_ty(expected)))
    }

    fn project_provenance(
        &self,
        source: &Source,
        projection: &Projection,
        expected: &Ty,
    ) -> Source {
        if let Projection::ConstructorSlot { capability, index } = projection {
            return self.project_constructor_slot(source, capability, *index, expected);
        }
        let actual = self
            .projection_type(&source.1, projection)
            .unwrap_or_else(|| expected.clone());
        match &source.0 {
            Provenance::ConstrainedTemplate {
                source: projected, ..
            } => self.project_provenance(projected, projection, &actual),
            Provenance::Intersection(sources) => {
                let sources = sources
                    .iter()
                    .map(|source| self.project_provenance(source, projection, &actual))
                    .collect::<Vec<_>>();
                (self.common_constructor_provenance(&sources), actual)
            }
            Provenance::Fields(fields) => match projection {
                Projection::Field { index, .. } => fields
                    .get(*index)
                    .cloned()
                    .unwrap_or((Provenance::RequiresProof, actual)),
                _ => (Provenance::RequiresProof, actual),
            },
            Provenance::Variants(variants) => match projection {
                Projection::Field { index, tag } => {
                    let fields = variants
                        .iter()
                        .filter(|(candidate, _)| tag.is_none_or(|tag| tag == *candidate))
                        .filter_map(|(_, fields)| fields.get(*index).cloned())
                        .collect::<Vec<_>>();
                    if fields.is_empty() {
                        (Provenance::RequiresProof, actual)
                    } else {
                        (self.common_constructor_provenance(&fields), actual)
                    }
                }
                Projection::TypeArgument(index) => {
                    let Ty::Enum(name, _) = &source.1 else {
                        return (Provenance::RequiresProof, actual);
                    };
                    let Some(metadata) = self.env.enum_variants_of(name) else {
                        return (Provenance::RequiresProof, actual);
                    };
                    let mut collected = HashMap::<u32, Vec<Source>>::new();
                    let mut sources = Vec::new();
                    for (tag, fields) in variants {
                        let Some(variant) = metadata.iter().find(|variant| variant.tag == *tag)
                        else {
                            continue;
                        };
                        let Ty::Enum(_, arguments) = &variant.enum_ty else {
                            continue;
                        };
                        let Some(Ty::Var(variable)) = arguments.get(*index) else {
                            continue;
                        };
                        for (template, field) in variant.payload.iter().zip(fields.iter().skip(1)) {
                            if let Some(outcome) =
                                self.collect_provenance_variables(template, field, &mut collected)
                            {
                                return (Provenance::ConstructorApplication(outcome), actual);
                            }
                        }
                        if let Some(found) = collected.remove(variable) {
                            sources.extend(found);
                        }
                    }
                    if sources.is_empty() {
                        (Provenance::RequiresProof, actual)
                    } else {
                        (self.common_constructor_provenance(&sources), actual)
                    }
                }
                _ => (Provenance::RequiresProof, actual),
            },
            Provenance::Sequence(elements)
                if matches!(
                    projection,
                    Projection::Element | Projection::TypeArgument(_)
                ) =>
            {
                if elements.is_empty() {
                    (Provenance::RequiresProof, actual)
                } else {
                    (self.common_constructor_provenance(elements), actual)
                }
            }
            Provenance::Template { ty, variables } => {
                let concrete = match ty {
                    Ty::SelfApp(items) if Self::constructor_application_parts(items).is_some() => {
                        match self.concrete_template(ty, &source.1) {
                            ConstructorApplicationOutcome::Applied(concrete) => concrete,
                            outcome @ (ConstructorApplicationOutcome::Deferred { .. }
                            | ConstructorApplicationOutcome::Rejected { .. }) => {
                                return (Provenance::ConstructorApplication(outcome), actual);
                            }
                        }
                    }
                    _ => ty.clone(),
                };
                if let Some(template) = self.projection_type(&concrete, projection) {
                    self.template_provenance(&template, &actual, variables)
                } else {
                    (Provenance::RequiresProof, actual)
                }
            }
            Provenance::Parameter(_) | Provenance::Projection { .. } => (
                Provenance::Projection {
                    source: Box::new(source.clone()),
                    projection: projection.clone(),
                },
                actual,
            ),
            _ => (Provenance::RequiresProof, actual),
        }
    }

    fn projection_type(&self, ty: &Ty, projection: &Projection) -> Option<Ty> {
        match (ty, projection) {
            (Ty::Tuple(items), Projection::Field { index, .. }) => items.get(*index).cloned(),
            (Ty::Struct(_, fields) | Ty::Record(_, fields), Projection::Field { index, .. }) => {
                fields.get(*index).map(|(_, ty)| ty.clone())
            }
            (Ty::Struct(_, nominal) | Ty::Record(_, nominal), Projection::TypeArgument(index)) => {
                nominal.arguments.get(*index).cloned()
            }
            (Ty::List(element), Projection::Element) => Some(element.as_ref().clone()),
            (
                Ty::MatchResult(payload),
                Projection::Field {
                    index: 1,
                    tag: Some(tag),
                },
            ) => {
                let variant = self
                    .lookup_enum_variants_of("MatchResult")?
                    .iter()
                    .find(|variant| variant.tag == *tag)?;
                match variant.short_name.as_str() {
                    "Ok" => Some(payload.as_ref().clone()),
                    "Err" => Some(Ty::Error),
                    _ => None,
                }
            }
            (Ty::Result(ok, error), Projection::Field { index: 0, tag }) => {
                Some(if *tag == Some(1) { error } else { ok }.as_ref().clone())
            }
            (Ty::Enum(_, arguments), Projection::TypeArgument(index)) => {
                arguments.get(*index).cloned()
            }
            (
                Ty::Enum(name, arguments),
                Projection::Field {
                    index,
                    tag: Some(tag),
                },
            ) => {
                let variant = self
                    .env
                    .enum_variants_of(name)?
                    .iter()
                    .find(|variant| variant.tag == *tag)?;
                if *index == 0 {
                    return Some(Ty::Int);
                }
                let Ty::Enum(_, parameters) = &variant.enum_ty else {
                    return None;
                };
                let mapping = parameters
                    .iter()
                    .zip(arguments)
                    .filter_map(|(parameter, argument)| {
                        if let Ty::Var(variable) = parameter {
                            Some((*variable, argument.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();
                variant
                    .payload
                    .get(index - 1)
                    .map(|payload| self.substitute_ty_with_mapping(payload, &mapping))
            }
            _ => None,
        }
    }

    fn concrete_template(&self, template: &Ty, actual: &Ty) -> ConstructorApplicationOutcome {
        let Ty::SelfApp(items) = template else {
            return ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::UnsupportedConstructor],
            };
        };
        let Some((_, arguments)) = Self::constructor_application_parts(items) else {
            return ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::UnsupportedConstructor],
            };
        };
        let Some(capability) = self.constructor_capability_for_type(template) else {
            return ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::MissingWitnessTrait],
            };
        };
        self.expand_constructor_template(&capability, actual, arguments)
    }

    fn expand_constructor_template(
        &self,
        capability: &str,
        actual: &Ty,
        arguments: &[Ty],
    ) -> ConstructorApplicationOutcome {
        if let Ty::SelfApp(items) = self.resolve_ty(actual) {
            if let Some((witness, slots)) = Self::constructor_application_parts(&items) {
                if self
                    .constructor_capability_for_type(&Ty::SelfApp(items.clone()))
                    .is_some_and(|source| {
                        self.constructor_capability_allows(&source, capability, &mut HashSet::new())
                    })
                {
                    if slots.len() != arguments.len() {
                        return ConstructorApplicationOutcome::Rejected {
                            failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                                expected: slots.len(),
                                actual: arguments.len(),
                            }],
                        };
                    }
                    let mut application = vec![Ty::Hole, witness.clone()];
                    application.extend(arguments.iter().cloned());
                    return ConstructorApplicationOutcome::Applied(Ty::SelfApp(application));
                }
            }
        }
        let (implementation, mut mapping) =
            match self.constructor_projection(capability, &self.resolve_ty(actual)) {
                ConstructorProjectionOutcome::Applicable { info, mapping } => (info, mapping),
                ConstructorProjectionOutcome::Deferred { waiting_on } => {
                    return ConstructorApplicationOutcome::Deferred { waiting_on };
                }
                ConstructorProjectionOutcome::Rejected { failures } => {
                    return ConstructorApplicationOutcome::Rejected { failures };
                }
            };
        if implementation.constructor_slot_vars.len() != arguments.len() {
            return ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                    expected: implementation.constructor_slot_vars.len(),
                    actual: arguments.len(),
                }],
            };
        };
        for (variable, argument) in implementation.constructor_slot_vars.iter().zip(arguments) {
            mapping.insert(*variable, argument.clone());
        }
        let Some(target) = implementation
            .head_type_list
            .entries
            .iter()
            .find(|entry| entry.role == TypeListRole::ImplTarget)
        else {
            return ConstructorApplicationOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::MissingImplTargetMetadata],
            };
        };
        let target = match self.canonical_to_ty(&target.ty) {
            Ok(target) => target,
            Err(_) => {
                return ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::Canonicalization],
                };
            }
        };
        ConstructorApplicationOutcome::Applied(self.substitute_ty_with_mapping(&target, &mapping))
    }

    fn collect_provenance_variables(
        &self,
        template: &Ty,
        source: &Source,
        variables: &mut HashMap<u32, Vec<Source>>,
    ) -> Option<ConstructorApplicationOutcome> {
        match template {
            Ty::SelfApp(items) if Self::constructor_application_parts(items).is_some() => {
                if let Ty::SelfApp(actual) = self.resolve_ty(&source.1) {
                    if let Some((_, actual_slots)) = Self::constructor_application_parts(&actual) {
                        let (_, template_slots) = Self::constructor_application_parts(items)
                            .expect("guarded constructor application");
                        if template_slots.len() != actual_slots.len() {
                            return Some(ConstructorApplicationOutcome::Rejected {
                                failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                                    expected: template_slots.len(),
                                    actual: actual_slots.len(),
                                }],
                            });
                        }
                        let Some(capability) = self.constructor_capability_for_type(template)
                        else {
                            return Some(ConstructorApplicationOutcome::Rejected {
                                failures: vec![ConstructorProjectionFailure::MissingWitnessTrait],
                            });
                        };
                        for (index, (template, actual)) in
                            template_slots.iter().zip(actual_slots).enumerate()
                        {
                            let projected =
                                self.project_constructor_slot(source, &capability, index, actual);
                            if let Some(outcome) =
                                self.collect_provenance_variables(template, &projected, variables)
                            {
                                return Some(outcome);
                            }
                        }
                        return None;
                    }
                }
                match self.concrete_template(template, &source.1) {
                    ConstructorApplicationOutcome::Applied(concrete) => {
                        self.collect_provenance_variables(&concrete, source, variables)
                    }
                    outcome @ (ConstructorApplicationOutcome::Deferred { .. }
                    | ConstructorApplicationOutcome::Rejected { .. }) => Some(outcome),
                }
            }
            Ty::SelfApp(_) => None,
            Ty::Var(variable) => {
                variables.entry(*variable).or_default().push(source.clone());
                None
            }
            Ty::List(element) => self.collect_provenance_variables(
                element,
                &self.project_provenance(source, &Projection::Element, element),
                variables,
            ),
            Ty::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    if let Some(outcome) = self.collect_provenance_variables(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            item,
                        ),
                        variables,
                    ) {
                        return Some(outcome);
                    }
                }
                None
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for (index, argument) in nominal.arguments.iter().enumerate() {
                    if let Some(outcome) = self.collect_provenance_variables(
                        argument,
                        &self.project_provenance(
                            source,
                            &Projection::TypeArgument(index),
                            argument,
                        ),
                        variables,
                    ) {
                        return Some(outcome);
                    }
                }
                for (index, (_, item)) in nominal.iter().enumerate() {
                    if let Some(outcome) = self.collect_provenance_variables(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            item,
                        ),
                        variables,
                    ) {
                        return Some(outcome);
                    }
                }
                None
            }
            Ty::Result(ok, error) => {
                for (tag, item) in [(0, ok.as_ref()), (1, error.as_ref())] {
                    if let Some(outcome) = self.collect_provenance_variables(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field {
                                index: 0,
                                tag: Some(tag),
                            },
                            item,
                        ),
                        variables,
                    ) {
                        return Some(outcome);
                    }
                }
                None
            }
            Ty::Enum(_, arguments) => {
                for (index, argument) in arguments.iter().enumerate() {
                    if let Some(outcome) = self.collect_provenance_variables(
                        argument,
                        &self.project_provenance(
                            source,
                            &Projection::TypeArgument(index),
                            argument,
                        ),
                        variables,
                    ) {
                        return Some(outcome);
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub(super) fn bind_constructor_provenance(&mut self, pattern: &TypedPattern, source: Source) {
        let mut bindings = Bindings::new();
        self.pattern_provenance_bindings(pattern, &source, &mut bindings);
        for (id, (provenance, _)) in bindings {
            self.constructor_capabilities.insert(id, provenance);
        }
    }

    pub(super) fn bind_match_constructor_provenance(
        &mut self,
        pattern: &TypedMatchPattern,
        source: Source,
    ) {
        let mut bindings = Bindings::new();
        self.match_provenance_bindings(pattern, &source, &mut bindings);
        for (id, (provenance, _)) in bindings {
            self.constructor_capabilities.insert(id, provenance);
        }
    }

    pub(super) fn result_constructor_provenance(&self, node: &TypedNode) -> Source {
        self.safebind_source_provenance(&self.source_provenance(node, &Bindings::new()))
    }

    fn safebind_source_provenance(&self, source: &Source) -> Source {
        if matches!(self.resolve_ty(&source.1), Ty::Result(..)) {
            self.project_provenance(
                source,
                &Projection::Field {
                    index: 0,
                    tag: Some(0),
                },
                &Ty::Hole,
            )
        } else {
            source.clone()
        }
    }

    fn extractor_payload_provenance(
        &self,
        extractor: &ResolvedId,
        extractor_ty: &Ty,
        pre_args: &[TypedNode],
        success_tag: u32,
        seq_tys: &[Ty],
        source: &Source,
        bindings: &Bindings,
    ) -> Vec<Source> {
        let function = TypedNode {
            ty: extractor_ty.clone(),
            span: extractor.span.clone(),
            node: TypedInner::Var(extractor.clone()),
        };
        let callable = match extractor_ty {
            Ty::UserFunc { .. } | Ty::BuiltinFunc { .. } => {
                // Named Extractors retain their original polymorphic declaration
                // in the environment, independently of this observed call type.
                let declared = self
                    .env
                    .lookup_var(extractor.unique_id)
                    .expect("typed named Extractor must retain its declaration");
                (
                    Provenance::Template {
                        ty: declared.clone(),
                        variables: Bindings::new(),
                    },
                    extractor_ty.clone(),
                )
            }
            Ty::ExtractorClosure(_) => self.source_provenance(&function, bindings),
            _ => unreachable!("typed Extractor must be named or an ExtractorClosure"),
        };
        let mut arguments = pre_args
            .iter()
            .map(|argument| self.source_provenance(argument, bindings))
            .collect::<Vec<_>>();
        arguments.push(source.clone());
        let (_, result_ty) = self
            .function_parts(Self::extractor_signature_ty(extractor_ty))
            .expect("typed Extractor must retain its callable signature");
        let Ty::MatchResult(payload_ty) = self.resolve_ty(result_ty) else {
            unreachable!("typed Extractor must return MatchResult")
        };
        let result = self.invoke_provenance(&callable, &arguments, result_ty);
        // MatchResult uses the ordinary enum layout: tag then success payload.
        let payload = self.project_provenance(
            &result,
            &Projection::Field {
                index: 1,
                tag: Some(success_tag),
            },
            &payload_ty,
        );
        if matches!(payload_ty.as_ref(), Ty::Tuple(_)) {
            seq_tys
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    self.project_provenance(&payload, &Projection::Field { index, tag: None }, ty)
                })
                .collect()
        } else {
            assert!(
                seq_tys.len() <= 1,
                "scalar Extractor must have one success slot"
            );
            seq_tys.iter().map(|_| payload.clone()).collect()
        }
    }

    fn pattern_provenance_bindings(
        &self,
        pattern: &TypedPattern,
        source: &Source,
        bindings: &mut Bindings,
    ) {
        match pattern {
            TypedPattern::Located(_, inner) => {
                self.pattern_provenance_bindings(inner, source, bindings)
            }
            TypedPattern::Var(_, id) => {
                bindings.insert(id.unique_id, source.clone());
            }
            TypedPattern::As(_, inner, id) => {
                bindings.insert(id.unique_id, source.clone());
                self.pattern_provenance_bindings(inner, source, bindings);
            }
            TypedPattern::Tuple(_, items) => {
                for (index, item) in items.iter().enumerate() {
                    self.pattern_provenance_bindings(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            &Ty::Hole,
                        ),
                        bindings,
                    );
                }
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.pattern_provenance_bindings(
                    head,
                    &self.project_provenance(source, &Projection::Element, &Ty::Hole),
                    bindings,
                );
                self.pattern_provenance_bindings(tail, source, bindings);
            }
            TypedPattern::Constructor {
                tag,
                fields,
                field_offset,
                ..
            } => {
                for (index, field) in fields.iter().enumerate() {
                    self.pattern_provenance_bindings(
                        field,
                        &self.project_provenance(
                            source,
                            &Projection::Field {
                                index: *field_offset as usize + index,
                                tag: Some(*tag),
                            },
                            &Ty::Hole,
                        ),
                        bindings,
                    );
                }
            }
            TypedPattern::Extractor {
                extractor,
                extractor_ty,
                pre_args,
                success_tag,
                seq_tys,
                items,
                ..
            } => {
                let payloads = self.extractor_payload_provenance(
                    extractor,
                    extractor_ty,
                    pre_args,
                    *success_tag,
                    seq_tys,
                    source,
                    bindings,
                );
                for (item, payload) in items.iter().zip(payloads) {
                    self.pattern_provenance_bindings(item, &payload, bindings);
                }
            }
            _ => {}
        }
    }

    fn match_provenance_bindings(
        &self,
        pattern: &TypedMatchPattern,
        source: &Source,
        bindings: &mut Bindings,
    ) {
        match pattern {
            TypedMatchPattern::Binding(id) => {
                bindings.insert(id.unique_id, source.clone());
            }
            TypedMatchPattern::As(inner, id) => {
                bindings.insert(id.unique_id, source.clone());
                self.match_provenance_bindings(inner, source, bindings);
            }
            TypedMatchPattern::Or(items) => {
                let alternatives = items
                    .iter()
                    .map(|item| {
                        let mut local = bindings.clone();
                        self.match_provenance_bindings(item, source, &mut local);
                        local
                    })
                    .collect::<Vec<_>>();
                let Some(first) = alternatives.first() else {
                    return;
                };
                let binding_ids = first
                    .keys()
                    .filter(|id| !bindings.contains_key(id))
                    .copied()
                    .collect::<HashSet<_>>();
                for alternative in &alternatives {
                    let ids = alternative
                        .keys()
                        .filter(|id| !bindings.contains_key(id))
                        .copied()
                        .collect::<HashSet<_>>();
                    assert_eq!(
                        ids, binding_ids,
                        "typed OR alternatives must publish identical binding IDs"
                    );
                }
                for id in binding_ids {
                    let sources = alternatives
                        .iter()
                        .map(|alternative| {
                            alternative
                                .get(&id)
                                .cloned()
                                .expect("typed OR alternative must publish shared binding")
                        })
                        .collect::<Vec<_>>();
                    let ty = sources[0].1.clone();
                    bindings.insert(id, (self.common_constructor_provenance(&sources), ty));
                }
            }
            TypedMatchPattern::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.match_provenance_bindings(
                        item,
                        &self.project_provenance(
                            source,
                            &Projection::Field { index, tag: None },
                            &Ty::Hole,
                        ),
                        bindings,
                    );
                }
            }
            TypedMatchPattern::ListCons(head, tail) => {
                self.match_provenance_bindings(
                    head,
                    &self.project_provenance(source, &Projection::Element, &Ty::Hole),
                    bindings,
                );
                self.match_provenance_bindings(tail, source, bindings);
            }
            TypedMatchPattern::Constructor {
                tag,
                fields,
                field_offset,
            } => {
                for (index, field) in fields.iter().enumerate() {
                    self.match_provenance_bindings(
                        field,
                        &self.project_provenance(
                            source,
                            &Projection::Field {
                                index: index + *field_offset as usize,
                                tag: Some(*tag),
                            },
                            &Ty::Hole,
                        ),
                        bindings,
                    );
                }
            }
            TypedMatchPattern::Extractor {
                extractor,
                extractor_ty,
                pre_args,
                success_tag,
                seq_tys,
                items,
                ..
            } => {
                let payloads = self.extractor_payload_provenance(
                    extractor,
                    extractor_ty,
                    pre_args,
                    *success_tag,
                    seq_tys,
                    source,
                    bindings,
                );
                for (item, payload) in items.iter().zip(payloads) {
                    self.match_provenance_bindings(item, &payload, bindings);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn or_binding_joins_constructor_provenance_from_all_alternatives() {
        let checker = Checker::new(TypecheckContext::default());
        let binding = ResolvedId {
            name: "value".into(),
            qualified_name: None,
            unique_id: 42,
            compiler_generated: false,
            symbol_info: None,
            span: Span { start: 0, end: 5 },
        };
        let pattern = TypedMatchPattern::Or(vec![
            TypedMatchPattern::Constructor {
                tag: 1,
                fields: vec![TypedMatchPattern::Binding(binding.clone())],
                field_offset: 0,
            },
            TypedMatchPattern::Constructor {
                tag: 2,
                fields: vec![TypedMatchPattern::Binding(binding.clone())],
                field_offset: 0,
            },
        ]);
        let source = (
            Provenance::Variants(vec![
                (1, vec![(Provenance::constrained("A".into()), Ty::Int)]),
                (2, vec![(Provenance::constrained("B".into()), Ty::Int)]),
            ]),
            Ty::Enum("Choice".into(), Vec::new()),
        );
        let mut bindings = Bindings::new();
        checker.match_provenance_bindings(&pattern, &source, &mut bindings);

        let (provenance, _) = bindings
            .get(&binding.unique_id)
            .expect("OR must publish its shared binding");
        assert!(matches!(provenance, Provenance::Intersection(sources)
            if sources.len() == 2
                && sources.iter().any(|source| source.0 == Provenance::constrained("A".into()))
                && sources.iter().any(|source| source.0 == Provenance::constrained("B".into()))));
    }
}
