use std::collections::{HashMap, HashSet};

use sigil::resolved::*;
use sindr::builtin::{
    builtin_meta_by_name, builtin_type_meta_by_name, builtin_uid, BuiltinMeta, BUILTIN_METAS,
    BUILTIN_TYPE_METAS,
};
use spire::ast::{AstTy, BinOp, Lit, Span};
use spire::{SetExitCodePolicy, SourceRules};

use crate::env::{TypeEnv, TypeKind};
use crate::error::TypeError;
use crate::typed::*;
use crate::types::Ty;

mod definitions;
mod expr;
mod matching;
mod patterns;
mod predeclare;
mod types;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSyntaxContext {
    General,
    FunctionReturn,
    ErrorMarker,
}

/// Type-check the resolved AST, producing a fully typed tree.
pub fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
    typecheck_with_context(resolved, TypecheckContext::default())
}

pub fn typecheck_with_context(
    resolved: Vec<Resolved>,
    context: TypecheckContext,
) -> Result<Vec<TypedNode>, TypeError> {
    let mut checker = Checker::new(context);
    checker.check_program(resolved)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckContext {
    pub source_rules: SourceRules,
    pub enforce_builtin_type_contracts: bool,
}

impl Default for TypecheckContext {
    fn default() -> Self {
        Self {
            source_rules: SourceRules::script(),
            enforce_builtin_type_contracts: false,
        }
    }
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
        "set_exit_code" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Unit),
        },
        "shl" | "shr" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
        },
        "bit_and" | "bit_or" | "bit_xor" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Int),
        },
        "bit_not" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Int),
        },
        "test_bit" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Bool), Box::new(Ty::Error))),
        },
        "set_bit" | "clear_bit" | "toggle_bit" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
        },
        "codepoints" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Str, Ty::Enum("StringEncoding".into(), Vec::new())],
            ret: Box::new(Ty::Result(
                Box::new(Ty::List(Box::new(Ty::Int))),
                Box::new(Ty::Error),
            )),
        },
        "from_codepoints" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![
                Ty::List(Box::new(Ty::Int)),
                Ty::Enum("StringEncoding".into(), Vec::new()),
            ],
            ret: Box::new(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Error))),
        },
        "len" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::List(Box::new(a))],
                ret: Box::new(Ty::Int),
            }
        }
        _ => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Unit; meta.arity as usize],
            ret: Box::new(Ty::Unit),
        },
    }
}

fn format_builtin_type_param_suffix(params: &[&str]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    }
}

#[derive(Debug, Clone)]
pub struct ScarCheckpoint {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
}

#[derive(Debug, Clone)]
pub struct ScarSession {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
}

struct CheckerParts {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
}

impl ScarSession {
    pub fn new() -> Self {
        Self {
            env: initialize_env(),
            user_func_params: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
        }
    }

    pub fn typecheck(&mut self, resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.typecheck_with_context(resolved, TypecheckContext::default())
    }

    pub fn typecheck_with_context(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut checker = Checker::with_env_and_params(
            self.env.clone(),
            self.user_func_params.clone(),
            self.impl_method_uids.clone(),
            self.function_ids_by_name.clone(),
            context,
        );
        let typed = checker.check_program(resolved)?;
        let CheckerParts {
            env,
            user_func_params,
            impl_method_uids,
            function_ids_by_name,
        } = checker.into_parts();
        self.env = env;
        self.user_func_params = user_func_params;
        self.impl_method_uids = impl_method_uids;
        self.function_ids_by_name = function_ids_by_name;
        Ok(typed)
    }

    pub fn checkpoint(&self) -> ScarCheckpoint {
        ScarCheckpoint {
            env: self.env.clone(),
            user_func_params: self.user_func_params.clone(),
            impl_method_uids: self.impl_method_uids.clone(),
            function_ids_by_name: self.function_ids_by_name.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: ScarCheckpoint) {
        self.env = checkpoint.env;
        self.user_func_params = checkpoint.user_func_params;
        self.impl_method_uids = checkpoint.impl_method_uids;
        self.function_ids_by_name = checkpoint.function_ids_by_name;
    }

    pub fn ensure_next_fun_idx_at_least(&mut self, next_fun_idx: u32) {
        self.env.next_fun_idx = self.env.next_fun_idx.max(next_fun_idx);
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
    current_function_symbol: Option<String>,
    current_impl_struct_target: Option<String>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    substitutions: HashMap<u32, Ty>,
    source_rules: SourceRules,
    enforce_builtin_type_contracts: bool,
    seen_builtin_type_decls: HashMap<String, (Vec<String>, Span)>,
}

impl Checker {
    fn new(context: TypecheckContext) -> Self {
        Self {
            env: initialize_env(),
            function_return_ty: None,
            current_function_symbol: None,
            current_impl_struct_target: None,
            user_func_params: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
            substitutions: HashMap::new(),
            source_rules: context.source_rules,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            seen_builtin_type_decls: HashMap::new(),
        }
    }

    fn with_env_and_params(
        env: TypeEnv,
        user_func_params: HashMap<u32, Vec<String>>,
        impl_method_uids: HashMap<String, u32>,
        function_ids_by_name: HashMap<String, ResolvedId>,
        context: TypecheckContext,
    ) -> Self {
        Self {
            env,
            function_return_ty: None,
            current_function_symbol: None,
            current_impl_struct_target: None,
            user_func_params,
            impl_method_uids,
            function_ids_by_name,
            substitutions: HashMap::new(),
            source_rules: context.source_rules,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            seen_builtin_type_decls: HashMap::new(),
        }
    }

    fn spawn_child_checker(&self, env: TypeEnv) -> Self {
        let mut checker = Checker::with_env_and_params(
            env,
            self.user_func_params.clone(),
            self.impl_method_uids.clone(),
            self.function_ids_by_name.clone(),
            TypecheckContext {
                source_rules: self.source_rules.clone(),
                enforce_builtin_type_contracts: self.enforce_builtin_type_contracts,
            },
        );
        checker.function_return_ty = self.function_return_ty.clone();
        checker.current_function_symbol = self.current_function_symbol.clone();
        checker.current_impl_struct_target = self.current_impl_struct_target.clone();
        checker.impl_method_uids = self.impl_method_uids.clone();
        checker.function_ids_by_name = self.function_ids_by_name.clone();
        checker.substitutions = self.substitutions.clone();
        checker.seen_builtin_type_decls = self.seen_builtin_type_decls.clone();
        checker
    }

    fn absorb_child_progress(&mut self, child: &Checker) {
        self.substitutions = child.substitutions.clone();
        self.env.next_tyvar = self.env.next_tyvar.max(child.env.next_tyvar);
        self.env.next_tag = self.env.next_tag.max(child.env.next_tag);
        self.seen_builtin_type_decls = child.seen_builtin_type_decls.clone();
        self.impl_method_uids = child.impl_method_uids.clone();
    }

    fn into_parts(self) -> CheckerParts {
        CheckerParts {
            env: self.env,
            user_func_params: self.user_func_params,
            impl_method_uids: self.impl_method_uids,
            function_ids_by_name: self.function_ids_by_name,
        }
    }

    fn check_program(&mut self, stmts: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.predeclare_error_types(&stmts);
        self.predeclare_type_signatures(&stmts)?;
        self.predeclare_functions(&stmts)?;
        self.ensure_struct_impl_new_contract(&stmts)?;
        let mut typed = Vec::new();
        for stmt in stmts {
            let node = self.check_node(&stmt)?;
            typed.push(self.resolve_typed_node(node));
        }
        self.ensure_builtin_type_contracts()?;
        Ok(typed)
    }
}
