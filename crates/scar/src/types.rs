use spire::ast::Symbol;

/// Surtr type — every typed node carries one of these.
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Str,
    Bool,
    Unit,

    /// `List<T>`
    List(Box<Ty>),

    /// MatchBlock-only heterogenous payload, represented at runtime as a list.
    Seq(Vec<Ty>),

    /// Generic function type: `(params) -> ret`
    Func(Vec<Ty>, Box<Ty>),

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
