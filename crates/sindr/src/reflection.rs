//! Compiler-owned source reflection functions; these have no VM builtin IDs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reflection {
    File,
    Dir,
    Line,
}

impl Reflection {
    pub const ALL: [Self; 3] = [Self::File, Self::Dir, Self::Line];

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.name() == name)
    }

    pub fn from_builtin_name(name: &str) -> Option<Self> {
        Self::from_name(name.strip_prefix("Bootstrap::")?)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::File => "__FILE__",
            Self::Dir => "__DIR__",
            Self::Line => "__LINE__",
        }
    }

    pub const fn type_name(self) -> &'static str {
        match self {
            Self::File | Self::Dir => "String",
            Self::Line => "Int",
        }
    }

    pub fn signature(self) -> String {
        format!("{}() -> {}", self.name(), self.type_name())
    }
}

/// Reserved for future reflection; only recognized to reject source use.
pub const RESERVED_ENV_NAME: &str = "__ENV__";
