//! Canonical compiler-owned Pattern consumer surfaces.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternConsumer {
    IfLet,
    IfLetThen,
    IsMatch,
    ApplyPattern,
}

impl PatternConsumer {
    pub const OWNER: &'static str = "Kernel";
    pub const fn pattern_index(self) -> usize {
        1
    }
    pub const ALL: [Self; 4] = [
        Self::IfLet,
        Self::IfLetThen,
        Self::IsMatch,
        Self::ApplyPattern,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::IfLet => "if_let",
            Self::IfLetThen => "if_let_then",
            Self::IsMatch => "is_match",
            Self::ApplyPattern => "apply_pattern",
        }
    }
    pub const fn arity(self) -> usize {
        match self {
            Self::IfLet => 4,
            Self::IfLetThen => 3,
            Self::IsMatch | Self::ApplyPattern => 2,
        }
    }
    pub const fn allows_or(self) -> bool {
        !matches!(self, Self::ApplyPattern)
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
    pub fn from_canonical_name(name: &str) -> Option<Self> {
        let name = name.strip_prefix("Global::").unwrap_or(name);
        let (owner, member) = name.split_once("::")?;
        (owner == Self::OWNER)
            .then(|| Self::from_name(member))
            .flatten()
    }
}

pub const MAX_PROJECTION_INDEX: u8 = 16;
