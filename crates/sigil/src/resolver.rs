#![allow(unused_imports)]

use spire::ast::{Ast, AstPattern, BinOp, Lit, Span};

use crate::error::ResolveError;
use crate::resolved::*;
use crate::scope::Scope;

/// Built-in function names that Sigil pre-registers.
const BUILTIN_NAMES: &[&str] = &["print", "to_string", "eprint"];

fn initialize_scope() -> Scope {
    let mut scope = Scope::new();
    let dummy = Span { start: 0, end: 0 };
    for name in BUILTIN_NAMES {
        scope.define(name, dummy.clone());
    }
    scope.define("Ok", dummy.clone());
    scope.define("Err", dummy);
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
        let mut resolved = Vec::new();
        for stmt in stmts {
            resolved.push(self.resolve_node(stmt)?);
        }
        Ok(resolved)
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
                    .map(|a| self.resolve_node(a))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::App(span, Box::new(resolved_func), resolved_args))
            }

            Ast::Bind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::Bind(span, resolved_pat, Box::new(resolved_rhs)))
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
                let rfields = fields
                    .into_iter()
                    .map(|f| ResolvedField {
                        name: f.name,
                        ty: f.ty,
                        span: f.span,
                    })
                    .collect();
                let resolved_show = self.resolve_node(*show_expr)?;
                Ok(Resolved::DeferrorDef(
                    span,
                    rid,
                    rfields,
                    Box::new(resolved_show),
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

    fn resolve_if(&mut self, span: Span, args: Vec<Ast>) -> Result<Resolved, ResolveError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(ResolveError {
                message: format!("if expects 2 or 3 arguments, got {}", args.len()),
                span,
            });
        }
        let mut iter = args.into_iter();
        let cond = self.resolve_node(iter.next().unwrap())?;
        let then = self.resolve_node(iter.next().unwrap())?;
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
}
