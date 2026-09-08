use crate::*;
use spire::ast::Span;

fn input(reason: TypeDiagnosticReason) -> StructuredDiagnostic {
    use TypeDiagnosticReason::*;
    let data = match reason {
        ArityMismatch | ArgumentModeMismatch | UnknownNamedArgument | DuplicateArgument
        | MissingArgument => DiagnosticData::ArgumentContract(ArgumentContractData {
            callable: "take".into(),
            name: Some("value".into()),
            expected_count: 1,
            actual_count: 2,
        }),
        ArgumentTypeMismatch | ReturnTypeMismatch | AnnotationTypeMismatch => {
            DiagnosticData::ArgumentRelation(ArgumentRelationData {
                callable: "take".into(),
                ordinal: 0,
                expected_type: Some("Either<String, Int>".into()),
                actual_type: Some("Either<Error, Boolean>".into()),
            })
        }
        NotCallable | CallableShapeMismatch => DiagnosticData::CallableShape(CallableShapeData {
            callable: "mapper".into(),
            actual_type: Some("Int".into()),
            expected_arity: Some(1),
            actual_arity: None,
            return_shape: CallableReturnShape::Any,
        }),
        CallableSignatureMetadataMismatch | ReturnTypeArgumentArityMismatch => {
            DiagnosticData::CallableSignature(CallableSignatureData {
                callable: "make".into(),
                role: "return type argument".into(),
                expected_count: Some(1),
                actual_count: Some(2),
                detail: "declaration differs from signature".into(),
            })
        }
        ReturnTypeArgumentMismatch
        | AmbiguousReturnTypeArgument
        | DuplicateReturnTypeArgumentInput
        | MissingReturnTypeArgument
        | UnusedReturnTypeArgument
        | ConcreteReturnTypeArgumentInDefinition
        | InlineReturnTypeArgumentConstraint => {
            DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
                callable: "make".into(),
                ordinal: 0,
                expected_type: "Option".into(),
                actual_type: "List".into(),
            })
        }
        InvalidTraitConstraintSubject | MissingTypeConstructorConstraint => {
            DiagnosticData::ConstraintSubject(ConstraintSubjectData {
                subject: "Functor".into(),
                constraint: "Monad".into(),
            })
        }
        MissingGenericBound | MissingTraitCapability => {
            DiagnosticData::TraitObligation(TraitObligationData {
                trait_name: "Convert".into(),
                trait_arguments: vec!["String".into()],
                subject_type: "Int".into(),
                position: Some(0),
            })
        }
        NoApplicableTraitImplementation
        | UnresolvedTraitMethodInstantiation
        | MissingTraitDispatchTarget => DiagnosticData::TraitDispatch(TraitDispatchData {
            trait_name: "Convert".into(),
            trait_arguments: vec!["String".into()],
            subject_type: Some("Int".into()),
            method: Some("convert".into()),
        }),
        TraitMethodTypeListMismatch | TraitMethodTypeListArityMismatch => {
            DiagnosticData::TraitMethodTypeList(TraitMethodTypeListData {
                identity: None,
                method_name: "convert".into(),
                role: TypeListRole::ReturnTypeArgument,
                ordinal: 0,
                nested_path: vec![1],
                expected_type: "Option<Int>".into(),
                actual_type: "List<Int>".into(),
                expected_count: Some(1),
                actual_count: Some(2),
            })
        }
        TraitMethodConstraintMismatch => {
            DiagnosticData::TraitMethodConstraint(TraitMethodConstraintData {
                identity: None,
                method_name: "convert".into(),
                expected_constraints: vec!["$A: Eq".into()],
                actual_constraints: vec!["$A: Show".into()],
            })
        }
        TypeConstructorFamilyMismatch | TypePayloadMismatch | MissingTypeConstructorCapability => {
            DiagnosticData::TypeConstructorCarrier(TypeConstructorCarrierData {
                family: "Monad".into(),
                family_id: "family:Functor+Monad".into(),
                expected_carrier: "Either<String, Int>".into(),
                actual_carrier: "Either<Error, Boolean>".into(),
            })
        }
        IfBranchTypeMismatch | MatchArmTypeMismatch | CondBranchTypeMismatch => {
            DiagnosticData::BranchAssertion(BranchAssertionData {
                expected_type: "Int".into(),
                actual_type: "String".into(),
                branch: Some(1),
            })
        }
    };
    let origin = match reason {
        IfBranchTypeMismatch => DiagnosticOrigin::Branch {
            form: BranchForm::If,
            ordinal: 1,
        },
        MatchArmTypeMismatch => DiagnosticOrigin::Branch {
            form: BranchForm::Match,
            ordinal: 1,
        },
        CondBranchTypeMismatch => DiagnosticOrigin::Branch {
            form: BranchForm::Cond,
            ordinal: 1,
        },
        _ => DiagnosticOrigin::Call,
    };
    StructuredDiagnostic {
        reason,
        origin,
        data,
        primary: SourceFact::typed(
            SourceRole::RightValue,
            SourceId(0),
            Span { start: 4, end: 8 },
            "Either<Error, Boolean>",
        ),
        related: vec![SourceFact::typed(
            SourceRole::LeftValue,
            SourceId(0),
            Span { start: 0, end: 4 },
            "Either<String, Int>",
        )],
        remediation: None,
    }
}

#[test]
fn every_common_reason_has_a_typed_template_and_schema() {
    use TypeDiagnosticReason::*;
    let reasons = [
        ArityMismatch,
        ArgumentModeMismatch,
        UnknownNamedArgument,
        DuplicateArgument,
        MissingArgument,
        ArgumentTypeMismatch,
        ReturnTypeMismatch,
        AnnotationTypeMismatch,
        NotCallable,
        CallableShapeMismatch,
        CallableSignatureMetadataMismatch,
        ReturnTypeArgumentArityMismatch,
        ReturnTypeArgumentMismatch,
        AmbiguousReturnTypeArgument,
        InvalidTraitConstraintSubject,
        MissingGenericBound,
        MissingTraitCapability,
        NoApplicableTraitImplementation,
        UnresolvedTraitMethodInstantiation,
        MissingTraitDispatchTarget,
        MissingTypeConstructorConstraint,
        TraitMethodTypeListMismatch,
        TraitMethodTypeListArityMismatch,
        TraitMethodConstraintMismatch,
        TypeConstructorFamilyMismatch,
        TypePayloadMismatch,
        MissingTypeConstructorCapability,
        DuplicateReturnTypeArgumentInput,
        MissingReturnTypeArgument,
        UnusedReturnTypeArgument,
        ConcreteReturnTypeArgumentInDefinition,
        InlineReturnTypeArgumentConstraint,
        IfBranchTypeMismatch,
        MatchArmTypeMismatch,
        CondBranchTypeMismatch,
    ];
    let mut sources = SourceRegistry::new();
    let source_id = sources.register("main.srt", "abcdefgh");
    for reason in reasons {
        let input = input(reason);
        let mut spec = structured_type_error_spec(&input);
        assert_ne!(spec.message, "Type checking failed", "{reason:?}");
        assert_ne!(
            spec.message,
            reason.as_str(),
            "a reason needs a human template: {reason:?}"
        );
        let before = serializable_report_by_id(&sources, source_id, "typecheck", &spec)
            .errors
            .remove(0);
        let data = &before.data;
        let keys: &[&str] = match data["kind"].as_str().unwrap() {
            "ArgumentRelation" => &[
                "ordinal",
                "expected_type",
                "actual_type",
                "expected_origin",
                "actual_origin",
            ],
            "ReturnTypeArgument" => &[
                "return_type_argument_ordinal",
                "declared_origin",
                "value_parameter_origin",
                "return_origin",
                "left_type",
                "right_type",
                "left_origin",
                "right_origin",
                "required_trait",
            ],
            "ConstraintSubject" => &[
                "subject_type",
                "subject_origin",
                "required_trait",
                "suggested_type_variable",
            ],
            "TraitObligation" => &[
                "trait_id",
                "trait_arguments",
                "subject_type",
                "obligation_origin",
            ],
            "TraitDispatch" => &[
                "trait_id",
                "trait_arguments",
                "subject_type",
                "method_name",
                "type_list_role",
                "ordinal",
                "expected_type",
                "actual_type",
                "impl_declaration",
            ],
            "TypeConstructorCarrier" => &[
                "family_id",
                "left_type",
                "right_type",
                "left_origin",
                "right_origin",
                "required_capability",
            ],
            "BranchAssertion" => &[
                "form",
                "left_ordinal",
                "right_ordinal",
                "left_type",
                "right_type",
                "left_origin",
                "right_origin",
            ],
            "CallableShape" | "ArgumentContract" | "CallableSignature" => &[],
            other => panic!("unexpected schema: {other}"),
        };
        for key in keys {
            assert!(
                data.get(*key).is_some(),
                "{reason:?} requires {key}: {data}"
            );
        }
        spec.message = "expected Poison, got Trap".into();
        spec.labels
            .iter_mut()
            .for_each(|label| label.message = "arbitrary prose".into());
        let after = serializable_report_by_id(&sources, source_id, "typecheck", &spec)
            .errors
            .remove(0);
        assert_eq!(before.reason, after.reason);
        assert_eq!(before.data, after.data);
        assert_eq!(before.expected, after.expected);
        assert_eq!(before.got, after.got);
        assert_eq!(before.hint, after.hint);
        assert_eq!(before.related, after.related);
    }
}

#[test]
fn source_roles_use_the_documented_json_vocabulary() {
    assert_eq!(
        serde_json::to_value(SourceRole::LeftValue).unwrap(),
        "left_value"
    );
    assert_eq!(SourceRole::LeftValue.json_name(), "left_value");
    assert_eq!(
        serde_json::to_value(SourceRole::RightValue).unwrap(),
        "right_value"
    );
}
