#![allow(unused_variables)]

use spire::ast::{AstTy, BinOp, Lit, Span};
use sigil::resolved::*;

use crate::env::TypeEnv;
use crate::error::TypeError;
use crate::typed::*;
use crate::types::Ty;

/// Type-check the resolved AST, producing a fully typed tree.
pub fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
    let mut checker = Checker::new();
    checker.check_program(resolved)
}

struct Checker {
    env: TypeEnv,
}

impl Checker {
    fn new() -> Self {
        let mut env = TypeEnv::new();
        // Register builtin function types.
        // These unique_ids must match the order in sigil's BUILTIN_NAMES + Ok/Err.
        // 0=print, 1=to_string, 2=eprint, 3=Ok, 4=Err

        // print: (String) -> Unit
        // V8 says print takes String. to_string must be called explicitly.
        env.bind_var(0, Ty::BuiltinFunc {
            name: "print".into(),
            params: vec![Ty::Str],
            ret: Box::new(Ty::Unit),
        });

        // to_string: ($A) -> String — polymorphic
        let a = env.fresh_tyvar();
        env.bind_var(1, Ty::BuiltinFunc {
            name: "to_string".into(),
            params: vec![a],
            ret: Box::new(Ty::Str),
        });

        // eprint: (Error) -> Unit
        env.bind_var(2, Ty::BuiltinFunc {
            name: "eprint".into(),
            params: vec![Ty::Error],
            ret: Box::new(Ty::Unit),
        });

        // Ok constructor: ($A) -> Result<$A, $E>
        let ok_a = env.fresh_tyvar();
        let ok_e = env.fresh_tyvar();
        env.bind_var(3, Ty::BuiltinFunc {
            name: "Ok".into(),
            params: vec![ok_a.clone()],
            ret: Box::new(Ty::Result(Box::new(ok_a), Box::new(ok_e))),
        });

        // Err constructor: ($E) -> Result<$A, $E>
        let err_a = env.fresh_tyvar();
        let err_e = env.fresh_tyvar();
        env.bind_var(4, Ty::BuiltinFunc {
            name: "Err".into(),
            params: vec![err_e.clone()],
            ret: Box::new(Ty::Result(Box::new(err_a), Box::new(err_e))),
        });

        Self { env }
    }

    fn check_program(&mut self, stmts: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        let mut typed = Vec::new();
        for stmt in stmts {
            typed.push(self.check_node(&stmt)?);
        }
        Ok(typed)
    }

    fn check_node(&mut self, node: &Resolved) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Lit(span, lit) => {
                let ty = self.lit_type(lit);
                Ok(TypedNode { ty, span: span.clone(), node: TypedInner::Lit(lit.clone()) })
            }

            Resolved::Var(span, id) => {
                let ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(|| TypeError {
                    message: format!("Undefined variable: {}", id.name),
                    span: span.clone(),
                    hint: None,
                })?;
                Ok(TypedNode { ty, span: span.clone(), node: TypedInner::Var(id.clone()) })
            }

            Resolved::Bind(span, pat, rhs) => {
                let typed_rhs = self.check_node(rhs)?;
                let (typed_pat, pat_ty) = self.check_pattern(pat, &typed_rhs.ty, span)?;

                // Store the binding type in env
                match &typed_pat {
                    TypedPattern::Var(_, id) => {
                        self.env.bind_var(id.unique_id, pat_ty.clone());
                    }
                    TypedPattern::Wildcard(_) => {}
                }

                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Bind(typed_pat, Box::new(typed_rhs)),
                })
            }

            Resolved::App(span, func, args) => {
                self.check_app(span, func, args)
            }

            Resolved::BinOp(span, op, left, right) => {
                self.check_binop(span, op, left, right)
            }

            Resolved::List(span, elems) => {
                self.check_list(span, elems)
            }

            Resolved::If(span, cond, then, else_opt) => {
                self.check_if(span, cond, then, else_opt)
            }

            Resolved::Match(span, scrutinee, arms) => {
                self.check_match(span, scrutinee, arms)
            }

            Resolved::FieldAccess(span, expr, field) => {
                self.check_field_access(span, expr, field)
            }

            Resolved::Block(span, stmts) => {
                let mut typed_stmts = Vec::new();
                let mut last_ty = Ty::Unit;
                for s in stmts {
                    let t = self.check_node(s)?;
                    last_ty = t.ty.clone();
                    typed_stmts.push(t);
                }
                Ok(TypedNode {
                    ty: last_ty,
                    span: span.clone(),
                    node: TypedInner::Block(typed_stmts),
                })
            }

            Resolved::Semi(span, inner) => {
                let typed_inner = self.check_node(inner)?;
                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Semi(Box::new(typed_inner)),
                })
            }

            // Pass-through for struct/record/error defs and lits — phase 7+
            Resolved::StructDef(span, id, fields) => {
                self.check_struct_def(span, id, fields)
            }
            Resolved::RecordDef(span, id, fields) => {
                self.check_record_def(span, id, fields)
            }
            Resolved::StructLit(span, id, field_vals) => {
                self.check_struct_lit(span, id, field_vals)
            }
            Resolved::RecordLit(span, id, args) => {
                self.check_record_lit(span, id, args)
            }
            Resolved::DeferrorDef(span, id, fields, show_expr) => {
                self.check_deferror_def(span, id, fields, show_expr)
            }
        }
    }

    // ── Helpers ──

    fn lit_type(&self, lit: &Lit) -> Ty {
        match lit {
            Lit::Int(_) => Ty::Int,
            Lit::Float(_) => Ty::Float,
            Lit::Str(_) => Ty::Str,
            Lit::Bool(_) => Ty::Bool,
            Lit::Unit => Ty::Unit,
        }
    }

    fn resolve_ast_ty(&self, ast_ty: &AstTy) -> Result<Ty, TypeError> {
        match ast_ty {
            AstTy::Named(span, name) => match name.as_str() {
                "Int" => Ok(Ty::Int),
                "Float" => Ok(Ty::Float),
                "String" => Ok(Ty::Str),
                "Boolean" => Ok(Ty::Bool),
                "Unit" => Ok(Ty::Unit),
                "Error" => Ok(Ty::Error),
                other => {
                    // Check user-defined types
                    if let Some(def) = self.env.lookup_type_def(other) {
                        match &def.kind {
                            crate::env::TypeKind::Struct => Ok(Ty::Struct(def.name.clone(), def.fields.clone())),
                            crate::env::TypeKind::Record => Ok(Ty::Record(def.name.clone(), def.fields.clone())),
                            crate::env::TypeKind::Error => Ok(Ty::Error),
                        }
                    } else {
                        Err(TypeError {
                            message: format!("Unknown type: {}", other),
                            span: span.clone(),
                            hint: None,
                        })
                    }
                }
            },
            AstTy::ListOf(_, inner) => {
                let inner_ty = self.resolve_ast_ty(inner)?;
                Ok(Ty::List(Box::new(inner_ty)))
            }
            AstTy::ResultOf(_, ok_ty, err_ty) => {
                let ok = self.resolve_ast_ty(ok_ty)?;
                let err = match err_ty {
                    Some(e) => self.resolve_ast_ty(e)?,
                    None => Ty::Error, // Result<T> = Result<T, Error>
                };
                Ok(Ty::Result(Box::new(ok), Box::new(err)))
            }
        }
    }

    fn types_compatible(&self, expected: &Ty, got: &Ty) -> bool {
        match (expected, got) {
            (Ty::Var(_), _) | (_, Ty::Var(_)) => true,
            (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) | (Ty::Str, Ty::Str)
            | (Ty::Bool, Ty::Bool) | (Ty::Unit, Ty::Unit) | (Ty::Error, Ty::Error) => true,
            (Ty::List(a), Ty::List(b)) => self.types_compatible(a, b),
            (Ty::Result(ok1, err1), Ty::Result(ok2, err2)) => {
                self.types_compatible(ok1, ok2) && self.types_compatible(err1, err2)
            }
            (Ty::Struct(n1, _), Ty::Struct(n2, _)) => n1 == n2,
            (Ty::Record(n1, _), Ty::Record(n2, _)) => n1 == n2,
            _ => false,
        }
    }

    fn ty_name(&self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Boolean".into(),
            Ty::Unit => "Unit".into(),
            Ty::Error => "Error".into(),
            Ty::List(inner) => format!("[{}]", self.ty_name(inner)),
            Ty::Result(ok, _) => format!("Result<{}>", self.ty_name(ok)),
            Ty::Var(n) => format!("${}", n),
            Ty::Struct(name, _) | Ty::Record(name, _) => name.clone(),
            Ty::Func(_, ret) => format!("Func -> {}", self.ty_name(ret)),
            Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
            Ty::UserFunc { .. } => "UserFunc".into(),
        }
    }

    // ── Pattern checking ──

    fn check_pattern(&mut self, pat: &ResolvedPattern, rhs_ty: &Ty, span: &Span) -> Result<(TypedPattern, Ty), TypeError> {
        match pat {
            ResolvedPattern::Var(id) => {
                Ok((TypedPattern::Var(rhs_ty.clone(), id.clone()), rhs_ty.clone()))
            }
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected = self.resolve_ast_ty(ast_ty)?;
                if !self.types_compatible(&expected, rhs_ty) {
                    return Err(TypeError {
                        message: format!("expected {}, got {}", self.ty_name(&expected), self.ty_name(rhs_ty)),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::Var(expected.clone(), id.clone()), expected))
            }
            ResolvedPattern::Wildcard(_wspan) => {
                Ok((TypedPattern::Wildcard(rhs_ty.clone()), rhs_ty.clone()))
            }
        }
    }

    // ── Function application ──

    fn check_app(&mut self, span: &Span, func: &Resolved, args: &[Resolved]) -> Result<TypedNode, TypeError> {
        let typed_func = self.check_node(func)?;
        let typed_args: Vec<TypedNode> = args.iter()
            .map(|a| self.check_node(a))
            .collect::<Result<Vec<_>, _>>()?;

        match &typed_func.ty {
            Ty::BuiltinFunc { name, params, ret } => {
                // Check arity
                if typed_args.len() != params.len() {
                    return Err(TypeError {
                        message: format!("{} expects {} argument(s), got {}", name, params.len(), typed_args.len()),
                        span: span.clone(),
                        hint: None,
                    });
                }
                // Check arg types (Var = polymorphic, accepts anything)
                for (param, arg) in params.iter().zip(&typed_args) {
                    if !self.types_compatible(param, &arg.ty) {
                        return Err(TypeError {
                            message: format!("Argument type mismatch: expected {}, got {}",
                                self.ty_name(param), self.ty_name(&arg.ty)),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                }

                // Specialize return type for Ok/Err constructors
                let ret_ty = match name.as_str() {
                    "Ok" => {
                        let ok_ty = typed_args[0].ty.clone();
                        let err_var = self.env.fresh_tyvar();
                        Ty::Result(Box::new(ok_ty), Box::new(err_var))
                    }
                    "Err" => {
                        let err_ty = typed_args[0].ty.clone();
                        let ok_var = self.env.fresh_tyvar();
                        Ty::Result(Box::new(ok_var), Box::new(err_ty))
                    }
                    _ => ret.as_ref().clone(),
                };

                Ok(TypedNode {
                    ty: ret_ty,
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            _ => {
                Err(TypeError {
                    message: format!("Not a function: {}", self.ty_name(&typed_func.ty)),
                    span: span.clone(),
                    hint: None,
                })
            }
        }
    }

    // ── Binary operators ──

    fn check_binop(&mut self, span: &Span, op: &BinOp, left: &Resolved, right: &Resolved) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let typed_right = self.check_node(right)?;
        let lt = &typed_left.ty;
        let rt = &typed_right.ty;

        let result_ty = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                match (lt, rt) {
                    (Ty::Int, Ty::Int) => Ok(Ty::Int),
                    (Ty::Float, Ty::Float) => Ok(Ty::Float),
                    _ => Err(TypeError {
                        message: format!("Cannot apply {:?} to {} and {}", op, self.ty_name(lt), self.ty_name(rt)),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
            BinOp::Mod => {
                match (lt, rt) {
                    (Ty::Int, Ty::Int) => Ok(Ty::Int),
                    _ => Err(TypeError {
                        message: format!("% requires (Int, Int), got ({}, {})", self.ty_name(lt), self.ty_name(rt)),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
            BinOp::Eq | BinOp::Neq => {
                match (lt, rt) {
                    (Ty::Int, Ty::Int) | (Ty::Str, Ty::Str) | (Ty::Bool, Ty::Bool) => Ok(Ty::Bool),
                    _ if !self.types_compatible(lt, rt) => Err(TypeError {
                        message: format!("Cannot compare {} and {}", self.ty_name(lt), self.ty_name(rt)),
                        span: span.clone(),
                        hint: None,
                    }),
                    _ => Err(TypeError {
                        message: format!("== / != not supported for {} in phase 1", self.ty_name(lt)),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => {
                match (lt, rt) {
                    (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) => Ok(Ty::Bool),
                    _ => Err(TypeError {
                        message: format!("Cannot compare {} and {}", self.ty_name(lt), self.ty_name(rt)),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
            BinOp::Concat => {
                match (lt, rt) {
                    (Ty::Str, Ty::Str) => Ok(Ty::Str),
                    _ => Err(TypeError {
                        message: format!("++ requires (String, String), got ({}, {})", self.ty_name(lt), self.ty_name(rt)),
                        span: span.clone(),
                        hint: None,
                    }),
                }
            }
        }?;

        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::BinOp(op.clone(), Box::new(typed_left), Box::new(typed_right)),
        })
    }

    // ── List ──

    fn check_list(&mut self, span: &Span, elems: &[Resolved]) -> Result<TypedNode, TypeError> {
        if elems.is_empty() {
            // Empty list — type must be inferred from annotation (handled in Bind)
            // For now, use a type variable
            let tv = self.env.fresh_tyvar();
            return Ok(TypedNode {
                ty: Ty::List(Box::new(tv)),
                span: span.clone(),
                node: TypedInner::List(Vec::new()),
            });
        }

        let typed_elems: Vec<TypedNode> = elems.iter()
            .map(|e| self.check_node(e))
            .collect::<Result<Vec<_>, _>>()?;

        let elem_ty = typed_elems[0].ty.clone();
        for (i, te) in typed_elems.iter().enumerate().skip(1) {
            if !self.types_compatible(&elem_ty, &te.ty) {
                return Err(TypeError {
                    message: format!("expected {}, got {}", self.ty_name(&elem_ty), self.ty_name(&te.ty)),
                    span: te.span.clone(),
                    hint: Some("All list elements must have the same type".into()),
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty)),
            span: span.clone(),
            node: TypedInner::List(typed_elems),
        })
    }

    // ── if expression ──

    fn check_if(&mut self, span: &Span, cond: &Resolved, then: &Resolved, else_opt: &Option<Box<Resolved>>) -> Result<TypedNode, TypeError> {
        let typed_cond = self.check_node(cond)?;
        if !self.types_compatible(&Ty::Bool, &typed_cond.ty) {
            return Err(TypeError {
                message: format!("if condition must be Boolean, got {}", self.ty_name(&typed_cond.ty)),
                span: typed_cond.span.clone(),
                hint: None,
            });
        }

        let typed_then = self.check_node(then)?;

        match else_opt {
            Some(else_branch) => {
                let typed_else = self.check_node(else_branch)?;
                if !self.types_compatible(&typed_then.ty, &typed_else.ty) {
                    return Err(TypeError {
                        message: format!("if branches have different types: {} and {}",
                            self.ty_name(&typed_then.ty), self.ty_name(&typed_else.ty)),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let ty = typed_then.ty.clone();
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::If(Box::new(typed_cond), Box::new(typed_then), Some(Box::new(typed_else))),
                })
            }
            None => {
                // 2-arg if — returns Unit
                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::If(Box::new(typed_cond), Box::new(typed_then), None),
                })
            }
        }
    }

    // ── match expression ──

    fn check_match(&mut self, span: &Span, scrutinee: &Resolved, arms: &[(ResolvedMatchPattern, Resolved)]) -> Result<TypedNode, TypeError> {
        let typed_scrut = self.check_node(scrutinee)?;
        let mut typed_arms = Vec::new();
        let mut result_ty: Option<Ty> = None;

        for (pat, body) in arms {
            let (typed_pat, body_node) = self.check_match_arm(pat, body, &typed_scrut.ty, span)?;
            if let Some(ref rt) = result_ty {
                if !self.types_compatible(rt, &body_node.ty) {
                    return Err(TypeError {
                        message: format!("Match arm type mismatch: expected {}, got {}",
                            self.ty_name(rt), self.ty_name(&body_node.ty)),
                        span: body_node.span.clone(),
                        hint: None,
                    });
                }
            } else {
                result_ty = Some(body_node.ty.clone());
            }
            typed_arms.push((typed_pat, body_node));
        }

        let ty = result_ty.unwrap_or(Ty::Unit);
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Match(Box::new(typed_scrut), typed_arms),
        })
    }

    fn check_match_arm(
        &mut self,
        pat: &ResolvedMatchPattern,
        body: &Resolved,
        scrut_ty: &Ty,
        span: &Span,
    ) -> Result<(TypedMatchPattern, TypedNode), TypeError> {
        match pat {
            ResolvedMatchPattern::BoolLit(_, b) => {
                if !self.types_compatible(&Ty::Bool, scrut_ty) {
                    return Err(TypeError {
                        message: "Boolean pattern on non-Boolean scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let typed_body = self.check_node(body)?;
                Ok((TypedMatchPattern::BoolLit(*b), typed_body))
            }
            ResolvedMatchPattern::Constructor(_, ctor_id, inner_id) => {
                // Ok => tag 0, Err => tag 1
                let tag = match ctor_id.name.as_str() {
                    "Ok" => 0u32,
                    "Err" => 1u32,
                    _ => {
                        return Err(TypeError {
                            message: format!("Unknown constructor: {}", ctor_id.name),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };

                // Bind inner variable
                if let Some(inner) = inner_id {
                    let inner_ty = match (tag, scrut_ty) {
                        (0, Ty::Result(ok, _)) => ok.as_ref().clone(),
                        (1, Ty::Result(_, err)) => err.as_ref().clone(),
                        _ => {
                            let tv = self.env.fresh_tyvar();
                            tv
                        }
                    };
                    self.env.bind_var(inner.unique_id, inner_ty.clone());
                }

                let typed_body = self.check_node(body)?;
                Ok((TypedMatchPattern::Constructor(tag, inner_id.clone()), typed_body))
            }
        }
    }

    // ── Field access ──

    fn check_field_access(&mut self, span: &Span, expr: &Resolved, field: &str) -> Result<TypedNode, TypeError> {
        let typed_expr = self.check_node(expr)?;

        let (idx, field_ty) = match &typed_expr.ty {
            Ty::Struct(_, fields) | Ty::Record(_, fields) => {
                fields.iter().enumerate()
                    .find(|(_, (name, _))| name == field)
                    .map(|(i, (_, ty))| (i as u32, ty.clone()))
                    .ok_or_else(|| TypeError {
                        message: format!("No field '{}' on {}", field, self.ty_name(&typed_expr.ty)),
                        span: span.clone(),
                        hint: None,
                    })?
            }
            _ => {
                return Err(TypeError {
                    message: format!("Cannot access field on {}", self.ty_name(&typed_expr.ty)),
                    span: span.clone(),
                    hint: None,
                });
            }
        };

        Ok(TypedNode {
            ty: field_ty,
            span: span.clone(),
            node: TypedInner::FieldAccess(Box::new(typed_expr), idx),
        })
    }

    // ── Struct/Record/Deferror definitions (stubs for step 7+) ──

    fn check_struct_def(&mut self, span: &Span, id: &ResolvedId, fields: &[ResolvedField]) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(String, Ty)> = fields.iter()
            .map(|f| Ok((f.name.clone(), self.resolve_ast_ty(&f.ty)?)))
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self.env.register_type_def(
            id.name.clone(),
            crate::env::TypeKind::Struct,
            ty_fields.clone(),
        );

        // Also bind the type name as a constructor-like entity
        self.env.bind_var(id.unique_id, Ty::Struct(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::StructDef(tag, id.name.clone(), field_names),
        })
    }

    fn check_record_def(&mut self, span: &Span, id: &ResolvedId, fields: &[ResolvedField]) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(String, Ty)> = fields.iter()
            .map(|f| Ok((f.name.clone(), self.resolve_ast_ty(&f.ty)?)))
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self.env.register_type_def(
            id.name.clone(),
            crate::env::TypeKind::Record,
            ty_fields.clone(),
        );

        self.env.bind_var(id.unique_id, Ty::Record(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::RecordDef(tag, id.name.clone(), field_names),
        })
    }

    fn check_struct_lit(&mut self, span: &Span, id: &ResolvedId, field_vals: &[(String, Resolved)]) -> Result<TypedNode, TypeError> {
        let def = self.env.lookup_type_def(&id.name).ok_or_else(|| TypeError {
            message: format!("Unknown struct type: {}", id.name),
            span: span.clone(),
            hint: None,
        })?.clone();

        let tag = def.tag;

        // Check fields match definition order and types
        let mut typed_fields = Vec::new();
        for (def_name, def_ty) in &def.fields {
            let (_, resolved_val) = field_vals.iter()
                .find(|(n, _)| n == def_name)
                .ok_or_else(|| TypeError {
                    message: format!("Missing field '{}' in {}", def_name, id.name),
                    span: span.clone(),
                    hint: None,
                })?;
            let typed_val = self.check_node(resolved_val)?;
            if !self.types_compatible(def_ty, &typed_val.ty) {
                return Err(TypeError {
                    message: format!("Field '{}': expected {}, got {}",
                        def_name, self.ty_name(def_ty), self.ty_name(&typed_val.ty)),
                    span: typed_val.span.clone(),
                    hint: None,
                });
            }
            typed_fields.push(typed_val);
        }

        let result_ty = Ty::Struct(id.name.clone(), def.fields.clone());
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::StructLit(tag, typed_fields),
        })
    }

    fn check_record_lit(&mut self, span: &Span, id: &ResolvedId, args: &[ResolvedRecordLitArg]) -> Result<TypedNode, TypeError> {
        let def = self.env.lookup_type_def(&id.name).ok_or_else(|| TypeError {
            message: format!("Unknown record type: {}", id.name),
            span: span.clone(),
            hint: None,
        })?.clone();

        let tag = def.tag;

        // Handle positional or named args — reorder to definition order
        let mut typed_fields = vec![None; def.fields.len()];

        let all_positional = args.iter().all(|a| matches!(a, ResolvedRecordLitArg::Positional(_)));
        let all_named = args.iter().all(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)));

        if all_positional {
            if args.len() != def.fields.len() {
                return Err(TypeError {
                    message: format!("{} expects {} field(s), got {}", id.name, def.fields.len(), args.len()),
                    span: span.clone(),
                    hint: None,
                });
            }
            for (i, arg) in args.iter().enumerate() {
                if let ResolvedRecordLitArg::Positional(expr) = arg {
                    let typed_val = self.check_node(expr)?;
                    let (_, def_ty) = &def.fields[i];
                    if !self.types_compatible(def_ty, &typed_val.ty) {
                        return Err(TypeError {
                            message: format!("Field '{}': expected {}, got {}",
                                def.fields[i].0, self.ty_name(def_ty), self.ty_name(&typed_val.ty)),
                            span: typed_val.span.clone(),
                            hint: None,
                        });
                    }
                    typed_fields[i] = Some(typed_val);
                }
            }
        } else if all_named {
            for arg in args {
                if let ResolvedRecordLitArg::Named(name, expr) = arg {
                    let idx = def.fields.iter().position(|(n, _)| n == name)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown field '{}' in {}", name, id.name),
                            span: span.clone(),
                            hint: None,
                        })?;
                    let typed_val = self.check_node(expr)?;
                    let (_, def_ty) = &def.fields[idx];
                    if !self.types_compatible(def_ty, &typed_val.ty) {
                        return Err(TypeError {
                            message: format!("Field '{}': expected {}, got {}",
                                name, self.ty_name(def_ty), self.ty_name(&typed_val.ty)),
                            span: typed_val.span.clone(),
                            hint: None,
                        });
                    }
                    typed_fields[idx] = Some(typed_val);
                }
            }
        } else {
            return Err(TypeError {
                message: "Cannot mix positional and named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let final_fields: Vec<TypedNode> = typed_fields.into_iter()
            .enumerate()
            .map(|(i, f)| f.ok_or_else(|| TypeError {
                message: format!("Missing field '{}' in {}", def.fields[i].0, id.name),
                span: span.clone(),
                hint: None,
            }))
            .collect::<Result<Vec<_>, _>>()?;

        let result_ty = Ty::Record(id.name.clone(), def.fields.clone());
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::RecordLit(tag, final_fields),
        })
    }

    fn check_deferror_def(&mut self, span: &Span, id: &ResolvedId, fields: &[ResolvedField], show_expr: &Resolved) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(String, Ty)> = fields.iter()
            .map(|f| Ok((f.name.clone(), self.resolve_ast_ty(&f.ty)?)))
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self.env.register_type_def(
            id.name.clone(),
            crate::env::TypeKind::Error,
            ty_fields,
        );

        // The error type name itself acts as a value (for no-arg errors) or constructor
        self.env.bind_var(id.unique_id, Ty::Error);

        // Check show expression type is String
        let typed_show = self.check_node(show_expr)?;
        if !self.types_compatible(&Ty::Str, &typed_show.ty) {
            return Err(TypeError {
                message: format!("deferror show block must return String, got {}", self.ty_name(&typed_show.ty)),
                span: typed_show.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::DeferrorDef(tag, Box::new(typed_show)),
        })
    }
}
