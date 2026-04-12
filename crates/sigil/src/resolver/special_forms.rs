use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IfKind {
    If3,
    IfThen2,
}

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
        let (expected_arity, callee_name) = match kind {
            IfKind::If3 => (3usize, "if"),
            IfKind::IfThen2 => (2usize, "if_then"),
        };
        if args.len() != expected_arity {
            return Err(ResolveError {
                message: format!(
                    "{} expects {} arguments, got {}",
                    callee_name,
                    expected_arity,
                    args.len()
                ),
                span,
            });
        }

        let mut positional = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(expr) => positional.push(expr),
                RecordLitArg::Named(name, _) => {
                    return Err(ResolveError {
                        message: format!(
                            "{} does not accept named argument '{}'",
                            callee_name, name
                        ),
                        span,
                    });
                }
            }
        }

        let mut iter = positional.into_iter();
        let cond = self.resolve_node(iter.next().expect("checked arg length"))?;
        let then = self.resolve_node(iter.next().expect("checked arg length"))?;
        let else_branch = match kind {
            IfKind::If3 => Some(Box::new(
                self.resolve_node(iter.next().expect("checked arg length"))?,
            )),
            IfKind::IfThen2 => None,
        };
        Ok(Resolved::If(
            span,
            Box::new(cond),
            Box::new(then),
            else_branch,
        ))
    }

    pub(super) fn resolve_assert(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        if args.len() != 2 {
            return Err(ResolveError {
                message: format!("assert expects 2 arguments, got {}", args.len()),
                span,
            });
        }

        let mut positional = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(expr) => positional.push(expr),
                RecordLitArg::Named(name, _) => {
                    return Err(ResolveError {
                        message: format!("assert does not accept named argument '{}'", name),
                        span,
                    });
                }
            }
        }

        let mut iter = positional.into_iter();
        let cond = self.resolve_node(iter.next().expect("checked arg length"))?;
        let err = self.resolve_node(iter.next().expect("checked arg length"))?;
        Ok(Resolved::Assert(span, Box::new(cond), Box::new(err)))
    }

    pub(super) fn resolve_ensure(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
    ) -> Result<Resolved, ResolveError> {
        if args.len() != 3 {
            return Err(ResolveError {
                message: format!("ensure expects 3 arguments, got {}", args.len()),
                span,
            });
        }

        let mut positional = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(expr) => positional.push(expr),
                RecordLitArg::Named(name, _) => {
                    return Err(ResolveError {
                        message: format!("ensure does not accept named argument '{}'", name),
                        span,
                    });
                }
            }
        }

        let mut iter = positional.into_iter();
        let value = self.resolve_node(iter.next().expect("checked arg length"))?;
        let pred = self.resolve_node(iter.next().expect("checked arg length"))?;
        let err = self.resolve_node(iter.next().expect("checked arg length"))?;
        Ok(Resolved::Ensure(
            span,
            Box::new(value),
            Box::new(pred),
            Box::new(err),
        ))
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
        let positional = collect_positional_args(span.clone(), args, callee_name, 2)?;
        let mut iter = positional.into_iter();
        let left = self.resolve_node(iter.next().expect("checked arg length"))?;
        let right = self.resolve_node(iter.next().expect("checked arg length"))?;
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
                });
            }
        }
    }
    Ok(positional)
}
