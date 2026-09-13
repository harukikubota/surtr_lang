use sindr::builtin::builtin_type_meta_by_name;
use sindr::intrinsic::{
    do_intrinsic_contract, CanonicalTraitIdentity, CanonicalTraitMethodIdentity,
    DoCapabilityPredicate, IntrinsicCarrierSource, IntrinsicId, IntrinsicOwner, IntrinsicType,
    ReturnTypeArgumentRole, SafeBindFailureAction, SafeBindInputAction, SafeBindInputMatcher,
    SafeBindRejection,
};
use sindr::names::{builtin_type_name, builtin_type_usage_policy, BuiltinTypeUsage, TypeName};

#[test]
fn do_block_is_a_canonical_intrinsic_signature_only_builtin_type() {
    assert_eq!(builtin_type_name("DoBlock"), Some(TypeName::DoBlock));

    let metadata = builtin_type_meta_by_name("DoBlock").expect("DoBlock builtin metadata");
    assert_eq!(metadata.name, "DoBlock");
    assert_eq!(metadata.params, &["$Result"]);

    let policy = builtin_type_usage_policy("DoBlock").expect("DoBlock usage policy");
    assert_eq!(
        policy.usage,
        BuiltinTypeUsage::IntrinsicSignatureOnly(IntrinsicId::Do)
    );
    assert!(!policy.type_annotation_allowed);
    assert!(!policy.runtime_value_allowed);
}

#[test]
fn do_contract_closes_signature_carrier_capabilities_and_lowering() {
    let contract = do_intrinsic_contract();
    assert_eq!(contract.identity, IntrinsicId::Do);
    assert_eq!(contract.identity.surface_name(), "do");
    assert_eq!(contract.owner, IntrinsicOwner::Bootstrap);
    assert_eq!(contract.owner.surface_name(), "Bootstrap");
    assert_eq!(contract.return_type_arguments.len(), 1);

    let rta = contract.return_type_arguments[0];
    assert_eq!(rta.position, 0);
    assert_eq!(
        rta.role,
        ReturnTypeArgumentRole::DirectTypeCtorTrait(CanonicalTraitIdentity::Monad)
    );
    assert_eq!(
        contract.do_local_carrier,
        IntrinsicCarrierSource::ReturnTypeArgument(0)
    );
    assert_eq!(
        contract.value_parameters,
        &[IntrinsicType::BuiltinApplication {
            head: TypeName::DoBlock,
            argument: sindr::intrinsic::IntrinsicTypeParameter::Result,
        }]
    );
    assert_eq!(
        contract.return_type,
        IntrinsicType::TraitApplication {
            trait_identity: CanonicalTraitIdentity::Monad,
            argument: sindr::intrinsic::IntrinsicTypeParameter::Result,
        }
    );

    assert_eq!(contract.capability_rules.len(), 3);
    assert_eq!(
        contract.capability_rules[0].predicate,
        DoCapabilityPredicate::Always
    );
    assert_eq!(
        contract.capability_rules[0].capability,
        CanonicalTraitIdentity::Monad
    );
    assert_eq!(
        contract.capability_rules[1].predicate,
        DoCapabilityPredicate::HasPartialExtractPattern
    );
    assert_eq!(
        contract.capability_rules[2].predicate,
        DoCapabilityPredicate::HasLegalSafeBindAndCarrierIsNot(TypeName::Result)
    );
    for rule in contract.capability_rules {
        assert_eq!(
            rule.same_carrier,
            IntrinsicCarrierSource::ReturnTypeArgument(0)
        );
    }

    assert_eq!(
        contract.lowering.sequence,
        CanonicalTraitMethodIdentity::MonadBind
    );
    assert_eq!(
        contract.lowering.partial_failure,
        CanonicalTraitMethodIdentity::AlternativeEmpty
    );
    assert_eq!(
        contract.lowering.safe_bind_failure.canonical_result,
        TypeName::Result
    );
    assert_eq!(
        contract.lowering.safe_bind_failure.canonical_result_action,
        SafeBindFailureAction::PreserveExistingSafeBindFailure
    );
    assert_eq!(
        contract.lowering.safe_bind_failure.otherwise_action,
        SafeBindFailureAction::OverrideWith(CanonicalTraitMethodIdentity::AlternativeEmpty)
    );
}

#[test]
fn do_contract_closes_safe_bind_input_classification() {
    let contract = do_intrinsic_contract();
    assert_eq!(contract.safe_bind_input.len(), 3);
    assert_eq!(
        contract.safe_bind_input[0].matcher,
        SafeBindInputMatcher::CanonicalType(TypeName::Result)
    );
    assert_eq!(
        contract.safe_bind_input[0].action,
        SafeBindInputAction::UnwrapOneLayer
    );
    assert_eq!(
        contract.safe_bind_input[1].matcher,
        SafeBindInputMatcher::OtherwisePartialPattern
    );
    assert_eq!(
        contract.safe_bind_input[1].action,
        SafeBindInputAction::PassThroughToPattern
    );
    assert_eq!(
        contract.safe_bind_input[2].matcher,
        SafeBindInputMatcher::OtherwiseTotalPattern
    );
    assert_eq!(
        contract.safe_bind_input[2].action,
        SafeBindInputAction::RejectAfterPatternTypeCheck(&[
            SafeBindRejection::NonMonad,
            SafeBindRejection::NonResultMonad,
        ])
    );
}
