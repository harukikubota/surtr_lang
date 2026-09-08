//! Type assertions commit on success and preserve both source facts on failure.
use super::*;
use diagnostics::{
    ArgumentRelationData, BranchAssertionData, DiagnosticData, DiagnosticOrigin, SourceFact,
    SourceId, SourceRole, StructuredDiagnostic, TypeDiagnosticReason,
};

struct TypeRelationCheckpoint {
    substitutions: Vec<(u32, Option<Ty>)>,
    tyvar_bounds: Vec<(u32, Option<Vec<String>>)>,
    pending_trait_obligations: Vec<(u32, Option<Vec<PendingTraitObligation>>)>,
}

impl Checker {
    fn type_relation_checkpoint(&self, variables: &[u32]) -> TypeRelationCheckpoint {
        let mut tracked = variables.to_vec();
        let mut cursor = 0;
        while cursor < tracked.len() {
            let variable = tracked[cursor];
            Self::collect_ty_vars(&self.resolve_ty(&Ty::Var(variable)), &mut tracked);
            if let Some(root) = self.constructor_family_witness_root(variable) {
                if !tracked.contains(&root) {
                    tracked.push(root);
                }
                Self::collect_ty_vars(&self.resolve_ty(&Ty::Var(root)), &mut tracked);
            }
            cursor += 1;
        }
        TypeRelationCheckpoint {
            substitutions: tracked
                .iter()
                .map(|var| (*var, self.substitutions.get(var).cloned()))
                .collect(),
            tyvar_bounds: tracked
                .iter()
                .map(|var| (*var, self.tyvar_bounds.get(var).cloned()))
                .collect(),
            pending_trait_obligations: tracked
                .iter()
                .map(|var| (*var, self.pending_trait_obligations.get(var).cloned()))
                .collect(),
        }
    }

    fn rollback_type_relation(&mut self, checkpoint: TypeRelationCheckpoint) {
        for (var, value) in checkpoint.substitutions {
            match value {
                Some(value) => {
                    self.substitutions.insert(var, value);
                }
                None => {
                    self.substitutions.remove(&var);
                }
            }
        }
        for (var, value) in checkpoint.tyvar_bounds {
            match value {
                Some(value) => {
                    self.tyvar_bounds.insert(var, value);
                }
                None => {
                    self.tyvar_bounds.remove(&var);
                }
            }
        }
        for (var, value) in checkpoint.pending_trait_obligations {
            match value {
                Some(value) => {
                    self.pending_trait_obligations.insert(var, value);
                }
                None => {
                    self.pending_trait_obligations.remove(&var);
                }
            }
        }
    }
}

impl Checker {
    pub(super) fn assert_type_relation(
        &mut self,
        expected: &Ty,
        actual: &Ty,
        expected_fact: SourceFact,
        actual_fact: SourceFact,
        reason: TypeDiagnosticReason,
        origin: DiagnosticOrigin,
        callable: &str,
        ordinal: u32,
    ) -> Result<(), TypeError> {
        // Concrete and rigid-only comparisons cannot bind inference variables.
        // Avoid cloning the candidate-probe state for declaration-owned generics:
        // they are common in standard-library bodies but are immutable here.
        let mut variables = Vec::new();
        Self::collect_ty_vars(expected, &mut variables);
        Self::collect_ty_vars(actual, &mut variables);
        let checkpoint = variables
            .iter()
            .any(|var| {
                !self.rigid_tyvars.contains(var)
                    && matches!(self.resolve_ty(&Ty::Var(*var)), Ty::Var(_))
            })
            .then(|| self.type_relation_checkpoint(&variables));
        if self.types_compatible(expected, actual) {
            return Ok(());
        }
        if let Some(checkpoint) = checkpoint {
            self.rollback_type_relation(checkpoint);
        }
        if reason == TypeDiagnosticReason::ArgumentTypeMismatch {
            if let Some((trait_name, subject)) = self.unsatisfied_relation_bound(expected, actual) {
                let bound_reason = match self.resolve_ty(&subject) {
                    Ty::Var(var) if self.rigid_tyvars.contains(&var) => {
                        TypeDiagnosticReason::MissingGenericBound
                    }
                    _ => TypeDiagnosticReason::MissingTraitCapability,
                };
                let mut error = self.trait_failure(
                    bound_reason,
                    &trait_name,
                    &subject,
                    &actual_fact.span,
                    origin,
                );
                let diagnostic = error.structured.as_mut().expect("structured trait failure");
                if let DiagnosticData::TraitObligation(data) = &mut diagnostic.data {
                    data.obligation_origin = Some(actual_fact.clone());
                }
                diagnostic.primary = actual_fact;
                diagnostic.related.push(expected_fact);
                return Err(error);
            }
        }
        Err(self.type_relation_error(
            expected,
            actual,
            expected_fact,
            actual_fact,
            reason,
            origin,
            callable,
            ordinal,
        ))
    }

    pub(super) fn type_fact(&self, role: SourceRole, span: &Span, ty: &Ty) -> SourceFact {
        SourceFact::typed(
            role,
            SourceId(0),
            span.clone(),
            self.diagnostic_ty_name(&self.resolve_ty(ty)),
        )
    }

    pub(super) fn type_relation_error(
        &self,
        expected: &Ty,
        actual: &Ty,
        mut expected_fact: SourceFact,
        mut actual_fact: SourceFact,
        reason: TypeDiagnosticReason,
        origin: DiagnosticOrigin,
        callable: &str,
        ordinal: u32,
    ) -> TypeError {
        let types =
            self.diagnostic_ty_names(&[&self.resolve_ty(expected), &self.resolve_ty(actual)]);
        if !matches!(
            expected_fact.role,
            SourceRole::LeftValue | SourceRole::RightValue
        ) {
            expected_fact.ty = Some(types[0].clone());
        }
        if !matches!(
            actual_fact.role,
            SourceRole::LeftValue | SourceRole::RightValue
        ) {
            actual_fact.ty = Some(types[1].clone());
        }
        let data = if matches!(
            reason,
            TypeDiagnosticReason::IfBranchTypeMismatch
                | TypeDiagnosticReason::MatchArmTypeMismatch
                | TypeDiagnosticReason::CondBranchTypeMismatch
        ) {
            DiagnosticData::BranchAssertion(BranchAssertionData {
                form: match origin {
                    DiagnosticOrigin::Branch { form, .. } => form,
                    _ => unreachable!("branch assertion origin"),
                },
                left_ordinal: expected_fact.ordinal,
                right_ordinal: actual_fact.ordinal,
                left_origin: Some(expected_fact.clone()),
                right_origin: Some(actual_fact.clone()),
                expected_type: types[0].clone(),
                actual_type: types[1].clone(),
                branch: Some(ordinal),
            })
        } else {
            DiagnosticData::ArgumentRelation(ArgumentRelationData {
                expected_origin: Some(expected_fact.clone()),
                actual_origin: Some(actual_fact.clone()),
                callable: callable.into(),
                ordinal,
                expected_type: Some(types[0].clone()),
                actual_type: Some(types[1].clone()),
            })
        };
        TypeError::from_structured(StructuredDiagnostic {
            reason,
            origin,
            data,
            primary: actual_fact,
            related: vec![expected_fact],
            remediation: None,
        })
    }
}

impl Checker {
    pub(super) fn check_cond(
        &mut self,
        span: &Span,
        clauses: &[(Resolved, Resolved)],
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let mut failure = None;
        let mut checked: Vec<(TypedNode, TypedNode)> = Vec::new();
        for (ordinal, (condition, body)) in clauses.iter().enumerate() {
            let condition = self.check_node(condition)?;
            self.assert_type_relation(
                &Ty::Bool,
                &condition.ty,
                self.type_fact(SourceRole::Expected, &condition.span, &Ty::Bool),
                self.type_fact(SourceRole::Value, &condition.span, &condition.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Branch {
                    form: diagnostics::BranchForm::Cond,
                    ordinal: ordinal as u32,
                },
                "cond",
                ordinal as u32,
            )?;
            let body = match expected {
                Some(expected) => self.check_lazy_argument_with_expected(body, expected, span)?,
                None => self.check_lazy_argument(body, span)?,
            };
            if let Some((_, first)) = checked.first() {
                let relation = self.assert_type_relation(
                    &first.ty,
                    &body.ty,
                    self.branch_fact(SourceRole::Branch, first, 0),
                    self.branch_fact(SourceRole::Branch, &body, ordinal),
                    TypeDiagnosticReason::CondBranchTypeMismatch,
                    DiagnosticOrigin::Branch {
                        form: diagnostics::BranchForm::Cond,
                        ordinal: ordinal as u32,
                    },
                    "cond",
                    ordinal as u32,
                );
                if failure.is_none() {
                    failure = relation.err();
                }
            }
            if let Some(expected) = expected {
                let relation = self.assert_type_relation(
                    expected,
                    &body.ty,
                    self.type_fact(SourceRole::Expected, span, expected),
                    self.branch_fact(SourceRole::Branch, &body, ordinal),
                    TypeDiagnosticReason::CondBranchTypeMismatch,
                    DiagnosticOrigin::Branch {
                        form: diagnostics::BranchForm::Cond,
                        ordinal: ordinal as u32,
                    },
                    "cond",
                    ordinal as u32,
                );
                if failure.is_none() {
                    failure = relation.err();
                }
            }
            checked.push((condition, body));
        }
        if let Some(error) = failure {
            return Err(self.complete_branch_error(
                error,
                &checked.iter().map(|(_, body)| body).collect::<Vec<_>>(),
                &checked
                    .iter()
                    .map(|(guard, _)| Some(guard))
                    .collect::<Vec<_>>(),
            ));
        }
        let (_, mut tail) = checked
            .pop()
            .ok_or_else(|| TypeError::new("cond requires at least one clause", span.clone()))?;
        while let Some((condition, body)) = checked.pop() {
            tail = TypedNode {
                ty: tail.ty.clone(),
                span: span.clone(),
                node: TypedInner::If(Box::new(condition), Box::new(body), Some(Box::new(tail))),
            };
        }
        Ok(tail)
    }
}

impl Checker {
    pub(super) fn argument_contract_diagnostic(
        &self,
        reason: TypeDiagnosticReason,
        callable: &str,
        name: Option<&str>,
        expected: usize,
        actual: usize,
        span: &Span,
        origin: DiagnosticOrigin,
    ) -> StructuredDiagnostic {
        StructuredDiagnostic {
            reason,
            origin,
            data: DiagnosticData::ArgumentContract(diagnostics::ArgumentContractData {
                callable: callable.into(),
                name: name.map(str::to_string),
                expected_count: expected as u32,
                actual_count: actual as u32,
            }),
            primary: SourceFact::untyped(SourceRole::CallTarget, SourceId(0), span.clone()),
            related: vec![],
            remediation: None,
        }
    }
    pub(super) fn trait_failure(
        &self,
        reason: TypeDiagnosticReason,
        trait_name: &str,
        subject: &Ty,
        span: &Span,
        origin: DiagnosticOrigin,
    ) -> TypeError {
        if reason == TypeDiagnosticReason::MissingTypeConstructorCapability {
            return TypeError::from_structured(StructuredDiagnostic {
                reason,
                origin,
                data: DiagnosticData::TypeConstructorCarrier(
                    diagnostics::TypeConstructorCarrierData {
                        left_type: None,
                        right_type: Some(self.diagnostic_ty_name(&self.resolve_ty(subject))),
                        left_origin: None,
                        right_origin: Some(self.type_fact(SourceRole::Value, span, subject)),
                        required_capability: self.trait_display_name(trait_name),
                        family: self.trait_display_name(trait_name),
                        family_id: self.diagnostic_constructor_family_id(trait_name),
                        expected_carrier: self.diagnostic_ty_name(&self.resolve_ty(subject)),
                        actual_carrier: self.diagnostic_ty_name(&self.resolve_ty(subject)),
                    },
                ),
                primary: self.type_fact(SourceRole::Value, span, subject),
                related: vec![],
                remediation: None,
            });
        }
        self.trait_obligation_failure(reason, trait_name, &[], None, subject, span, origin)
    }
}

impl Checker {
    pub(super) fn callable_shape_error(
        &self,
        ty: &Ty,
        callable: &str,
        span: &Span,
        arity: Option<usize>,
        return_shape: diagnostics::CallableReturnShape,
    ) -> TypeError {
        let actual_arity = self
            .function_parts(ty)
            .map(|(params, _)| params.len() as u32);
        let reason =
            if actual_arity.is_none() && return_shape == diagnostics::CallableReturnShape::Any {
                TypeDiagnosticReason::NotCallable
            } else {
                TypeDiagnosticReason::CallableShapeMismatch
            };
        TypeError::from_structured(StructuredDiagnostic {
            reason,
            origin: DiagnosticOrigin::Call,
            data: DiagnosticData::CallableShape(diagnostics::CallableShapeData {
                callable: callable.into(),
                actual_type: Some(self.diagnostic_ty_name(&self.resolve_ty(ty))),
                expected_arity: arity.map(|arity| arity as u32),
                actual_arity,
                return_shape,
            }),
            primary: self.type_fact(SourceRole::CallTarget, span, ty),
            related: vec![],
            remediation: None,
        })
    }

    pub(super) fn assert_operand_relation(
        &mut self,
        expected: &Ty,
        actual: &Ty,
        left: &TypedNode,
        right: &TypedNode,
        reason: TypeDiagnosticReason,
        callable: &str,
        operator: &str,
        capability: Option<&str>,
        expected_side: SourceRole,
    ) -> Result<(), TypeError> {
        if matches!(self.resolve_ty(expected), Ty::Hole) {
            return Ok(());
        }
        let (expected_fact, actual_fact) = self.operand_facts(left, right, expected_side);
        self.assert_type_relation(
            expected,
            actual,
            expected_fact.clone(),
            actual_fact.clone(),
            reason,
            DiagnosticOrigin::Operator {
                operator: operator.into(),
            },
            callable,
            1,
        )
        .map_err(|mut error| {
            if reason == TypeDiagnosticReason::TypePayloadMismatch {
                let capability =
                    capability.expect("payload assertion carries its constructor capability");
                let diagnostic = error.structured.as_mut().expect("typed payload failure");
                diagnostic.data = DiagnosticData::TypeConstructorCarrier(
                    diagnostics::TypeConstructorCarrierData {
                        left_type: Some(self.diagnostic_ty_name(&self.resolve_ty(&left.ty))),
                        right_type: Some(self.diagnostic_ty_name(&self.resolve_ty(&right.ty))),
                        left_origin: Some(self.type_fact(
                            SourceRole::LeftValue,
                            &left.span,
                            &left.ty,
                        )),
                        right_origin: Some(self.type_fact(
                            SourceRole::RightValue,
                            &right.span,
                            &right.ty,
                        )),
                        required_capability: self.trait_display_name(capability),
                        family: self.trait_display_name(capability),
                        family_id: self.diagnostic_constructor_family_id(capability),
                        expected_carrier: self.diagnostic_ty_name(&self.resolve_ty(expected)),
                        actual_carrier: self.diagnostic_ty_name(&self.resolve_ty(actual)),
                    },
                );
                return TypeError::from_structured(diagnostic.clone());
            }
            error
        })
    }
}

impl Checker {
    fn unsatisfied_relation_bound(&mut self, expected: &Ty, actual: &Ty) -> Option<(String, Ty)> {
        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(actual);
        match (&expected, &actual) {
            (Ty::Var(var), subject) => self
                .tyvar_bound_names(*var)
                .into_iter()
                .find(|bound| !self.ty_satisfies_bounds(subject, std::slice::from_ref(bound)))
                .map(|bound| (bound, subject.clone())),
            (Ty::List(a), Ty::List(b)) | (Ty::Lazy(a), Ty::Lazy(b)) => {
                self.unsatisfied_relation_bound(a, b)
            }
            (Ty::Result(a, e), Ty::Result(b, f)) => self
                .unsatisfied_relation_bound(a, b)
                .or_else(|| self.unsatisfied_relation_bound(e, f)),
            (Ty::Tuple(a), Ty::Tuple(b)) | (Ty::Enum(_, a), Ty::Enum(_, b))
                if a.len() == b.len() =>
            {
                a.iter()
                    .zip(b)
                    .find_map(|(a, b)| self.unsatisfied_relation_bound(a, b))
            }
            (Ty::Func(a, r), Ty::Func(b, s)) if a.len() == b.len() => a
                .iter()
                .zip(b)
                .find_map(|(a, b)| self.unsatisfied_relation_bound(a, b))
                .or_else(|| self.unsatisfied_relation_bound(r, s)),
            _ => None,
        }
    }
}

impl Checker {
    pub(super) fn assert_carrier_relation(
        &mut self,
        expected: &Ty,
        actual: &Ty,
        left: &TypedNode,
        right: &TypedNode,
        trait_name: &str,
        operator: &str,
        expected_side: SourceRole,
    ) -> Result<(), TypeError> {
        let (expected_fact, actual_fact) = self.operand_facts(left, right, expected_side);
        if let Err(mut error) = self.assert_type_relation(
            expected,
            actual,
            expected_fact,
            actual_fact,
            TypeDiagnosticReason::TypeConstructorFamilyMismatch,
            DiagnosticOrigin::Operator {
                operator: operator.into(),
            },
            trait_name,
            1,
        ) {
            let diagnostic = error.structured.as_mut().expect("relation diagnostic");
            diagnostic.data =
                DiagnosticData::TypeConstructorCarrier(diagnostics::TypeConstructorCarrierData {
                    left_type: Some(self.diagnostic_ty_name(&self.resolve_ty(&left.ty))),
                    right_type: Some(self.diagnostic_ty_name(&self.resolve_ty(&right.ty))),
                    left_origin: Some(self.type_fact(SourceRole::LeftValue, &left.span, &left.ty)),
                    right_origin: Some(self.type_fact(
                        SourceRole::RightValue,
                        &right.span,
                        &right.ty,
                    )),
                    required_capability: self.trait_display_name(trait_name),
                    family: self.trait_display_name(trait_name),
                    family_id: self.diagnostic_constructor_family_id(trait_name),
                    expected_carrier: self.diagnostic_ty_name(&self.resolve_ty(expected)),
                    actual_carrier: self.diagnostic_ty_name(&self.resolve_ty(actual)),
                });
            return Err(TypeError::from_structured(diagnostic.clone()));
        }
        Ok(())
    }
}

impl Checker {
    pub(super) fn trait_helper_origin(mut error: TypeError) -> TypeError {
        if let Some(diagnostic) = &mut error.structured {
            if matches!(diagnostic.origin, DiagnosticOrigin::Operator { .. }) {
                diagnostic.origin = DiagnosticOrigin::TraitCall;
            }
        }
        error
    }
}

impl Checker {
    pub(super) fn trait_dispatch_failure(
        &self,
        reason: TypeDiagnosticReason,
        trait_name: &str,
        method: &str,
        subject: Option<&Ty>,
        span: &Span,
    ) -> TypeError {
        TypeError::from_structured(StructuredDiagnostic {
            reason,
            origin: DiagnosticOrigin::TraitCall,
            data: DiagnosticData::TraitDispatch(diagnostics::TraitDispatchData {
                impl_declaration: None,
                trait_name: trait_name.into(),
                trait_arguments: vec![],
                method: Some(method.into()),
                subject_type: subject.map(|ty| self.diagnostic_ty_name(&self.resolve_ty(ty))),
            }),
            primary: match subject {
                Some(ty) => self.type_fact(SourceRole::CallTarget, span, ty),
                None => SourceFact::untyped(SourceRole::CallTarget, SourceId(0), span.clone()),
            },
            related: vec![],
            remediation: None,
        })
    }
}

impl Checker {
    pub(super) fn branch_fact(
        &self,
        role: SourceRole,
        node: &TypedNode,
        ordinal: usize,
    ) -> SourceFact {
        let mut fact = self.type_fact(role, &node.span, &node.ty);
        fact.ordinal = Some(ordinal as u32);
        fact
    }
    pub(super) fn complete_branch_error(
        &self,
        mut error: TypeError,
        bodies: &[&TypedNode],
        guards: &[Option<&TypedNode>],
    ) -> TypeError {
        let diagnostic = error
            .structured
            .as_mut()
            .expect("branch relation is structured");
        let resolved = bodies
            .iter()
            .map(|body| self.resolve_ty(&body.ty))
            .collect::<Vec<_>>();
        let names = self.diagnostic_ty_names(&resolved.iter().collect::<Vec<_>>());
        for (ordinal, body) in bodies.iter().enumerate() {
            let mut fact = self.branch_fact(SourceRole::Branch, body, ordinal);
            fact.ty = Some(names[ordinal].clone());
            if fact.span == diagnostic.primary.span && diagnostic.primary.role == SourceRole::Branch
            {
                diagnostic.primary = fact;
            } else if let Some(existing) = diagnostic
                .related
                .iter_mut()
                .find(|f| f.span == fact.span && f.role == fact.role)
            {
                *existing = fact;
            } else {
                diagnostic.related.push(fact);
            }
        }
        for (ordinal, guard) in guards.iter().enumerate() {
            if let Some(guard) = guard {
                diagnostic
                    .related
                    .push(self.branch_fact(SourceRole::Guard, guard, ordinal));
            }
        }
        error
    }
}

impl Checker {
    pub(super) fn trait_obligation_failure(
        &self,
        reason: TypeDiagnosticReason,
        trait_id: &str,
        arguments: &[Ty],
        method: Option<&str>,
        subject: &Ty,
        span: &Span,
        origin: DiagnosticOrigin,
    ) -> TypeError {
        let resolved = std::iter::once(subject)
            .chain(arguments.iter())
            .map(|ty| self.resolve_ty(ty))
            .collect::<Vec<_>>();
        let mut names = self.diagnostic_ty_names(&resolved.iter().collect::<Vec<_>>());
        let subject_type = names.remove(0);
        let arguments = names;
        let data = if reason == TypeDiagnosticReason::NoApplicableTraitImplementation {
            DiagnosticData::TraitDispatch(diagnostics::TraitDispatchData {
                impl_declaration: None,
                trait_name: trait_id.into(),
                trait_arguments: arguments,
                method: method.map(str::to_string),
                subject_type: Some(subject_type),
            })
        } else {
            DiagnosticData::TraitObligation(diagnostics::TraitObligationData {
                obligation_origin: Some(self.type_fact(SourceRole::Value, span, subject)),
                trait_name: trait_id.into(),
                trait_arguments: arguments,
                subject_type,
                position: None,
            })
        };
        TypeError::from_structured(StructuredDiagnostic {
            reason,
            origin,
            data,
            primary: self.type_fact(SourceRole::Value, span, subject),
            related: vec![],
            remediation: None,
        })
    }
}

impl Checker {
    pub(super) fn closure_arity_error(
        &self,
        expected: usize,
        actual: usize,
        span: &Span,
    ) -> TypeError {
        TypeError::from_structured(StructuredDiagnostic {
            reason: TypeDiagnosticReason::CallableShapeMismatch,
            origin: DiagnosticOrigin::Call,
            data: DiagnosticData::CallableShape(diagnostics::CallableShapeData {
                callable: "closure".into(),
                actual_type: None,
                expected_arity: Some(expected as u32),
                actual_arity: Some(actual as u32),
                return_shape: diagnostics::CallableReturnShape::Any,
            }),
            primary: SourceFact::untyped(SourceRole::CallTarget, SourceId(0), span.clone()),
            related: vec![],
            remediation: None,
        })
    }
}

impl Checker {
    fn operand_facts(
        &self,
        left: &TypedNode,
        right: &TypedNode,
        expected_side: SourceRole,
    ) -> (SourceFact, SourceFact) {
        let names =
            self.diagnostic_ty_names(&[&self.resolve_ty(&left.ty), &self.resolve_ty(&right.ty)]);
        let left = SourceFact::typed(
            SourceRole::LeftValue,
            SourceId(0),
            left.span.clone(),
            &names[0],
        );
        let right = SourceFact::typed(
            SourceRole::RightValue,
            SourceId(0),
            right.span.clone(),
            &names[1],
        );
        match expected_side {
            SourceRole::LeftValue => (left, right),
            SourceRole::RightValue => (right, left),
            _ => unreachable!("operand assertion requires a source operand"),
        }
    }
}

impl Checker {
    pub(super) fn ambiguous_constructor_result(
        &self,
        trait_name: &str,
        method: &str,
        span: &Span,
    ) -> TypeError {
        TypeError::from_structured(StructuredDiagnostic {
            reason: TypeDiagnosticReason::AmbiguousReturnTypeArgument,
            origin: DiagnosticOrigin::ReturnTypeArgument { ordinal: 0 },
            data: DiagnosticData::ReturnTypeArgument(diagnostics::ReturnTypeArgumentData {
                declared_origin: None,
                value_parameter_origin: None,
                return_origin: None,
                left_origin: None,
                right_origin: Some(SourceFact::untyped(
                    SourceRole::ReturnTypeArgument,
                    SourceId(0),
                    span.clone(),
                )),
                required_trait: None,
                expected_count: None,
                actual_count: None,
                callable: format!("{trait_name}::{method}"),
                ordinal: Some(0),
                expected_type: Some("concrete return type argument".into()),
                actual_type: Some("Self".into()),
            }),
            primary: SourceFact::untyped(SourceRole::ReturnTypeArgument, SourceId(0), span.clone()),
            related: vec![],
            remediation: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_type_assertion_rolls_back_partial_substitution() {
        let mut checker = Checker::new(TypecheckContext::default());
        let variable = checker.env.fresh_tyvar();
        let expected = Ty::Tuple(vec![variable.clone(), Ty::Int]);
        let actual = Ty::Tuple(vec![Ty::Str, Ty::Bool]);
        let span = Span { start: 0, end: 1 };
        checker
            .assert_type_relation(
                &expected,
                &actual,
                checker.type_fact(SourceRole::Expected, &span, &expected),
                checker.type_fact(SourceRole::Value, &span, &actual),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "pair",
                0,
            )
            .expect_err("second slot fails");
        assert_eq!(checker.resolve_ty(&variable), variable);
        assert_eq!(checker.candidate_probe_checkpoint_count.get(), 0);
        let accepted = Ty::Tuple(vec![Ty::Bool, Ty::Int]);
        checker
            .assert_type_relation(
                &expected,
                &accepted,
                checker.type_fact(SourceRole::Expected, &span, &expected),
                checker.type_fact(SourceRole::Value, &span, &accepted),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "pair",
                0,
            )
            .expect("a subsequent relation can select a different type");
        assert_eq!(checker.resolve_ty(&variable), Ty::Bool);
    }

    #[test]
    fn rigid_type_assertion_does_not_create_candidate_probe_checkpoint() {
        let mut checker = Checker::new(TypecheckContext::default());
        let Ty::Var(variable) = checker.env.fresh_tyvar() else {
            unreachable!("fresh type variable")
        };
        checker.rigid_tyvars.insert(variable);
        let expected = Ty::Tuple(vec![Ty::Var(variable), Ty::Int]);
        let actual = expected.clone();
        let span = Span { start: 0, end: 1 };

        checker
            .assert_type_relation(
                &expected,
                &actual,
                checker.type_fact(SourceRole::Expected, &span, &expected),
                checker.type_fact(SourceRole::Value, &span, &actual),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "pair",
                0,
            )
            .expect("identical rigid types are compatible");

        assert_eq!(checker.candidate_probe_checkpoint_count.get(), 0);
    }
}

impl Checker {
    pub(super) fn operator_operand_error(
        mut error: TypeError,
        operator: &str,
        left: &Span,
        right: &Span,
    ) -> TypeError {
        if let Some(diagnostic) = &mut error.structured {
            if diagnostic.origin == DiagnosticOrigin::Call
                && matches!(
                    diagnostic.reason,
                    TypeDiagnosticReason::NotCallable | TypeDiagnosticReason::CallableShapeMismatch
                )
            {
                let role = if &diagnostic.primary.span == left {
                    Some(SourceRole::LeftValue)
                } else if &diagnostic.primary.span == right {
                    Some(SourceRole::RightValue)
                } else {
                    None
                };
                if let Some(role) = role {
                    diagnostic.origin = DiagnosticOrigin::Operator {
                        operator: operator.into(),
                    };
                    diagnostic.primary.role = role;
                }
            }
        }
        error
    }
}
