use serde::{Deserialize, Serialize};

/// Surface-level type identity defined by the language spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeIdentity {
    Type,
    Struct,
    Record,
    Enum,
    ConcreteError,
    Mod,
    Const,
}

/// Canonical builtin type heads reserved by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeName {
    Int,
    Float,
    String,
    Boolean,
    Unit,
    Closure,
    Error,
    Regex,
    RegexCaptures,
    RegexMatch,
    RandomGenerator,
    List,
    HashMap,
    Generator,
    Result,
    TypeRef,
    Hole,
    Lens,
}

impl TypeName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Float => "Float",
            Self::String => "String",
            Self::Boolean => "Boolean",
            Self::Unit => "Unit",
            Self::Closure => "Closure",
            Self::Error => "Error",
            Self::Regex => "Regex",
            Self::RegexCaptures => "RegexCaptures",
            Self::RegexMatch => "RegexMatch",
            Self::RandomGenerator => "RandomGenerator",
            Self::List => "List",
            Self::HashMap => "HashMap",
            Self::Generator => "Generator",
            Self::Result => "Result",
            Self::TypeRef => "TypeRef",
            Self::Hole => "Hole",
            Self::Lens => "Lens",
        }
    }

    pub const fn identity(self) -> TypeIdentity {
        TypeIdentity::Type
    }

    pub const fn supports_inherent_impl(self) -> bool {
        !matches!(self, Self::TypeRef | Self::Hole | Self::Closure)
    }
}

pub fn builtin_type_name(name: &str) -> Option<TypeName> {
    match name {
        "Int" => Some(TypeName::Int),
        "Float" => Some(TypeName::Float),
        "String" => Some(TypeName::String),
        "Boolean" => Some(TypeName::Boolean),
        "Unit" => Some(TypeName::Unit),
        "Closure" => Some(TypeName::Closure),
        "Error" => Some(TypeName::Error),
        "Regex" => Some(TypeName::Regex),
        "RegexCaptures" => Some(TypeName::RegexCaptures),
        "RegexMatch" => Some(TypeName::RegexMatch),
        "RandomGenerator" => Some(TypeName::RandomGenerator),
        "List" => Some(TypeName::List),
        "HashMap" => Some(TypeName::HashMap),
        "Generator" => Some(TypeName::Generator),
        "Result" => Some(TypeName::Result),
        "TypeRef" => Some(TypeName::TypeRef),
        "Hole" => Some(TypeName::Hole),
        "Lens" => Some(TypeName::Lens),
        _ => None,
    }
}
