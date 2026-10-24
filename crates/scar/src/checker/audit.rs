use super::*;

impl Checker {
    fn type_contains_constructor_application(ty: &Ty) -> bool {
        match ty {
            Ty::SelfApp(items) => Self::constructor_application_parts(items).is_some(),
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => Self::type_contains_constructor_application(inner),
            Ty::Tuple(items) => items
                .iter()
                .any(Self::type_contains_constructor_application),
            Ty::Func(parameters, ret)
            | Ty::BuiltinFunc {
                params: parameters,
                ret,
                ..
            }
            | Ty::UserFunc {
                params: parameters,
                ret,
                ..
            } => {
                parameters
                    .iter()
                    .any(Self::type_contains_constructor_application)
                    || Self::type_contains_constructor_application(ret)
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => [
                source.as_ref(),
                focus.as_ref(),
                update_source.as_ref(),
                update_focus.as_ref(),
            ]
            .into_iter()
            .any(Self::type_contains_constructor_application),
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal
                .arguments
                .iter()
                .any(Self::type_contains_constructor_application),
            Ty::Enum(_, arguments) => arguments
                .iter()
                .any(Self::type_contains_constructor_application),
            Ty::Result(ok, error) => {
                Self::type_contains_constructor_application(ok)
                    || Self::type_contains_constructor_application(error)
            }
            Ty::Var(_)
            | Ty::Hole
            | Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::ProcessMarker(_) => false,
        }
    }

    fn unresolved_constructor_application_in_type(
        &self,
        ty: &Ty,
    ) -> Option<ConstructorApplicationOutcome> {
        match ty {
            Ty::SelfApp(items) => {
                let Some((source, slots)) = Self::constructor_application_parts(items) else {
                    return Some(ConstructorApplicationOutcome::Rejected {
                        failures: vec![ConstructorProjectionFailure::UnsupportedConstructor],
                    });
                };
                let resolved_witness = self.resolve_ty(source);
                let resolved_slots = slots
                    .iter()
                    .map(|slot| self.resolve_ty(slot))
                    .collect::<Vec<_>>();
                match self.apply_constructor_application(source, &resolved_witness, &resolved_slots)
                {
                    ConstructorApplicationOutcome::Applied(applied) => {
                        self.unresolved_constructor_application_in_type(&applied)
                    }
                    outcome => Some(outcome),
                }
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.unresolved_constructor_application_in_type(inner),
            Ty::Tuple(items) => items
                .iter()
                .find_map(|item| self.unresolved_constructor_application_in_type(item)),
            Ty::Func(parameters, ret) => parameters
                .iter()
                .find_map(|parameter| self.unresolved_constructor_application_in_type(parameter))
                .or_else(|| self.unresolved_constructor_application_in_type(ret)),
            // User and builtin callable signatures are declaration schemes. Their
            // constructor applications are instantiated at each executable call
            // site, so they are not unresolved runtime values by themselves.
            Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } => None,
            Ty::Facet(_, source, focus, update_source, update_focus) => [
                source.as_ref(),
                focus.as_ref(),
                update_source.as_ref(),
                update_focus.as_ref(),
            ]
            .into_iter()
            .find_map(|part| self.unresolved_constructor_application_in_type(part)),
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal
                .arguments
                .iter()
                .find_map(|argument| self.unresolved_constructor_application_in_type(argument)),
            Ty::Enum(_, arguments) => arguments
                .iter()
                .find_map(|argument| self.unresolved_constructor_application_in_type(argument)),
            Ty::Result(ok, error) => self
                .unresolved_constructor_application_in_type(ok)
                .or_else(|| self.unresolved_constructor_application_in_type(error)),
            Ty::Var(_)
            | Ty::Hole
            | Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::ProcessMarker(_) => None,
        }
    }

    pub(in crate::checker) fn unresolved_executable_constructor_application(
        &self,
        node: &TypedNode,
    ) -> Option<(ConstructorApplicationOutcome, Span)> {
        if let Some(pending) = Self::pattern_expression_nodes(node)
            .into_iter()
            .find_map(|expr| self.unresolved_executable_constructor_application(expr))
        {
            return Some(pending);
        }
        let declaration_scheme_has_constructor_application =
            match &node.node {
                TypedInner::TraitDef(..)
                | TypedInner::TraitImplDef(..)
                | TypedInner::PendingFacetPath(_)
                | TypedInner::StructDef(..)
                | TypedInner::RecordDef(..)
                | TypedInner::EnumDef(..) => return None,
                TypedInner::Def(_, _, return_arguments, parameters, ret, _, _, _)
                    if return_arguments.iter().any(|argument| {
                        Self::type_contains_constructor_application(&argument.ty)
                    }) || parameters.iter().any(|parameter| {
                        Self::type_contains_constructor_application(&parameter.ty)
                    }) || Self::type_contains_constructor_application(ret) =>
                {
                    true
                }
                _ => false,
            };
        // A generic declaration scheme may retain abstract constructor
        // applications, but its specialized executable body may not.  Skip
        // only the declaration node's own type and continue into the body.
        if !declaration_scheme_has_constructor_application {
            if let Some(outcome) = self.unresolved_constructor_application_in_type(&node.ty) {
                return Some((outcome, node.span.clone()));
            }
        }
        let recurse = |child: &TypedNode| self.unresolved_executable_constructor_application(child);
        if let Some(found) = node.monad_fail_call().and_then(recurse) {
            return Some(found);
        }
        match &node.node {
            TypedInner::ErrorConstruct {
                message, payload, ..
            } => recurse(message).or_else(|| payload.iter().find_map(recurse)),
            TypedInner::TraitCall {
                receiver_ty, args, ..
            } => self
                .unresolved_constructor_application_in_type(receiver_ty)
                .map(|outcome| (outcome, node.span.clone()))
                .or_else(|| args.iter().find_map(recurse)),
            TypedInner::App(function, arguments)
            | TypedInner::InjectCall(function, arguments)
            | TypedInner::Capture(function, arguments) => {
                recurse(function).or_else(|| arguments.iter().find_map(recurse))
            }
            TypedInner::Block(items)
            | TypedInner::TupleLiteral(items)
            | TypedInner::ListLiteral(items)
            | TypedInner::ConstructorCall(_, items)
            | TypedInner::StructLit(_, items) => items.iter().find_map(recurse),
            TypedInner::HashMapLiteral(entries) => entries
                .iter()
                .find_map(|(key, value)| recurse(key).or_else(|| recurse(value))),
            TypedInner::Bind(_, rhs)
            | TypedInner::ApplyPattern { value: rhs, .. }
            | TypedInner::SafeBind(_, rhs, _, _)
            | TypedInner::Semi(rhs)
            | TypedInner::FieldAccess(rhs, _)
            | TypedInner::EagerBoundary(rhs) => recurse(rhs),
            TypedInner::DoSafeBind(control) => recurse(&control.rhs)
                .or_else(|| match &control.failure_target {
                    SafeBindFailureTarget::DoAlternative { empty } => recurse(empty),
                    _ => None,
                })
                .or_else(|| recurse(&control.continuation)),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right)
            | TypedInner::Cause(left, right) => recurse(left).or_else(|| recurse(right)),
            TypedInner::If(condition, then_branch, else_branch) => recurse(condition)
                .or_else(|| recurse(then_branch))
                .or_else(|| else_branch.as_deref().and_then(recurse)),
            TypedInner::Match(scrutinee, arms) | TypedInner::AssertErrorMatches(scrutinee, arms, _) => recurse(scrutinee).or_else(|| {
                arms.iter().find_map(|arm| {
                    arm.guard
                        .as_ref()
                        .and_then(recurse)
                        .or_else(|| recurse(&arm.body))
                })
            }),
            TypedInner::InterpolatedStr(parts) => parts.iter().find_map(|part| match part {
                TypedInterpolatedPart::Text(_) => None,
                TypedInterpolatedPart::Expr(expr) => recurse(expr),
            }),
            TypedInner::Dbg(arguments) => arguments
                .iter()
                .find_map(|argument| recurse(&argument.expr)),
            TypedInner::DeferrorDef(_, _, _, _, body)
            | TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body)
            | TypedInner::ExtractorClosure(_, _, body)
            | TypedInner::CaptureClosure(_, _, body) => recurse(body),
            TypedInner::CaptureConstructorClosure(_, _, _, body) => recurse(body),
            TypedInner::SupervisorSpawn { init, .. } => recurse(init),
            TypedInner::SupervisorAdopt { pid, .. } => recurse(pid),
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                recurse(init).or_else(|| recurse(strategy))
            }
            TypedInner::FacetView { source, .. } => recurse(source),
            TypedInner::FacetSet { source, value, .. } => {
                recurse(source).or_else(|| recurse(value))
            }
            TypedInner::FacetOver {
                source, update_fun, ..
            } => recurse(source).or_else(|| recurse(update_fun)),
            TypedInner::Lit(_)
            | TypedInner::ErrorKind(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::FacetPath(_)
            | TypedInner::PendingFacetPath(_)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => None,
        }
    }

    pub(in crate::checker) fn first_pending_trait_helper<'a>(
        &self,
        node: &'a TypedNode,
    ) -> Option<(&'a str, &'a str, &'a Ty, &'a Span)> {
        self.find_typed_node(node, &|_, node| match &node.node {
            TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty,
                dispatch,
                ..
            } if matches!(dispatch, crate::typed::TraitDispatch::Pending) => Some((
                trait_name.as_str(),
                method_name.as_str(),
                receiver_ty,
                &node.span,
            )),
            _ => None,
        })
    }

    pub(in crate::checker) fn typed_facet_path_expression_nodes(
        path: &TypedFacetPath,
    ) -> impl Iterator<Item = &TypedNode> {
        path.segments.iter().flat_map(|segment| {
            let nodes: [Option<&TypedNode>; 2] = match segment {
                TypedFacetSegment::ListIndex { index, .. }
                | TypedFacetSegment::MapKey { key: index, .. } => [Some(index), None],
                TypedFacetSegment::ListRange { start, end, .. } => [Some(start), Some(end)],
                TypedFacetSegment::Field { .. }
                | TypedFacetSegment::Tuple { .. }
                | TypedFacetSegment::ReadonlyBuiltin { .. }
                | TypedFacetSegment::ErrorPayload { .. }
                | TypedFacetSegment::Variant { .. } => [None, None],
            };
            nodes.into_iter().flatten()
        })
    }

    pub(in crate::checker) fn pending_facet_path_expression_nodes(
        path: &PendingFacetPath,
    ) -> impl Iterator<Item = &TypedNode> {
        fn typed_expr(expr: &PendingFacetExpr) -> Option<&TypedNode> {
            match expr {
                PendingFacetExpr::Typed(node) => Some(node),
                PendingFacetExpr::Resolved(_) => None,
            }
        }
        path.segments.iter().flat_map(|segment| {
            let nodes = match segment {
                PendingFacetSegment::Field { .. } => [None, None],
                PendingFacetSegment::Bracket { expr, .. } => [typed_expr(expr), None],
                PendingFacetSegment::RangeBracket { start, end, .. } => {
                    [typed_expr(start), typed_expr(end)]
                }
            };
            nodes.into_iter().flatten()
        })
    }

    pub(in crate::checker) fn find_typed_node<'a, T>(
        &self,
        node: &'a TypedNode,
        inspect: &impl Fn(&Self, &'a TypedNode) -> Option<T>,
    ) -> Option<T> {
        if let Some(found) = inspect(self, node) {
            return Some(found);
        }
        if let Some(pending) = Self::pattern_expression_nodes(node)
            .into_iter()
            .find_map(|expr| self.find_typed_node(expr, inspect))
        {
            return Some(pending);
        }
        if let Some(found) = node
            .monad_fail_call()
            .and_then(|call| self.find_typed_node(call, inspect))
        {
            return Some(found);
        }
        match &node.node {
            TypedInner::ErrorConstruct {
                message, payload, ..
            } => self.find_typed_node(message, inspect).or_else(|| {
                payload
                    .iter()
                    .find_map(|value| self.find_typed_node(value, inspect))
            }),
            TypedInner::TraitCall { args, .. } => args
                .iter()
                .find_map(|arg| self.find_typed_node(arg, inspect)),
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.find_typed_node(func, inspect).or_else(|| {
                    args.iter()
                        .find_map(|arg| self.find_typed_node(arg, inspect))
                })
            }
            TypedInner::Block(stmts)
            | TypedInner::TupleLiteral(stmts)
            | TypedInner::ListLiteral(stmts)
            | TypedInner::ConstructorCall(_, stmts)
            | TypedInner::StructLit(_, stmts) => stmts
                .iter()
                .find_map(|stmt| self.find_typed_node(stmt, inspect)),
            TypedInner::HashMapLiteral(entries) => entries.iter().find_map(|(key, value)| {
                self.find_typed_node(key, inspect)
                    .or_else(|| self.find_typed_node(value, inspect))
            }),
            TypedInner::Bind(_, rhs)
            | TypedInner::ApplyPattern { value: rhs, .. }
            | TypedInner::SafeBind(_, rhs, _, _)
            | TypedInner::Semi(rhs)
            | TypedInner::FieldAccess(rhs, _) => self.find_typed_node(rhs, inspect),
            TypedInner::DoSafeBind(control) => {
                self.find_typed_node(&control.rhs, inspect).or_else(|| {
                    match &control.failure_target {
                        SafeBindFailureTarget::DoAlternative { empty } => {
                            self.find_typed_node(empty, inspect)
                        }
                        _ => None,
                    }
                    .or_else(|| self.find_typed_node(&control.continuation, inspect))
                })
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => self
                .find_typed_node(left, inspect)
                .or_else(|| self.find_typed_node(right, inspect)),
            TypedInner::If(cond, then_branch, else_branch) => self
                .find_typed_node(cond, inspect)
                .or_else(|| self.find_typed_node(then_branch, inspect))
                .or_else(|| {
                    else_branch
                        .as_deref()
                        .and_then(|branch| self.find_typed_node(branch, inspect))
                }),
            TypedInner::Cause(value, err) => self
                .find_typed_node(value, inspect)
                .or_else(|| self.find_typed_node(err, inspect)),
            TypedInner::Match(scrutinee, arms) | TypedInner::AssertErrorMatches(scrutinee, arms, _) => {
                self.find_typed_node(scrutinee, inspect).or_else(|| {
                    arms.iter().find_map(|arm| {
                        arm.guard
                            .as_ref()
                            .and_then(|guard| self.find_typed_node(guard, inspect))
                            .or_else(|| self.find_typed_node(&arm.body, inspect))
                    })
                })
            }
            TypedInner::InterpolatedStr(parts) => parts.iter().find_map(|part| match part {
                crate::typed::TypedInterpolatedPart::Text(_) => None,
                crate::typed::TypedInterpolatedPart::Expr(expr) => {
                    self.find_typed_node(expr, inspect)
                }
            }),
            TypedInner::Dbg(args) => args
                .iter()
                .find_map(|arg| self.find_typed_node(&arg.expr, inspect)),
            TypedInner::EagerBoundary(inner) => self.find_typed_node(inner, inspect),
            TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body)
            | TypedInner::ExtractorClosure(_, _, body)
            | TypedInner::CaptureClosure(_, _, body) => self.find_typed_node(body, inspect),
            TypedInner::CaptureConstructorClosure(_, _, _, body) => {
                self.find_typed_node(body, inspect)
            }
            TypedInner::SupervisorSpawn { init, .. } => self.find_typed_node(init, inspect),
            TypedInner::SupervisorAdopt { pid, .. } => self.find_typed_node(pid, inspect),
            TypedInner::SupervisorWorkers { init, strategy, .. } => self
                .find_typed_node(init, inspect)
                .or_else(|| self.find_typed_node(strategy, inspect)),
            TypedInner::FacetPath(path) => Self::typed_facet_path_expression_nodes(path)
                .find_map(|expr| self.find_typed_node(expr, inspect)),
            TypedInner::PendingFacetPath(path) => Self::pending_facet_path_expression_nodes(path)
                .find_map(|expr| self.find_typed_node(expr, inspect)),
            TypedInner::FacetView { source, path, .. } => {
                self.find_typed_node(source, inspect).or_else(|| {
                    Self::typed_facet_path_expression_nodes(path)
                        .find_map(|expr| self.find_typed_node(expr, inspect))
                })
            }
            TypedInner::FacetSet {
                source,
                path,
                value,
                ..
            } => self
                .find_typed_node(source, inspect)
                .or_else(|| {
                    Self::typed_facet_path_expression_nodes(path)
                        .find_map(|expr| self.find_typed_node(expr, inspect))
                })
                .or_else(|| self.find_typed_node(value, inspect)),
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                ..
            } => self
                .find_typed_node(source, inspect)
                .or_else(|| {
                    Self::typed_facet_path_expression_nodes(path)
                        .find_map(|expr| self.find_typed_node(expr, inspect))
                })
                .or_else(|| self.find_typed_node(update_fun, inspect)),
            TypedInner::Lit(_)
            | TypedInner::ErrorKind(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::DeferrorDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => None,
        }
    }

    pub(in crate::checker) fn full_trait_obligations(node: &TypedNode) -> Vec<TraitObligation> {
        fn collect(node: &TypedNode, obligations: &mut Vec<TraitObligation>) {
            if let Some(call) = node.monad_fail_call() {
                collect(call, obligations);
            }
            for expr in Checker::pattern_expression_nodes(node) {
                collect(expr, obligations);
            }
            match &node.node {
                TypedInner::ErrorConstruct {
                    message, payload, ..
                } => {
                    collect(message, obligations);
                    for value in payload {
                        collect(value, obligations);
                    }
                }
                TypedInner::TraitCall {
                    obligation, args, ..
                } => {
                    obligations.push(obligation.clone());
                    for arg in args {
                        collect(arg, obligations);
                    }
                }
                TypedInner::App(func, args)
                | TypedInner::InjectCall(func, args)
                | TypedInner::Capture(func, args) => {
                    collect(func, obligations);
                    for arg in args {
                        collect(arg, obligations);
                    }
                }
                TypedInner::Block(items)
                | TypedInner::ListLiteral(items)
                | TypedInner::TupleLiteral(items)
                | TypedInner::ConstructorCall(_, items)
                | TypedInner::StructLit(_, items) => {
                    for item in items {
                        collect(item, obligations);
                    }
                }
                TypedInner::HashMapLiteral(entries) => {
                    for (key, value) in entries {
                        collect(key, obligations);
                        collect(value, obligations);
                    }
                }
                TypedInner::DoSafeBind(control) => {
                    collect(&control.rhs, obligations);
                    if let SafeBindFailureTarget::DoAlternative { empty } = &control.failure_target
                    {
                        collect(empty, obligations);
                    }
                    collect(&control.continuation, obligations);
                }
                TypedInner::Bind(_, rhs)
                | TypedInner::ApplyPattern { value: rhs, .. }
                | TypedInner::SafeBind(_, rhs, _, _)
                | TypedInner::Semi(rhs)
                | TypedInner::FieldAccess(rhs, _)
                | TypedInner::EagerBoundary(rhs) => collect(rhs, obligations),
                TypedInner::BinOp(_, left, right)
                | TypedInner::Pipe(left, right)
                | TypedInner::Compose(_, left, right)
                | TypedInner::ListCons(left, right)
                | TypedInner::Cause(left, right) => {
                    collect(left, obligations);
                    collect(right, obligations);
                }
                TypedInner::If(cond, then_branch, else_branch) => {
                    collect(cond, obligations);
                    collect(then_branch, obligations);
                    if let Some(branch) = else_branch {
                        collect(branch, obligations);
                    }
                }
                TypedInner::Match(scrutinee, arms) | TypedInner::AssertErrorMatches(scrutinee, arms, _) => {
                    collect(scrutinee, obligations);
                    for arm in arms {
                        if let Some(guard) = &arm.guard {
                            collect(guard, obligations);
                        }
                        collect(&arm.body, obligations);
                    }
                }
                TypedInner::InterpolatedStr(parts) => {
                    for part in parts {
                        if let TypedInterpolatedPart::Expr(expr) = part {
                            collect(expr, obligations);
                        }
                    }
                }
                TypedInner::Dbg(args) => {
                    for arg in args {
                        collect(&arg.expr, obligations);
                    }
                }
                TypedInner::DeferrorDef(_, _, _, _, body)
                | TypedInner::Def(_, _, _, _, _, _, body, _)
                | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
                | TypedInner::Closure(_, _, body)
                | TypedInner::ExtractorClosure(_, _, body)
                | TypedInner::CaptureClosure(_, _, body) => collect(body, obligations),
                TypedInner::SupervisorSpawn { init, .. } => collect(init, obligations),
                TypedInner::SupervisorAdopt { pid, .. } => collect(pid, obligations),
                TypedInner::SupervisorWorkers { init, strategy, .. } => {
                    collect(init, obligations);
                    collect(strategy, obligations);
                }
                TypedInner::FacetPath(path) => {
                    for expr in Checker::typed_facet_path_expression_nodes(path) {
                        collect(expr, obligations);
                    }
                }
                TypedInner::PendingFacetPath(path) => {
                    for expr in Checker::pending_facet_path_expression_nodes(path) {
                        collect(expr, obligations);
                    }
                }
                TypedInner::FacetView { source, path, .. } => {
                    collect(source, obligations);
                    for expr in Checker::typed_facet_path_expression_nodes(path) {
                        collect(expr, obligations);
                    }
                }
                TypedInner::FacetSet {
                    source,
                    path,
                    value,
                    ..
                } => {
                    collect(source, obligations);
                    for expr in Checker::typed_facet_path_expression_nodes(path) {
                        collect(expr, obligations);
                    }
                    collect(value, obligations);
                }
                TypedInner::FacetOver {
                    source,
                    path,
                    update_fun,
                    ..
                } => {
                    collect(source, obligations);
                    for expr in Checker::typed_facet_path_expression_nodes(path) {
                        collect(expr, obligations);
                    }
                    collect(update_fun, obligations);
                }
                _ => {}
            }
        }

        let mut obligations = Vec::new();
        collect(node, &mut obligations);
        obligations
    }
}

pub(super) enum UnresolvedExecutableTypeArgument {
    ReturnTypeArgument {
        callable: String,
        ordinal: u32,
        unresolved: String,
        span: Span,
    },
    EnumConstructor {
        enum_name: String,
        constructor: String,
        ordinal: u32,
        span: Span,
    },
}

impl Checker {
    pub(super) fn first_unresolved_executable_type_argument(
        &self,
        node: &TypedNode,
        allowed_vars: &HashSet<u32>,
        allowed_enum_constructor_vars: &HashSet<u32>,
    ) -> Option<UnresolvedExecutableTypeArgument> {
        if let Some(found) = node.monad_fail_call().and_then(|call| {
            self.first_unresolved_executable_type_argument(
                call,
                allowed_vars,
                allowed_enum_constructor_vars,
            )
        }) {
            return Some(found);
        }
        if let Some(pending) = Self::pattern_expression_nodes(node)
            .into_iter()
            .find_map(|expr| {
                self.first_unresolved_executable_type_argument(
                    expr,
                    allowed_vars,
                    allowed_enum_constructor_vars,
                )
            })
        {
            return Some(pending);
        }
        if let TypedInner::ConstructorCall(tag, _) = &node.node {
            let enum_application = match self.resolve_ty(&node.ty) {
                Ty::Enum(enum_name, arguments) => Some((enum_name, arguments)),
                _ => None,
            };
            if let Some((enum_name, arguments)) = enum_application {
                if let Some(ordinal) = arguments.iter().position(|argument| {
                    let mut variables = Vec::new();
                    Self::collect_ty_vars(&self.resolve_ty(argument), &mut variables);
                    variables
                        .iter()
                        .any(|variable| !allowed_enum_constructor_vars.contains(variable))
                }) {
                    let variant = self
                        .env
                        .enum_variant_by_tag(*tag)
                        .expect("typed enum constructor tag has canonical metadata");
                    return Some(UnresolvedExecutableTypeArgument::EnumConstructor {
                        enum_name: Self::surface_name(&enum_name).into(),
                        constructor: Self::surface_name(&variant.constructor_name).into(),
                        ordinal: ordinal as u32,
                        span: node.span.clone(),
                    });
                }
            }
        }
        if let TypedInner::App(func, _) = &node.node {
            if let TypedInner::Var(id) = &func.node {
                // Compiler-generated process helpers carry synthetic generic
                // signatures whose variables are resolved by their lowering
                // path rather than by source-level return-type inference.
                if !id.compiler_generated {
                    if let Some(signature) = self.callable_signatures.get(&id.unique_id) {
                        if !signature.return_type_arguments.is_empty() {
                            let resolved = self.resolve_ty(&node.ty);
                            let mut unresolved = Vec::new();
                            Self::collect_ty_vars(&resolved, &mut unresolved);
                            let call_substitution = match &func.ty {
                                Ty::UserFunc {
                                    call_substitution, ..
                                } => call_substitution.as_slice(),
                                _ => &[],
                            };
                            let argument = signature
                                .return_type_arguments
                                .iter()
                                .find(|argument| {
                                    let mut declared = Vec::new();
                                    Self::collect_ty_vars(&argument.ty, &mut declared);
                                    declared.into_iter().any(|declared_var| {
                                        call_substitution
                                            .iter()
                                            .find(|(original, _)| *original == declared_var)
                                            .is_some_and(|(_, instantiated)| {
                                                let mut instantiated_vars = Vec::new();
                                                Self::collect_ty_vars(
                                                    &self.resolve_ty(instantiated),
                                                    &mut instantiated_vars,
                                                );
                                                instantiated_vars.iter().any(|var| {
                                                    unresolved.contains(var)
                                                        && !allowed_vars.contains(var)
                                                })
                                            })
                                    })
                                })
                                .unwrap_or(&signature.return_type_arguments[0]);
                            let result_can_receive_a_later_call_witness = matches!(
                                &resolved,
                                Ty::Func(..) | Ty::BuiltinFunc { .. } | Ty::UserFunc { .. }
                            );
                            if !result_can_receive_a_later_call_witness
                                && unresolved.iter().any(|var| !allowed_vars.contains(var))
                            {
                                return Some(
                                    UnresolvedExecutableTypeArgument::ReturnTypeArgument {
                                        callable: id.name.clone(),
                                        ordinal: argument.ordinal,
                                        unresolved: self.diagnostic_ty_name(&resolved),
                                        span: node.span.clone(),
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        let visit = |child: &TypedNode| {
            self.first_unresolved_executable_type_argument(
                child,
                allowed_vars,
                allowed_enum_constructor_vars,
            )
        };
        match &node.node {
            TypedInner::ErrorConstruct {
                message, payload, ..
            } => visit(message).or_else(|| payload.iter().find_map(visit)),
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                visit(func).or_else(|| args.iter().find_map(visit))
            }
            TypedInner::TraitCall { args, .. }
            | TypedInner::ListLiteral(args)
            | TypedInner::TupleLiteral(args)
            | TypedInner::StructLit(_, args)
            | TypedInner::ConstructorCall(_, args) => args.iter().find_map(visit),
            TypedInner::Block(stmts) => stmts.iter().find_map(visit),
            TypedInner::Bind(_, rhs)
            | TypedInner::ApplyPattern { value: rhs, .. }
            | TypedInner::SafeBind(_, rhs, _, _)
            | TypedInner::Semi(rhs)
            | TypedInner::EagerBoundary(rhs)
            | TypedInner::FieldAccess(rhs, _) => visit(rhs),
            TypedInner::DoSafeBind(control) => visit(&control.rhs)
                .or_else(|| match &control.failure_target {
                    SafeBindFailureTarget::DoAlternative { empty } => visit(empty),
                    _ => None,
                })
                .or_else(|| visit(&control.continuation)),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right)
            | TypedInner::Cause(left, right) => visit(left).or_else(|| visit(right)),
            TypedInner::If(cond, then_branch, else_branch) => visit(cond)
                .or_else(|| visit(then_branch))
                .or_else(|| else_branch.as_deref().and_then(visit)),
            TypedInner::Match(scrutinee, arms) | TypedInner::AssertErrorMatches(scrutinee, arms, _) => visit(scrutinee).or_else(|| {
                arms.iter().find_map(|arm| {
                    arm.guard
                        .as_ref()
                        .and_then(visit)
                        .or_else(|| visit(&arm.body))
                })
            }),
            TypedInner::InterpolatedStr(parts) => parts.iter().find_map(|part| match part {
                TypedInterpolatedPart::Text(_) => None,
                TypedInterpolatedPart::Expr(expr) => visit(expr),
            }),
            TypedInner::Dbg(args) => args.iter().find_map(|arg| visit(&arg.expr)),
            TypedInner::HashMapLiteral(entries) => entries
                .iter()
                .find_map(|(key, value)| visit(key).or_else(|| visit(value))),
            TypedInner::SupervisorSpawn { init, .. }
            | TypedInner::FacetView { source: init, .. } => visit(init),
            TypedInner::SupervisorAdopt { pid, .. } => visit(pid),
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                visit(init).or_else(|| visit(strategy))
            }
            TypedInner::FacetSet { source, value, .. } => visit(source).or_else(|| visit(value)),
            TypedInner::FacetOver {
                source, update_fun, ..
            } => visit(source).or_else(|| visit(update_fun)),
            TypedInner::Def(_, _, return_type_arguments, params, ret_ty, _, body, _) => {
                let mut allowed = allowed_vars.clone();
                let mut allowed_enum_constructors = allowed_enum_constructor_vars.clone();
                for argument in return_type_arguments {
                    self.extend_allowed_vars(&argument.ty, &mut allowed);
                    self.extend_allowed_vars(&argument.ty, &mut allowed_enum_constructors);
                }
                for param in params {
                    self.extend_allowed_vars(&param.ty, &mut allowed);
                    self.extend_allowed_vars(&param.ty, &mut allowed_enum_constructors);
                }
                self.extend_allowed_vars(ret_ty, &mut allowed);
                self.first_unresolved_executable_type_argument(
                    body,
                    &allowed,
                    &allowed_enum_constructors,
                )
            }
            TypedInner::ExtractorDef(_, _, type_params, param, ret_ty, body, _) => {
                let mut allowed = allowed_vars.clone();
                let mut allowed_enum_constructors = allowed_enum_constructor_vars.clone();
                for type_param in type_params {
                    allowed.insert(type_param.ty_var);
                    allowed_enum_constructors.insert(type_param.ty_var);
                }
                for param in param {
                    self.extend_allowed_vars(&param.ty, &mut allowed);
                    self.extend_allowed_vars(&param.ty, &mut allowed_enum_constructors);
                }
                self.extend_allowed_vars(ret_ty, &mut allowed);
                self.first_unresolved_executable_type_argument(
                    body,
                    &allowed,
                    &allowed_enum_constructors,
                )
            }
            TypedInner::DeferrorDef(_, _, _, params, body) => {
                let mut allowed = allowed_vars.clone();
                let mut allowed_enum_constructors = allowed_enum_constructor_vars.clone();
                for parameter in params {
                    self.extend_allowed_vars(&parameter.ty, &mut allowed);
                    self.extend_allowed_vars(&parameter.ty, &mut allowed_enum_constructors);
                }
                self.first_unresolved_executable_type_argument(
                    body,
                    &allowed,
                    &allowed_enum_constructors,
                )
            }
            TypedInner::Closure(params, _, body)
            | TypedInner::ExtractorClosure(params, _, body)
            | TypedInner::CaptureClosure(params, _, body) => {
                let mut allowed_enum_constructors = allowed_enum_constructor_vars.clone();
                for parameter in params {
                    self.extend_allowed_vars(&parameter.ty, &mut allowed_enum_constructors);
                }
                self.first_unresolved_executable_type_argument(
                    body,
                    allowed_vars,
                    &allowed_enum_constructors,
                )
            }
            TypedInner::CaptureConstructorClosure(_, params, _, body) => {
                let mut allowed_enum_constructors = allowed_enum_constructor_vars.clone();
                for parameter in params {
                    self.extend_allowed_vars(&parameter.ty, &mut allowed_enum_constructors);
                }
                self.first_unresolved_executable_type_argument(
                    body,
                    allowed_vars,
                    &allowed_enum_constructors,
                )
            }
            TypedInner::Lit(_)
            | TypedInner::ErrorKind(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::FacetPath(_)
            | TypedInner::PendingFacetPath(_)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => None,
        }
    }

    pub(super) fn extend_allowed_vars(&self, ty: &Ty, allowed: &mut HashSet<u32>) {
        let mut vars = Vec::new();
        Self::collect_ty_vars(&self.resolve_ty(ty), &mut vars);
        allowed.extend(vars);
    }

    pub(super) fn typed_match_pattern_has_pending_dispatch(pattern: &TypedMatchPattern) -> bool {
        match pattern {
            TypedMatchPattern::HashMap(entries) => entries.iter().any(|entry| {
                Self::typed_node_has_pending_trait_call(&entry.key)
                    || Self::typed_match_pattern_has_pending_dispatch(&entry.pattern)
            }),
            TypedMatchPattern::Pin { dispatch, .. } => {
                matches!(
                    dispatch,
                    TraitDispatch::Pending | TraitDispatch::Selected(_)
                )
            }
            TypedMatchPattern::Located(_, _, inner) | TypedMatchPattern::As(inner, _) => {
                Self::typed_match_pattern_has_pending_dispatch(inner)
            }
            TypedMatchPattern::Or(items)
            | TypedMatchPattern::Tuple(items)
            | TypedMatchPattern::ErrorPayload { fields: items, .. } => items
                .iter()
                .any(Self::typed_match_pattern_has_pending_dispatch),
            TypedMatchPattern::Constructor { fields, .. } => fields
                .iter()
                .any(Self::typed_match_pattern_has_pending_dispatch),
            TypedMatchPattern::ListCons(head, tail) => {
                Self::typed_match_pattern_has_pending_dispatch(head)
                    || Self::typed_match_pattern_has_pending_dispatch(tail)
            }
            TypedMatchPattern::Extractor { items, .. } => items
                .iter()
                .any(Self::typed_match_pattern_has_pending_dispatch),
            _ => false,
        }
    }

    pub(super) fn typed_pattern_has_pending_dispatch(pattern: &TypedPattern) -> bool {
        match pattern {
            TypedPattern::HashMap(_, entries) => entries.iter().any(|entry| {
                Self::typed_node_has_pending_trait_call(&entry.key)
                    || Self::typed_pattern_has_pending_dispatch(&entry.pattern)
            }),
            TypedPattern::Located(_, inner) => Self::typed_pattern_has_pending_dispatch(inner),
            TypedPattern::Pin(_, _, dispatch) => matches!(dispatch, TraitDispatch::Pending),
            TypedPattern::As(_, inner, _) => Self::typed_pattern_has_pending_dispatch(inner),
            TypedPattern::ListCons(_, head, tail) => {
                Self::typed_pattern_has_pending_dispatch(head)
                    || Self::typed_pattern_has_pending_dispatch(tail)
            }
            TypedPattern::Tuple(_, items)
            | TypedPattern::Constructor { fields: items, .. }
            | TypedPattern::Extractor { items, .. } => {
                items.iter().any(Self::typed_pattern_has_pending_dispatch)
            }
            _ => false,
        }
    }

    pub(super) fn typed_facet_path_has_pending_trait_call(path: &TypedFacetPath) -> bool {
        Self::typed_facet_path_expression_nodes(path).any(Self::typed_node_has_pending_trait_call)
    }

    pub(super) fn pending_facet_path_has_pending_trait_call(path: &PendingFacetPath) -> bool {
        Self::pending_facet_path_expression_nodes(path).any(Self::typed_node_has_pending_trait_call)
    }

    pub(super) fn typed_node_has_pending_trait_call(node: &TypedNode) -> bool {
        if node
            .monad_fail_call()
            .is_some_and(Self::typed_node_has_pending_trait_call)
        {
            return true;
        }
        if Self::pattern_expression_nodes(node)
            .into_iter()
            .any(Self::typed_node_has_pending_trait_call)
        {
            return true;
        }
        match &node.node {
            TypedInner::TraitCall {
                dispatch,
                receiver_ty,
                args,
                ..
            } => {
                matches!(dispatch, TraitDispatch::Pending)
                    || matches!(dispatch, TraitDispatch::Selected(instantiation)
                        if Self::selected_instantiation_has_pending_input(instantiation))
                    || matches!(receiver_ty, Ty::Var(_))
                    || args.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                Self::typed_node_has_pending_trait_call(func)
                    || args.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::Block(stmts) => stmts.iter().any(Self::typed_node_has_pending_trait_call),
            TypedInner::Bind(pattern, rhs)
            | TypedInner::SafeBind(pattern, rhs, _, _)
            | TypedInner::ApplyPattern {
                pattern,
                value: rhs,
                ..
            } => {
                Self::typed_pattern_has_pending_dispatch(pattern)
                    || Self::typed_node_has_pending_trait_call(rhs)
            }
            TypedInner::DoSafeBind(control) => {
                Self::typed_pattern_has_pending_dispatch(&control.pattern)
                    || Self::typed_node_has_pending_trait_call(&control.rhs)
                    || match &control.failure_target {
                        SafeBindFailureTarget::DoAlternative { empty } => {
                            Self::typed_node_has_pending_trait_call(empty)
                        }
                        SafeBindFailureTarget::Deferred(_) => true,
                        _ => false,
                    }
                    || Self::typed_node_has_pending_trait_call(&control.continuation)
            }
            TypedInner::Semi(rhs) => Self::typed_node_has_pending_trait_call(rhs),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right) => {
                Self::typed_node_has_pending_trait_call(left)
                    || Self::typed_node_has_pending_trait_call(right)
            }
            TypedInner::ListCons(head, tail) => {
                Self::typed_node_has_pending_trait_call(head)
                    || Self::typed_node_has_pending_trait_call(tail)
            }
            TypedInner::ErrorConstruct {
                message, payload, ..
            } => {
                Self::typed_node_has_pending_trait_call(message)
                    || payload.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::ListLiteral(items) | TypedInner::TupleLiteral(items) => {
                items.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::HashMapLiteral(entries) => entries.iter().any(|(key, value)| {
                Self::typed_node_has_pending_trait_call(key)
                    || Self::typed_node_has_pending_trait_call(value)
            }),
            TypedInner::InterpolatedStr(parts) => parts.iter().any(|part| match part {
                TypedInterpolatedPart::Text(_) => false,
                TypedInterpolatedPart::Expr(expr) => Self::typed_node_has_pending_trait_call(expr),
            }),
            TypedInner::Dbg(args) => args
                .iter()
                .any(|arg| Self::typed_node_has_pending_trait_call(&arg.expr)),
            TypedInner::EagerBoundary(inner) => Self::typed_node_has_pending_trait_call(inner),
            TypedInner::If(cond, then_branch, else_branch) => {
                Self::typed_node_has_pending_trait_call(cond)
                    || Self::typed_node_has_pending_trait_call(then_branch)
                    || else_branch
                        .as_ref()
                        .is_some_and(|branch| Self::typed_node_has_pending_trait_call(branch))
            }
            TypedInner::Cause(value, err) => {
                Self::typed_node_has_pending_trait_call(value)
                    || Self::typed_node_has_pending_trait_call(err)
            }
            TypedInner::Match(scrutinee, arms) | TypedInner::AssertErrorMatches(scrutinee, arms, _) => {
                Self::typed_node_has_pending_trait_call(scrutinee)
                    || arms.iter().any(|arm| {
                        Self::typed_match_pattern_has_pending_dispatch(&arm.pattern)
                            || arm
                                .guard
                                .as_ref()
                                .is_some_and(Self::typed_node_has_pending_trait_call)
                            || Self::typed_node_has_pending_trait_call(&arm.body)
                    })
            }
            TypedInner::FieldAccess(expr, _) => Self::typed_node_has_pending_trait_call(expr),
            TypedInner::SupervisorSpawn { init, .. } => {
                Self::typed_node_has_pending_trait_call(init)
            }
            TypedInner::SupervisorAdopt { pid, .. } => Self::typed_node_has_pending_trait_call(pid),
            TypedInner::SupervisorStatus { .. } => false,
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                Self::typed_node_has_pending_trait_call(init)
                    || Self::typed_node_has_pending_trait_call(strategy)
            }
            TypedInner::ProcessContextHandler { .. } => false,
            TypedInner::FacetPath(path) => Self::typed_facet_path_has_pending_trait_call(path),
            TypedInner::PendingFacetPath(path) => {
                Self::pending_facet_path_has_pending_trait_call(path)
            }
            TypedInner::FacetView { source, path, .. } => {
                Self::typed_node_has_pending_trait_call(source)
                    || Self::typed_facet_path_has_pending_trait_call(path)
            }
            TypedInner::FacetSet {
                source,
                path,
                value,
                ..
            } => {
                Self::typed_node_has_pending_trait_call(source)
                    || Self::typed_facet_path_has_pending_trait_call(path)
                    || Self::typed_node_has_pending_trait_call(value)
            }
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                ..
            } => {
                Self::typed_node_has_pending_trait_call(source)
                    || Self::typed_facet_path_has_pending_trait_call(path)
                    || Self::typed_node_has_pending_trait_call(update_fun)
            }
            TypedInner::StructLit(_, fields) | TypedInner::ConstructorCall(_, fields) => {
                fields.iter().any(Self::typed_node_has_pending_trait_call)
            }
            TypedInner::DeferrorDef(_, _, _, _, show) => {
                Self::typed_node_has_pending_trait_call(show)
            }
            TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
            | TypedInner::Closure(_, _, body)
            | TypedInner::ExtractorClosure(_, _, body)
            | TypedInner::CaptureClosure(_, _, body) => {
                Self::typed_node_has_pending_trait_call(body)
            }
            TypedInner::CaptureConstructorClosure(_, _, _, body) => {
                Self::typed_node_has_pending_trait_call(body)
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
            | TypedInner::TraitImplDef(..) => false,
        }
    }

    pub(super) fn selected_instantiation_has_pending_input(
        instantiation: &TraitMethodInstantiation,
    ) -> bool {
        let signature = &instantiation.callable_signature;
        signature
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
            .any(CanonicalTy::has_pending_instantiation)
            || instantiation
                .substitution
                .values()
                .chain(instantiation.caller_substitution.values())
                .any(|ty| {
                    let mut variables = Vec::new();
                    Self::collect_ty_vars(ty, &mut variables);
                    !variables.is_empty()
                })
    }
}
