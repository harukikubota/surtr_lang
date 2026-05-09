use serde::{Deserialize, Serialize};
use spire::ast::Symbol;

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

    /// Compiler-managed facet path capability: `Facet<S, A>`
    Facet(Box<Ty>, Box<Ty>),

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
