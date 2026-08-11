use serde::{Deserialize, Serialize};
use spire::ast::Symbol;

/// The compiler-managed eligibility kind of a Facet path.
///
/// Atomic kinds are derived from the actual path. The remaining variants are
/// declaration-level constraints used only by Facet intrinsics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FacetKind {
    InfallibleStructural,
    FallibleStructural,
    VariantPath,
    ReadablePath,
    WritablePath,
    PutPath,
    PreviewPath,
    CasePath,
}

impl FacetKind {
    pub fn from_surface_name(name: &str) -> Option<Self> {
        match name.strip_prefix("Global::").unwrap_or(name) {
            "InfallibleStructural" => Some(Self::InfallibleStructural),
            "FallibleStructural" => Some(Self::FallibleStructural),
            "VariantPath" => Some(Self::VariantPath),
            "ReadablePath" => Some(Self::ReadablePath),
            "WritablePath" => Some(Self::WritablePath),
            "PutPath" => Some(Self::PutPath),
            "PreviewPath" => Some(Self::PreviewPath),
            "CasePath" => Some(Self::CasePath),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InfallibleStructural => "InfallibleStructural",
            Self::FallibleStructural => "FallibleStructural",
            Self::VariantPath => "VariantPath",
            Self::ReadablePath => "ReadablePath",
            Self::WritablePath => "WritablePath",
            Self::PutPath => "PutPath",
            Self::PreviewPath => "PreviewPath",
            Self::CasePath => "CasePath",
        }
    }

    pub fn accepts(self, actual: Self) -> bool {
        // Alias kinds may appear internally while checking polymorphic
        // standard-library signatures.  At the path/API boundary they are
        // compared with an atomic derived kind below.
        if self == actual {
            return true;
        }
        match self {
            Self::ReadablePath | Self::WritablePath => matches!(
                actual,
                Self::InfallibleStructural | Self::FallibleStructural | Self::VariantPath
            ),
            Self::PutPath => actual == Self::InfallibleStructural,
            Self::PreviewPath | Self::CasePath => actual == Self::VariantPath,
            _ => self == actual,
        }
    }

    pub fn is_atomic(self) -> bool {
        matches!(
            self,
            Self::InfallibleStructural | Self::FallibleStructural | Self::VariantPath
        )
    }
}

/// Surtr type — every typed node carries one of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Ty {
    Int,
    Float,
    Str,
    Bool,
    Unit,

    /// `List<T>`
    List(Box<Ty>),

    /// Tuple `(A, B, ...)`
    Tuple(Vec<Ty>),

    /// Generic function type: `(params) -> ret`
    Func(Vec<Ty>, Box<Ty>),

    /// Compiler-reserved target-type witness used only in restricted trait
    /// method parameter positions such as `From<$To>::from(_, TypeRef<$To>)`.
    TypeRef(Box<Ty>),

    /// Compiler-reserved lazy special-form marker used only in std builtin
    /// declarations. User code cannot name or transport Lazy values directly.
    Lazy(Box<Ty>),

    /// Compiler-managed facet path capability: `Facet<K, S, A, T, B>`.
    Facet(FacetKind, Box<Ty>, Box<Ty>, Box<Ty>, Box<Ty>),

    /// Process identifier capability: `PID<ProcessName>`
    Pid(Symbol),

    /// Compiler-reserved ignored-input closure marker.
    /// This is not a first-class data type and only appears in restricted
    /// callable surface positions.
    Hole,

    /// Built-in function with a known name (for codegen dispatch)
    BuiltinFunc {
        name: String,
        params: Vec<Ty>,
        ret: Box<Ty>,
    },

    /// User-defined function (phase 2)
    UserFunc {
        fun_idx: u32,
        type_params: Vec<u32>,
        params: Vec<Ty>,
        ret: Box<Ty>,
    },

    /// Type variable (for polymorphism): `$A`
    Var(u32),

    /// Deferred higher-kinded trait receiver application such as `Self<$A>`.
    /// Step 4 preserves this shape; impl-slot substitution is performed by
    /// the later trait validation phases.
    SelfApp(Vec<Ty>),

    /// Named struct: `User { name: String, age: Int }`
    Struct(Symbol, Vec<(Symbol, Ty)>),

    /// Named record: `Point(x: Float, y: Float)`
    Record(Symbol, Vec<(Symbol, Ty)>),

    /// Named enum: `Direction`, `ReduceStep<Int>`
    Enum(Symbol, Vec<Ty>),

    /// `Result<Ok, Err>`
    Result(Box<Ty>, Box<Ty>),

    /// Error type (produced by `deferror`)
    Error,
}
