use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

use crate::builtin::builtin_meta_by_id;
use crate::primitives::{BuiltinId, FunctionId, RuntimeTag, SurtrInt};
use crate::runtime::{validate_type_registry_append, CallableOrigin, TypeEntry, TypeRegistry};

/// Surtr bytecode instructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Opcode {
    // Constants & locals
    LoadConst(u32),
    LoadBuiltinRef(BuiltinId),
    LoadFunctionRef(FunctionId),
    LoadLocal(u32),
    StoreLocal(u32),

    // Arithmetic (Int)
    AddInt,
    SubInt,
    MulInt,
    BitNotInt,
    BitAndInt,
    BitOrInt,
    BitXorInt,

    // Arithmetic (Float)
    AddFloat,
    SubFloat,
    MulFloat,

    // Comparison (Int)
    EqInt,
    NeqInt,
    LtInt,
    GtInt,
    LteInt,
    GteInt,

    // Comparison (Float)
    EqFloat,
    NeqFloat,
    LtFloat,
    GtFloat,
    LteFloat,
    GteFloat,

    // Comparison (String)
    EqStr,
    NeqStr,

    // Comparison (Bool)
    EqBool,
    NeqBool,

    // String
    ConcatStr,
    StringIsEmpty,
    StringHead,
    StringTail,

    // Unary
    NegInt,
    NegFloat,
    NotBool,

    // List
    ListEmpty,
    ListNil,
    ListCons,
    ListIsEmpty,
    ListHead,
    ListTail,
    ListFromItems {
        len: u32,
    },

    // Tuple
    TupleNew {
        len: u32,
    },
    GetTupleField {
        field_index: u32,
    },

    // Struct / Tagged
    StructNew {
        field_count: u32,
    },
    GetField {
        field_index: u32,
    },
    GetTag,
    EqTag,
    Dbg {
        template_id: u32,
        arg_count: u8,
    },

    // Built-in function call
    CallBuiltin {
        builtin_id: BuiltinId,
        arity: u8,
        span_start: u32,
        span_end: u32,
    },

    // User-defined function call
    Call {
        fun_idx: FunctionId,
        arity: u8,
        span_start: u32,
        span_end: u32,
    },
    CaptureClosure(u8),
    MakeError {
        template_id: u32,
    },
    MakeErrorLiteral {
        kind_const_idx: u32,
        message_const_idx: u32,
    },
    CallClosure {
        arity: u8,
        span_start: u32,
        span_end: u32,
    },

    // Control flow
    Jump(u32),
    JumpIfFalse(u32),
    JumpIfTrue(u32),

    // Stack management
    Pop,

    // Function return
    Return,

    // Program termination
    Halt,

    // Compressed opcodes. Keep appended to preserve existing bincode enum tags.
    StoreConstLocal {
        const_idx: u32,
        local_idx: u32,
    },
    CopyLocal {
        src_local_idx: u32,
        dst_local_idx: u32,
    },
    EqLocalTag {
        local_idx: u32,
        tag_const_idx: u32,
    },
    StringLen,
    ListLen,
    SafeModInt,
    ShlInt,
    ShrInt,
    TestBitInt,
    SetBitInt,
    ClearBitInt,
    ToggleBitInt,
    StringContains,
    StringStartsWith,
    StringEndsWith,
    MakeOk,
    MakeErr,
    JumpIfLocalTagEq {
        local_idx: u32,
        tag_const_idx: u32,
        target_pc: u32,
    },
    JumpIfLocalTagNe {
        local_idx: u32,
        tag_const_idx: u32,
        target_pc: u32,
    },
    TailCallClosure {
        arity: u8,
        span_start: u32,
        span_end: u32,
    },
    LoadCallableTemplateRef(u32),
}

impl Opcode {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::LoadConst(..) => "LoadConst",
            Self::LoadBuiltinRef(..) => "LoadBuiltinRef",
            Self::LoadFunctionRef(..) => "LoadFunctionRef",
            Self::LoadLocal(..) => "LoadLocal",
            Self::StoreLocal(..) => "StoreLocal",
            Self::StoreConstLocal { .. } => "StoreConstLocal",
            Self::CopyLocal { .. } => "CopyLocal",
            Self::EqLocalTag { .. } => "EqLocalTag",
            Self::StringLen => "StringLen",
            Self::ListLen => "ListLen",
            Self::SafeModInt => "SafeModInt",
            Self::ShlInt => "ShlInt",
            Self::ShrInt => "ShrInt",
            Self::TestBitInt => "TestBitInt",
            Self::SetBitInt => "SetBitInt",
            Self::ClearBitInt => "ClearBitInt",
            Self::ToggleBitInt => "ToggleBitInt",
            Self::StringContains => "StringContains",
            Self::StringStartsWith => "StringStartsWith",
            Self::StringEndsWith => "StringEndsWith",
            Self::MakeOk => "MakeOk",
            Self::MakeErr => "MakeErr",
            Self::JumpIfLocalTagEq { .. } => "JumpIfLocalTagEq",
            Self::JumpIfLocalTagNe { .. } => "JumpIfLocalTagNe",
            Self::TailCallClosure { .. } => "TailCallClosure",
            Self::LoadCallableTemplateRef(..) => "LoadCallableTemplateRef",
            Self::AddInt => "AddInt",
            Self::SubInt => "SubInt",
            Self::MulInt => "MulInt",
            Self::BitNotInt => "BitNotInt",
            Self::BitAndInt => "BitAndInt",
            Self::BitOrInt => "BitOrInt",
            Self::BitXorInt => "BitXorInt",
            Self::AddFloat => "AddFloat",
            Self::SubFloat => "SubFloat",
            Self::MulFloat => "MulFloat",
            Self::EqInt => "EqInt",
            Self::NeqInt => "NeqInt",
            Self::LtInt => "LtInt",
            Self::GtInt => "GtInt",
            Self::LteInt => "LteInt",
            Self::GteInt => "GteInt",
            Self::EqFloat => "EqFloat",
            Self::NeqFloat => "NeqFloat",
            Self::LtFloat => "LtFloat",
            Self::GtFloat => "GtFloat",
            Self::LteFloat => "LteFloat",
            Self::GteFloat => "GteFloat",
            Self::EqStr => "EqStr",
            Self::NeqStr => "NeqStr",
            Self::EqBool => "EqBool",
            Self::NeqBool => "NeqBool",
            Self::ConcatStr => "ConcatStr",
            Self::StringIsEmpty => "StringIsEmpty",
            Self::StringHead => "StringHead",
            Self::StringTail => "StringTail",
            Self::NegInt => "NegInt",
            Self::NegFloat => "NegFloat",
            Self::NotBool => "NotBool",
            Self::ListEmpty => "ListEmpty",
            Self::ListNil => "ListNil",
            Self::ListCons => "ListCons",
            Self::ListIsEmpty => "ListIsEmpty",
            Self::ListHead => "ListHead",
            Self::ListTail => "ListTail",
            Self::ListFromItems { .. } => "ListFromItems",
            Self::TupleNew { .. } => "TupleNew",
            Self::GetTupleField { .. } => "GetTupleField",
            Self::StructNew { .. } => "StructNew",
            Self::GetField { .. } => "GetField",
            Self::GetTag => "GetTag",
            Self::EqTag => "EqTag",
            Self::Dbg { .. } => "Dbg",
            Self::CallBuiltin { .. } => "CallBuiltin",
            Self::Call { .. } => "Call",
            Self::CaptureClosure(..) => "CaptureClosure",
            Self::MakeError { .. } => "MakeError",
            Self::MakeErrorLiteral { .. } => "MakeErrorLiteral",
            Self::CallClosure { .. } => "CallClosure",
            Self::Jump(..) => "Jump",
            Self::JumpIfFalse(..) => "JumpIfFalse",
            Self::JumpIfTrue(..) => "JumpIfTrue",
            Self::Pop => "Pop",
            Self::Return => "Return",
            Self::Halt => "Halt",
        }
    }
}

/// Opcode index -> source span/line metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMap {
    pub entries: Vec<OpcodeSource>,
}

/// Source location attached to one opcode index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpcodeSource {
    pub opcode_index: u32,
    pub span_start: u32,
    pub span_end: u32,
    pub line: u32,
    pub column: u32,
    #[serde(default)]
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FunctionFlags {
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub closure: bool,
    #[serde(default)]
    pub partial_apply_wrapper: bool,
    #[serde(default)]
    pub builtin_wrapper: bool,
    #[serde(default)]
    pub tail_entry: bool,
    #[serde(default)]
    pub generated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileInfo {
    pub bytecode_version: u32,
    pub debug_level: u32,
    pub num_locals: usize,
    #[serde(default)]
    pub compiler_version: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub build_profile: Option<String>,
    #[serde(default)]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub module_hash: Option<String>,
}

impl Default for CompileInfo {
    fn default() -> Self {
        Self {
            bytecode_version: 2,
            debug_level: 2,
            num_locals: 0,
            compiler_version: None,
            target: None,
            build_profile: None,
            source_hash: None,
            module_hash: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelEntry {
    pub name: String,
    pub pc: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportKind {
    Builtin,
    Function,
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportEntry {
    pub symbol: String,
    pub kind: ImportKind,
    pub arity: u8,
    #[serde(default)]
    pub builtin_id: Option<u16>,
    #[serde(default)]
    pub function_id: Option<u32>,
    #[serde(default)]
    pub call_pcs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportEntry {
    pub symbol: String,
    pub arity: u8,
    pub function_id: u32,
    pub entry_pc: u32,
    #[serde(default)]
    pub doc_qualified_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiteralKind {
    Int,
    Tag,
    Float,
    Str,
    Bool,
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiteralEntry {
    pub const_idx: u32,
    pub kind: LiteralKind,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineEntry {
    pub line: u32,
    pub start_pc: u32,
    pub end_pc: u32,
    #[serde(default)]
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanEntry {
    pub span_id: u32,
    pub span_start: u32,
    pub span_end: u32,
    pub line: u32,
    pub column: u32,
    #[serde(default)]
    pub source_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFileEntry {
    pub source_id: u32,
    pub path: String,
    #[serde(default)]
    pub normalized_path: Option<String>,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcSpanEntry {
    pub pc: u32,
    pub span_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeProcessKind {
    Agent,
    GenServer,
    Supervisor,
    RuntimeSupervisor,
    DynamicSupervisor,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeProcessInstance {
    Singleton,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProcessSpec {
    pub process_id: u32,
    pub type_name: String,
    pub kind: RuntimeProcessKind,
    pub instance: RuntimeProcessInstance,
    pub state: RuntimeStateSpec,
    pub init: RuntimeInitSpec,
    #[serde(default)]
    pub handlers: Vec<RuntimeHandlerSpec>,
    #[serde(default)]
    pub dependencies: RuntimeProcessDependencies,
    #[serde(default)]
    pub lifecycle: RuntimeLifecycleSpec,
    #[serde(default)]
    pub supervision: RuntimeSupervisionSpec,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeTypeRef {
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStateSpec {
    pub state_type: RuntimeTypeRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInitSpec {
    pub callable: RuntimeCallableRef,
    pub policy: RuntimeInitPolicy,
    pub result_shape: RuntimeInitResultShape,
    pub state_type: RuntimeTypeRef,
    #[serde(default)]
    pub init_route: Option<RuntimeInitRouteRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCallableRef {
    pub fun_idx: FunctionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInitRouteRef {
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeInitPolicy {
    Eager,
    Lazy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeInitResultShape {
    EagerState { result_type: RuntimeTypeRef },
    LazyProcessInit { result_type: RuntimeTypeRef },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProcessDependencies {
    #[serde(default)]
    pub handlers: Vec<RuntimeHandlerDependency>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLifecycleSpec {
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub exit_sink: Option<String>,
    #[serde(default)]
    pub restart_target: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSupervisionSpec {
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
    #[serde(default)]
    pub policy: Option<RuntimeSupervisorPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSupervisorPolicy {
    pub strategy: String,
    pub max_restarts: u64,
    pub max_seconds: u64,
    pub child_restart_default: String,
    pub allow_adopt: bool,
    #[serde(default)]
    pub shutdown_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHandlerDependency {
    pub slot: String,
    pub capability: String,
    pub default_target: RuntimeHandlerTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeHandlerKind {
    Init,
    Get,
    Set,
    Call,
    Cast,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHandlerSpec {
    pub handler_id: u32,
    pub name: String,
    pub kind: RuntimeHandlerKind,
    pub fun_idx: FunctionId,
    pub arity: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProcessSpecTable {
    #[serde(default)]
    pub entries: Vec<RuntimeProcessSpec>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBootPlan {
    #[serde(default)]
    pub root: RootSupervisorPlan,
    #[serde(default)]
    pub singletons: Vec<SingletonBootEntry>,
    #[serde(default)]
    pub standard_overrides: Vec<StandardOverrideEntry>,
    #[serde(default)]
    pub handler_overrides: Vec<RuntimeHandlerOverride>,
    #[serde(default)]
    pub supervisor_overrides: Vec<RuntimeSupervisorOverrideEntry>,
    #[serde(default)]
    pub runtime_limits: RuntimeLimitConfig,
}

impl RuntimeBootPlan {
    pub fn explicit_singleton(process_name: impl Into<String>) -> Self {
        Self {
            singletons: vec![SingletonBootEntry {
                process_name: process_name.into(),
                init_timeout_ms: RuntimeLimitConfig::default().default_init_timeout_ms,
                source: BootEntrySource::ExplicitConfig,
            }],
            ..Self::default()
        }
    }

    pub fn has_explicit_entries(&self) -> bool {
        !self.singletons.is_empty()
            || !self.standard_overrides.is_empty()
            || !self.supervisor_overrides.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootSupervisorPlan {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingletonBootEntry {
    pub process_name: String,
    #[serde(default = "default_init_timeout_ms")]
    pub init_timeout_ms: u64,
    pub source: BootEntrySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootEntrySource {
    ExplicitConfig,
    BuiltinStandardIo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardOverrideEntry {
    pub standard_name: String,
    pub handler_target: RuntimeHandlerTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHandlerOverride {
    pub target_process: String,
    pub slot: String,
    pub handler_target: RuntimeHandlerTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSupervisorOverrideEntry {
    pub process_name: String,
    pub policy: RuntimeSupervisorPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHandlerTarget {
    pub name: String,
    #[serde(default)]
    pub named_args: Vec<RuntimeHandlerArg>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHandlerArg {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLimitConfig {
    pub default_init_timeout_ms: u64,
    pub min_init_timeout_ms: u64,
    pub max_init_timeout_ms: u64,
    pub pending_initial_retry_ms: u64,
    pub pending_max_retry_ms: u64,
    pub min_scheduler_tick_ms: u64,
}

impl Default for RuntimeLimitConfig {
    fn default() -> Self {
        Self {
            default_init_timeout_ms: default_init_timeout_ms(),
            min_init_timeout_ms: 1,
            max_init_timeout_ms: 60_000,
            pending_initial_retry_ms: 10,
            pending_max_retry_ms: 1_000,
            min_scheduler_tick_ms: 1,
        }
    }
}

fn default_init_timeout_ms() -> u64 {
    5_000
}

/// A compiled Surtr program, ready for Eldr to execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bytecode {
    pub opcodes: Vec<Opcode>,
    pub constants: Vec<Constant>,
    pub num_locals: usize,
    pub type_registry: TypeRegistry,
    pub error_templates: Vec<ErrTemplate>,
    #[serde(default)]
    pub dbg_templates: Vec<DbgTemplate>,
    #[serde(default)]
    pub callable_templates: Vec<CallableTemplate>,
    pub functions: Vec<FunctionEntry>,
    pub source_map: Option<SourceMap>,
    /// Symbol-level documentation carried from `@doc` through `.eldr`.
    #[serde(default)]
    pub docs: Vec<DocEntry>,
    /// Signature-only surface metadata used by REPL `:sig`.
    #[serde(default)]
    pub signatures: Vec<SignatureEntry>,
    #[serde(default)]
    pub compile_info: CompileInfo,
    #[serde(default)]
    pub labels: Vec<LabelEntry>,
    #[serde(default)]
    pub imports: Vec<ImportEntry>,
    #[serde(default)]
    pub exports: Vec<ExportEntry>,
    #[serde(default)]
    pub literals: Vec<LiteralEntry>,
    #[serde(default)]
    pub lines: Vec<LineEntry>,
    #[serde(default)]
    pub spans: Vec<SpanEntry>,
    #[serde(default)]
    pub sources: Vec<SourceFileEntry>,
    #[serde(default)]
    pub pc_spans: Vec<PcSpanEntry>,
    #[serde(default)]
    pub runtime_process_specs: RuntimeProcessSpecTable,
    #[serde(default)]
    pub runtime_boot_plan: RuntimeBootPlan,
}

impl Default for Bytecode {
    fn default() -> Self {
        Self {
            opcodes: Vec::new(),
            constants: Vec::new(),
            num_locals: 0,
            type_registry: TypeRegistry::new(),
            error_templates: Vec::new(),
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            functions: Vec::new(),
            source_map: None,
            docs: Vec::new(),
            signatures: Vec::new(),
            compile_info: CompileInfo::default(),
            labels: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            literals: Vec::new(),
            lines: Vec::new(),
            spans: Vec::new(),
            sources: Vec::new(),
            pc_spans: Vec::new(),
            runtime_process_specs: RuntimeProcessSpecTable::default(),
            runtime_boot_plan: RuntimeBootPlan::default(),
        }
    }
}

/// Incremental bytecode payload for REPL execution.
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeChunk {
    pub opcodes: Vec<Opcode>,
    pub source_map: Option<SourceMap>,
    /// Base offset of constants in the VM-wide pool when this chunk is produced.
    pub const_base: u32,
    pub constants: Vec<Constant>,
    pub new_locals: usize,
    /// Base offset of runtime type metadata when this chunk is produced.
    pub type_registry_base: u32,
    pub type_entries: Vec<TypeEntry>,
    /// Base offset of error templates in the VM-wide pool when this chunk is produced.
    pub error_template_base: u32,
    pub error_templates: Vec<ErrTemplate>,
    /// Base offset of dbg templates in the VM-wide pool when this chunk is produced.
    pub dbg_template_base: u32,
    pub dbg_templates: Vec<DbgTemplate>,
    pub callable_templates: Vec<CallableTemplate>,
    pub functions: Vec<FunctionEntry>,
    pub docs: Vec<DocEntry>,
    pub signatures: Vec<SignatureEntry>,
    pub runtime_process_specs: Vec<RuntimeProcessSpec>,
    pub runtime_boot_plan: RuntimeBootPlan,
}

/// Function table entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionEntry {
    pub fun_idx: FunctionId,
    pub entry_pc: u32,
    pub num_locals: u32,
    pub arity: u8,
    pub qualified_name: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub end_pc: u32,
    #[serde(default)]
    pub span_start: u32,
    #[serde(default)]
    pub span_end: u32,
    #[serde(default)]
    pub flags: FunctionFlags,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableTemplateDirectTarget {
    Builtin(BuiltinId),
    Function(FunctionId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableTemplateArg {
    Bound(u8),
    Runtime(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableTemplateComposeFlavor {
    Plain,
    ResultMap,
    ResultBind,
    ListMap,
    ListBind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableTemplateKind {
    PartialDirectCall {
        target: CallableTemplateDirectTarget,
        arg_sources: Vec<CallableTemplateArg>,
    },
    InjectDirectCall {
        target: CallableTemplateDirectTarget,
        bound_arg_count: u8,
    },
    ComposeDirect {
        flavor: CallableTemplateComposeFlavor,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallableTemplate {
    pub template_id: u32,
    pub kind: CallableTemplateKind,
    #[serde(default)]
    pub metadata: CallableTemplateMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallableTemplateMetadata {
    pub origin: CallableOrigin,
    pub module: Option<String>,
    pub name: Option<String>,
    pub full_signature: Option<String>,
}

impl Default for CallableTemplateMetadata {
    fn default() -> Self {
        Self {
            origin: CallableOrigin::Unknown,
            module: None,
            name: None,
            full_signature: None,
        }
    }
}

/// Shared bytecode verifier error used by producers and the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeValidationError {
    message: String,
}

impl BytecodeValidationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for BytecodeValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BytecodeValidationError {}

impl From<crate::runtime::TypeRegistryError> for BytecodeValidationError {
    fn from(value: crate::runtime::TypeRegistryError) -> Self {
        Self::new(value.to_string())
    }
}

pub fn validate_type_registry_append_entries(
    existing: &[TypeEntry],
    entries: &[TypeEntry],
) -> Result<(), BytecodeValidationError> {
    validate_type_registry_append(existing, entries).map_err(Into::into)
}

pub fn validate_program_function_table(
    opcodes: &[Opcode],
    callable_templates: &[CallableTemplate],
    runtime_process_specs: &[RuntimeProcessSpec],
    functions: &[FunctionEntry],
) -> Result<(), BytecodeValidationError> {
    let halt_pos = top_level_halt_pos(opcodes, "bytecode")?;
    let mut seen_fun_idxs = BTreeSet::new();
    for (idx, entry) in functions.iter().enumerate() {
        if entry.fun_idx as usize != idx {
            return Err(BytecodeValidationError::new(format!(
                "Function table invariant violated: functions[{}].fun_idx = {}",
                idx, entry.fun_idx
            )));
        }
        if !seen_fun_idxs.insert(entry.fun_idx) {
            return Err(BytecodeValidationError::new(format!(
                "Bytecode verifier: duplicate function entry for fun_idx {}",
                entry.fun_idx
            )));
        }
        validate_function_entry_pc(entry, opcodes.len(), halt_pos, "")?;
    }

    validate_function_refs(
        functions.len(),
        opcodes,
        callable_templates,
        runtime_process_specs,
    )
}

pub fn validate_chunk_function_table(
    opcodes: &[Opcode],
    callable_templates: &[CallableTemplate],
    runtime_process_specs: &[RuntimeProcessSpec],
    functions: &[FunctionEntry],
    base_function_len: usize,
    allow_existing_replacements: bool,
) -> Result<usize, BytecodeValidationError> {
    let halt_pos = top_level_halt_pos(opcodes, "chunk")?;
    let mut seen_fun_idxs = BTreeSet::new();
    let mut next_append_idx = base_function_len;
    let mut sorted_entries = functions.iter().collect::<Vec<_>>();
    sorted_entries.sort_by_key(|entry| entry.fun_idx);

    for entry in sorted_entries {
        let idx = entry.fun_idx as usize;
        if !seen_fun_idxs.insert(entry.fun_idx) {
            return Err(BytecodeValidationError::new(format!(
                "Bytecode verifier: duplicate function entry for fun_idx {}",
                entry.fun_idx
            )));
        }
        if idx < base_function_len {
            if !allow_existing_replacements {
                return Err(BytecodeValidationError::new(format!(
                    "Function table invariant violated in chunk: fun_idx {} would replace existing prefix len {}",
                    idx, base_function_len
                )));
            }
        } else if idx == next_append_idx {
            next_append_idx += 1;
        } else {
            return Err(BytecodeValidationError::new(format!(
                "Function table invariant violated in chunk: fun_idx {} > len {}",
                idx, next_append_idx
            )));
        }
        validate_function_entry_pc(entry, opcodes.len(), halt_pos, "chunk ")?;
    }

    validate_function_refs(
        next_append_idx,
        opcodes,
        callable_templates,
        runtime_process_specs,
    )?;
    Ok(next_append_idx)
}

fn top_level_halt_pos(opcodes: &[Opcode], context: &str) -> Result<usize, BytecodeValidationError> {
    if let Some(pos) = opcodes.iter().position(|op| matches!(op, Opcode::Halt)) {
        return Ok(pos);
    }
    if opcodes
        .iter()
        .any(|op| matches!(op, Opcode::Return | Opcode::TailCallClosure { .. }))
    {
        return Err(BytecodeValidationError::new("Return at top-level"));
    }
    Err(BytecodeValidationError::new(format!(
        "Bytecode verifier: {context} missing Halt"
    )))
}

fn validate_function_entry_pc(
    entry: &FunctionEntry,
    opcode_len: usize,
    halt_pos: usize,
    context: &str,
) -> Result<(), BytecodeValidationError> {
    if entry.entry_pc as usize >= opcode_len {
        return Err(BytecodeValidationError::new(format!(
            "Bytecode verifier: function {} entry_pc out of {}bounds: {}",
            entry.fun_idx, context, entry.entry_pc
        )));
    }
    if entry.entry_pc as usize <= halt_pos {
        return Err(BytecodeValidationError::new(format!(
            "Bytecode verifier: function {} entry_pc {} must be after top-level Halt {}",
            entry.fun_idx, entry.entry_pc, halt_pos
        )));
    }
    Ok(())
}

fn validate_function_refs(
    function_len: usize,
    opcodes: &[Opcode],
    callable_templates: &[CallableTemplate],
    runtime_process_specs: &[RuntimeProcessSpec],
) -> Result<(), BytecodeValidationError> {
    for op in opcodes {
        match op {
            Opcode::LoadFunctionRef(fun_idx) | Opcode::Call { fun_idx, .. } => {
                validate_function_ref(function_len, *fun_idx, "opcode")?;
            }
            _ => {}
        }
    }

    for template in callable_templates {
        match &template.kind {
            CallableTemplateKind::PartialDirectCall { target, .. }
            | CallableTemplateKind::InjectDirectCall { target, .. } => {
                if let CallableTemplateDirectTarget::Function(fun_idx) = target {
                    validate_function_ref(function_len, *fun_idx, "callable template")?;
                }
            }
            CallableTemplateKind::ComposeDirect { .. } => {}
        }
    }

    for spec in runtime_process_specs {
        validate_function_ref(
            function_len,
            spec.init.callable.fun_idx,
            "runtime process init",
        )?;
        for handler in &spec.handlers {
            validate_function_ref(function_len, handler.fun_idx, "runtime process handler")?;
        }
    }

    Ok(())
}

fn validate_function_ref(
    function_len: usize,
    fun_idx: FunctionId,
    context: &str,
) -> Result<(), BytecodeValidationError> {
    if fun_idx as usize >= function_len {
        return Err(BytecodeValidationError::new(format!(
            "Bytecode verifier: Unknown function index {} in {} (function table len {})",
            fun_idx, context, function_len
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocKind {
    Module,
    Type,
    Function,
}

/// Persisted documentation entry stored in `.eldr` `Docs` chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocEntry {
    pub qualified_name: String,
    pub kind: DocKind,
    pub module_path: String,
    pub signature: Option<String>,
    pub doc: String,
}

/// Persisted signature entry stored in `.eldr` `SigT` chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureEntry {
    pub qualified_name: String,
    pub kind: DocKind,
    pub module_path: String,
    pub signature: String,
}

/// Constant pool entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Int(SurtrInt),
    Tag(RuntimeTag),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
}

/// Error template — baked location info for `deferror` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrTemplate {
    pub id: u32,
    pub kind: String,
    pub span_start: u32,
    pub span_end: u32,
    pub line: u32,
    pub column: u32,
    pub format: String,
    pub num_params: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbgArgTemplate {
    pub span_start: u32,
    pub span_end: u32,
    pub ty_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbgTemplate {
    pub id: u32,
    pub span_start: u32,
    pub span_end: u32,
    #[serde(default)]
    pub source_name: Option<String>,
    pub args: Vec<DbgArgTemplate>,
}

pub fn populate_error_template_lines(error_templates: &mut [ErrTemplate], source: &str) {
    for template in error_templates {
        let (line, column) = line_column_for_offset(source, template.span_start as usize);
        template.line = line;
        template.column = column;
    }
}

pub fn line_column_for_offset(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut column = 1u32;

    let limit = offset.min(source.chars().count());
    for ch in source.chars().take(limit) {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

pub fn stable_hash_hex(input: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

pub fn synthesize_source_map(
    opcodes: &[Opcode],
    functions: &[FunctionEntry],
    error_templates: &[ErrTemplate],
    dbg_templates: &[DbgTemplate],
    source: &str,
    source_name: Option<&str>,
) -> Option<SourceMap> {
    let mut entries = Vec::new();
    for (opcode_index, opcode) in opcodes.iter().enumerate() {
        let span = opcode_span(
            opcode,
            functions,
            error_templates,
            dbg_templates,
            opcode_index as u32,
        )?;
        let (line, column) = line_column_for_offset(source, span.0 as usize);
        entries.push(OpcodeSource {
            opcode_index: opcode_index as u32,
            span_start: span.0,
            span_end: span.1,
            line,
            column,
            source_name: source_name.map(str::to_string),
        });
    }

    if entries.is_empty() {
        None
    } else {
        Some(SourceMap { entries })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BytecodeFormatError {
    HeaderTooShort,
    InvalidMagic([u8; 4]),
    UnsupportedVersion(u32),
    TruncatedChunkHeader,
    TruncatedChunkData,
    MissingRequiredChunk(String),
    DuplicateChunkTag(String),
    UnknownChunkTag(String),
    EncodeFailed(String),
    DecodeFailed(String),
}

impl std::fmt::Display for BytecodeFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeFormatError::HeaderTooShort => write!(f, "bytecode header is too short"),
            BytecodeFormatError::InvalidMagic(magic) => {
                write!(f, "invalid bytecode magic: {:?}", magic)
            }
            BytecodeFormatError::UnsupportedVersion(version) => {
                write!(f, "unsupported bytecode version: {}", version)
            }
            BytecodeFormatError::TruncatedChunkHeader => write!(f, "truncated chunk header"),
            BytecodeFormatError::TruncatedChunkData => write!(f, "truncated chunk data"),
            BytecodeFormatError::MissingRequiredChunk(tag) => {
                write!(f, "missing required chunk: {}", tag)
            }
            BytecodeFormatError::DuplicateChunkTag(tag) => {
                write!(f, "duplicate chunk tag: {}", tag)
            }
            BytecodeFormatError::UnknownChunkTag(tag) => write!(f, "unknown chunk tag: {}", tag),
            BytecodeFormatError::EncodeFailed(msg) => {
                write!(f, "failed to encode bytecode: {}", msg)
            }
            BytecodeFormatError::DecodeFailed(msg) => {
                write!(f, "failed to decode bytecode payload: {}", msg)
            }
        }
    }
}

impl std::error::Error for BytecodeFormatError {}

/// `.eldr` header fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EldrHeader {
    pub magic: String,
    pub version: u32,
    pub debug_level: u32,
    pub num_chunks: u32,
}

/// `.eldr` chunk metadata.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EldrChunkInfo {
    pub tag: String,
    pub size: u32,
    pub payload_offset: usize,
    pub padded_size: usize,
}

/// Parsed `.eldr` container + decoded bytecode.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EldrInspect {
    pub header: EldrHeader,
    pub chunks: Vec<EldrChunkInfo>,
    pub bytecode: Bytecode,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedContainer<'a> {
    header: EldrHeader,
    chunks: Vec<EldrChunkInfo>,
    payloads: std::collections::BTreeMap<String, &'a [u8]>,
}

impl Bytecode {
    const MAGIC: [u8; 4] = *b"ELDR";
    const VERSION: u32 = 2;
    const HEADER_LEN: usize = 16;
    const CHUNK_HEADER_LEN: usize = 8;
    const CHUNK_CODE: [u8; 4] = *b"Code";
    const CHUNK_CONSTS: [u8; 4] = *b"Cnst";
    const CHUNK_FUNCS: [u8; 4] = *b"Func";
    const CHUNK_TYPES: [u8; 4] = *b"Type";
    const CHUNK_ERRORS: [u8; 4] = *b"ErrT";
    const CHUNK_DBGS: [u8; 4] = *b"DbgT";
    const CHUNK_CALLABLE_TEMPLATES: [u8; 4] = *b"CalT";
    const CHUNK_COMPILE_INFO: [u8; 4] = *b"CInf";
    const CHUNK_LABELS: [u8; 4] = *b"LblT";
    const CHUNK_IMPORTS: [u8; 4] = *b"ImpT";
    const CHUNK_EXPORTS: [u8; 4] = *b"ExpT";
    const CHUNK_LITERALS: [u8; 4] = *b"LitT";
    const CHUNK_LINES: [u8; 4] = *b"Line";
    const CHUNK_SPANS: [u8; 4] = *b"SpnT";
    const CHUNK_SOURCES: [u8; 4] = *b"SrcP";
    const CHUNK_PC_SPANS: [u8; 4] = *b"PcSp";
    const CHUNK_DOCS: [u8; 4] = *b"Docs";
    const CHUNK_SIGNATURES: [u8; 4] = *b"SigT";
    const CHUNK_PROCESS_SPECS: [u8; 4] = *b"Proc";
    const CHUNK_BOOT_PLAN: [u8; 4] = *b"Boot";

    pub fn refresh_viewer_metadata(&mut self) {
        self.compile_info.num_locals = self.num_locals;
        self.compile_info.bytecode_version = Self::VERSION;
        if self.sources.is_empty() {
            self.sources = derive_sources(self.source_map.as_ref());
        }
        populate_function_ranges(&mut self.functions, self.opcodes.len() as u32);
        self.labels = derive_labels(&self.opcodes, &self.functions);
        self.imports = derive_imports(&self.opcodes, &self.functions);
        self.exports = derive_exports(&self.functions, &self.docs);
        self.literals = derive_literals(&self.constants);
        let (spans, pc_spans, lines) = derive_source_tables(self.source_map.as_ref());
        self.spans = spans;
        self.pc_spans = pc_spans;
        self.lines = lines;
    }

    /// Encode bytecode as `.eldr` bytes:
    /// Header(16 bytes) + chunk table + payloads.
    pub fn encode(&self) -> Result<Vec<u8>, BytecodeFormatError> {
        let mut bytecode = self.clone();
        bytecode.refresh_viewer_metadata();

        let mut chunks = vec![
            (Self::CHUNK_CODE, serialize_chunk(&bytecode.opcodes)?),
            (Self::CHUNK_CONSTS, serialize_chunk(&bytecode.constants)?),
            (Self::CHUNK_FUNCS, serialize_chunk(&bytecode.functions)?),
            (Self::CHUNK_TYPES, serialize_chunk(&bytecode.type_registry)?),
            (
                Self::CHUNK_ERRORS,
                serialize_chunk(&bytecode.error_templates)?,
            ),
            (Self::CHUNK_DBGS, serialize_chunk(&bytecode.dbg_templates)?),
            (
                Self::CHUNK_CALLABLE_TEMPLATES,
                serialize_chunk(&bytecode.callable_templates)?,
            ),
            (
                Self::CHUNK_COMPILE_INFO,
                serialize_chunk(&bytecode.compile_info)?,
            ),
            (Self::CHUNK_LABELS, serialize_chunk(&bytecode.labels)?),
            (Self::CHUNK_IMPORTS, serialize_chunk(&bytecode.imports)?),
            (Self::CHUNK_EXPORTS, serialize_chunk(&bytecode.exports)?),
            (Self::CHUNK_LITERALS, serialize_chunk(&bytecode.literals)?),
            (Self::CHUNK_LINES, serialize_chunk(&bytecode.lines)?),
            (Self::CHUNK_SPANS, serialize_chunk(&bytecode.spans)?),
            (Self::CHUNK_SOURCES, serialize_chunk(&bytecode.sources)?),
            (Self::CHUNK_PC_SPANS, serialize_chunk(&bytecode.pc_spans)?),
            (
                Self::CHUNK_PROCESS_SPECS,
                serialize_chunk(&bytecode.runtime_process_specs)?,
            ),
            (
                Self::CHUNK_BOOT_PLAN,
                serialize_chunk(&bytecode.runtime_boot_plan)?,
            ),
        ];

        if !bytecode.docs.is_empty() {
            chunks.push((Self::CHUNK_DOCS, serialize_chunk(&bytecode.docs)?));
        }
        if !bytecode.signatures.is_empty() {
            chunks.push((
                Self::CHUNK_SIGNATURES,
                serialize_chunk(&bytecode.signatures)?,
            ));
        }

        let num_chunks = chunks.len() as u32;
        let total_len = Self::HEADER_LEN
            + (Self::CHUNK_HEADER_LEN * chunks.len())
            + chunks
                .iter()
                .map(|(_, payload)| align4(payload.len()))
                .sum::<usize>();
        let mut bytes = Vec::with_capacity(total_len);

        bytes.extend_from_slice(&Self::MAGIC);
        bytes.extend_from_slice(&Self::VERSION.to_le_bytes());
        bytes.extend_from_slice(&bytecode.compile_info.debug_level.to_le_bytes());
        bytes.extend_from_slice(&num_chunks.to_le_bytes());
        for (tag, payload) in &chunks {
            bytes.extend_from_slice(tag);
            bytes.extend_from_slice(&checked_payload_len(payload.len())?.to_le_bytes());
        }
        for (_, payload) in chunks {
            bytes.extend_from_slice(&payload);
            bytes.resize(align4(bytes.len()), 0);
        }
        Ok(bytes)
    }

    /// Inspect `.eldr` bytes and decode embedded bytecode.
    pub fn inspect(bytes: &[u8]) -> Result<EldrInspect, BytecodeFormatError> {
        let parsed = parse_container(bytes)?;
        let bytecode = decode_payloads(&parsed.payloads)?;
        Ok(EldrInspect {
            header: parsed.header,
            chunks: parsed.chunks,
            bytecode,
        })
    }

    /// Decode `.eldr` bytes into bytecode.
    pub fn decode(bytes: &[u8]) -> Result<Self, BytecodeFormatError> {
        let parsed = parse_container(bytes)?;
        decode_payloads(&parsed.payloads)
    }
}

fn decode_payloads(
    payloads: &std::collections::BTreeMap<String, &[u8]>,
) -> Result<Bytecode, BytecodeFormatError> {
    let opcodes = deserialize_required::<Vec<Opcode>>(payloads, "Code")?;
    let constants = deserialize_required::<Vec<Constant>>(payloads, "Cnst")?;
    let functions = deserialize_required::<Vec<FunctionEntry>>(payloads, "Func")?;
    let type_registry = deserialize_required::<TypeRegistry>(payloads, "Type")?;
    let error_templates = deserialize_required::<Vec<ErrTemplate>>(payloads, "ErrT")?;
    let dbg_templates =
        deserialize_optional::<Vec<DbgTemplate>>(payloads, "DbgT")?.unwrap_or_default();
    let callable_templates =
        deserialize_optional::<Vec<CallableTemplate>>(payloads, "CalT")?.unwrap_or_default();
    let compile_info = deserialize_required::<CompileInfo>(payloads, "CInf")?;
    let labels = deserialize_required::<Vec<LabelEntry>>(payloads, "LblT")?;
    let imports = deserialize_required::<Vec<ImportEntry>>(payloads, "ImpT")?;
    let exports = deserialize_required::<Vec<ExportEntry>>(payloads, "ExpT")?;
    let literals = deserialize_required::<Vec<LiteralEntry>>(payloads, "LitT")?;
    let lines = deserialize_required::<Vec<LineEntry>>(payloads, "Line")?;
    let spans = deserialize_required::<Vec<SpanEntry>>(payloads, "SpnT")?;
    let sources = deserialize_required::<Vec<SourceFileEntry>>(payloads, "SrcP")?;
    let pc_spans = deserialize_required::<Vec<PcSpanEntry>>(payloads, "PcSp")?;
    let docs = deserialize_optional::<Vec<DocEntry>>(payloads, "Docs")?.unwrap_or_default();
    let signatures =
        deserialize_optional::<Vec<SignatureEntry>>(payloads, "SigT")?.unwrap_or_default();
    let runtime_process_specs =
        deserialize_optional::<RuntimeProcessSpecTable>(payloads, "Proc")?.unwrap_or_default();
    let runtime_boot_plan =
        deserialize_optional::<RuntimeBootPlan>(payloads, "Boot")?.unwrap_or_default();

    Ok(Bytecode {
        opcodes,
        constants,
        num_locals: compile_info.num_locals,
        type_registry,
        error_templates,
        dbg_templates,
        callable_templates,
        functions,
        source_map: rebuild_source_map(&spans, &pc_spans),
        docs,
        signatures,
        compile_info,
        labels,
        imports,
        exports,
        literals,
        lines,
        spans,
        sources,
        pc_spans,
        runtime_process_specs,
        runtime_boot_plan,
    })
}

fn parse_container(bytes: &[u8]) -> Result<ParsedContainer<'_>, BytecodeFormatError> {
    if bytes.len() < Bytecode::HEADER_LEN {
        return Err(BytecodeFormatError::HeaderTooShort);
    }

    let magic_bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if magic_bytes != Bytecode::MAGIC {
        return Err(BytecodeFormatError::InvalidMagic(magic_bytes));
    }

    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version > Bytecode::VERSION {
        return Err(BytecodeFormatError::UnsupportedVersion(version));
    }

    let debug_level = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let num_chunks = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let mut table_offset = Bytecode::HEADER_LEN;
    let table_len = Bytecode::CHUNK_HEADER_LEN * num_chunks as usize;
    if table_offset + table_len > bytes.len() {
        return Err(BytecodeFormatError::TruncatedChunkHeader);
    }

    let mut raw_chunks = Vec::with_capacity(num_chunks as usize);
    for _ in 0..num_chunks as usize {
        if table_offset + Bytecode::CHUNK_HEADER_LEN > bytes.len() {
            return Err(BytecodeFormatError::TruncatedChunkHeader);
        }

        let tag_bytes = [
            bytes[table_offset],
            bytes[table_offset + 1],
            bytes[table_offset + 2],
            bytes[table_offset + 3],
        ];
        let size = u32::from_le_bytes([
            bytes[table_offset + 4],
            bytes[table_offset + 5],
            bytes[table_offset + 6],
            bytes[table_offset + 7],
        ]);
        raw_chunks.push((tag_bytes, size));
        table_offset += Bytecode::CHUNK_HEADER_LEN;
    }

    let mut payload_offset = Bytecode::HEADER_LEN + table_len;
    let mut payloads = std::collections::BTreeMap::new();
    let mut chunks = Vec::with_capacity(raw_chunks.len());

    for (tag_bytes, size) in raw_chunks {
        if payload_offset + size as usize > bytes.len() {
            return Err(BytecodeFormatError::TruncatedChunkData);
        }

        let chunk_payload_offset = payload_offset;
        let tag = String::from_utf8_lossy(&tag_bytes).to_string();
        if !is_known_chunk_tag(&tag) {
            return Err(BytecodeFormatError::UnknownChunkTag(tag));
        }
        if payloads.contains_key(&tag) {
            return Err(BytecodeFormatError::DuplicateChunkTag(tag));
        }
        payloads.insert(
            tag.clone(),
            &bytes[chunk_payload_offset..chunk_payload_offset + size as usize],
        );

        let padded_size = align4(size as usize);
        chunks.push(EldrChunkInfo {
            tag,
            size,
            payload_offset: chunk_payload_offset,
            padded_size,
        });

        payload_offset += padded_size;
    }

    for required in [
        "Code", "Cnst", "Func", "Type", "ErrT", "CInf", "LblT", "ImpT", "ExpT", "LitT", "Line",
        "SpnT", "SrcP", "PcSp",
    ] {
        if !payloads.contains_key(required) {
            return Err(BytecodeFormatError::MissingRequiredChunk(
                required.to_string(),
            ));
        }
    }
    let header = EldrHeader {
        magic: String::from_utf8_lossy(&magic_bytes).to_string(),
        version,
        debug_level,
        num_chunks,
    };
    Ok(ParsedContainer {
        header,
        chunks,
        payloads,
    })
}

fn serialize_chunk<T: Serialize>(value: &T) -> Result<Vec<u8>, BytecodeFormatError> {
    bincode::serialize(value).map_err(|e| BytecodeFormatError::EncodeFailed(e.to_string()))
}

fn deserialize_required<T: for<'de> Deserialize<'de>>(
    payloads: &std::collections::BTreeMap<String, &[u8]>,
    tag: &str,
) -> Result<T, BytecodeFormatError> {
    let payload = payloads
        .get(tag)
        .copied()
        .ok_or_else(|| BytecodeFormatError::MissingRequiredChunk(tag.to_string()))?;
    bincode::deserialize(payload).map_err(|e| BytecodeFormatError::DecodeFailed(e.to_string()))
}

fn deserialize_optional<T: for<'de> Deserialize<'de>>(
    payloads: &std::collections::BTreeMap<String, &[u8]>,
    tag: &str,
) -> Result<Option<T>, BytecodeFormatError> {
    match payloads.get(tag).copied() {
        Some(payload) => bincode::deserialize(payload)
            .map(Some)
            .map_err(|e| BytecodeFormatError::DecodeFailed(e.to_string())),
        None => Ok(None),
    }
}

fn is_known_chunk_tag(tag: &str) -> bool {
    matches!(
        tag,
        "Code"
            | "Cnst"
            | "Func"
            | "Type"
            | "ErrT"
            | "DbgT"
            | "CalT"
            | "CInf"
            | "LblT"
            | "ImpT"
            | "ExpT"
            | "LitT"
            | "Line"
            | "SpnT"
            | "SrcP"
            | "PcSp"
            | "Proc"
            | "Boot"
            | "Docs"
            | "SigT"
    )
}

fn populate_function_ranges(functions: &mut [FunctionEntry], opcode_len: u32) {
    let mut entries = functions.iter_mut().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.entry_pc);
    for idx in 0..entries.len() {
        let next_entry_pc = entries
            .get(idx + 1)
            .map(|entry| entry.entry_pc)
            .unwrap_or(opcode_len);
        entries[idx].end_pc = next_entry_pc;
    }
}

fn derive_labels(opcodes: &[Opcode], functions: &[FunctionEntry]) -> Vec<LabelEntry> {
    let mut pcs = std::collections::BTreeSet::new();
    for function in functions {
        pcs.insert(function.entry_pc);
    }
    for op in opcodes {
        match op {
            Opcode::Jump(pc) | Opcode::JumpIfFalse(pc) | Opcode::JumpIfTrue(pc) => {
                pcs.insert(*pc);
            }
            _ => {}
        }
    }
    pcs.into_iter()
        .map(|pc| LabelEntry {
            name: format!("L{}", pc),
            pc,
        })
        .collect()
}

fn derive_imports(opcodes: &[Opcode], functions: &[FunctionEntry]) -> Vec<ImportEntry> {
    let mut builtin_imports = std::collections::BTreeMap::<u16, ImportEntry>::new();
    let mut function_imports = std::collections::BTreeMap::<u32, ImportEntry>::new();

    for (pc, op) in opcodes.iter().enumerate() {
        match op {
            Opcode::CallBuiltin {
                builtin_id, arity, ..
            } => {
                let entry = builtin_imports
                    .entry(*builtin_id)
                    .or_insert_with(|| builtin_import_entry(*builtin_id, *arity));
                entry.call_pcs.push(pc as u32);
            }
            Opcode::LoadBuiltinRef(builtin_id) => {
                let arity = builtin_meta_by_id(*builtin_id)
                    .map(|meta| meta.arity)
                    .unwrap_or(0);
                let entry = builtin_imports
                    .entry(*builtin_id)
                    .or_insert_with(|| builtin_import_entry(*builtin_id, arity));
                entry.call_pcs.push(pc as u32);
            }
            Opcode::Call { fun_idx, .. } | Opcode::LoadFunctionRef(fun_idx) => {
                let fun_idx = *fun_idx;
                let arity = match op {
                    Opcode::Call { arity, .. } => *arity,
                    _ => functions
                        .iter()
                        .find(|entry| entry.fun_idx == fun_idx)
                        .map(|entry| entry.arity)
                        .unwrap_or(0),
                };
                let symbol = functions
                    .iter()
                    .find(|entry| entry.fun_idx == fun_idx)
                    .and_then(|entry| entry.qualified_name.clone())
                    .unwrap_or_else(|| format!("fun#{}", fun_idx));
                let entry = function_imports
                    .entry(fun_idx)
                    .or_insert_with(|| ImportEntry {
                        symbol,
                        kind: ImportKind::Function,
                        arity,
                        builtin_id: None,
                        function_id: Some(fun_idx),
                        call_pcs: Vec::new(),
                    });
                entry.call_pcs.push(pc as u32);
            }
            _ => {}
        }
    }

    builtin_imports
        .into_values()
        .chain(function_imports.into_values())
        .collect()
}

fn builtin_import_entry(builtin_id: u16, arity: u8) -> ImportEntry {
    ImportEntry {
        symbol: builtin_meta_by_id(builtin_id)
            .map(|meta| meta.name.to_string())
            .unwrap_or_else(|| format!("builtin#{}", builtin_id)),
        kind: ImportKind::Builtin,
        arity,
        builtin_id: Some(builtin_id),
        function_id: None,
        call_pcs: Vec::new(),
    }
}

fn derive_exports(functions: &[FunctionEntry], docs: &[DocEntry]) -> Vec<ExportEntry> {
    functions
        .iter()
        .filter_map(|entry| {
            let symbol = entry.qualified_name.clone()?;
            if entry.flags.generated {
                return None;
            }
            Some(ExportEntry {
                symbol: symbol.clone(),
                arity: entry.arity,
                function_id: entry.fun_idx,
                entry_pc: entry.entry_pc,
                doc_qualified_name: docs
                    .iter()
                    .find(|doc| doc.qualified_name == symbol)
                    .map(|doc| doc.qualified_name.clone()),
            })
        })
        .collect()
}

fn derive_literals(constants: &[Constant]) -> Vec<LiteralEntry> {
    constants
        .iter()
        .enumerate()
        .map(|(idx, constant)| {
            let (kind, display) = match constant {
                Constant::Int(value) => (LiteralKind::Int, value.to_string()),
                Constant::Tag(value) => (LiteralKind::Tag, value.to_string()),
                Constant::Float(value) => (LiteralKind::Float, value.to_string()),
                Constant::Str(value) => (LiteralKind::Str, value.clone()),
                Constant::Bool(value) => (LiteralKind::Bool, value.to_string()),
                Constant::Unit => (LiteralKind::Unit, "Unit".to_string()),
            };
            LiteralEntry {
                const_idx: idx as u32,
                kind,
                display,
            }
        })
        .collect()
}

fn derive_sources(source_map: Option<&SourceMap>) -> Vec<SourceFileEntry> {
    let mut names = std::collections::BTreeSet::new();
    if let Some(source_map) = source_map {
        for entry in &source_map.entries {
            if let Some(source_name) = &entry.source_name {
                names.insert(source_name.clone());
            }
        }
    }
    names
        .into_iter()
        .enumerate()
        .map(|(idx, path)| SourceFileEntry {
            source_id: idx as u32,
            normalized_path: Some(path.clone()),
            content_hash: None,
            text: None,
            path,
        })
        .collect()
}

fn derive_source_tables(
    source_map: Option<&SourceMap>,
) -> (Vec<SpanEntry>, Vec<PcSpanEntry>, Vec<LineEntry>) {
    let Some(source_map) = source_map else {
        return (Vec::new(), Vec::new(), Vec::new());
    };

    let mut span_ids = std::collections::BTreeMap::new();
    let mut spans = Vec::new();
    let mut pc_spans = Vec::new();
    let mut line_ranges = std::collections::BTreeMap::<(u32, Option<String>), (u32, u32)>::new();

    for entry in &source_map.entries {
        let key = (
            entry.span_start,
            entry.span_end,
            entry.line,
            entry.column,
            entry.source_name.clone(),
        );
        let span_id = match span_ids.get(&key) {
            Some(span_id) => *span_id,
            None => {
                let span_id = spans.len() as u32;
                span_ids.insert(key, span_id);
                spans.push(SpanEntry {
                    span_id,
                    span_start: entry.span_start,
                    span_end: entry.span_end,
                    line: entry.line,
                    column: entry.column,
                    source_name: entry.source_name.clone(),
                });
                span_id
            }
        };
        pc_spans.push(PcSpanEntry {
            pc: entry.opcode_index,
            span_id,
        });
        let range = line_ranges
            .entry((entry.line, entry.source_name.clone()))
            .or_insert((entry.opcode_index, entry.opcode_index + 1));
        range.0 = range.0.min(entry.opcode_index);
        range.1 = range.1.max(entry.opcode_index + 1);
    }

    let lines = line_ranges
        .into_iter()
        .map(|((line, source_name), (start_pc, end_pc))| LineEntry {
            line,
            start_pc,
            end_pc,
            source_name,
        })
        .collect();

    (spans, pc_spans, lines)
}

fn rebuild_source_map(spans: &[SpanEntry], pc_spans: &[PcSpanEntry]) -> Option<SourceMap> {
    if spans.is_empty() || pc_spans.is_empty() {
        return None;
    }
    let mut entries = Vec::new();
    for pc_span in pc_spans {
        if let Some(span) = spans.iter().find(|span| span.span_id == pc_span.span_id) {
            entries.push(OpcodeSource {
                opcode_index: pc_span.pc,
                span_start: span.span_start,
                span_end: span.span_end,
                line: span.line,
                column: span.column,
                source_name: span.source_name.clone(),
            });
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(SourceMap { entries })
    }
}

fn opcode_span(
    opcode: &Opcode,
    functions: &[FunctionEntry],
    error_templates: &[ErrTemplate],
    dbg_templates: &[DbgTemplate],
    opcode_index: u32,
) -> Option<(u32, u32)> {
    match opcode {
        Opcode::CallBuiltin {
            span_start,
            span_end,
            ..
        }
        | Opcode::Call {
            span_start,
            span_end,
            ..
        }
        | Opcode::CallClosure {
            span_start,
            span_end,
            ..
        }
        | Opcode::TailCallClosure {
            span_start,
            span_end,
            ..
        } => Some((*span_start, (*span_end).max(*span_start + 1))),
        Opcode::MakeError { template_id } => error_templates
            .iter()
            .find(|template| template.id == *template_id)
            .map(|template| {
                (
                    template.span_start,
                    template.span_end.max(template.span_start + 1),
                )
            }),
        Opcode::Dbg { template_id, .. } => dbg_templates
            .iter()
            .find(|template| template.id == *template_id)
            .map(|template| {
                (
                    template.span_start,
                    template.span_end.max(template.span_start + 1),
                )
            }),
        _ => functions
            .iter()
            .find(|entry| entry.entry_pc <= opcode_index && opcode_index < entry.end_pc)
            .and_then(|entry| {
                if entry.span_end > entry.span_start {
                    Some((entry.span_start, entry.span_end))
                } else {
                    None
                }
            }),
    }
}

fn align4(len: usize) -> usize {
    (len + 3) & !3
}

fn checked_payload_len(len: usize) -> Result<u32, BytecodeFormatError> {
    u32::try_from(len)
        .map_err(|_| BytecodeFormatError::EncodeFailed("payload too large".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        checked_payload_len, line_column_for_offset, populate_error_template_lines,
        stable_hash_hex, Bytecode, BytecodeFormatError, CallableTemplate, CallableTemplateArg,
        CallableTemplateComposeFlavor, CallableTemplateDirectTarget, CallableTemplateKind,
        CallableTemplateMetadata, CompileInfo, Constant, DocEntry, DocKind, ErrTemplate,
        FunctionEntry, FunctionFlags, Opcode, OpcodeSource, RuntimeBootPlan, RuntimeCallableRef,
        RuntimeInitPolicy, RuntimeInitResultShape, RuntimeInitSpec, RuntimeLifecycleSpec,
        RuntimeProcessDependencies, RuntimeProcessInstance, RuntimeProcessKind, RuntimeProcessSpec,
        RuntimeProcessSpecTable, RuntimeStateSpec, RuntimeSupervisionSpec, RuntimeTypeRef,
        SourceFileEntry, SourceMap,
    };
    use crate::primitives::int;
    use crate::runtime::{TypeEntry, TypeKind, TypeRegistry};

    fn sample_registry() -> TypeRegistry {
        let mut registry = TypeRegistry::new();
        registry.register(TypeEntry {
            tag: 10,
            name: "User".to_string(),
            kind: TypeKind::Struct,
            field_names: vec!["name".to_string(), "age".to_string()],
            private_flags: vec![false, false],
        });
        registry
    }

    fn sample_process_spec() -> RuntimeProcessSpec {
        RuntimeProcessSpec {
            process_id: 0,
            type_name: "Counter".to_string(),
            kind: RuntimeProcessKind::Agent,
            instance: RuntimeProcessInstance::Singleton,
            state: RuntimeStateSpec {
                state_type: RuntimeTypeRef {
                    name: "Int".to_string(),
                },
            },
            init: RuntimeInitSpec {
                callable: RuntimeCallableRef { fun_idx: 0 },
                policy: RuntimeInitPolicy::Eager,
                result_shape: RuntimeInitResultShape::EagerState {
                    result_type: RuntimeTypeRef {
                        name: "Result<Int, Error>".to_string(),
                    },
                },
                state_type: RuntimeTypeRef {
                    name: "Int".to_string(),
                },
                init_route: None,
            },
            handlers: Vec::new(),
            dependencies: RuntimeProcessDependencies::default(),
            lifecycle: RuntimeLifecycleSpec::default(),
            supervision: RuntimeSupervisionSpec::default(),
        }
    }

    fn sample_bytecode(source_map: Option<SourceMap>) -> Bytecode {
        let mut bytecode = Bytecode {
            opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
            constants: vec![Constant::Int(int(42))],
            num_locals: 1,
            type_registry: sample_registry(),
            error_templates: vec![ErrTemplate {
                id: 1,
                kind: "ValidationError".to_string(),
                span_start: 3,
                span_end: 8,
                line: 1,
                column: 4,
                format: "bad".to_string(),
                num_params: 0,
            }],
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 1,
                num_locals: 0,
                arity: 0,
                qualified_name: Some("Main::entry".to_string()),
                signature: Some("entry() -> Unit".to_string()),
                end_pc: 2,
                span_start: 0,
                span_end: 4,
                flags: FunctionFlags {
                    public: true,
                    closure: false,
                    partial_apply_wrapper: false,
                    builtin_wrapper: false,
                    tail_entry: false,
                    generated: false,
                },
            }],
            source_map,
            docs: vec![DocEntry {
                qualified_name: "Bootstrap::Int".to_string(),
                kind: DocKind::Type,
                module_path: "Bootstrap".to_string(),
                signature: Some("type Int".to_string()),
                doc: "Builtin Int type.".to_string(),
            }],
            signatures: Vec::new(),
            compile_info: CompileInfo {
                num_locals: 1,
                source_hash: Some(stable_hash_hex("let x = 42")),
                module_hash: Some(stable_hash_hex("Main")),
                ..CompileInfo::default()
            },
            labels: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            literals: Vec::new(),
            lines: Vec::new(),
            spans: Vec::new(),
            sources: vec![SourceFileEntry {
                source_id: 0,
                path: "main.srt".to_string(),
                normalized_path: Some("main.srt".to_string()),
                content_hash: Some(stable_hash_hex("let x = 42")),
                text: Some("let x = 42".to_string()),
            }],
            pc_spans: Vec::new(),
            runtime_process_specs: RuntimeProcessSpecTable {
                entries: vec![sample_process_spec()],
            },
            runtime_boot_plan: RuntimeBootPlan::explicit_singleton("Counter"),
        };
        bytecode.refresh_viewer_metadata();
        bytecode
    }

    #[test]
    fn roundtrip_encode_decode_with_source_map_none() {
        let bytecode = sample_bytecode(None);
        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, bytecode);
    }

    #[test]
    fn roundtrip_encode_decode_store_const_local_opcode() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![
            Opcode::StoreConstLocal {
                const_idx: 0,
                local_idx: 0,
            },
            Opcode::Halt,
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_copy_local_opcode() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![
            Opcode::CopyLocal {
                src_local_idx: 0,
                dst_local_idx: 1,
            },
            Opcode::Halt,
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_eq_local_tag_opcode() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![
            Opcode::EqLocalTag {
                local_idx: 0,
                tag_const_idx: 0,
            },
            Opcode::Halt,
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_result_branch_opcode_batch() {
        let mut bytecode = sample_bytecode(None);
        bytecode.constants = vec![Constant::Tag(0), Constant::Tag(1)];
        bytecode.opcodes = vec![
            Opcode::JumpIfLocalTagEq {
                local_idx: 0,
                tag_const_idx: 0,
                target_pc: 3,
            },
            Opcode::JumpIfLocalTagNe {
                local_idx: 0,
                tag_const_idx: 1,
                target_pc: 4,
            },
            Opcode::Halt,
            Opcode::Halt,
            Opcode::Halt,
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_tail_call_closure_opcode() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![
            Opcode::Halt,
            Opcode::TailCallClosure {
                arity: 1,
                span_start: 2,
                span_end: 8,
            },
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
        assert_eq!(decoded.opcodes[1].kind_name(), "TailCallClosure");
    }

    #[test]
    fn roundtrip_encode_decode_callable_templates() {
        let mut bytecode = sample_bytecode(None);
        bytecode.callable_templates = vec![
            CallableTemplate {
                template_id: 0,
                kind: CallableTemplateKind::PartialDirectCall {
                    target: CallableTemplateDirectTarget::Function(7),
                    arg_sources: vec![
                        CallableTemplateArg::Runtime(0),
                        CallableTemplateArg::Bound(0),
                    ],
                },
                metadata: CallableTemplateMetadata::default(),
            },
            CallableTemplate {
                template_id: 1,
                kind: CallableTemplateKind::InjectDirectCall {
                    target: CallableTemplateDirectTarget::Builtin(2),
                    bound_arg_count: 1,
                },
                metadata: CallableTemplateMetadata::default(),
            },
            CallableTemplate {
                template_id: 2,
                kind: CallableTemplateKind::ComposeDirect {
                    flavor: CallableTemplateComposeFlavor::ResultBind,
                },
                metadata: CallableTemplateMetadata::default(),
            },
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.callable_templates, bytecode.callable_templates);
    }

    #[test]
    fn roundtrip_encode_decode_result_and_local_tag_jump_opcodes() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![
            Opcode::MakeOk,
            Opcode::MakeErr,
            Opcode::JumpIfLocalTagEq {
                local_idx: 1,
                tag_const_idx: 0,
                target_pc: 4,
            },
            Opcode::JumpIfLocalTagNe {
                local_idx: 2,
                tag_const_idx: 1,
                target_pc: 5,
            },
            Opcode::Halt,
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_string_len_opcode() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![Opcode::StringLen, Opcode::Halt];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_list_len_opcode() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![Opcode::ListLen, Opcode::Halt];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_safe_mod_int_opcode() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![Opcode::SafeModInt, Opcode::Halt];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_string_predicate_opcodes() {
        let mut bytecode = sample_bytecode(None);
        bytecode.opcodes = vec![
            Opcode::StringContains,
            Opcode::StringStartsWith,
            Opcode::StringEndsWith,
            Opcode::Halt,
        ];

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(decoded.opcodes, bytecode.opcodes);
    }

    #[test]
    fn roundtrip_encode_decode_with_source_map_some() {
        let bytecode = sample_bytecode(Some(SourceMap {
            entries: vec![OpcodeSource {
                opcode_index: 0,
                span_start: 0,
                span_end: 4,
                line: 1,
                column: 1,
                source_name: None,
            }],
        }));
        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, bytecode);
    }

    #[test]
    fn roundtrip_rebuilds_type_registry_lookup_index() {
        let mut bytecode = sample_bytecode(None);
        bytecode.type_registry = TypeRegistry::from_entries(vec![
            TypeEntry {
                tag: 10,
                name: "Global::User".to_string(),
                kind: TypeKind::Struct,
                field_names: vec!["name".to_string(), "age".to_string()],
                private_flags: vec![false, false],
            },
            TypeEntry {
                tag: 42,
                name: "Global::Profile".to_string(),
                kind: TypeKind::Record,
                field_names: vec!["id".to_string()],
                private_flags: vec![false],
            },
        ]);

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");

        assert_eq!(
            decoded.type_registry.entries(),
            bytecode.type_registry.entries()
        );
        assert_eq!(
            decoded
                .type_registry
                .lookup(42)
                .map(|entry| entry.name.as_str()),
            Some("Global::Profile")
        );
    }

    #[test]
    fn process_spec_roundtrip_uses_v2_shape_without_legacy_boot_registry_lazy_fields() {
        let mut bytecode = sample_bytecode(None);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![RuntimeProcessSpec {
                process_id: 7,
                type_name: "LazyCache".to_string(),
                kind: RuntimeProcessKind::GenServer,
                instance: RuntimeProcessInstance::Singleton,
                state: RuntimeStateSpec {
                    state_type: RuntimeTypeRef {
                        name: "Int".to_string(),
                    },
                },
                init: RuntimeInitSpec {
                    callable: RuntimeCallableRef { fun_idx: 0 },
                    policy: RuntimeInitPolicy::Lazy,
                    result_shape: RuntimeInitResultShape::LazyProcessInit {
                        result_type: RuntimeTypeRef {
                            name: "Result<ProcessInit<Int>>".to_string(),
                        },
                    },
                    state_type: RuntimeTypeRef {
                        name: "Int".to_string(),
                    },
                    init_route: None,
                },
                handlers: Vec::new(),
                dependencies: RuntimeProcessDependencies::default(),
                lifecycle: RuntimeLifecycleSpec::default(),
                supervision: RuntimeSupervisionSpec::default(),
            }],
        };
        bytecode.refresh_viewer_metadata();

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");
        assert_eq!(
            decoded.runtime_process_specs,
            bytecode.runtime_process_specs
        );
    }

    #[test]
    fn decode_rejects_invalid_magic() {
        let bytes = b"BAD!\x01\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::InvalidMagic(_)));
    }

    #[test]
    fn decode_rejects_missing_code_chunk() {
        let bytes = b"ELDR\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::MissingRequiredChunk(_)));
    }

    #[test]
    fn decode_rejects_unsupported_version() {
        let bytes = b"ELDR\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::UnsupportedVersion(3)));
    }

    #[test]
    fn decode_accepts_previous_version_header() {
        let bytecode = sample_bytecode(None);
        let mut bytes = bytecode.encode().expect("encode should succeed");
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());

        let decoded = Bytecode::decode(&bytes).expect("decode should accept version 1");
        assert_eq!(decoded.callable_templates, bytecode.callable_templates);
        assert_eq!(decoded.compile_info.bytecode_version, 2);
    }

    #[test]
    fn decode_rejects_truncated_chunk_header() {
        let bytes = b"ELDR\x01\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00Code";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::TruncatedChunkHeader));
    }

    #[test]
    fn decode_rejects_truncated_chunk_data() {
        let bytes = b"ELDR\x01\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00Code\x04\x00\x00\x00\x01";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::TruncatedChunkData));
    }

    #[test]
    fn inspect_reports_header_and_chunk_layout() {
        let bytecode = sample_bytecode(None);
        let bytes = bytecode.encode().expect("encode should succeed");
        let inspected = Bytecode::inspect(&bytes).expect("inspect should succeed");
        assert_eq!(inspected.header.magic, "ELDR");
        assert_eq!(inspected.header.version, 2);
        assert!(inspected.chunks.len() >= 15);
        assert_eq!(inspected.chunks[0].tag, "Code");
        assert!(inspected.chunks.iter().any(|chunk| chunk.tag == "Proc"));
        assert!(inspected.chunks.iter().any(|chunk| chunk.tag == "Boot"));
        assert!(inspected.chunks.iter().any(|chunk| chunk.tag == "CalT"));
        assert!(inspected.chunks[0].payload_offset >= 16);
        assert!(inspected.chunks[0].padded_size >= inspected.chunks[0].size as usize);
    }

    #[test]
    fn line_column_for_offset_tracks_multiline_source() {
        let source = "deferror Boom {\n  \"boom\"\n}\n";
        assert_eq!(line_column_for_offset(source, 0), (1, 1));
        assert_eq!(line_column_for_offset(source, 16), (2, 1));
    }

    #[test]
    fn line_column_for_offset_tracks_utf8_columns() {
        let source = "あい\nうえお";
        assert_eq!(line_column_for_offset(source, 1), (1, 2));
        assert_eq!(line_column_for_offset(source, 4), (2, 2));
    }

    #[test]
    fn populate_error_template_lines_uses_span_start() {
        let source = "deferror Boom {\n  \"boom\"\n}\n";
        let mut templates = vec![ErrTemplate {
            id: 0,
            kind: "Boom".into(),
            span_start: 16,
            span_end: 24,
            line: 0,
            column: 0,
            format: "{}".into(),
            num_params: 1,
        }];

        populate_error_template_lines(&mut templates, source);

        assert_eq!(templates[0].line, 2);
        assert_eq!(templates[0].column, 1);
    }

    #[test]
    fn checked_payload_len_rejects_values_larger_than_u32() {
        let err = checked_payload_len(usize::MAX).expect_err("payload len must be rejected");
        assert!(matches!(err, BytecodeFormatError::EncodeFailed(_)));
    }
}
