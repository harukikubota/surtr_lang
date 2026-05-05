use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;
use sindr::builtin::builtin_type_meta_by_name;

use super::ast_ty_span;
use super::context::{DeclLevel, TopLevelDeclKind};
use super::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentKind {
    ReadOnly,
    State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentInstance {
    Singleton,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentMeta {
    kind: AgentKind,
    instance: AgentInstance,
    boot: bool,
    registry: bool,
    lazy: bool,
}

impl AgentKind {
    fn into_process_kind(self) -> ProcessKind {
        ProcessKind::Agent
    }
}

impl AgentInstance {
    fn into_process_instance(self) -> ProcessInstance {
        match self {
            AgentInstance::Singleton => ProcessInstance::Singleton,
            AgentInstance::Worker => ProcessInstance::Worker,
        }
    }
}

impl AgentMeta {
    fn into_process_spec(self, process_name: String) -> ProcessSpec {
        ProcessSpec {
            process_name,
            kind: self.kind.into_process_kind(),
            instance: self.instance.into_process_instance(),
            boot: self.boot,
            registry: self.registry,
            lazy: self.lazy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentHandlerKind {
    Init,
    Get,
    Set,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitPolicy {
    Eager,
    Lazy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessMeta {
    instance: AgentInstance,
    init_policy: InitPolicy,
}

#[derive(Debug, Clone)]
struct AgentHandler {
    def: Ast,
}

fn pid_ty(span: &Span, agent_name: &str) -> AstTy {
    AstTy::Generic(
        span.clone(),
        "PID".to_string(),
        vec![AstTy::Named(span.clone(), agent_name.to_string())],
    )
}

fn process_self_param(span: &Span, agent_name: &str) -> FunParam {
    FunParam {
        name: "__process_self_pid".to_string(),
        ty: pid_ty(span, agent_name),
        span: span.clone(),
    }
}

fn rewrite_process_self_refs(node: Ast) -> Ast {
    match node {
        Ast::App(span, func, args) => {
            let rewritten_func = rewrite_process_self_refs(*func);
            let rewritten_args: Vec<RecordLitArg> = args
                .into_iter()
                .map(|arg| match arg {
                    RecordLitArg::Positional(expr) => {
                        RecordLitArg::Positional(rewrite_process_self_refs(expr))
                    }
                    RecordLitArg::Named(name, expr) => {
                        RecordLitArg::Named(name, rewrite_process_self_refs(expr))
                    }
                })
                .collect();
            if matches!(
                &rewritten_func,
                Ast::Path(_, path) if path.segments.as_slice() == ["Process", "self"]
            ) && rewritten_args.is_empty()
            {
                Ast::Var(span, "__process_self_pid".to_string())
            } else {
                Ast::App(span, Box::new(rewritten_func), rewritten_args)
            }
        }
        Ast::Block(span, stmts) => Ast::Block(
            span,
            stmts.into_iter().map(rewrite_process_self_refs).collect(),
        ),
        Ast::Bind(span, pat, rhs) => {
            Ast::Bind(span, pat, Box::new(rewrite_process_self_refs(*rhs)))
        }
        Ast::SafeBind(span, pat, rhs) => {
            Ast::SafeBind(span, pat, Box::new(rewrite_process_self_refs(*rhs)))
        }
        Ast::BinOp(span, op, lhs, rhs) => Ast::BinOp(
            span,
            op,
            Box::new(rewrite_process_self_refs(*lhs)),
            Box::new(rewrite_process_self_refs(*rhs)),
        ),
        Ast::Pipe(span, lhs, rhs) => Ast::Pipe(
            span,
            Box::new(rewrite_process_self_refs(*lhs)),
            Box::new(rewrite_process_self_refs(*rhs)),
        ),
        Ast::ContextMap(span, lhs, rhs) => Ast::ContextMap(
            span,
            Box::new(rewrite_process_self_refs(*lhs)),
            Box::new(rewrite_process_self_refs(*rhs)),
        ),
        Ast::ContextBind(span, lhs, rhs) => Ast::ContextBind(
            span,
            Box::new(rewrite_process_self_refs(*lhs)),
            Box::new(rewrite_process_self_refs(*rhs)),
        ),
        Ast::Compose(span, lhs, rhs) => Ast::Compose(
            span,
            Box::new(rewrite_process_self_refs(*lhs)),
            Box::new(rewrite_process_self_refs(*rhs)),
        ),
        Ast::LiftedCompose(span, lhs, rhs) => Ast::LiftedCompose(
            span,
            Box::new(rewrite_process_self_refs(*lhs)),
            Box::new(rewrite_process_self_refs(*rhs)),
        ),
        Ast::KleisliCompose(span, lhs, rhs) => Ast::KleisliCompose(
            span,
            Box::new(rewrite_process_self_refs(*lhs)),
            Box::new(rewrite_process_self_refs(*rhs)),
        ),
        Ast::ListCons(span, head, tail) => Ast::ListCons(
            span,
            Box::new(rewrite_process_self_refs(*head)),
            Box::new(rewrite_process_self_refs(*tail)),
        ),
        Ast::ListLiteral(span, items) => Ast::ListLiteral(
            span,
            items.into_iter().map(rewrite_process_self_refs).collect(),
        ),
        Ast::RangeLiteral(span, start, stop) => Ast::RangeLiteral(
            span,
            Box::new(rewrite_process_self_refs(*start)),
            Box::new(rewrite_process_self_refs(*stop)),
        ),
        Ast::TupleLiteral(span, items) => Ast::TupleLiteral(
            span,
            items.into_iter().map(rewrite_process_self_refs).collect(),
        ),
        Ast::Grouped(span, inner) => {
            Ast::Grouped(span, Box::new(rewrite_process_self_refs(*inner)))
        }
        Ast::InterpolatedStr(span, parts) => Ast::InterpolatedStr(
            span,
            parts
                .into_iter()
                .map(|part| match part {
                    InterpolatedPart::Text(text) => InterpolatedPart::Text(text),
                    InterpolatedPart::Expr(expr) => {
                        InterpolatedPart::Expr(Box::new(rewrite_process_self_refs(*expr)))
                    }
                })
                .collect(),
        ),
        Ast::Dbg(span, args) => Ast::Dbg(
            span,
            args.into_iter()
                .map(|arg| DbgArg {
                    span: arg.span,
                    expr: rewrite_process_self_refs(arg.expr),
                })
                .collect(),
        ),
        Ast::Match(span, scrutinee, arms) => Ast::Match(
            span,
            Box::new(rewrite_process_self_refs(*scrutinee)),
            arms.into_iter()
                .map(|arm| AstMatchArm {
                    pattern: arm.pattern,
                    guard: arm.guard.map(rewrite_process_self_refs),
                    body: rewrite_process_self_refs(arm.body),
                })
                .collect(),
        ),
        Ast::FieldAccess(span, expr, field) => {
            Ast::FieldAccess(span, Box::new(rewrite_process_self_refs(*expr)), field)
        }
        Ast::StructLit(span, name, fields) => Ast::StructLit(
            span,
            name,
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(field, expr) => {
                        StructLitField::Explicit(field, rewrite_process_self_refs(expr))
                    }
                    StructLitField::Shorthand(field) => StructLitField::Shorthand(field),
                })
                .collect(),
        ),
        Ast::InternalStructLit(span, name, fields) => Ast::InternalStructLit(
            span,
            name,
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(field, expr) => {
                        StructLitField::Explicit(field, rewrite_process_self_refs(expr))
                    }
                    StructLitField::Shorthand(field) => StructLitField::Shorthand(field),
                })
                .collect(),
        ),
        Ast::ConstructorCall(span, name, args) => Ast::ConstructorCall(
            span,
            name,
            args.into_iter()
                .map(|arg| match arg {
                    RecordLitArg::Positional(expr) => {
                        RecordLitArg::Positional(rewrite_process_self_refs(expr))
                    }
                    RecordLitArg::Named(name, expr) => {
                        RecordLitArg::Named(name, rewrite_process_self_refs(expr))
                    }
                })
                .collect(),
        ),
        Ast::Closure(span, params, body) => {
            Ast::Closure(span, params, Box::new(rewrite_process_self_refs(*body)))
        }
        Ast::Capture(span, target, args) => Ast::Capture(
            span,
            Box::new(rewrite_process_self_refs(*target)),
            args.into_iter().map(rewrite_process_self_refs).collect(),
        ),
        Ast::Semi(span, inner) => Ast::Semi(span, Box::new(rewrite_process_self_refs(*inner))),
        other => other,
    }
}

fn rename_agent_handler(
    mut def: Ast,
    internal_name: &str,
    agent_name: &str,
    inject_process_self: bool,
) -> Result<Ast, ParseError> {
    match &mut def {
        Ast::Def(_, name, _, params, _, body, attrs) => {
            *name = internal_name.to_string();
            attrs.visibility = Visibility::Private;
            if inject_process_self {
                params.insert(0, process_self_param(&body.span().clone(), agent_name));
                let rewritten = rewrite_process_self_refs((**body).clone());
                **body = rewritten;
            }
            Ok(def)
        }
        other => Err(ParseError::syntax(
            "agent handlers must be `def` declarations",
            other.span().clone(),
        )),
    }
}

fn def_params(def: &Ast) -> Result<&Vec<FunParam>, ParseError> {
    match def {
        Ast::Def(_, _, _, params, _, _, _) => Ok(params),
        other => Err(ParseError::syntax(
            "agent lowering expected a function definition",
            other.span().clone(),
        )),
    }
}

fn def_ret_ty(def: &Ast) -> Result<Option<AstTy>, ParseError> {
    match def {
        Ast::Def(_, _, _, _, ret_ty, _, _) => Ok(ret_ty.clone()),
        other => Err(ParseError::syntax(
            "agent lowering expected a function definition",
            other.span().clone(),
        )),
    }
}

fn def_name(def: &Ast) -> Result<String, ParseError> {
    match def {
        Ast::Def(_, name, _, _, _, _, _) => Ok(name.clone()),
        other => Err(ParseError::syntax(
            "process lowering expected a function definition",
            other.span().clone(),
        )),
    }
}

fn def_type_params(def: &Ast) -> Result<Vec<TypeParam>, ParseError> {
    match def {
        Ast::Def(_, _, type_params, _, _, _, _) => Ok(type_params.clone()),
        other => Err(ParseError::syntax(
            "agent lowering expected a function definition",
            other.span().clone(),
        )),
    }
}

fn var(span: &Span, name: &str) -> Ast {
    Ast::Var(span.clone(), name.to_string())
}

fn internal_var(span: &Span, name: &str) -> Ast {
    Ast::InternalVar(span.clone(), name.to_string())
}

fn positional(expr: Ast) -> RecordLitArg {
    RecordLitArg::Positional(expr)
}

fn call(span: &Span, name: &str, args: Vec<Ast>) -> Ast {
    Ast::App(
        span.clone(),
        Box::new(var(span, name)),
        args.into_iter().map(positional).collect(),
    )
}

fn internal_call(span: &Span, name: &str, args: Vec<Ast>) -> Ast {
    Ast::App(
        span.clone(),
        Box::new(internal_var(span, name)),
        args.into_iter().map(positional).collect(),
    )
}

fn string_lit(span: &Span, value: &str) -> Ast {
    Ast::Lit(span.clone(), Lit::Str(value.to_string()))
}

fn capture_ref(span: &Span, name: &str) -> Ast {
    Ast::Capture(span.clone(), Box::new(var(span, name)), Vec::new())
}

fn result_unit_ty(span: &Span) -> AstTy {
    AstTy::Generic(
        span.clone(),
        "Result".to_string(),
        vec![AstTy::Named(span.clone(), "Unit".to_string())],
    )
}

fn unit_ty(span: &Span) -> AstTy {
    AstTy::Named(span.clone(), "Unit".to_string())
}

fn dummy_process_handler(span: &Span, name: &str) -> Ast {
    Ast::Def(
        span.clone(),
        name.to_string(),
        Vec::new(),
        Vec::new(),
        Some(unit_ty(span)),
        Box::new(Ast::Block(
            span.clone(),
            vec![Ast::Lit(span.clone(), Lit::Unit)],
        )),
        DeclAttrs {
            visibility: Visibility::Private,
            ..DeclAttrs::default()
        },
    )
}

fn result_pid_ty(span: &Span, agent_name: &str) -> AstTy {
    AstTy::Generic(
        span.clone(),
        "Result".to_string(),
        vec![pid_ty(span, agent_name)],
    )
}

fn param_vars(span: &Span, params: &[FunParam]) -> Vec<Ast> {
    params
        .iter()
        .map(|param| var(&param.span, &param.name))
        .chain(std::iter::empty::<Ast>())
        .collect::<Vec<_>>()
        .into_iter()
        .map(|node| match node {
            Ast::Var(_, _) => node,
            _ => var(span, ""),
        })
        .collect()
}

fn pid_bind(span: &Span, agent_name: &str) -> Ast {
    Ast::Bind(
        span.clone(),
        AstPattern::Var(span.clone(), "pid".to_string()),
        Box::new(process_pid_call(span, agent_name)),
    )
}

fn process_pid_call(span: &Span, agent_name: &str) -> Ast {
    internal_call(
        span,
        "__process_pid",
        vec![
            string_lit(span, agent_name),
            capture_ref(span, "__agent_init"),
        ],
    )
}

fn process_state_bind(span: &Span) -> Ast {
    Ast::SafeBind(
        span.clone(),
        AstPattern::Var(span.clone(), "state".to_string()),
        Box::new(internal_call(
            span,
            "__process_state",
            vec![var(span, "pid")],
        )),
    )
}

fn init_closure(span: &Span, params: &[FunParam]) -> Ast {
    Ast::Closure(
        span.clone(),
        Vec::new(),
        Box::new(call(span, "__agent_init", param_vars(span, params))),
    )
}

fn build_readonly_get_wrapper(
    span: &Span,
    agent_name: &str,
    get_def: &Ast,
) -> Result<Ast, ParseError> {
    let params = def_params(get_def)?;
    let surface_params = params.iter().skip(2).cloned().collect::<Vec<_>>();
    let mut call_args = vec![var(span, "pid"), var(span, "state")];
    call_args.extend(param_vars(span, &surface_params));
    let body = Ast::Block(
        span.clone(),
        vec![
            pid_bind(span, agent_name),
            process_state_bind(span),
            call(span, "__agent_get", call_args),
        ],
    );
    Ok(Ast::Def(
        span.clone(),
        "get".to_string(),
        def_type_params(get_def)?,
        surface_params,
        def_ret_ty(get_def)?,
        Box::new(body),
        DeclAttrs::default(),
    ))
}

fn pid_param(span: &Span, agent_name: &str) -> FunParam {
    FunParam {
        name: "pid".to_string(),
        ty: pid_ty(span, agent_name),
        span: span.clone(),
    }
}

fn build_pid_wrapper(span: &Span, agent_name: &str) -> Ast {
    Ast::Def(
        span.clone(),
        "pid".to_string(),
        Vec::new(),
        Vec::new(),
        Some(pid_ty(span, agent_name)),
        Box::new(Ast::Block(
            span.clone(),
            vec![process_pid_call(span, agent_name)],
        )),
        DeclAttrs::default(),
    )
}

fn build_spawn_wrapper(span: &Span, agent_name: &str, init_def: &Ast) -> Result<Ast, ParseError> {
    let params = def_params(init_def)?.clone();
    let body = Ast::Block(
        span.clone(),
        vec![internal_call(
            span,
            "__process_spawn",
            vec![string_lit(span, agent_name), init_closure(span, &params)],
        )],
    );
    Ok(Ast::Def(
        span.clone(),
        "spawn".to_string(),
        def_type_params(init_def)?,
        params,
        Some(result_pid_ty(span, agent_name)),
        Box::new(body),
        DeclAttrs::default(),
    ))
}

fn build_state_get_wrapper(
    span: &Span,
    agent_name: &str,
    get_def: &Ast,
    singleton: bool,
) -> Result<Ast, ParseError> {
    let params = def_params(get_def)?;
    let surface_params = if singleton {
        params.iter().skip(2).cloned().collect::<Vec<_>>()
    } else {
        let mut surface_params = vec![pid_param(span, agent_name)];
        surface_params.extend(params.iter().skip(2).cloned());
        surface_params
    };
    let forwarded_params = if singleton {
        surface_params.as_slice()
    } else {
        &surface_params[1..]
    };
    let mut call_args = vec![var(span, "pid"), var(span, "state")];
    call_args.extend(param_vars(span, forwarded_params));
    let mut stmts = Vec::new();
    if singleton {
        stmts.push(pid_bind(span, agent_name));
    }
    stmts.push(process_state_bind(span));
    stmts.push(call(span, "__agent_get", call_args));
    let body = Ast::Block(span.clone(), stmts);
    Ok(Ast::Def(
        span.clone(),
        "get".to_string(),
        def_type_params(get_def)?,
        surface_params,
        def_ret_ty(get_def)?,
        Box::new(body),
        DeclAttrs::default(),
    ))
}

fn build_state_set_wrapper(
    span: &Span,
    agent_name: &str,
    set_def: &Ast,
    singleton: bool,
) -> Result<Ast, ParseError> {
    let params = def_params(set_def)?;
    let surface_params = if singleton {
        params.iter().skip(2).cloned().collect::<Vec<_>>()
    } else {
        let mut surface_params = vec![pid_param(span, agent_name)];
        surface_params.extend(params.iter().skip(2).cloned());
        surface_params
    };
    let forwarded_params = if singleton {
        surface_params.as_slice()
    } else {
        &surface_params[1..]
    };
    let mut call_args = vec![var(span, "pid"), var(span, "state")];
    call_args.extend(param_vars(span, forwarded_params));
    let mut stmts = Vec::new();
    if singleton {
        stmts.push(pid_bind(span, agent_name));
    }
    stmts.push(process_state_bind(span));
    stmts.push(Ast::SafeBind(
        span.clone(),
        AstPattern::Var(span.clone(), "next_state".to_string()),
        Box::new(call(span, "__agent_set", call_args)),
    ));
    stmts.push(internal_call(
        span,
        "__process_store",
        vec![var(span, "pid"), var(span, "next_state")],
    ));
    let body = Ast::Block(span.clone(), stmts);
    Ok(Ast::Def(
        span.clone(),
        "set".to_string(),
        def_type_params(set_def)?,
        surface_params,
        Some(result_unit_ty(span)),
        Box::new(body),
        DeclAttrs::default(),
    ))
}

fn result_reply_ty_from_call_ret(span: &Span, ret_ty: Option<AstTy>) -> Option<AstTy> {
    match ret_ty {
        Some(AstTy::Generic(result_span, name, args)) if name == "Result" => {
            match args.as_slice() {
                [AstTy::Tuple(_, items)] if !items.is_empty() => Some(AstTy::Generic(
                    result_span,
                    "Result".to_string(),
                    vec![items[0].clone()],
                )),
                _ => Some(AstTy::Generic(result_span, name, args)),
            }
        }
        Some(other) => Some(other),
        None => Some(AstTy::Generic(
            span.clone(),
            "Result".to_string(),
            vec![AstTy::Named(span.clone(), "Unit".to_string())],
        )),
    }
}

fn genserver_pair_field(span: &Span, field: &str) -> Ast {
    Ast::FieldAccess(
        span.clone(),
        Box::new(var(span, "reply_state")),
        field.to_string(),
    )
}

fn build_genserver_call_wrapper(
    span: &Span,
    process_name: &str,
    wrapper_name: &str,
    call_def: &Ast,
) -> Result<Ast, ParseError> {
    let params = def_params(call_def)?;
    let surface_params = params.iter().skip(2).cloned().collect::<Vec<_>>();
    let mut call_args = vec![var(span, "pid"), var(span, "state")];
    call_args.extend(param_vars(span, &surface_params));
    let body = Ast::Block(
        span.clone(),
        vec![
            pid_bind(span, process_name),
            process_state_bind(span),
            Ast::SafeBind(
                span.clone(),
                AstPattern::Var(span.clone(), "reply_state".to_string()),
                Box::new(call(span, "__agent_get", call_args)),
            ),
            Ast::SafeBind(
                span.clone(),
                AstPattern::Wildcard(span.clone()),
                Box::new(internal_call(
                    span,
                    "__process_store",
                    vec![var(span, "pid"), genserver_pair_field(span, "_1")],
                )),
            ),
            call(span, "Ok", vec![genserver_pair_field(span, "_0")]),
        ],
    );
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(call_def)?,
        surface_params,
        result_reply_ty_from_call_ret(span, def_ret_ty(call_def)?),
        Box::new(body),
        DeclAttrs::default(),
    ))
}

fn build_genserver_cast_wrapper(
    span: &Span,
    process_name: &str,
    wrapper_name: &str,
    cast_def: &Ast,
) -> Result<Ast, ParseError> {
    let params = def_params(cast_def)?;
    let surface_params = params.iter().skip(2).cloned().collect::<Vec<_>>();
    let mut call_args = vec![var(span, "pid"), var(span, "state")];
    call_args.extend(param_vars(span, &surface_params));
    let body = Ast::Block(
        span.clone(),
        vec![
            pid_bind(span, process_name),
            process_state_bind(span),
            Ast::SafeBind(
                span.clone(),
                AstPattern::Var(span.clone(), "next_state".to_string()),
                Box::new(call(span, "__agent_set", call_args)),
            ),
            internal_call(
                span,
                "__process_store",
                vec![var(span, "pid"), var(span, "next_state")],
            ),
        ],
    );
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(cast_def)?,
        surface_params,
        Some(result_unit_ty(span)),
        Box::new(body),
        DeclAttrs::default(),
    ))
}

impl Parser<'_> {
    fn is_cap_pattern(name: &str) -> bool {
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !first.is_ascii_uppercase() {
            return false;
        }
        chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    }

    pub(super) fn ensure_non_const_identifier(
        &self,
        name: &str,
        span: Span,
        kind: &str,
    ) -> Result<(), ParseError> {
        if Self::is_cap_pattern(name) {
            return Err(ParseError::syntax(
                format!("{kind} cannot use CAP_PATTERN names; `{name}` is reserved for const"),
                span,
            ));
        }
        Ok(())
    }

    fn ensure_const_name(&self, name: &str, span: Span) -> Result<(), ParseError> {
        if !Self::is_cap_pattern(name)
            || name.starts_with('_')
            || name.ends_with('_')
            || name.contains("__")
        {
            return Err(ParseError::syntax(
                format!(
                    "const name must match CAP_PATTERN `[A-Z][A-Z0-9_]*` without leading/trailing/double underscores: {name}"
                ),
                span,
            ));
        }
        Ok(())
    }

    pub(super) fn parse_field_visibility(&mut self) -> Visibility {
        if matches!(self.peek(), Token::Private) {
            self.advance();
            self.skip_newlines();
            Visibility::Private
        } else if matches!(self.peek(), Token::Public) {
            self.advance();
            self.skip_newlines();
            Visibility::Public
        } else {
            Visibility::Public
        }
    }

    pub(super) fn parse_import_selector_list(&mut self) -> Result<(Vec<Symbol>, Span), ParseError> {
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut names = Vec::new();
        loop {
            if matches!(self.peek(), Token::RBrace) {
                break;
            }
            let (name, _span) = self.expect_ident()?;
            names.push(name);
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                continue;
            }
            break;
        }

        if names.is_empty() {
            return Err(ParseError::syntax(
                "Import list requires at least one symbol",
                self.peek_span(),
            ));
        }

        let end = self.expect(&Token::RBrace)?;
        Ok((names, end))
    }

    pub(super) fn parse_import(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Import)?;
        let (first_seg, first_span) = self.expect_ident()?;
        let path_start = first_span.start;
        let mut qualified = vec![(first_seg, first_span)];
        let mut saw_separator = false;

        while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
            saw_separator = true;
            self.consume_path_separator()?;
            let (seg, seg_span) = self.expect_ident()?;
            qualified.push((seg, seg_span));
        }

        let (module_segments, module_end, spec, mut stmt_end) =
            if self.has_path_separator() && matches!(self.peek_n(2), Some(Token::LBrace)) {
                self.consume_path_separator()?;
                let (names, end) = self.parse_import_selector_list()?;
                (
                    qualified.iter().map(|(name, _)| name.clone()).collect(),
                    qualified.last().expect("non-empty path").1.end,
                    ImportSpec::List(names),
                    end.end,
                )
            } else if self.has_path_separator() {
                return Err(ParseError::syntax(
                    "Expected identifier or `{` after `::` in import",
                    self.peek_span(),
                ));
            } else if saw_separator {
                let (name, selected_span) = qualified
                    .pop()
                    .expect("qualified import with separator has at least 2 segments");
                (
                    qualified.iter().map(|(module, _)| module.clone()).collect(),
                    qualified.last().expect("module path is non-empty").1.end,
                    ImportSpec::Single(name),
                    selected_span.end,
                )
            } else {
                (
                    qualified.iter().map(|(name, _)| name.clone()).collect(),
                    qualified.last().expect("non-empty path").1.end,
                    ImportSpec::All,
                    qualified.last().expect("non-empty path").1.end,
                )
            };

        if matches!(self.peek(), Token::Semicolon) {
            stmt_end = self.advance().span.end;
        }

        let path = AstPath {
            span: Span {
                start: path_start,
                end: module_end,
            },
            segments: module_segments,
        };

        Ok(Ast::Import(
            Span {
                start: sp.start,
                end: stmt_end,
            },
            path,
            spec,
        ))
    }

    pub(super) fn parse_include(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Include)?;
        self.skip_newlines();
        let (path, mut stmt_end) = match self.peek().clone() {
            Token::Str(path) => {
                let str_span = self.advance().span.clone();
                (path, str_span.end)
            }
            _ => {
                return Err(ParseError::syntax(
                    "include expects a string literal path",
                    self.peek_span(),
                ))
            }
        };

        if matches!(self.peek(), Token::Semicolon) {
            stmt_end = self.advance().span.end;
        }

        Ok(Ast::Include(
            Span {
                start: sp.start,
                end: stmt_end,
            },
            path,
        ))
    }

    pub(super) fn parse_defmod(&mut self) -> Result<Ast, ParseError> {
        self.parse_defmod_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_namespace(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Namespace)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body = self.parse_namespace_body_stmts()?;
        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::Namespace(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            body,
        ))
    }

    pub(super) fn parse_trait_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_trait_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_trait_impl_head(&mut self) -> Result<(Symbol, Vec<AstTy>), ParseError> {
        let (trait_name, _) = self.expect_qualified_ident(2, "trait")?;
        let trait_args = if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            let mut args = Vec::new();
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    args.push(self.parse_type_in_impl_context(None)?);
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        continue;
                    }
                    break;
                }
            }
            self.expect_type_gt()?;
            args
        } else {
            Vec::new()
        };
        Ok((trait_name, trait_args))
    }

    pub(super) fn parse_impl_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_impl_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_impl_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Impl)?;
        let (head, trait_args) = self.parse_trait_impl_head()?;
        let start = start.unwrap_or(sp.start);
        self.skip_newlines();

        if matches!(self.peek(), Token::For) {
            self.advance();
            self.skip_newlines();
            let target_ty = self.parse_type_in_impl_context(None)?;
            let self_target = self.trait_impl_self_target_name(&target_ty)?;
            self.skip_newlines();
            self.expect(&Token::LBrace)?;
            self.skip_newlines();

            let mut methods = Vec::new();
            while !matches!(self.peek(), Token::RBrace) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete("}", self.peek_span()));
                }
                if matches!(self.peek(), Token::Import) {
                    let import = self.parse_import()?;
                    self.ensure_stmt_boundary(&import, true)?;
                    methods.push(import);
                    while matches!(self.peek(), Token::Newline) {
                        self.advance();
                    }
                    continue;
                }
                if !matches!(self.peek(), Token::Def | Token::Annotator(_)) {
                    return Err(ParseError::syntax(
                        "trait impl body may only contain `def` declarations",
                        self.peek_span(),
                    ));
                }
                let method = if matches!(self.peek(), Token::Annotator(_)) {
                    self.parse_annotated_impl_method(&self_target, true)?
                } else {
                    self.parse_impl_method(&self_target)?
                };
                self.ensure_stmt_boundary(&method, true)?;
                methods.push(method);
                while matches!(self.peek(), Token::Newline) {
                    self.advance();
                }
            }

            let end = self.expect(&Token::RBrace)?;
            return Ok(Ast::TraitImplDef(
                Span {
                    start,
                    end: end.end,
                },
                head,
                trait_args,
                target_ty,
                methods,
                attrs,
            ));
        }

        if !trait_args.is_empty() {
            return Err(ParseError::syntax(
                "Plain `impl Type { ... }` does not accept trait-style type arguments",
                self.peek_span(),
            ));
        }

        if attrs.doc.is_some() {
            return Err(ParseError::syntax(
                "@doc is not allowed before `impl Type`; attach docs to the type declaration, defagent, or impl members",
                sp.clone(),
            ));
        }
        if attrs.hidden {
            return Err(ParseError::syntax(
                "@hidden is only allowed together with @builtin in standard/internal source",
                sp.clone(),
            ));
        }

        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            if matches!(self.peek(), Token::Import) {
                let import = self.parse_import()?;
                self.ensure_stmt_boundary(&import, true)?;
                methods.push(import);
                while matches!(self.peek(), Token::Newline) {
                    self.advance();
                }
                continue;
            }
            if !matches!(
                self.peek(),
                Token::Def | Token::Defp | Token::Defextractor | Token::Annotator(_)
            ) {
                return Err(ParseError::syntax(
                    "impl body may only contain `def` / `defp` / `defextractor` declarations",
                    self.peek_span(),
                ));
            }
            let method = if matches!(self.peek(), Token::Annotator(_)) {
                self.parse_annotated_impl_method(&head, false)?
            } else {
                self.parse_impl_method(&head)?
            };
            self.ensure_stmt_boundary(&method, true)?;
            methods.push(method);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::ImplDef(
            Span {
                start,
                end: end.end,
            },
            head,
            methods,
            attrs,
        ))
    }

    pub(super) fn parse_impl_method(&mut self, target: &str) -> Result<Ast, ParseError> {
        self.parse_impl_method_with_attrs(target, DeclAttrs::default())
    }

    pub(super) fn parse_impl_method_with_attrs(
        &mut self,
        target: &str,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        if matches!(self.peek(), Token::Defextractor) {
            return self.parse_impl_extractor_method_with_attrs(target, attrs);
        }

        let sp = self.peek_span();
        let visibility = match self.peek() {
            Token::Def => {
                self.advance();
                Visibility::Public
            }
            Token::Defp => {
                self.advance();
                Visibility::Private
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected `def` or `defp`",
                    self.peek_span(),
                ));
            }
        };
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        let mut params = Vec::new();

        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();
            let mut first_param = true;
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    let (param_name, param_span) = self.expect_ident()?;

                    let param_ty = if param_name == "self" {
                        if !first_param {
                            return Err(ParseError::syntax(
                                "`self` is only allowed as the first parameter of impl methods",
                                param_span,
                            ));
                        }
                        if matches!(self.peek(), Token::Colon) {
                            self.advance();
                            self.skip_newlines();
                            let ty = self.parse_type_in_impl_context(Some(target.to_string()))?;
                            if !Self::is_self_type(&ty) {
                                return Err(ParseError::syntax(
                                    "`self` receiver type must be `Self`",
                                    ast_ty_span(&ty).clone(),
                                ));
                            }
                            ty
                        } else {
                            AstTy::Named(param_span.clone(), "Self".to_string())
                        }
                    } else {
                        self.expect(&Token::Colon)?;
                        self.skip_newlines();
                        self.parse_type_in_impl_context(Some(target.to_string()))?
                    };

                    params.push(FunParam {
                        name: param_name,
                        ty: param_ty,
                        span: param_span,
                    });
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                    first_param = false;
                }
            }
            self.expect(&Token::RParen)?;
        }

        let ret_ty = if matches!(self.peek(), Token::Arrow) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type_in_impl_context(Some(target.to_string()))?)
        } else {
            None
        };
        self.reject_where_clause()?;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.impl_target_stack.push(target.to_string());
        let body_stmts = self.parse_block_stmts();
        self.impl_target_stack.pop();
        let body_stmts = body_stmts?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Function body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::Def(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            type_params,
            params,
            ret_ty,
            Box::new(body),
            DeclAttrs {
                visibility,
                doc: attrs.doc,
                auto_import: attrs.auto_import,
                hidden: attrs.hidden,
                process_spec: attrs.process_spec,
            },
        ))
    }

    pub(super) fn parse_builtin_impl_method_decl(
        &mut self,
        target: &str,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_builtin_decl_name()?;
        let type_params = self.parse_decl_type_params()?;
        if !type_params.is_empty() {
            return Err(ParseError::syntax(
                "@builtin impl method declarations do not accept method type parameters",
                type_params[0].span.clone(),
            ));
        }

        let mut params = Vec::new();
        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();
            let mut first_param = true;
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    let (param_name, param_span) = self.expect_ident()?;
                    let param_ty = if param_name == "self" {
                        if !first_param {
                            return Err(ParseError::syntax(
                                "`self` is only allowed as the first parameter of impl methods",
                                param_span,
                            ));
                        }
                        if matches!(self.peek(), Token::Colon) {
                            self.advance();
                            self.skip_newlines();
                            let ty = self.parse_type_in_impl_context(Some(target.to_string()))?;
                            if !Self::is_self_type(&ty) {
                                return Err(ParseError::syntax(
                                    "`self` receiver type must be `Self`",
                                    ast_ty_span(&ty).clone(),
                                ));
                            }
                            ty
                        } else {
                            AstTy::Named(param_span.clone(), "Self".to_string())
                        }
                    } else {
                        self.expect(&Token::Colon)?;
                        self.skip_newlines();
                        self.parse_type_in_impl_context(Some(target.to_string()))?
                    };
                    params.push(FunParam {
                        name: param_name,
                        ty: param_ty,
                        span: param_span,
                    });
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                    first_param = false;
                }
            }
            self.expect(&Token::RParen)?;
        }

        let ret_ty = if matches!(self.peek(), Token::Arrow) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type_in_impl_context(Some(target.to_string()))?)
        } else {
            None
        };
        self.reject_where_clause()?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }
        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "@builtin declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinDecl(
            Span { start, end },
            name,
            params,
            ret_ty,
            attrs,
        ))
    }

    pub(super) fn parse_impl_extractor_method_with_attrs(
        &mut self,
        target: &str,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.impl_target_stack.push(target.to_string());
        let (sp, name, type_params, param, ret_ty) = self.parse_extractor_signature()?;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts();
        self.impl_target_stack.pop();
        let body_stmts = body_stmts?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Extractor body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::ExtractorDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            type_params,
            param,
            ret_ty,
            Box::new(body),
            attrs,
        ))
    }

    pub(super) fn parse_annotated_impl_method(
        &mut self,
        target: &str,
        trait_impl_only: bool,
    ) -> Result<Ast, ParseError> {
        let mut attrs = DeclAttrs::default();
        let mut saw_annotator = false;
        let mut saw_builtin = false;
        let mut start_span: Option<Span> = None;

        while let Token::Annotator(name) = self.peek().clone() {
            let annotator_span = self.peek_span();
            saw_annotator = true;
            if start_span.is_none() {
                start_span = Some(annotator_span.clone());
            }
            self.advance();
            self.skip_newlines();
            match name.as_str() {
                "builtin" => {
                    if saw_builtin {
                        return Err(ParseError::syntax(
                            "@builtin may only appear once before an impl member",
                            annotator_span,
                        ));
                    }
                    saw_builtin = true;
                }
                "doc" => {
                    if attrs.doc.is_some() {
                        return Err(ParseError::syntax(
                            "@doc may only appear once before an impl member",
                            annotator_span,
                        ));
                    }
                    match self.peek().clone() {
                        Token::DocString(text) => {
                            if Self::string_has_interpolation(&text) {
                                return Err(ParseError::syntax(
                                    "@doc does not allow string interpolation",
                                    self.peek_span(),
                                ));
                            }
                            self.advance();
                            attrs.doc = Some(text);
                        }
                        Token::Eof => {
                            return Err(ParseError::incomplete("doc string", self.peek_span()));
                        }
                        _ => {
                            return Err(ParseError::syntax(
                                "@doc expects a triple-quoted doc string",
                                self.peek_span(),
                            ));
                        }
                    }
                }
                "hidden" => {
                    if attrs.hidden {
                        return Err(ParseError::syntax(
                            "@hidden may only appear once before an impl member",
                            annotator_span,
                        ));
                    }
                    attrs.hidden = true;
                }
                _ => {
                    return Err(ParseError::syntax(
                        "Only @doc / @hidden / @builtin are allowed before impl members",
                        annotator_span,
                    ));
                }
            }
            self.skip_newlines();
        }

        if !saw_annotator {
            return Err(ParseError::syntax(
                "Expected impl member annotation",
                self.peek_span(),
            ));
        }

        if saw_builtin {
            let start = start_span
                .as_ref()
                .map(|span| span.start)
                .unwrap_or_else(|| self.peek_span().start);
            if attrs.hidden
                && !self
                    .context
                    .parse_rules
                    .allowed_top_level_decl_kinds
                    .allows(super::context::TopLevelDeclKind::BuiltinDecl)
            {
                return Err(ParseError::syntax(
                    "@hidden is only allowed together with @builtin in standard/internal source",
                    start_span.unwrap_or_else(|| self.peek_span()),
                ));
            }
            return match self.peek() {
                Token::Def if trait_impl_only => {
                    self.parse_builtin_impl_method_decl(target, start, attrs)
                }
                Token::Def => self.parse_builtin_decl(start, attrs),
                Token::Defextractor if !trait_impl_only => {
                    self.parse_builtin_extractor_decl(start, attrs)
                }
                Token::Defextractor => Err(ParseError::syntax(
                    "trait impl body may only contain `@builtin def` declarations",
                    self.peek_span(),
                )),
                Token::Defp => Err(ParseError::syntax(
                    "@builtin is not allowed before `defp` impl members",
                    self.peek_span(),
                )),
                _ => Err(ParseError::syntax(
                    "impl body may only contain `@builtin def` / `@builtin defextractor` declarations",
                    self.peek_span(),
                )),
            };
        }

        if trait_impl_only {
            if !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "trait impl body may only contain `def` declarations",
                    self.peek_span(),
                ));
            }
        } else if !matches!(self.peek(), Token::Def | Token::Defp | Token::Defextractor) {
            return Err(ParseError::syntax(
                "impl body may only contain `def` / `defp` / `defextractor` declarations",
                self.peek_span(),
            ));
        }

        self.parse_impl_method_with_attrs(target, attrs)
    }

    pub(super) fn trait_impl_self_target_name(&self, ty: &AstTy) -> Result<String, ParseError> {
        match ty {
            AstTy::Named(_, name) => Ok(name.clone()),
            AstTy::Generic(_, name, _) => Ok(name.clone()),
            AstTy::Func(_, _, _) => Ok("Function".to_string()),
            _ => Err(ParseError::syntax(
                "trait impl target must be a concrete named type or function type in V1",
                ast_ty_span(ty).clone(),
            )),
        }
    }

    pub(super) fn parse_defmod_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        if self.context.module_path.is_some() {
            return Err(ParseError::syntax(
                "Nested module declarations are not allowed",
                sp,
            ));
        }
        self.expect(&Token::Defmod)?;
        let (name, _) = self.expect_qualified_ident(2, "module")?;
        if builtin_type_meta_by_name(&name).is_some() {
            return Err(ParseError::syntax(
                format!(
                    "Module name `{}` is reserved by a canonical builtin type declaration",
                    name
                ),
                sp,
            ));
        }
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body = self.parse_module_body_stmts(Some(name.clone()))?;
        let end = self.expect(&Token::RBrace)?;

        Ok(Ast::Defmod(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            body,
            attrs,
        ))
    }

    pub(super) fn parse_trait_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Deftrait)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            if !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "trait body may only contain `def` signatures",
                    self.peek_span(),
                ));
            }
            let method = self.parse_trait_method_sig()?;
            methods.push(method);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::TraitDef(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            methods,
            attrs,
        ))
    }

    pub(super) fn parse_trait_method_sig(&mut self) -> Result<TraitMethodSig, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        let mut params = Vec::new();
        let self_context = Some("Self".to_string());

        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();

            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    params.push(
                        self.parse_trait_method_param(params.is_empty(), self_context.clone())?,
                    );
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type_in_impl_context(self_context)?;
        if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
            return Err(ParseError::syntax(
                "return-position `impl Trait` is not supported; name the type parameter explicitly",
                ast_ty_span(&ret_ty).clone(),
            ));
        }
        self.reject_where_clause()?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }

        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "trait method declarations must not have a body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            sp.end
        };

        Ok(TraitMethodSig {
            name,
            type_params,
            params,
            ret_ty,
            span: Span {
                start: sp.start,
                end,
            },
        })
    }

    pub(super) fn parse_trait_method_param(
        &mut self,
        is_first_param: bool,
        self_context: Option<String>,
    ) -> Result<FunParam, ParseError> {
        let (name, span) = self.expect_ident()?;
        if name == "self" {
            if !is_first_param {
                return Err(ParseError::syntax(
                    "`self` is only allowed as the first parameter of trait methods",
                    span,
                ));
            }

            let ty = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                let ty = self.parse_type_in_impl_context(self_context)?;
                if !Self::is_self_type(&ty) {
                    return Err(ParseError::syntax(
                        "`self` receiver type must be `Self`",
                        ast_ty_span(&ty).clone(),
                    ));
                }
                ty
            } else {
                AstTy::Named(span.clone(), "Self".to_string())
            };
            return Ok(FunParam { name, ty, span });
        }

        self.expect(&Token::Colon)?;
        let ty = self.parse_type_in_impl_context(self_context)?;
        Ok(FunParam { name, ty, span })
    }

    // ── Data definitions (step 7, 9) ──

    /// `defstruct Name { field: Type, ... }`
    pub(super) fn parse_struct_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_struct_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_struct_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defstruct)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let visibility = self.parse_field_visibility();
            let (fname, fspan) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let fty = self.parse_type()?;
            fields.push(StructField {
                name: fname,
                ty: fty,
                span: fspan,
                visibility,
            });
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::StructDef(
            Span {
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            fields,
            attrs,
        ))
    }

    /// `defrecord Name(field: Type, ...)`
    pub(super) fn parse_record_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_record_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_record_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defrecord)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(")", self.peek_span()));
                }
                self.skip_newlines();
                let visibility = self.parse_field_visibility();
                let (fname, fspan) = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let fty = self.parse_type()?;
                fields.push(RecordField {
                    name: fname,
                    ty: fty,
                    span: fspan,
                    visibility,
                });
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        let end = self.expect(&Token::RParen)?;
        Ok(Ast::RecordDef(
            Span {
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            fields,
            attrs,
        ))
    }

    pub(super) fn parse_enum_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_enum_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_enum_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defenum)?;
        let (name, _name_span) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut variants = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let variant_start = self.peek_span().start;
            let (variant_name, _) = self.expect_ident()?;
            let mut payload = Vec::new();

            if matches!(self.peek(), Token::LParen) {
                self.advance();
                self.skip_newlines();
                if !matches!(self.peek(), Token::RParen) {
                    payload.push(self.parse_type()?);
                    self.skip_newlines();
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        payload.push(self.parse_type()?);
                        self.skip_newlines();
                    }
                }
                self.expect(&Token::RParen)?;
            }

            let discriminant = if matches!(self.peek(), Token::Bind) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_enum_discriminant()?)
            } else {
                None
            };

            let variant_end = if self.pos > 0 {
                self.tokens[self.pos - 1].span.end
            } else {
                variant_start
            };
            variants.push(EnumVariant {
                name: variant_name,
                payload,
                discriminant,
                span: Span {
                    start: variant_start,
                    end: variant_end,
                },
            });

            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                continue;
            }
        }

        if variants.is_empty() {
            return Err(ParseError::syntax(
                "Enum definition requires at least one variant",
                Span {
                    start: sp.start,
                    end: sp.end,
                },
            ));
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::EnumDef(
            Span {
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: param.span,
                })
                .collect(),
            variants,
            attrs,
        ))
    }

    pub(super) fn parse_decl_type_params(&mut self) -> Result<Vec<TypeParam>, ParseError> {
        if !matches!(self.peek(), Token::Lt) {
            return Ok(Vec::new());
        }

        self.advance();
        self.skip_newlines();

        let mut params = Vec::new();
        loop {
            let param_span = self.peek_span();
            self.expect(&Token::Dollar)?;
            let (param_name, _) = self.expect_ident()?;
            let bound = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                let (bound_name, _) = self.expect_ident()?;
                Some(bound_name)
            } else {
                None
            };
            params.push(TypeParam {
                name: format!("${}", param_name),
                bound,
                span: param_span,
            });
            self.skip_newlines();

            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                continue;
            }

            if matches!(self.peek(), Token::Gt) {
                self.expect(&Token::Gt)?;
                break;
            }

            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete(">", self.peek_span()));
            }

            return Err(ParseError::syntax(
                "Expected `,` or `>` in declaration type parameter list",
                self.peek_span(),
            ));
        }

        Ok(params)
    }

    pub(super) fn parse_enum_discriminant(
        &mut self,
    ) -> Result<sindr::primitives::SurtrInt, ParseError> {
        let span = self.peek_span();
        if matches!(self.peek(), Token::Minus) {
            self.advance();
            let int_span = self.peek_span();
            let Token::Int(n) = self.peek().clone() else {
                return Err(ParseError::syntax(
                    "Expected integer literal after '-' in enum discriminant",
                    int_span,
                ));
            };
            self.advance();
            return Ok(-n);
        }
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(n)
            }
            Token::Eof => Err(ParseError::incomplete("integer literal", span)),
            _ => Err(ParseError::syntax(
                "Enum discriminant must be an integer literal",
                span,
            )),
        }
    }

    /// `deferror Name { expr }` or `deferror Name(fields) { expr }`
    pub(super) fn parse_deferror_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_deferror_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_deferror_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Deferror)?;
        let (name, _) = self.expect_ident()?;

        // Optional fields: (field: Type, ...)
        let mut fields = Vec::new();
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    let visibility = self.parse_field_visibility();
                    let (fname, fspan) = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let fty = self.parse_type()?;
                    fields.push(RecordField {
                        name: fname,
                        ty: fty,
                        span: fspan,
                        visibility,
                    });
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        // Show block: { expr }
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let show_expr = self.parse_expr()?;
        self.skip_newlines();
        let end = self.expect(&Token::RBrace)?;

        Ok(Ast::DeferrorDef(
            Span {
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            fields,
            Box::new(show_expr),
            attrs,
        ))
    }

    pub(super) fn parse_def_signature(
        &mut self,
    ) -> Result<
        (
            Span,
            Symbol,
            Vec<TypeParam>,
            Vec<FunParam>,
            Option<AstTy>,
            Visibility,
        ),
        ParseError,
    > {
        self.parse_def_signature_with_name_mode(false)
    }

    pub(super) fn parse_def_signature_with_name_mode(
        &mut self,
        allow_builtin_keyword_name: bool,
    ) -> Result<
        (
            Span,
            Symbol,
            Vec<TypeParam>,
            Vec<FunParam>,
            Option<AstTy>,
            Visibility,
        ),
        ParseError,
    > {
        let sp = self.peek_span();
        let visibility = match self.peek() {
            Token::Def => {
                self.advance();
                Visibility::Public
            }
            Token::Defp => {
                self.advance();
                Visibility::Private
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected `def` or `defp`",
                    self.peek_span(),
                ));
            }
        };
        let (name, name_span) = if allow_builtin_keyword_name {
            self.expect_builtin_decl_name()?
        } else {
            self.expect_ident()?
        };
        if !allow_builtin_keyword_name {
            self.ensure_non_const_identifier(&name, name_span.clone(), "Function name")?;
        }
        let type_params = self.parse_decl_type_params()?;
        let mut params = Vec::new();
        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();

            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    params.push(self.parse_fun_param()?);
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        let ret_ty = if matches!(self.peek(), Token::Arrow) {
            self.advance();
            self.skip_newlines();
            let ret_ty = self.parse_type()?;
            if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
                return Err(ParseError::syntax(
                    "return-position `impl Trait` is not supported; name the type parameter explicitly",
                    ast_ty_span(&ret_ty).clone(),
                ));
            }
            Some(ret_ty)
        } else {
            None
        };

        self.reject_where_clause()?;

        Ok((sp, name, type_params, params, ret_ty, visibility))
    }

    pub(super) fn parse_extractor_signature(
        &mut self,
    ) -> Result<(Span, Symbol, Vec<TypeParam>, ExtractorParam, AstTy), ParseError> {
        self.parse_extractor_signature_with_name_mode(false)
    }

    pub(super) fn parse_extractor_signature_with_name_mode(
        &mut self,
        allow_builtin_keyword_name: bool,
    ) -> Result<(Span, Symbol, Vec<TypeParam>, ExtractorParam, AstTy), ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defextractor)?;
        let (name, name_span) = if allow_builtin_keyword_name {
            self.expect_builtin_decl_name()?
        } else {
            self.expect_ident()?
        };
        if !allow_builtin_keyword_name {
            self.ensure_non_const_identifier(&name, name_span.clone(), "Extractor name")?;
        }
        let type_params = self.parse_decl_type_params()?;
        if Self::is_constructor_style_name(&name) {
            return Err(ParseError::syntax(
                format!(
                    "Extractor names must not use constructor-style names like `{}`; implement `impl {} {{ defextractor deconstruct(...) ... }}` instead",
                    name, name
                ),
                name_span,
            ));
        }
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let (param_name, param_span) = self.expect_ident()?;
        self.ensure_non_const_identifier(&param_name, param_span.clone(), "Extractor parameter")?;
        self.skip_newlines();
        let param_ty = if matches!(self.peek(), Token::Colon) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;
        if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
            return Err(ParseError::syntax(
                "return-position `impl Trait` is not supported; name the type parameter explicitly",
                ast_ty_span(&ret_ty).clone(),
            ));
        }
        self.reject_where_clause()?;
        Ok((
            sp,
            name,
            type_params,
            ExtractorParam {
                name: param_name,
                ty: param_ty,
                span: param_span,
            },
            ret_ty,
        ))
    }

    pub(super) fn reject_where_clause(&self) -> Result<(), ParseError> {
        if matches!(self.peek(), Token::Where) {
            return Err(ParseError::syntax(
                "`where` clauses are staged and not implemented yet",
                self.peek_span(),
            ));
        }
        Ok(())
    }

    pub(super) fn is_constructor_style_name(name: &str) -> bool {
        name.chars().next().is_some_and(|ch| ch.is_uppercase())
    }

    pub(super) fn parse_annotated_decl(&mut self) -> Result<Ast, ParseError> {
        let mut attrs = DeclAttrs::default();
        let mut saw_builtin = false;
        let mut saw_intrinsic = false;
        let mut start_span: Option<Span> = None;
        let mut intrinsic_start_span: Option<Span> = None;

        while let Token::Annotator(name) = self.peek().clone() {
            let annotator_span = self.peek_span();
            if start_span.is_none() {
                start_span = Some(annotator_span.clone());
            }
            self.advance();
            self.skip_newlines();
            match name.as_str() {
                "builtin" => {
                    if saw_builtin {
                        return Err(ParseError::syntax(
                            "@builtin may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    saw_builtin = true;
                }
                "intrinsic" => {
                    if saw_intrinsic {
                        return Err(ParseError::syntax(
                            "@intrinsic may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    if saw_builtin {
                        return Err(ParseError::syntax(
                            "@builtin and @intrinsic cannot be combined",
                            annotator_span,
                        ));
                    }
                    saw_intrinsic = true;
                    intrinsic_start_span = Some(annotator_span);
                }
                "agent" => {
                    return Err(ParseError::syntax(
                        "@agent(...) metadata is no longer supported. Use `meta { instance, init_policy }` inside the process definition.",
                        annotator_span,
                    ));
                }
                "doc" => {
                    if attrs.doc.is_some() {
                        return Err(ParseError::syntax(
                            "@doc may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    let token = self.peek().clone();
                    match token {
                        Token::DocString(text) => {
                            if Self::string_has_interpolation(&text) {
                                return Err(ParseError::syntax(
                                    "@doc does not allow string interpolation",
                                    self.peek_span(),
                                ));
                            }
                            self.advance();
                            attrs.doc = Some(text);
                        }
                        Token::Eof => {
                            return Err(ParseError::incomplete("doc string", self.peek_span()));
                        }
                        _ => {
                            return Err(ParseError::syntax(
                                "@doc expects a triple-quoted doc string",
                                self.peek_span(),
                            ));
                        }
                    }
                }
                "autoimport" => {
                    if attrs.auto_import {
                        return Err(ParseError::syntax(
                            "@autoimport may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    attrs.auto_import = true;
                }
                "hidden" => {
                    if attrs.hidden {
                        return Err(ParseError::syntax(
                            "@hidden may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    attrs.hidden = true;
                }
                "entrypoint" => {
                    return Err(ParseError::syntax(
                        "@entrypoint has been removed",
                        annotator_span,
                    ));
                }
                "test" => {
                    return Err(ParseError::syntax("@test has been removed", annotator_span));
                }
                "init" | "get" | "set" => {
                    return Err(ParseError::syntax(
                        "@init/@get/@set are only allowed on def declarations inside defagent",
                        annotator_span,
                    ));
                }
                _ => {
                    return Err(ParseError::syntax(
                        format!("Unknown annotation: @{name}"),
                        annotator_span,
                    ));
                }
            }
            self.skip_newlines();
        }

        let start = start_span
            .as_ref()
            .map(|span| span.start)
            .unwrap_or_else(|| self.peek_span().start);

        if saw_intrinsic {
            let intrinsic_start = intrinsic_start_span
                .as_ref()
                .map(|span| span.start)
                .unwrap_or(start);
            match self.peek() {
                Token::Def => self.parse_intrinsic_decl(intrinsic_start, attrs),
                _ => Err(ParseError::syntax(
                    "Expected `def` after @intrinsic",
                    self.peek_span(),
                )),
            }
        } else if saw_builtin {
            if attrs.hidden
                && !self
                    .context
                    .parse_rules
                    .allowed_top_level_decl_kinds
                    .allows(super::context::TopLevelDeclKind::BuiltinDecl)
            {
                return Err(ParseError::syntax(
                    "@hidden is only allowed together with @builtin in standard/internal source",
                    start_span.unwrap_or_else(|| self.peek_span()),
                ));
            }
            match self.peek() {
                Token::Def => self.parse_builtin_decl(start, attrs),
                Token::Defextractor => self.parse_builtin_extractor_decl(start, attrs),
                Token::Type => self.parse_builtin_type_decl(start, attrs),
                _ => Err(ParseError::syntax(
                    "Expected `def`, `defextractor`, or `type` after @builtin",
                    self.peek_span(),
                )),
            }
        } else {
            if attrs.hidden {
                return Err(ParseError::syntax(
                    "@hidden is only allowed together with @builtin in standard/internal source",
                    start_span.unwrap_or_else(|| self.peek_span()),
                ));
            }
            match self.peek() {
                Token::Def => self.parse_def_with_attrs(attrs, Some(start)),
                Token::Defmod => self.parse_defmod_with_attrs(attrs, Some(start)),
                Token::Deftrait => self.parse_trait_def_with_attrs(attrs, Some(start)),
                Token::Impl => self.parse_impl_def_with_attrs(attrs, Some(start)),
                Token::Defagent => self.parse_defagent(None, attrs, start),
                Token::Defgenserver => self.parse_defgenserver_with_attrs(attrs, start),
                Token::Defsupervisor => self.parse_defsupervisor_with_attrs(false, attrs, start),
                Token::DefdynamicSupervisor => {
                    self.parse_defsupervisor_with_attrs(true, attrs, start)
                }
                Token::Defstruct => self.parse_struct_def_with_attrs(attrs, Some(start)),
                Token::Defrecord => self.parse_record_def_with_attrs(attrs, Some(start)),
                Token::Deferror => self.parse_deferror_def_with_attrs(attrs, Some(start)),
                Token::Defenum => self.parse_enum_def_with_attrs(attrs, Some(start)),
                Token::Defextractor => self.parse_extractor_def_with_attrs(attrs, Some(start)),
                Token::Eof => Err(ParseError::incomplete("declaration", self.peek_span())),
                _ => Err(ParseError::syntax(
                    "@doc / @autoimport must annotate `def`, `defmod`, `deftrait`, `impl`, `defagent`, `defstruct`, `defrecord`, `deferror`, `defenum`, `defextractor`, `@builtin type/def/defextractor`, or `@intrinsic def`",
                    self.peek_span(),
                )),
            }
        }
    }

    pub(super) fn parse_defagent_without_legacy_meta(&mut self) -> Result<Ast, ParseError> {
        let start = self.peek_span().start;
        self.parse_defagent(None, DeclAttrs::default(), start)
    }

    pub(super) fn parse_defgenserver(&mut self) -> Result<Ast, ParseError> {
        let start = self.peek_span().start;
        self.parse_defgenserver_with_attrs(DeclAttrs::default(), start)
    }

    pub(super) fn parse_defsupervisor(&mut self, dynamic: bool) -> Result<Ast, ParseError> {
        let start = self.peek_span().start;
        self.parse_defsupervisor_with_attrs(dynamic, DeclAttrs::default(), start)
    }

    pub(super) fn parse_supervisor_init(&mut self) -> Result<Ast, ParseError> {
        let start = self.expect(&Token::SupervisorInit)?.start;
        self.skip_newlines();
        self.skip_balanced_brace_block()?;
        let span = Span {
            start,
            end: self.peek_span().start,
        };
        Ok(Ast::Def(
            span.clone(),
            "__supervisor_init".to_string(),
            Vec::new(),
            Vec::new(),
            Some(unit_ty(&span)),
            Box::new(Ast::Block(
                span.clone(),
                vec![Ast::Lit(span.clone(), Lit::Unit)],
            )),
            DeclAttrs {
                visibility: Visibility::Private,
                ..DeclAttrs::default()
            },
        ))
    }

    fn parse_defsupervisor_with_attrs(
        &mut self,
        dynamic: bool,
        mut attrs: DeclAttrs,
        start: usize,
    ) -> Result<Ast, ParseError> {
        let token = if dynamic {
            Token::DefdynamicSupervisor
        } else {
            Token::Defsupervisor
        };
        self.expect(&token)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let process_meta = self.parse_process_meta_block()?;
        self.skip_newlines();
        let mut body = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            if matches!(self.peek(), Token::Defp) {
                return Err(ParseError::syntax(
                    "Supervisor body uses public `def` declarations for user-visible helpers.",
                    self.peek_span(),
                ));
            }
            if !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "Supervisor body currently accepts user-visible `def` declarations",
                    self.peek_span(),
                ));
            }
            let def = self.parse_def_with_attrs(DeclAttrs::default(), None)?;
            self.ensure_stmt_boundary(&def, true)?;
            body.push(def);
            self.skip_newlines();
        }
        let end = self.expect(&Token::RBrace)?;
        let span = Span {
            start,
            end: end.end,
        };
        if process_meta.init_policy == InitPolicy::Lazy {
            return Err(ParseError::syntax(
                "init_policy: Lazy is not allowed for Supervisor",
                span,
            ));
        }
        attrs.process_spec = Some(ProcessSpec {
            process_name: name.clone(),
            kind: if dynamic {
                ProcessKind::DynamicSupervisor
            } else {
                ProcessKind::Supervisor
            },
            instance: process_meta.instance.into_process_instance(),
            boot: false,
            registry: process_meta.instance == AgentInstance::Singleton,
            lazy: false,
        });
        body.insert(
            0,
            dummy_process_handler(
                &Span {
                    start,
                    end: end.end,
                },
                "__agent_get",
            ),
        );
        body.insert(
            0,
            dummy_process_handler(
                &Span {
                    start,
                    end: end.end,
                },
                "__agent_init",
            ),
        );
        Ok(Ast::Defmod(
            Span {
                start,
                end: end.end,
            },
            name,
            body,
            attrs,
        ))
    }

    fn parse_process_meta_block(&mut self) -> Result<ProcessMeta, ParseError> {
        let (head, head_span) = self.expect_ident()?;
        if head != "meta" {
            return Err(ParseError::syntax(
                "process declarations must start with `meta { ... }`",
                head_span,
            ));
        }
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut instance = None;
        let mut init_policy = None;

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let (key, key_span) = self.expect_ident()?;
            self.skip_newlines();
            match key.as_str() {
                "instance" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    let (value, value_span) = self.expect_ident()?;
                    instance = Some(match value.as_str() {
                        "Singleton" => AgentInstance::Singleton,
                        "Worker" => AgentInstance::Worker,
                        _ => {
                            return Err(ParseError::syntax(
                                "process instance must be Singleton or Worker",
                                value_span,
                            ))
                        }
                    });
                }
                "init_policy" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    let (value, value_span) = self.expect_ident()?;
                    init_policy = Some(match value.as_str() {
                        "Eager" => InitPolicy::Eager,
                        "Lazy" => InitPolicy::Lazy,
                        _ => {
                            return Err(ParseError::syntax(
                                "init_policy must be Eager or Lazy",
                                value_span,
                            ))
                        }
                    });
                }
                "handlers" => {
                    self.skip_balanced_brace_block()?;
                }
                _ => {
                    return Err(ParseError::syntax(
                        format!("Unknown process meta key: {key}"),
                        key_span,
                    ))
                }
            }
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(ProcessMeta {
            instance: instance
                .ok_or_else(|| ParseError::syntax("meta requires instance", self.peek_span()))?,
            init_policy: init_policy.unwrap_or(InitPolicy::Eager),
        })
    }

    fn skip_balanced_brace_block(&mut self) -> Result<(), ParseError> {
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let mut depth = 1usize;
        while depth > 0 {
            match self.peek() {
                Token::LBrace => {
                    self.advance();
                    depth += 1;
                }
                Token::RBrace => {
                    self.advance();
                    depth -= 1;
                }
                Token::Eof => return Err(ParseError::incomplete("}", self.peek_span())),
                _ => {
                    self.advance();
                }
            }
        }
        Ok(())
    }

    fn parse_defagent(
        &mut self,
        meta: Option<AgentMeta>,
        attrs: DeclAttrs,
        start: usize,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Defagent)?;
        let (name, _name_span) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut init = None;
        let mut get = None;
        let mut set = None;
        let mut helpers = Vec::new();
        let process_meta = if meta.is_none() {
            let parsed = self.parse_process_meta_block()?;
            self.skip_newlines();
            Some(parsed)
        } else {
            None
        };

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let marker = if matches!(self.peek(), Token::Annotator(name) if matches!(name.as_str(), "init" | "get" | "set"))
            {
                Some(self.parse_agent_handler_marker()?)
            } else {
                None
            };
            self.skip_newlines();
            if marker.is_some() && !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "Agent handler marker must be followed by def inside defagent",
                    self.peek_span(),
                ));
            }
            let def = self.parse_def_with_attrs(DeclAttrs::default(), None)?;
            self.ensure_stmt_boundary(&def, true)?;
            match marker {
                Some(AgentHandlerKind::Init) => {
                    if init.is_some() {
                        return Err(ParseError::syntax(
                            "duplicate @init handler",
                            def.span().clone(),
                        ));
                    }
                    init = Some(AgentHandler { def });
                }
                Some(AgentHandlerKind::Get) => {
                    if get.is_some() {
                        return Err(ParseError::syntax(
                            "duplicate @get handler",
                            def.span().clone(),
                        ));
                    }
                    get = Some(AgentHandler { def });
                }
                Some(AgentHandlerKind::Set) => {
                    if set.is_some() {
                        return Err(ParseError::syntax(
                            "duplicate @set handler",
                            def.span().clone(),
                        ));
                    }
                    set = Some(AgentHandler { def });
                }
                None => helpers.push(def),
            }
            self.skip_newlines();
        }
        let end = self.expect(&Token::RBrace)?;

        let init = init.ok_or_else(|| {
            ParseError::syntax(
                "agents must define an @init handler",
                Span {
                    start,
                    end: end.end,
                },
            )
        })?;
        let get = get.ok_or_else(|| {
            ParseError::syntax(
                "agents must define a @get handler",
                Span {
                    start,
                    end: end.end,
                },
            )
        })?;

        let meta = meta.unwrap_or_else(|| {
            let process_meta = process_meta.expect("new defagent should parse process meta");
            AgentMeta {
                kind: if set.is_some() {
                    AgentKind::State
                } else {
                    AgentKind::ReadOnly
                },
                instance: process_meta.instance,
                boot: false,
                registry: process_meta.instance == AgentInstance::Singleton,
                lazy: process_meta.init_policy == InitPolicy::Lazy,
            }
        });

        self.validate_agent_meta(
            &meta,
            set.as_ref(),
            Span {
                start,
                end: end.end,
            },
        )?;
        self.lower_defagent_to_defmod(
            Span {
                start,
                end: end.end,
            },
            name,
            attrs,
            meta,
            init,
            get,
            set,
            helpers,
        )
    }

    fn parse_defgenserver_with_attrs(
        &mut self,
        mut attrs: DeclAttrs,
        start: usize,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Defgenserver)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let process_meta = self.parse_process_meta_block()?;
        self.skip_newlines();

        let mut init = None;
        let mut call_handler: Option<(String, AgentHandler)> = None;
        let mut cast_handler: Option<(String, AgentHandler)> = None;
        let mut helpers = Vec::new();

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let marker = if let Token::Annotator(marker) = self.peek().clone() {
                if matches!(marker.as_str(), "init" | "call" | "cast") {
                    let span = self.peek_span();
                    self.advance();
                    self.skip_newlines();
                    Some((marker, span))
                } else {
                    None
                }
            } else {
                None
            };

            if marker.is_some() && !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "GenServer handler marker must be followed by def inside defgenserver",
                    self.peek_span(),
                ));
            }
            if matches!(self.peek(), Token::Defp) {
                return Err(ParseError::syntax(
                    "GenServer body uses `def`; visibility is controlled by annotations.",
                    self.peek_span(),
                ));
            }
            let def = self.parse_def_with_attrs(DeclAttrs::default(), None)?;
            self.ensure_stmt_boundary(&def, true)?;
            match marker {
                Some((marker, marker_span)) if marker == "init" => {
                    if init.is_some() {
                        return Err(ParseError::syntax("duplicate @init handler", marker_span));
                    }
                    init = Some(AgentHandler { def });
                }
                Some((marker, marker_span)) if marker == "call" => {
                    if call_handler.is_some() {
                        return Err(ParseError::syntax(
                            "this implementation currently supports one @call handler per GenServer",
                            marker_span,
                        ));
                    }
                    let wrapper_name = def_name(&def)?;
                    call_handler = Some((wrapper_name, AgentHandler { def }));
                }
                Some((marker, marker_span)) if marker == "cast" => {
                    if cast_handler.is_some() {
                        return Err(ParseError::syntax(
                            "this implementation currently supports one @cast handler per GenServer",
                            marker_span,
                        ));
                    }
                    let wrapper_name = def_name(&def)?;
                    cast_handler = Some((wrapper_name, AgentHandler { def }));
                }
                Some((_, marker_span)) => {
                    return Err(ParseError::syntax(
                        "GenServer handler marker must be @init, @call, or @cast",
                        marker_span,
                    ));
                }
                None => helpers.push(def),
            }
            self.skip_newlines();
        }
        let end = self.expect(&Token::RBrace)?;
        let span = Span {
            start,
            end: end.end,
        };
        if process_meta.init_policy == InitPolicy::Lazy
            && process_meta.instance != AgentInstance::Singleton
        {
            return Err(ParseError::syntax(
                "init_policy: Lazy is only allowed for Singleton GenServer",
                span,
            ));
        }
        let init = init.ok_or_else(|| {
            ParseError::syntax(
                "GenServer requires exactly one @init handler",
                Span {
                    start,
                    end: end.end,
                },
            )
        })?;
        let (call_name, call_handler) = call_handler.ok_or_else(|| {
            ParseError::syntax(
                "GenServer requires at least one @call handler",
                Span {
                    start,
                    end: end.end,
                },
            )
        })?;

        let init_def = rename_agent_handler(init.def, "__agent_init", &name, false)?;
        let call_def = rename_agent_handler(call_handler.def, "__agent_get", &name, true)?;
        let cast_def = cast_handler
            .as_ref()
            .map(|(_, handler)| {
                rename_agent_handler(handler.def.clone(), "__agent_set", &name, true)
            })
            .transpose()?;

        let mut body = vec![init_def.clone(), call_def.clone()];
        if let Some(cast_def) = &cast_def {
            body.push(cast_def.clone());
        }
        body.extend(helpers);
        body.push(build_genserver_call_wrapper(
            &span, &name, &call_name, &call_def,
        )?);
        if let (Some((cast_name, _)), Some(cast_def)) = (cast_handler.as_ref(), cast_def.as_ref()) {
            body.push(build_genserver_cast_wrapper(
                &span, &name, cast_name, cast_def,
            )?);
        }

        attrs.process_spec = Some(ProcessSpec {
            process_name: name.clone(),
            kind: ProcessKind::GenServer,
            instance: process_meta.instance.into_process_instance(),
            boot: false,
            registry: process_meta.instance == AgentInstance::Singleton,
            lazy: process_meta.init_policy == InitPolicy::Lazy,
        });
        Ok(Ast::Defmod(span, name, body, attrs))
    }

    fn parse_agent_handler_marker(&mut self) -> Result<AgentHandlerKind, ParseError> {
        let span = self.peek_span();
        let Token::Annotator(name) = self.peek().clone() else {
            return Err(ParseError::syntax(
                "agent handler marker must be @init, @get, or @set",
                self.peek_span(),
            ));
        };
        self.advance();
        self.skip_newlines();
        match name.as_str() {
            "init" => Ok(AgentHandlerKind::Init),
            "get" => Ok(AgentHandlerKind::Get),
            "set" => Ok(AgentHandlerKind::Set),
            _ => Err(ParseError::syntax(
                "agent handler marker must be @init, @get, or @set",
                span,
            )),
        }
    }

    fn validate_agent_meta(
        &self,
        meta: &AgentMeta,
        set: Option<&AgentHandler>,
        span: Span,
    ) -> Result<(), ParseError> {
        match meta.kind {
            AgentKind::ReadOnly => {
                if set.is_some() {
                    return Err(ParseError::syntax(
                        "Agent with no write protocol must not define @set",
                        span,
                    ));
                }
            }
            AgentKind::State => {
                if set.is_none() {
                    return Err(ParseError::syntax(
                        "State agents must define an @set handler",
                        span,
                    ));
                }
                if meta.lazy && meta.instance != AgentInstance::Singleton {
                    return Err(ParseError::syntax(
                        "init_policy: Lazy is only allowed for Singleton Agent",
                        span,
                    ));
                }
            }
        }
        Ok(())
    }

    fn lower_defagent_to_defmod(
        &self,
        span: Span,
        name: Symbol,
        mut attrs: DeclAttrs,
        meta: AgentMeta,
        init: AgentHandler,
        get: AgentHandler,
        set: Option<AgentHandler>,
        helpers: Vec<Ast>,
    ) -> Result<Ast, ParseError> {
        let mut body = Vec::new();
        let init_def = rename_agent_handler(init.def, "__agent_init", &name, false)?;
        let get_def = rename_agent_handler(get.def, "__agent_get", &name, true)?;
        let set_def = set
            .map(|handler| rename_agent_handler(handler.def, "__agent_set", &name, true))
            .transpose()?;

        let init_params = def_params(&init_def)?;
        let get_params = def_params(&get_def)?;
        if meta.instance == AgentInstance::Singleton && !init_params.is_empty() {
            return Err(ParseError::syntax(
                "Singleton agent @init handlers must not take parameters",
                span.clone(),
            ));
        }
        if get_params.is_empty() {
            return Err(ParseError::syntax(
                "@get handlers must take state as their first parameter",
                span.clone(),
            ));
        }
        if let Some(set_def) = &set_def {
            let set_params = def_params(set_def)?;
            if set_params.is_empty() {
                return Err(ParseError::syntax(
                    "@set handlers must take state as their first parameter",
                    span.clone(),
                ));
            }
        }

        body.push(init_def.clone());
        body.push(get_def.clone());
        if let Some(set_def) = &set_def {
            body.push(set_def.clone());
        }
        body.extend(helpers);

        match meta.kind {
            AgentKind::ReadOnly => {
                body.push(build_readonly_get_wrapper(&span, &name, &get_def)?);
            }
            AgentKind::State => {
                if meta.instance == AgentInstance::Worker {
                    body.push(build_spawn_wrapper(&span, &name, &init_def)?);
                } else {
                    body.push(build_pid_wrapper(&span, &name));
                }
                let singleton = meta.instance == AgentInstance::Singleton;
                body.push(build_state_get_wrapper(&span, &name, &get_def, singleton)?);
                if let Some(set_def) = &set_def {
                    body.push(build_state_set_wrapper(&span, &name, set_def, singleton)?);
                }
            }
        }

        attrs.process_spec = Some(meta.into_process_spec(name.clone()));
        Ok(Ast::Defmod(span, name, body, attrs))
    }

    pub(super) fn parse_intrinsic_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Def)?;
        let name = match self.peek().clone() {
            Token::Ident(base_name) => {
                self.advance();
                if matches!(self.peek(), Token::Bang) {
                    self.advance();
                    format!("{base_name}!")
                } else {
                    base_name
                }
            }
            Token::Match => {
                self.advance();
                "match".to_string()
            }
            Token::Cond => {
                self.advance();
                "cond".to_string()
            }
            Token::Bind => {
                self.advance();
                "=".to_string()
            }
            Token::SafeBind => {
                self.advance();
                "=?".to_string()
            }
            Token::Eof => {
                return Err(ParseError::incomplete(
                    "intrinsic declaration name",
                    self.peek_span(),
                ))
            }
            _ => {
                return Err(ParseError::syntax(
                    format!("Expected identifier, got {:?}", self.peek()),
                    self.peek_span(),
                ))
            }
        };

        while !matches!(self.peek(), Token::Newline | Token::Eof | Token::LBrace) {
            self.advance();
        }

        if matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "@intrinsic declaration must not have a function body",
                self.peek_span(),
            ));
        }

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }
        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "@intrinsic declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };
        let signature = self.source_text_for_span(&Span { start, end });

        Ok(Ast::IntrinsicDecl(
            Span { start, end },
            name,
            signature,
            attrs,
        ))
    }

    pub(super) fn parse_builtin_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        let (_def_span, name, _type_params, params, ret_ty, _visibility) =
            self.parse_def_signature_with_name_mode(true)?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }

        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "@builtin declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinDecl(
            Span { start, end },
            name,
            params,
            ret_ty,
            attrs,
        ))
    }

    pub(super) fn parse_builtin_extractor_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        let (_sp, name, _type_params, param, ret_ty) =
            self.parse_extractor_signature_with_name_mode(true)?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }

        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "@builtin extractor declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinExtractorDecl(
            Span { start, end },
            name,
            param,
            ret_ty,
            attrs,
        ))
    }

    pub(super) fn parse_builtin_type_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Type)?;
        self.skip_newlines();
        let (name, name_span) = self.expect_ident()?;

        // `Result` keeps `Ok` / `Err` as declaration-only constructor
        // contracts. They intentionally live behind `@builtin type ...` so
        // the std-module declaration layer stays visually uniform, even though
        // the payload that follows is function-shaped rather than type-shaped.
        if (name == "Ok" || name == "Err") && matches!(self.peek(), Token::LParen) {
            return self.parse_result_ctor_builtin_type_decl(start, name, attrs);
        }

        let mut params = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            loop {
                self.expect(&Token::Dollar)?;
                let (param_name, _) = self.expect_ident()?;
                params.push(format!("${}", param_name));
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    continue;
                }
                if matches!(self.peek(), Token::Gt) {
                    let gt = self.expect(&Token::Gt)?;
                    let end = if self.pos > 0 {
                        self.tokens[self.pos - 1].span.end
                    } else {
                        gt.end
                    };
                    return Ok(Ast::BuiltinTypeDecl(
                        Span { start, end },
                        BuiltinTypeHead {
                            span: Span {
                                start: name_span.start,
                                end,
                            },
                            name,
                            params,
                        },
                        attrs,
                    ));
                }
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(">", self.peek_span()));
                }
                return Err(ParseError::syntax(
                    "Expected `,` or `>` in builtin type parameter list",
                    self.peek_span(),
                ));
            }
        }
        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinTypeDecl(
            Span { start, end },
            BuiltinTypeHead {
                span: Span { start, end },
                name,
                params,
            },
            attrs,
        ))
    }

    pub(super) fn parse_result_ctor_builtin_type_decl(
        &mut self,
        start: usize,
        name: Symbol,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let param_ty = self.parse_type()?;
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;

        if matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Result constructor builtin contracts in std modules must not have a function body",
                self.peek_span(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::ResultCtorDecl(
            Span { start, end },
            name,
            param_ty,
            ret_ty,
            attrs,
        ))
    }

    /// `def name(arg: Type, ...) -> Type { expr }`
    pub(super) fn parse_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_const_def(&mut self) -> Result<Ast, ParseError> {
        let start = self.peek_span().start;
        let visibility = match self.peek() {
            Token::Private => {
                self.advance();
                self.skip_newlines();
                Visibility::Private
            }
            Token::Public => {
                self.advance();
                self.skip_newlines();
                Visibility::Public
            }
            _ => Visibility::Public,
        };
        self.expect(&Token::Const)?;
        self.skip_newlines();
        let (name, name_span) = self.expect_ident()?;
        self.ensure_const_name(&name, name_span)?;
        self.skip_newlines();
        let ty = if matches!(self.peek(), Token::Colon) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.skip_newlines();
        self.expect(&Token::Bind)?;
        self.skip_newlines();
        let value = self.parse_expr()?;
        let mut attrs = DeclAttrs::default();
        attrs.visibility = visibility;
        Ok(Ast::ConstDef(
            Span {
                start,
                end: value.span().end,
            },
            name,
            ty,
            Box::new(value),
            attrs,
        ))
    }

    pub(super) fn parse_extractor_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_extractor_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        if self.should_parse_result_ctor_decl() {
            return self.parse_result_ctor_decl_with_attrs(attrs, annotator_start);
        }

        let (sp, name, type_params, params, ret_ty, visibility) = self.parse_def_signature()?;
        let mut attrs = attrs;
        attrs.visibility = visibility;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts()?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Function body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::Def(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            params,
            ret_ty,
            Box::new(body),
            attrs,
        ))
    }

    pub(super) fn parse_extractor_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let (sp, name, type_params, param, ret_ty) = self.parse_extractor_signature()?;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts()?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Extractor body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::ExtractorDef(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            param,
            ret_ty,
            Box::new(body),
            attrs,
        ))
    }

    pub(super) fn should_parse_result_ctor_decl(&self) -> bool {
        if self.context.level != DeclLevel::Top {
            return false;
        }
        if self.context.module_path.is_some() {
            return false;
        }
        if !self
            .context
            .parse_rules
            .allowed_top_level_decl_kinds
            .allows(TopLevelDeclKind::BuiltinDecl)
        {
            return false;
        }
        if !matches!(self.peek(), Token::Def) {
            return false;
        }
        matches!(
            self.tokens.get(self.pos + 1).map(|sp| &sp.token),
            Some(Token::Ident(name)) if name == "Ok" || name == "Err"
        )
    }

    pub(super) fn parse_result_ctor_decl_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let param_ty = self.parse_type()?;
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;

        if matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Result constructor declarations in std modules must not have a function body",
                self.peek_span(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            sp.start
        };

        Ok(Ast::ResultCtorDecl(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end,
            },
            name,
            param_ty,
            ret_ty,
            attrs,
        ))
    }

    pub(super) fn parse_fun_param(&mut self) -> Result<FunParam, ParseError> {
        let (name, span) = self.expect_ident()?;
        if name == "self" {
            return Err(ParseError::syntax(
                "`self` is only allowed as the first parameter of impl methods",
                span,
            ));
        }
        self.ensure_non_const_identifier(&name, span.clone(), "Function parameter")?;
        self.expect(&Token::Colon)?;
        let ty = self.parse_type()?;
        Ok(FunParam { name, ty, span })
    }
}
