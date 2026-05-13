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

#[derive(Debug, Clone, PartialEq)]
struct AgentMeta {
    kind: AgentKind,
    instance: AgentInstance,
    state: AstTy,
    boot: bool,
    registry: bool,
    lazy: bool,
    handlers: Vec<ProcessHandlerDependency>,
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
            state: self.state,
            boot: self.boot,
            registry: self.registry,
            lazy: self.lazy,
            handlers: self.handlers,
            handler_specs: Vec::new(),
            supervisor_policy: None,
        }
    }
}

fn validate_doc_visibility(attrs: &DeclAttrs, span: &Span) -> Result<(), ParseError> {
    if attrs.doc.is_some() && attrs.visibility == Visibility::Private {
        return Err(private_doc_forbidden_error(span.clone()));
    }
    Ok(())
}

fn parse_doc_attr_in_place(parser: &mut Parser, attrs: &mut DeclAttrs) -> Result<(), ParseError> {
    if attrs.doc.is_some() {
        return Err(ParseError::syntax(
            "@doc may only appear once before a declaration",
            parser.peek_span(),
        ));
    }
    match parser.peek().clone() {
        Token::DocString(text) => {
            if Parser::string_has_interpolation(&text) {
                return Err(ParseError::syntax(
                    "@doc does not allow string interpolation",
                    parser.peek_span(),
                ));
            }
            parser.advance();
            attrs.doc = Some(text);
            Ok(())
        }
        Token::Eof => Err(ParseError::incomplete("doc string", parser.peek_span())),
        _ => Err(ParseError::syntax(
            "@doc expects a triple-quoted doc string",
            parser.peek_span(),
        )),
    }
}

fn make_process_helper_private(def: Ast) -> Ast {
    match def {
        Ast::Def(span, name, type_params, params, ret_ty, body, mut attrs) => {
            attrs.visibility = Visibility::Private;
            Ast::Def(span, name, type_params, params, ret_ty, body, attrs)
        }
        other => other,
    }
}

fn private_doc_forbidden_error(span: Span) -> ParseError {
    ParseError::syntax("@doc is only allowed on public declarations", span)
}

fn ast_decl_attrs(ast: &Ast) -> Option<&DeclAttrs> {
    match ast {
        Ast::Def(_, _, _, _, _, _, attrs)
        | Ast::ConstDef(_, _, _, _, attrs)
        | Ast::ExtractorDef(_, _, _, _, _, _, attrs)
        | Ast::BuiltinDecl(_, _, _, _, attrs)
        | Ast::IntrinsicDecl(_, _, _, attrs)
        | Ast::BuiltinExtractorDecl(_, _, _, _, attrs)
        | Ast::BuiltinTypeDecl(_, _, attrs)
        | Ast::ResultCtorDecl(_, _, _, _, attrs)
        | Ast::StructDef(_, _, _, attrs)
        | Ast::RecordDef(_, _, _, attrs)
        | Ast::DeferrorDef(_, _, _, _, attrs)
        | Ast::EnumDef(_, _, _, _, attrs)
        | Ast::Defmod(_, _, _, attrs)
        | Ast::Defagent(_, _, _, _, attrs)
        | Ast::Defgenserver(_, _, _, _, attrs)
        | Ast::Defsupervisor(_, _, _, _, attrs)
        | Ast::DefdynamicSupervisor(_, _, _, _, attrs)
        | Ast::TraitDef(_, _, _, _, attrs)
        | Ast::ImplDef(_, _, _, attrs)
        | Ast::TraitImplDef(_, _, _, _, _, attrs) => Some(attrs),
        _ => None,
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

#[derive(Debug, Clone, PartialEq)]
struct ProcessMeta {
    instance: AgentInstance,
    init_policy: InitPolicy,
    state: AstTy,
    handlers: Vec<ProcessHandlerDependency>,
}

#[derive(Debug, Clone, PartialEq)]
struct SupervisorMeta {
    policy: SupervisorPolicy,
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
        Ast::FacetSegmentAccess(span, expr, segment) => {
            Ast::FacetSegmentAccess(span, Box::new(rewrite_process_self_refs(*expr)), segment)
        }
        Ast::FacetCapture(span, expr) => {
            Ast::FacetCapture(span, Box::new(rewrite_process_self_refs(*expr)))
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

fn path_call(span: &Span, segments: &[&str], args: Vec<Ast>) -> Ast {
    Ast::App(
        span.clone(),
        Box::new(Ast::Path(
            span.clone(),
            AstPath {
                span: span.clone(),
                segments: segments
                    .iter()
                    .map(|segment| (*segment).to_string())
                    .collect(),
            },
        )),
        args.into_iter().map(positional).collect(),
    )
}

fn internal_qualified_call(span: &Span, segments: &[&str], args: Vec<Ast>) -> Ast {
    Ast::App(
        span.clone(),
        Box::new(internal_var(span, &segments.join("::"))),
        args.into_iter().map(positional).collect(),
    )
}

fn hidden_runtime_call(span: &Span, name: &str, args: Vec<Ast>) -> Ast {
    Ast::App(
        span.clone(),
        Box::new(Ast::InternalVar(
            Span {
                start: span.start,
                end: span.start,
            },
            name.to_string(),
        )),
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

fn result_named_ty(span: &Span, type_name: &str) -> AstTy {
    AstTy::Generic(
        span.clone(),
        "Result".to_string(),
        vec![AstTy::Named(span.clone(), type_name.to_string())],
    )
}

fn process_route_attrs(user_importable: bool, user_callable: bool) -> DeclAttrs {
    DeclAttrs {
        user_importable,
        user_callable,
        ..DeclAttrs::default()
    }
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

fn pid_bind(span: &Span, lower_module: &str, process_name: &str) -> Ast {
    Ast::Bind(
        span.clone(),
        AstPattern::Var(span.clone(), "pid".to_string()),
        Box::new(process_pid_call(span, lower_module, process_name)),
    )
}

fn process_pid_call(span: &Span, lower_module: &str, process_name: &str) -> Ast {
    internal_qualified_call(
        span,
        &[lower_module, "pid"],
        vec![
            string_lit(span, process_name),
            capture_ref(span, "__agent_init"),
        ],
    )
}

fn process_state_bind(span: &Span, lower_module: &str) -> Ast {
    Ast::SafeBind(
        span.clone(),
        AstPattern::Var(span.clone(), "state".to_string()),
        Box::new(internal_qualified_call(
            span,
            &[lower_module, "state"],
            vec![var(span, "pid")],
        )),
    )
}

fn init_spawn_closure(span: &Span, init_def: &Ast) -> Result<Ast, ParseError> {
    let params = def_params(init_def)?.clone();
    Ok(Ast::Closure(
        span.clone(),
        Vec::new(),
        Box::new(call(span, "__agent_init", param_vars(span, &params))),
    ))
}

fn build_worker_init_route_wrapper(
    span: &Span,
    process_name: &str,
    wrapper_name: &str,
    init_def: &Ast,
) -> Result<Ast, ParseError> {
    let params = def_params(init_def)?.clone();
    let body = Ast::Block(
        span.clone(),
        vec![path_call(
            span,
            &["DynamicSupervisor", "spawn"],
            vec![init_spawn_closure(span, init_def)?],
        )],
    );
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(init_def)?,
        params,
        Some(result_pid_ty(span, process_name)),
        Box::new(body),
        process_route_attrs(true, true),
    ))
}

fn build_readonly_get_wrapper(
    span: &Span,
    agent_name: &str,
    wrapper_name: &str,
    get_def: &Ast,
) -> Result<Ast, ParseError> {
    let params = def_params(get_def)?;
    let surface_params = params.iter().skip(2).cloned().collect::<Vec<_>>();
    let mut call_args = vec![var(span, "pid"), var(span, "state")];
    call_args.extend(param_vars(span, &surface_params));
    let body = Ast::Block(
        span.clone(),
        vec![
            pid_bind(span, "Agent", agent_name),
            process_state_bind(span, "Agent"),
            call(span, "__agent_get", call_args),
        ],
    );
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(get_def)?,
        surface_params,
        def_ret_ty(get_def)?,
        Box::new(body),
        process_route_attrs(true, true),
    ))
}

fn pid_param(span: &Span, agent_name: &str) -> FunParam {
    FunParam {
        name: "pid".to_string(),
        ty: pid_ty(span, agent_name),
        span: span.clone(),
    }
}

fn build_pid_wrapper(span: &Span, lower_module: &str, agent_name: &str) -> Ast {
    Ast::Def(
        span.clone(),
        "pid".to_string(),
        Vec::new(),
        Vec::new(),
        Some(pid_ty(span, agent_name)),
        Box::new(Ast::Block(
            span.clone(),
            vec![process_pid_call(span, lower_module, agent_name)],
        )),
        process_route_attrs(true, true),
    )
}

fn build_singleton_init_route_wrapper(
    span: &Span,
    lower_module: &str,
    process_name: &str,
    wrapper_name: &str,
) -> Ast {
    Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        Vec::new(),
        Vec::new(),
        Some(pid_ty(span, process_name)),
        Box::new(Ast::Block(
            span.clone(),
            vec![process_pid_call(span, lower_module, process_name)],
        )),
        process_route_attrs(false, false),
    )
}

fn build_supervisor_spawn_wrapper(span: &Span, supervisor_name: &str) -> Ast {
    Ast::Def(
        span.clone(),
        "spawn".to_string(),
        Vec::new(),
        vec![FunParam {
            name: "worker_init".to_string(),
            ty: AstTy::Func(
                span.clone(),
                Vec::new(),
                Box::new(AstTy::Generic(
                    span.clone(),
                    "Result".to_string(),
                    vec![AstTy::Named(span.clone(), "$State".to_string())],
                )),
            ),
            span: span.clone(),
        }],
        Some(result_pid_ty(span, "$Process")),
        Box::new(Ast::Block(
            span.clone(),
            vec![internal_qualified_call(
                span,
                &["Supervisor", "spawn"],
                vec![string_lit(span, supervisor_name), var(span, "worker_init")],
            )],
        )),
        DeclAttrs::default(),
    )
}

fn build_supervisor_adopt_wrapper(span: &Span, supervisor_name: &str) -> Ast {
    Ast::Def(
        span.clone(),
        "adopt".to_string(),
        Vec::new(),
        vec![FunParam {
            name: "pid".to_string(),
            ty: pid_ty(span, "$Process"),
            span: span.clone(),
        }],
        Some(result_unit_ty(span)),
        Box::new(Ast::Block(
            span.clone(),
            vec![internal_qualified_call(
                span,
                &["Supervisor", "adopt"],
                vec![string_lit(span, supervisor_name), var(span, "pid")],
            )],
        )),
        DeclAttrs::default(),
    )
}

fn build_supervisor_status_wrapper(span: &Span, supervisor_name: &str) -> Ast {
    Ast::Def(
        span.clone(),
        "status".to_string(),
        Vec::new(),
        Vec::new(),
        Some(result_named_ty(span, "SupervisorStatus")),
        Box::new(Ast::Block(
            span.clone(),
            vec![internal_qualified_call(
                span,
                &["Supervisor", "status"],
                vec![string_lit(span, supervisor_name)],
            )],
        )),
        DeclAttrs::default(),
    )
}

fn build_supervisor_workers_wrapper(span: &Span, supervisor_name: &str) -> Ast {
    Ast::Def(
        span.clone(),
        "workers".to_string(),
        Vec::new(),
        vec![
            FunParam {
                name: "worker_init".to_string(),
                ty: AstTy::Func(
                    span.clone(),
                    Vec::new(),
                    Box::new(AstTy::Generic(
                        span.clone(),
                        "Result".to_string(),
                        vec![AstTy::Named(span.clone(), "$State".to_string())],
                    )),
                ),
                span: span.clone(),
            },
            FunParam {
                name: "strategy".to_string(),
                ty: AstTy::Named(span.clone(), "WorkerStrategy".to_string()),
                span: span.clone(),
            },
        ],
        Some(AstTy::Generic(
            span.clone(),
            "Result".to_string(),
            vec![AstTy::Generic(
                span.clone(),
                "Workers".to_string(),
                vec![AstTy::Named(span.clone(), "$Process".to_string())],
            )],
        )),
        Box::new(Ast::Block(
            span.clone(),
            vec![internal_qualified_call(
                span,
                &["Supervisor", "workers"],
                vec![
                    string_lit(span, supervisor_name),
                    var(span, "worker_init"),
                    var(span, "strategy"),
                ],
            )],
        )),
        DeclAttrs::default(),
    )
}

fn is_compiler_managed_process_surface_name(name: &str) -> bool {
    matches!(name, "pid" | "spawn" | "adopt" | "status" | "workers")
}

fn ensure_no_compiler_managed_process_surface_names(
    defs: &[Ast],
    process_name: &str,
) -> Result<(), ParseError> {
    for def in defs {
        let def_name = match def {
            Ast::Def(_, name, ..) => name.as_str(),
            _ => continue,
        };
        if is_compiler_managed_process_surface_name(def_name) {
            return Err(ParseError::syntax(
                format!(
                    "`{}::{}` is compiler-managed and cannot be user-defined",
                    process_name, def_name
                ),
                def.span().clone(),
            ));
        }
    }
    Ok(())
}

fn ensure_process_surface_name_not_reserved(
    name: &str,
    span: &Span,
    process_name: &str,
) -> Result<(), ParseError> {
    if is_compiler_managed_process_surface_name(name) {
        return Err(ParseError::syntax(
            format!(
                "`{}::{}` is compiler-managed and cannot be user-defined",
                process_name, name
            ),
            span.clone(),
        ));
    }
    Ok(())
}

fn build_state_get_wrapper(
    span: &Span,
    agent_name: &str,
    wrapper_name: &str,
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
        stmts.push(pid_bind(span, "Agent", agent_name));
    }
    stmts.push(process_state_bind(span, "Agent"));
    stmts.push(call(span, "__agent_get", call_args));
    let body = Ast::Block(span.clone(), stmts);
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(get_def)?,
        surface_params,
        def_ret_ty(get_def)?,
        Box::new(body),
        process_route_attrs(true, true),
    ))
}

fn build_state_set_wrapper(
    span: &Span,
    agent_name: &str,
    wrapper_name: &str,
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
        stmts.push(pid_bind(span, "Agent", agent_name));
    }
    stmts.push(process_state_bind(span, "Agent"));
    stmts.push(Ast::SafeBind(
        span.clone(),
        AstPattern::Var(span.clone(), "next_state".to_string()),
        Box::new(call(span, "__agent_set", call_args)),
    ));
    stmts.push(internal_qualified_call(
        span,
        &["Agent", "store"],
        vec![var(span, "pid"), var(span, "next_state")],
    ));
    let body = Ast::Block(span.clone(), stmts);
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(set_def)?,
        surface_params,
        Some(result_unit_ty(span)),
        Box::new(body),
        process_route_attrs(true, true),
    ))
}

fn result_reply_ty_from_call_ret(span: &Span, ret_ty: Option<AstTy>) -> Option<AstTy> {
    match ret_ty {
        Some(AstTy::Generic(result_span, name, args)) if name == "Result" => {
            match args.as_slice() {
                [AstTy::Generic(_, call_result, items)]
                    if call_result == "CallResult" && !items.is_empty() =>
                {
                    Some(AstTy::Generic(
                        result_span,
                        "Result".to_string(),
                        vec![items[0].clone()],
                    ))
                }
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

fn ctor_pattern(span: &Span, name: &str, args: Vec<AstPattern>) -> AstPattern {
    AstPattern::Constructor(span.clone(), name.to_string(), args)
}

fn build_genserver_call_wrapper(
    span: &Span,
    process_name: &str,
    wrapper_name: &str,
    internal_handler_name: &str,
    call_def: &Ast,
    singleton: bool,
) -> Result<Ast, ParseError> {
    let params = def_params(call_def)?;
    let surface_params = if singleton {
        params.iter().skip(2).cloned().collect::<Vec<_>>()
    } else {
        let mut surface_params = vec![pid_param(span, process_name)];
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
        stmts.push(pid_bind(span, "GenServer", process_name));
    }
    stmts.push(process_state_bind(span, "GenServer"));
    stmts.push(Ast::SafeBind(
        span.clone(),
        AstPattern::Var(span.clone(), "call_result".to_string()),
        Box::new(call(span, internal_handler_name, call_args)),
    ));
    stmts.push(Ast::Match(
        span.clone(),
        Box::new(var(span, "call_result")),
        vec![
            AstMatchArm {
                pattern: ctor_pattern(
                    span,
                    "CallResult::Reply",
                    vec![
                        AstPattern::Var(span.clone(), "reply".to_string()),
                        AstPattern::Var(span.clone(), "next_state".to_string()),
                    ],
                ),
                guard: None,
                body: hidden_runtime_call(
                    span,
                    "__genserver_call_reply",
                    vec![
                        var(span, "pid"),
                        var(span, "next_state"),
                        var(span, "reply"),
                    ],
                ),
            },
            AstMatchArm {
                pattern: ctor_pattern(
                    span,
                    "CallResult::ReplyLater",
                    vec![
                        AstPattern::Var(span.clone(), "next_state".to_string()),
                        AstPattern::Var(span.clone(), "callback".to_string()),
                    ],
                ),
                guard: None,
                body: hidden_runtime_call(
                    span,
                    "__genserver_call_reply_later",
                    vec![
                        var(span, "pid"),
                        var(span, "next_state"),
                        var(span, "callback"),
                    ],
                ),
            },
            AstMatchArm {
                pattern: ctor_pattern(
                    span,
                    "CallResult::Stop",
                    vec![ctor_pattern(
                        span,
                        "StopReply::Normal",
                        vec![AstPattern::Var(span.clone(), "reply".to_string())],
                    )],
                ),
                guard: None,
                body: hidden_runtime_call(
                    span,
                    "__genserver_call_stop_normal",
                    vec![var(span, "pid"), var(span, "reply")],
                ),
            },
            AstMatchArm {
                pattern: ctor_pattern(
                    span,
                    "CallResult::Stop",
                    vec![ctor_pattern(
                        span,
                        "StopReply::Error",
                        vec![AstPattern::Var(span.clone(), "err".to_string())],
                    )],
                ),
                guard: None,
                body: hidden_runtime_call(
                    span,
                    "__genserver_call_stop_error",
                    vec![var(span, "pid"), var(span, "err")],
                ),
            },
        ],
    ));
    let body = Ast::Block(span.clone(), stmts);
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(call_def)?,
        surface_params,
        result_reply_ty_from_call_ret(span, def_ret_ty(call_def)?),
        Box::new(body),
        process_route_attrs(true, true),
    ))
}

fn build_genserver_cast_wrapper(
    span: &Span,
    process_name: &str,
    wrapper_name: &str,
    internal_handler_name: &str,
    cast_def: &Ast,
    singleton: bool,
) -> Result<Ast, ParseError> {
    let params = def_params(cast_def)?;
    let surface_params = if singleton {
        params.iter().skip(2).cloned().collect::<Vec<_>>()
    } else {
        let mut surface_params = vec![pid_param(span, process_name)];
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
        stmts.push(pid_bind(span, "GenServer", process_name));
    }
    stmts.push(process_state_bind(span, "GenServer"));
    stmts.push(Ast::SafeBind(
        span.clone(),
        AstPattern::Var(span.clone(), "cast_result".to_string()),
        Box::new(call(span, internal_handler_name, call_args)),
    ));
    stmts.push(Ast::Match(
        span.clone(),
        Box::new(var(span, "cast_result")),
        vec![
            AstMatchArm {
                pattern: ctor_pattern(
                    span,
                    "CastResult::Next",
                    vec![AstPattern::Var(span.clone(), "next_state".to_string())],
                ),
                guard: None,
                body: hidden_runtime_call(
                    span,
                    "__genserver_cast_next",
                    vec![var(span, "pid"), var(span, "next_state")],
                ),
            },
            AstMatchArm {
                pattern: ctor_pattern(
                    span,
                    "CastResult::Stop",
                    vec![ctor_pattern(span, "StopReason::Normal", vec![])],
                ),
                guard: None,
                body: hidden_runtime_call(
                    span,
                    "__genserver_cast_stop_normal",
                    vec![var(span, "pid")],
                ),
            },
            AstMatchArm {
                pattern: ctor_pattern(
                    span,
                    "CastResult::Stop",
                    vec![ctor_pattern(
                        span,
                        "StopReason::Error",
                        vec![AstPattern::Var(span.clone(), "err".to_string())],
                    )],
                ),
                guard: None,
                body: hidden_runtime_call(
                    span,
                    "__genserver_cast_stop_error",
                    vec![var(span, "pid"), var(span, "err")],
                ),
            },
        ],
    ));
    let body = Ast::Block(span.clone(), stmts);
    Ok(Ast::Def(
        span.clone(),
        wrapper_name.to_string(),
        def_type_params(cast_def)?,
        surface_params,
        Some(result_unit_ty(span)),
        Box::new(body),
        process_route_attrs(true, true),
    ))
}

impl Parser<'_> {
    fn canonicalize_impl_target_name(name: String) -> String {
        if name.contains("::") {
            name
        } else {
            format!("Global::{name}")
        }
    }

    fn canonicalize_impl_target_ty(ty: AstTy) -> AstTy {
        match ty {
            AstTy::Named(span, name) if !name.starts_with('$') && name != "Self" && name != "_" => {
                AstTy::Named(span, Self::canonicalize_impl_target_name(name))
            }
            AstTy::ImplTrait(span, name) => {
                AstTy::ImplTrait(span, Self::canonicalize_impl_target_name(name))
            }
            AstTy::Generic(span, name, args) => {
                AstTy::Generic(span, Self::canonicalize_impl_target_name(name), args)
            }
            other => other,
        }
    }

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

    pub(super) fn parse_field_modifiers(&mut self) -> Result<(Visibility, bool), ParseError> {
        let mut visibility = Visibility::Public;
        let mut saw_visibility = false;
        let mut readonly = false;

        loop {
            match self.peek() {
                Token::Private => {
                    if saw_visibility {
                        return Err(ParseError::syntax(
                            "field visibility may only be specified once",
                            self.peek_span(),
                        ));
                    }
                    saw_visibility = true;
                    visibility = Visibility::Private;
                    self.advance();
                    self.skip_newlines();
                }
                Token::Public => {
                    if saw_visibility {
                        return Err(ParseError::syntax(
                            "field visibility may only be specified once",
                            self.peek_span(),
                        ));
                    }
                    saw_visibility = true;
                    visibility = Visibility::Public;
                    self.advance();
                    self.skip_newlines();
                }
                Token::Readonly => {
                    if readonly {
                        return Err(ParseError::syntax(
                            "readonly field modifier may only be specified once",
                            self.peek_span(),
                        ));
                    }
                    readonly = true;
                    self.advance();
                    self.skip_newlines();
                }
                _ => return Ok((visibility, readonly)),
            }
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
        if name == "Global" {
            return Err(ParseError::syntax(
                "`Global` is reserved for the implicit root namespace",
                sp,
            ));
        }
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
            let target_ty =
                Self::canonicalize_impl_target_ty(self.parse_type_in_impl_context(None)?);
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
        let head = Self::canonicalize_impl_target_name(head);
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

        let ast = Ast::Def(
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
                doc: attrs.doc,
                auto_import: attrs.auto_import,
                hidden: attrs.hidden,
                readonly: attrs.readonly,
                visibility,
                user_importable: attrs.user_importable,
                user_callable: attrs.user_callable,
            },
        );
        let attrs = ast_decl_attrs(&ast).expect("impl method is a declaration");
        validate_doc_visibility(attrs, ast.span())?;
        Ok(ast)
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
        let mut saw_intrinsic = false;
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
                "intrinsic" => {
                    if saw_intrinsic {
                        return Err(ParseError::syntax(
                            "@intrinsic may only appear once before an impl member",
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
                        "Only @doc / @hidden / @builtin / @intrinsic are allowed before impl members",
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

        if saw_intrinsic {
            let start = start_span
                .as_ref()
                .map(|span| span.start)
                .unwrap_or_else(|| self.peek_span().start);
            return match self.peek() {
                Token::Def => self.parse_intrinsic_decl(start, attrs),
                Token::Defp => Err(ParseError::syntax(
                    "@intrinsic is not allowed before `defp` impl members",
                    self.peek_span(),
                )),
                _ => Err(ParseError::syntax(
                    "impl body may only contain `@intrinsic def` declarations for intrinsic members",
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
            AstTy::Tuple(_, items) if items.len() >= 2 => Ok(format!("Tuple{}", items.len())),
            _ => Err(ParseError::syntax(
                "trait impl target must be a concrete named type, tuple type, or function type in V1",
                ast_ty_span(ty).clone(),
            )),
        }
    }

    fn parse_agent_member_prefixes(
        &mut self,
    ) -> Result<(Option<AgentHandlerKind>, DeclAttrs), ParseError> {
        let mut marker = None;
        let mut attrs = DeclAttrs::default();
        while let Token::Annotator(name) = self.peek().clone() {
            match name.as_str() {
                "doc" => {
                    self.advance();
                    self.skip_newlines();
                    parse_doc_attr_in_place(self, &mut attrs)?;
                }
                "init" | "get" | "set" => {
                    if marker.is_some() {
                        return Err(ParseError::syntax(
                            "process member may only have one handler marker",
                            self.peek_span(),
                        ));
                    }
                    marker = Some(self.parse_agent_handler_marker()?);
                }
                _ => break,
            }
            self.skip_newlines();
        }
        Ok((marker, attrs))
    }

    fn parse_genserver_member_prefixes(
        &mut self,
    ) -> Result<(Option<(String, Span)>, DeclAttrs), ParseError> {
        let mut marker = None;
        let mut attrs = DeclAttrs::default();
        while let Token::Annotator(name) = self.peek().clone() {
            match name.as_str() {
                "doc" => {
                    self.advance();
                    self.skip_newlines();
                    parse_doc_attr_in_place(self, &mut attrs)?;
                }
                "init" | "call" | "cast" => {
                    if marker.is_some() {
                        return Err(ParseError::syntax(
                            "process member may only have one handler marker",
                            self.peek_span(),
                        ));
                    }
                    let span = self.peek_span();
                    self.advance();
                    self.skip_newlines();
                    marker = Some((name, span));
                }
                _ => break,
            }
            self.skip_newlines();
        }
        Ok((marker, attrs))
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
        let reserved_check_name = name.rsplit("::").next().unwrap_or(&name);
        if reserved_check_name != "ProcessInit"
            && builtin_type_meta_by_name(reserved_check_name).is_some()
        {
            return Err(ParseError::syntax(
                format!(
                    "Module name `{}` is reserved by a canonical builtin type declaration",
                    reserved_check_name
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
        let (name, _) = self.expect_qualified_ident(2, "type")?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let (visibility, readonly) = self.parse_field_modifiers()?;
            let (fname, fspan) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let fty = self.parse_type()?;
            fields.push(StructField {
                name: fname,
                ty: fty,
                span: fspan,
                visibility,
                readonly,
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
        let (name, _) = self.expect_qualified_ident(2, "type")?;
        self.expect(&Token::LParen)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(")", self.peek_span()));
                }
                self.skip_newlines();
                let (visibility, readonly) = self.parse_field_modifiers()?;
                if readonly {
                    return Err(ParseError::syntax(
                        "readonly field modifier is only supported on `defstruct` fields",
                        self.peek_span(),
                    ));
                }
                let (fname, fspan) = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let fty = self.parse_type()?;
                fields.push(RecordField {
                    name: fname,
                    ty: fty,
                    span: fspan,
                    visibility,
                    readonly,
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
        let (name, _name_span) = self.expect_qualified_ident(2, "type")?;
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
        let (name, _) = self.expect_qualified_ident(2, "type")?;

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
                    let (visibility, readonly) = self.parse_field_modifiers()?;
                    if readonly {
                        return Err(ParseError::syntax(
                            "readonly field modifier is only supported on `defstruct` fields",
                            self.peek_span(),
                        ));
                    }
                    let (fname, fspan) = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let fty = self.parse_type()?;
                    fields.push(RecordField {
                        name: fname,
                        ty: fty,
                        span: fspan,
                        visibility,
                        readonly,
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
                        "@agent(...) metadata is no longer supported. Use `meta { instance, init_policy, state }` inside the process definition.",
                        annotator_span,
                    ));
                }
                "process_state" => {
                    return Err(ParseError::syntax(
                        "@process_state has been removed. Declare process state with `meta { state: StateTy }` inside the process definition.",
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
                "readonly" => {
                    if attrs.readonly {
                        return Err(ParseError::syntax(
                            "@readonly may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    attrs.readonly = true;
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
            if attrs.readonly && !matches!(self.peek(), Token::Defstruct) {
                return Err(ParseError::syntax(
                    "@readonly may only annotate `defstruct` declarations",
                    start_span.unwrap_or_else(|| self.peek_span()),
                ));
            }
            match self.peek() {
                Token::Def | Token::Defp => self.parse_def_with_attrs(attrs, Some(start)),
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
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let mut entries = Vec::new();

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let (entry_name, entry_span) = self.expect_ident()?;
            if entry_name == "singleton" {
                return Err(ParseError::syntax(
                    "supervisor_init `singleton` keyword is no longer used",
                    entry_span,
                ));
            }
            let entry = self.parse_supervisor_init_entry(entry_name, entry_span)?;
            if entries
                .iter()
                .any(|existing: &SupervisorInitEntry| existing.process_name == entry.process_name)
            {
                return Err(ParseError::syntax(
                    "supervisor_init entry is duplicated",
                    entry.span,
                ));
            }
            entries.push(entry);
            self.skip_newlines();
        }
        let end = self.expect(&Token::RBrace)?;
        let span = Span {
            start,
            end: end.end,
        };
        Ok(Ast::SupervisorInit(
            span,
            SupervisorInitSpec {
                entries,
                singletons: Vec::new(),
                supervisors: Vec::new(),
            },
        ))
    }

    fn parse_supervisor_init_entry(
        &mut self,
        process_name: String,
        name_span: Span,
    ) -> Result<SupervisorInitEntry, ParseError> {
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let mut timeout_ms = None;
        let mut handlers = Vec::new();
        let mut overrides = SupervisorPolicyOverride::default();

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let (key, key_span) = self.expect_ident()?;
            self.skip_newlines();
            match key.as_str() {
                "timeout" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    let parsed = self.parse_supervisor_init_timeout_ms()?;
                    if !(1..=60_000).contains(&parsed) {
                        return Err(ParseError::syntax(
                            if parsed == 0 {
                                "init timeout must be at least `1ms`"
                            } else {
                                "init timeout must not exceed `60s`"
                            },
                            key_span,
                        ));
                    }
                    timeout_ms = Some(parsed);
                }
                "handlers" => {
                    handlers = self.parse_supervisor_init_handlers()?;
                }
                "strategy" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    if overrides.strategy.is_some() {
                        return Err(ParseError::syntax(
                            "strategy override is duplicated",
                            key_span,
                        ));
                    }
                    overrides.strategy = Some(self.parse_supervisor_strategy()?);
                }
                "max_restarts" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    if overrides.max_restarts.is_some() {
                        return Err(ParseError::syntax(
                            "max_restarts override is duplicated",
                            key_span,
                        ));
                    }
                    overrides.max_restarts = Some(self.parse_non_negative_int_literal()?);
                }
                "max_seconds" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    if overrides.max_seconds.is_some() {
                        return Err(ParseError::syntax(
                            "max_seconds override is duplicated",
                            key_span,
                        ));
                    }
                    overrides.max_seconds = Some(self.parse_non_negative_int_literal()?);
                }
                "child_restart_default" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    if overrides.child_restart_default.is_some() {
                        return Err(ParseError::syntax(
                            "child_restart_default override is duplicated",
                            key_span,
                        ));
                    }
                    overrides.child_restart_default = Some(self.parse_child_restart_policy()?);
                }
                "allow_adopt" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    if overrides.allow_adopt.is_some() {
                        return Err(ParseError::syntax(
                            "allow_adopt override is duplicated",
                            key_span,
                        ));
                    }
                    overrides.allow_adopt = Some(self.parse_bool_literal()?);
                }
                "shutdown_timeout" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    if overrides.shutdown_timeout_ms.is_some() {
                        return Err(ParseError::syntax(
                            "shutdown_timeout override is duplicated",
                            key_span,
                        ));
                    }
                    overrides.shutdown_timeout_ms =
                        Some(self.parse_duration_literal_ms("shutdown_timeout")?);
                }
                "parent" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    return Err(ParseError::syntax(
                        "supervisor parent override is fixed in the initial phase",
                        key_span,
                    ));
                }
                "init_policy" => {
                    return Err(ParseError::syntax(
                        "init policy belongs to process definition",
                        key_span,
                    ));
                }
                "boot" => {
                    return Err(ParseError::syntax(
                        "boot policy is no longer used",
                        key_span,
                    ));
                }
                _ => {
                    return Err(ParseError::syntax(
                        format!("Unknown supervisor_init key: {key}"),
                        key_span,
                    ));
                }
            }
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(SupervisorInitEntry {
            process_name,
            timeout_ms,
            handlers,
            overrides,
            span: Span {
                start: name_span.start,
                end: end.end,
            },
        })
    }

    fn parse_supervisor_init_timeout_ms(&mut self) -> Result<u64, ParseError> {
        let span = self.peek_span();
        let Token::Int(n) = self.peek().clone() else {
            return Err(ParseError::syntax(
                "init timeout must be a duration literal like `5s` or `100ms`",
                span,
            ));
        };
        self.advance();
        let (suffix, suffix_span) = self.expect_ident()?;
        let Some(value) = n.to_string().parse::<u64>().ok() else {
            return Err(ParseError::syntax(
                "init timeout literal is too large",
                span,
            ));
        };
        match suffix.as_str() {
            "ms" => Ok(value),
            "s" => value.checked_mul(1_000).ok_or_else(|| {
                ParseError::syntax("init timeout literal is too large", suffix_span)
            }),
            _ => Err(ParseError::syntax(
                "init timeout must use `ms` or `s`",
                suffix_span,
            )),
        }
    }

    fn parse_non_negative_int_literal(&mut self) -> Result<u64, ParseError> {
        let span = self.peek_span();
        let Token::Int(n) = self.peek().clone() else {
            return Err(ParseError::syntax("expected integer literal", span));
        };
        self.advance();
        n.to_string()
            .parse::<u64>()
            .map_err(|_| ParseError::syntax("integer literal is too large", span))
    }

    fn parse_bool_literal(&mut self) -> Result<bool, ParseError> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::True => {
                self.advance();
                Ok(true)
            }
            Token::False => {
                self.advance();
                Ok(false)
            }
            _ => Err(ParseError::syntax("expected `True` or `False`", span)),
        }
    }

    fn parse_duration_literal_ms(&mut self, field_name: &str) -> Result<u64, ParseError> {
        let span = self.peek_span();
        let Token::Int(n) = self.peek().clone() else {
            return Err(ParseError::syntax(
                format!("{field_name} must be a duration literal like `5s` or `100ms`"),
                span,
            ));
        };
        self.advance();
        let (suffix, suffix_span) = self.expect_ident()?;
        let value = n
            .to_string()
            .parse::<u64>()
            .map_err(|_| ParseError::syntax("duration literal is too large", span.clone()))?;
        match suffix.as_str() {
            "ms" => Ok(value),
            "s" => value
                .checked_mul(1_000)
                .ok_or_else(|| ParseError::syntax("duration literal is too large", suffix_span)),
            _ => Err(ParseError::syntax(
                format!("{field_name} must use `ms` or `s`"),
                suffix_span,
            )),
        }
    }

    fn parse_supervisor_strategy(&mut self) -> Result<SupervisorStrategy, ParseError> {
        let (value, span) = self.expect_ident()?;
        match value.as_str() {
            "OneForOne" => Ok(SupervisorStrategy::OneForOne),
            _ => Err(ParseError::syntax(
                "supervisor strategy must be OneForOne",
                span,
            )),
        }
    }

    fn parse_child_restart_policy(&mut self) -> Result<ChildRestartPolicy, ParseError> {
        let (value, span) = self.expect_ident()?;
        match value.as_str() {
            "Permanent" => Ok(ChildRestartPolicy::Permanent),
            "Transient" => Ok(ChildRestartPolicy::Transient),
            "Temporary" => Ok(ChildRestartPolicy::Temporary),
            _ => Err(ParseError::syntax(
                "child_restart_default must be Permanent, Transient, or Temporary",
                span,
            )),
        }
    }

    fn parse_supervisor_init_handlers(
        &mut self,
    ) -> Result<Vec<SupervisorInitHandlerOverride>, ParseError> {
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let mut handlers = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let (slot, slot_span) = self.expect_ident()?;
            self.skip_newlines();
            self.expect(&Token::Colon)?;
            self.skip_newlines();
            let target = self.parse_supervisor_init_handler_target()?;
            if handlers
                .iter()
                .any(|entry: &SupervisorInitHandlerOverride| entry.slot == slot)
            {
                return Err(ParseError::syntax(
                    "handler override is duplicated",
                    slot_span,
                ));
            }
            handlers.push(SupervisorInitHandlerOverride {
                slot,
                span: Span {
                    start: slot_span.start,
                    end: target.span.end,
                },
                target,
            });
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(handlers)
    }

    fn parse_supervisor_init_handler_target(
        &mut self,
    ) -> Result<SupervisorInitHandlerTarget, ParseError> {
        let (name, name_span) = self.expect_ident()?;
        let mut end = name_span.end;
        let mut named_args = Vec::new();
        self.skip_newlines();
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            while !matches!(self.peek(), Token::RParen) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(")", self.peek_span()));
                }
                let (arg_name, arg_span) = self.expect_ident()?;
                self.skip_newlines();
                self.expect(&Token::Colon)?;
                self.skip_newlines();
                let value = self.parse_supervisor_init_handler_arg_value()?;
                named_args.push(SupervisorInitHandlerArg {
                    name: arg_name,
                    value,
                    span: arg_span,
                });
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                } else if !matches!(self.peek(), Token::RParen) {
                    return Err(ParseError::syntax(
                        "Expected `,` or `)` in handler target arguments",
                        self.peek_span(),
                    ));
                }
            }
            end = self.expect(&Token::RParen)?.end;
        }
        Ok(SupervisorInitHandlerTarget {
            name,
            named_args,
            span: Span {
                start: name_span.start,
                end,
            },
        })
    }

    fn parse_supervisor_init_handler_arg_value(&mut self) -> Result<String, ParseError> {
        let span = self.peek_span();
        match self.peek().clone() {
            Token::Str(value) => {
                self.advance();
                Ok(value)
            }
            Token::Int(value) => {
                self.advance();
                Ok(value.to_string())
            }
            Token::Ident(value) => {
                self.advance();
                Ok(value)
            }
            Token::Eof => Err(ParseError::incomplete("handler argument value", span)),
            _ => Err(ParseError::syntax(
                "handler argument values currently accept string, integer, or identifier literals",
                span,
            )),
        }
    }

    fn parse_defsupervisor_with_attrs(
        &mut self,
        dynamic: bool,
        attrs: DeclAttrs,
        start: usize,
    ) -> Result<Ast, ParseError> {
        let token = if dynamic {
            Token::DefdynamicSupervisor
        } else {
            Token::Defsupervisor
        };
        self.expect(&token)?;
        let (name, _) = self.expect_qualified_ident(2, "process")?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let supervisor_meta = self.parse_supervisor_meta_block()?;
        self.skip_newlines();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let err_span = self.peek_span();
            return Err(ParseError::syntax(
                "defsupervisor is policy-only; spawn, adopt, and status are compiler-managed",
                err_span,
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let span = Span {
            start,
            end: end.end,
        };
        let runtime_kind = if name == "DynamicSupervisor" || dynamic {
            ProcessKind::DynamicSupervisor
        } else {
            ProcessKind::Supervisor
        };
        let process_spec = ProcessSpec {
            process_name: name.clone(),
            kind: runtime_kind,
            instance: ProcessInstance::Singleton,
            state: AstTy::Named(span.clone(), "Unit".to_string()),
            boot: false,
            registry: true,
            lazy: false,
            handlers: Vec::new(),
            handler_specs: Vec::new(),
            supervisor_policy: Some(supervisor_meta.policy),
        };
        let mut body = Vec::new();
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
        body.push(build_supervisor_spawn_wrapper(&span, &name));
        body.push(build_supervisor_adopt_wrapper(&span, &name));
        body.push(build_supervisor_status_wrapper(&span, &name));
        body.push(build_supervisor_workers_wrapper(&span, &name));
        let span = Span {
            start,
            end: end.end,
        };
        if dynamic && name != "DynamicSupervisor" {
            Ok(Ast::DefdynamicSupervisor(
                span,
                name,
                body,
                process_spec,
                attrs,
            ))
        } else {
            Ok(Ast::Defsupervisor(span, name, body, process_spec, attrs))
        }
    }

    fn parse_supervisor_meta_block(&mut self) -> Result<SupervisorMeta, ParseError> {
        let (head, head_span) = self.expect_ident()?;
        if head != "meta" {
            return Err(ParseError::syntax(
                "supervisor declarations must start with `meta { ... }`",
                head_span,
            ));
        }
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut strategy = None;
        let mut max_restarts = None;
        let mut max_seconds = None;
        let mut child_restart_default = None;
        let mut allow_adopt = None;
        let mut shutdown_timeout_ms = None;

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let (key, key_span) = self.expect_ident()?;
            self.skip_newlines();
            self.expect(&Token::Colon)?;
            self.skip_newlines();
            match key.as_str() {
                "strategy" => strategy = Some(self.parse_supervisor_strategy()?),
                "max_restarts" => max_restarts = Some(self.parse_non_negative_int_literal()?),
                "max_seconds" => max_seconds = Some(self.parse_non_negative_int_literal()?),
                "child_restart_default" => {
                    child_restart_default = Some(self.parse_child_restart_policy()?)
                }
                "allow_adopt" => allow_adopt = Some(self.parse_bool_literal()?),
                "shutdown_timeout" => {
                    shutdown_timeout_ms = Some(self.parse_duration_literal_ms("shutdown_timeout")?)
                }
                "instance" | "init_policy" | "handlers" => {
                    return Err(ParseError::syntax(
                        "defsupervisor meta only accepts supervisor policy keys",
                        key_span,
                    ))
                }
                _ => {
                    return Err(ParseError::syntax(
                        format!("Unknown supervisor meta key: {key}"),
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

        Ok(SupervisorMeta {
            policy: SupervisorPolicy {
                strategy: strategy.ok_or_else(|| {
                    ParseError::syntax("meta requires strategy", self.peek_span())
                })?,
                max_restarts: max_restarts.ok_or_else(|| {
                    ParseError::syntax("meta requires max_restarts", self.peek_span())
                })?,
                max_seconds: max_seconds.ok_or_else(|| {
                    ParseError::syntax("meta requires max_seconds", self.peek_span())
                })?,
                child_restart_default: child_restart_default.ok_or_else(|| {
                    ParseError::syntax("meta requires child_restart_default", self.peek_span())
                })?,
                allow_adopt: allow_adopt.ok_or_else(|| {
                    ParseError::syntax("meta requires allow_adopt", self.peek_span())
                })?,
                shutdown_timeout_ms,
            },
        })
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
        let lbrace_span = self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let meta_span = Span {
            start: head_span.start,
            end: lbrace_span.end,
        };

        let mut instance = None;
        let mut init_policy = None;
        let mut state = None;
        let mut handlers = Vec::new();

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
                "state" => {
                    self.expect(&Token::Colon)?;
                    self.skip_newlines();
                    state = Some(self.parse_type()?);
                }
                "handlers" => {
                    handlers = self.parse_process_meta_handlers()?;
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
                .ok_or_else(|| ParseError::syntax("meta requires instance", meta_span.clone()))?,
            init_policy: init_policy.unwrap_or(InitPolicy::Eager),
            state: state.ok_or_else(|| ParseError::syntax("meta requires state", meta_span))?,
            handlers,
        })
    }

    fn parse_process_meta_handlers(&mut self) -> Result<Vec<ProcessHandlerDependency>, ParseError> {
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let mut handlers = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let (slot, slot_span) = self.expect_ident()?;
            self.skip_newlines();
            self.expect(&Token::Colon)?;
            self.skip_newlines();
            let (capability, _) = self.expect_ident()?;
            self.skip_newlines();
            self.expect(&Token::Bind)?;
            self.skip_newlines();
            let (target_name, target_span) = self.expect_ident()?;
            if handlers
                .iter()
                .any(|entry: &ProcessHandlerDependency| entry.slot == slot)
            {
                return Err(ParseError::syntax("handler slot is duplicated", slot_span));
            }
            handlers.push(ProcessHandlerDependency {
                slot,
                capability,
                default_target: ProcessHandlerTarget {
                    name: target_name,
                    span: target_span.clone(),
                },
                span: Span {
                    start: slot_span.start,
                    end: target_span.end,
                },
            });
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(handlers)
    }

    fn parse_defagent(
        &mut self,
        meta: Option<AgentMeta>,
        attrs: DeclAttrs,
        start: usize,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Defagent)?;
        let (name, _name_span) = self.expect_qualified_ident(2, "process")?;
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
            let (marker, member_attrs) = self.parse_agent_member_prefixes()?;
            if marker.is_some() && !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "Agent handler marker must be followed by def inside defagent",
                    self.peek_span(),
                ));
            }
            let def = self.parse_def_with_attrs(member_attrs, None)?;
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
                None => {
                    let def = make_process_helper_private(def);
                    let attrs = ast_decl_attrs(&def)
                        .expect("process helper lowering always produces a declaration");
                    validate_doc_visibility(attrs, def.span())?;
                    helpers.push(def);
                }
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
                state: process_meta.state,
                boot: false,
                registry: process_meta.instance == AgentInstance::Singleton,
                lazy: process_meta.init_policy == InitPolicy::Lazy,
                handlers: process_meta.handlers,
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
        attrs: DeclAttrs,
        start: usize,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Defgenserver)?;
        let (name, _) = self.expect_qualified_ident(2, "process")?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let process_meta = self.parse_process_meta_block()?;
        self.skip_newlines();

        let mut init = None;
        let mut call_handlers: Vec<(String, AgentHandler)> = Vec::new();
        let mut cast_handlers: Vec<(String, AgentHandler)> = Vec::new();
        let mut helpers = Vec::new();

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let (marker, member_attrs) = self.parse_genserver_member_prefixes()?;

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
            let def = self.parse_def_with_attrs(member_attrs, None)?;
            self.ensure_stmt_boundary(&def, true)?;
            match marker {
                Some((marker, marker_span)) if marker == "init" => {
                    if init.is_some() {
                        return Err(ParseError::syntax("duplicate @init handler", marker_span));
                    }
                    init = Some(AgentHandler { def });
                }
                Some((marker, marker_span)) if marker == "call" => {
                    let _ = marker_span;
                    let wrapper_name = def_name(&def)?;
                    call_handlers.push((wrapper_name, AgentHandler { def }));
                }
                Some((marker, marker_span)) if marker == "cast" => {
                    let _ = marker_span;
                    let wrapper_name = def_name(&def)?;
                    cast_handlers.push((wrapper_name, AgentHandler { def }));
                }
                Some((_, marker_span)) => {
                    return Err(ParseError::syntax(
                        "GenServer handler marker must be @init, @call, or @cast",
                        marker_span,
                    ));
                }
                None => {
                    let def = make_process_helper_private(def);
                    let attrs = ast_decl_attrs(&def)
                        .expect("process helper lowering always produces a declaration");
                    validate_doc_visibility(attrs, def.span())?;
                    helpers.push(def);
                }
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
        if call_handlers.is_empty() {
            return Err(ParseError::syntax(
                "GenServer requires at least one @call handler",
                Span {
                    start,
                    end: end.end,
                },
            ));
        }
        ensure_no_compiler_managed_process_surface_names(&helpers, &name)?;
        for (call_name, call_handler) in &call_handlers {
            ensure_process_surface_name_not_reserved(call_name, call_handler.def.span(), &name)?;
        }
        for (cast_name, cast_handler) in &cast_handlers {
            ensure_process_surface_name_not_reserved(cast_name, cast_handler.def.span(), &name)?;
        }

        let init_name = def_name(&init.def)?;
        let init_def = rename_agent_handler(init.def, "__agent_init", &name, false)?;
        let mut lowered_calls = Vec::new();
        for (idx, (call_name, call_handler)) in call_handlers.into_iter().enumerate() {
            let internal_name = if idx == 0 {
                "__agent_get".to_string()
            } else {
                format!("__agent_call_{call_name}")
            };
            let call_def = rename_agent_handler(call_handler.def, &internal_name, &name, true)?;
            lowered_calls.push((call_name, internal_name, call_def));
        }
        let mut lowered_casts = Vec::new();
        for (idx, (cast_name, cast_handler)) in cast_handlers.into_iter().enumerate() {
            let internal_name = if idx == 0 {
                "__agent_set".to_string()
            } else {
                format!("__agent_cast_{cast_name}")
            };
            let cast_def = rename_agent_handler(cast_handler.def, &internal_name, &name, true)?;
            lowered_casts.push((cast_name, internal_name, cast_def));
        }

        let mut body = vec![init_def.clone()];
        body.extend(lowered_calls.iter().map(|(_, _, def)| def.clone()));
        body.extend(lowered_casts.iter().map(|(_, _, def)| def.clone()));
        body.extend(helpers);
        for (call_name, internal_name, call_def) in &lowered_calls {
            body.push(build_genserver_call_wrapper(
                &span,
                &name,
                call_name,
                internal_name,
                call_def,
                process_meta.instance == AgentInstance::Singleton,
            )?);
        }
        for (cast_name, internal_name, cast_def) in &lowered_casts {
            body.push(build_genserver_cast_wrapper(
                &span,
                &name,
                cast_name,
                internal_name,
                cast_def,
                process_meta.instance == AgentInstance::Singleton,
            )?);
        }

        if process_meta.instance == AgentInstance::Worker {
            body.push(build_worker_init_route_wrapper(
                &span, &name, &init_name, &init_def,
            )?);
        } else {
            body.push(build_pid_wrapper(&span, "GenServer", &name));
            body.push(build_singleton_init_route_wrapper(
                &span,
                "GenServer",
                &name,
                &init_name,
            ));
        }

        let process_spec =
            ProcessSpec {
                process_name: name.clone(),
                kind: ProcessKind::GenServer,
                instance: process_meta.instance.into_process_instance(),
                state: process_meta.state,
                boot: false,
                registry: process_meta.instance == AgentInstance::Singleton,
                lazy: process_meta.init_policy == InitPolicy::Lazy,
                handlers: process_meta.handlers,
                handler_specs: {
                    let mut specs = vec![ProcessRuntimeHandlerSpec {
                        name: init_name.clone(),
                        internal_name: "__agent_init".to_string(),
                        kind: ProcessRuntimeHandlerKind::Init,
                        span: init_def.span().clone(),
                    }];
                    specs.extend(lowered_calls.iter().map(
                        |(call_name, internal_name, call_def)| ProcessRuntimeHandlerSpec {
                            name: call_name.clone(),
                            internal_name: internal_name.clone(),
                            kind: ProcessRuntimeHandlerKind::Call,
                            span: call_def.span().clone(),
                        },
                    ));
                    specs.extend(lowered_casts.iter().map(
                        |(cast_name, internal_name, cast_def)| ProcessRuntimeHandlerSpec {
                            name: cast_name.clone(),
                            internal_name: internal_name.clone(),
                            kind: ProcessRuntimeHandlerKind::Cast,
                            span: cast_def.span().clone(),
                        },
                    ));
                    specs
                },
                supervisor_policy: None,
            };
        Ok(Ast::Defgenserver(span, name, body, process_spec, attrs))
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
        attrs: DeclAttrs,
        meta: AgentMeta,
        init: AgentHandler,
        get: AgentHandler,
        set: Option<AgentHandler>,
        helpers: Vec<Ast>,
    ) -> Result<Ast, ParseError> {
        let mut body = Vec::new();
        let init_name = def_name(&init.def)?;
        let get_name = def_name(&get.def)?;
        let set_name = set
            .as_ref()
            .map(|handler| def_name(&handler.def))
            .transpose()?;
        let get_surface_span = get.def.span().clone();
        let set_surface_span = set.as_ref().map(|handler| handler.def.span().clone());
        let init_def = rename_agent_handler(init.def, "__agent_init", &name, false)?;
        let get_def = rename_agent_handler(get.def, "__agent_get", &name, true)?;
        let set_def = set
            .map(|handler| rename_agent_handler(handler.def, "__agent_set", &name, true))
            .transpose()?;
        ensure_no_compiler_managed_process_surface_names(&helpers, &name)?;
        ensure_process_surface_name_not_reserved(&get_name, &get_surface_span, &name)?;
        if let (Some(set_name), Some(set_span)) = (&set_name, set_surface_span.as_ref()) {
            ensure_process_surface_name_not_reserved(set_name, set_span, &name)?;
        }

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
                body.push(build_readonly_get_wrapper(
                    &span, &name, &get_name, &get_def,
                )?);
            }
            AgentKind::State => {
                if meta.instance == AgentInstance::Worker {
                    body.push(build_worker_init_route_wrapper(
                        &span, &name, &init_name, &init_def,
                    )?);
                } else {
                    body.push(build_pid_wrapper(&span, "Agent", &name));
                    body.push(build_singleton_init_route_wrapper(
                        &span, "Agent", &name, &init_name,
                    ));
                }
                let singleton = meta.instance == AgentInstance::Singleton;
                body.push(build_state_get_wrapper(
                    &span, &name, &get_name, &get_def, singleton,
                )?);
                if let Some(set_def) = &set_def {
                    body.push(build_state_set_wrapper(
                        &span,
                        &name,
                        set_name.as_deref().expect("@set name should exist"),
                        set_def,
                        singleton,
                    )?);
                }
            }
        }

        let mut process_spec = meta.into_process_spec(name.clone());
        process_spec.handler_specs = {
            let mut specs = vec![
                ProcessRuntimeHandlerSpec {
                    name: init_name,
                    internal_name: "__agent_init".to_string(),
                    kind: ProcessRuntimeHandlerKind::Init,
                    span: init_def.span().clone(),
                },
                ProcessRuntimeHandlerSpec {
                    name: get_name,
                    internal_name: "__agent_get".to_string(),
                    kind: ProcessRuntimeHandlerKind::Get,
                    span: get_def.span().clone(),
                },
            ];
            if let (Some(name), Some(set_def)) = (set_name, set_def.as_ref()) {
                specs.push(ProcessRuntimeHandlerSpec {
                    name,
                    internal_name: "__agent_set".to_string(),
                    kind: ProcessRuntimeHandlerKind::Set,
                    span: set_def.span().clone(),
                });
            }
            specs
        };
        Ok(Ast::Defagent(span, name, body, process_spec, attrs))
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
        validate_doc_visibility(
            &attrs,
            &Span {
                start: annotator_start.unwrap_or(sp.start),
                end: sp.end,
            },
        )?;

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

        let ast = Ast::Def(
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
        );
        Ok(ast)
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
