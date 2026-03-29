use serde::{Deserialize, Serialize};

/// Surtr bytecode instructions.
///
/// Phase 1 opcodes only. MakeFrame / PopFrame / Return are phase 2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Opcode {
    // ── Constants & locals ──
    /// Push a constant from the pool onto the stack.
    LoadConst(u32),
    /// Push a builtin function reference onto the stack.
    LoadBuiltinRef(u16),
    /// Push a user-defined function reference onto the stack.
    LoadFunctionRef(u32),
    /// Load a local variable onto the stack.
    LoadLocal(u32),
    /// Pop the stack and store into a local variable slot.
    StoreLocal(u32),

    // ── Arithmetic (Int) ──
    AddInt,
    SubInt,
    MulInt,
    DivInt,
    ModInt,

    // ── Arithmetic (Float) ──
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,

    // ── Comparison (Int) ──
    EqInt,
    NeqInt,
    LtInt,
    GtInt,
    LteInt,
    GteInt,

    // ── Comparison (Float) ──
    EqFloat,
    NeqFloat,
    LtFloat,
    GtFloat,
    LteFloat,
    GteFloat,

    // ── Comparison (String) ──
    EqStr,
    NeqStr,

    // ── Comparison (Bool) ──
    EqBool,
    NeqBool,

    // ── String ──
    ConcatStr,

    // ── Unary (phase 1 — if needed) ──
    NegInt,
    NegFloat,
    NotBool,

    // ── List ──
    /// Construct a list from the top `n` stack values.
    ListNew(u32),
    /// Push an empty list.
    ListEmpty,

    // ── Struct / Tagged ──
    /// Construct a tagged value from the top `n` stack values.
    StructNew(u32),
    /// Extract field at index from a tagged value on top of the stack.
    GetField(u32),
    /// Push the tag (u32) of a tagged value on top of the stack.
    GetTag,

    // ── Built-in function call ──
    /// Call a built-in function by id with the given arity.
    /// `builtin_id` indexes into the BUILTINS table.
    CallBuiltin(u16, u8),

    // ── User-defined function call ──
    /// Call a user-defined function by function table index with the given arity.
    /// The trailing span is the call-site span in source offsets.
    Call(u32, u8, u32, u32),
    /// Create a closure from a function reference and captured values.
    MakeClosure(u8),
    /// Create an error value from a rendered message string.
    MakeError(u32),
    /// Call a callable value on the stack with the given arity.
    /// The trailing span is the call-site span in source offsets.
    CallClosure(u8, u32, u32),

    // ── Control flow ──
    /// Unconditional jump to absolute address.
    Jump(u32),
    /// Pop stack; jump if value is `false`.
    JumpIfFalse(u32),
    /// Pop stack; jump if value is `true`.
    JumpIfTrue(u32),

    // ── Stack management ──
    /// Discard top of stack.
    Pop,

    // ── Frame management ──
    /// Allocate a new locals frame for the current function.
    MakeFrame(u32),
    /// Tear down the current locals frame.
    PopFrame,

    // ── Function return ──
    /// Return from the current function.
    Return,

    // ── Program termination ──
    Halt,
}
