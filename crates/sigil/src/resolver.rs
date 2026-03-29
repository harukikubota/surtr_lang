#![allow(unused_imports)]

use std::collections::HashSet;

use spire::ast::{Ast, AstPattern, BinOp, ClosureParam, FunParam, Lit, RecordLitArg, Span};

use crate::error::ResolveError;
use crate::resolved::*;
use crate::scope::Scope;

/// Built-in function names that Sigil pre-registers.
const BUILTIN_NAMES: &[&str] = &["print", "to_string", "eprint"];

fn initialize_scope() -> Scope {
    let mut scope = Scope::new();
    let dummy = Span { start: 0, end: 0 };
    scope.define("Ok", dummy.clone());
    scope.define("Err", dummy);
    for name in BUILTIN_NAMES {
        scope.define(name, Span { start: 0, end: 0 });
    }
    scope
}

/// Resolve all identifiers in the AST to unique references.
pub fn resolve(ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
    let mut resolver = Resolver::new();
    resolver.resolve_program(ast)
}

#[derive(Debug, Clone)]
pub struct SigilCheckpoint {
    scope: Scope,
}

#[derive(Debug, Clone)]
pub struct SigilSession {
    scope: Scope,
}

impl SigilSession {
    pub fn new() -> Self {
        Self {
            scope: initialize_scope(),
        }
    }

    pub fn resolve(&mut self, ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        let mut resolver = Resolver::with_scope(self.scope.clone());
        let resolved = resolver.resolve_program(ast)?;
        self.scope = resolver.into_scope();
        Ok(resolved)
    }

    pub fn checkpoint(&self) -> SigilCheckpoint {
        SigilCheckpoint {
            scope: self.scope.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: SigilCheckpoint) {
        self.scope = checkpoint.scope;
    }
}

impl Default for SigilSession {
    fn default() -> Self {
        Self::new()
    }
}

struct Resolver {
    scope: Scope,
}

impl Resolver {
    fn new() -> Self {
        Self {
            scope: initialize_scope(),
        }
    }

    fn with_scope(scope: Scope) -> Self {
        Self { scope }
    }

    fn into_scope(self) -> Scope {
        self.scope
    }

    fn resolve_program(&mut self, stmts: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        self.predeclare_functions(&stmts);
        let mut resolved = Vec::new();
        for stmt in stmts {
            resolved.push(self.resolve_node(stmt)?);
        }
        Ok(resolved)
    }

    fn predeclare_functions(&mut self, stmts: &[Ast]) {
        for stmt in stmts {
            match stmt {
                Ast::Def(_, name, _, _, _) | Ast::BuiltinDecl(_, name, _, _) => {
                    if self.scope.lookup(name).is_none() {
                        let uid = self.scope.reserve_id();
                        self.scope.define_with_id(name, uid);
                    }
                }
                _ => {}
            }
        }
    }

    fn resolve_node(&mut self, node: Ast) -> Result<Resolved, ResolveError> {
        match node {
            Ast::Lit(span, lit) => Ok(Resolved::Lit(span, lit)),

            Ast::Var(span, name) => {
                let uid = self.scope.lookup(&name).ok_or_else(|| ResolveError {
                    message: format!("Undefined variable: {}", name),
                    span: span.clone(),
                })?;
                Ok(Resolved::Var(
                    span.clone(),
                    ResolvedId {
                        name,
                        unique_id: uid,
                        span,
                    },
                ))
            }

            Ast::App(span, func, args) => {
                // Check for `if` special form
                if let Ast::Var(_, ref name) = *func {
                    if name == "if" {
                        return self.resolve_if(span, args);
                    }
                }

                let resolved_func = self.resolve_node(*func)?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(expr)?))
                        }
                        RecordLitArg::Named(name, expr) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(expr)?))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::App(span, Box::new(resolved_func), resolved_args))
            }

            Ast::Bind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::Bind(span, resolved_pat, Box::new(resolved_rhs)))
            }

            Ast::SafeBind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::SafeBind(
                    span,
                    resolved_pat,
                    Box::new(resolved_rhs),
                ))
            }

            Ast::BinOp(span, op, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::BinOp(span, op, Box::new(l), Box::new(r)))
            }

            Ast::List(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::List(span, resolved))
            }

            Ast::InterpolatedStr(span, parts) => {
                let mut resolved_parts = Vec::new();
                for part in parts {
                    match part {
                        spire::ast::InterpolatedPart::Text(s) => {
                            resolved_parts.push(ResolvedInterpolatedPart::Text(s));
                        }
                        spire::ast::InterpolatedPart::Expr(expr) => {
                            let resolved_expr = self.resolve_node(*expr)?;
                            resolved_parts
                                .push(ResolvedInterpolatedPart::Expr(Box::new(resolved_expr)));
                        }
                    }
                }
                Ok(Resolved::InterpolatedStr(span, resolved_parts))
            }

            Ast::FieldAccess(span, expr, field) => {
                let resolved_expr = self.resolve_node(*expr)?;
                Ok(Resolved::FieldAccess(span, Box::new(resolved_expr), field))
            }

            Ast::Block(span, stmts) => {
                let resolved = stmts
                    .into_iter()
                    .map(|s| self.resolve_node(s))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::Block(span, resolved))
            }

            Ast::Semi(span, inner) => {
                let resolved = self.resolve_node(*inner)?;
                Ok(Resolved::Semi(span, Box::new(resolved)))
            }

            // Struct/Record/Deferror definitions — register type names
            Ast::StructDef(span, name, fields) => {
                let uid = self.scope.define(&name, span.clone());
                let rid = ResolvedId {
                    name,
                    unique_id: uid,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| ResolvedField {
                        id: None,
                        name: f.name,
                        ty: f.ty,
                        span: f.span,
                    })
                    .collect();
                Ok(Resolved::StructDef(span, rid, rfields))
            }

            Ast::RecordDef(span, name, fields) => {
                let uid = self.scope.define(&name, span.clone());
                let rid = ResolvedId {
                    name,
                    unique_id: uid,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| ResolvedField {
                        id: None,
                        name: f.name,
                        ty: f.ty,
                        span: f.span,
                    })
                    .collect();
                Ok(Resolved::RecordDef(span, rid, rfields))
            }

            Ast::DeferrorDef(span, name, fields, show_expr) => {
                let uid = self.scope.define(&name, span.clone());
                let rid = ResolvedId {
                    name,
                    unique_id: uid,
                    span: span.clone(),
                };
                let mut error_scope = self.scope.clone();
                let mut rfields = Vec::new();
                for f in fields {
                    let uid = error_scope.define(&f.name, f.span.clone());
                    rfields.push(ResolvedField {
                        id: Some(ResolvedId {
                            name: f.name.clone(),
                            unique_id: uid,
                            span: f.span.clone(),
                        }),
                        name: f.name,
                        ty: f.ty,
                        span: f.span,
                    });
                }
                let mut show_resolver = Resolver::with_scope(error_scope);
                let resolved_show = show_resolver.resolve_node(*show_expr)?;
                self.scope.advance_next_id_to(show_resolver.scope.next_id());
                Ok(Resolved::DeferrorDef(
                    span,
                    rid,
                    rfields,
                    Box::new(resolved_show),
                ))
            }

            Ast::Def(span, name, params, ret_ty, body) => {
                let fun_uid = self
                    .scope
                    .lookup(&name)
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_resolver = Resolver::with_scope(self.scope.clone());
                let resolved_params = params
                    .into_iter()
                    .map(|param| body_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let rid = ResolvedId {
                    name,
                    unique_id: fun_uid,
                    span: span.clone(),
                };

                Ok(Resolved::Def(
                    span,
                    rid,
                    resolved_params,
                    ret_ty,
                    Box::new(resolved_body),
                ))
            }

            Ast::BuiltinDecl(span, name, params, ret_ty) => {
                if !BUILTIN_NAMES.contains(&name.as_str()) {
                    return Err(ResolveError {
                        message: format!("Unknown builtin declaration: {}", name),
                        span,
                    });
                }

                let builtin_uid = self
                    .scope
                    .lookup(&name)
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut decl_resolver = Resolver::with_scope(self.scope.clone());
                let resolved_params = params
                    .into_iter()
                    .map(|param| decl_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                self.scope.advance_next_id_to(decl_resolver.scope.next_id());
                self.scope.define_with_id(&name, builtin_uid);
                let rid = ResolvedId {
                    name,
                    unique_id: builtin_uid,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinDecl(span, rid, resolved_params, ret_ty))
            }

            Ast::Closure(span, params, body) => {
                let mut closure_scope = self.scope.clone();
                let mut resolved_params = Vec::new();
                for param in params {
                    let uid = closure_scope.define(&param.name, param.span.clone());
                    resolved_params.push(ResolvedClosureParam {
                        id: ResolvedId {
                            name: param.name,
                            unique_id: uid,
                            span: param.span,
                        },
                    });
                }

                let mut body_resolver = Resolver::with_scope(closure_scope);
                let resolved_body = body_resolver.resolve_node(*body)?;
                self.scope.advance_next_id_to(body_resolver.scope.next_id());

                let captures = collect_captures(&resolved_body, &resolved_params);

                Ok(Resolved::Closure(
                    span,
                    resolved_params,
                    captures,
                    Box::new(resolved_body),
                ))
            }

            Ast::Capture(span, target, args) => {
                let resolved_target = self.resolve_node(*target)?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| self.resolve_node(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::Capture(
                    span,
                    Box::new(resolved_target),
                    resolved_args,
                ))
            }

            Ast::StructLit(span, type_name, field_vals) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                })?;
                let rid = ResolvedId {
                    name: type_name,
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_fields = field_vals
                    .into_iter()
                    .map(|(name, expr)| Ok((name, self.resolve_node(expr)?)))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructLit(span, rid, resolved_fields))
            }

            Ast::ConstructorCall(span, type_name, args) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                })?;
                let rid = ResolvedId {
                    name: type_name,
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        spire::ast::RecordLitArg::Positional(e) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(e)?))
                        }
                        spire::ast::RecordLitArg::Named(name, e) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(e)?))
                        }
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::ConstructorCall(span, rid, resolved_args))
            }

            Ast::Match(span, scrutinee, arms) => {
                let resolved_scrut = self.resolve_node(*scrutinee)?;
                let resolved_arms = arms
                    .into_iter()
                    .map(|(pat, body)| {
                        let (rpat, body) = self.resolve_match_arm(pat, body)?;
                        Ok((rpat, body))
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::Match(
                    span,
                    Box::new(resolved_scrut),
                    resolved_arms,
                ))
            }
        }
    }

    fn resolve_fun_param(&mut self, param: FunParam) -> Result<ResolvedFunParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedFunParam {
            id: ResolvedId {
                name: param.name,
                unique_id: uid,
                span: param.span,
            },
            ty: param.ty,
        })
    }

    fn resolve_if(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(ResolveError {
                message: format!("if expects 2 or 3 arguments, got {}", args.len()),
                span,
            });
        }

        let mut positional = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(expr) => positional.push(expr),
                RecordLitArg::Named(name, _) => {
                    return Err(ResolveError {
                        message: format!("if does not accept named argument '{}'", name),
                        span,
                    });
                }
            }
        }

        let mut iter = positional.into_iter();
        let cond = self.resolve_node(iter.next().expect("checked arg length"))?;
        let then = self.resolve_node(iter.next().expect("checked arg length"))?;
        let else_branch = match iter.next() {
            Some(e) => Some(Box::new(self.resolve_node(e)?)),
            None => None,
        };
        Ok(Resolved::If(
            span,
            Box::new(cond),
            Box::new(then),
            else_branch,
        ))
    }

    fn resolve_pattern(&mut self, pat: AstPattern) -> Result<ResolvedPattern, ResolveError> {
        match pat {
            AstPattern::Var(span, name) => {
                let uid = self.scope.define(&name, span.clone());
                Ok(ResolvedPattern::Var(ResolvedId {
                    name,
                    unique_id: uid,
                    span,
                }))
            }
            AstPattern::Annotated(span, name, ty) => {
                let uid = self.scope.define(&name, span.clone());
                Ok(ResolvedPattern::Annotated(
                    ResolvedId {
                        name,
                        unique_id: uid,
                        span,
                    },
                    ty,
                ))
            }
            AstPattern::Wildcard(span) => Ok(ResolvedPattern::Wildcard(span)),
        }
    }

    fn resolve_match_arm(
        &mut self,
        pat: spire::ast::AstMatchPattern,
        body: Ast,
    ) -> Result<(ResolvedMatchPattern, Resolved), ResolveError> {
        match pat {
            spire::ast::AstMatchPattern::Wildcard(span) => {
                let resolved_body = self.resolve_node(body)?;
                Ok((ResolvedMatchPattern::Wildcard(span), resolved_body))
            }
            spire::ast::AstMatchPattern::BoolLit(span, b) => {
                let resolved_body = self.resolve_node(body)?;
                Ok((ResolvedMatchPattern::BoolLit(span, b), resolved_body))
            }
            spire::ast::AstMatchPattern::IntLit(span, n) => {
                let resolved_body = self.resolve_node(body)?;
                Ok((ResolvedMatchPattern::IntLit(span, n), resolved_body))
            }
            spire::ast::AstMatchPattern::StrLit(span, s) => {
                let resolved_body = self.resolve_node(body)?;
                Ok((ResolvedMatchPattern::StrLit(span, s), resolved_body))
            }
            spire::ast::AstMatchPattern::Constructor(span, ctor_name, inner_name) => {
                let ctor_uid = self.scope.lookup(&ctor_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined constructor: {}", ctor_name),
                    span: span.clone(),
                })?;
                let ctor_id = ResolvedId {
                    name: ctor_name,
                    unique_id: ctor_uid,
                    span: span.clone(),
                };
                let inner_id = match inner_name {
                    Some(name) => {
                        let uid = self.scope.define(&name, span.clone());
                        Some(ResolvedId {
                            name,
                            unique_id: uid,
                            span: span.clone(),
                        })
                    }
                    None => None,
                };
                let resolved_body = self.resolve_node(body)?;
                Ok((
                    ResolvedMatchPattern::Constructor(span, ctor_id, inner_id),
                    resolved_body,
                ))
            }
        }
    }
}

fn collect_captures(body: &Resolved, params: &[ResolvedClosureParam]) -> Vec<ResolvedId> {
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
                    Resolved::Bind(_, pat, _) => match pat {
                        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
                            local_bound.insert(id.unique_id);
                        }
                        ResolvedPattern::Wildcard(_) => {}
                    },
                    Resolved::SafeBind(_, pat, _) => match pat {
                        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
                            local_bound.insert(id.unique_id);
                        }
                        ResolvedPattern::Wildcard(_) => {}
                    },
                    Resolved::Def(_, id, params, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinDecl(_, id, params, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
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
            match pat {
                ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
                    bound.insert(id.unique_id);
                }
                ResolvedPattern::Wildcard(_) => {}
            }
        }
        Resolved::SafeBind(_, pat, rhs) => {
            collect_captures_inner(rhs, bound, free);
            match pat {
                ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
                    bound.insert(id.unique_id);
                }
                ResolvedPattern::Wildcard(_) => {}
            }
        }
        Resolved::BinOp(_, _, left, right) => {
            collect_captures_inner(left, bound, free);
            collect_captures_inner(right, bound, free);
        }
        Resolved::List(_, elems) => {
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
        Resolved::Match(_, scrutinee, arms) => {
            collect_captures_inner(scrutinee, bound, free);
            for (pat, body) in arms {
                let mut arm_bound = bound.clone();
                if let ResolvedMatchPattern::Constructor(_, _, Some(inner)) = pat {
                    arm_bound.insert(inner.unique_id);
                }
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
        | Resolved::BuiltinDecl(_, _, _, _) => {}
        Resolved::Def(_, id, params, _, body) => {
            let mut fun_bound = bound.clone();
            fun_bound.insert(id.unique_id);
            for param in params {
                fun_bound.insert(param.id.unique_id);
            }
            collect_captures_inner(body, &mut fun_bound, free);
        }
        Resolved::Closure(_, _, captures, _) => {
            for cap in captures {
                if !free.iter().any(|seen| seen.unique_id == cap.unique_id) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_resolve(src: &str) -> Result<Vec<Resolved>, ResolveError> {
        let ast = spire::parse(src).expect("parse failed");
        resolve(ast)
    }

    #[test]
    fn test_simple_bind() {
        let resolved = parse_and_resolve("x = 10").unwrap();
        assert_eq!(resolved.len(), 1);
        match &resolved[0] {
            Resolved::Bind(_, ResolvedPattern::Var(id), _) => {
                assert_eq!(id.name, "x");
            }
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_builtin_ref() {
        let resolved = parse_and_resolve("print(to_string(42))").unwrap();
        match &resolved[0] {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => assert_eq!(id.name, "print"),
                _ => panic!("Expected Var for print"),
            },
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_builtin_decl_resolution() {
        let resolved = parse_and_resolve("@builtin def print(a: String) -> Unit").unwrap();
        match &resolved[0] {
            Resolved::BuiltinDecl(_, id, params, ret_ty) => {
                assert_eq!(id.name, "print");
                assert_eq!(id.unique_id, 2); // 0=Ok, 1=Err, 2=print
                assert_eq!(params.len(), 1);
                assert!(matches!(
                    ret_ty,
                    Some(spire::ast::AstTy::Named(_, ty)) if ty == "Unit"
                ));
            }
            _ => panic!("Expected BuiltinDecl"),
        }
    }

    #[test]
    fn test_named_args_resolution() {
        let resolved = parse_and_resolve(
            r#"def add(x: Int, y: Int) -> Int { x + y }
result = add(y: 2, x: 1)"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::App(_, _, args) => {
                    assert!(matches!(&args[0], ResolvedRecordLitArg::Named(n, _) if n == "y"));
                    assert!(matches!(&args[1], ResolvedRecordLitArg::Named(n, _) if n == "x"));
                }
                _ => panic!("Expected App"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_function_def_resolution() {
        let resolved = parse_and_resolve(
            r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(1))"#,
        )
        .unwrap();
        match &resolved[0] {
            Resolved::Def(_, id, params, ret_ty, body) => {
                assert_eq!(id.name, "add");
                assert_eq!(params.len(), 2);
                assert!(matches!(ret_ty, Some(spire::ast::AstTy::Named(_, ty)) if ty == "Int"));
                assert!(
                    matches!(body.as_ref(), Resolved::Block(_, stmts) if matches!(stmts.as_slice(), [Resolved::BinOp(_, _, _, _)]))
                );
            }
            _ => panic!("Expected Def"),
        }
        match &resolved[1] {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => assert_eq!(id.name, "print"),
                _ => panic!("Expected Var"),
            },
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_undefined_var() {
        let result = parse_and_resolve("print(unknown_var)");
        assert!(result.is_err());
    }

    #[test]
    fn test_if_conversion() {
        let resolved = parse_and_resolve("x = if(True, 1, 2)").unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Resolved::If(_, _, _, Some(_))));
            }
            _ => panic!("Expected Bind with If"),
        }
    }

    #[test]
    fn test_shadowing() {
        let resolved = parse_and_resolve("x = 1\nx = x + 1").unwrap();
        // The second x should have a different unique_id
        match (&resolved[0], &resolved[1]) {
            (
                Resolved::Bind(_, ResolvedPattern::Var(id1), _),
                Resolved::Bind(_, ResolvedPattern::Var(id2), _),
            ) => {
                assert_ne!(id1.unique_id, id2.unique_id);
            }
            _ => panic!("Expected two Binds"),
        }
    }

    #[test]
    fn test_match_wildcard_and_literals() {
        let resolved = parse_and_resolve(
            r#"s = "a"
x = match s {
  "a" => 1,
  2 => 2,
  _ => 0,
}"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Match(_, _, arms) => {
                    assert!(matches!(&arms[0].0, ResolvedMatchPattern::StrLit(_, s) if s == "a"));
                    assert!(matches!(&arms[1].0, ResolvedMatchPattern::IntLit(_, 2)));
                    assert!(matches!(&arms[2].0, ResolvedMatchPattern::Wildcard(_)));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_closure_and_capture_resolution() {
        let resolved = parse_and_resolve(
            r#"x = 1
f = {|y| x + y}
g = &print(1)"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Closure(_, params, captures, body) => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(captures.len(), 1);
                    assert!(matches!(
                        body.as_ref(),
                        Resolved::BinOp(_, BinOp::Add, _, _)
                    ));
                }
                _ => panic!("Expected Closure"),
            },
            _ => panic!("Expected Bind"),
        }
        match &resolved[2] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Capture(_, target, args) => {
                    assert_eq!(args.len(), 1);
                    assert!(matches!(target.as_ref(), Resolved::Var(_, id) if id.name == "print"));
                }
                _ => panic!("Expected Capture"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_safebind_resolution() {
        let resolved = parse_and_resolve("num =? Ok(1)").unwrap();
        match &resolved[0] {
            Resolved::SafeBind(_, ResolvedPattern::Var(id), rhs) => {
                assert_eq!(id.name, "num");
                assert!(matches!(rhs.as_ref(), Resolved::ConstructorCall(_, _, _)));
            }
            _ => panic!("Expected SafeBind"),
        }
    }
}
