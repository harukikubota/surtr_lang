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
                expected_origin: None,
                actual_origin: None,
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
        CallableSignatureMetadataMismatch => {
            DiagnosticData::CallableSignature(CallableSignatureData {
                callable: "make".into(),
                role: "return type argument".into(),
                expected_count: Some(1),
                actual_count: Some(2),
                detail: "declaration differs from signature".into(),
            })
        }
        ReturnTypeArgumentArityMismatch
        | ReturnTypeArgumentMismatch
        | AmbiguousReturnTypeArgument
        | DuplicateReturnTypeArgumentInput
        | MissingReturnTypeArgument
        | UnusedReturnTypeArgument
        | ConcreteReturnTypeArgumentInDefinition
        | InlineReturnTypeArgumentConstraint => {
            DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
                declared_origin: None,
                value_parameter_origin: None,
                return_origin: None,
                left_origin: None,
                right_origin: None,
                required_trait: None,
                expected_count: Some(1),
                actual_count: Some(2),
                callable: "make".into(),
                ordinal: Some(0),
                expected_type: Some("Option".into()),
                actual_type: Some("List".into()),
            })
        }
        UnresolvedEnumConstructorTypeArgument => {
            DiagnosticData::EnumConstructorTypeArgument(EnumConstructorTypeArgumentData {
                enum_name: "Either".into(),
                constructor: "Either::Left".into(),
                ordinal: 1,
                constraint_status: EnumConstructorConstraintStatus::Insufficient,
            })
        }
        InvalidTraitConstraintSubject | MissingTypeConstructorConstraint => {
            DiagnosticData::ConstraintSubject(ConstraintSubjectData {
                subject_origin: None,
                required_trait: None,
                suggested_type_variable: None,
                subject: "Functor".into(),
                constraint: "Monad".into(),
            })
        }
        MissingGenericBound | MissingTraitCapability => {
            DiagnosticData::TraitObligation(TraitObligationData {
                obligation_origin: None,
                trait_name: "Convert".into(),
                trait_arguments: vec!["String".into()],
                subject_type: "Int".into(),
                position: Some(0),
            })
        }
        NoApplicableTraitImplementation
        | UnresolvedTraitMethodInstantiation
        | MissingTraitDispatchTarget => DiagnosticData::TraitDispatch(TraitDispatchData {
            impl_declaration: None,
            trait_name: "Convert".into(),
            trait_arguments: vec!["String".into()],
            subject_type: Some("Int".into()),
            method: Some("convert".into()),
        }),
        TraitMethodTypeListMismatch | TraitMethodTypeListArityMismatch => {
            DiagnosticData::TraitMethodTypeList(TraitMethodTypeListData {
                impl_declaration: None,
                identity: None,
                method_name: "convert".into(),
                role: TypeListRole::ReturnTypeArgument,
                ordinal: 0,
                nested_path: vec![1],
                expected_type: Some("Option<Int>".into()),
                actual_type: Some("List<Int>".into()),
                expected_count: Some(1),
                actual_count: Some(2),
            })
        }
        TraitMethodConstraintMismatch => {
            DiagnosticData::TraitMethodConstraint(TraitMethodConstraintData {
                impl_declaration: None,
                identity: None,
                method_name: "convert".into(),
                expected_constraints: vec!["$A: Eq".into()],
                actual_constraints: vec!["$A: Show".into()],
            })
        }
        TypeConstructorFamilyMismatch | TypePayloadMismatch | MissingTypeConstructorCapability => {
            DiagnosticData::TypeConstructorCarrier(TypeConstructorCarrierData {
                left_type: None,
                right_type: None,
                left_origin: None,
                right_origin: None,
                required_capability: String::new(),
                family: "Monad".into(),
                family_id: "family:Functor+Monad".into(),
                expected_carrier: "Either<String, Int>".into(),
                actual_carrier: "Either<Error, Boolean>".into(),
            })
        }
        IfBranchTypeMismatch | MatchArmTypeMismatch | CondBranchTypeMismatch => {
            DiagnosticData::BranchAssertion(BranchAssertionData {
                form: crate::BranchForm::If,
                left_ordinal: None,
                right_ordinal: None,
                left_origin: None,
                right_origin: None,
                expected_type: "Int".into(),
                actual_type: "String".into(),
                branch: Some(1),
            })
        }
        SafeBindTotalPatternNonMonadRhs | SafeBindTotalPatternNonResultMonadRhs => {
            DiagnosticData::SafeBindRelation(SafeBindRelationData {
                lhs_type: "Option<Int>".into(),
                rhs_type: "Option<Int>".into(),
                lhs_is_total: true,
                rhs_is_canonical_result: false,
                monad_capability: if reason == SafeBindTotalPatternNonResultMonadRhs {
                    "satisfied"
                } else {
                    "unsatisfied"
                }
                .into(),
            })
        }
        PatternTypeMismatch | ExtractorInputTypeMismatch => {
            DiagnosticData::Pattern(PatternDiagnosticData {
                pattern_kind: PatternKind::Variable,
                name: Some("value".into()),
                expected_type: Some("Int".into()),
                actual_type: Some("String".into()),
                expected_count: None,
                actual_count: None,
                details: vec![],
            })
        }
        PatternShapeMismatch | PatternArityMismatch | ExtractorArityMismatch => {
            DiagnosticData::Pattern(PatternDiagnosticData {
                pattern_kind: if matches!(reason, ExtractorArityMismatch) {
                    PatternKind::Extractor
                } else {
                    PatternKind::Tuple
                },
                name: Some("Pair".into()),
                expected_type: Some("Tuple".into()),
                actual_type: Some("List".into()),
                expected_count: Some(2),
                actual_count: Some(1),
                details: vec![],
            })
        }
        NonExhaustiveMatch => DiagnosticData::Pattern(PatternDiagnosticData {
            pattern_kind: PatternKind::Match,
            name: None,
            expected_type: None,
            actual_type: None,
            expected_count: None,
            actual_count: None,
            details: vec!["None".into()],
        }),
        NonTotalBindingPattern | NestedResultErrorPattern => {
            DiagnosticData::Pattern(PatternDiagnosticData {
                pattern_kind: if reason == NonTotalBindingPattern {
                    PatternKind::Match
                } else {
                    PatternKind::Constructor
                },
                name: None,
                expected_type: None,
                actual_type: None,
                expected_count: None,
                actual_count: None,
                details: vec![reason.as_str().into()],
            })
        }
        MatchGuardTypeMismatch => DiagnosticData::Pattern(PatternDiagnosticData {
            pattern_kind: PatternKind::Match,
            name: Some("guard".into()),
            expected_type: Some("Boolean".into()),
            actual_type: Some("Int".into()),
            expected_count: None,
            actual_count: None,
            details: vec![],
        }),
        ConstructorPatternRequiresEnumOrResultRhs => {
            DiagnosticData::Pattern(PatternDiagnosticData {
                pattern_kind: PatternKind::Constructor,
                name: Some("Some".into()),
                expected_type: Some("enum or Result".into()),
                actual_type: Some("String".into()),
                expected_count: None,
                actual_count: None,
                details: vec![],
            })
        }
        SafeBindErrorTypeMismatch => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::SafeBindFailureTarget,
            subject: Some("failure target".into()),
            expected_type: Some("Error".into()),
            actual_type: Some("String".into()),
            stage: None,
            entrypoint: None,
        }),
        SafeBindRequiresResultTarget => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::SafeBindRequiresResultTarget,
            subject: Some("safe bind target".into()),
            expected_type: Some("Result<T>".into()),
            actual_type: Some("Option<T>".into()),
            stage: None,
            entrypoint: None,
        }),
        ErrorValueMustBeWrapped => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::ErrorValuePlacement,
            subject: Some("error value".into()),
            expected_type: Some("Result::Err(error)".into()),
            actual_type: Some("Error".into()),
            stage: None,
            entrypoint: None,
        }),
        FacetSafeBindForbidden => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::SafeBindFacet,
            subject: Some("facet path".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
        FacetPatternBindingForbidden => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::FacetPatternBinding,
            subject: Some("facet pattern".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
        FacetOperationPolicyViolation => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::FacetOperation,
            subject: Some("facet operation".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
        FacetCompileTimeOnly => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::FacetStageRestriction,
            subject: Some("Facet".into()),
            expected_type: None,
            actual_type: None,
            stage: Some("runtime".into()),
            entrypoint: None,
        }),
        ProcessHandlerScope => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::ProcessHandlerScope,
            subject: Some("process handler".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
        SourcePolicyViolation => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::SourceExitCode,
            subject: Some("source".into()),
            expected_type: None,
            actual_type: None,
            stage: Some("script".into()),
            entrypoint: None,
        }),
        CompilePolicyViolation => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::EntrypointRequirement,
            subject: Some("compile unit".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: Some("main".into()),
        }),
        NominalDeclarationConstraintViolation => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::NominalDeclarationConstraint,
            subject: Some("$M".into()),
            expected_type: Some("Monad".into()),
            actual_type: Some("Plain".into()),
            stage: Some("OptionT".into()),
            entrypoint: None,
        }),
        InvalidResultEffectAnnotation => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::ResultEffectAnnotation,
            subject: Some("@result_effect requires one public base field".into()),
            expected_type: Some("MonadT base field".into()),
            actual_type: Some("invalid declaration".into()),
            stage: Some("Wrapper".into()),
            entrypoint: None,
        }),
        TraitHelperCaptureNeedsExpectedType => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::TraitHelperCaptureInference,
            subject: Some("concat".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
        ReservedIntrinsicMarkerUsage => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::IntrinsicMarkerUsage,
            subject: Some("DoBlock".into()),
            expected_type: None,
            actual_type: Some("DoBlock".into()),
            stage: None,
            entrypoint: Some("do".into()),
        }),
        TypecheckInvariantViolation => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::ProducerContract,
            subject: Some("typechecker".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
        ProcessPolicyViolation => DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::ProcessCapabilityPolicy,
            subject: Some("process declaration".into()),
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
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
        UnresolvedEnumConstructorTypeArgument => DiagnosticOrigin::EnumConstructor { ordinal: 1 },
        _ => DiagnosticOrigin::Call,
    };
    StructuredDiagnostic {
        reason: reason.into(),
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
        UnresolvedEnumConstructorTypeArgument,
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
        SafeBindTotalPatternNonMonadRhs,
        SafeBindTotalPatternNonResultMonadRhs,
        PatternTypeMismatch,
        PatternShapeMismatch,
        PatternArityMismatch,
        NonTotalBindingPattern,
        NestedResultErrorPattern,
        MatchGuardTypeMismatch,
        ConstructorPatternRequiresEnumOrResultRhs,
        ExtractorInputTypeMismatch,
        ExtractorArityMismatch,
        NonExhaustiveMatch,
        SafeBindErrorTypeMismatch,
        SafeBindRequiresResultTarget,
        ErrorValueMustBeWrapped,
        FacetSafeBindForbidden,
        FacetPatternBindingForbidden,
        FacetOperationPolicyViolation,
        FacetCompileTimeOnly,
        ProcessHandlerScope,
        SourcePolicyViolation,
        CompilePolicyViolation,
        ReservedIntrinsicMarkerUsage,
        TypecheckInvariantViolation,
        ProcessPolicyViolation,
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
            "EnumConstructorTypeArgument" => {
                &["enum_name", "constructor", "ordinal", "constraint_status"]
            }
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
            "SafeBindRelation" => &[
                "lhs_type",
                "rhs_type",
                "lhs_is_total",
                "rhs_is_canonical_result",
                "monad_capability",
            ],
            "Pattern" => &[
                "pattern_kind",
                "name",
                "expected_type",
                "actual_type",
                "expected_count",
                "actual_count",
                "details",
            ],
            "Policy" => &[
                "policy",
                "subject",
                "expected_type",
                "actual_type",
                "stage",
                "entrypoint",
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
fn reserved_intrinsic_marker_usage_has_specific_label_note_and_help() {
    let mut diagnostic = input(TypeDiagnosticReason::ReservedIntrinsicMarkerUsage);
    diagnostic.primary = SourceFact::untyped(
        SourceRole::Annotation,
        SourceId(0),
        Span { start: 4, end: 11 },
    );
    diagnostic.remediation = Some(Remediation::Help {
        text: "Remove `DoBlock`; write a `do { ... }` expression to sequence Monad values.".into(),
    });

    let spec = structured_type_error_spec(&diagnostic);
    assert_eq!(
        spec.message,
        "`DoBlock` is reserved for the compiler-owned `do` signature"
    );
    assert_eq!(
        spec.labels[0].message,
        "`DoBlock` cannot be used in this type position"
    );
    assert_eq!(spec.notes, ["`DoBlock` is not an ordinary value type"]);
    assert_eq!(
        spec.help.as_deref(),
        Some("Remove `DoBlock`; write a `do { ... }` expression to sequence Monad values.")
    );
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
