#![allow(unused_variables)]

use std::collections::HashMap;

use sigil::resolved::*;
use sindr::builtin::{builtin_meta_by_name, builtin_uid, BuiltinMeta, BUILTIN_METAS};
use spire::ast::{AstTy, BinOp, Lit, Span};

use crate::env::TypeEnv;
use crate::error::TypeError;
use crate::typed::*;
use crate::types::Ty;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSyntaxContext {
    General,
    FunctionReturn,
    ErrorMarker,
}

/// Type-check the resolved AST, producing a fully typed tree.
pub fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
    let mut checker = Checker::new();
    checker.check_program(resolved)
}

fn initialize_env() -> TypeEnv {
    let mut env = TypeEnv::new();
    // Ok constructor: ($A) -> Result<$A, $E>
    let ok_a = env.fresh_tyvar();
    let ok_e = env.fresh_tyvar();
    env.bind_var(
        0,
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
        1,
        Ty::BuiltinFunc {
            name: "Err".into(),
            params: vec![err_e.clone()],
            ret: Box::new(Ty::Result(Box::new(err_a), Box::new(err_e))),
        },
    );

    for meta in BUILTIN_METAS {
        let uid = builtin_uid(meta.builtin_id);
        let ty = builtin_ty_from_meta(meta, &mut env);
        env.bind_var(uid, ty);
    }

    env
}

fn builtin_ty_from_meta(meta: &BuiltinMeta, env: &mut TypeEnv) -> Ty {
    match meta.name {
        "print" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Str],
            ret: Box::new(Ty::Unit),
        },
        "to_string" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![a],
                ret: Box::new(Ty::Str),
            }
        }
        "inspect" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![a],
                ret: Box::new(Ty::Str),
            }
        }
        "safe_div" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![a.clone(), a.clone()],
                ret: Box::new(Ty::Result(Box::new(a), Box::new(Ty::Error))),
            }
        }
        "safe_mod" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
        },
        "eprint" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Error],
            ret: Box::new(Ty::Unit),
        },
        _ => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Unit; meta.arity as usize],
            ret: Box::new(Ty::Unit),
        },
    }
}

#[derive(Debug, Clone)]
pub struct ScarCheckpoint {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ScarSession {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
}

impl ScarSession {
    pub fn new() -> Self {
        Self {
            env: initialize_env(),
            user_func_params: HashMap::new(),
        }
    }

    pub fn typecheck(&mut self, resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        let mut checker =
            Checker::with_env_and_params(self.env.clone(), self.user_func_params.clone());
        let typed = checker.check_program(resolved)?;
        let (env, user_func_params) = checker.into_parts();
        self.env = env;
        self.user_func_params = user_func_params;
        Ok(typed)
    }

    pub fn checkpoint(&self) -> ScarCheckpoint {
        ScarCheckpoint {
            env: self.env.clone(),
            user_func_params: self.user_func_params.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: ScarCheckpoint) {
        self.env = checkpoint.env;
        self.user_func_params = checkpoint.user_func_params;
    }
}

impl Default for ScarSession {
    fn default() -> Self {
        Self::new()
    }
}

struct Checker {
    env: TypeEnv,
    function_return_ty: Option<Ty>,
    user_func_params: HashMap<u32, Vec<String>>,
    substitutions: HashMap<u32, Ty>,
}

impl Checker {
    fn new() -> Self {
        Self {
            env: initialize_env(),
            function_return_ty: None,
            user_func_params: HashMap::new(),
            substitutions: HashMap::new(),
        }
    }

    fn with_env_and_params(env: TypeEnv, user_func_params: HashMap<u32, Vec<String>>) -> Self {
        Self {
            env,
            function_return_ty: None,
            user_func_params,
            substitutions: HashMap::new(),
        }
    }

    fn into_parts(self) -> (TypeEnv, HashMap<u32, Vec<String>>) {
        (self.env, self.user_func_params)
    }

    fn check_program(&mut self, stmts: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.predeclare_error_types(&stmts);
        self.predeclare_functions(&stmts)?;
        let mut typed = Vec::new();
        for stmt in stmts {
            let node = self.check_node(&stmt)?;
            typed.push(self.resolve_typed_node(node));
        }
        Ok(typed)
    }

    fn predeclare_error_types(&mut self, stmts: &[Resolved]) {
        for stmt in stmts {
            if let Resolved::DeferrorDef(_, id, _, _) = stmt {
                self.env.declare_error_type_name(id.name.clone());
            }
        }
    }

    fn predeclare_functions(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut fun_idx = self.env.next_fun_idx;

        for stmt in stmts {
            match stmt {
                Resolved::BuiltinDecl(_, id, params, ret_ty) => {
                    let mut tyvars = HashMap::new();
                    let param_tys = params
                        .iter()
                        .map(|param| {
                            self.resolve_builtin_ast_ty_in_context(
                                &param.ty,
                                TypeSyntaxContext::General,
                                &mut tyvars,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let ret = match ret_ty {
                        Some(ty) => self.resolve_builtin_ast_ty_in_context(
                            ty,
                            TypeSyntaxContext::FunctionReturn,
                            &mut tyvars,
                        )?,
                        None => Ty::Unit,
                    };
                    self.env.bind_var(
                        id.unique_id,
                        Ty::BuiltinFunc {
                            name: id.name.clone(),
                            params: param_tys,
                            ret: Box::new(ret),
                        },
                    );
                }
                Resolved::Def(_, id, params, ret_ty, _) => {
                    let param_tys = params
                        .iter()
                        .map(|param| {
                            self.resolve_ast_ty_in_context(&param.ty, TypeSyntaxContext::General)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let param_names = params
                        .iter()
                        .map(|param| param.id.name.clone())
                        .collect::<Vec<_>>();
                    let ret = match ret_ty {
                        Some(ty) => {
                            self.resolve_ast_ty_in_context(ty, TypeSyntaxContext::FunctionReturn)?
                        }
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
                    self.user_func_params.insert(id.unique_id, param_names);
                    fun_idx += 1;
                }
                _ => {}
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
                let stored_ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(|| {
                    TypeError {
                        message: format!("Undefined variable: {}", id.name),
                        span: span.clone(),
                        hint: None,
                    }
                })?;
                let ty = match &stored_ty {
                    Ty::BuiltinFunc { .. } => self.instantiate_builtin_ty(&stored_ty),
                    _ => self.resolve_ty(&stored_ty),
                };
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
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
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

            Resolved::SafeBind(span, pat, rhs) => self.check_safebind(span, pat, rhs),

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
            Resolved::BuiltinDecl(span, id, params, ret_ty) => {
                self.check_builtin_decl(span, id, params, ret_ty)
            }
            Resolved::Closure(span, params, captures, body) => {
                self.check_closure(span, params, captures, body, None)
            }
            Resolved::Capture(span, target, args) => self.check_capture(span, target, args),
        }
    }

    fn check_safebind(
        &mut self,
        span: &Span,
        pat: &ResolvedPattern,
        rhs: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_rhs = self.check_node(rhs)?;
        let (ok_ty, err_ty) = match &typed_rhs.ty {
            Ty::Result(ok, err) => (ok.as_ref().clone(), err.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "`=?` requires Result on the right-hand side, got {}",
                        self.ty_name(other)
                    ),
                    span: typed_rhs.span.clone(),
                    hint: Some(
                        "Use `=` for plain values, or return Result<T> from the expression".into(),
                    ),
                });
            }
        };

        if let Some(ret_ty) = self.function_return_ty.clone() {
            match ret_ty {
                Ty::Result(_, fn_err_ty) => {
                    if !self.types_compatible(fn_err_ty.as_ref(), &err_ty) {
                        return Err(TypeError {
                            message: format!(
                                "`=?` error type mismatch: function returns {}, but expression returns {}",
                                self.ty_name(fn_err_ty.as_ref()),
                                self.ty_name(&err_ty)
                            ),
                            span: typed_rhs.span.clone(),
                            hint: None,
                        });
                    }
                }
                other => {
                    return Err(TypeError {
                        message: format!(
                            "`=?` can only be used in functions returning Result<...>, got {}",
                            self.ty_name(&other)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            }
        }

        let (typed_pat, pat_ty) = self.check_pattern(pat, &ok_ty, span)?;
        if let TypedPattern::Var(_, id) = &typed_pat {
            self.env.bind_var(id.unique_id, pat_ty);
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::SafeBind(typed_pat, Box::new(typed_rhs)),
        })
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

    fn ast_ty_span(ast_ty: &AstTy) -> &Span {
        match ast_ty {
            AstTy::Named(span, _)
            | AstTy::ListOf(span, _)
            | AstTy::ResultOf(span, _, _)
            | AstTy::Func(span, _, _) => span,
        }
    }

    fn resolve_ast_ty_in_context(
        &self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
    ) -> Result<Ty, TypeError> {
        if context == TypeSyntaxContext::ErrorMarker {
            return self.resolve_error_marker_type(ast_ty);
        }

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
                let inner_ty = self.resolve_ast_ty_in_context(inner, TypeSyntaxContext::General)?;
                Ok(Ty::List(Box::new(inner_ty)))
            }
            AstTy::ResultOf(span, ok_ty, err_ty) => {
                let ok = self.resolve_ast_ty_in_context(ok_ty, TypeSyntaxContext::General)?;
                let err = match err_ty {
                    Some(e) => {
                        if context != TypeSyntaxContext::FunctionReturn {
                            return Err(TypeError {
                                message:
                                    "Result<T, E> is only allowed in function return signatures."
                                        .into(),
                                span: span.clone(),
                                hint: Some("Use Result<T> in local code.".into()),
                            });
                        }
                        self.resolve_ast_ty_in_context(e, TypeSyntaxContext::ErrorMarker)?
                    }
                    None => Ty::Error, // Result<T> = Result<T, Error>
                };
                Ok(Ty::Result(Box::new(ok), Box::new(err)))
            }
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|p| self.resolve_ast_ty_in_context(p, TypeSyntaxContext::General))
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_ast_ty_in_context(ret, TypeSyntaxContext::General)?;
                Ok(Ty::Func(params, Box::new(ret)))
            }
        }
    }

    fn resolve_builtin_ast_ty(
        &mut self,
        ast_ty: &AstTy,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        self.resolve_builtin_ast_ty_in_context(ast_ty, TypeSyntaxContext::General, tyvars)
    }

    fn resolve_builtin_ast_ty_in_context(
        &mut self,
        ast_ty: &AstTy,
        context: TypeSyntaxContext,
        tyvars: &mut HashMap<String, Ty>,
    ) -> Result<Ty, TypeError> {
        match ast_ty {
            AstTy::Named(_, name) if name.starts_with('$') => {
                if context == TypeSyntaxContext::ErrorMarker {
                    return Err(TypeError {
                        message:
                            "The error marker E in Result<T, E> must be a deferror-defined type."
                                .into(),
                        span: Self::ast_ty_span(ast_ty).clone(),
                        hint: None,
                    });
                }
                if let Some(existing) = tyvars.get(name) {
                    return Ok(existing.clone());
                }
                let fresh = self.env.fresh_tyvar();
                tyvars.insert(name.clone(), fresh.clone());
                Ok(fresh)
            }
            AstTy::ListOf(_, inner) => {
                let inner_ty = self.resolve_builtin_ast_ty_in_context(
                    inner,
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
                Ok(Ty::List(Box::new(inner_ty)))
            }
            AstTy::ResultOf(span, ok_ty, err_ty) => {
                let ok = self.resolve_builtin_ast_ty_in_context(
                    ok_ty,
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
                let err = match err_ty {
                    Some(e) => {
                        if context != TypeSyntaxContext::FunctionReturn {
                            return Err(TypeError {
                                message:
                                    "Result<T, E> is only allowed in function return signatures."
                                        .into(),
                                span: span.clone(),
                                hint: Some("Use Result<T> in local code.".into()),
                            });
                        }
                        self.resolve_builtin_ast_ty_in_context(
                            e,
                            TypeSyntaxContext::ErrorMarker,
                            tyvars,
                        )?
                    }
                    None => Ty::Error,
                };
                Ok(Ty::Result(Box::new(ok), Box::new(err)))
            }
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(|p| {
                        self.resolve_builtin_ast_ty_in_context(
                            p,
                            TypeSyntaxContext::General,
                            tyvars,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let ret = self.resolve_builtin_ast_ty_in_context(
                    ret,
                    TypeSyntaxContext::General,
                    tyvars,
                )?;
                Ok(Ty::Func(params, Box::new(ret)))
            }
            _ => self.resolve_ast_ty_in_context(ast_ty, context),
        }
    }

    fn resolve_error_marker_type(&self, ast_ty: &AstTy) -> Result<Ty, TypeError> {
        let span = Self::ast_ty_span(ast_ty).clone();
        let AstTy::Named(_, name) = ast_ty else {
            return Err(TypeError {
                message: "The error marker E in Result<T, E> must be a deferror-defined type."
                    .into(),
                span,
                hint: None,
            });
        };

        let def = self.env.lookup_type_def(name).ok_or_else(|| TypeError {
            message: "The error marker E in Result<T, E> must be a deferror-defined type.".into(),
            span: span.clone(),
            hint: None,
        });

        if let Ok(def) = def {
            if def.kind != crate::env::TypeKind::Error {
                return Err(TypeError {
                    message:
                        "The error marker E in Result<T, E> must be a deferror-defined type."
                            .into(),
                    span,
                    hint: None,
                });
            }
            return Ok(Ty::Error);
        }

        if !self.env.is_declared_error_type_name(name) {
            return Err(TypeError {
                message: "The error marker E in Result<T, E> must be a deferror-defined type."
                    .into(),
                span,
                hint: None,
            });
        }

        Ok(Ty::Error)
    }

    fn types_compatible(&mut self, expected: &Ty, got: &Ty) -> bool {
        let expected = self.resolve_ty(expected);
        let got = self.resolve_ty(got);
        match (&expected, &got) {
            (Ty::Var(var), ty) | (ty, Ty::Var(var)) => self.bind_tyvar(*var, ty),
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

    fn bind_tyvar(&mut self, var: u32, ty: &Ty) -> bool {
        let ty = self.resolve_ty(ty);
        if ty == Ty::Var(var) {
            return true;
        }
        if self.ty_contains_var(&ty, var) {
            return false;
        }
        self.substitutions.insert(var, ty);
        true
    }

    fn ty_contains_var(&self, ty: &Ty, needle: u32) -> bool {
        match self.resolve_ty(ty) {
            Ty::Var(var) => var == needle,
            Ty::List(inner) => self.ty_contains_var(&inner, needle),
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .any(|param| self.ty_contains_var(param, needle))
                    || self.ty_contains_var(&ret, needle)
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.ty_contains_var(param, needle))
                    || self.ty_contains_var(&ret, needle)
            }
            Ty::Result(ok, err) => {
                self.ty_contains_var(&ok, needle) || self.ty_contains_var(&err, needle)
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
                .iter()
                .any(|(_, field_ty)| self.ty_contains_var(field_ty, needle)),
            _ => false,
        }
    }

    fn resolve_ty(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(var) => match self.substitutions.get(var) {
                Some(bound) => self.resolve_ty(bound),
                None => Ty::Var(*var),
            },
            Ty::List(inner) => Ty::List(Box::new(self.resolve_ty(inner))),
            Ty::Func(params, ret) => Ty::Func(
                params.iter().map(|param| self.resolve_ty(param)).collect(),
                Box::new(self.resolve_ty(ret)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params.iter().map(|param| self.resolve_ty(param)).collect(),
                ret: Box::new(self.resolve_ty(ret)),
            },
            Ty::UserFunc {
                fun_idx,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                params: params.iter().map(|param| self.resolve_ty(param)).collect(),
                ret: Box::new(self.resolve_ty(ret)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| (field.clone(), self.resolve_ty(field_ty)))
                    .collect(),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                fields
                    .iter()
                    .map(|(field, field_ty)| (field.clone(), self.resolve_ty(field_ty)))
                    .collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.resolve_ty(ok)),
                Box::new(self.resolve_ty(err)),
            ),
            other => other.clone(),
        }
    }

    fn instantiate_builtin_ty(&mut self, ty: &Ty) -> Ty {
        fn instantiate(checker: &mut Checker, ty: &Ty, fresh: &mut HashMap<u32, Ty>) -> Ty {
            match ty {
                Ty::Var(var) => fresh
                    .entry(*var)
                    .or_insert_with(|| checker.env.fresh_tyvar())
                    .clone(),
                Ty::List(inner) => Ty::List(Box::new(instantiate(checker, inner, fresh))),
                Ty::Func(params, ret) => Ty::Func(
                    params
                        .iter()
                        .map(|param| instantiate(checker, param, fresh))
                        .collect(),
                    Box::new(instantiate(checker, ret, fresh)),
                ),
                Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                    name: name.clone(),
                    params: params
                        .iter()
                        .map(|param| instantiate(checker, param, fresh))
                        .collect(),
                    ret: Box::new(instantiate(checker, ret, fresh)),
                },
                Ty::UserFunc {
                    fun_idx,
                    params,
                    ret,
                } => Ty::UserFunc {
                    fun_idx: *fun_idx,
                    params: params
                        .iter()
                        .map(|param| instantiate(checker, param, fresh))
                        .collect(),
                    ret: Box::new(instantiate(checker, ret, fresh)),
                },
                Ty::Struct(name, fields) => Ty::Struct(
                    name.clone(),
                    fields
                        .iter()
                        .map(|(field, field_ty)| {
                            (field.clone(), instantiate(checker, field_ty, fresh))
                        })
                        .collect(),
                ),
                Ty::Record(name, fields) => Ty::Record(
                    name.clone(),
                    fields
                        .iter()
                        .map(|(field, field_ty)| {
                            (field.clone(), instantiate(checker, field_ty, fresh))
                        })
                        .collect(),
                ),
                Ty::Result(ok, err) => Ty::Result(
                    Box::new(instantiate(checker, ok, fresh)),
                    Box::new(instantiate(checker, err, fresh)),
                ),
                other => other.clone(),
            }
        }

        let mut fresh = HashMap::new();
        instantiate(self, ty, &mut fresh)
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
            Ty::Func(params, ret) => format!("{}", {
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
            }),
            Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
            Ty::UserFunc { .. } => "UserFunc".into(),
        }
    }

    fn resolve_typed_node(&self, node: TypedNode) -> TypedNode {
        let span = node.span.clone();
        let ty = self.resolve_ty(&node.ty);
        let node = match node.node {
            TypedInner::Lit(lit) => TypedInner::Lit(lit),
            TypedInner::Var(id) => TypedInner::Var(id),
            TypedInner::App(func, args) => TypedInner::App(
                Box::new(self.resolve_typed_node(*func)),
                args.into_iter()
                    .map(|arg| self.resolve_typed_node(arg))
                    .collect(),
            ),
            TypedInner::Block(stmts) => TypedInner::Block(
                stmts
                    .into_iter()
                    .map(|stmt| self.resolve_typed_node(stmt))
                    .collect(),
            ),
            TypedInner::Bind(pattern, rhs) => TypedInner::Bind(
                self.resolve_typed_pattern(pattern),
                Box::new(self.resolve_typed_node(*rhs)),
            ),
            TypedInner::SafeBind(pattern, rhs) => TypedInner::SafeBind(
                self.resolve_typed_pattern(pattern),
                Box::new(self.resolve_typed_node(*rhs)),
            ),
            TypedInner::BinOp(op, left, right) => TypedInner::BinOp(
                op,
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::List(elems) => TypedInner::List(
                elems
                    .into_iter()
                    .map(|elem| self.resolve_typed_node(elem))
                    .collect(),
            ),
            TypedInner::InterpolatedStr(parts) => TypedInner::InterpolatedStr(
                parts
                    .into_iter()
                    .map(|part| match part {
                        TypedInterpolatedPart::Text(text) => TypedInterpolatedPart::Text(text),
                        TypedInterpolatedPart::Expr(expr) => {
                            TypedInterpolatedPart::Expr(Box::new(self.resolve_typed_node(*expr)))
                        }
                    })
                    .collect(),
            ),
            TypedInner::If(cond, then, else_opt) => TypedInner::If(
                Box::new(self.resolve_typed_node(*cond)),
                Box::new(self.resolve_typed_node(*then)),
                else_opt.map(|node| Box::new(self.resolve_typed_node(*node))),
            ),
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                Box::new(self.resolve_typed_node(*scrutinee)),
                arms.into_iter()
                    .map(|(pat, body)| (pat, self.resolve_typed_node(body)))
                    .collect(),
            ),
            TypedInner::FieldAccess(expr, idx) => {
                TypedInner::FieldAccess(Box::new(self.resolve_typed_node(*expr)), idx)
            }
            TypedInner::StructLit(tag, fields) => TypedInner::StructLit(
                tag,
                fields
                    .into_iter()
                    .map(|field| self.resolve_typed_node(field))
                    .collect(),
            ),
            TypedInner::ConstructorCall(tag, fields) => TypedInner::ConstructorCall(
                tag,
                fields
                    .into_iter()
                    .map(|field| self.resolve_typed_node(field))
                    .collect(),
            ),
            TypedInner::DeferrorDef(tag, binding, id, params, show) => TypedInner::DeferrorDef(
                tag,
                binding,
                id,
                params
                    .into_iter()
                    .map(|param| TypedFunParam {
                        id: param.id,
                        ty: self.resolve_ty(&param.ty),
                    })
                    .collect(),
                Box::new(self.resolve_typed_node(*show)),
            ),
            TypedInner::Def(fun_idx, id, params, ret_ty, body) => TypedInner::Def(
                fun_idx,
                id,
                params
                    .into_iter()
                    .map(|param| TypedFunParam {
                        id: param.id,
                        ty: self.resolve_ty(&param.ty),
                    })
                    .collect(),
                self.resolve_ty(&ret_ty),
                Box::new(self.resolve_typed_node(*body)),
            ),
            TypedInner::Closure(params, captures, body) => TypedInner::Closure(
                params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: self.resolve_ty(&param.ty),
                    })
                    .collect(),
                captures,
                Box::new(self.resolve_typed_node(*body)),
            ),
            TypedInner::Capture(target, args) => TypedInner::Capture(
                Box::new(self.resolve_typed_node(*target)),
                args.into_iter()
                    .map(|arg| self.resolve_typed_node(arg))
                    .collect(),
            ),
            TypedInner::StructDef(tag, name, field_names) => {
                TypedInner::StructDef(tag, name, field_names)
            }
            TypedInner::RecordDef(tag, name, field_names) => {
                TypedInner::RecordDef(tag, name, field_names)
            }
            TypedInner::Semi(inner) => TypedInner::Semi(Box::new(self.resolve_typed_node(*inner))),
        };

        TypedNode { ty, span, node }
    }

    fn resolve_typed_pattern(&self, pattern: TypedPattern) -> TypedPattern {
        match pattern {
            TypedPattern::Var(ty, id) => TypedPattern::Var(self.resolve_ty(&ty), id),
            TypedPattern::Wildcard(ty) => TypedPattern::Wildcard(self.resolve_ty(&ty)),
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
            TypedInner::Block(stmts) => stmts
                .last()
                .and_then(|last| self.find_tail_print_call(last)),
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
        self.tail_expr_span(body)
            .unwrap_or_else(|| body.span.clone())
    }

    fn tail_expr_span(&self, node: &TypedNode) -> Option<Span> {
        match &node.node {
            TypedInner::Block(stmts) => stmts.last().map(|last| {
                self.tail_expr_span(last)
                    .unwrap_or_else(|| last.span.clone())
            }),
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
            ResolvedPattern::Var(id) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Var(rhs_ty.clone(), id.clone()), rhs_ty))
            }
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected = self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
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
                let expected = self.resolve_ty(&expected);
                Ok((TypedPattern::Var(expected.clone(), id.clone()), expected))
            }
            ResolvedPattern::Wildcard(_wspan) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Wildcard(rhs_ty.clone()), rhs_ty))
            }
        }
    }

    // ── Function application ──

    fn check_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let typed_func = self.check_node(func)?;
        let func_ty = self.resolve_ty(&typed_func.ty);

        match &func_ty {
            Ty::BuiltinFunc { name, params, ret } => {
                if args
                    .iter()
                    .any(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)))
                {
                    return Err(TypeError {
                        message: format!("{} does not accept named arguments", name),
                        span: span.clone(),
                        hint: None,
                    });
                }

                let typed_args: Vec<TypedNode> = args
                    .iter()
                    .map(|arg| match arg {
                        ResolvedRecordLitArg::Positional(expr) => self.check_node(expr),
                        ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
                    })
                    .collect::<Result<Vec<_>, _>>()?;

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
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::UserFunc { params, ret, .. } => {
                let param_names = match func {
                    Resolved::Var(_, id) => self.user_func_params.get(&id.unique_id).cloned(),
                    _ => None,
                };

                let mut reordered = vec![None; params.len()];
                let mut positional_idx = 0usize;
                let mut seen_named = false;

                for arg in args {
                    match arg {
                        ResolvedRecordLitArg::Positional(expr) => {
                            if seen_named {
                                return Err(TypeError {
                                    message:
                                        "Positional arguments must come before named arguments"
                                            .into(),
                                    span: span.clone(),
                                    hint: None,
                                });
                            }
                            if positional_idx >= params.len() {
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
                            reordered[positional_idx] = Some(expr);
                            positional_idx += 1;
                        }
                        ResolvedRecordLitArg::Named(name, expr) => {
                            seen_named = true;
                            let names = param_names.as_ref().ok_or_else(|| TypeError {
                                message: "This function value does not accept named arguments"
                                    .into(),
                                span: span.clone(),
                                hint: None,
                            })?;
                            let idx =
                                names
                                    .iter()
                                    .position(|n| n == name)
                                    .ok_or_else(|| TypeError {
                                        message: format!(
                                            "Unknown argument name '{}' for function",
                                            name
                                        ),
                                        span: span.clone(),
                                        hint: None,
                                    })?;
                            if reordered[idx].is_some() {
                                return Err(TypeError {
                                    message: format!("Duplicate argument '{}'", name),
                                    span: span.clone(),
                                    hint: None,
                                });
                            }
                            reordered[idx] = Some(expr);
                        }
                    }
                }

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

                let mut typed_args = Vec::with_capacity(params.len());
                for (idx, param) in params.iter().enumerate() {
                    let expr = reordered[idx].ok_or_else(|| TypeError {
                        message: if let Some(names) = param_names.as_ref() {
                            format!("Missing argument '{}'", names[idx])
                        } else {
                            format!("Missing argument #{}", idx + 1)
                        },
                        span: span.clone(),
                        hint: None,
                    })?;
                    let arg = self.check_node(expr)?;
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
                    typed_args.push(arg);
                }

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::Func(params, ret) => {
                if args
                    .iter()
                    .any(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)))
                {
                    return Err(TypeError {
                        message: "Function values do not accept named arguments".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }

                let typed_args: Vec<TypedNode> = args
                    .iter()
                    .map(|arg| match arg {
                        ResolvedRecordLitArg::Positional(expr) => self.check_node(expr),
                        ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
                    })
                    .collect::<Result<Vec<_>, _>>()?;

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
                    ty: self.resolve_ty(ret),
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

        let mut body_checker = Checker::with_env_and_params(fun_env, self.user_func_params.clone());
        if let Some(Ty::Func(_, expected_ret)) = expected {
            body_checker.function_return_ty = Some(expected_ret.as_ref().clone());
        }
        let typed_body = body_checker.check_node(body)?;
        let typed_body = body_checker.resolve_typed_node(typed_body);
        self.env.next_tyvar = self.env.next_tyvar.max(body_checker.env.next_tyvar);
        self.env.next_tag = self.env.next_tag.max(body_checker.env.next_tag);

        let param_tys = typed_params
            .iter()
            .map(|p| body_checker.resolve_ty(&p.ty))
            .collect::<Vec<_>>();
        Ok(TypedNode {
            ty: Ty::Func(param_tys, Box::new(body_checker.resolve_ty(&typed_body.ty))),
            span: span.clone(),
            node: TypedInner::Closure(
                typed_params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: body_checker.resolve_ty(&param.ty),
                    })
                    .collect(),
                captures.to_vec(),
                Box::new(typed_body),
            ),
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

        let target_ty = self.resolve_ty(&typed_target.ty);
        let (params, ret) = match &target_ty {
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
            ty: Ty::Func(
                remaining
                    .into_iter()
                    .map(|ty| self.resolve_ty(&ty))
                    .collect(),
                Box::new(self.resolve_ty(&ret)),
            ),
            span: span.clone(),
            node: TypedInner::Capture(Box::new(typed_target), typed_args),
        })
    }

    fn maybe_call_zero_arg_function(&self, node: TypedNode, _call_span: Span) -> TypedNode {
        match &node.ty {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret)
                if params.is_empty() =>
            {
                TypedNode {
                    ty: ret.as_ref().clone(),
                    span: node.span.clone(),
                    node: TypedInner::App(Box::new(node), Vec::new()),
                }
            }
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
        let lt = self.resolve_ty(&typed_left.ty);
        let rt = self.resolve_ty(&typed_right.ty);

        let result_ty = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => match (&lt, &rt) {
                (Ty::Int, Ty::Int) => Ok(Ty::Int),
                (Ty::Float, Ty::Float) => Ok(Ty::Float),
                (Ty::Var(_), Ty::Int) | (Ty::Int, Ty::Var(_)) => {
                    self.types_compatible(&lt, &Ty::Int);
                    self.types_compatible(&rt, &Ty::Int);
                    Ok(Ty::Int)
                }
                (Ty::Var(_), Ty::Float) | (Ty::Float, Ty::Var(_)) => {
                    self.types_compatible(&lt, &Ty::Float);
                    self.types_compatible(&rt, &Ty::Float);
                    Ok(Ty::Float)
                }
                _ => Err(TypeError {
                    message: format!(
                        "Cannot apply {:?} to {} and {}",
                        op,
                        self.ty_name(&lt),
                        self.ty_name(&rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            },
            BinOp::Eq | BinOp::Neq => match (&lt, &rt) {
                (Ty::Int, Ty::Int) | (Ty::Str, Ty::Str) | (Ty::Bool, Ty::Bool) => Ok(Ty::Bool),
                _ if !self.types_compatible(&lt, &rt) => Err(TypeError {
                    message: format!(
                        "Cannot compare {} and {}",
                        self.ty_name(&lt),
                        self.ty_name(&rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
                _ => Err(TypeError {
                    message: format!("== / != not supported for {} in phase 1", self.ty_name(&lt)),
                    span: span.clone(),
                    hint: None,
                }),
            },
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => match (&lt, &rt) {
                (Ty::Int, Ty::Int) | (Ty::Float, Ty::Float) => Ok(Ty::Bool),
                _ => Err(TypeError {
                    message: format!(
                        "Cannot compare {} and {}",
                        self.ty_name(&lt),
                        self.ty_name(&rt)
                    ),
                    span: span.clone(),
                    hint: None,
                }),
            },
            BinOp::Concat => match (&lt, &rt) {
                (Ty::Str, Ty::Str) => Ok(Ty::Str),
                _ => Err(TypeError {
                    message: format!(
                        "++ requires (String, String), got ({}, {})",
                        self.ty_name(&lt),
                        self.ty_name(&rt)
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
                // if_then/2 — returns Unit
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

    fn check_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedFunParam],
        ret_ty: &Option<AstTy>,
    ) -> Result<TypedNode, TypeError> {
        let meta = builtin_meta_by_name(&id.name).ok_or_else(|| TypeError {
            message: format!("Unknown builtin declaration: {}", id.name),
            span: span.clone(),
            hint: None,
        })?;
        let expected_uid = builtin_uid(meta.builtin_id);
        if id.unique_id != expected_uid {
            return Err(TypeError {
                message: format!(
                    "Builtin {} has inconsistent unique_id: expected {}, got {}",
                    id.name, expected_uid, id.unique_id
                ),
                span: span.clone(),
                hint: None,
            });
        }
        if params.len() != usize::from(meta.arity) {
            return Err(TypeError {
                message: format!(
                    "Builtin {} arity mismatch: expected {}, got {}",
                    id.name,
                    meta.arity,
                    params.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let mut tyvars = HashMap::new();
        let param_tys = params
            .iter()
            .map(|param| self.resolve_builtin_ast_ty(&param.ty, &mut tyvars))
            .collect::<Result<Vec<_>, _>>()?;
        let ret = match ret_ty {
            Some(ty) => self.resolve_builtin_ast_ty(ty, &mut tyvars)?,
            None => Ty::Unit,
        };

        self.env.bind_var(
            id.unique_id,
            Ty::BuiltinFunc {
                name: id.name.clone(),
                params: param_tys,
                ret: Box::new(ret),
            },
        );

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Unit),
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
            let param_ty = self.resolve_ast_ty_in_context(&param.ty, TypeSyntaxContext::General)?;
            fun_env.bind_var(param.id.unique_id, param_ty.clone());
            typed_params.push(TypedFunParam {
                id: param.id.clone(),
                ty: param_ty.clone(),
            });
        }

        let expected_ret = match ret_ty {
            Some(ty) => self.resolve_ast_ty_in_context(ty, TypeSyntaxContext::FunctionReturn)?,
            None => Ty::Unit,
        };

        let mut body_checker = Checker::with_env_and_params(fun_env, self.user_func_params.clone());
        body_checker.function_return_ty = Some(expected_ret.clone());
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
            node: TypedInner::Def(
                fun_idx,
                id.clone(),
                typed_params,
                expected_ret,
                Box::new(typed_body),
            ),
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
            .map(|f| {
                Ok((
                    f.name.clone(),
                    self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                ))
            })
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
            .map(|f| {
                Ok((
                    f.name.clone(),
                    self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                ))
            })
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
            if id.name == "Err" {
                if !matches!(inner.ty, Ty::Error) {
                    return Err(TypeError {
                        message: "Err(...) requires a concrete deferror value.".into(),
                        span: inner.span.clone(),
                        hint: Some(
                            "Use a deferror-defined value in Err(...), not a plain value.".into(),
                        ),
                    });
                }
                if !self.is_concrete_error_value(&inner) {
                    return Err(TypeError {
                        message: "Error is abstract and cannot be constructed directly.".into(),
                        span: inner.span.clone(),
                        hint: Some("Use a concrete deferror value in Err(...).".into()),
                    });
                }
            }
            let (tag, result_ty) = if id.name == "Ok" {
                (
                    0u32,
                    Ty::Result(Box::new(inner.ty.clone()), Box::new(Ty::Error)),
                )
            } else {
                let ok_var = self.env.fresh_tyvar();
                (
                    1u32,
                    Ty::Result(Box::new(ok_var), Box::new(Ty::Error)),
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
                let ty = self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?;
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
        self.env.register_error_constructor(id.unique_id);

        for (ty, resolved_id) in &ty_fields {
            show_env.bind_var(resolved_id.unique_id, ty.clone());
        }
        let mut show_checker =
            Checker::with_env_and_params(show_env, self.user_func_params.clone());
        show_checker.function_return_ty = Some(Ty::Str);
        let typed_show = show_checker
            .check_node(show_expr)
            .map_err(|err| TypeError {
                message: err.message,
                span: span.clone(),
                hint: err.hint,
            })?;
        self.env.next_tyvar = self.env.next_tyvar.max(show_checker.env.next_tyvar);
        self.env.next_tag = self.env.next_tag.max(show_checker.env.next_tag);
        if !self.types_compatible(&Ty::Str, &typed_show.ty) {
            return Err(TypeError {
                message: format!(
                    "deferror show block must return String, got {}",
                    self.ty_name(&typed_show.ty)
                ),
                span: span.clone(),
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

    fn is_concrete_error_value(&self, node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::Var(id) => self.env.is_error_constructor(id.unique_id),
            TypedInner::App(func, _) => match &func.node {
                TypedInner::Var(id) => self.env.is_error_constructor(id.unique_id),
                _ => false,
            },
            _ => false,
        }
    }
}
