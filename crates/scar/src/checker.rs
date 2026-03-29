#![allow(unused_variables)]

use sigil::resolved::*;
use spire::ast::{AstTy, BinOp, Lit, Span};

use crate::env::TypeEnv;
use crate::error::TypeError;
use crate::typed::*;
use crate::types::Ty;

/// Type-check the resolved AST, producing a fully typed tree.
pub fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
    let mut checker = Checker::new();
    checker.check_program(resolved)
}

fn initialize_env() -> TypeEnv {
    let mut env = TypeEnv::new();
    // Register builtin function types.
    // These unique_ids must match the order in sigil's BUILTIN_NAMES + Ok/Err.
    // 0=print, 1=to_string, 2=eprint, 3=Ok, 4=Err

    // print: (String) -> Unit
    // V8 says print takes String. to_string must be called explicitly.
    env.bind_var(
        0,
        Ty::BuiltinFunc {
            name: "print".into(),
            params: vec![Ty::Str],
            ret: Box::new(Ty::Unit),
        },
    );

    // to_string: ($A) -> String — polymorphic
    let a = env.fresh_tyvar();
    env.bind_var(
        1,
        Ty::BuiltinFunc {
            name: "to_string".into(),
            params: vec![a],
            ret: Box::new(Ty::Str),
        },
    );

    // eprint: (Error) -> Unit
    env.bind_var(
        2,
        Ty::BuiltinFunc {
            name: "eprint".into(),
            params: vec![Ty::Error],
            ret: Box::new(Ty::Unit),
        },
    );

    // Ok constructor: ($A) -> Result<$A, $E>
    let ok_a = env.fresh_tyvar();
    let ok_e = env.fresh_tyvar();
    env.bind_var(
        3,
        Ty::BuiltinFunc {
            name: "Ok".into(),
            params: vec![ok_a.clone()],
            ret: Box::new(Ty::Result(Box::new(ok_a), Box::new(ok_e))),
        },
    );

    // Err constructor: ($E) -> Result<$A, $E>
    let err_a = env.fresh_tyvar();
    let err_e = env.fresh_tyvar();
    env.bind_var(
        4,
        Ty::BuiltinFunc {
            name: "Err".into(),
            params: vec![err_e.clone()],
            ret: Box::new(Ty::Result(Box::new(err_a), Box::new(err_e))),
        },
    );

    env
}

#[derive(Debug, Clone)]
pub struct ScarCheckpoint {
    env: TypeEnv,
}

#[derive(Debug, Clone)]
pub struct ScarSession {
    env: TypeEnv,
}

impl ScarSession {
    pub fn new() -> Self {
        Self {
            env: initialize_env(),
        }
    }

    pub fn typecheck(&mut self, resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        let mut checker = Checker::with_env(self.env.clone());
        let typed = checker.check_program(resolved)?;
        self.env = checker.into_env();
        Ok(typed)
    }

    pub fn checkpoint(&self) -> ScarCheckpoint {
        ScarCheckpoint {
            env: self.env.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: ScarCheckpoint) {
        self.env = checkpoint.env;
    }
}

impl Default for ScarSession {
    fn default() -> Self {
        Self::new()
    }
}

struct Checker {
    env: TypeEnv,
}

impl Checker {
    fn new() -> Self {
        Self {
            env: initialize_env(),
        }
    }

    fn with_env(env: TypeEnv) -> Self {
        Self { env }
    }

    fn into_env(self) -> TypeEnv {
        self.env
    }

    fn check_program(&mut self, stmts: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.predeclare_functions(&stmts)?;
        let mut typed = Vec::new();
        for stmt in stmts {
            typed.push(self.check_node(&stmt)?);
        }
        Ok(typed)
    }

    fn predeclare_functions(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut fun_idx = self.env.next_fun_idx;

        for stmt in stmts {
            if let Resolved::Def(_, id, params, ret_ty, _) = stmt {
                let param_tys = params
                    .iter()
                    .map(|param| self.resolve_ast_ty(&param.ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = match ret_ty {
                    Some(ty) => self.resolve_ast_ty(ty)?,
                    None => Ty::Unit,
                };
                self.env.bind_var(
                    id.unique_id,
                    Ty::UserFunc {
                        fun_idx,
                        params: param_tys,
                        ret: Box::new(ret),
                    },
                );
                fun_idx += 1;
            }
        }

        self.env.next_fun_idx = fun_idx;
        Ok(())
    }

    fn check_node(&mut self, node: &Resolved) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Lit(span, lit) => {
                let ty = self.lit_type(lit);
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::Lit(lit.clone()),
                })
            }

            Resolved::Var(span, id) => {
                let ty = self
                    .env
                    .lookup_var(id.unique_id)
                    .cloned()
                    .ok_or_else(|| TypeError {
                        message: format!("Undefined variable: {}", id.name),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::Var(id.clone()),
                })
            }

            Resolved::Bind(span, pat, rhs) => {
                let typed_rhs = if let (
                    ResolvedPattern::Annotated(_, ast_ty),
                    Resolved::Closure(cspan, params, captures, body),
                ) = (pat, rhs.as_ref())
                {
                    let expected = self.resolve_ast_ty(ast_ty)?;
                    self.check_closure(cspan, params, captures, body, Some(&expected))?
                } else {
                    self.check_node(rhs)?
                };
                if matches!(typed_rhs.ty, Ty::Error) {
                    return Err(TypeError {
                        message: "Error values must be wrapped with Err(...)".into(),
                        span: typed_rhs.span.clone(),
                        hint: None,
                    });
                }
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

            Resolved::App(span, func, args) => self.check_app(span, func, args),

            Resolved::BinOp(span, op, left, right) => self.check_binop(span, op, left, right),

            Resolved::List(span, elems) => self.check_list(span, elems),

            Resolved::InterpolatedStr(span, parts) => self.check_interpolated_str(span, parts),

            Resolved::If(span, cond, then, else_opt) => self.check_if(span, cond, then, else_opt),

            Resolved::Match(span, scrutinee, arms) => self.check_match(span, scrutinee, arms),

            Resolved::FieldAccess(span, expr, field) => self.check_field_access(span, expr, field),

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

            // Pass-through for struct/record/error defs and constructor calls — phase 7+
            Resolved::StructDef(span, id, fields) => self.check_struct_def(span, id, fields),
            Resolved::RecordDef(span, id, fields) => self.check_record_def(span, id, fields),
            Resolved::StructLit(span, id, field_vals) => {
                self.check_struct_lit(span, id, field_vals)
            }
            Resolved::ConstructorCall(span, id, args) => {
                self.check_constructor_call(span, id, args)
            }
            Resolved::DeferrorDef(span, id, fields, show_expr) => {
                self.check_deferror_def(span, id, fields, show_expr)
            }
            Resolved::Def(span, id, params, ret_ty, body) => {
                self.check_def(span, id, params, ret_ty, body)
            }
            Resolved::Closure(span, params, captures, body) => {
                self.check_closure(span, params, captures, body, None)
            }
            Resolved::Capture(span, target, args) => self.check_capture(span, target, args),
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
                            crate::env::TypeKind::Struct => {
                                Ok(Ty::Struct(def.name.clone(), def.fields.clone()))
                            }
                            crate::env::TypeKind::Record => {
                                Ok(Ty::Record(def.name.clone(), def.fields.clone()))
                            }
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
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|p| self.resolve_ast_ty(p))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_ast_ty(ret)?;
                Ok(Ty::Func(params, Box::new(ret)))
            }
        }
    }

    fn types_compatible(&self, expected: &Ty, got: &Ty) -> bool {
        match (expected, got) {
            (Ty::Var(_), _) | (_, Ty::Var(_)) => true,
            (Ty::Int, Ty::Int)
            | (Ty::Float, Ty::Float)
            | (Ty::Str, Ty::Str)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Unit, Ty::Unit)
            | (Ty::Error, Ty::Error) => true,
            (Ty::List(a), Ty::List(b)) => self.types_compatible(a, b),
            (Ty::Func(a_params, a_ret), Ty::Func(b_params, b_ret)) => {
                a_params.len() == b_params.len()
                    && a_params
                        .iter()
                        .zip(b_params.iter())
                        .all(|(a, b)| self.types_compatible(a, b))
                    && self.types_compatible(a_ret, b_ret)
            }
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
            Ty::Func(params, ret) => format!(
                "{}",
                {
                    let param_str = params
                        .iter()
                        .map(|ty| self.ty_name(ty))
                        .collect::<Vec<_>>()
                        .join(", ");
                    if param_str.is_empty() {
                        format!("(-> {})", self.ty_name(ret))
                    } else {
                        format!("({} -> {})", param_str, self.ty_name(ret))
                    }
                }
            ),
            Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
            Ty::UserFunc { .. } => "UserFunc".into(),
        }
    }

    fn format_signature(&self, name: &str, params: &[Ty], ret: &Ty) -> String {
        format!(
            "{}: ({}) -> {}",
            name,
            params
                .iter()
                .map(|ty| self.ty_name(ty))
                .collect::<Vec<_>>()
                .join(", "),
            self.ty_name(ret)
        )
    }

    fn find_tail_print_call<'a>(&self, node: &'a TypedNode) -> Option<&'a TypedNode> {
        match &node.node {
            TypedInner::Block(stmts) => stmts.last().and_then(|last| self.find_tail_print_call(last)),
            TypedInner::Semi(inner) => self.find_tail_print_call(inner),
            TypedInner::App(func, _) => match &func.ty {
                Ty::BuiltinFunc { name, .. } if name == "print" => Some(node),
                _ => None,
            },
            _ => None,
        }
    }

    fn describe_unit_return_hint(&self, body: &TypedNode) -> Option<String> {
        let call = self.find_tail_print_call(body)?;
        if let TypedInner::App(func, _) = &call.node {
            if let Ty::BuiltinFunc { name, params, ret } = &func.ty {
                return Some(format!(
                    "The function body ends with `print(...)`, which returns Unit.\n{}\nUse `print(...)` as a statement and end the function with an Int expression.",
                    self.format_signature(name, params, ret)
                ));
            }
        }
        None
    }

    fn return_mismatch_span(&self, body: &TypedNode) -> Span {
        self.tail_expr_span(body).unwrap_or_else(|| body.span.clone())
    }

    fn tail_expr_span(&self, node: &TypedNode) -> Option<Span> {
        match &node.node {
            TypedInner::Block(stmts) => stmts
                .last()
                .map(|last| self.tail_expr_span(last).unwrap_or_else(|| last.span.clone())),
            TypedInner::Semi(inner) => Some(
                self.tail_expr_span(inner)
                    .unwrap_or_else(|| inner.span.clone()),
            ),
            _ => Some(node.span.clone()),
        }
    }

    // ── Pattern checking ──

    fn check_pattern(
        &mut self,
        pat: &ResolvedPattern,
        rhs_ty: &Ty,
        span: &Span,
    ) -> Result<(TypedPattern, Ty), TypeError> {
        match pat {
            ResolvedPattern::Var(id) => Ok((
                TypedPattern::Var(rhs_ty.clone(), id.clone()),
                rhs_ty.clone(),
            )),
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected = self.resolve_ast_ty(ast_ty)?;
                if !self.types_compatible(&expected, rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "expected {}, got {}",
                            self.ty_name(&expected),
                            self.ty_name(rhs_ty)
                        ),
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

    fn check_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        let typed_func = self.check_node(func)?;
        let typed_args: Vec<TypedNode> = args
            .iter()
            .map(|a| self.check_node(a))
            .collect::<Result<Vec<_>, _>>()?;

        match &typed_func.ty {
            Ty::BuiltinFunc { name, params, ret } => {
                // Check arity
                if typed_args.len() != params.len() {
                    return Err(TypeError {
                        message: format!(
                            "{} expects {} argument(s), got {}",
                            name,
                            params.len(),
                            typed_args.len()
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                // Check arg types (Var = polymorphic, accepts anything)
                for (param, arg) in params.iter().zip(&typed_args) {
                    if !self.types_compatible(param, &arg.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Argument type mismatch: expected {}, got {}",
                                self.ty_name(param),
                                self.ty_name(&arg.ty)
                            ),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                }

                Ok(TypedNode {
                    ty: ret.as_ref().clone(),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::UserFunc { params, ret, .. } => {
                if typed_args.len() != params.len() {
                    return Err(TypeError {
                        message: format!(
                            "function expects {} argument(s), got {}",
                            params.len(),
                            typed_args.len()
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                for (param, arg) in params.iter().zip(&typed_args) {
                    if !self.types_compatible(param, &arg.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Argument type mismatch: expected {}, got {}",
                                self.ty_name(param),
                                self.ty_name(&arg.ty)
                            ),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                }

                Ok(TypedNode {
                    ty: ret.as_ref().clone(),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::Func(params, ret) => {
                if typed_args.len() != params.len() {
                    return Err(TypeError {
                        message: format!(
                            "function expects {} argument(s), got {}",
                            params.len(),
                            typed_args.len()
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                for (param, arg) in params.iter().zip(&typed_args) {
                    if !self.types_compatible(param, &arg.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Argument type mismatch: expected {}, got {}",
                                self.ty_name(param),
                                self.ty_name(&arg.ty)
                            ),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                }

                Ok(TypedNode {
                    ty: ret.as_ref().clone(),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            _ => Err(TypeError {
                message: format!("Not a function: {}", self.ty_name(&typed_func.ty)),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    fn check_closure(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let mut fun_env = self.env.clone();
        let mut typed_params = Vec::new();
        let param_tys = match expected {
            Some(Ty::Func(expected_params, _)) => {
                if expected_params.len() != params.len() {
                    return Err(TypeError {
                        message: format!(
                            "closure expects {} parameter(s), got {}",
                            expected_params.len(),
                            params.len()
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                expected_params.clone()
            }
            Some(other) => {
                return Err(TypeError {
                    message: format!("Expected function type, got {}", self.ty_name(other)),
                    span: span.clone(),
                    hint: None,
                });
            }
            None => params.iter().map(|_| self.env.fresh_tyvar()).collect(),
        };

        for (param, param_ty) in params.iter().zip(param_tys.iter()) {
            fun_env.bind_var(param.id.unique_id, param_ty.clone());
            typed_params.push(TypedClosureParam {
                id: param.id.clone(),
                ty: param_ty.clone(),
            });
        }

        for capture in captures {
            if let Some(ty) = self.env.lookup_var(capture.unique_id).cloned() {
                fun_env.bind_var(capture.unique_id, ty);
            }
        }

        let mut body_checker = Checker::with_env(fun_env);
        let typed_body = body_checker.check_node(body)?;
        self.env.next_tyvar = self.env.next_tyvar.max(body_checker.env.next_tyvar);
        self.env.next_tag = self.env.next_tag.max(body_checker.env.next_tag);

        let param_tys = typed_params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>();
        Ok(TypedNode {
            ty: Ty::Func(param_tys, Box::new(typed_body.ty.clone())),
            span: span.clone(),
            node: TypedInner::Closure(typed_params, captures.to_vec(), Box::new(typed_body)),
        })
    }

    fn check_capture(
        &mut self,
        span: &Span,
        target: &Resolved,
        args: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        let typed_target = self.check_node(target)?;
        let typed_args: Vec<TypedNode> = args
            .iter()
            .map(|a| self.check_node(a))
            .collect::<Result<Vec<_>, _>>()?;

        let (params, ret) = match &typed_target.ty {
            Ty::BuiltinFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::UserFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!("Not a function: {}", self.ty_name(other)),
                    span: typed_target.span.clone(),
                    hint: None,
                });
            }
        };

        if typed_args.len() > params.len() {
            return Err(TypeError {
                message: format!(
                    "partial application expects at most {} argument(s), got {}",
                    params.len(),
                    typed_args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        for (param, arg) in params.iter().zip(&typed_args) {
            if !self.types_compatible(param, &arg.ty) {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(param),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }

        let remaining = params[typed_args.len()..].to_vec();
        Ok(TypedNode {
            ty: Ty::Func(remaining, Box::new(ret)),
            span: span.clone(),
            node: TypedInner::Capture(Box::new(typed_target), typed_args),
        })
    }

    fn maybe_call_zero_arg_function(&self, node: TypedNode, _call_span: Span) -> TypedNode {
        match &node.ty {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret) if params.is_empty() => TypedNode {
                ty: ret.as_ref().clone(),
                span: node.span.clone(),
                node: TypedInner::App(Box::new(node), Vec::new()),
            },
            _ => node,
        }
    }

    // ── Binary operators ──

    fn check_binop(
        &mut self,
        span: &Span,
        op: &BinOp,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let typed_right = self.check_node(right)?;
        let lt = &typed_left.ty;
        let rt = &typed_right.ty;

        let result_ty = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => match (lt, rt) {
                (Ty::Int, Ty::Int) => Ok(Ty::Int),
                (Ty::Float, Ty::Float) => Ok(Ty::Float),
                _ => Err(TypeError {
                    message: format!(
                        "Cannot apply {:?} to {} and {}",
                        op,
                        self.ty_name(lt),
                        self.ty_name(rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            },
            BinOp::Mod => match (lt, rt) {
                (Ty::Int, Ty::Int) => Ok(Ty::Int),
                _ => Err(TypeError {
                    message: format!(
                        "% requires (Int, Int), got ({}, {})",
                        self.ty_name(lt),
                        self.ty_name(rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            },
            BinOp::Eq | BinOp::Neq => match (lt, rt) {
                (Ty::Int, Ty::Int) | (Ty::Str, Ty::Str) | (Ty::Bool, Ty::Bool) => Ok(Ty::Bool),
                _ if !self.types_compatible(lt, rt) => Err(TypeError {
                    message: format!(
                        "Cannot compare {} and {}",
                        self.ty_name(lt),
                        self.ty_name(rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
                _ => Err(TypeError {
                    message: format!("== / != not supported for {} in phase 1", self.ty_name(lt)),
                    span: span.clone(),
                    hint: None,
                }),
            },
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => match (lt, rt) {
                (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) => Ok(Ty::Bool),
                _ => Err(TypeError {
                    message: format!(
                        "Cannot compare {} and {}",
                        self.ty_name(lt),
                        self.ty_name(rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            },
            BinOp::Concat => match (lt, rt) {
                (Ty::Str, Ty::Str) => Ok(Ty::Str),
                _ => Err(TypeError {
                    message: format!(
                        "++ requires (String, String), got ({}, {})",
                        self.ty_name(lt),
                        self.ty_name(rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            },
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

        let typed_elems: Vec<TypedNode> = elems
            .iter()
            .map(|e| self.check_node(e))
            .collect::<Result<Vec<_>, _>>()?;

        let elem_ty = typed_elems[0].ty.clone();
        for (i, te) in typed_elems.iter().enumerate().skip(1) {
            if !self.types_compatible(&elem_ty, &te.ty) {
                return Err(TypeError {
                    message: format!(
                        "expected {}, got {}",
                        self.ty_name(&elem_ty),
                        self.ty_name(&te.ty)
                    ),
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

    fn check_interpolated_str(
        &mut self,
        span: &Span,
        parts: &[ResolvedInterpolatedPart],
    ) -> Result<TypedNode, TypeError> {
        let mut typed_parts = Vec::new();
        for part in parts {
            match part {
                ResolvedInterpolatedPart::Text(s) => {
                    typed_parts.push(TypedInterpolatedPart::Text(s.clone()));
                }
                ResolvedInterpolatedPart::Expr(expr) => {
                    let typed_expr = self.check_node(expr)?;
                    if matches!(typed_expr.ty, Ty::Result(_, _)) {
                        return Err(TypeError {
                            message: "Interpolation does not allow Result type".into(),
                            span: typed_expr.span.clone(),
                            hint: Some(
                                "Unwrap/match the Result first, or convert it to a printable value"
                                    .into(),
                            ),
                        });
                    }
                    typed_parts.push(TypedInterpolatedPart::Expr(Box::new(typed_expr)));
                }
            }
        }

        Ok(TypedNode {
            ty: Ty::Str,
            span: span.clone(),
            node: TypedInner::InterpolatedStr(typed_parts),
        })
    }

    // ── if expression ──

    fn check_if(
        &mut self,
        span: &Span,
        cond: &Resolved,
        then: &Resolved,
        else_opt: &Option<Box<Resolved>>,
    ) -> Result<TypedNode, TypeError> {
        let typed_cond = self.check_node(cond)?;
        if !self.types_compatible(&Ty::Bool, &typed_cond.ty) {
            return Err(TypeError {
                message: format!(
                    "if condition must be Boolean, got {}",
                    self.ty_name(&typed_cond.ty)
                ),
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
                        message: format!(
                            "if branches have different types: {} and {}",
                            self.ty_name(&typed_then.ty),
                            self.ty_name(&typed_else.ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let ty = typed_then.ty.clone();
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::If(
                        Box::new(typed_cond),
                        Box::new(typed_then),
                        Some(Box::new(typed_else)),
                    ),
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

    fn check_match(
        &mut self,
        span: &Span,
        scrutinee: &Resolved,
        arms: &[(ResolvedMatchPattern, Resolved)],
    ) -> Result<TypedNode, TypeError> {
        let typed_scrut = self.check_node(scrutinee)?;
        let mut typed_arms = Vec::new();
        let mut result_ty: Option<Ty> = None;

        for (pat, body) in arms {
            let (typed_pat, body_node) = self.check_match_arm(pat, body, &typed_scrut.ty, span)?;
            if let Some(ref rt) = result_ty {
                if !self.types_compatible(rt, &body_node.ty) {
                    return Err(TypeError {
                        message: format!(
                            "Match arm type mismatch: expected {}, got {}",
                            self.ty_name(rt),
                            self.ty_name(&body_node.ty)
                        ),
                        span: body_node.span.clone(),
                        hint: None,
                    });
                }
            } else {
                result_ty = Some(body_node.ty.clone());
            }
            typed_arms.push((typed_pat, body_node));
        }

        self.check_match_exhaustive(span, &typed_scrut.ty, &typed_arms)?;

        let ty = result_ty.unwrap_or(Ty::Unit);
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Match(Box::new(typed_scrut), typed_arms),
        })
    }

    fn check_match_exhaustive(
        &self,
        span: &Span,
        scrut_ty: &Ty,
        arms: &[(TypedMatchPattern, TypedNode)],
    ) -> Result<(), TypeError> {
        if arms
            .iter()
            .any(|(pat, _)| matches!(pat, TypedMatchPattern::Wildcard))
        {
            return Ok(());
        }

        match scrut_ty {
            Ty::Bool => {
                let has_true = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::BoolLit(true)));
                let has_false = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::BoolLit(false)));

                if has_true && has_false {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_true {
                        missing.push("True");
                    }
                    if !has_false {
                        missing.push("False");
                    }
                    Err(TypeError {
                        message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                        span: span.clone(),
                        hint: None,
                    })
                }
            }
            Ty::Result(_, _) => {
                let has_ok = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::Constructor(0, _)));
                let has_err = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::Constructor(1, _)));

                if has_ok && has_err {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_ok {
                        missing.push("Ok");
                    }
                    if !has_err {
                        missing.push("Err");
                    }
                    Err(TypeError {
                        message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                        span: span.clone(),
                        hint: None,
                    })
                }
            }
            _ => Err(TypeError {
                message: "Non-exhaustive match. Missing: _".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    fn check_match_arm(
        &mut self,
        pat: &ResolvedMatchPattern,
        body: &Resolved,
        scrut_ty: &Ty,
        _span: &Span,
    ) -> Result<(TypedMatchPattern, TypedNode), TypeError> {
        match pat {
            ResolvedMatchPattern::Wildcard(_) => {
                let typed_body = self.check_node(body)?;
                Ok((TypedMatchPattern::Wildcard, typed_body))
            }
            ResolvedMatchPattern::BoolLit(span, b) => {
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
            ResolvedMatchPattern::IntLit(span, n) => {
                if !self.types_compatible(&Ty::Int, scrut_ty) {
                    return Err(TypeError {
                        message: "Int pattern on non-Int scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let typed_body = self.check_node(body)?;
                Ok((TypedMatchPattern::IntLit(*n), typed_body))
            }
            ResolvedMatchPattern::StrLit(span, s) => {
                if !self.types_compatible(&Ty::Str, scrut_ty) {
                    return Err(TypeError {
                        message: "String pattern on non-String scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let typed_body = self.check_node(body)?;
                Ok((TypedMatchPattern::StrLit(s.clone()), typed_body))
            }
            ResolvedMatchPattern::Constructor(_, ctor_id, inner_id) => {
                if !matches!(scrut_ty, Ty::Result(_, _)) {
                    return Err(TypeError {
                        message: "Result constructor pattern on non-Result scrutinee".into(),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }

                // Ok => tag 0, Err => tag 1
                let tag = match ctor_id.name.as_str() {
                    "Ok" => 0u32,
                    "Err" => 1u32,
                    _ => {
                        return Err(TypeError {
                            message: format!("Unknown constructor: {}", ctor_id.name),
                            span: ctor_id.span.clone(),
                            hint: None,
                        });
                    }
                };

                // Bind inner variable
                if let Some(inner) = inner_id {
                    let inner_ty = match (tag, scrut_ty) {
                        (0, Ty::Result(ok, _)) => ok.as_ref().clone(),
                        (1, Ty::Result(_, err)) => err.as_ref().clone(),
                        _ => unreachable!("scrutinee type checked as Result above"),
                    };
                    self.env.bind_var(inner.unique_id, inner_ty.clone());
                }

                let typed_body = self.check_node(body)?;
                Ok((
                    TypedMatchPattern::Constructor(tag, inner_id.clone()),
                    typed_body,
                ))
            }
        }
    }

    // ── Field access ──

    fn check_field_access(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
    ) -> Result<TypedNode, TypeError> {
        let typed_expr = self.check_node(expr)?;

        let (idx, field_ty) = match &typed_expr.ty {
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
                .iter()
                .enumerate()
                .find(|(_, (name, _))| name == field)
                .map(|(i, (_, ty))| (i as u32, ty.clone()))
                .ok_or_else(|| TypeError {
                    message: format!("No field '{}' on {}", field, self.ty_name(&typed_expr.ty)),
                    span: span.clone(),
                    hint: None,
                })?,
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

    fn check_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedFunParam],
        ret_ty: &Option<AstTy>,
        body: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let mut fun_env = self.env.clone();
        let mut typed_params = Vec::new();

        for param in params {
            let param_ty = self.resolve_ast_ty(&param.ty)?;
            fun_env.bind_var(param.id.unique_id, param_ty.clone());
            typed_params.push(TypedFunParam {
                id: param.id.clone(),
                ty: param_ty.clone(),
            });
        }

        let expected_ret = match ret_ty {
            Some(ty) => self.resolve_ast_ty(ty)?,
            None => Ty::Unit,
        };

        let mut body_checker = Checker::with_env(fun_env);
        let typed_body = body_checker.check_node(body)?;

        self.env.next_tyvar = self.env.next_tyvar.max(body_checker.env.next_tyvar);
        self.env.next_tag = self.env.next_tag.max(body_checker.env.next_tag);

        if !self.types_compatible(&expected_ret, &typed_body.ty) {
            let hint = if matches!(typed_body.ty, Ty::Unit) {
                body_checker.describe_unit_return_hint(&typed_body)
            } else {
                None
            };
            return Err(TypeError {
                message: if ret_ty.is_some() {
                    format!(
                        "expected {}, got {}",
                        self.ty_name(&expected_ret),
                        self.ty_name(&typed_body.ty)
                    )
                } else {
                    format!(
                        "def {} without an explicit return type must return Unit, got {}",
                        id.name,
                        self.ty_name(&typed_body.ty)
                    )
                },
                span: body_checker.return_mismatch_span(&typed_body),
                hint,
            });
        }

        let fun_idx = match self.env.lookup_var(id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => {
                return Err(TypeError {
                    message: format!("Undefined function: {}", id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        };

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Def(fun_idx, id.clone(), typed_params, expected_ret, Box::new(typed_body)),
        })
    }

    // ── Struct/Record/Deferror definitions (stubs for step 7+) ──

    fn check_struct_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(String, Ty)> = fields
            .iter()
            .map(|f| Ok((f.name.clone(), self.resolve_ast_ty(&f.ty)?)))
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self.env.register_type_def(
            id.name.clone(),
            crate::env::TypeKind::Struct,
            ty_fields.clone(),
        );

        // Also bind the type name as a constructor-like entity
        self.env
            .bind_var(id.unique_id, Ty::Struct(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::StructDef(tag, id.name.clone(), field_names),
        })
    }

    fn check_record_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(String, Ty)> = fields
            .iter()
            .map(|f| Ok((f.name.clone(), self.resolve_ast_ty(&f.ty)?)))
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self.env.register_type_def(
            id.name.clone(),
            crate::env::TypeKind::Record,
            ty_fields.clone(),
        );

        self.env
            .bind_var(id.unique_id, Ty::Record(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::RecordDef(tag, id.name.clone(), field_names),
        })
    }

    fn check_struct_lit(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        field_vals: &[(String, Resolved)],
    ) -> Result<TypedNode, TypeError> {
        let def = self
            .env
            .lookup_type_def(&id.name)
            .ok_or_else(|| TypeError {
                message: format!("Unknown struct type: {}", id.name),
                span: span.clone(),
                hint: None,
            })?
            .clone();

        let tag = def.tag;

        // Check fields match definition order and types
        let mut typed_fields = Vec::new();
        for (def_name, def_ty) in &def.fields {
            let (_, resolved_val) =
                field_vals
                    .iter()
                    .find(|(n, _)| n == def_name)
                    .ok_or_else(|| TypeError {
                        message: format!("Missing field '{}' in {}", def_name, id.name),
                        span: span.clone(),
                        hint: None,
                    })?;
            let typed_val = self.check_node(resolved_val)?;
            if !self.types_compatible(def_ty, &typed_val.ty) {
                return Err(TypeError {
                    message: format!(
                        "Field '{}': expected {}, got {}",
                        def_name,
                        self.ty_name(def_ty),
                        self.ty_name(&typed_val.ty)
                    ),
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

    fn check_constructor_call(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if let Some(ty) = self.env.lookup_var(id.unique_id).cloned() {
            match &ty {
                Ty::BuiltinFunc { name, .. } if name == "Ok" || name == "Err" => {}
                Ty::BuiltinFunc { params, ret, .. }
                | Ty::UserFunc { params, ret, .. }
                | Ty::Func(params, ret) => {
                    if args.len() != params.len() {
                        return Err(TypeError {
                            message: format!(
                                "function expects {} argument(s), got {}",
                                params.len(),
                                args.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }

                    let mut typed_args = Vec::new();
                    for (param_ty, arg) in params.iter().zip(args) {
                        let typed_val = match arg {
                            ResolvedRecordLitArg::Positional(expr) => self.check_node(expr)?,
                            ResolvedRecordLitArg::Named(_, _) => {
                                return Err(TypeError {
                                    message: "Function calls do not accept named arguments".into(),
                                    span: span.clone(),
                                    hint: None,
                                });
                            }
                        };
                        if !self.types_compatible(param_ty, &typed_val.ty) {
                            return Err(TypeError {
                                message: format!(
                                    "Argument type mismatch: expected {}, got {}",
                                    self.ty_name(param_ty),
                                    self.ty_name(&typed_val.ty)
                                ),
                                span: typed_val.span.clone(),
                                hint: None,
                            });
                        }
                        typed_args.push(typed_val);
                    }

                    return Ok(TypedNode {
                        ty: ret.as_ref().clone(),
                        span: span.clone(),
                        node: TypedInner::App(
                            Box::new(TypedNode {
                                ty: ty.clone(),
                                span: id.span.clone(),
                                node: TypedInner::Var(id.clone()),
                            }),
                            typed_args,
                        ),
                    });
                }
                _ => {}
            }
        }

        if id.name == "Ok" || id.name == "Err" {
            if args.len() != 1 {
                return Err(TypeError {
                    message: format!("{} expects 1 argument(s), got {}", id.name, args.len()),
                    span: span.clone(),
                    hint: None,
                });
            }
            let inner = match &args[0] {
                ResolvedRecordLitArg::Positional(expr) => {
                    let typed = self.check_node(expr)?;
                    self.maybe_call_zero_arg_function(typed, span.clone())
                }
                ResolvedRecordLitArg::Named(_, _) => {
                    return Err(TypeError {
                        message: format!("{} does not accept named arguments", id.name),
                        span: span.clone(),
                        hint: None,
                    });
                }
            };
            let (tag, result_ty) = if id.name == "Ok" {
                let err_var = self.env.fresh_tyvar();
                (
                    0u32,
                    Ty::Result(Box::new(inner.ty.clone()), Box::new(err_var)),
                )
            } else {
                let ok_var = self.env.fresh_tyvar();
                (
                    1u32,
                    Ty::Result(Box::new(ok_var), Box::new(inner.ty.clone())),
                )
            };
            return Ok(TypedNode {
                ty: result_ty,
                span: span.clone(),
                node: TypedInner::ConstructorCall(tag, vec![inner]),
            });
        }

        let def = self
            .env
            .lookup_type_def(&id.name)
            .ok_or_else(|| TypeError {
                message: format!("Unknown constructor type: {}", id.name),
                span: span.clone(),
                hint: None,
            })?
            .clone();

        if !matches!(
            def.kind,
            crate::env::TypeKind::Record | crate::env::TypeKind::Error
        ) {
            return Err(TypeError {
                message: format!("{} is not a constructor-call type", id.name),
                span: span.clone(),
                hint: None,
            });
        }

        let tag = def.tag;

        // Handle positional or named args — reorder to definition order
        let mut typed_fields = vec![None; def.fields.len()];

        let all_positional = args
            .iter()
            .all(|a| matches!(a, ResolvedRecordLitArg::Positional(_)));
        let all_named = args
            .iter()
            .all(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)));

        if all_positional {
            if args.len() != def.fields.len() {
                return Err(TypeError {
                    message: format!(
                        "{} expects {} field(s), got {}",
                        id.name,
                        def.fields.len(),
                        args.len()
                    ),
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
                            message: format!(
                                "Field '{}': expected {}, got {}",
                                def.fields[i].0,
                                self.ty_name(def_ty),
                                self.ty_name(&typed_val.ty)
                            ),
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
                    let idx = def
                        .fields
                        .iter()
                        .position(|(n, _)| n == name)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown field '{}' in {}", name, id.name),
                            span: span.clone(),
                            hint: None,
                        })?;
                    let typed_val = self.check_node(expr)?;
                    let (_, def_ty) = &def.fields[idx];
                    if !self.types_compatible(def_ty, &typed_val.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Field '{}': expected {}, got {}",
                                name,
                                self.ty_name(def_ty),
                                self.ty_name(&typed_val.ty)
                            ),
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

        let final_fields: Vec<TypedNode> = typed_fields
            .into_iter()
            .enumerate()
            .map(|(i, f)| {
                f.ok_or_else(|| TypeError {
                    message: format!("Missing field '{}' in {}", def.fields[i].0, id.name),
                    span: span.clone(),
                    hint: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let result_ty = match def.kind {
            crate::env::TypeKind::Record => Ty::Record(id.name.clone(), def.fields.clone()),
            crate::env::TypeKind::Error => Ty::Error,
            crate::env::TypeKind::Struct => unreachable!("validated above"),
        };
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::ConstructorCall(tag, final_fields),
        })
    }

    fn check_deferror_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
        show_expr: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(Ty, ResolvedId)> = fields
            .iter()
            .map(|f| {
                let ty = self.resolve_ast_ty(&f.ty)?;
                let id = f.id.clone().ok_or_else(|| TypeError {
                    message: format!("Missing resolved field id for {}", f.name),
                    span: f.span.clone(),
                    hint: None,
                })?;
                Ok((ty, id))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self.env.register_type_def(
            id.name.clone(),
            crate::env::TypeKind::Error,
            ty_fields
                .iter()
                .map(|(ty, rid)| (rid.name.clone(), ty.clone()))
                .collect(),
        );

        let fun_idx = self.env.next_fun_idx;
        self.env.next_fun_idx += 1;

        let mut show_env = self.env.clone();
        let typed_params: Vec<TypedFunParam> = ty_fields
            .iter()
            .map(|(ty, resolved_id)| {
                show_env.bind_var(resolved_id.unique_id, ty.clone());
                TypedFunParam {
                    id: resolved_id.clone(),
                    ty: ty.clone(),
                }
            })
            .collect();

        // The error builder behaves like a function returning Error.
        self.env.bind_var(
            id.unique_id,
            Ty::UserFunc {
                fun_idx,
                params: typed_params.iter().map(|p| p.ty.clone()).collect(),
                ret: Box::new(Ty::Error),
            },
        );

        for (ty, resolved_id) in &ty_fields {
            show_env.bind_var(resolved_id.unique_id, ty.clone());
        }
        let mut show_checker = Checker::with_env(show_env);
        let typed_show = show_checker.check_node(show_expr)?;
        self.env.next_tyvar = self.env.next_tyvar.max(show_checker.env.next_tyvar);
        self.env.next_tag = self.env.next_tag.max(show_checker.env.next_tag);
        if !self.types_compatible(&Ty::Str, &typed_show.ty) {
            return Err(TypeError {
                message: format!(
                    "deferror show block must return String, got {}",
                    self.ty_name(&typed_show.ty)
                ),
                span: typed_show.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::DeferrorDef(
                tag,
                fun_idx,
                id.clone(),
                typed_params,
                Box::new(typed_show),
            ),
        })
    }
}
