//! JSON projections borrow the same typed facts as the human renderer.
//! No prose or source scanning is involved. Source references are materialized
//! here, after phase adapters have rebased their source IDs and spans.
use crate::data::*;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct Origin<'a> {
    kind: SourceRole,
    ordinal: Option<u32>,
    source_id: u32,
    span: [u32; 2],
    declaration_identity: Option<&'a DeclarationIdentity>,
}
impl<'a> From<&'a SourceFact> for Origin<'a> {
    fn from(fact: &'a SourceFact) -> Self {
        Self {
            kind: fact.role,
            ordinal: fact.ordinal,
            source_id: fact.source_id.0,
            span: [fact.span.start as u32, fact.span.end as u32],
            declaration_identity: fact.declaration_identity.as_ref(),
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum Projection<'a> {
    ArgumentRelation {
        #[serde(flatten)]
        relation: &'a ArgumentRelationData,
        expected_origin: Option<Origin<'a>>,
        actual_origin: Origin<'a>,
    },
    ReturnTypeArgument {
        callable: &'a str,
        ordinal: Option<u32>,
        return_type_argument_ordinal: Option<u32>,
        expected_type: Option<&'a str>,
        actual_type: Option<&'a str>,
        declared_origin: Option<Origin<'a>>,
        value_parameter_origin: Option<Origin<'a>>,
        return_origin: Option<Origin<'a>>,
        left_type: Option<&'a str>,
        right_type: Option<&'a str>,
        left_origin: Option<Origin<'a>>,
        right_origin: Origin<'a>,
        required_trait: Option<&'a str>,
        expected_count: Option<u32>,
        actual_count: Option<u32>,
    },
    ConstraintSubject {
        #[serde(flatten)]
        subject: &'a ConstraintSubjectData,
        subject_type: &'a str,
        subject_origin: Origin<'a>,
        required_trait: Option<&'a str>,
        suggested_type_variable: Option<&'a str>,
    },
    TraitObligation {
        #[serde(flatten)]
        obligation: &'a TraitObligationData,
        trait_id: &'a str,
        obligation_origin: Origin<'a>,
    },
    TraitDispatch {
        trait_id: Option<&'a str>,
        trait_arguments: &'a [String],
        subject_type: Option<&'a str>,
        method_name: Option<&'a str>,
        type_list_role: Option<TypeListRole>,
        ordinal: Option<u32>,
        expected_type: Option<&'a str>,
        actual_type: Option<&'a str>,
        impl_declaration: Option<Origin<'a>>,
        nested_path: &'a [u32],
        expected_count: Option<u32>,
        actual_count: Option<u32>,
        expected_constraints: &'a [String],
        actual_constraints: &'a [String],
        failures: &'a [CandidateFailureData],
    },
    TypeConstructorCarrier {
        #[serde(flatten)]
        carrier: &'a TypeConstructorCarrierData,
        left_type: Option<&'a str>,
        right_type: Option<&'a str>,
        left_origin: Option<Origin<'a>>,
        right_origin: Origin<'a>,
        required_capability: &'a str,
    },
    BranchAssertion {
        #[serde(flatten)]
        branch: &'a BranchAssertionData,
        form: BranchForm,
        left_ordinal: Option<u32>,
        right_ordinal: Option<u32>,
        left_type: &'a str,
        right_type: &'a str,
        left_origin: Option<Origin<'a>>,
        right_origin: Origin<'a>,
    },
}
impl StructuredDiagnostic {
    pub fn data_json(&self) -> Value {
        let first = self.related.first();
        let fact = |role| {
            std::iter::once(&self.primary)
                .chain(self.related.iter())
                .find(|f| f.role == role)
        };
        let rta = |callable, ordinal, expected_type, actual_type, expected_count, actual_count| {
            Projection::ReturnTypeArgument {
                callable,
                ordinal,
                return_type_argument_ordinal: ordinal,
                expected_type,
                actual_type,
                declared_origin: fact(SourceRole::Declaration).map(Origin::from),
                value_parameter_origin: fact(SourceRole::Value).map(Origin::from),
                return_origin: fact(SourceRole::Expected).map(Origin::from),
                left_type: expected_type,
                right_type: actual_type,
                left_origin: first.map(Origin::from),
                right_origin: (&self.primary).into(),
                required_trait: None,
                expected_count,
                actual_count,
            }
        };
        let projection = match &self.data {
            DiagnosticData::ArgumentRelation(relation) => Projection::ArgumentRelation {
                relation,
                expected_origin: first.map(Origin::from),
                actual_origin: (&self.primary).into(),
            },
            DiagnosticData::ReturnTypeArgument(v) => rta(
                &v.callable,
                Some(v.ordinal),
                Some(v.expected_type.as_str()),
                Some(v.actual_type.as_str()),
                None,
                None,
            ),
            DiagnosticData::CallableSignature(v)
                if self.reason == TypeDiagnosticReason::ReturnTypeArgumentArityMismatch =>
            {
                rta(
                    &v.callable,
                    None,
                    None,
                    None,
                    v.expected_count,
                    v.actual_count,
                )
            }
            DiagnosticData::ConstraintSubject(v) => Projection::ConstraintSubject {
                subject: v,
                subject_type: &v.subject,
                subject_origin: (&self.primary).into(),
                required_trait: if self.reason
                    == TypeDiagnosticReason::InvalidTraitConstraintSubject
                {
                    None
                } else {
                    Some(&v.constraint)
                },
                suggested_type_variable: Some("$F"),
            },
            DiagnosticData::TraitObligation(v)
                if self.reason != TypeDiagnosticReason::NoApplicableTraitImplementation =>
            {
                Projection::TraitObligation {
                    obligation: v,
                    trait_id: &v.trait_name,
                    obligation_origin: (&self.primary).into(),
                }
            }
            DiagnosticData::TraitDispatch(_)
            | DiagnosticData::TraitObligation(_)
            | DiagnosticData::TraitMethodTypeList(_)
            | DiagnosticData::TraitMethodConstraint(_)
            | DiagnosticData::CandidateSelection(_) => {
                let (trait_id, trait_arguments, subject_type, method_name) = match &self.data {
                    DiagnosticData::TraitDispatch(v) => (
                        Some(v.trait_name.as_str()),
                        v.trait_arguments.as_slice(),
                        v.subject_type.as_deref(),
                        v.method.as_deref(),
                    ),
                    DiagnosticData::TraitObligation(v) => (
                        Some(v.trait_name.as_str()),
                        v.trait_arguments.as_slice(),
                        Some(v.subject_type.as_str()),
                        None,
                    ),
                    DiagnosticData::CandidateSelection(v) => (
                        Some(v.trait_name.as_str()),
                        &[][..],
                        self.primary.ty.as_deref(),
                        Some(v.method.as_str()),
                    ),
                    DiagnosticData::TraitMethodTypeList(v) => (
                        v.identity.as_ref().map(|id| id.trait_id.as_str()),
                        v.identity
                            .as_ref()
                            .map_or(&[][..], |id| id.trait_arguments.as_slice()),
                        v.identity.as_ref().map(|id| id.subject_type.as_str()),
                        Some(v.method_name.as_str()),
                    ),
                    DiagnosticData::TraitMethodConstraint(v) => (
                        v.identity.as_ref().map(|id| id.trait_id.as_str()),
                        v.identity
                            .as_ref()
                            .map_or(&[][..], |id| id.trait_arguments.as_slice()),
                        v.identity.as_ref().map(|id| id.subject_type.as_str()),
                        Some(v.method_name.as_str()),
                    ),
                    _ => unreachable!(),
                };
                let list = match &self.data {
                    DiagnosticData::TraitMethodTypeList(v) => Some(v),
                    _ => None,
                };
                let constraints = match &self.data {
                    DiagnosticData::TraitMethodConstraint(v) => Some(v),
                    _ => None,
                };
                Projection::TraitDispatch {
                    trait_id,
                    trait_arguments,
                    subject_type,
                    method_name,
                    type_list_role: list.map(|v| v.role),
                    ordinal: list.map(|v| v.ordinal),
                    expected_type: list.map(|v| v.expected_type.as_str()),
                    actual_type: list.map(|v| v.actual_type.as_str()),
                    impl_declaration: fact(SourceRole::Impl).map(Origin::from),
                    nested_path: list.map_or(&[], |v| v.nested_path.as_slice()),
                    expected_count: list.and_then(|v| v.expected_count),
                    actual_count: list.and_then(|v| v.actual_count),
                    expected_constraints: constraints
                        .map_or(&[], |v| v.expected_constraints.as_slice()),
                    actual_constraints: constraints
                        .map_or(&[], |v| v.actual_constraints.as_slice()),
                    failures: match &self.data {
                        DiagnosticData::CandidateSelection(v) => &v.failures,
                        _ => &[],
                    },
                }
            }
            DiagnosticData::TypeConstructorCarrier(v) => Projection::TypeConstructorCarrier {
                carrier: v,
                left_type: fact(SourceRole::LeftValue)
                    .or(first)
                    .and_then(|f| f.ty.as_deref()),
                right_type: fact(SourceRole::RightValue)
                    .unwrap_or(&self.primary)
                    .ty
                    .as_deref(),
                left_origin: fact(SourceRole::LeftValue).or(first).map(Origin::from),
                right_origin: fact(SourceRole::RightValue).unwrap_or(&self.primary).into(),
                required_capability: &v.family,
            },
            DiagnosticData::BranchAssertion(v) => Projection::BranchAssertion {
                branch: v,
                form: match self.origin {
                    DiagnosticOrigin::Branch { form, .. } => form,
                    _ => unreachable!("branch diagnostic requires branch origin"),
                },
                left_ordinal: first.and_then(|f| f.ordinal),
                right_ordinal: v.branch,
                left_type: &v.expected_type,
                right_type: &v.actual_type,
                left_origin: first.map(Origin::from),
                right_origin: (&self.primary).into(),
            },
            _ => return self.data.to_json_value(),
        };
        serde_json::to_value(projection).expect("typed diagnostic projection is serializable")
    }
}
