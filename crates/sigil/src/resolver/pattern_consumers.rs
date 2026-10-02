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

#[cfg(test)]
mod pipe_contract_tests {
    use super::*;
    use spire::ast::AstPath;

    fn named_target(resolver: &mut Resolver, owner: &str, member: &str) -> Box<Ast> {
        let span = Span { start: 0, end: 1 };
        let name = format!("{owner}::{member}");
        let uid = resolver.scope.define(&name, span.clone());
        resolver.declaration_uids.insert(name, uid);
        Box::new(Ast::Path(
            span.clone(),
            AstPath {
                span,
                segments: vec![owner.into(), member.into()],
            },
        ))
    }

    #[test]
    fn consumer_pipe_inserts_first_without_selecting_by_arity() {
        let mut resolver = Resolver::new();
        let callee = named_target(&mut resolver, "Kernel", "is_match");
        let span = Span { start: 0, end: 1 };
        let argument = AstPatternArgument {
            span: span.clone(),
            expression: Some(Box::new(Ast::Lit(span.clone(), Lit::Bool(true)))),
            pattern: None,
            named_pattern: None,
            expression_error: None,
            pattern_error: None,
        };
        let rewritten = resolver
            .prepare_pattern_consumer_pipe(span, callee, vec![argument.clone(), argument])
            .expect("insertion is determined before arity validation");
        assert!(matches!(rewritten, Ast::Closure(_, _, body)
            if matches!(*body, Ast::PatternConsumerCall(_, _, ref args) if args.len() == 3)));
    }

    #[test]
    fn consumer_syntax_rejects_an_ordinary_builtin_identity() {
        let mut resolver = Resolver::new();
        let callee = named_target(&mut resolver, "Regex", "is_match");
        let error = resolver
            .resolve_pattern_consumer_identity(&callee)
            .expect_err("consumer syntax cannot become an ordinary call");
        assert!(
            error.message.contains("canonical Kernel consumer"),
            "{error:?}"
        );
    }
}

impl Resolver {
    pub(super) fn resolve_pattern_consumer_identity(
        &mut self,
        callee: &Ast,
    ) -> Result<PatternConsumer, ResolveError> {
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
            return Ok(kind);
        }
        Err(consumer_error(
            "Pattern consumer syntax does not identify the canonical Kernel consumer",
            callee.span().clone(),
        ))
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
                Self::consumer_expression(args.next().unwrap())?,
                Ast::Lit(span.clone(), Lit::Unit),
            ),
            PatternConsumer::ApplyPattern => unreachable!(),
        };
        // A Lazy argument's grouping is evaluated in the scope before matching.
        // Grouped synthetic capture parameters are ordinary parameter references.
        let eager_body = if matches!(&body, Ast::Grouped(..))
            && !self.is_direct_capture_parameter(&body)
        {
            Some(self.resolve_lazy_input(body.clone())?)
        } else {
            None
        };
        let eager_fallback = matches!(&fallback, Ast::Grouped(..))
            && !self.is_direct_capture_parameter(&fallback);
        let resolving_body = if eager_body.is_some() {
            Ast::Lit(span.clone(), Lit::Unit)
        } else {
            body
        };
        let resolved = self.resolve_node(Ast::Match(
            span.clone(),
            Box::new(term),
            vec![
                AstMatchArm {
                    pattern,
                    guard: None,
                    body: resolving_body,
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
        if let Some(body) = eager_body {
            arms[0].body = body;
        }
        if eager_fallback {
            self.reject_eager_capture_parameters(&arms[1].body)?;
        }
        if kind == PatternConsumer::IsMatch {
            if let ResolvedPattern::Deferred { allow_bindings, .. } = &mut arms[0].pattern {
                *allow_bindings = false;
            }
            Ok(Resolved::IsMatch(span, input, arms))
        } else {
            Ok(Resolved::IfLet(
                span,
                input,
                arms,
                kind == PatternConsumer::IfLetThen,
            ))
        }
    }

    pub(super) fn prepare_pattern_consumer_pipe(
        &mut self,
        span: Span,
        callee: Box<Ast>,
        mut args: Vec<AstPatternArgument>,
    ) -> Result<Ast, ResolveError> {
        let kind = self.resolve_pattern_consumer_identity(&callee)?;
        let slot =
            Self::pipe_argument_slot(&span, args.iter().map(|arg| arg.expression.as_deref()))?;
        if slot == Some(kind.pattern_index()) {
            return Err(consumer_error(
                "pipe injection into a Pattern is not allowed",
                span,
            ));
        }
        if slot.is_some_and(|index| kind.is_lazy_argument(index)) {
            return Err(consumer_error(
                "pipe injection into a Lazy parameter is not allowed",
                span,
            ));
        }
        let param_name = Self::pipe_slot_param_name(&span);
        let argument = AstPatternArgument {
            span: span.clone(),
            expression: Some(Box::new(Ast::Var(span.clone(), param_name.clone()))),
            pattern: None,
            named_pattern: None,
            expression_error: None,
            pattern_error: None,
        };
        Self::insert_pipe_argument(&mut args, slot, argument);
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
