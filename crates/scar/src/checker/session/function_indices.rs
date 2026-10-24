use super::*;

impl ScarSession {
    pub fn reconcile_function_indices<'a, I>(&mut self, functions: I)
    where
        I: IntoIterator<Item = (&'a str, u32)>,
    {
        let functions = functions.into_iter().collect::<Vec<_>>();
        let materialized_fun_idxs = functions
            .iter()
            .map(|(_, fun_idx)| *fun_idx)
            .collect::<HashSet<_>>();
        let function_indices = functions.into_iter().collect::<HashMap<_, _>>();
        let mut function_id_entries = self.state.function_ids_by_name.values().collect::<Vec<_>>();
        function_id_entries.sort_by_key(|id| {
            let registered_index = match self.state.env.vars.get(&id.unique_id) {
                Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
                _ => u32::MAX,
            };
            (registered_index, id.unique_id)
        });
        function_id_entries.dedup_by_key(|id| id.unique_id);
        let mut next_fun_idx = function_indices
            .values()
            .copied()
            .max()
            .map(|idx| idx + 1)
            .unwrap_or(0)
            .max(self.state.env.next_fun_idx);
        let mut specializable_rekeys = Vec::new();
        let mut fun_idx_rewrites = HashMap::new();
        for id in function_id_entries {
            let qualified_name = id.qualified_name.as_deref().unwrap_or(&id.name);
            let old_fun_idx = match self.state.env.vars.get(&id.unique_id) {
                Some(Ty::UserFunc { fun_idx, .. }) => Some(*fun_idx),
                _ => None,
            };
            let fun_idx = if let Some(old_fun_idx) = old_fun_idx.filter(|fun_idx| {
                self.state
                    .specializable_defs
                    .get(fun_idx)
                    .is_some_and(|def| match &def.node {
                        TypedInner::Def(_, def_id, ..)
                        | TypedInner::ExtractorDef(_, def_id, ..) => {
                            def_id.unique_id == id.unique_id
                        }
                        _ => false,
                    })
            }) {
                // A generated specialization can retain its origin's qualified
                // name. Match the retained origin by its current environment
                // index, not by a bytecode name that may refer to that
                // specialization instead.
                let new_fun_idx = next_fun_idx;
                next_fun_idx += 1;
                specializable_rekeys.push((old_fun_idx, new_fun_idx));
                fun_idx_rewrites.insert(old_fun_idx, new_fun_idx);
                new_fun_idx
            } else if let Some(fun_idx) = function_indices.get(qualified_name) {
                *fun_idx
            } else {
                continue;
            };
            if let Some(Ty::UserFunc {
                fun_idx: stored_fun_idx,
                ..
            }) = self.state.env.vars.get_mut(&id.unique_id)
            {
                if let Some(old_fun_idx) = old_fun_idx {
                    fun_idx_rewrites.insert(old_fun_idx, fun_idx);
                }
                *stored_fun_idx = fun_idx;
            }
        }
        self.rekey_specializable_defs(specializable_rekeys);
        fun_idx_rewrites.retain(|old, new| old != new);
        if !fun_idx_rewrites.is_empty() {
            for def in self.state.specializable_defs.values_mut() {
                Self::rewrite_fun_indices_in_node(Arc::make_mut(def), &fun_idx_rewrites);
            }
        }
        Self::rewrite_specialization_fun_indices(
            &mut self.state.specialization_fun_idxs,
            &fun_idx_rewrites,
        );
        // A typecheck stage can retain a generated specialization even when
        // codegen did not materialize it. Reusing its cached index in a later
        // chunk would call a function absent from the bytecode prefix.
        self.state
            .specialization_fun_idxs
            .retain(|_, fun_idx| materialized_fun_idxs.contains(fun_idx));
        self.state.env.next_fun_idx = self.state.env.next_fun_idx.max(next_fun_idx);
    }

    pub fn reconcile_visible_function_indices<I>(&mut self, functions: I)
    where
        I: IntoIterator<Item = (u32, u32)>,
    {
        let mut next_fun_idx = self.state.env.next_fun_idx;
        let mut specializable_rekeys = Vec::new();
        let mut fun_idx_rewrites = HashMap::new();

        for (uid, fun_idx) in functions {
            let old_fun_idx = match self.state.env.vars.get(&uid) {
                Some(Ty::UserFunc { fun_idx, .. }) => Some(*fun_idx),
                _ => None,
            };
            if let Some(Ty::UserFunc {
                fun_idx: stored_fun_idx,
                ..
            }) = self.state.env.vars.get_mut(&uid)
            {
                if let Some(old_fun_idx) = old_fun_idx {
                    fun_idx_rewrites.insert(old_fun_idx, fun_idx);
                    specializable_rekeys.push((old_fun_idx, fun_idx));
                }
                *stored_fun_idx = fun_idx;
                next_fun_idx = next_fun_idx.max(fun_idx + 1);
            }
        }

        self.rekey_specializable_defs(specializable_rekeys);
        fun_idx_rewrites.retain(|old, new| old != new);
        if !fun_idx_rewrites.is_empty() {
            for def in self.state.specializable_defs.values_mut() {
                Self::rewrite_fun_indices_in_node(Arc::make_mut(def), &fun_idx_rewrites);
            }
        }
        Self::rewrite_specialization_fun_indices(
            &mut self.state.specialization_fun_idxs,
            &fun_idx_rewrites,
        );
        self.state.env.next_fun_idx = self.state.env.next_fun_idx.max(next_fun_idx);
    }

    fn rekey_specializable_defs(&mut self, rekeys: Vec<(u32, u32)>) {
        // Remove every old key before inserting any new key. A new index may
        // be another definition's old index (for example 451 -> 452 and
        // 452 -> 453); moving entries one at a time would overwrite the
        // first entry before it gets moved.
        let moved = rekeys
            .into_iter()
            .filter_map(|(old_fun_idx, new_fun_idx)| {
                self.state
                    .specializable_defs
                    .remove(&old_fun_idx)
                    .map(|mut def| {
                        if old_fun_idx != new_fun_idx {
                            Self::set_def_fun_idx(Arc::make_mut(&mut def), new_fun_idx);
                        }
                        (new_fun_idx, def)
                    })
            })
            .collect::<Vec<_>>();
        for (new_fun_idx, def) in moved {
            self.state.specializable_defs.insert(new_fun_idx, def);
        }
    }

    fn rewrite_specialization_fun_indices(
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        rewrites: &HashMap<u32, u32>,
    ) {
        for fun_idx in specialization_fun_idxs.values_mut() {
            if let Some(new_fun_idx) = rewrites.get(fun_idx) {
                *fun_idx = *new_fun_idx;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn def_qualified_name(def: &TypedNode) -> Option<String> {
        match &def.node {
            TypedInner::Def(_, id, ..) | TypedInner::ExtractorDef(_, id, ..) => {
                Some(id.qualified_name.clone().unwrap_or_else(|| id.name.clone()))
            }
            _ => None,
        }
    }

    fn set_def_fun_idx(def: &mut TypedNode, new_fun_idx: u32) {
        match &mut def.node {
            TypedInner::Def(fun_idx, ..) | TypedInner::ExtractorDef(fun_idx, ..) => {
                *fun_idx = new_fun_idx;
            }
            _ => {}
        }
    }

    fn rewrite_fun_indices_in_ty(ty: &mut Ty, rewrites: &HashMap<u32, u32>) {
        match ty {
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => Self::rewrite_fun_indices_in_ty(inner, rewrites),
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                for item in items {
                    Self::rewrite_fun_indices_in_ty(item, rewrites);
                }
            }
            Ty::Func(params, ret) => {
                for param in params {
                    Self::rewrite_fun_indices_in_ty(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret, rewrites);
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                Self::rewrite_fun_indices_in_ty(source, rewrites);
                Self::rewrite_fun_indices_in_ty(focus, rewrites);
                Self::rewrite_fun_indices_in_ty(update_source, rewrites);
                Self::rewrite_fun_indices_in_ty(update_focus, rewrites);
            }
            Ty::BuiltinFunc { params, ret, .. } => {
                for param in params {
                    Self::rewrite_fun_indices_in_ty(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret, rewrites);
            }
            Ty::UserFunc {
                fun_idx,
                params,
                ret,
                ..
            } => {
                if let Some(new_fun_idx) = rewrites.get(fun_idx) {
                    *fun_idx = *new_fun_idx;
                }
                for param in params {
                    Self::rewrite_fun_indices_in_ty(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret, rewrites);
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for argument in &mut nominal.arguments {
                    Self::rewrite_fun_indices_in_ty(argument, rewrites);
                }
            }
            Ty::Enum(_, args) => {
                for arg in args {
                    Self::rewrite_fun_indices_in_ty(arg, rewrites);
                }
            }
            Ty::Result(ok, err) => {
                Self::rewrite_fun_indices_in_ty(ok, rewrites);
                Self::rewrite_fun_indices_in_ty(err, rewrites);
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::ProcessMarker(_)
            | Ty::Hole
            | Ty::Var(_)
            | Ty::Error => {}
        }
    }

    fn rewrite_fun_indices_in_dispatch(dispatch: &mut TraitDispatch, rewrites: &HashMap<u32, u32>) {
        let target = match dispatch {
            TraitDispatch::Static(target) => target,
            TraitDispatch::Selected(instantiation) => {
                for ty in instantiation.substitution.values_mut() {
                    Self::rewrite_fun_indices_in_ty(ty, rewrites);
                }
                if let sindr::signature::RuntimeTarget::UserFunction(fun_idx) =
                    &mut instantiation.callable_signature.runtime_target
                {
                    if let Some(new_fun_idx) = rewrites.get(fun_idx) {
                        *fun_idx = *new_fun_idx;
                    }
                }
                &mut instantiation.dispatch_target
            }
            TraitDispatch::Pending => return,
        };
        if let TraitDispatchTarget::UserFunction { fun_idx, .. } = target {
            if let Some(new_fun_idx) = rewrites.get(fun_idx) {
                *fun_idx = *new_fun_idx;
            }
        }
    }

    fn rewrite_fun_indices_in_facet_path(path: &mut TypedFacetPath, rewrites: &HashMap<u32, u32>) {
        Self::rewrite_fun_indices_in_ty(&mut path.source_ty, rewrites);
        Self::rewrite_fun_indices_in_ty(&mut path.focus_ty, rewrites);
        for ty in &mut path.segment_focus_tys {
            Self::rewrite_fun_indices_in_ty(ty, rewrites);
        }
        Self::rewrite_fun_indices_in_ty(&mut path.update_source_ty, rewrites);
        Self::rewrite_fun_indices_in_ty(&mut path.update_focus_ty, rewrites);
        for segment in &mut path.segments {
            match segment {
                TypedFacetSegment::ListIndex { index, .. }
                | TypedFacetSegment::MapKey { key: index, .. } => {
                    Self::rewrite_fun_indices_in_node(index, rewrites);
                }
                TypedFacetSegment::ListRange { start, end, .. } => {
                    Self::rewrite_fun_indices_in_node(start, rewrites);
                    Self::rewrite_fun_indices_in_node(end, rewrites);
                }
                TypedFacetSegment::Field { .. }
                | TypedFacetSegment::Tuple { .. }
                | TypedFacetSegment::ReadonlyBuiltin { .. }
                | TypedFacetSegment::ErrorPayload { .. }
                | TypedFacetSegment::Variant { .. } => {}
            }
        }
    }

    fn rewrite_fun_indices_in_pending_facet_path(
        path: &mut PendingFacetPath,
        rewrites: &HashMap<u32, u32>,
    ) {
        if let Some(source_ty_hint) = &mut path.source_ty_hint {
            Self::rewrite_fun_indices_in_ty(source_ty_hint, rewrites);
        }
        for segment in &mut path.segments {
            match segment {
                PendingFacetSegment::Field { .. } => {}
                PendingFacetSegment::Bracket { expr, .. } => {
                    Self::rewrite_fun_indices_in_pending_facet_expr(expr, rewrites);
                }
                PendingFacetSegment::RangeBracket { start, end, .. } => {
                    Self::rewrite_fun_indices_in_pending_facet_expr(start, rewrites);
                    Self::rewrite_fun_indices_in_pending_facet_expr(end, rewrites);
                }
            }
        }
    }

    fn rewrite_fun_indices_in_pending_facet_expr(
        expr: &mut PendingFacetExpr,
        rewrites: &HashMap<u32, u32>,
    ) {
        if let PendingFacetExpr::Typed(expr) = expr {
            Self::rewrite_fun_indices_in_node(expr, rewrites);
        }
    }

    pub(super) fn rewrite_fun_indices_in_node(node: &mut TypedNode, rewrites: &HashMap<u32, u32>) {
        Self::rewrite_fun_indices_in_ty(&mut node.ty, rewrites);
        if let TypedInner::ApplyPattern { projections, .. } = &mut node.node {
            for (_, ty) in projections {
                Self::rewrite_fun_indices_in_ty(ty, rewrites);
            }
        }
        if let TypedInner::SafeBind(_, _, projection, failure_target) = &mut node.node {
            match projection {
                SafeBindRhsProjection::CanonicalResultOnce {
                    payload_ty,
                    error_ty,
                } => {
                    Self::rewrite_fun_indices_in_ty(payload_ty, rewrites);
                    Self::rewrite_fun_indices_in_ty(error_ty, rewrites);
                }
                SafeBindRhsProjection::PatternInput { pattern_input_ty } => {
                    Self::rewrite_fun_indices_in_ty(pattern_input_ty, rewrites);
                }
            }
            match failure_target {
                SafeBindFailureTarget::EnclosingMonadFail(target)
                | SafeBindFailureTarget::DoMonadFail(target) => {
                    Self::rewrite_fun_indices_in_ty(&mut target.carrier_ty, rewrites);

                    Self::rewrite_fun_indices_in_node(&mut target.call, rewrites);
                }
                SafeBindFailureTarget::DoAlternative { empty } => {
                    Self::rewrite_fun_indices_in_node(empty, rewrites)
                }
                SafeBindFailureTarget::Deferred(deferred) => {
                    Self::rewrite_fun_indices_in_ty(&mut deferred.carrier_ty, rewrites);
                    for error_ty in &mut deferred.propagated_error_tys {
                        Self::rewrite_fun_indices_in_ty(error_ty, rewrites);
                    }
                }
                SafeBindFailureTarget::TopLevel
                | SafeBindFailureTarget::EnclosingMatchResultContext { .. } => {}
            }
        }
        match &mut node.node {
            TypedInner::ErrorConstruct {
                message,
                payload,
                payload_fields,
                ..
            } => {
                Self::rewrite_fun_indices_in_node(message, rewrites);
                for value in payload {
                    Self::rewrite_fun_indices_in_node(value, rewrites);
                }
                for (_, ty) in payload_fields {
                    Self::rewrite_fun_indices_in_ty(ty, rewrites);
                }
            }
            TypedInner::Lit(_)
            | TypedInner::ErrorKind(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil => {}
            TypedInner::SupervisorSpawn { init, .. } => {
                Self::rewrite_fun_indices_in_node(init, rewrites);
            }
            TypedInner::SupervisorAdopt { pid, .. } => {
                Self::rewrite_fun_indices_in_node(pid, rewrites);
            }
            TypedInner::SupervisorStatus { .. } => {}
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                Self::rewrite_fun_indices_in_node(init, rewrites);
                Self::rewrite_fun_indices_in_node(strategy, rewrites);
            }
            TypedInner::App(func, args) => {
                Self::rewrite_fun_indices_in_node(func, rewrites);
                for arg in args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
            }
            TypedInner::TraitCall {
                receiver_ty,
                obligation,
                dispatch,
                origin,
                args,
                ..
            } => {
                Self::rewrite_fun_indices_in_ty(receiver_ty, rewrites);
                Self::rewrite_fun_indices_in_ty(&mut obligation.receiver, rewrites);
                for trait_arg in &mut obligation.trait_args {
                    Self::rewrite_fun_indices_in_ty(trait_arg, rewrites);
                }
                Self::rewrite_fun_indices_in_dispatch(dispatch, rewrites);
                match origin {
                    TraitCallOrigin::Operator { lhs_ty, rhs_ty, .. }
                    | TraitCallOrigin::Comparison { lhs_ty, rhs_ty, .. } => {
                        Self::rewrite_fun_indices_in_ty(lhs_ty, rewrites);
                        Self::rewrite_fun_indices_in_ty(rhs_ty, rewrites);
                    }
                    TraitCallOrigin::Explicit => {}
                }
                for arg in args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
            }
            TypedInner::InjectCall(func, args) => {
                Self::rewrite_fun_indices_in_node(func, rewrites);
                for arg in args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
            }
            TypedInner::Block(stmts)
            | TypedInner::ListLiteral(stmts)
            | TypedInner::TupleLiteral(stmts) => {
                for stmt in stmts {
                    Self::rewrite_fun_indices_in_node(stmt, rewrites);
                }
            }
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    Self::rewrite_fun_indices_in_node(key, rewrites);
                    Self::rewrite_fun_indices_in_node(value, rewrites);
                }
            }
            TypedInner::Bind(pattern, rhs)
            | TypedInner::SafeBind(pattern, rhs, _, _)
            | TypedInner::ApplyPattern {
                pattern,
                value: rhs,
                ..
            } => {
                Self::rewrite_fun_indices_in_pattern(pattern, rewrites);
                Self::rewrite_fun_indices_in_node(rhs, rewrites);
            }
            TypedInner::DoSafeBind(control) => {
                Self::rewrite_fun_indices_in_pattern(&mut control.pattern, rewrites);
                Self::rewrite_fun_indices_in_node(&mut control.rhs, rewrites);
                match &mut control.projection {
                    SafeBindRhsProjection::CanonicalResultOnce {
                        payload_ty,
                        error_ty,
                    } => {
                        Self::rewrite_fun_indices_in_ty(payload_ty, rewrites);
                        Self::rewrite_fun_indices_in_ty(error_ty, rewrites);
                    }
                    SafeBindRhsProjection::PatternInput { pattern_input_ty } => {
                        Self::rewrite_fun_indices_in_ty(pattern_input_ty, rewrites);
                    }
                }
                match &mut control.failure_target {
                    SafeBindFailureTarget::DoMonadFail(target) => {
                        Self::rewrite_fun_indices_in_ty(&mut target.carrier_ty, rewrites);

                        Self::rewrite_fun_indices_in_node(&mut target.call, rewrites);
                    }
                    SafeBindFailureTarget::DoAlternative { empty } => {
                        Self::rewrite_fun_indices_in_node(empty, rewrites);
                    }
                    SafeBindFailureTarget::Deferred(deferred) => {
                        Self::rewrite_fun_indices_in_ty(&mut deferred.carrier_ty, rewrites);
                        for error_ty in &mut deferred.propagated_error_tys {
                            Self::rewrite_fun_indices_in_ty(error_ty, rewrites);
                        }
                    }
                    SafeBindFailureTarget::EnclosingMonadFail(target) => {
                        Self::rewrite_fun_indices_in_ty(&mut target.carrier_ty, rewrites);

                        Self::rewrite_fun_indices_in_node(&mut target.call, rewrites);
                    }
                    SafeBindFailureTarget::TopLevel
                    | SafeBindFailureTarget::EnclosingMatchResultContext { .. } => {}
                }
                Self::rewrite_fun_indices_in_node(&mut control.continuation, rewrites);
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                Self::rewrite_fun_indices_in_node(left, rewrites);
                Self::rewrite_fun_indices_in_node(right, rewrites);
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        Self::rewrite_fun_indices_in_node(expr, rewrites);
                    }
                }
            }
            TypedInner::Dbg(args) => {
                for arg in args {
                    Self::rewrite_fun_indices_in_node(&mut arg.expr, rewrites);
                }
            }
            TypedInner::EagerBoundary(inner) => Self::rewrite_fun_indices_in_node(inner, rewrites),
            TypedInner::If(cond, then_node, else_node) => {
                Self::rewrite_fun_indices_in_node(cond, rewrites);
                Self::rewrite_fun_indices_in_node(then_node, rewrites);
                if let Some(else_node) = else_node {
                    Self::rewrite_fun_indices_in_node(else_node, rewrites);
                }
            }
            TypedInner::Cause(value, err) => {
                Self::rewrite_fun_indices_in_node(value, rewrites);
                Self::rewrite_fun_indices_in_node(err, rewrites);
            }
            TypedInner::Match(scrutinee, arms) => {
                Self::rewrite_fun_indices_in_node(scrutinee, rewrites);
                for arm in arms {
                    Self::rewrite_fun_indices_in_match_arm(arm, rewrites);
                }
            }
            TypedInner::FieldAccess(value, _)
            | TypedInner::Semi(value)
            | TypedInner::Capture(value, _) => {
                Self::rewrite_fun_indices_in_node(value, rewrites);
            }
            TypedInner::ProcessContextHandler { .. } => {}
            TypedInner::FacetPath(path) => Self::rewrite_fun_indices_in_facet_path(path, rewrites),
            TypedInner::PendingFacetPath(path) => {
                Self::rewrite_fun_indices_in_pending_facet_path(path, rewrites);
            }
            TypedInner::FacetView {
                api: _,
                source,
                path,
                source_is_result: _,
            } => {
                Self::rewrite_fun_indices_in_node(source, rewrites);
                Self::rewrite_fun_indices_in_facet_path(path, rewrites);
            }
            TypedInner::FacetSet {
                api: _,
                source,
                path,
                value,
                source_is_result: _,
                mode: _,
            } => {
                Self::rewrite_fun_indices_in_node(source, rewrites);
                Self::rewrite_fun_indices_in_facet_path(path, rewrites);
                Self::rewrite_fun_indices_in_node(value, rewrites);
            }
            TypedInner::FacetOver {
                api: _,
                source,
                path,
                update_fun,
                source_is_result: _,
                mode: _,
            } => {
                Self::rewrite_fun_indices_in_node(source, rewrites);
                Self::rewrite_fun_indices_in_facet_path(path, rewrites);
                Self::rewrite_fun_indices_in_node(update_fun, rewrites);
            }
            TypedInner::StructLit(_, fields) | TypedInner::ConstructorCall(_, fields) => {
                for field in fields {
                    Self::rewrite_fun_indices_in_node(field, rewrites);
                }
            }
            TypedInner::DeferrorDef(_, _, _, params, body) => {
                for param in params {
                    Self::rewrite_fun_indices_in_value_parameter(param, rewrites);
                }
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::Def(_, _, type_params, params, ret_ty, _, body, _) => {
                for type_param in type_params {
                    let _ = type_param;
                }
                for param in params {
                    Self::rewrite_fun_indices_in_value_parameter(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret_ty, rewrites);
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::ExtractorDef(_, _, type_params, param, ret_ty, body, _) => {
                for type_param in type_params {
                    let _ = type_param;
                }
                for param in param {
                    Self::rewrite_fun_indices_in_value_parameter(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret_ty, rewrites);
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::Closure(params, _, body)
            | TypedInner::ExtractorClosure(params, _, body)
            | TypedInner::CaptureClosure(params, _, body) => {
                for param in params {
                    Self::rewrite_fun_indices_in_closure_param(param, rewrites);
                }
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::CaptureConstructorClosure(_, params, _, body) => {
                for param in params {
                    Self::rewrite_fun_indices_in_closure_param(param, rewrites);
                }
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::EnumDef(_, _)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(_, _, _)
            | TypedInner::StructDef(_, _, _, _)
            | TypedInner::RecordDef(_, _, _, _) => {}
        }
    }

    fn rewrite_fun_indices_in_pattern(pattern: &mut TypedPattern, rewrites: &HashMap<u32, u32>) {
        if let TypedPattern::Located(_, inner) = pattern {
            return Self::rewrite_fun_indices_in_pattern(inner, rewrites);
        }
        match pattern {
            TypedPattern::Located(_, _) => unreachable!("metadata handled before dispatch"),
            TypedPattern::HashMap(ty, _) => Self::rewrite_fun_indices_in_ty(ty, rewrites),
            TypedPattern::Pin(ty, _, dispatch) => {
                Self::rewrite_fun_indices_in_ty(ty, rewrites);
                Self::rewrite_fun_indices_in_dispatch(dispatch, rewrites);
            }
            TypedPattern::Var(ty, _)
            | TypedPattern::As(ty, _, _)
            | TypedPattern::Wildcard(ty)
            | TypedPattern::ListNil(ty)
            | TypedPattern::ListCons(ty, _, _)
            | TypedPattern::IntLit(ty, _)
            | TypedPattern::StrLit(ty, _)
            | TypedPattern::BoolLit(ty, _)
            | TypedPattern::DurationLit(ty, _)
            | TypedPattern::Tuple(ty, _) => Self::rewrite_fun_indices_in_ty(ty, rewrites),
            TypedPattern::Constructor { ty, field_tys, .. } => {
                Self::rewrite_fun_indices_in_ty(ty, rewrites);
                for field_ty in field_tys {
                    Self::rewrite_fun_indices_in_ty(field_ty, rewrites);
                }
            }
            TypedPattern::Extractor {
                input_ty,
                extractor_ty,
                seq_tys,
                ..
            } => {
                Self::rewrite_fun_indices_in_ty(input_ty, rewrites);
                Self::rewrite_fun_indices_in_ty(extractor_ty, rewrites);
                for ty in seq_tys {
                    Self::rewrite_fun_indices_in_ty(ty, rewrites);
                }
            }
        }
        match pattern {
            TypedPattern::Located(_, _) => unreachable!("metadata handled before dispatch"),
            TypedPattern::HashMap(_, entries) => {
                for entry in entries {
                    Self::rewrite_fun_indices_in_node(&mut entry.key, rewrites);
                    Self::rewrite_fun_indices_in_pattern(&mut entry.pattern, rewrites);
                }
            }
            TypedPattern::As(_, inner, _) => {
                Self::rewrite_fun_indices_in_pattern(inner, rewrites);
            }
            TypedPattern::ListCons(_, head, tail) => {
                Self::rewrite_fun_indices_in_pattern(head, rewrites);
                Self::rewrite_fun_indices_in_pattern(tail, rewrites);
            }
            TypedPattern::Tuple(_, items) | TypedPattern::Constructor { fields: items, .. } => {
                for item in items {
                    Self::rewrite_fun_indices_in_pattern(item, rewrites);
                }
            }
            TypedPattern::Extractor {
                pre_args, items, ..
            } => {
                for arg in pre_args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
                for item in items {
                    Self::rewrite_fun_indices_in_pattern(item, rewrites);
                }
            }
            TypedPattern::Var(_, _)
            | TypedPattern::Pin(_, _, _)
            | TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
        }
    }

    fn rewrite_fun_indices_in_match_pattern(
        pattern: &mut TypedMatchPattern,
        rewrites: &HashMap<u32, u32>,
    ) {
        match pattern {
            TypedMatchPattern::HashMap(entries) => {
                for entry in entries {
                    Self::rewrite_fun_indices_in_node(&mut entry.key, rewrites);
                    Self::rewrite_fun_indices_in_match_pattern(&mut entry.pattern, rewrites);
                }
            }
            TypedMatchPattern::As(inner, _) => {
                Self::rewrite_fun_indices_in_match_pattern(inner, rewrites)
            }
            TypedMatchPattern::Or(items)
            | TypedMatchPattern::Tuple(items)
            | TypedMatchPattern::Record(items)
            | TypedMatchPattern::ErrorPayload { fields: items, .. } => {
                for item in items {
                    Self::rewrite_fun_indices_in_match_pattern(item, rewrites);
                }
            }
            TypedMatchPattern::Extractor {
                input_ty,
                extractor_ty,
                pre_args,
                seq_tys,
                items,
                ..
            } => {
                Self::rewrite_fun_indices_in_ty(input_ty, rewrites);
                Self::rewrite_fun_indices_in_ty(extractor_ty, rewrites);
                for ty in seq_tys {
                    Self::rewrite_fun_indices_in_ty(ty, rewrites);
                }
                for arg in pre_args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
                for item in items {
                    Self::rewrite_fun_indices_in_match_pattern(item, rewrites);
                }
            }
            TypedMatchPattern::Constructor { fields, .. } => {
                for field in fields {
                    Self::rewrite_fun_indices_in_match_pattern(field, rewrites);
                }
            }
            TypedMatchPattern::ListCons(head, tail) => {
                Self::rewrite_fun_indices_in_match_pattern(head, rewrites);
                Self::rewrite_fun_indices_in_match_pattern(tail, rewrites);
            }
            TypedMatchPattern::Binding(_)
            | TypedMatchPattern::Pin { .. }
            | TypedMatchPattern::Wildcard
            | TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::DurationLit(_)
            | TypedMatchPattern::ErrorKind(_)
            | TypedMatchPattern::ListNil => {}
        }
    }

    fn rewrite_fun_indices_in_match_arm(arm: &mut TypedMatchArm, rewrites: &HashMap<u32, u32>) {
        Self::rewrite_fun_indices_in_match_pattern(&mut arm.pattern, rewrites);
        if let Some(guard) = &mut arm.guard {
            Self::rewrite_fun_indices_in_node(guard, rewrites);
        }
        Self::rewrite_fun_indices_in_node(&mut arm.body, rewrites);
    }

    fn rewrite_fun_indices_in_value_parameter(
        param: &mut TypedValueParameter,
        rewrites: &HashMap<u32, u32>,
    ) {
        Self::rewrite_fun_indices_in_ty(&mut param.ty, rewrites);
    }

    fn rewrite_fun_indices_in_closure_param(
        param: &mut TypedClosureParam,
        rewrites: &HashMap<u32, u32>,
    ) {
        Self::rewrite_fun_indices_in_ty(&mut param.ty, rewrites);
    }
}
