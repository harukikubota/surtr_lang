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
        let err = self.resolve_node(err_expr)?;
        Ok(Resolved::MapErr(span, Box::new(value), Box::new(err)))
    }

    pub(super) fn resolve_cause(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [value_expr, err_expr] = collect_fixed_positional_args(span.clone(), args, "cause", 2)?;
        let value = self.resolve_node(value_expr)?;
        let err = self.resolve_node(err_expr)?;
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
        let marker = self.resolve_error_kind_name(marker_expr)?;
        let handler = self.resolve_node(handler_expr)?;
        Ok(Resolved::RecoverKind(
            span,
            Box::new(value),
            marker,
            Box::new(handler),
        ))
    }

    fn resolve_error_kind_name(&mut self, expr: Ast) -> Result<ResolvedId, ResolveError> {
        let span = expr.span().clone();
        let invalid = || ResolveError {
            message: "recover_kind marker must be a concrete deferror type name".into(),
            span: span.clone(),
            diagnostic: crate::error::ResolveErrorDiagnostic {
                reason: crate::error::ResolveErrorReason::SpecialForm,
                subject: None,
            },
            related_labels: Vec::new(),
        };
        if matches!(&expr, Ast::Var(_, name) if name == "Error")
            || !matches!(expr, Ast::Var(..) | Ast::Path(..))
        {
            return Err(invalid());
        }
        // A qualified name in this position is a declaration reference,
        // not the ordinary value expression's nullary constructor lowering.
        let name_expr = match expr {
            Ast::Path(path_span, path) => Ast::Var(path_span, path.segments.join("::")),
            other => other,
        };
        let Resolved::Var(_, mut id) = self.resolve_node(name_expr)? else {
            return Err(invalid());
        };
        // Declaration identity excludes the abstract Error head and all
        // runtime values, regardless of their name or constructor layout.
        if self.declaration_uid_kinds.get(&id.unique_id) != Some(&DeclarationKind::Deferror) {
            return Err(invalid());
        }
        id.qualified_name = Some(self.declaration_fq_name_for_uid(id.unique_id).ok_or_else(
            || ResolveError {
                message: "recover_kind ErrorKind identity has no canonical declaration name".into(),
                span,
                diagnostic: crate::error::ResolveErrorDiagnostic {
                    reason: crate::error::ResolveErrorReason::CompilerInvariant,
                    subject: Some(id.name.clone()),
                },
                related_labels: Vec::new(),
            },
        )?);
        Ok(id)
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

pub(super) fn pattern_has_binding_vars(pattern: &AstPattern) -> bool {
    match pattern {
        AstPattern::Projection { inner, .. } => pattern_has_binding_vars(inner),
        AstPattern::Var(_, _) | AstPattern::Annotated(_, _, _) | AstPattern::As(_, _, _, _, _) => {
            true
        }
        AstPattern::ListCons(_, head, tail) => {
            pattern_has_binding_vars(head) || pattern_has_binding_vars(tail)
        }
        AstPattern::Constructor(_, _, inners)
        | AstPattern::Tuple(_, inners)
        | AstPattern::Or(_, inners) => inners.iter().any(pattern_has_binding_vars),
        AstPattern::Call(_, _, args) => args
            .iter()
            .filter_map(|arg| arg.pattern.as_deref())
            .any(pattern_has_binding_vars),
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

pub(super) fn ast_pattern_span(pattern: &AstPattern) -> &Span {
    match pattern {
        AstPattern::Projection { span, .. } => span,
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
