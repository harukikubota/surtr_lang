use super::*;

impl Checker {
    /// Runtime expression children, including expressions owned by Patterns.
    fn capture_expression_children(node: &TypedNode) -> Vec<&TypedNode> {
        fn path_expressions<'a>(path: &'a TypedFacetPath, children: &mut Vec<&'a TypedNode>) {
            for segment in &path.segments {
                match segment {
                    TypedFacetSegment::ListIndex { index, .. } => children.push(index),
                    TypedFacetSegment::ListRange { start, end, .. } => {
                        children.push(start);
                        children.push(end);
                    }
                    TypedFacetSegment::MapKey { key, .. } => children.push(key),
                    TypedFacetSegment::Field { .. }
                    | TypedFacetSegment::Tuple { .. }
                    | TypedFacetSegment::Variant { .. } => {}
                }
            }
        }
        let mut children = Self::pattern_expression_nodes(node);
        match &node.node {
            TypedInner::TraitCall { args, .. } => children.extend(args),
            TypedInner::App(function, args)
            | TypedInner::InjectCall(function, args)
            | TypedInner::Capture(function, args) => {
                children.push(function);
                children.extend(args);
            }
            TypedInner::Block(items)
            | TypedInner::TupleLiteral(items)
            | TypedInner::ListLiteral(items)
            | TypedInner::ConstructorCall(_, items)
            | TypedInner::StructLit(_, items) => children.extend(items),
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    children.push(key);
                    children.push(value);
                }
            }
            TypedInner::Bind(_, rhs)
            | TypedInner::ApplyPattern { value: rhs, .. }
            | TypedInner::SafeBind(_, rhs, ..)
            | TypedInner::Semi(rhs)
            | TypedInner::FieldAccess(rhs, _)
            | TypedInner::EagerBoundary(rhs) => children.push(rhs),
            TypedInner::DoSafeBind(control) => {
                children.push(&control.rhs);
                children.push(&control.continuation);
                if let SafeBindFailureTarget::DoAlternative { empty } = &control.failure_target {
                    children.push(empty);
                }
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right)
            | TypedInner::Assert(left, right)
            | TypedInner::MapErr(left, right)
            | TypedInner::Cause(left, right) => {
                children.push(left);
                children.push(right);
            }
            TypedInner::If(cond, then_branch, else_branch) => {
                children.push(cond);
                children.push(then_branch);
                children.extend(else_branch.as_deref());
            }
            TypedInner::RecoverKind(a, _, c) => {
                children.push(a);
                children.push(c);
            }
            TypedInner::Ensure(a, b, c) => {
                children.push(a);
                children.push(b);
                children.push(c);
            }
            TypedInner::Match(value, arms) => {
                children.push(value);
                for arm in arms {
                    children.extend(arm.guard.as_ref());
                    children.push(&arm.body);
                }
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        children.push(expr);
                    }
                }
            }
            TypedInner::Dbg(args) => children.extend(args.iter().map(|arg| &arg.expr)),
            TypedInner::DeferrorDef(_, _, _, _, body)
            | TypedInner::Def(_, _, _, _, _, _, body, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, body, _) => children.push(body),
            TypedInner::SupervisorSpawn { init, .. } => children.push(init),
            TypedInner::SupervisorAdopt { pid, .. } => children.push(pid),
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                children.push(init);
                children.push(strategy);
            }
            TypedInner::PendingFacetPath(path) => {
                for segment in &path.segments {
                    let expressions = match segment {
                        PendingFacetSegment::Bracket { expr, .. } => vec![expr],
                        PendingFacetSegment::RangeBracket { start, end, .. } => vec![start, end],
                        PendingFacetSegment::Field { .. } => Vec::new(),
                    };
                    for expr in expressions {
                        if let PendingFacetExpr::Typed(node) = expr {
                            children.push(node);
                        }
                    }
                }
            }
            TypedInner::FacetPath(path) => path_expressions(path, &mut children),
            TypedInner::FacetView { path, source, .. } => {
                path_expressions(path, &mut children);
                children.push(source);
            }
            TypedInner::FacetSet {
                path,
                source,
                value,
                ..
            } => {
                path_expressions(path, &mut children);
                children.push(source);
                children.push(value);
            }
            TypedInner::FacetOver {
                path,
                source,
                update_fun,
                ..
            } => {
                path_expressions(path, &mut children);
                children.push(source);
                children.push(update_fun);
            }
            // A nested callable's completed capture list is its lexical dependency.
            TypedInner::Closure(..)
            | TypedInner::CaptureClosure(..)
            | TypedInner::CaptureConstructorClosure(..)
            | TypedInner::ExtractorClosure(..)
            | TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ResultEffectFailure(_)
            | TypedInner::DeferredDoFailure(_)
            | TypedInner::ListNil
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..) => {}
        }
        children
    }

    pub(super) fn finalized_closure_captures(
        body: &TypedNode,
        outer_ids: &HashSet<u32>,
    ) -> Vec<ResolvedId> {
        fn reference(id: &ResolvedId, outer: &HashSet<u32>, out: &mut Vec<ResolvedId>) {
            if outer.contains(&id.unique_id)
                && !out
                    .iter()
                    .any(|existing| existing.unique_id == id.unique_id)
            {
                out.push(id.clone());
            }
        }
        fn binding(pat: &TypedPattern, outer: &HashSet<u32>, out: &mut Vec<ResolvedId>) {
            match pat {
                TypedPattern::Located(_, inner) => binding(inner, outer, out),
                TypedPattern::Pin(_, id, _) => reference(id, outer, out),
                TypedPattern::Extractor {
                    extractor,
                    extractor_ty,
                    items,
                    ..
                } => {
                    if matches!(extractor_ty, Ty::ExtractorClosure(_)) {
                        reference(extractor, outer, out);
                    }
                    for item in items {
                        binding(item, outer, out);
                    }
                }
                TypedPattern::As(_, inner, _) => binding(inner, outer, out),
                TypedPattern::ListCons(_, a, b) => {
                    binding(a, outer, out);
                    binding(b, outer, out);
                }
                TypedPattern::Tuple(_, items) | TypedPattern::Constructor { fields: items, .. } => {
                    for item in items {
                        binding(item, outer, out);
                    }
                }
                _ => {}
            }
        }
        fn matching(pat: &TypedMatchPattern, outer: &HashSet<u32>, out: &mut Vec<ResolvedId>) {
            match pat {
                TypedMatchPattern::Pin { id, .. } => reference(id, outer, out),
                TypedMatchPattern::Extractor {
                    extractor,
                    extractor_ty,
                    items,
                    ..
                } => {
                    if matches!(extractor_ty, Ty::ExtractorClosure(_)) {
                        reference(extractor, outer, out);
                    }
                    for item in items {
                        matching(item, outer, out);
                    }
                }
                TypedMatchPattern::As(inner, _) => matching(inner, outer, out),
                TypedMatchPattern::ListCons(a, b) => {
                    matching(a, outer, out);
                    matching(b, outer, out);
                }
                TypedMatchPattern::Tuple(items)
                | TypedMatchPattern::Or(items)
                | TypedMatchPattern::Constructor { fields: items, .. } => {
                    for item in items {
                        matching(item, outer, out);
                    }
                }
                _ => {}
            }
        }
        fn collect(node: &TypedNode, outer: &HashSet<u32>, out: &mut Vec<ResolvedId>) {
            match &node.node {
                TypedInner::Var(id) => reference(id, outer, out),
                TypedInner::Closure(_, captures, _)
                | TypedInner::CaptureClosure(_, captures, _)
                | TypedInner::ExtractorClosure(_, captures, _) => {
                    for id in captures {
                        reference(id, outer, out);
                    }
                }
                TypedInner::CaptureConstructorClosure(_, _, captures, _) => {
                    for id in captures {
                        reference(id, outer, out);
                    }
                }
                TypedInner::Bind(pattern, _)
                | TypedInner::SafeBind(pattern, ..)
                | TypedInner::ApplyPattern { pattern, .. } => binding(pattern, outer, out),
                TypedInner::DoSafeBind(control) => binding(&control.pattern, outer, out),
                TypedInner::Match(_, arms) => {
                    for arm in arms {
                        matching(&arm.pattern, outer, out);
                    }
                }
                _ => {}
            }
            for child in Checker::capture_expression_children(node) {
                collect(child, outer, out);
            }
        }
        let mut captures = Vec::new();
        collect(body, outer_ids, &mut captures);
        captures
    }
}
