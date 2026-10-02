use super::*;

pub(super) fn collect_captures(
    body: &Resolved,
    params: &[ResolvedClosureParam],
) -> Vec<ResolvedId> {
    collect_references(body, params, false)
}

/// Lexical scope validation must inspect references in unresolved Pattern roles,
/// even though ordinary closure capture metadata waits for Scar to select them.
pub(super) fn collect_eager_references(body: &Resolved) -> Vec<ResolvedId> {
    collect_references(body, &[], true)
}

struct CaptureCollection {
    ids: Vec<ResolvedId>,
    include_deferred_patterns: bool,
}

impl CaptureCollection {
    fn reference(&mut self, id: &ResolvedId, bound: &HashSet<u32>) {
        if !bound.contains(&id.unique_id)
            && !self.ids.iter().any(|seen| seen.unique_id == id.unique_id)
        {
            self.ids.push(id.clone());
        }
    }
}

fn collect_references(
    body: &Resolved,
    params: &[ResolvedClosureParam],
    include_deferred_patterns: bool,
) -> Vec<ResolvedId> {
    let mut bound = HashSet::new();
    for param in params {
        bound.insert(param.id.unique_id);
    }
    let mut free = CaptureCollection {
        ids: Vec::new(),
        include_deferred_patterns,
    };
    collect_captures_inner(body, &mut bound, &mut free);
    free.ids
}

fn collect_captures_inner(node: &Resolved, bound: &mut HashSet<u32>, free: &mut CaptureCollection) {
    match node {
        Resolved::ApplyPattern(_, value, pattern) => {
            collect_captures_inner(value, bound, free);
            collect_pattern_captures(pattern, bound, free);
        }
        Resolved::Lit(_, _) => {}
        Resolved::Var(_, id) => {
            free.reference(id, bound);
        }
        Resolved::App(_, func, args) => {
            collect_captures_inner(func, bound, free);
            for arg in args {
                match arg {
                    ResolvedRecordLitArg::Positional(expr)
                    | ResolvedRecordLitArg::Named(_, expr) => {
                        collect_captures_inner(expr, bound, free);
                    }
                }
            }
        }
        Resolved::ReturnTypeArgumentApply(_, target, _) => {
            collect_captures_inner(target, bound, free)
        }
        Resolved::Block(_, stmts) => {
            let mut local_bound = bound.clone();
            for stmt in stmts {
                collect_captures_inner(stmt, &mut local_bound, free);
                match stmt {
                    Resolved::Bind(_, pat, _) | Resolved::SafeBind(_, pat, _) => {
                        collect_bind_pattern_bindings(pat, &mut local_bound);
                    }
                    Resolved::Def(_, id, _, params, _, _, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::ConstDef(_, id, _, _, _) => {
                        local_bound.insert(id.unique_id);
                    }
                    Resolved::ExtractorDef(_, id, _, param, _, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in param {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinDecl(_, id, _, params, _, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinExtractorDecl(_, id, param, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in param {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinTypeDecl(_, _, _, _) => {}
                    Resolved::TypeAlias(_, _, _, _, _) => {}
                    Resolved::ResultCtorDecl(_, _, _, _, _) => {}
                    Resolved::Closure(_, params, _, _)
                    | Resolved::ExtractorClosure(_, params, _, _)
                    | Resolved::CaptureClosure(_, params, _, _) => {
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    _ => {}
                }
            }
        }
        Resolved::Bind(_, pat, rhs) => {
            collect_captures_inner(rhs, bound, free);
            collect_pattern_captures(pat, bound, free);
            collect_bind_pattern_bindings(pat, bound);
        }
        Resolved::SafeBind(_, pat, rhs) => {
            collect_captures_inner(rhs, bound, free);
            collect_pattern_captures(pat, bound, free);
            collect_bind_pattern_bindings(pat, bound);
        }
        Resolved::Do(_, _, _, _, statements) => {
            let mut local_bound = bound.clone();
            for statement in statements {
                match statement {
                    ResolvedDoStatement::Extract { pattern, rhs, .. }
                    | ResolvedDoStatement::SafeBind { pattern, rhs, .. } => {
                        collect_captures_inner(rhs, &mut local_bound, free);
                        collect_pattern_captures(pattern, &local_bound, free);
                        collect_bind_pattern_bindings(pattern, &mut local_bound);
                    }
                    ResolvedDoStatement::Statement(statement) => {
                        collect_captures_inner(statement, &mut local_bound, free);
                    }
                }
            }
        }
        Resolved::BinOp(_, _, left, right) => {
            collect_captures_inner(left, bound, free);
            collect_captures_inner(right, bound, free);
        }
        Resolved::Pipe(_, left, right)
        | Resolved::ContextMap(_, left, right)
        | Resolved::ContextApply(_, left, right)
        | Resolved::ContextBind(_, left, right)
        | Resolved::Compose(_, left, right)
        | Resolved::LiftedCompose(_, left, right)
        | Resolved::KleisliCompose(_, left, right) => {
            collect_captures_inner(left, bound, free);
            collect_captures_inner(right, bound, free);
        }
        Resolved::ListNil(_) => {}
        Resolved::ListCons(_, head, tail) => {
            collect_captures_inner(head, bound, free);
            collect_captures_inner(tail, bound, free);
        }
        Resolved::ListLiteral(_, elems) => {
            for elem in elems {
                collect_captures_inner(elem, bound, free);
            }
        }
        Resolved::HashMapLiteral(_, entries) => {
            for entry in entries {
                collect_captures_inner(&entry.key, bound, free);
                collect_captures_inner(&entry.value, bound, free);
            }
        }
        Resolved::RangeLiteral(_, start, stop) => {
            collect_captures_inner(start, bound, free);
            collect_captures_inner(stop, bound, free);
        }
        Resolved::TupleLiteral(_, elems) => {
            for elem in elems {
                collect_captures_inner(elem, bound, free);
            }
        }
        Resolved::Cond(_, clauses) => {
            for (condition, body) in clauses {
                collect_captures_inner(condition, bound, free);
                collect_captures_inner(body, bound, free);
            }
        }
        Resolved::Grouped(_, inner) => collect_captures_inner(inner, bound, free),
        Resolved::InterpolatedStr(_, parts) => {
            for part in parts {
                if let ResolvedInterpolatedPart::Expr(expr) = part {
                    collect_captures_inner(expr, bound, free);
                }
            }
        }
        Resolved::Dbg(_, args) => {
            for arg in args {
                collect_captures_inner(arg, bound, free);
            }
        }
        Resolved::If(_, cond, then, else_opt) => {
            collect_captures_inner(cond, bound, free);
            collect_captures_inner(then, bound, free);
            if let Some(else_branch) = else_opt {
                collect_captures_inner(else_branch, bound, free);
            }
        }
        Resolved::Assert(_, cond, err) => {
            collect_captures_inner(cond, bound, free);
            collect_captures_inner(err, bound, free);
        }
        Resolved::Ensure(_, value, pred, err) => {
            collect_captures_inner(value, bound, free);
            collect_captures_inner(pred, bound, free);
            collect_captures_inner(err, bound, free);
        }
        Resolved::MapErr(_, value, err) | Resolved::Cause(_, value, err) => {
            collect_captures_inner(value, bound, free);
            collect_captures_inner(err, bound, free);
        }
        Resolved::RecoverKind(_, value, _, handler) => {
            collect_captures_inner(value, bound, free);
            collect_captures_inner(handler, bound, free);
        }
        Resolved::Match(_, scrutinee, arms)
        | Resolved::IsMatch(_, scrutinee, arms)
        | Resolved::IfLet(_, scrutinee, arms, _) => {
            collect_captures_inner(scrutinee, bound, free);
            for arm in arms {
                let mut arm_bound = bound.clone();
                collect_pattern_captures(&arm.pattern, &arm_bound, free);
                collect_bind_pattern_bindings(&arm.pattern, &mut arm_bound);
                if let Some(guard) = &arm.guard {
                    collect_captures_inner(guard, &mut arm_bound, free);
                }
                collect_captures_inner(&arm.body, &mut arm_bound, free);
            }
        }
        Resolved::FieldAccess(_, expr, _) | Resolved::FacetCapture(_, expr) => {
            collect_captures_inner(expr, bound, free)
        }
        Resolved::FacetSegmentAccess(_, expr, segment) => {
            collect_captures_inner(expr, bound, free);
            if let ResolvedFacetPathSegment::Bracket(bracket) = segment {
                collect_captures_inner(&bracket.expr, bound, free);
            }
        }
        Resolved::InferredFacetCapture(_, _) => {}
        Resolved::ProcessContextHandler(_, _) => {}
        Resolved::StructLit(_, _, fields) => {
            for field in fields {
                match field {
                    ResolvedStructLitField::Explicit(_, expr)
                    | ResolvedStructLitField::Shorthand(_, expr) => {
                        collect_captures_inner(expr, bound, free);
                    }
                }
            }
        }
        Resolved::ConstructorCall(_, _, args) | Resolved::EnumConstructorCall(_, _, _, args) => {
            for arg in args {
                match arg {
                    ResolvedRecordLitArg::Positional(expr) => {
                        collect_captures_inner(expr, bound, free)
                    }
                    ResolvedRecordLitArg::Named(_, expr) => {
                        collect_captures_inner(expr, bound, free)
                    }
                }
            }
        }
        Resolved::StructDef(_, _, _, _, _)
        | Resolved::RecordDef(_, _, _, _)
        | Resolved::DeferrorDef(_, _, _, _)
        | Resolved::EnumDef(_, _, _, _, _)
        | Resolved::ConstDef(_, _, _, _, _)
        | Resolved::TraitDef(_, _, _, _, _, _)
        | Resolved::TraitImplDef(_, _, _, _, _, _, _)
        | Resolved::BuiltinDecl(..)
        | Resolved::BuiltinExtractorDecl(_, _, _, _, _)
        | Resolved::BuiltinTypeDecl(_, _, _, _)
        | Resolved::TypeAlias(_, _, _, _, _)
        | Resolved::ResultCtorDecl(_, _, _, _, _) => {}
        Resolved::Def(_, id, _, params, _, _, body, _) => {
            let mut fun_bound = bound.clone();
            fun_bound.insert(id.unique_id);
            for param in params {
                fun_bound.insert(param.id.unique_id);
            }
            collect_captures_inner(body, &mut fun_bound, free);
        }
        Resolved::ExtractorDef(_, id, _, param, _, body, _) => {
            let mut fun_bound = bound.clone();
            fun_bound.insert(id.unique_id);
            for param in param {
                fun_bound.insert(param.id.unique_id);
            }
            collect_captures_inner(body, &mut fun_bound, free);
        }
        Resolved::Closure(_, params, captures, body)
        | Resolved::CaptureClosure(_, params, captures, body)
        | Resolved::ExtractorClosure(_, params, captures, body) => {
            if free.include_deferred_patterns {
                let mut closure_bound = bound.clone();
                for param in params {
                    closure_bound.insert(param.id.unique_id);
                }
                collect_captures_inner(body, &mut closure_bound, free);
            } else {
                for cap in captures {
                    free.reference(cap, bound);
                }
            }
        }
        Resolved::Capture(_, target, args) => {
            collect_captures_inner(target, bound, free);
            for arg in args {
                collect_captures_inner(arg, bound, free);
            }
        }
        Resolved::Semi(_, inner) => collect_captures_inner(inner, bound, free),
    }
}

fn collect_pattern_captures(
    pat: &ResolvedPattern,
    bound: &HashSet<u32>,
    free: &mut CaptureCollection,
) {
    match pat {
        ResolvedPattern::Projection { inner, .. } => collect_pattern_captures(inner, bound, free),
        ResolvedPattern::Deferred { pattern, .. } => {
            if free.include_deferred_patterns {
                collect_pattern_captures(pattern, bound, free);
            }
        }
        ResolvedPattern::ExtractorApplication { head, args } => {
            if free.include_deferred_patterns {
                free.reference(head, bound);
                for arg in args {
                    if let Ok(expr) = &arg.expr {
                        collect_captures_inner(expr, &mut bound.clone(), free);
                    }
                    if let Ok(pattern) = &arg.pattern {
                        collect_pattern_captures(pattern, bound, free);
                    }
                }
            }
        }
        ResolvedPattern::Pin(id) => {
            free.reference(id, bound);
        }
        ResolvedPattern::Constructor(_, inners)
        | ResolvedPattern::Tuple(inners)
        | ResolvedPattern::Or(inners) => {
            for inner in inners {
                collect_pattern_captures(inner, bound, free);
            }
        }
        ResolvedPattern::Record(_, fields) => {
            for (_, inner) in fields {
                collect_pattern_captures(inner, bound, free);
            }
        }
        ResolvedPattern::Extractor(_, pre_args, inners) => {
            for arg in pre_args {
                collect_captures_inner(arg, &mut bound.clone(), free);
            }
            for inner in inners {
                collect_pattern_captures(inner, bound, free);
            }
        }
        ResolvedPattern::As(inner, _, _) => collect_pattern_captures(inner, bound, free),
        ResolvedPattern::ListCons(head, tail) => {
            collect_pattern_captures(head, bound, free);
            collect_pattern_captures(tail, bound, free);
        }
        ResolvedPattern::Var(_)
        | ResolvedPattern::Annotated(_, _)
        | ResolvedPattern::Wildcard(_)
        | ResolvedPattern::AnnotatedWildcard(_, _)
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(_, _)
        | ResolvedPattern::StrLit(_, _)
        | ResolvedPattern::BoolLit(_, _)
        | ResolvedPattern::DurationLit(_, _) => {}
    }
}

fn collect_bind_pattern_bindings(pat: &ResolvedPattern, bound: &mut HashSet<u32>) {
    match pat {
        ResolvedPattern::Projection { inner, .. } => collect_bind_pattern_bindings(inner, bound),
        // Signature-dependent captures are finalized from canonical Typed IDs.
        ResolvedPattern::Deferred { .. } | ResolvedPattern::ExtractorApplication { .. } => {}
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
            bound.insert(id.unique_id);
        }
        ResolvedPattern::Constructor(_, inners) => {
            for inner in inners {
                collect_bind_pattern_bindings(inner, bound);
            }
        }
        ResolvedPattern::Record(_, fields) => {
            for (_, inner) in fields {
                collect_bind_pattern_bindings(inner, bound);
            }
        }
        ResolvedPattern::Extractor(_, _, inners) => {
            for inner in inners {
                collect_bind_pattern_bindings(inner, bound);
            }
        }
        ResolvedPattern::Tuple(items) | ResolvedPattern::Or(items) => {
            for item in items {
                collect_bind_pattern_bindings(item, bound);
            }
        }
        ResolvedPattern::As(inner, id, _) => {
            bound.insert(id.unique_id);
            collect_bind_pattern_bindings(inner, bound);
        }
        ResolvedPattern::ListCons(head, tail) => {
            collect_bind_pattern_bindings(head, bound);
            collect_bind_pattern_bindings(tail, bound);
        }
        ResolvedPattern::Wildcard(_)
        | ResolvedPattern::AnnotatedWildcard(_, _)
        | ResolvedPattern::Pin(_)
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(_, _)
        | ResolvedPattern::StrLit(_, _)
        | ResolvedPattern::BoolLit(_, _)
        | ResolvedPattern::DurationLit(_, _) => {}
    }
}
