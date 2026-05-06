use super::captures::collect_captures;
use super::declarations::trait_instance_key;
use super::scope_init::{
    initialize_scope, is_doc_only_builtin_decl, is_runtime_builtin_decl,
    is_special_form_builtin_decl, resolve_decl_attrs,
};
use super::special_forms::{IfKind, LogicKind};
use super::*;
use spire::ast::{BinOp, DbgArg, InterpolatedPart};

const TUPLE_TYPE_ROOT_UID: u32 = u32::MAX - 7;

impl Resolver {
    fn partial_pipeline_special_form_arity(name: &str) -> Option<usize> {
        match name {
            "if" => Some(3),
            "if_then" => Some(2),
            "if_let" => Some(4),
            "if_let_then" => Some(3),
            "is_match" => Some(2),
            "assert" => Some(2),
            "ensure" => Some(3),
            "map_err" | "cause" => Some(2),
            "and" | "or" => Some(2),
            _ => None,
        }
    }

    fn desugar_pipeline_rhs_special_form_partial(&self, rhs: Ast) -> Ast {
        let Ast::App(span, func, args) = rhs else {
            return rhs;
        };

        let Ast::Var(_, ref name) = *func else {
            return Ast::App(span, func, args);
        };
        let Some(expected_arity) = Self::partial_pipeline_special_form_arity(name) else {
            return Ast::App(span, func, args);
        };
        if args.len() + 1 != expected_arity {
            return Ast::App(span, func, args);
        }

        let param_name = format!("__pipe_injected_{}_{}", span.start, span.end);
        let param_span = span.clone();
        let mut injected_args = Vec::with_capacity(args.len() + 1);
        injected_args.push(RecordLitArg::Positional(Ast::Var(
            param_span.clone(),
            param_name.clone(),
        )));
        injected_args.extend(args);

        let call = Ast::App(span.clone(), func, injected_args);
        Ast::Closure(
            span.clone(),
            vec![ClosureParam {
                name: param_name,
                ty: None,
                span: param_span,
            }],
            Box::new(call),
        )
    }

    fn capture_placeholder_param_name(span: &Span, index: usize) -> String {
        format!("__cap_{}_{}_{}", span.start, span.end, index)
    }

    fn pipe_slot_param_name(span: &Span) -> String {
        format!("__pipe_slot_{}_{}", span.start, span.end)
    }

    fn make_closure_from_call(
        &self,
        span: &Span,
        params: Vec<ClosureParam>,
        func: Ast,
        args: Vec<Ast>,
    ) -> Ast {
        Ast::Closure(
            span.clone(),
            params,
            Box::new(Ast::App(
                span.clone(),
                Box::new(func),
                args.into_iter().map(RecordLitArg::Positional).collect(),
            )),
        )
    }

    fn make_operator_capture_body(
        &self,
        span: &Span,
        body: &str,
        left: Ast,
        right: Ast,
    ) -> Result<Ast, ResolveError> {
        let op = match body {
            "+" => BinOp::Add,
            "-" => BinOp::Sub,
            "*" => BinOp::Mul,
            "++" => BinOp::Concat,
            "==" => BinOp::Eq,
            "!=" => BinOp::Neq,
            "<" => BinOp::Lt,
            ">" => BinOp::Gt,
            "<=" => BinOp::Lte,
            ">=" => BinOp::Gte,
            _ => {
                return Err(ResolveError {
                    message: format!("unsupported operator capture target `{}`", body),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
        };
        Ok(Ast::BinOp(
            span.clone(),
            op,
            Box::new(left),
            Box::new(right),
        ))
    }

    fn validate_capture_placeholders(
        &self,
        span: &Span,
        args: &[Ast],
    ) -> Result<usize, ResolveError> {
        let mut used = HashSet::new();
        for arg in args {
            self.collect_capture_placeholders(arg, true, true, &mut used)?;
        }
        if used.is_empty() {
            return Err(ResolveError {
                message: "capture call is missing placeholder arguments".into(),
                span: span.clone(),
                related_labels: Vec::new(),
            });
        }

        let max_index = *used.iter().max().expect("used is not empty");
        for index in 1..=max_index {
            if !used.contains(&index) {
                return Err(ResolveError {
                    message: format!("capture placeholder &{} is missing", index),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
        }
        Ok(max_index)
    }

    fn collect_capture_placeholders(
        &self,
        expr: &Ast,
        allow_placeholders: bool,
        inside_placeholder_capture: bool,
        used: &mut HashSet<usize>,
    ) -> Result<(), ResolveError> {
        match expr {
            Ast::CapturePlaceholder(span, index) => {
                if !allow_placeholders {
                    return Err(ResolveError {
                        message: "capture placeholders are only valid in the outer capture body"
                            .into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                }
                used.insert(*index);
                Ok(())
            }
            Ast::App(_, func, args) => {
                self.collect_capture_placeholders(
                    func,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                for arg in args {
                    match arg {
                        RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                            self.collect_capture_placeholders(
                                expr,
                                allow_placeholders,
                                inside_placeholder_capture,
                                used,
                            )?;
                        }
                    }
                }
                Ok(())
            }
            Ast::Block(_, stmts) | Ast::ListLiteral(_, stmts) | Ast::TupleLiteral(_, stmts) => {
                for stmt in stmts {
                    self.collect_capture_placeholders(
                        stmt,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::RangeLiteral(_, start, stop) => {
                self.collect_capture_placeholders(
                    start,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                self.collect_capture_placeholders(
                    stop,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )
            }
            Ast::Bind(_, _, rhs)
            | Ast::SafeBind(_, _, rhs)
            | Ast::Grouped(_, rhs)
            | Ast::Semi(_, rhs)
            | Ast::FieldAccess(_, rhs, _) => self.collect_capture_placeholders(
                rhs,
                allow_placeholders,
                inside_placeholder_capture,
                used,
            ),
            Ast::BinOp(_, _, left, right)
            | Ast::Pipe(_, left, right)
            | Ast::ContextMap(_, left, right)
            | Ast::ContextBind(_, left, right)
            | Ast::Compose(_, left, right)
            | Ast::LiftedCompose(_, left, right)
            | Ast::KleisliCompose(_, left, right)
            | Ast::ListCons(_, left, right) => {
                self.collect_capture_placeholders(
                    left,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                self.collect_capture_placeholders(
                    right,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )
            }
            Ast::InterpolatedStr(_, parts) => {
                for part in parts {
                    if let InterpolatedPart::Expr(expr) = part {
                        self.collect_capture_placeholders(
                            expr,
                            allow_placeholders,
                            inside_placeholder_capture,
                            used,
                        )?;
                    }
                }
                Ok(())
            }
            Ast::Dbg(_, args) => {
                for arg in args {
                    self.collect_capture_placeholders(
                        &arg.expr,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::Match(_, scrutinee, arms) => {
                self.collect_capture_placeholders(
                    scrutinee,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_capture_placeholders(
                            guard,
                            allow_placeholders,
                            inside_placeholder_capture,
                            used,
                        )?;
                    }
                    self.collect_capture_placeholders(
                        &arm.body,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::StructLit(_, _, fields) | Ast::InternalStructLit(_, _, fields) => {
                for field in fields {
                    match field {
                        StructLitField::Explicit(_, expr) => {
                            self.collect_capture_placeholders(
                                expr,
                                allow_placeholders,
                                inside_placeholder_capture,
                                used,
                            )?;
                        }
                        StructLitField::Shorthand(_) => {}
                    }
                }
                Ok(())
            }
            Ast::ConstructorCall(_, _, args) => {
                for arg in args {
                    match arg {
                        RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                            self.collect_capture_placeholders(
                                expr,
                                allow_placeholders,
                                inside_placeholder_capture,
                                used,
                            )?;
                        }
                    }
                }
                Ok(())
            }
            Ast::Closure(_, _, body) => self.collect_capture_placeholders(body, false, true, used),
            Ast::Capture(span, target, args) => {
                if inside_placeholder_capture && !args.is_empty() {
                    return Err(ResolveError {
                        message: "outer capture placeholders are only valid in the outer capture body; nested capture argument blocks are not allowed".into(),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
                }
                self.collect_capture_placeholders(
                    target,
                    allow_placeholders,
                    inside_placeholder_capture,
                    used,
                )?;
                for arg in args {
                    self.collect_capture_placeholders(
                        arg,
                        allow_placeholders,
                        inside_placeholder_capture,
                        used,
                    )?;
                }
                Ok(())
            }
            Ast::Lit(_, _)
            | Ast::Var(_, _)
            | Ast::InternalVar(_, _)
            | Ast::Path(_, _)
            | Ast::FuncLiteralRef(_, _)
            | Ast::ListNil(_)
            | Ast::StructDef(..)
            | Ast::RecordDef(..)
            | Ast::DeferrorDef(_, _, _, _, _)
            | Ast::EnumDef(_, _, _, _, _)
            | Ast::Def(_, _, _, _, _, _, _)
            | Ast::ConstDef(_, _, _, _, _)
            | Ast::SupervisorInit(_, _)
            | Ast::ExtractorDef(_, _, _, _, _, _, _)
            | Ast::BuiltinDecl(_, _, _, _, _)
            | Ast::IntrinsicDecl(_, _, _, _)
            | Ast::BuiltinExtractorDecl(_, _, _, _, _)
            | Ast::BuiltinTypeDecl(_, _, _)
            | Ast::ResultCtorDecl(_, _, _, _, _)
            | Ast::Defmod(_, _, _, _)
            | Ast::Defagent(_, _, _, _, _)
            | Ast::Defgenserver(_, _, _, _, _)
            | Ast::Defsupervisor(_, _, _, _, _)
            | Ast::DefdynamicSupervisor(_, _, _, _, _)
            | Ast::Namespace(_, _, _)
            | Ast::ImplDef(_, _, _, _)
            | Ast::TraitDef(_, _, _, _, _)
            | Ast::TraitImplDef(_, _, _, _, _, _)
            | Ast::Import(_, _, _)
            | Ast::Include(_, _) => Ok(()),
        }
    }

    fn rewrite_capture_placeholders(
        &self,
        expr: Ast,
        capture_span: &Span,
        allow_placeholders: bool,
        inside_placeholder_capture: bool,
    ) -> Result<Ast, ResolveError> {
        match expr {
            Ast::CapturePlaceholder(span, index) => {
                if !allow_placeholders {
                    return Err(ResolveError {
                        message: "capture placeholders are only valid in the outer capture body"
                            .into(),
                        span,
                        related_labels: Vec::new(),
                    });
                }
                Ok(Ast::Var(
                    span.clone(),
                    Self::capture_placeholder_param_name(capture_span, index),
                ))
            }
            Ast::App(span, func, args) => Ok(Ast::App(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *func,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                args.into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => Ok(RecordLitArg::Positional(
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        RecordLitArg::Named(name, expr) => Ok(RecordLitArg::Named(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Block(span, stmts) => Ok(Ast::Block(
                span,
                stmts
                    .into_iter()
                    .map(|stmt| {
                        self.rewrite_capture_placeholders(
                            stmt,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Bind(span, pat, rhs) => Ok(Ast::Bind(
                span,
                pat,
                Box::new(self.rewrite_capture_placeholders(
                    *rhs,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::SafeBind(span, pat, rhs) => Ok(Ast::SafeBind(
                span,
                pat,
                Box::new(self.rewrite_capture_placeholders(
                    *rhs,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::BinOp(span, op, left, right) => Ok(Ast::BinOp(
                span,
                op,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::Pipe(span, left, right) => Ok(Ast::Pipe(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ContextMap(span, left, right) => Ok(Ast::ContextMap(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ContextBind(span, left, right) => Ok(Ast::ContextBind(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::Compose(span, left, right) => Ok(Ast::Compose(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::LiftedCompose(span, left, right) => Ok(Ast::LiftedCompose(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::KleisliCompose(span, left, right) => Ok(Ast::KleisliCompose(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ListCons(span, left, right) => Ok(Ast::ListCons(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *left,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *right,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::ListLiteral(span, elems) => Ok(Ast::ListLiteral(
                span,
                elems
                    .into_iter()
                    .map(|elem| {
                        self.rewrite_capture_placeholders(
                            elem,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::RangeLiteral(span, start, stop) => Ok(Ast::RangeLiteral(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *start,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                Box::new(self.rewrite_capture_placeholders(
                    *stop,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::TupleLiteral(span, elems) => Ok(Ast::TupleLiteral(
                span,
                elems
                    .into_iter()
                    .map(|elem| {
                        self.rewrite_capture_placeholders(
                            elem,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Grouped(span, inner) => Ok(Ast::Grouped(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *inner,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
            )),
            Ast::InterpolatedStr(span, parts) => Ok(Ast::InterpolatedStr(
                span,
                parts
                    .into_iter()
                    .map(|part| match part {
                        InterpolatedPart::Text(text) => Ok(InterpolatedPart::Text(text)),
                        InterpolatedPart::Expr(expr) => Ok(InterpolatedPart::Expr(Box::new(
                            self.rewrite_capture_placeholders(
                                *expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        ))),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Dbg(span, args) => Ok(Ast::Dbg(
                span,
                args.into_iter()
                    .map(|arg| {
                        let expr = self.rewrite_capture_placeholders(
                            arg.expr,
                            capture_span,
                            allow_placeholders,
                            inside_placeholder_capture,
                        )?;
                        Ok(DbgArg {
                            span: expr.span().clone(),
                            expr,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::Match(span, scrutinee, arms) => Ok(Ast::Match(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *scrutinee,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                arms.into_iter()
                    .map(|arm| {
                        Ok(AstMatchArm {
                            pattern: arm.pattern,
                            guard: arm
                                .guard
                                .map(|guard| {
                                    self.rewrite_capture_placeholders(
                                        guard,
                                        capture_span,
                                        allow_placeholders,
                                        inside_placeholder_capture,
                                    )
                                })
                                .transpose()?,
                            body: self.rewrite_capture_placeholders(
                                arm.body,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::FieldAccess(span, expr, field) => Ok(Ast::FieldAccess(
                span,
                Box::new(self.rewrite_capture_placeholders(
                    *expr,
                    capture_span,
                    allow_placeholders,
                    inside_placeholder_capture,
                )?),
                field,
            )),
            Ast::StructLit(span, name, fields) => Ok(Ast::StructLit(
                span,
                name,
                fields
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(StructLitField::Explicit(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        StructLitField::Shorthand(name) => Ok(StructLitField::Shorthand(name)),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::InternalStructLit(span, name, fields) => Ok(Ast::InternalStructLit(
                span,
                name,
                fields
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(StructLitField::Explicit(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        StructLitField::Shorthand(name) => Ok(StructLitField::Shorthand(name)),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            Ast::ConstructorCall(span, name, args) => Ok(Ast::ConstructorCall(
                span,
                name,
                args.into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => Ok(RecordLitArg::Positional(
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                        RecordLitArg::Named(name, expr) => Ok(RecordLitArg::Named(
                            name,
                            self.rewrite_capture_placeholders(
                                expr,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )?,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            Ast::Closure(span, params, body) => Ok(Ast::Closure(
                span,
                params,
                Box::new(self.rewrite_capture_placeholders(*body, capture_span, false, true)?),
            )),
            Ast::Capture(span, target, args) => {
                if inside_placeholder_capture && !args.is_empty() {
                    return Err(ResolveError {
                        message: "outer capture placeholders are only valid in the outer capture body; nested capture argument blocks are not allowed".into(),
                        span,
                        related_labels: Vec::new(),
                    });
                }
                Ok(Ast::Capture(
                    span,
                    Box::new(self.rewrite_capture_placeholders(
                        *target,
                        capture_span,
                        allow_placeholders,
                        inside_placeholder_capture,
                    )?),
                    args.into_iter()
                        .map(|arg| {
                            self.rewrite_capture_placeholders(
                                arg,
                                capture_span,
                                allow_placeholders,
                                inside_placeholder_capture,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
            other => Ok(other),
        }
    }

    fn lower_capture_expr(
        &self,
        span: Span,
        target: Ast,
        args: Vec<Ast>,
    ) -> Result<Ast, ResolveError> {
        if let Ast::FuncLiteralRef(_, func) = target {
            if args.is_empty() {
                let left_name = Self::capture_placeholder_param_name(&span, 1);
                let right_name = Self::capture_placeholder_param_name(&span, 2);
                let body = self.make_operator_capture_body(
                    &span,
                    &func.body,
                    Ast::Var(span.clone(), left_name.clone()),
                    Ast::Var(span.clone(), right_name.clone()),
                )?;
                return Ok(Ast::Closure(
                    span.clone(),
                    vec![
                        ClosureParam {
                            name: left_name,
                            ty: None,
                            span: span.clone(),
                        },
                        ClosureParam {
                            name: right_name,
                            ty: None,
                            span: span.clone(),
                        },
                    ],
                    Box::new(body),
                ));
            }

            if args.len() != 2 {
                return Err(ResolveError {
                    message: format!(
                        "operator capture `{}` expects exactly 2 argument expressions",
                        func.body
                    ),
                    span,
                    related_labels: Vec::new(),
                });
            }

            let max_index = self.validate_capture_placeholders(&span, &args)?;
            let rewritten_args = args
                .into_iter()
                .map(|arg| self.rewrite_capture_placeholders(arg, &span, true, true))
                .collect::<Result<Vec<_>, _>>()?;
            let mut rewritten_args = rewritten_args.into_iter();
            let left = rewritten_args.next().expect("checked len == 2");
            let right = rewritten_args.next().expect("checked len == 2");
            let body = self.make_operator_capture_body(&span, &func.body, left, right)?;
            let params = (1..=max_index)
                .map(|index| ClosureParam {
                    name: Self::capture_placeholder_param_name(&span, index),
                    ty: None,
                    span: span.clone(),
                })
                .collect();
            return Ok(Ast::Closure(span, params, Box::new(body)));
        }

        if args.is_empty() {
            return Ok(Ast::Capture(span, Box::new(target), args));
        }

        let max_index = self.validate_capture_placeholders(&span, &args)?;

        let rewritten_args = args
            .into_iter()
            .map(|arg| self.rewrite_capture_placeholders(arg, &span, true, true))
            .collect::<Result<Vec<_>, _>>()?;
        let params = (1..=max_index)
            .map(|index| ClosureParam {
                name: Self::capture_placeholder_param_name(&span, index),
                ty: None,
                span: span.clone(),
            })
            .collect();
        Ok(self.make_closure_from_call(&span, params, target, rewritten_args))
    }

    fn pipe_slot_span(expr: &Ast) -> Option<Span> {
        match expr {
            Ast::Var(span, name) if name == "_1" => Some(span.clone()),
            Ast::App(_, func, args) => Self::pipe_slot_span(func).or_else(|| {
                args.iter().find_map(|arg| match arg {
                    RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                        Self::pipe_slot_span(expr)
                    }
                })
            }),
            Ast::Block(_, stmts) | Ast::ListLiteral(_, stmts) | Ast::TupleLiteral(_, stmts) => {
                stmts.iter().find_map(Self::pipe_slot_span)
            }
            Ast::RangeLiteral(_, start, stop) => {
                Self::pipe_slot_span(start).or_else(|| Self::pipe_slot_span(stop))
            }
            Ast::Bind(_, _, rhs)
            | Ast::SafeBind(_, _, rhs)
            | Ast::Grouped(_, rhs)
            | Ast::Semi(_, rhs)
            | Ast::FieldAccess(_, rhs, _) => Self::pipe_slot_span(rhs),
            Ast::BinOp(_, _, left, right)
            | Ast::Pipe(_, left, right)
            | Ast::ContextMap(_, left, right)
            | Ast::ContextBind(_, left, right)
            | Ast::Compose(_, left, right)
            | Ast::LiftedCompose(_, left, right)
            | Ast::KleisliCompose(_, left, right)
            | Ast::ListCons(_, left, right) => {
                Self::pipe_slot_span(left).or_else(|| Self::pipe_slot_span(right))
            }
            Ast::InterpolatedStr(_, parts) => parts.iter().find_map(|part| match part {
                InterpolatedPart::Text(_) => None,
                InterpolatedPart::Expr(expr) => Self::pipe_slot_span(expr),
            }),
            Ast::Match(_, scrutinee, arms) => Self::pipe_slot_span(scrutinee).or_else(|| {
                arms.iter().find_map(|arm| {
                    arm.guard
                        .as_ref()
                        .and_then(Self::pipe_slot_span)
                        .or_else(|| Self::pipe_slot_span(&arm.body))
                })
            }),
            Ast::StructLit(_, _, fields) | Ast::InternalStructLit(_, _, fields) => {
                fields.iter().find_map(|field| match field {
                    StructLitField::Explicit(_, expr) => Self::pipe_slot_span(expr),
                    StructLitField::Shorthand(_) => None,
                })
            }
            Ast::ConstructorCall(_, _, args) => args.iter().find_map(|arg| match arg {
                RecordLitArg::Positional(expr) | RecordLitArg::Named(_, expr) => {
                    Self::pipe_slot_span(expr)
                }
            }),
            Ast::Closure(_, _, body) => Self::pipe_slot_span(body),
            Ast::Capture(_, target, args) => {
                Self::pipe_slot_span(target).or_else(|| args.iter().find_map(Self::pipe_slot_span))
            }
            Ast::FuncLiteralRef(_, _) => None,
            _ => None,
        }
    }

    fn lower_pipe_rhs_slots(&self, rhs: Ast) -> Result<Ast, ResolveError> {
        let Ast::App(span, func, args) = rhs else {
            if let Some(slot_span) = Self::pipe_slot_span(&rhs) {
                return Err(ResolveError {
                    message: "pipe placeholder `_1` is only allowed as a direct argument of the outermost call on the right-hand side".into(),
                    span: slot_span,
                    related_labels: Vec::new(),
                });
            }
            return Ok(rhs);
        };

        let mut slot_count = 0usize;
        let mut lowered_args = Vec::with_capacity(args.len());
        let mut positional_only = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(Ast::Var(arg_span, name)) if name == "_1" => {
                    slot_count += 1;
                    let lowered = Ast::Var(arg_span.clone(), Self::pipe_slot_param_name(&span));
                    lowered_args.push(lowered.clone());
                    positional_only.push(RecordLitArg::Positional(lowered));
                }
                RecordLitArg::Positional(expr) => {
                    if let Some(slot_span) = Self::pipe_slot_span(&expr) {
                        return Err(ResolveError {
                            message: "pipe placeholder `_1` cannot be used as an expression".into(),
                            span: slot_span,
                            related_labels: Vec::new(),
                        });
                    }
                    lowered_args.push(expr.clone());
                    positional_only.push(RecordLitArg::Positional(expr));
                }
                RecordLitArg::Named(name, expr) => {
                    if let Some(slot_span) = Self::pipe_slot_span(&expr) {
                        return Err(ResolveError {
                            message: "pipe placeholder `_1` cannot be used as an expression".into(),
                            span: slot_span,
                            related_labels: Vec::new(),
                        });
                    }
                    if slot_count > 0 {
                        return Err(ResolveError {
                            message: "pipe placeholder `_1` does not support named arguments"
                                .into(),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    positional_only.push(RecordLitArg::Named(name, expr));
                }
            }
        }

        if slot_count == 0 {
            return Ok(Ast::App(span, func, positional_only));
        }
        if slot_count > 1 {
            return Err(ResolveError {
                message: "pipe placeholder `_1` can only be used once".into(),
                span,
                related_labels: Vec::new(),
            });
        }

        Ok(self.make_closure_from_call(
            &span,
            vec![ClosureParam {
                name: Self::pipe_slot_param_name(&span),
                ty: None,
                span: span.clone(),
            }],
            *func,
            lowered_args,
        ))
    }

    fn prepare_pipe_rhs(&self, rhs: Ast) -> Result<Ast, ResolveError> {
        let rhs = self.lower_pipe_rhs_slots(rhs)?;
        Ok(self.desugar_pipeline_rhs_special_form_partial(rhs))
    }

    fn conversion_call_head(func: &Ast) -> Option<&'static str> {
        match func {
            Ast::Var(_, name) if name == "from" => Some("from"),
            Ast::Var(_, name) if name == "try_from" => Some("try_from"),
            Ast::Path(_, path) if path.segments.len() >= 2 => {
                let method = path.segments.last()?;
                let owner = path.segments.get(path.segments.len() - 2)?;
                match (owner.as_str(), method.as_str()) {
                    ("From", "from") => Some("from"),
                    ("TryFrom", "try_from") => Some("try_from"),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn undefined_callable_arity_message(func: &Ast, arity: usize) -> Option<String> {
        match func {
            Ast::Var(_, name) => Some(format!("Undefined function {}/{}", name, arity)),
            Ast::Path(_, path) => Some(format!(
                "Undefined function {}/{}",
                path.segments.join("::"),
                arity
            )),
            _ => None,
        }
    }

    fn map_undefined_callable_error(err: ResolveError, func: &Ast, arity: usize) -> ResolveError {
        match func {
            Ast::Var(_, name) if err.message == format!("Undefined variable: {}", name) => {
                ResolveError {
                    message: Self::undefined_callable_arity_message(func, arity)
                        .unwrap_or_else(|| format!("Undefined variable or function: {}", name)),
                    span: err.span,
                    related_labels: Vec::new(),
                }
            }
            Ast::Path(_, path)
                if err.message == format!("Undefined variable: {}", path.segments.join("::")) =>
            {
                ResolveError {
                    message: Self::undefined_callable_arity_message(func, arity).unwrap_or_else(
                        || {
                            format!(
                                "Undefined variable or function: {}",
                                path.segments.join("::")
                            )
                        },
                    ),
                    span: err.span,
                    related_labels: Vec::new(),
                }
            }
            _ => err,
        }
    }

    fn type_witness_from_expr(expr: Ast) -> Result<AstTy, ResolveError> {
        match expr {
            Ast::ConstructorCall(span, name, args) => {
                if args.is_empty() {
                    return Ok(AstTy::Named(span, name));
                }
                let inner = args
                    .into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => Self::type_witness_from_expr(expr),
                        RecordLitArg::Named(_, expr) => Err(ResolveError {
                            message: "type witness arguments do not accept named type parameters"
                                .into(),
                            span: expr.span().clone(),
                            related_labels: Vec::new(),
                        }),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(AstTy::Generic(span, name, inner))
            }
            Ast::Path(span, path) => Ok(AstTy::Named(span, path.segments.join("::"))),
            Ast::Var(span, name) if name == "Unit" => Ok(AstTy::Named(span, name)),
            Ast::Var(span, name) if name.chars().next().is_some_and(|ch| ch.is_uppercase()) => {
                Ok(AstTy::Named(span, name))
            }
            other => Err(ResolveError {
                message: "conversion target must be a bare type name such as String or Result<Int>"
                    .into(),
                span: other.span().clone(),
                related_labels: Vec::new(),
            }),
        }
    }

    fn type_witness_span(ast_ty: &AstTy) -> &Span {
        match ast_ty {
            AstTy::Named(span, _)
            | AstTy::Generic(span, _, _)
            | AstTy::Tuple(span, _)
            | AstTy::Func(span, _, _)
            | AstTy::ImplTrait(span, _) => span,
        }
    }

    pub(super) fn new() -> Self {
        Self {
            scope: initialize_scope(),
            predeclared_ids: HashMap::new(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            declaration_hidden_by_uid: HashMap::new(),
            current_module_path: None,
            current_stage_impl_targets: None,
            allow_top_level_shadowing: false,
        }
    }

    pub(super) fn with_scope(scope: Scope) -> Self {
        Self {
            scope,
            predeclared_ids: HashMap::new(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            declaration_hidden_by_uid: HashMap::new(),
            current_module_path: None,
            current_stage_impl_targets: None,
            allow_top_level_shadowing: false,
        }
    }

    pub(super) fn into_scope(self) -> Scope {
        self.scope
    }

    pub(super) fn qualify_current_declaration_name(&self, name: &str) -> String {
        match self.current_module_path.as_deref() {
            Some(module_path) if !module_path.is_empty() => format!("{}::{}", module_path, name),
            _ => name.to_string(),
        }
    }

    pub(super) fn with_child_scope<T>(
        &mut self,
        f: impl FnOnce(&mut Resolver) -> Result<T, ResolveError>,
    ) -> Result<T, ResolveError> {
        let mut child = Resolver::with_scope(self.scope.clone());
        child.declaration_uids = self.declaration_uids.clone();
        child.declaration_uid_kinds = self.declaration_uid_kinds.clone();
        child.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
        child.current_module_path = self.current_module_path.clone();
        child.current_stage_impl_targets = self.current_stage_impl_targets.clone();
        child.allow_top_level_shadowing = self.allow_top_level_shadowing;
        let out = f(&mut child)?;
        self.scope.advance_next_id_to(child.scope.next_id());
        Ok(out)
    }

    pub(super) fn is_constructor_style_head(name: &str) -> bool {
        name.rsplit("::")
            .next()
            .and_then(|segment| segment.chars().next())
            .is_some_and(|ch| ch.is_uppercase())
    }

    pub(super) fn declaration_fq_name_for_uid(&self, uid: u32) -> Option<String> {
        self.declaration_uids
            .iter()
            .find_map(|(fq_name, entry_uid)| (*entry_uid == uid).then(|| fq_name.clone()))
    }

    fn hidden_builtin_message(name: &str) -> String {
        let display_name = match name {
            "Agent::pid"
            | "Agent::spawn"
            | "Agent::state"
            | "Agent::store"
            | "Agent::self"
            | "Agent::context_handler"
            | "GenServer::pid"
            | "GenServer::spawn"
            | "GenServer::state"
            | "GenServer::store"
            | "GenServer::self"
            | "GenServer::context_handler"
            | "Supervisor::spawn"
            | "Supervisor::adopt"
            | "Supervisor::status"
            | "Supervisor::workers" => name,
            _ => name.rsplit("::").next().unwrap_or(name),
        };
        let guidance = match name {
            "Agent::pid"
            | "Agent::spawn"
            | "Agent::state"
            | "Agent::store"
            | "Agent::self"
            | "Agent::context_handler" => {
                "This Agent module surface is compiler-managed; use `defagent` or generated owner helpers instead."
            }
            "GenServer::pid"
            | "GenServer::spawn"
            | "GenServer::state"
            | "GenServer::store"
            | "GenServer::self"
            | "GenServer::context_handler" => {
                "This GenServer module surface is compiler-managed; use `defagent`, `defgenserver`, or generated owner helpers instead."
            }
            "Supervisor::spawn" => {
                "This Supervisor module surface is compiler-managed; use `DynamicSupervisor::spawn(...)` or a generated `SupName::spawn(...)` wrapper instead."
            }
            "Supervisor::adopt" => {
                "This Supervisor module surface is compiler-managed; use `DynamicSupervisor::adopt(...)` or a generated `SupName::adopt(...)` wrapper instead."
            }
            "Supervisor::status" => {
                "This Supervisor module surface is compiler-managed; use `DynamicSupervisor::status()` or a generated `SupName::status()` wrapper instead."
            }
            "Supervisor::workers" => {
                "This Supervisor module surface is compiler-managed; use a generated `SupName::workers(...)` wrapper or the public Workers API instead."
            }
            _ => match display_name {
            "__process_self" => "Use `Process::self()` instead.",
            "__process_sleep" => "Use `Process::sleep(...)` instead.",
            "__task_call" => "Use `Task::call(...)` instead.",
            "__task_async" => "Use `Task::async(...)` instead.",
            "__task_launch" => "Use `Task::launch(...)` instead.",
            "__task_cast" => "Use `Task::cast(...)` instead.",
            "__task_call_timeout"
            | "__task_async_timeout"
            | "__task_launch_timeout"
            | "__task_cast_timeout" => "Use the public Task API with `@timeout(...)` instead.",
            "__process_pid" | "__process_spawn" | "__process_state" | "__process_store" => {
                "This helper is compiler-managed; use `defagent`, `defgenserver`, or the public process surface instead."
            }
            "__supervisor_spawn" => {
                "Use `DynamicSupervisor::spawn(...)` or a generated Supervisor `spawn` wrapper instead."
            }
            "__supervisor_adopt" => {
                "Use `DynamicSupervisor::adopt(...)` or a generated Supervisor `adopt` wrapper instead."
            }
            "__supervisor_status" => {
                "Use `DynamicSupervisor::status()` or a generated Supervisor `status` wrapper instead."
            }
            "__supervisor_workers" => {
                "Use a generated Supervisor `workers` wrapper or the public Workers surface instead."
            }
            _ => "Use the public standard-library surface instead.",
        },
        };
        format!("hidden builtin `{display_name}` is compiler-internal. {guidance}")
    }

    fn hidden_builtin_error(&self, name: &str, span: Span) -> ResolveError {
        ResolveError {
            message: Self::hidden_builtin_message(name),
            span,
            related_labels: Vec::new(),
        }
    }

    fn resolve_var_like(
        &self,
        span: Span,
        name: String,
        compiler_generated: bool,
    ) -> Result<Resolved, ResolveError> {
        let uid = self
            .scope
            .lookup(&name)
            .or_else(|| {
                if compiler_generated && is_runtime_builtin_decl(&name) {
                    BUILTIN_METAS
                        .iter()
                        .position(|meta| meta.name == name)
                        .map(|idx| builtin_uid(idx as u16))
                } else {
                    None
                }
            })
            .or_else(|| {
                if name == "Tuple" {
                    Some(TUPLE_TYPE_ROOT_UID)
                } else {
                    None
                }
            })
            .ok_or_else(|| ResolveError {
                message: format!("Undefined variable: {}", name),
                span: span.clone(),
                related_labels: Vec::new(),
            })?;
        let qualified_name = (uid != TUPLE_TYPE_ROOT_UID)
            .then(|| self.declaration_fq_name_for_uid(uid))
            .flatten();
        if self
            .declaration_uid_kinds
            .get(&uid)
            .is_some_and(|kind| matches!(kind, DeclarationKind::Extractor))
        {
            return Err(ResolveError {
                message: format!(
                    "Extractor '{}' can only be used in MatchBlock/LHS positions. Use it on the left side of match, =?, or =. If you need a value-level API, write a normal def that returns Result or Option explicitly.",
                    name
                ),
                span,
                related_labels: Vec::new(),
            });
        }
        if !compiler_generated && self.declaration_hidden_by_uid.get(&uid) == Some(&true) {
            return Err(self.hidden_builtin_error(&name, span));
        }
        Ok(Resolved::Var(
            span.clone(),
            ResolvedId {
                name,
                qualified_name,
                unique_id: uid,
                compiler_generated,
                span,
            },
        ))
    }

    pub(super) fn attached_extractor_for_struct(
        &self,
        struct_uid: u32,
        surface_head: &str,
    ) -> Option<(Option<String>, u32, DeclarationKind)> {
        // Struct heads are resolved by declaration name, not by `import`.
        // In MatchBlock, `User(...)` is sugar for `User::deconstruct(...)`.
        let surface_extractor_name = format!("{}::deconstruct", surface_head);
        if let Some(extractor_uid) = self.scope.lookup(&surface_extractor_name) {
            let extractor_kind = self.declaration_uid_kinds.get(&extractor_uid).cloned()?;
            let qualified_name = self.declaration_fq_name_for_uid(extractor_uid);
            return Some((qualified_name, extractor_uid, extractor_kind));
        }

        let struct_fq_name = self.declaration_fq_name_for_uid(struct_uid)?;
        let extractor_fq_name = format!("{}::deconstruct", struct_fq_name);
        let extractor_uid = *self.declaration_uids.get(&extractor_fq_name)?;
        let extractor_kind = self.declaration_uid_kinds.get(&extractor_uid).cloned()?;
        Some((Some(extractor_fq_name), extractor_uid, extractor_kind))
    }

    pub(super) fn resolve_program(
        &mut self,
        stmts: Vec<Ast>,
    ) -> Result<Vec<Resolved>, ResolveError> {
        let stmts = self.lower_impl_defs(stmts)?;
        self.validate_auto_import_conflicts(&stmts)?;
        self.predeclare_functions(&stmts)?;
        let mut resolved = Vec::new();
        for stmt in stmts {
            if matches!(stmt, Ast::Import(_, _, _))
                || matches!(stmt, Ast::SupervisorInit(_, _))
                || matches!(stmt, Ast::IntrinsicDecl(_, _, _, _))
                || matches!(&stmt, Ast::BuiltinDecl(_, name, _, _, _) if is_doc_only_builtin_decl(name))
            {
                // `import` declarations are consumed by resolver-side module/import handling.
                // Until full module resolution lands, they are intentionally no-op here.
                continue;
            }
            resolved.push(self.resolve_node(stmt)?);
        }
        validate_trait_impl_pairs_in_nodes(&resolved)?;
        self.predeclared_ids.clear();
        Ok(resolved)
    }
}

impl Resolver {
    pub(super) fn resolve_node(&mut self, node: Ast) -> Result<Resolved, ResolveError> {
        match node {
            Ast::Lit(span, lit) => Ok(Resolved::Lit(span, lit)),

            Ast::Var(span, name) => self.resolve_var_like(span, name, false),
            Ast::InternalVar(span, name) => self.resolve_var_like(span, name, true),
            Ast::Path(span, path) => {
                let name = path.segments.join("::");
                self.resolve_var_like(span, name, false)
            }
            Ast::FuncLiteralRef(span, func) => Err(ResolveError {
                message: format!(
                    "standalone func literal ref `{}` must be lowered before resolution",
                    func.body
                ),
                span,
                related_labels: Vec::new(),
            }),

            Ast::App(span, func, args) => {
                // Check for special forms
                if let Ast::Var(_, ref name) = *func {
                    if name == "if" {
                        return self.resolve_if(span, args, IfKind::If3);
                    }
                    if name == "if_then" {
                        return self.resolve_if(span, args, IfKind::IfThen2);
                    }
                    if name == "if_let" {
                        return self.resolve_if_let(span, args);
                    }
                    if name == "if_let_then" {
                        return self.resolve_if_let_then(span, args);
                    }
                    if name == "is_match" {
                        return self.resolve_is_match(span, args);
                    }
                    if name == "assert" {
                        return self.resolve_assert(span, args);
                    }
                    if name == "ensure" {
                        return self.resolve_ensure(span, args);
                    }
                    if name == "map_err" {
                        return self.resolve_map_err(span, args);
                    }
                    if name == "cause" {
                        return self.resolve_cause(span, args);
                    }
                    if name == "recover_kind" {
                        return self.resolve_recover_kind(span, args);
                    }
                    if name == "and" {
                        return self.resolve_logic_call(span, args, LogicKind::And);
                    }
                    if name == "or" {
                        return self.resolve_logic_call(span, args, LogicKind::Or);
                    }
                    if name == "&&" {
                        return self.resolve_logic_call(span, args, LogicKind::And);
                    }
                    if name == "||" {
                        return self.resolve_logic_call(span, args, LogicKind::Or);
                    }
                }
                if let Ast::Path(_, ref path) = *func {
                    if path.segments.len() == 2
                        && path.segments[0] == "Result"
                        && path.segments[1] == "map_err"
                    {
                        return self.resolve_map_err(span, args);
                    }
                    if path.segments.len() == 2
                        && path.segments[0] == "Result"
                        && path.segments[1] == "cause"
                    {
                        return self.resolve_cause(span, args);
                    }
                    if path.segments.len() == 2
                        && path.segments[0] == "Result"
                        && path.segments[1] == "recover_kind"
                    {
                        return self.resolve_recover_kind(span, args);
                    }
                }

                if Self::conversion_call_head(&func).is_some() {
                    let resolved_func = self.resolve_node(*func.clone()).map_err(|err| {
                        Self::map_undefined_callable_error(err, &func, args.len())
                    })?;
                    if args.len() != 2 {
                        return Err(ResolveError {
                            message: "from/try_from expects exactly 2 positional arguments".into(),
                            span,
                            related_labels: Vec::new(),
                        });
                    }
                    let mut resolved_args = Vec::with_capacity(2);
                    for (idx, arg) in args.into_iter().enumerate() {
                        match arg {
                            RecordLitArg::Positional(expr) if idx == 1 => {
                                let witness_ty = Self::type_witness_from_expr(expr)?;
                                resolved_args.push(ResolvedRecordLitArg::Positional(
                                    Resolved::TypeRefWitness(
                                        Self::type_witness_span(&witness_ty).clone(),
                                        witness_ty,
                                    ),
                                ));
                            }
                            RecordLitArg::Positional(expr) => resolved_args
                                .push(ResolvedRecordLitArg::Positional(self.resolve_node(expr)?)),
                            RecordLitArg::Named(_, expr) => {
                                return Err(ResolveError {
                                    message: "from/try_from does not accept named arguments".into(),
                                    span: expr.span().clone(),
                                    related_labels: Vec::new(),
                                });
                            }
                        }
                    }
                    return Ok(Resolved::App(span, Box::new(resolved_func), resolved_args));
                }

                let resolved_func = self
                    .resolve_node(*func.clone())
                    .map_err(|err| Self::map_undefined_callable_error(err, &func, args.len()))?;
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

            Ast::Pipe(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.prepare_pipe_rhs(*right)?;
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::Pipe(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextMap(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.prepare_pipe_rhs(*right)?;
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::ContextMap(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextBind(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let rhs = self.prepare_pipe_rhs(*right)?;
                let r = self.resolve_node(rhs)?;
                Ok(Resolved::ContextBind(span, Box::new(l), Box::new(r)))
            }

            Ast::Compose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::Compose(span, Box::new(l), Box::new(r)))
            }

            Ast::LiftedCompose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::LiftedCompose(span, Box::new(l), Box::new(r)))
            }

            Ast::KleisliCompose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::KleisliCompose(span, Box::new(l), Box::new(r)))
            }

            Ast::ListNil(span) => Ok(Resolved::ListNil(span)),

            Ast::ListCons(span, head, tail) => {
                let head = self.resolve_node(*head)?;
                let tail = self.resolve_node(*tail)?;
                Ok(Resolved::ListCons(span, Box::new(head), Box::new(tail)))
            }

            Ast::ListLiteral(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::ListLiteral(span, resolved))
            }

            Ast::RangeLiteral(span, start, stop) => {
                let start = self.resolve_node(*start)?;
                let stop = self.resolve_node(*stop)?;
                Ok(Resolved::RangeLiteral(
                    span,
                    Box::new(start),
                    Box::new(stop),
                ))
            }

            Ast::TupleLiteral(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::TupleLiteral(span, resolved))
            }

            Ast::Grouped(span, inner) => {
                let inner = self.resolve_node(*inner)?;
                Ok(Resolved::Grouped(span, Box::new(inner)))
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
            Ast::Dbg(span, args) => Ok(Resolved::Dbg(
                span,
                args.into_iter()
                    .map(|arg| self.resolve_node(arg.expr))
                    .collect::<Result<Vec<_>, _>>()?,
            )),

            Ast::FieldAccess(span, expr, field) => {
                if matches!(expr.as_ref(), Ast::Var(_, name) if name == "ctx") {
                    return Ok(Resolved::ProcessContextHandler(span, field));
                }
                let resolved_expr = self.resolve_node(*expr)?;
                Ok(Resolved::FieldAccess(span, Box::new(resolved_expr), field))
            }

            Ast::Block(span, stmts) => {
                let resolved = self.with_child_scope(|child| {
                    stmts
                        .into_iter()
                        .map(|s| child.resolve_node(s))
                        .collect::<Result<Vec<_>, _>>()
                })?;
                Ok(Resolved::Block(span, resolved))
            }

            Ast::Semi(span, inner) => {
                let resolved = self.resolve_node(*inner)?;
                Ok(Resolved::Semi(span, Box::new(resolved)))
            }

            // Struct/Record/Deferror definitions — reuse predeclared IDs
            Ast::StructDef(span, name, fields, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| {
                        Ok(ResolvedField {
                            id: None,
                            name: f.name,
                            ty: self.resolve_type_annotation(f.ty)?,
                            span: f.span,
                            visibility: f.visibility,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructDef(
                    span,
                    rid,
                    rfields,
                    resolve_decl_attrs(&attrs),
                ))
            }

            Ast::RecordDef(span, name, fields, _) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| {
                        Ok(ResolvedField {
                            id: None,
                            name: f.name,
                            ty: self.resolve_type_annotation(f.ty)?,
                            span: f.span,
                            visibility: f.visibility,
                        })
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::RecordDef(span, rid, rfields))
            }

            Ast::DeferrorDef(span, name, fields, show_expr, _) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let mut error_scope = self.scope.clone();
                let mut rfields = Vec::new();
                for f in fields {
                    let uid = error_scope.define(&f.name, f.span.clone());
                    rfields.push(ResolvedField {
                        id: Some(ResolvedId {
                            name: f.name.clone(),
                            qualified_name: None,
                            unique_id: uid,
                            compiler_generated: false,
                            span: f.span.clone(),
                        }),
                        name: f.name,
                        ty: self.resolve_type_annotation(f.ty)?,
                        span: f.span,
                        visibility: f.visibility,
                    });
                }
                let mut show_resolver = Resolver::with_scope(error_scope);
                show_resolver.declaration_uids = self.declaration_uids.clone();
                show_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                show_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                show_resolver.current_module_path = self.current_module_path.clone();
                show_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_show = show_resolver.resolve_node(*show_expr)?;
                self.scope.advance_next_id_to(show_resolver.scope.next_id());
                Ok(Resolved::DeferrorDef(
                    span,
                    rid,
                    rfields,
                    Box::new(resolved_show),
                ))
            }

            Ast::EnumDef(span, name, type_params, variants, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name: name.clone(),
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let resolved_type_params = type_params
                    .into_iter()
                    .map(|param| self.resolve_type_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;

                let mut resolved_variants = Vec::new();
                for variant in variants {
                    let ctor_name = format!("{}::{}", name, variant.name);
                    let ctor_uid = self
                        .take_predeclared_id(&ctor_name)
                        .or_else(|| self.scope.lookup(&ctor_name))
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.scope.define_with_id(&ctor_name, ctor_uid);
                    let qualified_ctor_name = self.qualify_current_declaration_name(&ctor_name);
                    resolved_variants.push(ResolvedEnumVariant {
                        id: ResolvedId {
                            name: ctor_name,
                            qualified_name: Some(qualified_ctor_name),
                            unique_id: ctor_uid,
                            compiler_generated: false,
                            span: variant.span.clone(),
                        },
                        payload: variant
                            .payload
                            .into_iter()
                            .map(|ty| self.resolve_type_annotation(ty))
                            .collect::<Result<Vec<_>, ResolveError>>()?,
                        discriminant: variant.discriminant,
                        span: variant.span,
                    });
                }

                Ok(Resolved::EnumDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_variants,
                    resolve_decl_attrs(&attrs),
                ))
            }

            Ast::Def(span, name, type_params, params, ret_ty, body, attrs) => {
                let fun_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_scope = self.scope.clone();
                // Ensure self-recursion inside this definition binds to this declaration,
                // not to a newer same-name declaration predeclared later in the chunk.
                body_scope.define_with_id(&name, fun_uid);
                let mut body_resolver = Resolver::with_scope(body_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let resolved_params = params
                    .into_iter()
                    .map(|param| body_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: fun_uid,
                    compiler_generated: false,
                    span: span.clone(),
                };

                Ok(Resolved::Def(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_params,
                    ret_ty
                        .map(|ty| self.resolve_type_annotation(ty))
                        .transpose()?,
                    Box::new(resolved_body),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ConstDef(span, name, ty, value, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let resolved_value = self.resolve_node(*value)?;
                self.scope.define_with_id(&name, uid);
                let qualified_name = if attrs.visibility == Visibility::Public {
                    Some(self.qualify_current_declaration_name(&name))
                } else {
                    Some(self.qualify_current_declaration_name(&format!("__const__::{}", name)))
                };
                let rid = ResolvedId {
                    name,
                    qualified_name,
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                Ok(Resolved::ConstDef(
                    span,
                    rid,
                    ty.map(|ty| self.resolve_type_annotation(ty)).transpose()?,
                    Box::new(resolved_value),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ExtractorDef(span, name, type_params, param, ret_ty, body, attrs) => {
                let fun_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_scope = self.scope.clone();
                body_scope.define_with_id(&name, fun_uid);
                let mut body_resolver = Resolver::with_scope(body_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let resolved_param = body_resolver.resolve_extractor_param(param)?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: fun_uid,
                    compiler_generated: false,
                    span: span.clone(),
                };

                Ok(Resolved::ExtractorDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_param,
                    self.resolve_type_annotation(ret_ty)?,
                    Box::new(resolved_body),
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::TraitDef(span, name, type_params, methods, attrs) => {
                let qualified_trait_name = self.qualify_current_declaration_name(&name);
                let trait_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, trait_uid);
                let rid = ResolvedId {
                    name: name.clone(),
                    qualified_name: Some(qualified_trait_name.clone()),
                    unique_id: trait_uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let resolved_type_params = self.resolve_type_params(type_params)?;
                let mut resolved_methods = Vec::new();
                for method in methods {
                    let spire::ast::TraitMethodSig {
                        name: method_name,
                        type_params,
                        params,
                        ret_ty,
                        span: method_span,
                    } = method;
                    let method_alias = trait_method_qualified_name(&name, &method_name);
                    let qualified_method =
                        trait_method_qualified_name(&qualified_trait_name, &method_name);
                    let method_uid = self
                        .take_predeclared_id(&method_alias)
                        .or_else(|| self.scope.lookup(&method_alias))
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.scope.define_with_id(&method_alias, method_uid);

                    let mut method_resolver = Resolver::with_scope(self.scope.clone());
                    method_resolver.declaration_uids = self.declaration_uids.clone();
                    method_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                    method_resolver.declaration_hidden_by_uid =
                        self.declaration_hidden_by_uid.clone();
                    method_resolver.current_module_path = self.current_module_path.clone();
                    method_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                    let resolved_params = params
                        .into_iter()
                        .map(|param| method_resolver.resolve_fun_param(param))
                        .collect::<Result<Vec<_>, ResolveError>>()?;
                    self.scope
                        .advance_next_id_to(method_resolver.scope.next_id());
                    resolved_methods.push(ResolvedTraitMethodSig {
                        id: ResolvedId {
                            name: method_name,
                            qualified_name: Some(qualified_method),
                            unique_id: method_uid,
                            compiler_generated: false,
                            span: method_span.clone(),
                        },
                        type_params: self.resolve_type_params(type_params)?,
                        params: resolved_params,
                        ret_ty: self.resolve_type_annotation(ret_ty)?,
                        span: method_span,
                    });
                }
                Ok(Resolved::TraitDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_methods,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::TraitImplDef(span, trait_name, trait_args, target_ty, methods, _attrs) => {
                let (trait_uid, qualified_trait_name) =
                    self.resolve_trait_reference(&trait_name, &span)?;
                let trait_id = ResolvedId {
                    name: trait_name.clone(),
                    qualified_name: Some(qualified_trait_name.clone()),
                    unique_id: trait_uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let resolved_target_ty = self.resolve_type_annotation(target_ty)?;
                let target_key = match &resolved_target_ty {
                    AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
                    AstTy::Generic(_, name, args) => format!(
                        "{}<{}>",
                        name,
                        args.iter()
                            .map(Self::ast_ty_symbol_key)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    AstTy::Tuple(_, items) => format!(
                        "({})",
                        items
                            .iter()
                            .map(Self::ast_ty_symbol_key)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    AstTy::Func(_, params, ret) => format!(
                        "({} -> {})",
                        params
                            .iter()
                            .map(Self::ast_ty_symbol_key)
                            .collect::<Vec<_>>()
                            .join(", "),
                        Self::ast_ty_symbol_key(ret)
                    ),
                };
                let mut resolved_methods = Vec::new();
                for method in methods {
                    let (
                        method_span,
                        method_name,
                        type_params,
                        params,
                        ret_ty,
                        body,
                        attrs,
                        is_builtin,
                    ) = match method {
                        Ast::Def(
                            method_span,
                            method_name,
                            type_params,
                            params,
                            ret_ty,
                            body,
                            attrs,
                        ) => (
                            method_span,
                            method_name,
                            type_params,
                            params,
                            ret_ty,
                            Some(body),
                            attrs,
                            false,
                        ),
                        Ast::BuiltinDecl(method_span, method_name, params, ret_ty, attrs) => (
                            method_span,
                            method_name,
                            Vec::new(),
                            params,
                            ret_ty,
                            None,
                            attrs,
                            true,
                        ),
                        _ => {
                            return Err(ResolveError {
                                message:
                                    "trait impl body may only contain `def` / `@builtin def` declarations"
                                        .to_string(),
                                span: span.clone(),
                                related_labels: Vec::new(),
                            });
                        }
                    };
                    let qualified_function_name = trait_impl_method_qualified_name(
                        self.current_module_path.as_deref(),
                        &trait_name,
                        &trait_args,
                        &resolved_target_ty,
                        &method_name,
                        method_span.start,
                    );
                    let method_uid = self
                        .declaration_uids
                        .get(&qualified_function_name)
                        .copied()
                        .unwrap_or_else(|| self.scope.reserve_id());
                    let mut method_scope = self.scope.clone();
                    method_scope.define_with_id(&method_name, method_uid);
                    let mut method_resolver = Resolver::with_scope(method_scope);
                    method_resolver.declaration_uids = self.declaration_uids.clone();
                    method_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                    method_resolver.declaration_hidden_by_uid =
                        self.declaration_hidden_by_uid.clone();
                    method_resolver.current_module_path = self.current_module_path.clone();
                    method_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                    let resolved_params = params
                        .into_iter()
                        .map(|param| method_resolver.resolve_fun_param(param))
                        .collect::<Result<Vec<_>, ResolveError>>()?;
                    let resolved_body = if let Some(body) = body {
                        method_resolver.resolve_node(*body)?
                    } else {
                        Resolved::Lit(method_span.clone(), spire::ast::Lit::Unit)
                    };
                    self.scope
                        .advance_next_id_to(method_resolver.scope.next_id());
                    let local_function_name = if trait_args.is_empty() {
                        format!("{}::{}", target_key, method_name)
                    } else {
                        format!(
                            "{}::{}::{}",
                            trait_instance_key(&qualified_trait_name, &trait_args),
                            target_key,
                            method_name
                        )
                    };

                    resolved_methods.push(ResolvedTraitImplMethod {
                        method_name: method_name.clone(),
                        function_id: ResolvedId {
                            name: local_function_name,
                            qualified_name: Some(qualified_function_name),
                            unique_id: method_uid,
                            compiler_generated: false,
                            span: method_span.clone(),
                        },
                        type_params: self.resolve_type_params(type_params)?,
                        params: resolved_params,
                        ret_ty: ret_ty
                            .map(|ty| self.resolve_type_annotation(ty))
                            .transpose()?,
                        body: Box::new(resolved_body),
                        attrs: resolve_decl_attrs(&attrs),
                        span: method_span,
                        is_builtin,
                    });
                }

                Ok(Resolved::TraitImplDef(
                    span,
                    trait_id,
                    trait_args
                        .into_iter()
                        .map(|arg| self.resolve_type_annotation(arg))
                        .collect::<Result<Vec<_>, _>>()?,
                    resolved_target_ty,
                    resolved_methods,
                ))
            }

            Ast::BuiltinDecl(span, name, params, ret_ty, attrs) => {
                let qualified_name = self.qualify_current_declaration_name(&name);
                let is_io_builtin =
                    sindr::builtin::builtin_meta_for_decl(&name, Some(&qualified_name)).is_some();
                if !is_runtime_builtin_decl(&name)
                    && !is_special_form_builtin_decl(&name)
                    && !is_io_builtin
                {
                    return Err(ResolveError {
                        message: format!("Unknown builtin declaration: {}", name),
                        span,
                        related_labels: Vec::new(),
                    });
                }

                let builtin_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut decl_resolver = Resolver::with_scope(self.scope.clone());
                decl_resolver.declaration_uids = self.declaration_uids.clone();
                decl_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                decl_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                decl_resolver.current_module_path = self.current_module_path.clone();
                decl_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
                let resolved_params = params
                    .into_iter()
                    .map(|param| decl_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                self.scope.advance_next_id_to(decl_resolver.scope.next_id());
                self.scope.define_with_id(&name, builtin_uid);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinDecl(
                    span,
                    rid,
                    resolved_params,
                    ret_ty
                        .map(|ty| self.resolve_type_annotation(ty))
                        .transpose()?,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::IntrinsicDecl(span, name, _, _) => Err(ResolveError {
                message: format!(
                    "Intrinsic declaration `{name}` is docs-only and should not reach resolution"
                ),
                span,
                related_labels: Vec::new(),
            }),
            Ast::BuiltinExtractorDecl(span, name, param, ret_ty, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let resolved_param = self.resolve_extractor_param(param)?;
                Ok(Resolved::BuiltinExtractorDecl(
                    span,
                    rid,
                    resolved_param,
                    self.resolve_type_annotation(ret_ty)?,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::BuiltinTypeDecl(span, head, attrs) => {
                let builtin_type_uid = self
                    .take_predeclared_id(&head.name)
                    .unwrap_or_else(|| self.scope.reserve_id());
                let qualified_name = self.qualify_current_declaration_name(&head.name);
                let rid = ResolvedId {
                    name: head.name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_type_uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinTypeDecl(
                    span,
                    rid,
                    head.params,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ResultCtorDecl(span, name, param_ty, ret_ty, attrs) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                Ok(Resolved::ResultCtorDecl(
                    span,
                    rid,
                    param_ty,
                    ret_ty,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::Defmod(span, name, _, _) => Err(ResolveError {
                message: format!("Module resolution is not implemented yet: {}", name),
                span,
                related_labels: Vec::new(),
            }),
            Ast::Defagent(span, name, _, _, _)
            | Ast::Defgenserver(span, name, _, _, _)
            | Ast::Defsupervisor(span, name, _, _, _)
            | Ast::DefdynamicSupervisor(span, name, _, _, _) => Err(ResolveError {
                message: format!("Process module resolution is not implemented yet: {}", name),
                span,
                related_labels: Vec::new(),
            }),
            Ast::Import(span, _, _) => Err(ResolveError {
                message: "Import resolution is not implemented yet".to_string(),
                span,
                related_labels: Vec::new(),
            }),
            Ast::Include(span, _) => Err(ResolveError {
                message: "include directives must be resolved before name resolution".to_string(),
                span,
                related_labels: Vec::new(),
            }),
            Ast::ImplDef(span, target, _, _) => Err(ResolveError {
                message: format!("impl lowering failed for target `{}`", target),
                span,
                related_labels: Vec::new(),
            }),

            Ast::Closure(span, params, body) => {
                let mut closure_scope = self.scope.clone();
                let mut resolved_params = Vec::new();
                for param in params {
                    let uid = closure_scope.define(&param.name, param.span.clone());
                    resolved_params.push(ResolvedClosureParam {
                        id: ResolvedId {
                            name: param.name,
                            qualified_name: None,
                            unique_id: uid,
                            compiler_generated: false,
                            span: param.span,
                        },
                        ty: param.ty,
                    });
                }

                let mut body_resolver = Resolver::with_scope(closure_scope);
                body_resolver.declaration_uids = self.declaration_uids.clone();
                body_resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
                body_resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
                body_resolver.current_module_path = self.current_module_path.clone();
                body_resolver.allow_top_level_shadowing = self.allow_top_level_shadowing;
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
                match self.lower_capture_expr(span.clone(), *target, args)? {
                    Ast::Capture(_, target, args) => {
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
                    lowered => self.resolve_node(lowered),
                }
            }

            Ast::CapturePlaceholder(span, index) => Err(ResolveError {
                message: format!(
                    "capture placeholder &{} must appear inside a capture call",
                    index
                ),
                span,
                related_labels: Vec::new(),
            }),

            Ast::StructLit(span, type_name, field_vals) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                    related_labels: Vec::new(),
                })?;
                let rid = ResolvedId {
                    name: type_name,
                    qualified_name: None,
                    unique_id: uid,
                    compiler_generated: false,
                    span: span.clone(),
                };
                let resolved_fields = field_vals
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(
                            ResolvedStructLitField::Explicit(name, self.resolve_node(expr)?),
                        ),
                        StructLitField::Shorthand(name) => Ok(ResolvedStructLitField::Shorthand(
                            name.clone(),
                            self.resolve_node(Ast::Var(span.clone(), name))?,
                        )),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructLit(span, rid, resolved_fields))
            }

            Ast::InternalStructLit(span, type_name, field_vals) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                    related_labels: Vec::new(),
                })?;
                let rid = ResolvedId {
                    name: type_name,
                    qualified_name: None,
                    unique_id: uid,
                    compiler_generated: true,
                    span: span.clone(),
                };
                let resolved_fields = field_vals
                    .into_iter()
                    .map(|field| match field {
                        StructLitField::Explicit(name, expr) => Ok(
                            ResolvedStructLitField::Explicit(name, self.resolve_node(expr)?),
                        ),
                        StructLitField::Shorthand(name) => Ok(ResolvedStructLitField::Shorthand(
                            name.clone(),
                            self.resolve_node(Ast::Var(span.clone(), name))?,
                        )),
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructLit(span, rid, resolved_fields))
            }

            Ast::ConstructorCall(span, type_name, args) => {
                let normalized_name = {
                    // In ExprBlock, a struct head like `User(...)` dispatches to
                    // `User::new(...)` when that constructor exists.
                    let sugared = format!("{}::new", type_name);
                    if self.scope.lookup(&sugared).is_some() {
                        sugared
                    } else {
                        type_name
                    }
                };
                if let Some(uid) = self.scope.lookup(&normalized_name) {
                    if self
                        .declaration_uid_kinds
                        .get(&uid)
                        .is_some_and(|kind| matches!(kind, DeclarationKind::Const))
                    {
                        let qualified_name = self.declaration_fq_name_for_uid(uid);
                        let rid = ResolvedId {
                            name: normalized_name,
                            qualified_name,
                            unique_id: uid,
                            compiler_generated: false,
                            span: span.clone(),
                        };
                        if args.is_empty() {
                            return Ok(Resolved::Var(span, rid));
                        }
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
                        return Ok(Resolved::App(
                            span.clone(),
                            Box::new(Resolved::Var(span, rid)),
                            resolved_args,
                        ));
                    }
                }
                let uid = self
                    .scope
                    .lookup(&normalized_name)
                    .ok_or_else(|| ResolveError {
                        message: format!("Undefined type: {}", normalized_name),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    })?;
                let rid = ResolvedId {
                    name: normalized_name,
                    qualified_name: None,
                    unique_id: uid,
                    compiler_generated: false,
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
                    .map(|arm| self.resolve_match_arm(arm))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::Match(
                    span,
                    Box::new(resolved_scrut),
                    resolved_arms,
                ))
            }
            Ast::Namespace(span, _, _) => Err(ResolveError {
                message: "namespace declarations must be lowered before name resolution".into(),
                span,
                related_labels: Vec::new(),
            }),
            Ast::SupervisorInit(span, _) => Err(ResolveError {
                message: "supervisor_init must be collected before name resolution".into(),
                span,
                related_labels: Vec::new(),
            }),
        }
    }

    pub(super) fn resolve_fun_param(
        &mut self,
        param: FunParam,
    ) -> Result<ResolvedFunParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedFunParam {
            id: ResolvedId {
                name: param.name,
                qualified_name: None,
                unique_id: uid,
                compiler_generated: false,
                span: param.span,
            },
            ty: self.resolve_type_annotation(param.ty)?,
        })
    }

    pub(super) fn resolve_type_params(
        &self,
        type_params: Vec<spire::ast::TypeParam>,
    ) -> Result<Vec<ResolvedTypeParam>, ResolveError> {
        type_params
            .into_iter()
            .map(|param| self.resolve_type_param(param))
            .collect()
    }

    pub(super) fn resolve_extractor_param(
        &mut self,
        param: ExtractorParam,
    ) -> Result<ResolvedExtractorParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedExtractorParam {
            id: ResolvedId {
                name: param.name,
                qualified_name: None,
                unique_id: uid,
                compiler_generated: false,
                span: param.span,
            },
            ty: param
                .ty
                .map(|ty| self.resolve_type_annotation(ty))
                .transpose()?,
        })
    }

    fn resolve_type_param(
        &self,
        param: spire::ast::TypeParam,
    ) -> Result<ResolvedTypeParam, ResolveError> {
        Ok(ResolvedTypeParam {
            name: param.name,
            bound: param
                .bound
                .map(|bound| self.resolve_trait_bound_name(&bound, &param.span))
                .transpose()?,
            span: param.span,
        })
    }

    fn resolve_type_annotation(&self, ty: AstTy) -> Result<AstTy, ResolveError> {
        match ty {
            AstTy::Named(span, name) => Ok(AstTy::Named(span, name)),
            AstTy::ImplTrait(span, name) => Ok(AstTy::ImplTrait(
                span.clone(),
                self.resolve_trait_bound_name(&name, &span)?,
            )),
            AstTy::Generic(span, name, args) => Ok(AstTy::Generic(
                span,
                name,
                args.into_iter()
                    .map(|arg| self.resolve_type_annotation(arg))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            AstTy::Tuple(span, items) => Ok(AstTy::Tuple(
                span,
                items
                    .into_iter()
                    .map(|item| self.resolve_type_annotation(item))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
            )),
            AstTy::Func(span, params, ret) => Ok(AstTy::Func(
                span,
                params
                    .into_iter()
                    .map(|param| self.resolve_type_annotation(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?,
                Box::new(self.resolve_type_annotation(*ret)?),
            )),
        }
    }

    fn resolve_trait_reference(
        &self,
        trait_name: &str,
        span: &Span,
    ) -> Result<(u32, String), ResolveError> {
        let trait_uid = self.scope.lookup(trait_name).ok_or_else(|| ResolveError {
            message: format!("Undefined trait: {}", trait_name),
            span: span.clone(),
            related_labels: Vec::new(),
        })?;
        match self.declaration_uid_kinds.get(&trait_uid) {
            Some(DeclarationKind::Trait) => {}
            _ => {
                return Err(ResolveError {
                    message: format!("{} is not a trait", trait_name),
                    span: span.clone(),
                    related_labels: Vec::new(),
                });
            }
        }
        let qualified_name = self
            .declaration_fq_name_for_uid(trait_uid)
            .unwrap_or_else(|| trait_name.to_string());
        Ok((trait_uid, qualified_name))
    }

    fn resolve_trait_bound_name(
        &self,
        trait_name: &str,
        span: &Span,
    ) -> Result<String, ResolveError> {
        self.resolve_trait_reference(trait_name, span)
            .map(|(_, qualified_name)| qualified_name)
    }

    fn ast_ty_symbol_key(ty: &AstTy) -> String {
        match ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                name,
                args.iter()
                    .map(Self::ast_ty_symbol_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::ast_ty_symbol_key)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => format!(
                "({} -> {})",
                params
                    .iter()
                    .map(Self::ast_ty_symbol_key)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::ast_ty_symbol_key(ret)
            ),
        }
    }
}

pub(super) fn validate_trait_impl_pairs_in_nodes(
    resolved: &[Resolved],
) -> Result<(), ResolveError> {
    let mut seen_pairs: HashMap<String, Span> = HashMap::new();
    for node in resolved {
        let Resolved::TraitImplDef(span, trait_id, trait_args, target_ty, _) = node else {
            continue;
        };
        let trait_name = trait_instance_key(
            trait_id.qualified_name.as_deref().unwrap_or(&trait_id.name),
            trait_args,
        );
        let pair_key = format!(
            "{} for {}",
            trait_name,
            Resolver::ast_ty_symbol_key(target_ty)
        );
        if let Some(first_span) = seen_pairs.get(&pair_key) {
            return Err(ResolveError {
                message: format!(
                    "Multiple trait impl blocks for `{}` are not allowed",
                    pair_key
                ),
                span: span.clone(),
                related_labels: vec![
                    ResolveErrorLabel {
                        span: first_span.clone(),
                        message: "first definition".to_string(),
                    },
                    ResolveErrorLabel {
                        span: span.clone(),
                        message: "conflicting definition".to_string(),
                    },
                ],
            });
        } else {
            seen_pairs.insert(pair_key.clone(), span.clone());
        }
    }
    Ok(())
}
