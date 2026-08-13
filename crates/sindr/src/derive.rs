use crate::names::{surface_path_name, TypeIdentity};

/// Canonical identity of a trait referenced by derive metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraitIdentity(String);

impl TraitIdentity {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Type declarations to which a derive recipe may be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveApplicability {
    StructRecordEnum,
}

impl DeriveApplicability {
    pub const fn supports(self, identity: TypeIdentity) -> bool {
        matches!(
            (self, identity),
            (
                Self::StructRecordEnum,
                TypeIdentity::Struct | TypeIdentity::Record | TypeIdentity::Enum
            )
        )
    }
}

/// Trait capability required from each field or enum payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldTraitRequirement {
    None,
    RequiresTrait(TraitIdentity),
}

/// Compiler-supported structural derive recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveGenerator {
    StructuralEq,
    LexicographicCompare,
    InspectShow,
}

/// Complete metadata for one deriveable trait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeriveTraitMeta {
    pub trait_name: TraitIdentity,
    pub applicability: DeriveApplicability,
    pub field_requirement: FieldTraitRequirement,
    pub generator: DeriveGenerator,
}

struct DeriveTraitSpec {
    name: &'static str,
    field_requirement: Option<&'static str>,
    generator: DeriveGenerator,
}

const DERIVE_TRAIT_SPECS: &[DeriveTraitSpec] = &[
    DeriveTraitSpec {
        name: "Eq",
        field_requirement: Some("Eq"),
        generator: DeriveGenerator::StructuralEq,
    },
    DeriveTraitSpec {
        name: "Compare",
        field_requirement: Some("Compare"),
        generator: DeriveGenerator::LexicographicCompare,
    },
    DeriveTraitSpec {
        name: "Show",
        field_requirement: None,
        generator: DeriveGenerator::InspectShow,
    },
];

/// Resolve a surface or canonical trait name to its derive recipe.
///
/// The returned metadata is owned so the registry can later be extended with
/// dynamically registered user recipes without exposing the static table.
pub fn derive_trait_meta(name: &str) -> Option<DeriveTraitMeta> {
    let surface_name = surface_path_name(name);
    DERIVE_TRAIT_SPECS
        .iter()
        .find(|spec| surface_path_name(spec.name) == surface_name)
        .map(|spec| DeriveTraitMeta {
            trait_name: TraitIdentity::new(spec.name),
            applicability: DeriveApplicability::StructRecordEnum,
            field_requirement: spec
                .field_requirement
                .map(TraitIdentity::new)
                .map(FieldTraitRequirement::RequiresTrait)
                .unwrap_or(FieldTraitRequirement::None),
            generator: spec.generator,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_derive_registry_contains_the_v1_recipes() {
        let eq = derive_trait_meta("Global::Eq").expect("Eq recipe");
        assert_eq!(eq.trait_name.as_str(), "Eq");
        assert_eq!(
            eq.field_requirement,
            FieldTraitRequirement::RequiresTrait(TraitIdentity::new("Eq"))
        );
        assert_eq!(eq.generator, DeriveGenerator::StructuralEq);

        let show = derive_trait_meta("Show").expect("Show recipe");
        assert_eq!(show.field_requirement, FieldTraitRequirement::None);
        assert_eq!(show.generator, DeriveGenerator::InspectShow);
    }

    #[test]
    fn unknown_traits_are_not_deriveable() {
        assert!(derive_trait_meta("Serialize").is_none());
        assert!(derive_trait_meta("Eq<Int>").is_none());
    }
}
