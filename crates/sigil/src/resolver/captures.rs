use super::*;

pub(super) fn collect_captures(
    body: &Resolved,
    params: &[ResolvedClosureParam],
) -> Vec<ResolvedId> {
    let mut bound = HashSet::new();
    for param in params {
        bound.insert(param.id.unique_id);
    }
    let mut free = Vec::new();
    collect_captures_inner(body, &mut bound, &mut free);
    free
}

fn collect_captures_inner(node: &Resolved, bound: &mut HashSet<u32>, free: &mut Vec<ResolvedId>) {
    match node {
        Resolved::Lit(_, _) => {}
        Resolved::Var(_, id) => {
            if !bound.contains(&id.unique_id)
                && !free.iter().any(|seen| seen.unique_id == id.unique_id)
            {
                free.push(id.clone());
            }
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
        Resolved::Block(_, stmts) => {
            let mut local_bound = bound.clone();
            for stmt in stmts {
                collect_captures_inner(stmt, &mut local_bound, free);
                match stmt {
                    Resolved::Bind(_, pat, _) | Resolved::SafeBind(_, pat, _) => {
                        collect_bind_pattern_bindings(pat, &mut local_bound);
                    }
                    Resolved::Def(_, id, params, _, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::ExtractorDef(_, id, param, _, _, _) => {
                        local_bound.insert(id.unique_id);
                        local_bound.insert(param.id.unique_id);
                    }
                    Resolved::BuiltinDecl(_, id, params, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinExtractorDecl(_, id, param, _, _) => {
                        local_bound.insert(id.unique_id);
                        local_bound.insert(param.id.unique_id);
                    }
                    Resolved::BuiltinTypeDecl(_, _, _, _) => {}
                    Resolved::ResultCtorDecl(_, _, _, _, _) => {}
                    Resolved::Closure(_, params, _, _) => {
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
            collect_bind_pattern_bindings(pat, bound);
        }
        Resolved::SafeBind(_, pat, rhs) => {
            collect_captures_inner(rhs, bound, free);
            collect_bind_pattern_bindings(pat, bound);
        }
        Resolved::BinOp(_, _, left, right) => {
            collect_captures_inner(left, bound, free);
            collect_captures_inner(right, bound, free);
        }
        Resolved::Pipe(_, left, right)
        | Resolved::ContextMap(_, left, right)
        | Resolved::ContextBind(_, left, right)
        | Resolved::Compose(_, left, right)
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
        Resolved::TupleLiteral(_, elems) => {
            for elem in elems {
                collect_captures_inner(elem, bound, free);
            }
        }
        Resolved::InterpolatedStr(_, parts) => {
            for part in parts {
                if let ResolvedInterpolatedPart::Expr(expr) = part {
                    collect_captures_inner(expr, bound, free);
                }
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
        Resolved::Match(_, scrutinee, arms) => {
            collect_captures_inner(scrutinee, bound, free);
            for (pat, body) in arms {
                let mut arm_bound = bound.clone();
                collect_bind_pattern_bindings(pat, &mut arm_bound);
                collect_captures_inner(body, &mut arm_bound, free);
            }
        }
        Resolved::FieldAccess(_, expr, _) => collect_captures_inner(expr, bound, free),
        Resolved::StructLit(_, _, fields) => {
            for (_, expr) in fields {
                collect_captures_inner(expr, bound, free);
            }
        }
        Resolved::ConstructorCall(_, _, args) => {
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
        Resolved::StructDef(_, _, _)
        | Resolved::RecordDef(_, _, _)
        | Resolved::DeferrorDef(_, _, _, _)
        | Resolved::EnumDef(_, _, _, _)
        | Resolved::BuiltinDecl(_, _, _, _, _)
        | Resolved::BuiltinExtractorDecl(_, _, _, _, _)
        | Resolved::BuiltinTypeDecl(_, _, _, _)
        | Resolved::ResultCtorDecl(_, _, _, _, _) => {}
        Resolved::Def(_, id, params, _, body, _) => {
            let mut fun_bound = bound.clone();
            fun_bound.insert(id.unique_id);
            for param in params {
                fun_bound.insert(param.id.unique_id);
            }
            collect_captures_inner(body, &mut fun_bound, free);
        }
        Resolved::ExtractorDef(_, id, param, _, body, _) => {
            let mut fun_bound = bound.clone();
            fun_bound.insert(id.unique_id);
            fun_bound.insert(param.id.unique_id);
            collect_captures_inner(body, &mut fun_bound, free);
        }
        Resolved::Closure(_, _, captures, _) => {
            for cap in captures {
                if !bound.contains(&cap.unique_id)
                    && !free.iter().any(|seen| seen.unique_id == cap.unique_id)
                {
                    free.push(cap.clone());
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

fn collect_bind_pattern_bindings(pat: &ResolvedPattern, bound: &mut HashSet<u32>) {
    match pat {
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
            bound.insert(id.unique_id);
        }
        ResolvedPattern::Constructor(_, inners) => {
            for inner in inners {
                collect_bind_pattern_bindings(inner, bound);
            }
        }
        ResolvedPattern::Extractor(_, inners) => {
            for inner in inners {
                collect_bind_pattern_bindings(inner, bound);
            }
        }
        ResolvedPattern::Tuple(items) => {
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
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(_, _)
        | ResolvedPattern::StrLit(_, _)
        | ResolvedPattern::BoolLit(_, _) => {}
    }
}
