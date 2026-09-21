use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IfKind {
    If3,
    IfThen2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LogicKind {
    And,
    Or,
}

impl Resolver {
    pub(super) fn resolve_if(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
        kind: IfKind,
    ) -> Result<Resolved, ResolveError> {
        match kind {
            IfKind::If3 => {
                let [cond_expr, then_expr, else_expr] =
                    collect_fixed_positional_args(span.clone(), args, "if", 3)?;
                let cond = self.resolve_node(cond_expr)?;
                let then = self.resolve_node(then_expr)?;
                return Ok(Resolved::If(
                    span,
                    Box::new(cond),
                    Box::new(then),
                    Some(Box::new(self.resolve_node(else_expr)?)),
                ));
            }
            IfKind::IfThen2 => {
                let [cond_expr, then_expr] =
                    collect_fixed_positional_args(span.clone(), args, "if_then", 2)?;
                let cond = self.resolve_node(cond_expr)?;
                let then = self.resolve_node(then_expr)?;
                return Ok(Resolved::If(span, Box::new(cond), Box::new(then), None));
            }
        }
    }

    pub(super) fn resolve_assert(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [cond_expr, err_expr] = collect_fixed_positional_args(span.clone(), args, "assert", 2)?;
        let cond = self.resolve_node(cond_expr)?;
        let err = self.resolve_node(err_expr)?;
        Ok(Resolved::Assert(span, Box::new(cond), Box::new(err)))
    }

    pub(super) fn resolve_ensure(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [value_expr, pred_expr, err_expr] =
            collect_fixed_positional_args(span.clone(), args, "ensure", 3)?;
        let value = self.resolve_node(value_expr)?;
        let pred = self.resolve_node(pred_expr)?;
        let err = self.resolve_node(err_expr)?;
        Ok(Resolved::Ensure(
            span,
            Box::new(value),
            Box::new(pred),
            Box::new(err),
        ))
    }

    pub(super) fn resolve_map_err(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [value_expr, err_expr] =
            collect_fixed_positional_args(span.clone(), args, "map_err", 2)?;
        let value = self.resolve_node(value_expr)?;
        let err = self.resolve_error_constructor_expr(err_expr, "map_err", "error argument")?;
        Ok(Resolved::MapErr(span, Box::new(value), Box::new(err)))
    }

    pub(super) fn resolve_cause(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [value_expr, err_expr] = collect_fixed_positional_args(span.clone(), args, "cause", 2)?;
        let value = self.resolve_node(value_expr)?;
        let err = self.resolve_error_constructor_expr(err_expr, "cause", "error argument")?;
        Ok(Resolved::Cause(span, Box::new(value), Box::new(err)))
    }

    pub(super) fn resolve_recover_kind(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [value_expr, marker_expr, handler_expr] =
            collect_fixed_positional_args(span.clone(), args, "recover_kind", 3)?;
        let value = self.resolve_node(value_expr)?;
        let marker = self.resolve_error_constructor_expr(marker_expr, "recover_kind", "marker")?;
        let handler = self.resolve_node(handler_expr)?;
        Ok(Resolved::RecoverKind(
            span,
            Box::new(value),
            Box::new(marker),
            Box::new(handler),
        ))
    }

    fn resolve_error_constructor_expr(
        &mut self,
        expr: Ast,
        form_name: &str,
        role_name: &str,
    ) -> Result<Resolved, ResolveError> {
        match expr {
            Ast::Var(..) | Ast::Path(..) | Ast::ConstructorCall(..) => self.resolve_node(expr),
            Ast::App(span, func, args) => match *func {
                Ast::Var(..) | Ast::Path(..) => {
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
                        .collect::<Result<Vec<_>, ResolveError>>()?;
                    Ok(Resolved::App(span, Box::new(resolved_func), resolved_args))
                }
                other => Err(ResolveError {
                    message: format!(
                        "{} {} must be a deferror name or constructor",
                        form_name, role_name
                    ),
                    span: other.span().clone(),
                    diagnostic: crate::error::ResolveErrorDiagnostic {
                        reason: crate::error::ResolveErrorReason::SpecialForm,
                        subject: None,
                    },
                    related_labels: Vec::new(),
                }),
            },
            other => Err(ResolveError {
                message: format!(
                    "{} {} must be a deferror name or constructor",
                    form_name, role_name
                ),
                span: other.span().clone(),
                diagnostic: crate::error::ResolveErrorDiagnostic {
                    reason: crate::error::ResolveErrorReason::SpecialForm,
                    subject: None,
                },
                related_labels: Vec::new(),
            }),
        }
    }

    pub(super) fn resolve_if_let(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [term, pattern_expr, then_expr, else_expr] =
            collect_fixed_positional_args(span.clone(), args, "if_let", 4)?;

        let pattern = Self::pattern_from_parser_carrier(pattern_expr, "if_let")?;
        let fallback = AstPattern::Wildcard(span.clone());

        self.resolve_node(Ast::Match(
            span.clone(),
            Box::new(term),
            vec![
                AstMatchArm {
                    pattern,
                    guard: None,
                    body: then_expr,
                },
                AstMatchArm {
                    pattern: fallback,
                    guard: None,
                    body: else_expr,
                },
            ],
        ))
        .map(|resolved| match resolved {
            Resolved::Match(span, scrutinee, arms) => Resolved::IfLet(span, scrutinee, arms),
            _ => unreachable!("match resolver retains its form"),
        })
    }

    pub(super) fn resolve_if_let_then(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [term, pattern_expr, then_expr] =
            collect_fixed_positional_args(span.clone(), args, "if_let_then", 3)?;

        let pattern = Self::pattern_from_parser_carrier(pattern_expr, "if_let_then")?;
        let unit_lit = Ast::Lit(span.clone(), Lit::Unit);
        let then_block = Ast::Block(
            span.clone(),
            vec![then_expr, Ast::Lit(span.clone(), Lit::Unit)],
        );

        self.resolve_node(Ast::Match(
            span.clone(),
            Box::new(term),
            vec![
                AstMatchArm {
                    pattern,
                    guard: None,
                    body: then_block,
                },
                AstMatchArm {
                    pattern: AstPattern::Wildcard(span.clone()),
                    guard: None,
                    body: unit_lit,
                },
            ],
        ))
    }

    pub(super) fn resolve_is_match(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [term, pattern_expr] =
            collect_fixed_positional_args(span.clone(), args, "is_match", 2)?;
        let pattern = Self::pattern_from_parser_carrier(pattern_expr, "is_match")?;

        if pattern_has_binding_vars(&pattern) {
            return Err(ResolveError {
                message: "`is_match` pattern does not allow binding variables. Use `_` to ignore a value, or use `if_let` / `match` when you need bindings.".into(),
                span: ast_pattern_span(&pattern).clone(),
            diagnostic: crate::error::ResolveErrorDiagnostic { reason: crate::error::ResolveErrorReason::SpecialForm, subject: None },
            related_labels: Vec::new(),
            });
        }

        self.resolve_node(Ast::Match(
            span.clone(),
            Box::new(term),
            vec![
                AstMatchArm {
                    pattern,
                    guard: None,
                    body: Ast::Lit(span.clone(), Lit::Bool(true)),
                },
                AstMatchArm {
                    pattern: AstPattern::Wildcard(span.clone()),
                    guard: None,
                    body: Ast::Lit(span, Lit::Bool(false)),
                },
            ],
        ))
    }

    fn pattern_from_parser_carrier(
        expr: Ast,
        callee_name: &str,
    ) -> Result<AstPattern, ResolveError> {
        let span = expr.span().clone();
        let Ast::Match(_, scrutinee, arms) = expr else {
            return Err(pattern_argument_invariant_error(callee_name, span));
        };
        if !matches!(scrutinee.as_ref(), Ast::Lit(_, Lit::Unit)) || arms.len() != 1 {
            return Err(pattern_argument_invariant_error(callee_name, span));
        }
        let arm = arms.into_iter().next().expect("one pattern carrier arm");
        if arm.guard.is_some() || !matches!(arm.body, Ast::Lit(_, Lit::Unit)) {
            return Err(pattern_argument_invariant_error(callee_name, span));
        }
        Ok(arm.pattern)
    }

    pub(super) fn resolve_logic_call(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
        kind: LogicKind,
    ) -> Result<Resolved, ResolveError> {
        let callee_name = match kind {
            LogicKind::And => "and",
            LogicKind::Or => "or",
        };
        let [left_expr, right_expr] =
            collect_fixed_positional_args(span.clone(), args, callee_name, 2)?;
        let left = self.resolve_node(left_expr)?;
        let right = self.resolve_node(right_expr)?;
        let bool_lit = |value| Resolved::Lit(span.clone(), Lit::Bool(value));

        let (then_branch, else_branch) = match kind {
            LogicKind::And => (right, bool_lit(false)),
            LogicKind::Or => (bool_lit(true), right),
        };

        Ok(Resolved::If(
            span,
            Box::new(left),
            Box::new(then_branch),
            Some(Box::new(else_branch)),
        ))
    }
}

fn collect_positional_args(
    span: Span,
    args: Vec<RecordLitArg>,
    callee_name: &str,
    expected_arity: usize,
) -> Result<Vec<Ast>, ResolveError> {
    if args.len() != expected_arity {
        return Err(ResolveError {
            message: format!(
                "{} expects {} arguments, got {}",
                callee_name,
                expected_arity,
                args.len()
            ),
            span,
            diagnostic: crate::error::ResolveErrorDiagnostic {
                reason: crate::error::ResolveErrorReason::SpecialForm,
                subject: None,
            },
            related_labels: Vec::new(),
        });
    }

    let mut positional = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            RecordLitArg::Positional(expr) => positional.push(expr),
            RecordLitArg::Named(name, _) => {
                return Err(ResolveError {
                    message: format!("{} does not accept named argument '{}'", callee_name, name),
                    span,
                    diagnostic: crate::error::ResolveErrorDiagnostic {
                        reason: crate::error::ResolveErrorReason::SpecialForm,
                        subject: None,
                    },
                    related_labels: Vec::new(),
                });
            }
        }
    }
    Ok(positional)
}

fn collect_fixed_positional_args<const N: usize>(
    span: Span,
    args: Vec<RecordLitArg>,
    callee_name: &str,
    expected_arity: usize,
) -> Result<[Ast; N], ResolveError> {
    let positional = collect_positional_args(span.clone(), args, callee_name, expected_arity)?;
    let actual_arity = positional.len();
    positional.try_into().map_err(|_| ResolveError {
        message: format!(
            "{} expects {} arguments, got {}",
            callee_name, expected_arity, actual_arity
        ),
        span,
        diagnostic: crate::error::ResolveErrorDiagnostic {
            reason: crate::error::ResolveErrorReason::SpecialForm,
            subject: None,
        },
        related_labels: Vec::new(),
    })
}

fn pattern_argument_invariant_error(callee_name: &str, span: Span) -> ResolveError {
    ResolveError {
        message: format!(
            "Internal invariant broken: `{callee_name}` Pattern argument lacks its parser carrier"
        ),
        span,
        diagnostic: crate::error::ResolveErrorDiagnostic {
            reason: crate::error::ResolveErrorReason::SpecialForm,
            subject: None,
        },
        related_labels: Vec::new(),
    }
}

fn pattern_has_binding_vars(pattern: &AstPattern) -> bool {
    match pattern {
        AstPattern::Var(_, _) | AstPattern::Annotated(_, _, _) | AstPattern::As(_, _, _, _, _) => {
            true
        }
        AstPattern::ListCons(_, head, tail) => {
            pattern_has_binding_vars(head) || pattern_has_binding_vars(tail)
        }
        AstPattern::Constructor(_, _, inners)
        | AstPattern::Call(_, _, inners)
        | AstPattern::Tuple(_, inners)
        | AstPattern::Or(_, inners) => inners.iter().any(pattern_has_binding_vars),
        AstPattern::Wildcard(_)
        | AstPattern::AnnotatedWildcard(_, _)
        | AstPattern::Pin(_, _)
        | AstPattern::ListNil(_)
        | AstPattern::IntLit(_, _)
        | AstPattern::StrLit(_, _)
        | AstPattern::BoolLit(_, _)
        | AstPattern::DurationLit(_, _) => false,
    }
}

fn ast_pattern_span(pattern: &AstPattern) -> &Span {
    match pattern {
        AstPattern::Var(span, _)
        | AstPattern::Annotated(span, _, _)
        | AstPattern::Pin(span, _)
        | AstPattern::Wildcard(span)
        | AstPattern::AnnotatedWildcard(span, _)
        | AstPattern::ListNil(span)
        | AstPattern::ListCons(span, _, _)
        | AstPattern::IntLit(span, _)
        | AstPattern::StrLit(span, _)
        | AstPattern::BoolLit(span, _)
        | AstPattern::DurationLit(span, _)
        | AstPattern::Constructor(span, _, _)
        | AstPattern::Call(span, _, _)
        | AstPattern::Tuple(span, _)
        | AstPattern::Or(span, _)
        | AstPattern::As(span, _, _, _, _) => span,
    }
}
