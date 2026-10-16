use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;

/// Derived only for this specialization pass. All edges borrow the immutable
/// definitions, so neither saved substitutions nor declaration metadata are copied.
pub(super) struct CallableSummary<'a> {
    pub(super) signature: HashSet<u32>,
    pub(super) bound_tyvars: Vec<u32>,
    pub(super) direct_pattern_tyvars: Vec<u32>,
    pub(super) requires_concrete: bool,
    pub(super) calls: Vec<CallableEdge<'a>>,
}

pub(super) struct CallableEdge<'a> {
    callee: u32,
    substitution: &'a [(u32, Ty)],
    span: &'a Span,
    direct_pattern: bool,
    ordinary: bool,
}

pub(super) type CallableSummaries<'a> = HashMap<u32, CallableSummary<'a>>;
pub(super) type ReverseCalls<'a> = HashMap<u32, Vec<(u32, &'a CallableEdge<'a>)>>;

impl Checker {
    pub(super) fn callable_summaries<'a>(
        &self,
        definitions: &'a SpecializableDefinitions,
        trait_impl_method_uids: &HashSet<u32>,
    ) -> Result<CallableSummaries<'a>, TypeError> {
        let mut summaries = HashMap::with_capacity(definitions.len());
        let mut indices = definitions.keys().copied().collect::<Vec<_>>();
        indices.sort_unstable();
        for index in indices {
            let definition = &definitions[&index];
            let mut variables = Vec::new();
            match &definition.node {
                TypedInner::Def(_, _, arguments, parameters, result, ..) => {
                    for argument in arguments {
                        Self::collect_ty_vars(&argument.ty, &mut variables);
                    }
                    for parameter in parameters {
                        Self::collect_ty_vars(&parameter.ty, &mut variables);
                    }
                    Self::collect_ty_vars(result, &mut variables);
                }
                TypedInner::ExtractorDef(_, _, _, parameters, result, ..) => {
                    for parameter in parameters {
                        Self::collect_ty_vars(&parameter.ty, &mut variables);
                    }
                    Self::collect_ty_vars(result, &mut variables);
                }
                _ => {
                    return Err(self.typecheck_invariant_error(
                        "specializable definition metadata must name a callable definition",
                        &definition.span,
                    ))
                }
            }
            let signature = variables.into_iter().collect::<HashSet<_>>();
            let collected = RefCell::new((Vec::new(), Vec::new(), Vec::new(), false));
            if let Some(error) = self.find_typed_node(definition, &|checker, node| {
                let mut collected = collected.borrow_mut();
                let (direct, calls, pending_variables, pending) = &mut *collected;
                if let TypedInner::Match(_, arms) = &node.node {
                    for arm in arms {
                        match checker.deferred_direct_pattern_variable(arm) {
                            Ok(Some(variable)) if signature.contains(&variable) => {
                                if !direct.contains(&variable) {
                                    direct.push(variable);
                                }
                            }
                            Err(error) => return Some(error),
                            _ => {}
                        }
                    }
                }
                Self::collect_pending_trait_receiver_tyvars(node, pending_variables);
                *pending |= Self::node_requires_concrete_specialization(node);
                for (ty, span) in Self::pattern_callable_types(node) {
                    if let Ty::UserFunc {
                        fun_idx,
                        call_substitution,
                        ..
                    } = ty
                    {
                        if definitions.contains_key(fun_idx) {
                            calls.push(CallableEdge {
                                callee: *fun_idx,
                                substitution: call_substitution,
                                span,
                                direct_pattern: false,
                                ordinary: true,
                            });
                        }
                    }
                }
                if let Ty::UserFunc {
                    fun_idx,
                    call_substitution,
                    ..
                } = &node.ty
                {
                    if definitions.contains_key(fun_idx) {
                        calls.push(CallableEdge {
                            callee: *fun_idx,
                            substitution: call_substitution,
                            span: &node.span,
                            direct_pattern: true,
                            ordinary: !matches!(
                                node.node,
                                TypedInner::Def(..) | TypedInner::ExtractorDef(..)
                            ),
                        });
                    }
                }
                None
            }) {
                return Err(error);
            }
            let (mut direct_pattern_tyvars, calls, pending_variables, requires_concrete) =
                collected.into_inner();
            direct_pattern_tyvars.sort_unstable();
            let bound_tyvars = self.collect_bound_tyvars_for_def(
                definition,
                trait_impl_method_uids,
                &signature,
                &pending_variables,
            );
            summaries.insert(
                index,
                CallableSummary {
                    signature,
                    bound_tyvars,
                    direct_pattern_tyvars,
                    requires_concrete,
                    calls,
                },
            );
        }
        Ok(summaries)
    }

    // Child expressions are visited by find_typed_node. Only this node's own
    // dispatch and deferred failure metadata contribute here.
    fn node_requires_concrete_specialization(node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::TraitCall {
                dispatch,
                receiver_ty,
                ..
            } => {
                matches!(dispatch, TraitDispatch::Pending)
                    || matches!(dispatch, TraitDispatch::Selected(instantiation)
                        if Self::selected_instantiation_has_pending_input(instantiation))
                    || matches!(receiver_ty, Ty::Var(_))
            }
            TypedInner::Bind(pattern, _)
            | TypedInner::SafeBind(pattern, ..)
            | TypedInner::ApplyPattern { pattern, .. } => {
                Self::typed_pattern_has_pending_dispatch(pattern)
            }
            TypedInner::DoSafeBind(control) => {
                Self::typed_pattern_has_pending_dispatch(&control.pattern)
                    || matches!(control.failure_target, SafeBindFailureTarget::Deferred(_))
            }
            TypedInner::Match(_, arms) => arms
                .iter()
                .any(|arm| Self::typed_match_pattern_has_pending_dispatch(&arm.pattern)),
            _ => false,
        }
    }

    pub(super) fn reverse_callable_edges<'a>(
        summaries: &'a CallableSummaries<'a>,
    ) -> ReverseCalls<'a> {
        let mut reverse = HashMap::<_, Vec<_>>::new();
        let mut indices = summaries.keys().copied().collect::<Vec<_>>();
        indices.sort_unstable();
        for caller in indices {
            for edge in &summaries[&caller].calls {
                reverse.entry(edge.callee).or_default().push((caller, edge));
            }
        }
        reverse
    }

    pub(super) fn collect_direct_pattern_requirements(
        &self,
        summaries: &CallableSummaries<'_>,
        reverse: &ReverseCalls<'_>,
    ) -> HashMap<u32, Vec<u32>> {
        let mut requirements = summaries
            .iter()
            .filter_map(|(index, summary)| {
                (!summary.direct_pattern_tyvars.is_empty())
                    .then(|| (*index, summary.direct_pattern_tyvars.clone()))
            })
            .collect::<HashMap<_, _>>();
        let mut queue = requirements.keys().copied().collect::<Vec<_>>();
        queue.sort_unstable();
        let mut queued = queue.iter().copied().collect::<HashSet<_>>();
        let mut queue = VecDeque::from(queue);
        while let Some(callee) = queue.pop_front() {
            queued.remove(&callee);
            let required = requirements[&callee].clone();
            for (caller, edge) in reverse.get(&callee).into_iter().flatten() {
                if !edge.direct_pattern {
                    continue;
                }
                let mut changed = false;
                for variable in &required {
                    if let Some((_, Ty::Var(input))) =
                        edge.substitution.iter().find(|(slot, _)| slot == variable)
                    {
                        if summaries[caller].signature.contains(input) {
                            let forwarded = requirements.entry(*caller).or_default();
                            if !forwarded.contains(input) {
                                forwarded.push(*input);
                                changed = true;
                            }
                        }
                    }
                }
                if changed {
                    requirements.get_mut(caller).unwrap().sort_unstable();
                    if queued.insert(*caller) {
                        queue.push_back(*caller);
                    }
                }
            }
        }
        requirements
    }

    pub(super) fn propagate_callable_specialization_requirements(
        &self,
        summaries: &CallableSummaries<'_>,
        reverse: &ReverseCalls<'_>,
        requirements: &mut HashMap<u32, CallableSpecializationRequirements>,
    ) -> Result<SpecializationWork, TypeError> {
        let mut work = SpecializationWork::default();
        let mut initial = Vec::new();
        for (index, required) in requirements.iter_mut() {
            required.tyvars.sort_unstable();
            if required.requires_concrete || !required.tyvars.is_empty() {
                initial.push(*index);
            }
        }
        initial.sort_unstable();
        let mut queued = initial.iter().copied().collect::<HashSet<_>>();
        let mut queue = VecDeque::from(initial);
        while let Some(callee) = queue.pop_front() {
            queued.remove(&callee);
            let required = requirements[&callee].clone();
            for (caller, edge) in reverse.get(&callee).into_iter().flatten() {
                if !edge.ordinary {
                    continue;
                }
                work.visit_edge();
                let mut forwarded = requirements[caller].clone();
                forwarded.requires_concrete |= required.requires_concrete;
                for variable in &required.tyvars {
                    let Some((_, input)) =
                        edge.substitution.iter().find(|(slot, _)| slot == variable)
                    else {
                        return Err(self.typecheck_invariant_error(
                            "Missing call substitution for ordinary specialization",
                            edge.span,
                        ));
                    };
                    let mut inputs = Vec::new();
                    Self::collect_ty_vars(&self.resolve_ty(input), &mut inputs);
                    for input in inputs {
                        if summaries[caller].signature.contains(&input)
                            && !forwarded.tyvars.contains(&input)
                        {
                            forwarded.tyvars.push(input);
                        }
                    }
                }
                forwarded.tyvars.sort_unstable();
                if forwarded.tyvars != requirements[caller].tyvars
                    || forwarded.requires_concrete != requirements[caller].requires_concrete
                {
                    requirements.insert(*caller, forwarded);
                    if queued.insert(*caller) {
                        queue.push_back(*caller);
                    }
                }
            }
        }
        Ok(work)
    }
}
