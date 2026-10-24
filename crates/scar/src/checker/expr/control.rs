use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

impl Checker {
    pub(super) fn check_block(
        &mut self,
        span: &Span,
        stmts: &[Resolved],
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let inherited_substitutions = self.substitutions.clone();
        let mut typed_stmts = Vec::new();
        let mut last_ty = Ty::Unit;
        for (index, stmt) in stmts.iter().enumerate() {
            // Inference substitutions are statement-local inside blocks too.
            // Retain the surrounding callable contract while preventing an
            // earlier generic call from monomorphizing later siblings.
            self.substitutions = inherited_substitutions.clone();
            let is_last = index + 1 == stmts.len();
            let typed = if is_last {
                match expected {
                    Some(expected_ty) => self.check_node_with_expected_relation(
                        stmt,
                        Some(expected_ty),
                        expected_relation,
                    ),
                    None => self.check_node(stmt),
                }
            } else {
                self.check_node(stmt)
            }?;
            if !is_last {
                self.ensure_unit_statement(&typed)?;
            }
            last_ty = typed.ty.clone();
            typed_stmts.push(typed);
        }
        if let Some(last) = typed_stmts.last() {
            self.propagate_safe_operator_result(&last.span, span);
        }
        Ok(TypedNode {
            ty: last_ty,
            span: span.clone(),
            node: TypedInner::Block(typed_stmts),
        })
    }

    pub(in crate::checker) fn ensure_unit_statement(
        &self,
        typed: &TypedNode,
    ) -> Result<(), TypeError> {
        if self.resolve_ty(&typed.ty) == Ty::Unit {
            return Ok(());
        }
        Err(self.policy_error(
            TypeDiagnosticReason::NonUnitStatement,
            diagnostics::TypePolicy::NonFinalStatementUnit,
            None,
            Some(&Ty::Unit),
            Some(&typed.ty),
            None,
            None,
            &typed.span,
            Some("Add `;` or bind the value with `_ = expr` to discard it explicitly.".into()),
        ))
    }

    pub(in crate::checker) fn check_branch_node_with_expected(
        &mut self,
        node: &Resolved,
        expected: &Ty,
    ) -> Result<TypedNode, TypeError> {
        if self.branch_tail_is_return_type_argument_call(node) {
            let checkpoint = self.candidate_probe_checkpoint();
            match self.check_node(node) {
                Ok(mut typed) => {
                    self.coerce_enum_error_receiver(node, &mut typed, expected)?;
                    return Ok(typed);
                }
                Err(error)
                    if error.reason()
                        == Some(TypeDiagnosticReason::AmbiguousReturnTypeArgument) =>
                {
                    self.rollback_candidate_probe(checkpoint);
                }
                Err(error) => {
                    self.rollback_candidate_probe(checkpoint);
                    return Err(error);
                }
            }
        }
        self.check_node_with_expected(node, Some(expected))
    }

    pub(super) fn branch_tail_is_return_type_argument_call(&self, node: &Resolved) -> bool {
        match node {
            Resolved::Closure(_, _, _, body)
            | Resolved::ExtractorClosure(_, _, _, body)
            | Resolved::CaptureClosure(_, _, _, body) => {
                self.body_tail_is_return_type_argument_call(body)
            }
            _ => self.body_tail_is_return_type_argument_call(node),
        }
    }

    pub(super) fn check_if_with_expected(
        &mut self,
        span: &Span,
        cond: &Resolved,
        then: &Resolved,
        else_opt: &Option<Box<Resolved>>,
        expected: &Ty,
    ) -> Result<TypedNode, TypeError> {
        self.check_if_branches(span, cond, then, else_opt, Some(expected))
    }

    pub(in crate::checker) fn check_if(
        &mut self,
        span: &Span,
        cond: &Resolved,
        then: &Resolved,
        else_opt: &Option<Box<Resolved>>,
    ) -> Result<TypedNode, TypeError> {
        self.check_if_branches(span, cond, then, else_opt, None)
    }

    pub(super) fn check_if_branches(
        &mut self,
        span: &Span,
        cond: &Resolved,
        then: &Resolved,
        else_opt: &Option<Box<Resolved>>,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let callable_expected = self.contextual_callable_expected(expected);
        let typed_cond = self.check_node(cond)?;
        self.assert_type_relation(
            &Ty::Bool,
            &typed_cond.ty,
            self.type_fact(SourceRole::Expected, span, &Ty::Bool),
            self.type_fact(SourceRole::Value, &typed_cond.span, &typed_cond.ty),
            TypeDiagnosticReason::ArgumentTypeMismatch,
            DiagnosticOrigin::Branch {
                form: diagnostics::BranchForm::If,
                ordinal: 0,
            },
            "if",
            0,
        )?;
        let (typed_then, typed_else) = if let Some(branch) = else_opt {
            let then_input = self.check_lazy_input(then, expected, span)?;
            let else_input = self.check_lazy_input(branch, expected, span)?;
            let (then, other) = self.normalize_lazy_pair(then_input, else_input, expected, span)?;
            (then, Some(other))
        } else {
            (
                self.check_lazy_argument_with_expected(then, &Ty::Unit, span)?,
                None,
            )
        };
        if let Some(other) = &typed_else {
            if let Some(expected) = &callable_expected {
                for (ordinal, branch) in [&typed_then, other].into_iter().enumerate() {
                    self.assert_type_relation(
                        expected,
                        &branch.ty,
                        self.type_fact(SourceRole::Expected, span, expected),
                        self.branch_fact(SourceRole::Branch, branch, ordinal),
                        TypeDiagnosticReason::IfBranchTypeMismatch,
                        DiagnosticOrigin::Branch {
                            form: diagnostics::BranchForm::If,
                            ordinal: ordinal as u32,
                        },
                        "if",
                        ordinal as u32,
                    )
                    .map_err(|error| {
                        self.complete_branch_error(
                            error,
                            &[&typed_then, other],
                            &[Some(&typed_cond), None],
                        )
                    })?;
                }
            } else {
                self.assert_type_relation(
                    &typed_then.ty,
                    &other.ty,
                    self.branch_fact(SourceRole::Branch, &typed_then, 0),
                    self.branch_fact(SourceRole::Branch, other, 1),
                    TypeDiagnosticReason::IfBranchTypeMismatch,
                    DiagnosticOrigin::Branch {
                        form: diagnostics::BranchForm::If,
                        ordinal: 1,
                    },
                    "if",
                    1,
                )
                .map_err(|error| {
                    self.complete_branch_error(
                        error,
                        &[&typed_then, other],
                        &[Some(&typed_cond), None],
                    )
                })?;
            }
        }
        if typed_else.is_none() {
            self.assert_type_relation(
                &Ty::Unit,
                &typed_then.ty,
                self.type_fact(SourceRole::Expected, span, &Ty::Unit),
                self.type_fact(SourceRole::Value, &typed_then.span, &typed_then.ty),
                TypeDiagnosticReason::IfBranchTypeMismatch,
                DiagnosticOrigin::Branch {
                    form: diagnostics::BranchForm::If,
                    ordinal: 0,
                },
                "if_then",
                0,
            )?;
        }
        let ty = if typed_else.is_some() {
            callable_expected.unwrap_or_else(|| typed_then.ty.clone())
        } else {
            Ty::Unit
        };
        self.record_lazy_capture_signature(span, &ty);
        if let Some(expected) = expected {
            self.assert_type_relation(
                expected,
                &ty,
                self.type_fact(SourceRole::Expected, span, expected),
                self.type_fact(SourceRole::Value, span, &ty),
                TypeDiagnosticReason::IfBranchTypeMismatch,
                DiagnosticOrigin::Branch {
                    form: diagnostics::BranchForm::If,
                    ordinal: 0,
                },
                "if",
                0,
            )
            .map_err(|error| {
                let mut bodies = vec![&typed_then];
                if let Some(other) = &typed_else {
                    bodies.push(other);
                }
                self.complete_branch_error(error, &bodies, &[Some(&typed_cond)])
            })?;
        }
        Ok(TypedNode {
            ty: self.resolve_ty(&ty),
            span: span.clone(),
            node: TypedInner::If(
                Box::new(typed_cond),
                Box::new(typed_then),
                typed_else.map(Box::new),
            ),
        })
    }

    pub(in crate::checker) fn check_dbg(
        &mut self,
        span: &Span,
        args: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        let typed_args = args
            .iter()
            .map(|arg| {
                let expr = self.check_node(arg)?;
                Ok(TypedDbgArg {
                    span: expr.span.clone(),
                    ty_name: self.ty_name(&expr.ty),
                    expr,
                })
            })
            .collect::<Result<Vec<_>, TypeError>>()?;

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Dbg(typed_args),
        })
    }

    pub(in crate::checker) fn check_cause(
        &mut self,
        span: &Span,
        value: &Resolved,
        err: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_value = self.check_result_value(value, "cause", None)?;
        let typed_err = self.check_lazy_argument_with_expected(err, &Ty::Error, span)?;
        self.ensure_result_error_arg(&typed_err, "cause")?;

        Ok(TypedNode {
            ty: typed_value.ty.clone(),
            span: span.clone(),
            node: TypedInner::Cause(Box::new(typed_value), Box::new(typed_err)),
        })
    }

    pub(super) fn check_result_value(
        &mut self,
        value: &Resolved,
        form_name: &str,
        expected_ok_ty: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let expected_result_ty = Ty::Result(
            Box::new(
                expected_ok_ty
                    .cloned()
                    .unwrap_or_else(|| self.env.fresh_tyvar()),
            ),
            Box::new(Ty::Error),
        );
        let typed_value = self.check_node_with_expected(value, Some(&expected_result_ty))?;
        let value_ty = self.resolve_ty(&typed_value.ty);
        if !self.types_compatible(&expected_result_ty, &value_ty)? {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} value must be Result<...>, got {}",
                    form_name,
                    self.ty_name(&value_ty)
                ),
                span: typed_value.span.clone(),
                hint: None,
            });
        }
        Ok(typed_value)
    }

    pub(super) fn bind_local_error_identity(&mut self, pattern: &TypedPattern, kind: &str) {
        match pattern {
            TypedPattern::Var(_, binding) => {
                self.env
                    .concrete_error_bindings
                    .insert(binding.unique_id, kind.to_string());
            }
            TypedPattern::Located(_, inner) => self.bind_local_error_identity(inner, kind),
            _ => {}
        }
    }
}
