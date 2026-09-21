use super::*;
use sindr::pattern::PatternConsumer;
use spire::ast::AstPatternArgument;

fn consumer_error(message: impl Into<String>, span: Span) -> ResolveError {
    ResolveError {
        message: message.into(),
        span,
        related_labels: Vec::new(),
        diagnostic: crate::error::ResolveErrorDiagnostic {
            reason: crate::error::ResolveErrorReason::SpecialForm,
            subject: None,
        },
    }
}

impl Resolver {
    fn resolve_pattern_consumer_identity(
        &mut self,
        callee: &Ast,
    ) -> Result<Option<PatternConsumer>, ResolveError> {
        let resolved = match callee {
            Ast::Var(span, name) => self.resolve_var_like(span.clone(), name.clone(), false)?,
            Ast::Path(span, path) => {
                self.resolve_var_like(span.clone(), path.segments.join("::"), false)?
            }
            _ => {
                return Err(consumer_error(
                    "Pattern consumer requires a named canonical callee",
                    callee.span().clone(),
                ))
            }
        };
        let Resolved::Var(_, id) = resolved else {
            return Err(consumer_error(
                "Pattern consumer must have a canonical declaration identity",
                callee.span().clone(),
            ));
        };
        let qualified = id.qualified_name.as_deref().or_else(|| {
            self.declaration_entry_for_uid(id.unique_id)
                .map(|entry| entry.fq_name.as_str())
        });
        if let Some(kind) = qualified.and_then(PatternConsumer::from_canonical_name) {
            return Ok(Some(kind));
        }
        // A reserved member spelling does not turn a canonical runtime builtin
        // (notably Regex::is_match) into a Pattern consumer.
        if let Some(qualified) = qualified {
            let member = qualified.rsplit("::").next().unwrap_or(qualified);
            if sindr::builtin::builtin_surface_variant_for_decl(member, Some(qualified)).is_some() {
                return Ok(None);
            }
        }
        Err(consumer_error("Reserved Pattern consumer name does not identify a canonical Kernel consumer or standard builtin", callee.span().clone()))
    }

    fn consumer_expression(arg: AstPatternArgument) -> Result<Ast, ResolveError> {
        arg.expression.map(|expr| *expr).ok_or_else(|| {
            super::patterns::deferred_pattern_parse_error(
                arg.expression_error,
                "consumer argument must be an expression",
                arg.span,
            )
        })
    }

    pub(super) fn resolve_pattern_consumer_call(
        &mut self,
        span: Span,
        callee: Box<Ast>,
        mut args: Vec<AstPatternArgument>,
    ) -> Result<Resolved, ResolveError> {
        let kind = self.resolve_pattern_consumer_identity(&callee)?;
        let Some(kind) = kind else {
            let args = args
                .into_iter()
                .map(|arg| Self::consumer_expression(arg).map(RecordLitArg::Positional))
                .collect::<Result<Vec<_>, _>>()?;
            return self.resolve_node(Ast::App(span, callee, args));
        };
        if args.len() != kind.arity() {
            return Err(consumer_error(
                format!(
                    "{} expects exactly {} positional arguments",
                    kind.name(),
                    kind.arity()
                ),
                span,
            ));
        }
        let arg = args.remove(kind.pattern_index());
        let term = Self::consumer_expression(args.remove(0))?;
        let mut args = args.into_iter();
        let pattern = arg.pattern.map(|pattern| *pattern).ok_or_else(|| {
            super::patterns::deferred_pattern_parse_error(
                arg.pattern_error,
                "consumer argument must be a Pattern",
                arg.span,
            )
        })?;
        if kind == PatternConsumer::ApplyPattern {
            let input = self.resolve_node(term)?;
            return self.with_child_scope(|child| {
                Ok(Resolved::ApplyPattern(
                    span,
                    Box::new(input),
                    child.resolve_pattern(pattern)?,
                ))
            });
        }
        let pattern = self.select_pattern_argument_roles(pattern)?;
        if kind == PatternConsumer::IsMatch
            && !self.pattern_has_deferred_application(&pattern)
            && super::special_forms::pattern_has_binding_vars(&pattern)
        {
            return Err(consumer_error("`is_match` pattern does not allow binding variables. Use `_` to ignore a value, or use `if_let` / `match` when you need bindings.", super::special_forms::ast_pattern_span(&pattern).clone()));
        }
        let (body, fallback) = match kind {
            PatternConsumer::IsMatch => (
                Ast::Lit(span.clone(), Lit::Bool(true)),
                Ast::Lit(span.clone(), Lit::Bool(false)),
            ),
            PatternConsumer::IfLet => (
                Self::consumer_expression(args.next().unwrap())?,
                Self::consumer_expression(args.next().unwrap())?,
            ),
            PatternConsumer::IfLetThen => (
                Ast::Block(
                    span.clone(),
                    vec![
                        Self::consumer_expression(args.next().unwrap())?,
                        Ast::Lit(span.clone(), Lit::Unit),
                    ],
                ),
                Ast::Lit(span.clone(), Lit::Unit),
            ),
            PatternConsumer::ApplyPattern => unreachable!(),
        };
        let resolved = self.resolve_node(Ast::Match(
            span.clone(),
            Box::new(term),
            vec![
                AstMatchArm {
                    pattern,
                    guard: None,
                    body,
                },
                AstMatchArm {
                    pattern: AstPattern::Wildcard(span.clone()),
                    guard: None,
                    body: fallback,
                },
            ],
        ))?;
        let Resolved::Match(span, input, mut arms) = resolved else {
            unreachable!()
        };
        if kind == PatternConsumer::IsMatch {
            if let ResolvedPattern::Deferred { allow_bindings, .. } = &mut arms[0].pattern {
                *allow_bindings = false;
            }
            Ok(Resolved::Match(span, input, arms))
        } else {
            Ok(Resolved::IfLet(span, input, arms))
        }
    }

    pub(super) fn prepare_pattern_consumer_pipe(
        &mut self,
        span: Span,
        callee: Box<Ast>,
        mut args: Vec<AstPatternArgument>,
    ) -> Result<Ast, ResolveError> {
        let Some(kind) = self.resolve_pattern_consumer_identity(&callee)? else {
            let args = args
                .into_iter()
                .map(|arg| Self::consumer_expression(arg).map(RecordLitArg::Positional))
                .collect::<Result<Vec<_>, _>>()?;
            return self.prepare_pipe_rhs(Ast::App(span, callee, args));
        };
        let param_name = format!("__pattern_pipe_{}_{}", span.start, span.end);
        let param = Ast::Var(span.clone(), param_name.clone());
        if args.len() + 1 == kind.arity() {
            args.insert(
                0,
                AstPatternArgument {
                    span: span.clone(),
                    expression: Some(Box::new(param)),
                    pattern: None,
                    expression_error: None,
                    pattern_error: None,
                },
            );
        } else if args.len() == kind.arity() {
            let mut slots = 0;
            for (index, arg) in args.iter_mut().enumerate() {
                if index == kind.pattern_index() {
                    continue;
                }
                if matches!(
                    arg.expression.as_deref(),
                    Some(Ast::NumberedPlaceholder(_, 1))
                ) {
                    slots += 1;
                    arg.expression = Some(Box::new(param.clone()));
                }
            }
            if slots != 1 {
                return Err(consumer_error("Pattern consumer pipe requires one direct Expr argument slot; pipe injection into a Pattern is not allowed", span));
            }
        } else {
            return Err(consumer_error(
                format!("{} has invalid pipe argument count", kind.name()),
                span,
            ));
        }
        Ok(Ast::Closure(
            span.clone(),
            vec![ClosureParam {
                name: param_name,
                ty: None,
                span: span.clone(),
            }],
            Box::new(Ast::PatternConsumerCall(span, callee, args)),
        ))
    }
}
