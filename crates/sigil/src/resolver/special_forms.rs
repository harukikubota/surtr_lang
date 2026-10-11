use super::captures::collect_eager_references;
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
    pub(super) fn is_direct_capture_parameter(&self, expr: &Ast) -> bool {
        match expr {
            Ast::InternalVar(_, name) => self
                .scope
                .lookup(name)
                .is_some_and(|uid| self.capture_placeholder_ids.contains(&uid)),
            Ast::Grouped(_, inner) => self.is_direct_capture_parameter(inner),
            _ => false,
        }
    }

    pub(super) fn reject_eager_capture_parameters(
        &self,
        resolved: &Resolved,
    ) -> Result<(), ResolveError> {
        if let Some(id) = collect_eager_references(resolved)
            .into_iter()
            .find(|id| self.capture_placeholder_ids.contains(&id.unique_id))
        {
            return Err(ResolveError {
                message: "capture placeholder is not available in a Lazy eager expression".into(),
                span: id.span,
                diagnostic: crate::error::ResolveErrorDiagnostic {
                    reason: crate::error::ResolveErrorReason::Capture,
                    subject: None,
                },
                related_labels: Vec::new(),
            });
        }
        Ok(())
    }

    pub(super) fn resolve_lazy_input(&mut self, expr: Ast) -> Result<Resolved, ResolveError> {
        let eager = matches!(&expr, Ast::Grouped(..)) && !self.is_direct_capture_parameter(&expr);
        let resolved = self.resolve_node(expr)?;
        if eager {
            // Preserve ordinary name-resolution diagnostic precedence.
            self.reject_eager_capture_parameters(&resolved)?;
        }
        Ok(resolved)
    }

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
                let then = self.resolve_lazy_input(then_expr)?;
                return Ok(Resolved::If(
                    span,
                    Box::new(cond),
                    Box::new(then),
                    Some(Box::new(self.resolve_lazy_input(else_expr)?)),
                ));
            }
            IfKind::IfThen2 => {
                let [cond_expr, then_expr] =
                    collect_fixed_positional_args(span.clone(), args, "if_then", 2)?;
                let cond = self.resolve_node(cond_expr)?;
                let then = self.resolve_lazy_input(then_expr)?;
                return Ok(Resolved::If(span, Box::new(cond), Box::new(then), None));
            }
        }
    }

    pub(super) fn resolve_require(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [cond_expr, err_expr] =
            collect_fixed_positional_args(span.clone(), args, "require", 2)?;
        let cond = self.resolve_node(cond_expr)?;
        let err = self.resolve_lazy_input(err_expr)?;
        Ok(Resolved::Require(span, Box::new(cond), Box::new(err)))
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
        let err = self.resolve_lazy_input(err_expr)?;
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
        let err = self.resolve_lazy_input(err_expr)?;
        Ok(Resolved::MapErr(span, Box::new(value), Box::new(err)))
    }

    pub(super) fn resolve_cause(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        let [value_expr, err_expr] = collect_fixed_positional_args(span.clone(), args, "cause", 2)?;
        let value = self.resolve_node(value_expr)?;
        let err = self.resolve_lazy_input(err_expr)?;
        Ok(Resolved::Cause(span, Box::new(value), Box::new(err)))
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
        let right = self.resolve_lazy_input(right_expr)?;
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
        AstPattern::HashMap(_, entries) => entries
            .iter()
            .any(|(_, child)| pattern_has_binding_vars(child)),
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
        | AstPattern::HashMap(span, _)
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
