use super::*;

// Visit runtime expressions, including pattern expressions. Named definitions
// are separate function boundaries and receive their own normalization pass.
fn children(node: &mut TypedNode, visitor: &mut dyn FnMut(&mut TypedNode)) {
    for child in Checker::pattern_expression_nodes_mut(node) {
        visitor(child);
    }
    fn failure(target: &mut SafeBindFailureTarget, visitor: &mut dyn FnMut(&mut TypedNode)) {
        match target {
            SafeBindFailureTarget::EnclosingMonadFail(target)
            | SafeBindFailureTarget::DoMonadFail(target) => visitor(&mut target.call),
            SafeBindFailureTarget::DoAlternative { empty } => visitor(empty),
            SafeBindFailureTarget::EnclosingMatchResultContext { .. }
            | SafeBindFailureTarget::TopLevel
            | SafeBindFailureTarget::Deferred(_) => {}
        }
    }
    fn path(path: &mut TypedFacetPath, visitor: &mut dyn FnMut(&mut TypedNode)) {
        for segment in &mut path.segments {
            match segment {
                TypedFacetSegment::ListIndex { index, .. } => visitor(index),
                TypedFacetSegment::ListRange { start, end, .. } => {
                    visitor(start);
                    visitor(end);
                }
                TypedFacetSegment::MapKey { key, .. } => visitor(key),
                TypedFacetSegment::Field { .. }
                | TypedFacetSegment::Tuple { .. }
                | TypedFacetSegment::ReadonlyBuiltin { .. }
                | TypedFacetSegment::ErrorPayload { .. }
                | TypedFacetSegment::Variant { .. } => {}
            }
        }
    }
    match &mut node.node {
        TypedInner::ErrorConstruct {
            message, payload, ..
        } => {
            visitor(message);
            for item in payload {
                visitor(item);
            }
        }
        TypedInner::TraitCall { args, .. } => {
            for item in args {
                visitor(item);
            }
        }
        TypedInner::App(function, args)
        | TypedInner::InjectCall(function, args)
        | TypedInner::Capture(function, args) => {
            visitor(function);
            for item in args {
                visitor(item);
            }
        }
        TypedInner::Block(items)
        | TypedInner::TupleLiteral(items)
        | TypedInner::ListLiteral(items)
        | TypedInner::ConstructorCall(_, items)
        | TypedInner::StructLit(_, items) => {
            for item in items {
                visitor(item);
            }
        }
        TypedInner::HashMapLiteral(entries) => {
            for (key, value) in entries {
                visitor(key);
                visitor(value);
            }
        }
        TypedInner::Bind(_, rhs)
        | TypedInner::ApplyPattern { value: rhs, .. }
        | TypedInner::Semi(rhs)
        | TypedInner::FieldAccess(rhs, _)
        | TypedInner::EagerBoundary(rhs) => visitor(rhs),
        TypedInner::SafeBind(_, rhs, _, target) => {
            visitor(rhs);
            failure(target, visitor);
        }
        TypedInner::DoSafeBind(control) => {
            visitor(&mut control.rhs);
            visitor(&mut control.continuation);
            failure(&mut control.failure_target, visitor);
        }
        TypedInner::BinOp(_, left, right)
        | TypedInner::Pipe(left, right)
        | TypedInner::Compose(_, left, right)
        | TypedInner::ListCons(left, right)
        | TypedInner::Cause(left, right) => {
            visitor(left);
            visitor(right);
        }
        TypedInner::If(cond, yes, no) => {
            visitor(cond);
            visitor(yes);
            if let Some(no) = no {
                visitor(no);
            }
        }
        TypedInner::Match(value, arms) | TypedInner::AssertErrorMatches(value, arms, _) => {
            visitor(value);
            for arm in arms {
                if let Some(guard) = &mut arm.guard {
                    visitor(guard);
                }
                visitor(&mut arm.body);
            }
        }
        TypedInner::InterpolatedStr(parts) => {
            for part in parts {
                if let TypedInterpolatedPart::Expr(expr) = part {
                    visitor(expr);
                }
            }
        }
        TypedInner::Dbg(args) => {
            for arg in args {
                visitor(&mut arg.expr);
            }
        }
        TypedInner::SupervisorSpawn { init, .. } => visitor(init),
        TypedInner::SupervisorAdopt { pid, .. } => visitor(pid),
        TypedInner::SupervisorWorkers { init, strategy, .. } => {
            visitor(init);
            visitor(strategy);
        }
        TypedInner::PendingFacetPath(path) => {
            for segment in &mut path.segments {
                match segment {
                    PendingFacetSegment::Bracket {
                        expr: PendingFacetExpr::Typed(expr),
                        ..
                    } => visitor(expr),
                    PendingFacetSegment::RangeBracket { start, end, .. } => {
                        if let PendingFacetExpr::Typed(expr) = start {
                            visitor(expr);
                        }
                        if let PendingFacetExpr::Typed(expr) = end {
                            visitor(expr);
                        }
                    }
                    _ => {}
                }
            }
        }
        TypedInner::FacetPath(value) => path(value, visitor),
        TypedInner::FacetView {
            path: value,
            source,
            ..
        } => {
            path(value, visitor);
            visitor(source);
        }
        TypedInner::FacetSet {
            path: value,
            source,
            value: rhs,
            ..
        } => {
            path(value, visitor);
            visitor(source);
            visitor(rhs);
        }
        TypedInner::FacetOver {
            path: value,
            source,
            update_fun,
            ..
        } => {
            path(value, visitor);
            visitor(source);
            visitor(update_fun);
        }
        TypedInner::Closure(_, _, body)
        | TypedInner::CaptureClosure(_, _, body)
        | TypedInner::ExtractorClosure(_, _, body)
        | TypedInner::CaptureConstructorClosure(_, _, _, body) => visitor(body),
        TypedInner::DeferrorDef(..)
        | TypedInner::Def(..)
        | TypedInner::ExtractorDef(..)
        | TypedInner::Lit(_)
        | TypedInner::ErrorKind(_)
        | TypedInner::Var(_)
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
}

fn is_closure(node: &TypedNode) -> bool {
    matches!(
        node.node,
        TypedInner::Closure(..)
            | TypedInner::CaptureClosure(..)
            | TypedInner::ExtractorClosure(..)
            | TypedInner::CaptureConstructorClosure(..)
    )
}

fn replace_getters(node: &mut TypedNode, bindings: &mut Vec<(ResolvedId, TypedNode)>) {
    if let TypedInner::ProcessContextHandler { process_name, slot } = &node.node {
        let existing = bindings.iter().find(|(_, getter)| matches!(&getter.node,
            TypedInner::ProcessContextHandler { process_name: name, slot: key } if name == process_name && key == slot));
        let id = if let Some((id, _)) = existing {
            id.clone()
        } else {
            let uid = Checker::next_synthetic_range_uid();
            let id = ResolvedId {
                name: format!("__ctx_handler_{slot}_{uid}"),
                qualified_name: None,
                symbol_info: None,
                unique_id: uid,
                compiler_generated: true,
                span: node.span.clone(),
            };
            bindings.push((id.clone(), node.clone()));
            id
        };
        node.node = TypedInner::Var(id);
        return;
    }
    children(node, &mut |child| replace_getters(child, bindings));
}

fn add_captures(node: &mut TypedNode, hidden: &HashSet<u32>) {
    children(node, &mut |child| add_captures(child, hidden));
    match &mut node.node {
        TypedInner::Closure(_, captures, body)
        | TypedInner::CaptureClosure(_, captures, body)
        | TypedInner::ExtractorClosure(_, captures, body)
        | TypedInner::CaptureConstructorClosure(_, _, captures, body) => {
            for id in Checker::finalized_closure_captures(body, hidden) {
                if !captures.iter().any(|old| old.unique_id == id.unique_id) {
                    captures.push(id);
                }
            }
        }
        _ => {}
    }
}

impl Checker {
    pub(super) fn normalize_handler_captures(node: &mut TypedNode) {
        if !is_closure(node) {
            children(node, &mut Self::normalize_handler_captures);
            return;
        }
        let mut bindings = Vec::new();
        replace_getters(node, &mut bindings);
        if bindings.is_empty() {
            return;
        }
        let hidden = bindings.iter().map(|(id, _)| id.unique_id).collect();
        add_captures(node, &hidden);
        let original = TypedNode {
            ty: node.ty.clone(),
            span: node.span.clone(),
            node: std::mem::replace(&mut node.node, TypedInner::Block(Vec::new())),
        };
        let mut items: Vec<_> = bindings
            .into_iter()
            .map(|(id, getter)| TypedNode {
                ty: Ty::Unit,
                span: getter.span.clone(),
                node: TypedInner::Bind(TypedPattern::Var(getter.ty.clone(), id), Box::new(getter)),
            })
            .collect();
        items.push(original);
        node.node = TypedInner::Block(items);
    }
}
