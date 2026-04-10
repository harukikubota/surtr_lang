#![allow(unused_variables)]

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
            ret: Box::new(Ty::Int),
        },
        "wrap" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![a.clone()],
                ret: Box::new(Ty::List(Box::new(a))),
            }
        }
        "map" => {
            let a = env.fresh_tyvar();
            let b = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![
                    Ty::List(Box::new(a.clone())),
                    Ty::Func(vec![a], Box::new(b.clone())),
                ],
                ret: Box::new(Ty::List(Box::new(b))),
            }
        }
        "flat_map" => {
            let a = env.fresh_tyvar();
            let b = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![
                    Ty::List(Box::new(a.clone())),
                    Ty::Func(vec![a], Box::new(Ty::List(Box::new(b.clone())))),
                ],
                ret: Box::new(Ty::List(Box::new(b))),
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
        let (env, user_func_params, impl_method_uids, function_ids_by_name) = checker.into_parts();
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

    fn into_parts(
        self,
    ) -> (
        TypeEnv,
        HashMap<u32, Vec<String>>,
        HashMap<String, u32>,
        HashMap<String, ResolvedId>,
    ) {
        (
            self.env,
            self.user_func_params,
            self.impl_method_uids,
            self.function_ids_by_name,
        )
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

    fn predeclare_error_types(&mut self, stmts: &[Resolved]) {
        for stmt in stmts {
            if let Resolved::DeferrorDef(_, id, _, _) = stmt {
                self.env.declare_error_type_name(id.name.clone());
            }
        }
    }

    fn predeclare_type_signatures(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        // Pass 1: reserve deterministic tags for all user-defined types.
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, _) => {
                    self.env
                        .predeclare_type_def(id.name.clone(), TypeKind::Struct);
                }
                Resolved::RecordDef(_, id, _) => {
                    self.env
                        .predeclare_type_def(id.name.clone(), TypeKind::Record);
                }
                Resolved::DeferrorDef(_, id, _, _) => {
                    self.env
                        .predeclare_type_def(id.name.clone(), TypeKind::Error);
                }
                Resolved::EnumDef(_, id, _) => {
                    self.env
                        .predeclare_type_def(id.name.clone(), TypeKind::Enum);
                }
                _ => {}
            }
        }

        self.ensure_no_type_cycles(stmts)?;

        // Pass 2: finalize field signatures and constructor-like bindings.
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, fields) => {
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            Ok((
                                f.name.clone(),
                                self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields.clone())
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env
                        .bind_var(id.unique_id, Ty::Struct(id.name.clone(), ty_fields));
                }
                Resolved::RecordDef(_, id, fields) => {
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            Ok((
                                f.name.clone(),
                                self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields.clone())
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env
                        .bind_var(id.unique_id, Ty::Record(id.name.clone(), ty_fields));
                }
                Resolved::DeferrorDef(_, id, fields, _) => {
                    let ty_fields = fields
                        .iter()
                        .map(|f| {
                            Ok((
                                f.name.clone(),
                                self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, TypeError>>()?;
                    self.env
                        .resolve_type_def_signature(&id.name, ty_fields)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                }
                Resolved::EnumDef(_, id, variants) => {
                    let _ = self
                        .env
                        .resolve_type_def_signature(&id.name, Vec::new())
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown type declaration: {}", id.name),
                            span: id.span.clone(),
                            hint: None,
                        })?;
                    self.env.bind_var(id.unique_id, Ty::Enum(id.name.clone()));

                    let mut next_discriminant = sindr::primitives::int(0);
                    let mut seen_discriminants: HashSet<sindr::primitives::SurtrInt> =
                        HashSet::new();
                    let mut enum_variants = Vec::new();

                    for variant in variants {
                        let discriminant = if let Some(explicit) = &variant.discriminant {
                            explicit.clone()
                        } else {
                            next_discriminant.clone()
                        };
                        if seen_discriminants.contains(&discriminant) {
                            return Err(TypeError {
                                message: format!(
                                    "Duplicate enum discriminant {} in {}",
                                    discriminant, id.name
                                ),
                                span: variant.span.clone(),
                                hint: None,
                            });
                        }
                        seen_discriminants.insert(discriminant.clone());
                        next_discriminant = discriminant.clone() + sindr::primitives::int(1);

                        let payload = variant
                            .payload
                            .iter()
                            .map(|ty| {
                                self.resolve_ast_ty_in_context(ty, TypeSyntaxContext::General)
                            })
                            .collect::<Result<Vec<_>, _>>()?;

                        let tag = self.env.reserve_tag();
                        let short_name = variant
                            .id
                            .name
                            .rsplit("::")
                            .next()
                            .unwrap_or(variant.id.name.as_str())
                            .to_string();
                        let info = crate::env::EnumVariantInfo {
                            constructor_name: variant.id.name.clone(),
                            short_name,
                            enum_name: id.name.clone(),
                            tag,
                            payload: payload.clone(),
                            discriminant: discriminant.clone(),
                        };
                        self.env
                            .register_enum_variant(variant.id.unique_id, info.clone())
                            .map_err(|message| TypeError {
                                message,
                                span: variant.span.clone(),
                                hint: None,
                            })?;
                        enum_variants.push(info);
                    }

                    self.env
                        .enum_variants_by_enum
                        .insert(id.name.clone(), enum_variants);
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn ensure_no_type_cycles(&self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut decl_spans: HashMap<String, Span> = HashMap::new();
        let mut edges: HashMap<String, HashSet<String>> = HashMap::new();

        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, fields)
                | Resolved::RecordDef(_, id, fields)
                | Resolved::DeferrorDef(_, id, fields, _) => {
                    decl_spans.insert(id.name.clone(), id.span.clone());
                    edges.entry(id.name.clone()).or_default();
                    for field in fields {
                        let mut refs = Vec::new();
                        Self::collect_type_ref_names(&field.ty, &mut refs);
                        for ref_name in refs {
                            edges.entry(id.name.clone()).or_default().insert(ref_name);
                        }
                    }
                }
                Resolved::EnumDef(_, id, variants) => {
                    decl_spans.insert(id.name.clone(), id.span.clone());
                    edges.entry(id.name.clone()).or_default();
                    let mut common_refs: Option<HashSet<String>> = None;
                    for variant in variants {
                        let mut variant_refs = HashSet::new();
                        for payload_ty in &variant.payload {
                            let mut refs = Vec::new();
                            Self::collect_type_ref_names(payload_ty, &mut refs);
                            for ref_name in refs {
                                variant_refs.insert(ref_name);
                            }
                        }
                        common_refs = Some(match common_refs {
                            Some(existing) => existing
                                .intersection(&variant_refs)
                                .cloned()
                                .collect::<HashSet<_>>(),
                            None => variant_refs,
                        });
                    }
                    for ref_name in common_refs.unwrap_or_default() {
                        edges.entry(id.name.clone()).or_default().insert(ref_name);
                    }
                }
                _ => {}
            }
        }

        for refs in edges.values_mut() {
            refs.retain(|name| decl_spans.contains_key(name));
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Visit {
            Visiting,
            Done,
        }

        fn dfs(
            node: &str,
            edges: &HashMap<String, HashSet<String>>,
            states: &mut HashMap<String, Visit>,
            stack: &mut Vec<String>,
        ) -> Option<Vec<String>> {
            if let Some(state) = states.get(node) {
                if *state == Visit::Visiting {
                    let start = stack.iter().position(|name| name == node).unwrap_or(0);
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(node.to_string());
                    return Some(cycle);
                }
                if *state == Visit::Done {
                    return None;
                }
            }

            states.insert(node.to_string(), Visit::Visiting);
            stack.push(node.to_string());

            if let Some(nexts) = edges.get(node) {
                for next in nexts {
                    if let Some(cycle) = dfs(next, edges, states, stack) {
                        return Some(cycle);
                    }
                }
            }

            stack.pop();
            states.insert(node.to_string(), Visit::Done);
            None
        }

        let mut states: HashMap<String, Visit> = HashMap::new();
        let mut stack = Vec::new();
        for name in decl_spans.keys() {
            if let Some(cycle) = dfs(name, &edges, &mut states, &mut stack) {
                let head = cycle.first().cloned().unwrap_or_else(|| name.clone());
                return Err(TypeError {
                    message: format!("Cyclic type definition detected: {}", cycle.join(" -> ")),
                    span: decl_spans
                        .get(&head)
                        .cloned()
                        .unwrap_or_else(|| Span { start: 0, end: 0 }),
                    hint: None,
                });
            }
        }

        Ok(())
    }

    fn split_impl_method_name(name: &str) -> Option<(String, String)> {
        let (target, method) = name.rsplit_once("::")?;
        if target.is_empty() || method.is_empty() {
            None
        } else {
            Some((target.to_string(), method.to_string()))
        }
    }

    fn current_impl_self_ty(&self) -> Option<Ty> {
        let symbol = self.current_function_symbol.as_deref()?;
        let mut parts = symbol.split("::").collect::<Vec<_>>();
        if parts.len() < 2 {
            return None;
        }
        let target = parts.pop()?;
        let _method = target;
        let type_name = parts.pop()?;
        let def = self.env.lookup_type_def(type_name)?;
        match def.kind {
            TypeKind::Struct => Some(Ty::Struct(def.name.clone(), def.fields.clone())),
            TypeKind::Enum => Some(Ty::Enum(def.name.clone())),
            TypeKind::Record | TypeKind::Error => None,
        }
    }

    fn ensure_self_rebinding_types(
        &mut self,
        pattern: &TypedPattern,
        span: &Span,
    ) -> Result<(), TypeError> {
        let expected_self = self.current_impl_self_ty();
        self.ensure_self_rebinding_types_inner(pattern, span, expected_self.as_ref())
    }

    fn ensure_self_rebinding_types_inner(
        &mut self,
        pattern: &TypedPattern,
        span: &Span,
        expected_self: Option<&Ty>,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedPattern::Var(bind_ty, id) => {
                if id.name == "self" {
                    let Some(expected) = expected_self else {
                        return Err(TypeError {
                            message: "`self` can only be rebound inside impl methods".to_string(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !self.types_compatible(expected, bind_ty) {
                        return Err(TypeError {
                            message: format!(
                                "`self` rebinding requires Self type ({}), got {}",
                                self.ty_name(expected),
                                self.ty_name(bind_ty)
                            ),
                            span: id.span.clone(),
                            hint: None,
                        });
                    }
                }
                Ok(())
            }
            TypedPattern::As(alias_ty, inner, id) => {
                if id.name == "self" {
                    let Some(expected) = expected_self else {
                        return Err(TypeError {
                            message: "`self` can only be rebound inside impl methods".to_string(),
                            span: span.clone(),
                            hint: None,
                        });
                    };
                    if !self.types_compatible(expected, alias_ty) {
                        return Err(TypeError {
                            message: format!(
                                "`self` rebinding requires Self type ({}), got {}",
                                self.ty_name(expected),
                                self.ty_name(alias_ty)
                            ),
                            span: id.span.clone(),
                            hint: None,
                        });
                    }
                }
                self.ensure_self_rebinding_types_inner(inner, span, expected_self)
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.ensure_self_rebinding_types_inner(head, span, expected_self)?;
                self.ensure_self_rebinding_types_inner(tail, span, expected_self)
            }
            TypedPattern::ResultOk(_, inner) => {
                self.ensure_self_rebinding_types_inner(inner, span, expected_self)
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _) => Ok(()),
        }
    }

    fn ensure_struct_impl_new_contract(&self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut struct_decl_spans: HashMap<String, Span> = HashMap::new();
        let mut structs_with_new: HashSet<String> = HashSet::new();

        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, _) => {
                    struct_decl_spans.insert(id.name.clone(), id.span.clone());
                }
                Resolved::Def(_, id, _, _, _, _) => {
                    if let Some((target, method)) = Self::split_impl_method_name(&id.name) {
                        if method == "new" {
                            structs_with_new.insert(target);
                        }
                    }
                }
                _ => {}
            }
        }

        for (struct_name, span) in struct_decl_spans {
            if !structs_with_new.contains(&struct_name) {
                return Err(TypeError {
                    message: format!(
                        "Struct `{}` must define `new` in its impl block (e.g. `impl {} {{ def new(...) -> Self {{ ... }} }}`)",
                        struct_name, struct_name
                    ),
                    span,
                    hint: None,
                });
            }
        }

        Ok(())
    }

    fn predeclare_functions(&mut self, stmts: &[Resolved]) -> Result<(), TypeError> {
        let mut fun_idx = self.env.next_fun_idx;

        for stmt in stmts {
            match stmt {
                Resolved::BuiltinDecl(_, id, params, ret_ty, _) => {
                    self.register_function_id(id);
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
                Resolved::Def(_, id, params, ret_ty, _, _) => {
                    self.register_function_id(id);
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
                    if Self::split_impl_method_name(&id.name).is_some() {
                        self.impl_method_uids.insert(id.name.clone(), id.unique_id);
                    }
                    fun_idx += 1;
                }
                Resolved::DeferrorDef(_, id, fields, _) => {
                    self.register_function_id(id);
                    let param_tys = fields
                        .iter()
                        .map(|field| {
                            self.resolve_ast_ty_in_context(&field.ty, TypeSyntaxContext::General)
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                    self.env.bind_var(
                        id.unique_id,
                        Ty::UserFunc {
                            fun_idx,
                            params: param_tys,
                            ret: Box::new(Ty::Error),
                        },
                    );
                    self.env.register_error_constructor(id.unique_id);
                    fun_idx += 1;
                }
                Resolved::BuiltinTypeDecl(_, _, _, _) => {}
                Resolved::ResultCtorDecl(_, _, _, _, _) => {}
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
                if let Some(stored_ty) = self.env.lookup_var(id.unique_id).cloned() {
                    let ty = match &stored_ty {
                        Ty::BuiltinFunc { .. } => self.instantiate_builtin_ty(&stored_ty),
                        _ => self.resolve_ty(&stored_ty),
                    };
                    return Ok(TypedNode {
                        ty,
                        span: span.clone(),
                        node: TypedInner::Var(id.clone()),
                    });
                }

                if let Some(variant) = self.env.enum_variant_by_constructor_id(id.unique_id) {
                    if !variant.payload.is_empty() {
                        return Err(TypeError {
                            message: format!(
                                "Enum constructor {} expects {} argument(s)",
                                id.name,
                                variant.payload.len()
                            ),
                            span: span.clone(),
                            hint: Some("Call it as `Enum::Variant(...)`".into()),
                        });
                    }
                    let idx_node = TypedNode {
                        ty: Ty::Int,
                        span: span.clone(),
                        node: TypedInner::Lit(Lit::Int(variant.discriminant.clone())),
                    };
                    return Ok(TypedNode {
                        ty: Ty::Enum(variant.enum_name.clone()),
                        span: span.clone(),
                        node: TypedInner::ConstructorCall(variant.tag, vec![idx_node]),
                    });
                }

                Err(TypeError {
                    message: format!("Undefined variable: {}", id.name),
                    span: span.clone(),
                    hint: None,
                })
            }

            Resolved::Bind(span, pat, rhs) => {
                if Self::contains_result_test_pattern(pat) {
                    return Err(TypeError {
                        message: "Result destructuring patterns must use `=?`, not `=`".into(),
                        span: span.clone(),
                        hint: Some(
                            "Use `=?` for `Ok(...)` / nested Result pattern matching.".into(),
                        ),
                    });
                }
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
                self.ensure_self_rebinding_types(&typed_pat, span)?;

                // Store the binding type in env
                self.bind_typed_pattern(&typed_pat, &self.resolve_ty(&pat_ty));
                self.normalize_env_bindings();

                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Bind(typed_pat, Box::new(typed_rhs)),
                })
            }

            Resolved::SafeBind(span, pat, rhs) => self.check_safebind(span, pat, rhs),

            Resolved::App(span, func, args) => self.check_app(span, func, args),

            Resolved::BinOp(span, op, left, right) => self.check_binop(span, op, left, right),
            Resolved::Pipe(span, left, right) => self.check_pipe(span, left, right),
            Resolved::ContextMap(span, left, right) => self.check_context_map(span, left, right),
            Resolved::ContextBind(span, left, right) => {
                self.check_context_bind(span, left, right)
            }
            Resolved::Compose(span, left, right) => self.check_compose(span, left, right),
            Resolved::KleisliCompose(span, left, right) => {
                self.check_kleisli_compose(span, left, right)
            }

            Resolved::ListNil(span) => self.check_list_nil(span),
            Resolved::ListCons(span, head, tail) => self.check_list_cons(span, head, tail),
            Resolved::ListLiteral(span, elems) => self.check_list_literal(span, elems),

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
            Resolved::EnumDef(span, id, variants) => self.check_enum_def(span, id, variants),
            Resolved::StructLit(span, id, field_vals) => {
                self.check_struct_lit(span, id, field_vals)
            }
            Resolved::ConstructorCall(span, id, args) => {
                self.check_constructor_call(span, id, args)
            }
            Resolved::DeferrorDef(span, id, fields, show_expr) => {
                self.check_deferror_def(span, id, fields, show_expr)
            }
            Resolved::Def(span, id, params, ret_ty, body, _) => {
                self.check_def(span, id, params, ret_ty, body)
            }
            Resolved::BuiltinDecl(span, id, params, ret_ty, _) => {
                self.check_builtin_decl(span, id, params, ret_ty)
            }
            Resolved::BuiltinTypeDecl(span, id, params, attrs) => {
                self.check_builtin_type_decl(span, id, params, attrs)
            }
            Resolved::ResultCtorDecl(span, id, param_ty, ret_ty, attrs) => {
                self.check_result_ctor_decl(span, id, param_ty, ret_ty, attrs)
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
        let rhs_ty = self.resolve_ty(&typed_rhs.ty);
        let (ok_ty, mut propagated_err_tys) = match rhs_ty {
            Ty::Result(ok, err) => (ok.as_ref().clone(), vec![err.as_ref().clone()]),
            Ty::List(_) if Self::is_top_level_list_pattern(pat) => {
                // `uncons(List<A>) -> Result<_, Error>`-like behavior for list destructuring.
                (self.resolve_ty(&typed_rhs.ty), vec![Ty::Error])
            }
            other => {
                return Err(TypeError {
                    message: format!(
                        "`=?` requires Result on the right-hand side, got {}",
                        self.ty_name(&other)
                    ),
                    span: typed_rhs.span.clone(),
                    hint: Some(
                        "Use `=` for plain values, or return Result<T> from the expression".into(),
                    ),
                });
            }
        };

        let (typed_pat, pat_ty) = self.check_pattern(pat, &ok_ty, span)?;
        self.ensure_self_rebinding_types(&typed_pat, span)?;
        if let Some(ret_ty) = self.function_return_ty.clone() {
            let fn_err_ty = match ret_ty {
                Ty::Result(_, fn_err_ty) => fn_err_ty,
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
            };

            self.collect_pattern_result_error_types(&typed_pat, &mut propagated_err_tys);

            for propagated in propagated_err_tys {
                if !self.types_compatible(fn_err_ty.as_ref(), &propagated) {
                    return Err(TypeError {
                        message: format!(
                            "`=?` error type mismatch: function returns {}, but expression returns {}",
                            self.ty_name(fn_err_ty.as_ref()),
                            self.ty_name(&propagated)
                        ),
                        span: typed_rhs.span.clone(),
                        hint: None,
                    });
                }
            }
        }

        self.bind_typed_pattern(&typed_pat, &pat_ty);

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::SafeBind(typed_pat, Box::new(typed_rhs)),
        })
    }

    fn register_function_id(&mut self, id: &ResolvedId) {
        let key = id
            .qualified_name
            .clone()
            .unwrap_or_else(|| id.name.clone());
        self.function_ids_by_name.insert(key, id.clone());
    }

    fn check_compose_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _) | Resolved::Closure(_, _, _, _) => self.check_node(node),
            _ => Err(TypeError {
                message: format!(
                    "{} requires a closure or capture (`&f`, `&Type::method`, or closure)",
                    op_name
                ),
                span: self.resolved_span(node).clone(),
                hint: None,
            }),
        }
    }

    fn check_apply_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _) | Resolved::Closure(_, _, _, _) => self.check_node(node),
            Resolved::App(span, func, args) => self.check_injected_call(span, func, args, op_name),
            _ => Err(TypeError {
                message: format!(
                    "{} requires `&f`, closure, or a function call like `f(...)`",
                    op_name
                ),
                span: self.resolved_span(node).clone(),
                hint: None,
            }),
        }
    }

    fn resolved_span<'a>(&self, node: &'a Resolved) -> &'a Span {
        match node {
            Resolved::Lit(span, _)
            | Resolved::Var(span, _)
            | Resolved::App(span, _, _)
            | Resolved::Block(span, _)
            | Resolved::Bind(span, _, _)
            | Resolved::SafeBind(span, _, _)
            | Resolved::BinOp(span, _, _, _)
            | Resolved::Pipe(span, _, _)
            | Resolved::ContextMap(span, _, _)
            | Resolved::ContextBind(span, _, _)
            | Resolved::Compose(span, _, _)
            | Resolved::KleisliCompose(span, _, _)
            | Resolved::ListNil(span)
            | Resolved::ListCons(span, _, _)
            | Resolved::ListLiteral(span, _)
            | Resolved::InterpolatedStr(span, _)
            | Resolved::If(span, _, _, _)
            | Resolved::Match(span, _, _)
            | Resolved::FieldAccess(span, _, _)
            | Resolved::StructLit(span, _, _)
            | Resolved::ConstructorCall(span, _, _)
            | Resolved::StructDef(span, _, _)
            | Resolved::RecordDef(span, _, _)
            | Resolved::DeferrorDef(span, _, _, _)
            | Resolved::EnumDef(span, _, _)
            | Resolved::Def(span, _, _, _, _, _)
            | Resolved::BuiltinDecl(span, _, _, _, _)
            | Resolved::BuiltinTypeDecl(span, _, _, _)
            | Resolved::ResultCtorDecl(span, _, _, _, _)
            | Resolved::Closure(span, _, _, _)
            | Resolved::Capture(span, _, _)
            | Resolved::Semi(span, _) => span,
        }
    }

    fn function_parts<'a>(&'a self, ty: &'a Ty) -> Option<(&'a [Ty], &'a Ty)> {
        match ty {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                Some((params.as_slice(), ret.as_ref()))
            }
            Ty::Func(params, ret) => Some((params.as_slice(), ret.as_ref())),
            _ => None,
        }
    }

    fn unary_function_parts(
        &self,
        ty: &Ty,
        op_name: &str,
        span: &Span,
    ) -> Result<(Ty, Ty), TypeError> {
        let Some((params, ret)) = self.function_parts(ty) else {
            return Err(TypeError {
                message: format!("{} expects a function value", op_name),
                span: span.clone(),
                hint: None,
            });
        };
        if params.len() != 1 {
            return Err(TypeError {
                message: format!("{} expects a unary callable", op_name),
                span: span.clone(),
                hint: None,
            });
        }
        Ok((self.resolve_ty(&params[0]), self.resolve_ty(ret)))
    }

    fn typed_function_var_by_name(
        &mut self,
        name: &str,
        span: &Span,
    ) -> Result<TypedNode, TypeError> {
        let id = self.function_ids_by_name.get(name).cloned().ok_or_else(|| TypeError {
            message: format!("Missing helper function: {}", name),
            span: span.clone(),
            hint: None,
        })?;
        let ty = self
            .env
            .lookup_var(id.unique_id)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Missing helper function type: {}", name),
                span: span.clone(),
                hint: None,
            })?;
        let ty = match &ty {
            Ty::BuiltinFunc { .. } => self.instantiate_builtin_ty(&ty),
            _ => self.resolve_ty(&ty),
        };
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Var(ResolvedId {
                span: span.clone(),
                ..id
            }),
        })
    }

    fn build_typed_app(
        &mut self,
        span: &Span,
        func: TypedNode,
        args: Vec<TypedNode>,
    ) -> Result<TypedNode, TypeError> {
        let (params, ret) = match self.resolve_ty(&func.ty) {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                (params, ret.as_ref().clone())
            }
            Ty::Func(params, ret) => (params, ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!("Not a function: {}", self.ty_name(&other)),
                    span: span.clone(),
                    hint: None,
                })
            }
        };
        if params.len() != args.len() {
            return Err(TypeError {
                message: format!("function expects {} argument(s), got {}", params.len(), args.len()),
                span: span.clone(),
                hint: None,
            });
        }
        for (expected, arg) in params.iter().zip(&args) {
            if !self.types_compatible(expected, &arg.ty) {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(expected),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }
        Ok(TypedNode {
            ty: self.resolve_ty(&ret),
            span: span.clone(),
            node: TypedInner::App(Box::new(func), args),
        })
    }

    fn check_injected_call(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!("{} does not support named arguments on the right-hand side", op_name),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_func = self.check_node(func)?;
        let typed_args: Vec<TypedNode> = args
            .iter()
            .map(|arg| match arg {
                ResolvedRecordLitArg::Positional(expr) => self.check_node(expr),
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;

        let (params, ret) = match self.resolve_ty(&typed_func.ty) {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                (params, ret.as_ref().clone())
            }
            Ty::Func(params, ret) => (params, ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!("{} right-hand side is not a function call target: {}", op_name, self.ty_name(&other)),
                    span: span.clone(),
                    hint: None,
                })
            }
        };

        if params.len() != typed_args.len() + 1 {
            return Err(TypeError {
                message: format!(
                    "{} injects the left value as the first argument, so the call expects {} explicit argument(s), got {}",
                    op_name,
                    params.len().saturating_sub(1),
                    typed_args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        for (expected, arg) in params.iter().skip(1).zip(&typed_args) {
            if !self.types_compatible(expected, &arg.ty) {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(expected),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::Func(vec![self.resolve_ty(&params[0])], Box::new(self.resolve_ty(&ret))),
            span: span.clone(),
            node: TypedInner::InjectCall(Box::new(typed_func), typed_args),
        })
    }

    fn build_injected_app(
        &mut self,
        span: &Span,
        injected_value: TypedNode,
        callable: TypedNode,
    ) -> Result<TypedNode, TypeError> {
        let TypedInner::InjectCall(func, mut args) = callable.node else {
            return Err(TypeError {
                message: "internal error: expected injected call".into(),
                span: span.clone(),
                hint: None,
            });
        };
        let mut full_args = Vec::with_capacity(args.len() + 1);
        full_args.push(injected_value);
        full_args.append(&mut args);
        self.build_typed_app(span, *func, full_args)
    }

    fn list_helper_ref_by_name(
        &mut self,
        helper_name: &str,
        span: &Span,
    ) -> Result<ListHelperRef, TypeError> {
        let helper = self.typed_function_var_by_name(helper_name, span)?;
        match helper.ty {
            Ty::UserFunc { fun_idx, .. } => Ok(ListHelperRef::User(fun_idx)),
            Ty::BuiltinFunc { ref name, .. } => {
                let builtin_id = builtin_meta_by_name(name)
                    .map(|meta| meta.builtin_id)
                    .ok_or_else(|| TypeError {
                        message: format!("Unknown builtin helper: {}", helper_name),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok(ListHelperRef::Builtin(builtin_id))
            }
            _ => Err(TypeError {
                message: format!("{} must be a callable helper", helper_name),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    fn build_list_helper_call(
        &mut self,
        helper_name: &str,
        span: &Span,
        value: TypedNode,
        callable: TypedNode,
    ) -> Result<TypedNode, TypeError> {
        let helper = self.typed_function_var_by_name(helper_name, span)?;
        self.build_typed_app(span, helper, vec![value, callable])
    }

    fn ensure_plain_map_output(
        &self,
        output_ty: &Ty,
        op_name: &str,
        span: &Span,
    ) -> Result<(), TypeError> {
        match self.resolve_ty(output_ty) {
            Ty::Result(_, _) | Ty::List(_) => Err(TypeError {
                message: format!(
                    "{} expects a plain function on the right-hand side; use `|>=` for contextual output",
                    op_name
                ),
                span: span.clone(),
                hint: None,
            }),
            _ => Ok(()),
        }
    }

    fn check_pipe(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let typed_right = self.check_apply_callable(right, "`|>`")?;
        let (param, ret) = self.unary_function_parts(&typed_right.ty, "`|>`", &typed_right.span)?;
        if !self.types_compatible(&param, &typed_left.ty) {
            return Err(TypeError {
                message: format!(
                    "`|>` type mismatch: expected {}, got {}",
                    self.ty_name(&param),
                    self.ty_name(&typed_left.ty)
                ),
                span: typed_left.span.clone(),
                hint: None,
            });
        }
        match typed_right.node {
            TypedInner::InjectCall(_, _) => self.build_injected_app(span, typed_left, typed_right),
            _ => Ok(TypedNode {
                ty: ret,
                span: span.clone(),
                node: TypedInner::Pipe(Box::new(typed_left), Box::new(typed_right)),
            }),
        }
    }

    fn check_context_map(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_right = self.check_apply_callable(right, "`|*>`")?;
        let (rhs_in, rhs_out) = self.unary_function_parts(&typed_right.ty, "`|*>`", &typed_right.span)?;
        self.ensure_plain_map_output(&rhs_out, "`|*>`", &typed_right.span)?;

        let typed_left = self.check_node(left)?;
        match self.resolve_ty(&typed_left.ty) {
            Ty::Result(ok, err) => {
                if !self.types_compatible(ok.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|*>` type mismatch: expected {}, got {}",
                            self.ty_name(ok.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Result(Box::new(rhs_out), Box::new(self.resolve_ty(err.as_ref()))),
                    span: span.clone(),
                    node: TypedInner::ResultMap(Box::new(typed_left), Box::new(typed_right)),
                })
            }
            Ty::List(item) => {
                if !self.types_compatible(item.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|*>` type mismatch: expected {}, got {}",
                            self.ty_name(item.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: None,
                    });
                }
                self.build_list_helper_call("List::map", span, typed_left, typed_right)
            }
            other => Err(TypeError {
                message: format!("`|*>` requires Result or List on the left, got {}", self.ty_name(&other)),
                span: typed_left.span.clone(),
                hint: None,
            }),
        }
    }

    fn check_context_bind(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_right = self.check_apply_callable(right, "`|>=`")?;
        let (rhs_in, rhs_ret) = self.unary_function_parts(&typed_right.ty, "`|>=`", &typed_right.span)?;

        let typed_left = self.check_node(left)?;
        match (self.resolve_ty(&typed_left.ty), self.resolve_ty(&rhs_ret)) {
            (Ty::Result(ok, err), Ty::Result(next_ok, next_err)) => {
                if !self.types_compatible(ok.as_ref(), &rhs_in)
                    || !self.types_compatible(err.as_ref(), next_err.as_ref())
                {
                    return Err(TypeError {
                        message: "`|>=` requires matching Result context on both sides".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Result(
                        Box::new(self.resolve_ty(next_ok.as_ref())),
                        Box::new(self.resolve_ty(err.as_ref())),
                    ),
                    span: span.clone(),
                    node: TypedInner::ResultBind(Box::new(typed_left), Box::new(typed_right)),
                })
            }
            (Ty::List(item), Ty::List(_)) => {
                if !self.types_compatible(item.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|>=` type mismatch: expected {}, got {}",
                            self.ty_name(item.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: None,
                    });
                }
                self.build_list_helper_call("List::flat_map", span, typed_left, typed_right)
            }
            (Ty::Result(_, _), Ty::List(_)) | (Ty::List(_), Ty::Result(_, _)) => Err(TypeError {
                message: "`|>=` cannot mix Result and List context".into(),
                span: span.clone(),
                hint: None,
            }),
            (other, _) => Err(TypeError {
                message: format!("`|>=` requires Result or List on the left, got {}", self.ty_name(&other)),
                span: typed_left.span.clone(),
                hint: None,
            }),
        }
    }

    fn check_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_compose_callable(left, "`>>`")?;
        let typed_right = self.check_compose_callable(right, "`>>`")?;
        let (left_in, left_out) = self.unary_function_parts(&typed_left.ty, "`>>`", &typed_left.span)?;
        let (right_in, right_out) = self.unary_function_parts(&typed_right.ty, "`>>`", &typed_right.span)?;
        if !self.types_compatible(&left_out, &right_in) {
            return Err(TypeError {
                message: "`>>` requires the left output type to match the right input type".into(),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(TypedNode {
            ty: Ty::Func(vec![left_in], Box::new(right_out)),
            span: span.clone(),
            node: TypedInner::Compose(
                ComposeFlavor::Plain,
                Box::new(typed_left),
                Box::new(typed_right),
            ),
        })
    }

    fn check_kleisli_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_compose_callable(left, "`|=>`")?;
        let typed_right = self.check_compose_callable(right, "`|=>`")?;
        let (left_in, left_out) = self.unary_function_parts(&typed_left.ty, "`|=>`", &typed_left.span)?;
        let (right_in, right_out) = self.unary_function_parts(&typed_right.ty, "`|=>`", &typed_right.span)?;
        match (self.resolve_ty(&left_out), self.resolve_ty(&right_out)) {
            (Ty::Result(ok, err), Ty::Result(next_ok, next_err)) => {
                if !self.types_compatible(ok.as_ref(), &right_in)
                    || !self.types_compatible(err.as_ref(), next_err.as_ref())
                {
                    return Err(TypeError {
                        message: "`|=>` requires matching Result context on both sides".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Func(
                        vec![left_in],
                        Box::new(Ty::Result(
                            Box::new(self.resolve_ty(next_ok.as_ref())),
                            Box::new(self.resolve_ty(err.as_ref())),
                        )),
                    ),
                    span: span.clone(),
                    node: TypedInner::Compose(
                        ComposeFlavor::ResultBind,
                        Box::new(typed_left),
                        Box::new(typed_right),
                    ),
                })
            }
            (Ty::List(item), Ty::List(next_item)) => {
                if !self.types_compatible(item.as_ref(), &right_in) {
                    return Err(TypeError {
                        message: "`|=>` requires matching List element types across both sides".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Func(
                        vec![left_in],
                        Box::new(Ty::List(Box::new(self.resolve_ty(next_item.as_ref())))),
                    ),
                    span: span.clone(),
                    node: TypedInner::Compose(
                        ComposeFlavor::ListBind {
                            helper: self.list_helper_ref_by_name("List::flat_map", span)?,
                        },
                        Box::new(typed_left),
                        Box::new(typed_right),
                    ),
                })
            }
            _ => Err(TypeError {
                message: "`|=>` requires matching Result or List context on both sides".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    fn is_top_level_list_pattern(pat: &ResolvedPattern) -> bool {
        matches!(
            pat,
            ResolvedPattern::ListNil(_) | ResolvedPattern::ListCons(_, _)
        )
    }

    fn contains_result_test_pattern(pat: &ResolvedPattern) -> bool {
        match pat {
            ResolvedPattern::Constructor(ctor, inners) => {
                ctor.name == "Ok"
                    || ctor.name == "Err"
                    || inners
                        .iter()
                        .any(|inner| Self::contains_result_test_pattern(inner))
            }
            ResolvedPattern::As(inner, _, _) => Self::contains_result_test_pattern(inner),
            ResolvedPattern::ListCons(head, tail) => {
                Self::contains_result_test_pattern(head) || Self::contains_result_test_pattern(tail)
            }
            ResolvedPattern::Var(_)
            | ResolvedPattern::Annotated(_, _)
            | ResolvedPattern::Wildcard(_)
            | ResolvedPattern::ListNil(_)
            | ResolvedPattern::IntLit(_, _)
            | ResolvedPattern::StrLit(_, _)
            | ResolvedPattern::BoolLit(_, _) => false,
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

    fn ast_ty_span(ast_ty: &AstTy) -> &Span {
        match ast_ty {
            AstTy::Named(span, _) | AstTy::Generic(span, _, _) | AstTy::Func(span, _, _) => span,
        }
    }

    fn collect_type_ref_names(ast_ty: &AstTy, out: &mut Vec<String>) {
        match ast_ty {
            AstTy::Named(_, name) => out.push(name.clone()),
            AstTy::Generic(_, _, args) => {
                for arg in args {
                    Self::collect_type_ref_names(arg, out);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_type_ref_names(param, out);
                }
                Self::collect_type_ref_names(ret, out);
            }
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
                            crate::env::TypeKind::Enum => Ok(Ty::Enum(def.name.clone())),
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
            AstTy::Generic(span, name, args) => match name.as_str() {
                "List" => {
                    if args.len() != 1 {
                        return Err(TypeError {
                            message: "List<T> requires exactly 1 type argument".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let inner_ty =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    Ok(Ty::List(Box::new(inner_ty)))
                }
                "Result" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(TypeError {
                            message: "Result<T> or Result<T, E> requires 1 or 2 type arguments"
                                .into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let ok =
                        self.resolve_ast_ty_in_context(&args[0], TypeSyntaxContext::General)?;
                    let err = if args.len() == 2 {
                        if context != TypeSyntaxContext::FunctionReturn {
                            return Err(TypeError {
                                message:
                                    "Result<T, E> is only allowed in function return signatures."
                                        .into(),
                                span: span.clone(),
                                hint: Some("Use Result<T> in local code.".into()),
                            });
                        }
                        self.resolve_ast_ty_in_context(&args[1], TypeSyntaxContext::ErrorMarker)?
                    } else {
                        Ty::Error
                    };
                    Ok(Ty::Result(Box::new(ok), Box::new(err)))
                }
                other => Err(TypeError {
                    message: format!("Unknown generic type: {}", other),
                    span: span.clone(),
                    hint: None,
                }),
            },
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
            AstTy::Generic(span, name, args) => match name.as_str() {
                "List" => {
                    if args.len() != 1 {
                        return Err(TypeError {
                            message: "List<T> requires exactly 1 type argument".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let inner_ty = self.resolve_builtin_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                    )?;
                    Ok(Ty::List(Box::new(inner_ty)))
                }
                "Result" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(TypeError {
                            message: "Result<T> or Result<T, E> requires 1 or 2 type arguments"
                                .into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let ok = self.resolve_builtin_ast_ty_in_context(
                        &args[0],
                        TypeSyntaxContext::General,
                        tyvars,
                    )?;
                    let err = if args.len() == 2 {
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
                            &args[1],
                            TypeSyntaxContext::ErrorMarker,
                            tyvars,
                        )?
                    } else {
                        Ty::Error
                    };
                    Ok(Ty::Result(Box::new(ok), Box::new(err)))
                }
                _ => self.resolve_ast_ty_in_context(ast_ty, context),
            },
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
                    message: "The error marker E in Result<T, E> must be a deferror-defined type."
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
            (Ty::Enum(n1), Ty::Enum(n2)) => n1 == n2,
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
            Ty::Enum(_) => false,
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
            Ty::Enum(name) => Ty::Enum(name.clone()),
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
                Ty::Enum(name) => Ty::Enum(name.clone()),
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
            Ty::List(inner) => format!("List<{}>", self.ty_name(inner)),
            Ty::Result(ok, _) => format!("Result<{}>", self.ty_name(ok)),
            Ty::Var(n) => format!("${}", n),
            Ty::Struct(name, _) | Ty::Record(name, _) | Ty::Enum(name) => name.clone(),
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
            TypedInner::InjectCall(func, args) => TypedInner::InjectCall(
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
            TypedInner::Pipe(left, right) => TypedInner::Pipe(
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::ResultMap(left, right) => TypedInner::ResultMap(
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::ResultBind(left, right) => TypedInner::ResultBind(
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::Compose(flavor, left, right) => TypedInner::Compose(
                flavor,
                Box::new(self.resolve_typed_node(*left)),
                Box::new(self.resolve_typed_node(*right)),
            ),
            TypedInner::ListNil => TypedInner::ListNil,
            TypedInner::ListCons(head, tail) => TypedInner::ListCons(
                Box::new(self.resolve_typed_node(*head)),
                Box::new(self.resolve_typed_node(*tail)),
            ),
            TypedInner::ListLiteral(elems) => TypedInner::ListLiteral(
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
            TypedInner::EnumDef(name, variants) => TypedInner::EnumDef(name, variants),
            TypedInner::Semi(inner) => TypedInner::Semi(Box::new(self.resolve_typed_node(*inner))),
        };

        TypedNode { ty, span, node }
    }

    fn resolve_typed_pattern(&self, pattern: TypedPattern) -> TypedPattern {
        match pattern {
            TypedPattern::Var(ty, id) => TypedPattern::Var(self.resolve_ty(&ty), id),
            TypedPattern::As(ty, inner, id) => TypedPattern::As(
                self.resolve_ty(&ty),
                Box::new(self.resolve_typed_pattern(*inner)),
                id,
            ),
            TypedPattern::Wildcard(ty) => TypedPattern::Wildcard(self.resolve_ty(&ty)),
            TypedPattern::ListNil(ty) => TypedPattern::ListNil(self.resolve_ty(&ty)),
            TypedPattern::ListCons(ty, head, tail) => TypedPattern::ListCons(
                self.resolve_ty(&ty),
                Box::new(self.resolve_typed_pattern(*head)),
                Box::new(self.resolve_typed_pattern(*tail)),
            ),
            TypedPattern::IntLit(ty, n) => TypedPattern::IntLit(self.resolve_ty(&ty), n),
            TypedPattern::StrLit(ty, s) => TypedPattern::StrLit(self.resolve_ty(&ty), s),
            TypedPattern::BoolLit(ty, b) => TypedPattern::BoolLit(self.resolve_ty(&ty), b),
            TypedPattern::ResultOk(ty, inner) => TypedPattern::ResultOk(
                self.resolve_ty(&ty),
                Box::new(self.resolve_typed_pattern(*inner)),
            ),
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
                let expected =
                    self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
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
            ResolvedPattern::As(inner, alias, alias_ty) => {
                let (typed_inner, inner_ty) = self.check_pattern(inner, rhs_ty, span)?;
                let alias_bind_ty = if let Some(ast_ty) = alias_ty {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
                    if !self.types_compatible(&expected, &inner_ty) {
                        return Err(TypeError {
                            message: format!(
                                "expected {}, got {}",
                                self.ty_name(&expected),
                                self.ty_name(&inner_ty)
                            ),
                            span: alias.span.clone(),
                            hint: None,
                        });
                    }
                    self.resolve_ty(&expected)
                } else {
                    self.resolve_ty(&inner_ty)
                };

                Ok((
                    TypedPattern::As(alias_bind_ty, Box::new(typed_inner), alias.clone()),
                    inner_ty,
                ))
            }
            ResolvedPattern::Wildcard(_wspan) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Wildcard(rhs_ty.clone()), rhs_ty))
            }
            ResolvedPattern::ListNil(pspan) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                match rhs_ty {
                    Ty::List(_) => Ok((TypedPattern::ListNil(rhs_ty.clone()), rhs_ty)),
                    other => Err(TypeError {
                        message: format!(
                            "empty list pattern requires List<...>, got {}",
                            self.ty_name(&other)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    }),
                }
            }
            ResolvedPattern::ListCons(head, tail) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                let elem_ty = match &rhs_ty {
                    Ty::List(inner) => inner.as_ref().clone(),
                    other => {
                        return Err(TypeError {
                            message: format!(
                                "list pattern requires List<...>, got {}",
                                self.ty_name(other)
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                let (typed_head, _) = self.check_pattern(head, &elem_ty, span)?;
                let tail_ty = Ty::List(Box::new(elem_ty.clone()));
                let (typed_tail, _) = self.check_pattern(tail, &tail_ty, span)?;
                Ok((
                    TypedPattern::ListCons(
                        rhs_ty.clone(),
                        Box::new(typed_head),
                        Box::new(typed_tail),
                    ),
                    rhs_ty,
                ))
            }
            ResolvedPattern::IntLit(pspan, n) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Int, &rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "integer literal pattern requires Int, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::IntLit(Ty::Int, n.clone()), rhs_ty))
            }
            ResolvedPattern::StrLit(pspan, s) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Str, &rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "string literal pattern requires String, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::StrLit(Ty::Str, s.clone()), rhs_ty))
            }
            ResolvedPattern::BoolLit(pspan, b) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Bool, &rhs_ty) {
                    return Err(TypeError {
                        message: format!(
                            "boolean literal pattern requires Boolean, got {}",
                            self.ty_name(&rhs_ty)
                        ),
                        span: pspan.clone(),
                        hint: None,
                    });
                }
                Ok((TypedPattern::BoolLit(Ty::Bool, *b), rhs_ty))
            }
            ResolvedPattern::Constructor(ctor_id, inners) => {
                if ctor_id.name != "Ok" {
                    return Err(TypeError {
                        message: format!(
                            "SafeBind constructor pattern only supports Ok(...), got {}(...)",
                            ctor_id.name
                        ),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }

                let rhs_ty = self.resolve_ty(rhs_ty);
                let ok_ty = match &rhs_ty {
                    Ty::Result(ok, _) => ok.as_ref().clone(),
                    other => {
                        return Err(TypeError {
                            message: format!(
                                "`Ok(...)` pattern requires Result<...>, got {}",
                                self.ty_name(other)
                            ),
                            span: ctor_id.span.clone(),
                            hint: Some(
                                "Use `num =? expr` directly for Result<T>, and only add `Ok(...)` on the left for nested Result values.".into(),
                            ),
                        });
                    }
                };

                if inners.len() != 1 {
                    return Err(TypeError {
                        message: "SafeBind Ok(...) pattern requires exactly one inner pattern"
                            .into(),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }

                let (typed_inner, _) = self.check_pattern(&inners[0], &ok_ty, span)?;
                Ok((
                    TypedPattern::ResultOk(rhs_ty.clone(), Box::new(typed_inner)),
                    rhs_ty,
                ))
            }
        }
    }

    fn bind_typed_pattern(&mut self, pat: &TypedPattern, rhs_ty: &Ty) {
        let rhs_ty = self.resolve_ty(rhs_ty);
        match pat {
            TypedPattern::Var(_, id) => {
                self.env.bind_var(id.unique_id, rhs_ty.clone());
            }
            TypedPattern::As(alias_ty, inner, id) => {
                self.env.bind_var(id.unique_id, self.resolve_ty(alias_ty));
                self.bind_typed_pattern(inner, &rhs_ty);
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _) => {}
            TypedPattern::ListCons(_, head, tail) => {
                let elem_ty = match &rhs_ty {
                    Ty::List(inner) => inner.as_ref().clone(),
                    _ => return,
                };
                self.bind_typed_pattern(head, &elem_ty);
                let tail_ty = Ty::List(Box::new(elem_ty));
                self.bind_typed_pattern(tail, &tail_ty);
            }
            TypedPattern::ResultOk(_, inner) => {
                let ok_ty = match &rhs_ty {
                    Ty::Result(ok, _) => ok.as_ref().clone(),
                    _ => return,
                };
                self.bind_typed_pattern(inner, &ok_ty);
            }
        }
    }

    fn normalize_env_bindings(&mut self) {
        let keys = self.env.vars.keys().copied().collect::<Vec<_>>();
        for key in keys {
            if let Some(ty) = self.env.vars.get(&key).cloned() {
                self.env.vars.insert(key, self.resolve_ty(&ty));
            }
        }
    }

    fn collect_pattern_result_error_types(&self, pat: &TypedPattern, out: &mut Vec<Ty>) {
        match pat {
            TypedPattern::ResultOk(ty, inner) => {
                if let Ty::Result(_, err) = self.resolve_ty(ty) {
                    out.push(err.as_ref().clone());
                }
                self.collect_pattern_result_error_types(inner, out);
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.collect_pattern_result_error_types(head, out);
                self.collect_pattern_result_error_types(tail, out);
            }
            TypedPattern::As(_, inner, _) => {
                self.collect_pattern_result_error_types(inner, out);
            }
            TypedPattern::Var(_, _)
            | TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _) => {}
        }
    }

    // ── Function application ──

    fn typecheck_user_function_args(
        &mut self,
        span: &Span,
        callee_uid: u32,
        params: &[Ty],
        args: &[ResolvedRecordLitArg],
    ) -> Result<Vec<TypedNode>, TypeError> {
        let has_named = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
        let has_positional = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Positional(_)));
        if has_named && has_positional {
            return Err(TypeError {
                message: "Cannot mix positional and named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let param_names = self.user_func_params.get(&callee_uid).cloned();
        let mut typed_args = Vec::with_capacity(params.len());

        if has_named {
            let names = param_names.as_ref().ok_or_else(|| TypeError {
                message: "This function value does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            })?;

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

            let mut reordered: Vec<Option<&Resolved>> = vec![None; params.len()];
            for arg in args {
                let ResolvedRecordLitArg::Named(name, expr) = arg else {
                    unreachable!("validated argument form above")
                };
                let idx = names
                    .iter()
                    .position(|n| n == name)
                    .ok_or_else(|| TypeError {
                        message: format!("Unknown argument name '{}' for function", name),
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

            for (idx, expected_ty) in params.iter().enumerate() {
                let expr = reordered[idx].ok_or_else(|| TypeError {
                    message: format!("Missing argument '{}'", names[idx]),
                    span: span.clone(),
                    hint: None,
                })?;
                let typed = self.check_node(expr)?;
                if !self.types_compatible(expected_ty, &typed.ty) {
                    return Err(TypeError {
                        message: format!(
                            "Argument type mismatch: expected {}, got {}",
                            self.ty_name(expected_ty),
                            self.ty_name(&typed.ty)
                        ),
                        span: typed.span.clone(),
                        hint: None,
                    });
                }
                typed_args.push(typed);
            }
            return Ok(typed_args);
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

        for (expected_ty, arg) in params.iter().zip(args) {
            let ResolvedRecordLitArg::Positional(expr) = arg else {
                unreachable!("validated argument form above")
            };
            let typed = self.check_node(expr)?;
            if !self.types_compatible(expected_ty, &typed.ty) {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(expected_ty),
                        self.ty_name(&typed.ty)
                    ),
                    span: typed.span.clone(),
                    hint: None,
                });
            }
            typed_args.push(typed);
        }

        Ok(typed_args)
    }

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

                if name == "set_exit_code" {
                    match self.source_rules.set_exit_code_policy {
                        SetExitCodePolicy::Anywhere => {}
                        SetExitCodePolicy::Forbidden => {
                            return Err(TypeError {
                                message: format!(
                                    "set_exit_code is forbidden by source policy ({})",
                                    self.source_rules.set_exit_code_policy.as_str()
                                ),
                                span: span.clone(),
                                hint: Some(
                                    "This source kind does not allow set_exit_code. Use Result-based failure handling instead."
                                        .into(),
                                ),
                            });
                        }
                        SetExitCodePolicy::EntryOnly => {
                            let Some(entrypoint) = self.source_rules.normalized_entrypoint.as_ref()
                            else {
                                return Err(TypeError {
                                    message:
                                        "set_exit_code requires a normalized entrypoint but none was provided".into(),
                                    span: span.clone(),
                                    hint: Some(
                                        "Configure an entrypoint, or avoid set_exit_code in this compile unit."
                                            .into(),
                                    ),
                                });
                            };
                            if self.current_function_symbol.as_deref() != Some(entrypoint.as_str())
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "set_exit_code is only allowed inside entrypoint `{}` (policy: {})",
                                        entrypoint,
                                        self.source_rules.set_exit_code_policy.as_str()
                                    ),
                                    span: span.clone(),
                                    hint: Some(
                                        "Move set_exit_code into the configured entrypoint function."
                                            .into(),
                                    ),
                                });
                            }
                        }
                    }
                }

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::UserFunc { params, ret, .. } => {
                let has_named = args
                    .iter()
                    .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
                let callee_uid = match func {
                    Resolved::Var(_, id) => id.unique_id,
                    _ if !has_named => u32::MAX,
                    _ => {
                        return Err(TypeError {
                            message: "This function value does not accept named arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                let typed_args =
                    self.typecheck_user_function_args(span, callee_uid, params, args)?;

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
        let mut body_checker = self.spawn_child_checker(self.env.clone());
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
            None => params
                .iter()
                .map(|param| match &param.ty {
                    Some(ast_ty) => {
                        body_checker.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)
                    }
                    None => Ok(body_checker.env.fresh_tyvar()),
                })
                .collect::<Result<Vec<_>, _>>()?,
        };

        for (param, param_ty) in params.iter().zip(param_tys.iter()) {
            let param_ty = if let Some(ast_ty) = &param.ty {
                let annotated =
                    body_checker.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
                if !body_checker.types_compatible(param_ty, &annotated) {
                    return Err(TypeError {
                        message: format!(
                            "closure parameter `{}` expected {}, got {}",
                            param.id.name,
                            body_checker.ty_name(param_ty),
                            body_checker.ty_name(&annotated)
                        ),
                        span: param.id.span.clone(),
                        hint: None,
                    });
                }
                body_checker.resolve_ty(&annotated)
            } else {
                body_checker.resolve_ty(param_ty)
            };
            body_checker
                .env
                .bind_var(param.id.unique_id, param_ty.clone());
            typed_params.push(TypedClosureParam {
                id: param.id.clone(),
                ty: param_ty,
            });
        }

        for capture in captures {
            if let Some(ty) = self.env.lookup_var(capture.unique_id).cloned() {
                body_checker
                    .env
                    .bind_var(capture.unique_id, body_checker.resolve_ty(&ty));
            }
        }

        if let Some(Ty::Func(_, expected_ret)) = expected {
            body_checker.function_return_ty = Some(expected_ret.as_ref().clone());
        }
        let typed_body = body_checker.check_node(body)?;
        let typed_body = body_checker.resolve_typed_node(typed_body);
        self.absorb_child_progress(&body_checker);

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
                (Ty::Int, Ty::Int)
                | (Ty::Str, Ty::Str)
                | (Ty::Bool, Ty::Bool)
                | (Ty::Enum(_), Ty::Enum(_)) => {
                    if self.types_compatible(&lt, &rt) {
                        Ok(Ty::Bool)
                    } else {
                        Err(TypeError {
                            message: format!(
                                "Cannot compare {} and {}",
                                self.ty_name(&lt),
                                self.ty_name(&rt)
                            ),
                            span: span.clone(),
                            hint: None,
                        })
                    }
                }
                _ => {
                    // Equality checks are intentionally restricted for now.
                    // Probe comparability without committing substitutions on failure.
                    let before = self.substitutions.clone();
                    let comparable = self.types_compatible(&lt, &rt);
                    self.substitutions = before;
                    if !comparable {
                        Err(TypeError {
                            message: format!(
                                "Cannot compare {} and {}",
                                self.ty_name(&lt),
                                self.ty_name(&rt)
                            ),
                            span: span.clone(),
                            hint: None,
                        })
                    } else {
                        Err(TypeError {
                            message: format!(
                                "== / != not supported for {} in phase 1",
                                self.ty_name(&lt)
                            ),
                            span: span.clone(),
                            hint: None,
                        })
                    }
                }
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

    fn check_list_nil(&mut self, span: &Span) -> Result<TypedNode, TypeError> {
        let tv = self.env.fresh_tyvar();
        Ok(TypedNode {
            ty: Ty::List(Box::new(tv)),
            span: span.clone(),
            node: TypedInner::ListNil,
        })
    }

    fn check_list_cons(
        &mut self,
        span: &Span,
        head: &Resolved,
        tail: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_head = self.check_node(head)?;
        let typed_tail = self.check_node(tail)?;
        let tail_elem_ty = match &typed_tail.ty {
            Ty::List(inner) => inner.as_ref().clone(),
            other => {
                return Err(TypeError {
                    message: format!("list tail must be List<...>, got {}", self.ty_name(other)),
                    span: typed_tail.span.clone(),
                    hint: Some("Use `[head, ..tail]` with a list tail value".into()),
                });
            }
        };

        if !self.types_compatible(&typed_head.ty, &tail_elem_ty) {
            return Err(TypeError {
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(&tail_elem_ty),
                    self.ty_name(&typed_head.ty)
                ),
                span: typed_head.span.clone(),
                hint: Some("List head and tail element types must match".into()),
            });
        }

        let elem_ty = self.resolve_ty(&tail_elem_ty);
        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty.clone())),
            span: span.clone(),
            node: TypedInner::ListCons(Box::new(typed_head), Box::new(typed_tail)),
        })
    }

    fn check_list_literal(
        &mut self,
        span: &Span,
        elems: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        if elems.is_empty() {
            return self.check_list_nil(span);
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
            node: TypedInner::ListLiteral(typed_elems),
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
        arms: &[(ResolvedPattern, Resolved)],
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
            self.normalize_env_bindings();
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
        if arms.iter().any(|(pat, _)| self.is_match_catch_all(pat)) {
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
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::Constructor { tag: 0, .. }));
                let has_err = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::Constructor { tag: 1, .. }));

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
            Ty::Enum(enum_name) => {
                let variants = self
                    .env
                    .enum_variants_of(enum_name)
                    .cloned()
                    .unwrap_or_default();
                let mut missing = Vec::new();
                for variant in variants {
                    let covered = arms.iter().any(|(pat, _)| {
                        matches!(
                            pat,
                            TypedMatchPattern::Constructor { tag, .. } if *tag == variant.tag
                        )
                    });
                    if !covered {
                        missing.push(variant.short_name);
                    }
                }
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(TypeError {
                        message: format!("Non-exhaustive match. Missing: {}", missing.join(", ")),
                        span: span.clone(),
                        hint: None,
                    })
                }
            }
            Ty::List(_) => {
                let has_nil = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::ListNil));
                let has_cons = arms
                    .iter()
                    .any(|(pat, _)| matches!(pat, TypedMatchPattern::ListCons(_, _)));
                if has_nil && has_cons {
                    Ok(())
                } else {
                    let mut missing = Vec::new();
                    if !has_nil {
                        missing.push("[]");
                    }
                    if !has_cons {
                        missing.push("[head, ..tail]");
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
        pat: &ResolvedPattern,
        body: &Resolved,
        scrut_ty: &Ty,
        span: &Span,
    ) -> Result<(TypedMatchPattern, TypedNode), TypeError> {
        let mut arm_checker = self.spawn_child_checker(self.env.clone());
        let typed_pat = arm_checker.check_match_subpattern(pat, scrut_ty)?;
        let typed_body = arm_checker.check_node(body)?;
        arm_checker.normalize_env_bindings();
        let typed_body = arm_checker.resolve_typed_node(typed_body);
        self.absorb_child_progress(&arm_checker);
        Ok((typed_pat, typed_body))
    }

    fn check_match_subpattern(
        &mut self,
        pat: &ResolvedPattern,
        expected_ty: &Ty,
    ) -> Result<TypedMatchPattern, TypeError> {
        match pat {
            ResolvedPattern::Var(id) => {
                self.env
                    .bind_var(id.unique_id, self.resolve_ty(expected_ty));
                Ok(TypedMatchPattern::Binding(id.clone()))
            }
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected =
                    self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
                if !self.types_compatible(&expected, expected_ty) {
                    return Err(TypeError {
                        message: format!(
                            "expected {}, got {}",
                            self.ty_name(&expected),
                            self.ty_name(expected_ty)
                        ),
                        span: id.span.clone(),
                        hint: None,
                    });
                }
                let bind_ty = self.resolve_ty(&expected);
                self.env.bind_var(id.unique_id, bind_ty);
                Ok(TypedMatchPattern::Binding(id.clone()))
            }
            ResolvedPattern::As(inner, alias, alias_ty) => {
                let typed_inner = self.check_match_subpattern(inner, expected_ty)?;
                let alias_bind_ty = if let Some(ast_ty) = alias_ty {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
                    if !self.types_compatible(&expected, expected_ty) {
                        return Err(TypeError {
                            message: format!(
                                "expected {}, got {}",
                                self.ty_name(&expected),
                                self.ty_name(expected_ty)
                            ),
                            span: alias.span.clone(),
                            hint: None,
                        });
                    }
                    self.resolve_ty(&expected)
                } else {
                    self.resolve_ty(expected_ty)
                };
                self.env.bind_var(alias.unique_id, alias_bind_ty);
                Ok(TypedMatchPattern::As(Box::new(typed_inner), alias.clone()))
            }
            ResolvedPattern::Wildcard(_) => Ok(TypedMatchPattern::Wildcard),
            ResolvedPattern::BoolLit(span, b) => {
                if !self.types_compatible(&Ty::Bool, expected_ty) {
                    return Err(TypeError {
                        message: "Boolean pattern on non-Boolean scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::BoolLit(*b))
            }
            ResolvedPattern::IntLit(span, n) => {
                if !self.types_compatible(&Ty::Int, expected_ty) {
                    return Err(TypeError {
                        message: "Int pattern on non-Int scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::IntLit(n.clone()))
            }
            ResolvedPattern::StrLit(span, s) => {
                if !self.types_compatible(&Ty::Str, expected_ty) {
                    return Err(TypeError {
                        message: "String pattern on non-String scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::StrLit(s.clone()))
            }
            ResolvedPattern::Constructor(ctor_id, inner_pats) => {
                if matches!(expected_ty, Ty::Result(_, _)) {
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
                    if inner_pats.len() != 1 {
                        return Err(TypeError {
                            message: format!(
                                "{}(...) match pattern requires exactly one argument",
                                ctor_id.name
                            ),
                            span: ctor_id.span.clone(),
                            hint: None,
                        });
                    }
                    let inner_ty = match (tag, expected_ty) {
                        (0, Ty::Result(ok, _)) => ok.as_ref().clone(),
                        (1, Ty::Result(_, err)) => err.as_ref().clone(),
                        _ => unreachable!(),
                    };
                    let typed_inner = self.check_match_subpattern(&inner_pats[0], &inner_ty)?;
                    return Ok(TypedMatchPattern::Constructor {
                        tag,
                        fields: vec![typed_inner],
                        field_offset: 0,
                    });
                }

                let Ty::Enum(expected_enum_name) = expected_ty else {
                    return Err(TypeError {
                        message: "Constructor pattern on non-enum/non-Result scrutinee".into(),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                };
                let variant = self
                    .env
                    .enum_variant_by_constructor_id(ctor_id.unique_id)
                    .ok_or_else(|| TypeError {
                        message: format!("Unknown constructor: {}", ctor_id.name),
                        span: ctor_id.span.clone(),
                        hint: None,
                    })?
                    .clone();
                if &variant.enum_name != expected_enum_name {
                    return Err(TypeError {
                        message: format!(
                            "Constructor {} does not belong to enum {}",
                            ctor_id.name, expected_enum_name
                        ),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }
                if inner_pats.len() != variant.payload.len() {
                    return Err(TypeError {
                        message: format!(
                            "{} pattern expects {} argument(s), got {}",
                            ctor_id.name,
                            variant.payload.len(),
                            inner_pats.len()
                        ),
                        span: ctor_id.span.clone(),
                        hint: None,
                    });
                }
                let mut typed_fields = Vec::new();
                for (pat, field_ty) in inner_pats.iter().zip(variant.payload.iter()) {
                    typed_fields.push(self.check_match_subpattern(pat, field_ty)?);
                }
                Ok(TypedMatchPattern::Constructor {
                    tag: variant.tag,
                    fields: typed_fields,
                    field_offset: 1,
                })
            }
            ResolvedPattern::ListNil(span) => {
                if !matches!(expected_ty, Ty::List(_)) {
                    return Err(TypeError {
                        message: "empty list pattern on non-List scrutinee".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedMatchPattern::ListNil)
            }
            ResolvedPattern::ListCons(head, tail) => {
                let elem_ty = match expected_ty {
                    Ty::List(inner) => inner.as_ref().clone(),
                    other => {
                        return Err(TypeError {
                            message: format!(
                                "list pattern requires List<...>, got {}",
                                self.ty_name(other)
                            ),
                            span: Span { start: 0, end: 0 },
                            hint: None,
                        });
                    }
                };
                let typed_head = self.check_match_subpattern(head, &elem_ty)?;
                let tail_ty = Ty::List(Box::new(elem_ty));
                let typed_tail = self.check_match_subpattern(tail, &tail_ty)?;
                Ok(TypedMatchPattern::ListCons(
                    Box::new(typed_head),
                    Box::new(typed_tail),
                ))
            }
        }
    }

    fn is_match_catch_all(&self, pat: &TypedMatchPattern) -> bool {
        match pat {
            TypedMatchPattern::Binding(_) | TypedMatchPattern::Wildcard => true,
            TypedMatchPattern::As(inner, _) => self.is_match_catch_all(inner),
            TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::Constructor { .. }
            | TypedMatchPattern::ListNil
            | TypedMatchPattern::ListCons(_, _) => false,
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
        if Self::is_special_form_builtin_decl_name(&id.name) {
            return self.check_special_form_builtin_decl(span, id, params, ret_ty);
        }

        let meta = builtin_meta_by_name(&id.name).ok_or_else(|| TypeError {
            message: format!("Unknown builtin declaration: {}", id.name),
            span: span.clone(),
            hint: None,
        })?;
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

    fn check_special_form_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedFunParam],
        ret_ty: &Option<AstTy>,
    ) -> Result<TypedNode, TypeError> {
        let expected_qname = match id.name.as_str() {
            "if" => "Kernel::if",
            "if_then" => "Kernel::if_then",
            _ => unreachable!(),
        };

        if id.qualified_name.as_deref() != Some(expected_qname) {
            return Err(TypeError {
                message: format!(
                    "Special-form declaration `{}` is only allowed in std module `Kernel`.",
                    id.name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let shape_ok = match id.name.as_str() {
            "if" => {
                params.len() == 3
                    && Self::is_named_type(&params[0].ty, "Boolean")
                    && Self::is_zero_arg_func_to_named(&params[1].ty, "$A")
                    && Self::is_zero_arg_func_to_named(&params[2].ty, "$A")
                    && ret_ty
                        .as_ref()
                        .is_some_and(|ty| Self::is_named_type(ty, "$A"))
            }
            "if_then" => {
                params.len() == 2
                    && Self::is_named_type(&params[0].ty, "Boolean")
                    && Self::is_zero_arg_func_to_unit(&params[1].ty)
                    && ret_ty.as_ref().is_some_and(Self::is_unit_type)
            }
            _ => false,
        };

        if !shape_ok {
            let expected = match id.name.as_str() {
                "if" => "@@builtin def if(cond: Boolean, then_branch: (-> $A), else_branch: (-> $A)) -> $A",
                "if_then" => "@@builtin def if_then(cond: Boolean, then_branch: (-> ())) -> ()",
                _ => unreachable!(),
            };
            return Err(TypeError {
                message: format!(
                    "Special-form declaration must match the canonical contract: {}",
                    expected
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

    fn check_builtin_type_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[String],
        _attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let Some(meta) = builtin_type_meta_by_name(&id.name) else {
            return Err(TypeError {
                message: format!("Unknown builtin type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            });
        };

        let exact_params_match = params.len() == meta.params.len()
            && params
                .iter()
                .zip(meta.params.iter())
                .all(|(actual, expected)| actual == expected);
        if !exact_params_match {
            return Err(TypeError {
                message: format!(
                    "Builtin type {} must be declared as {}{}",
                    id.name,
                    id.name,
                    format_builtin_type_param_suffix(meta.params)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        if self.enforce_builtin_type_contracts {
            if let Some((_, first_span)) = self.seen_builtin_type_decls.get(&id.name) {
                return Err(TypeError {
                    message: format!("Duplicate builtin type declaration: {}", id.name),
                    span: span.clone(),
                    hint: Some(format!(
                        "Already declared at {}..{}",
                        first_span.start, first_span.end
                    )),
                });
            }
            self.seen_builtin_type_decls
                .insert(id.name.clone(), (params.to_vec(), span.clone()));
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Unit),
        })
    }

    fn check_result_ctor_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        param_ty: &AstTy,
        ret_ty: &AstTy,
        _attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        // `Ok` / `Err` are intentionally specified through a declaration-only
        // contract instead of a normal function body. By checking the exact
        // source-level shape here, we keep the compiler honest about the
        // standard-library contract while still letting the runtime own the
        // actual constructor behavior.
        let expected_qname = match id.name.as_str() {
            "Ok" => "Result::Ok",
            "Err" => "Result::Err",
            other => {
                return Err(TypeError {
                    message: format!(
                        "Unknown Result constructor declaration: {}. Only Ok and Err are supported.",
                        other
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
        };

        if id.qualified_name.as_deref() != Some(expected_qname) {
            return Err(TypeError {
                message: format!(
                    "Result constructor declaration `{}` is only allowed in std module `Result`.",
                    id.name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let shape_ok = match id.name.as_str() {
            "Ok" => Self::is_named_type(param_ty, "$T") && Self::is_result_t_of_t(ret_ty),
            "Err" => Self::is_named_type(param_ty, "Error") && Self::is_result_t_of_t(ret_ty),
            _ => false,
        };

        if !shape_ok {
            let expected = match id.name.as_str() {
                "Ok" => "@@builtin type Ok($T) -> Result<$T>",
                "Err" => "@@builtin type Err(Error) -> Result<$T>",
                _ => unreachable!(),
            };
            return Err(TypeError {
                message: format!(
                    "Result constructor declaration must match the canonical contract: {}",
                    expected
                ),
                span: span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Unit),
        })
    }

    fn is_named_type(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(ast_ty, AstTy::Named(_, name) if name == expected_name)
    }

    fn is_unit_type(ast_ty: &AstTy) -> bool {
        Self::is_named_type(ast_ty, "Unit")
    }

    fn is_zero_arg_func_to_named(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(
            ast_ty,
            AstTy::Func(_, params, ret)
                if params.is_empty()
                    && matches!(ret.as_ref(), AstTy::Named(_, name) if name == expected_name)
        )
    }

    fn is_zero_arg_func_to_unit(ast_ty: &AstTy) -> bool {
        Self::is_zero_arg_func_to_named(ast_ty, "Unit")
    }

    fn is_special_form_builtin_decl_name(name: &str) -> bool {
        matches!(name, "if" | "if_then")
    }

    fn is_result_t_of_t(ast_ty: &AstTy) -> bool {
        matches!(
            ast_ty,
            AstTy::Generic(_, name, args)
                if name == "Result"
                    && args.len() == 1
                    && matches!(&args[0], AstTy::Named(_, param_name) if param_name == "$T")
        )
    }

    fn ensure_builtin_type_contracts(&self) -> Result<(), TypeError> {
        if !self.enforce_builtin_type_contracts {
            return Ok(());
        }

        for meta in BUILTIN_TYPE_METAS {
            if !self.seen_builtin_type_decls.contains_key(meta.name) {
                return Err(TypeError {
                    message: format!(
                        "Missing builtin type declaration: {}{}",
                        meta.name,
                        format_builtin_type_param_suffix(meta.params)
                    ),
                    span: Span { start: 0, end: 0 },
                    hint: None,
                });
            }
        }

        Ok(())
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

        let current_symbol = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        let is_entrypoint = self
            .source_rules
            .normalized_entrypoint
            .as_deref()
            .is_some_and(|entry| entry == current_symbol);
        if is_entrypoint {
            if !params.is_empty() {
                return Err(TypeError {
                    message: format!(
                        "entrypoint `{}` must have signature () -> Result<()>",
                        current_symbol
                    ),
                    span: span.clone(),
                    hint: Some("Remove entrypoint parameters and return Result<()>.".into()),
                });
            }
            if !Self::is_main_result_unit_ty(&expected_ret) {
                let legacy_main = current_symbol == "main"
                    && self
                        .source_rules
                        .normalized_entrypoint
                        .as_deref()
                        .is_some_and(|entry| entry == "main");
                return Err(TypeError {
                    message: if legacy_main {
                        "main must declare return type Result<()>".into()
                    } else {
                        format!(
                            "entrypoint `{}` must declare return type Result<()>",
                            current_symbol
                        )
                    },
                    span: span.clone(),
                    hint: Some(
                        "Define entrypoint as `def <name>() -> Result<()> { ... }` and return Ok(()) or Err(error)."
                            .into(),
                    ),
                });
            }
        }

        let mut body_checker = self.spawn_child_checker(fun_env);
        if let Some((impl_target, _method)) = Self::split_impl_method_name(&id.name) {
            if self
                .env
                .lookup_type_def(&impl_target)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
            {
                body_checker.current_impl_struct_target = Some(impl_target);
            }
        }
        body_checker.function_return_ty = Some(expected_ret.clone());
        body_checker.current_function_symbol = Some(current_symbol);
        let typed_body = body_checker.check_node(body)?;
        let typed_body = body_checker.resolve_typed_node(typed_body);
        self.absorb_child_progress(&body_checker);

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

    fn is_main_result_unit_ty(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::Result(ok, err)
                if matches!(ok.as_ref(), Ty::Unit) && matches!(err.as_ref(), Ty::Error)
        )
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

        let tag = self
            .env
            .resolve_type_def_signature(&id.name, ty_fields.clone())
            .ok_or_else(|| TypeError {
                message: format!("Unknown struct type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

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

    fn check_enum_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        variants: &[ResolvedEnumVariant],
    ) -> Result<TypedNode, TypeError> {
        let enum_variants = self
            .env
            .enum_variants_of(&id.name)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Unknown enum type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        if enum_variants.len() != variants.len() {
            return Err(TypeError {
                message: format!("Enum variant metadata mismatch: {}", id.name),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_variants = enum_variants
            .into_iter()
            .map(|variant| TypedEnumVariantDef {
                tag: variant.tag,
                constructor_name: variant.constructor_name,
                field_names: variant
                    .payload
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| format!("_{}", idx))
                    .collect(),
            })
            .collect::<Vec<_>>();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::EnumDef(id.name.clone(), typed_variants),
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

        let tag = self
            .env
            .resolve_type_def_signature(&id.name, ty_fields.clone())
            .ok_or_else(|| TypeError {
                message: format!("Unknown record type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

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

        if self.current_impl_struct_target.as_deref() != Some(id.name.as_str()) {
            return Err(TypeError {
                message: format!(
                    "Struct literal `{}` is only allowed inside `impl {} {{ ... }}` method bodies",
                    id.name, id.name
                ),
                span: span.clone(),
                hint: Some(format!(
                    "Construct `{}` values via `{}(...)` / `{}::new(...)` outside the impl body.",
                    id.name, id.name, id.name
                )),
            });
        }

        let tag = def.tag;

        // Reject unknown/duplicate fields before type-checking values.
        let mut seen = HashSet::new();
        for (name, _value) in field_vals {
            if !def.fields.iter().any(|(field_name, _)| field_name == name) {
                return Err(TypeError {
                    message: format!("Unknown field '{}' in {}", name, id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
            if !seen.insert(name.clone()) {
                return Err(TypeError {
                    message: format!("Duplicate field '{}' in {}", name, id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        }

        // Check fields match definition order and types.
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
                Ty::BuiltinFunc { params, ret, .. } => {
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
                Ty::UserFunc { params, ret, .. } => {
                    let typed_args =
                        self.typecheck_user_function_args(span, id.unique_id, params, args)?;
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
                Ty::Func(params, ret) => {
                    if args
                        .iter()
                        .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
                    {
                        return Err(TypeError {
                            message: "Function calls do not accept named arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
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
                    for (expected_ty, arg) in params.iter().zip(args) {
                        let ResolvedRecordLitArg::Positional(expr) = arg else {
                            unreachable!("validated argument form above")
                        };
                        let typed = self.check_node(expr)?;
                        if !self.types_compatible(expected_ty, &typed.ty) {
                            return Err(TypeError {
                                message: format!(
                                    "Argument type mismatch: expected {}, got {}",
                                    self.ty_name(expected_ty),
                                    self.ty_name(&typed.ty)
                                ),
                                span: typed.span.clone(),
                                hint: None,
                            });
                        }
                        typed_args.push(typed);
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
                (1u32, Ty::Result(Box::new(ok_var), Box::new(Ty::Error)))
            };
            return Ok(TypedNode {
                ty: result_ty,
                span: span.clone(),
                node: TypedInner::ConstructorCall(tag, vec![inner]),
            });
        }

        if let Some(variant) = self
            .env
            .enum_variant_by_constructor_id(id.unique_id)
            .cloned()
        {
            if args.len() != variant.payload.len() {
                return Err(TypeError {
                    message: format!(
                        "{} expects {} argument(s), got {}",
                        id.name,
                        variant.payload.len(),
                        args.len()
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
            let mut payload_values = Vec::new();
            for (idx, arg) in args.iter().enumerate() {
                let expected = &variant.payload[idx];
                let typed = match arg {
                    ResolvedRecordLitArg::Positional(expr) => self.check_node(expr)?,
                    ResolvedRecordLitArg::Named(_, _) => {
                        return Err(TypeError {
                            message: "Enum constructors do not accept named arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                if !self.types_compatible(expected, &typed.ty) {
                    return Err(TypeError {
                        message: format!(
                            "Argument type mismatch: expected {}, got {}",
                            self.ty_name(expected),
                            self.ty_name(&typed.ty)
                        ),
                        span: typed.span.clone(),
                        hint: None,
                    });
                }
                payload_values.push(typed);
            }

            let mut fields = Vec::with_capacity(payload_values.len() + 1);
            fields.push(TypedNode {
                ty: Ty::Int,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Int(variant.discriminant)),
            });
            fields.extend(payload_values);

            return Ok(TypedNode {
                ty: Ty::Enum(variant.enum_name),
                span: span.clone(),
                node: TypedInner::ConstructorCall(variant.tag, fields),
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

        if matches!(def.kind, crate::env::TypeKind::Struct) {
            let new_name = format!("{}::new", id.name);
            let Some(new_uid) = self.impl_method_uids.get(&new_name).copied() else {
                return Err(TypeError {
                    message: format!(
                        "Struct `{}` constructor call requires `{}` but no such method was found",
                        id.name, new_name
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "Define `impl {} {{ def new(...) -> Self {{ ... }} }}`.",
                        id.name
                    )),
                });
            };
            let new_ty = self
                .env
                .lookup_var(new_uid)
                .cloned()
                .ok_or_else(|| TypeError {
                    message: format!("Undefined function: {}", new_name),
                    span: span.clone(),
                    hint: None,
                })?;
            let (params, ret_ty) = match new_ty.clone() {
                Ty::UserFunc { params, ret, .. }
                | Ty::BuiltinFunc { params, ret, .. }
                | Ty::Func(params, ret) => (params, *ret),
                other => {
                    return Err(TypeError {
                        message: format!(
                            "`{}` is not callable (got {})",
                            new_name,
                            self.ty_name(&other)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            };

            let typed_args = self.typecheck_user_function_args(span, new_uid, &params, args)?;
            let expected_self_ty = Ty::Struct(id.name.clone(), def.fields.clone());
            if !self.types_compatible(&expected_self_ty, &ret_ty) {
                return Err(TypeError {
                    message: format!(
                        "`{}` must return Self ({}), got {}",
                        new_name,
                        self.ty_name(&expected_self_ty),
                        self.ty_name(&ret_ty)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }

            return Ok(TypedNode {
                ty: ret_ty.clone(),
                span: span.clone(),
                node: TypedInner::App(
                    Box::new(TypedNode {
                        ty: new_ty,
                        span: id.span.clone(),
                        node: TypedInner::Var(ResolvedId {
                            name: new_name,
                            qualified_name: None,
                            unique_id: new_uid,
                            span: id.span.clone(),
                        }),
                    }),
                    typed_args,
                ),
            });
        }

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
            let mut seen = HashSet::new();
            for arg in args {
                if let ResolvedRecordLitArg::Named(name, expr) = arg {
                    if !seen.insert(name.clone()) {
                        return Err(TypeError {
                            message: format!("Duplicate field '{}' in {}", name, id.name),
                            span: span.clone(),
                            hint: None,
                        });
                    }
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
            crate::env::TypeKind::Struct | crate::env::TypeKind::Enum => {
                unreachable!("validated above")
            }
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

        let tag = self
            .env
            .resolve_type_def_signature(
                &id.name,
                ty_fields
                    .iter()
                    .map(|(ty, rid)| (rid.name.clone(), ty.clone()))
                    .collect(),
            )
            .ok_or_else(|| TypeError {
                message: format!("Unknown error type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

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
        let mut show_checker = self.spawn_child_checker(show_env);
        show_checker.function_return_ty = Some(Ty::Str);
        let typed_show = show_checker
            .check_node(show_expr)
            .map_err(|err| TypeError {
                message: err.message,
                span: err.span,
                hint: err.hint,
            })?;
        let typed_show = show_checker.resolve_typed_node(typed_show);
        self.absorb_child_progress(&show_checker);
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
