use std::collections::HashMap;

#[cfg(feature = "viewer-schema")]
use schemars::{schema::RootSchema, schema_for, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::builtin::builtin_meta_by_id;
use crate::ir::{
    Bytecode, Constant, EldrInspect, ErrTemplate, FunctionEntry, Opcode, OpcodeSource,
    RuntimeProcessInstance, RuntimeProcessKind, RuntimeProcessSpec, SourceFileEntry,
};

pub const VIEWER_SCHEMA_VERSION: u32 = 1;
pub const VIEWER_FORMAT: &str = "eldr_viewer";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct ViewerFile {
    pub schema_version: u32,
    pub format: String,
    pub header: ViewerHeader,
    pub chunks: Vec<ChunkView>,
    pub process_specs: Vec<ProcessSpecView>,
    pub functions: Vec<FunctionView>,
    pub constants: Vec<ConstantView>,
    pub opcodes: Vec<OpcodeRowView>,
    pub sources: Vec<SourceFileView>,
    pub errors: Vec<ErrorTemplateView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct ViewerHeader {
    pub magic: String,
    pub version: u32,
    pub debug_level: u32,
    pub num_chunks: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct ChunkView {
    pub chunk_id: String,
    pub tag: String,
    pub size: u32,
    pub payload_offset: u32,
    pub padded_size: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct FunctionView {
    pub function_id: String,
    pub fun_idx: u32,
    pub name: Option<String>,
    pub entry_pc: u32,
    pub end_pc: Option<u32>,
    pub arity: u8,
    pub num_locals: u32,
    pub chunk_id: String,
    pub source_ref: Option<SourceRefView>,
    pub opcode_pcs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
#[serde(tag = "kind")]
pub enum ConstantView {
    Int { idx: u32, value: String },
    Tag { idx: u32, value: u32 },
    Float { idx: u32, value: f64 },
    Str { idx: u32, value: String },
    Bool { idx: u32, value: bool },
    Unit { idx: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct OpcodeRowView {
    pub pc: u32,
    pub function_id: Option<String>,
    pub op: OpcodeView,
    pub source_ref: Option<SourceRefView>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
#[serde(tag = "kind")]
pub enum OpcodeView {
    LoadConst {
        const_idx: u32,
    },
    LoadBuiltinRef {
        builtin_id: u16,
        builtin: String,
    },
    LoadFunctionRef {
        fun_idx: u32,
    },
    LoadLocal {
        local_idx: u32,
    },
    StoreLocal {
        local_idx: u32,
    },
    StoreConstLocal {
        const_idx: u32,
        local_idx: u32,
    },
    CopyLocal {
        src_local_idx: u32,
        dst_local_idx: u32,
    },
    AddInt,
    SubInt,
    MulInt,
    BitNotInt,
    BitAndInt,
    BitOrInt,
    BitXorInt,
    AddFloat,
    SubFloat,
    MulFloat,
    EqInt,
    NeqInt,
    LtInt,
    GtInt,
    LteInt,
    GteInt,
    EqFloat,
    NeqFloat,
    LtFloat,
    GtFloat,
    LteFloat,
    GteFloat,
    EqStr,
    NeqStr,
    EqBool,
    NeqBool,
    ConcatStr,
    StringIsEmpty,
    StringHead,
    StringTail,
    NegInt,
    NegFloat,
    NotBool,
    ListNew {
        len: u32,
    },
    ListEmpty,
    ListNil,
    ListCons,
    ListIsEmpty,
    ListHead,
    ListTail,
    ListFromItems {
        len: u32,
    },
    TupleNew {
        len: u32,
    },
    GetTupleField {
        field_index: u32,
    },
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
    CallBuiltin {
        builtin_id: u16,
        builtin: String,
        arity: u8,
        span_start: u32,
        span_end: u32,
    },
    Call {
        fun_idx: u32,
        arity: u8,
        span_start: u32,
        span_end: u32,
    },
    CaptureClosure {
        capture_count: u8,
    },
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
    Jump {
        target_pc: u32,
    },
    JumpIfFalse {
        target_pc: u32,
    },
    JumpIfTrue {
        target_pc: u32,
    },
    Pop,
    Return,
    Halt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct SourceFileView {
    pub source_id: String,
    pub name: Option<String>,
    pub normalized_path: Option<String>,
    pub content_hash: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct SourceRefView {
    pub source_id: String,
    pub span_start: u32,
    pub span_end: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct ErrorTemplateView {
    pub template_id: u32,
    pub kind: String,
    pub format: String,
    pub num_params: u8,
    pub source_ref: Option<SourceRefView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub struct ProcessSpecView {
    pub process_id: u32,
    pub type_name: String,
    pub kind: ProcessSpecKindView,
    pub instance: ProcessSpecInstanceView,
    pub init_fun_idx: u32,
    pub init_policy: String,
    pub state_type: String,
    pub handler_count: usize,
    pub dependency_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub enum ProcessSpecKindView {
    Agent,
    GenServer,
    Supervisor,
    RuntimeSupervisor,
    DynamicSupervisor,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "viewer-schema", derive(JsonSchema))]
pub enum ProcessSpecInstanceView {
    Singleton,
    Worker,
}

#[cfg(feature = "viewer-schema")]
pub fn viewer_schema() -> RootSchema {
    schema_for!(ViewerFile)
}

pub fn viewer_file_from_inspect(inspected: &EldrInspect) -> ViewerFile {
    let source_lookup = source_lookup(&inspected.bytecode.sources);
    let function_ranges = inspected
        .bytecode
        .functions
        .iter()
        .map(|entry| function_range(entry))
        .collect::<Vec<_>>();
    let label_lookup = inspected
        .bytecode
        .labels
        .iter()
        .map(|label| (label.pc, label.name.clone()))
        .collect::<HashMap<_, _>>();

    ViewerFile {
        schema_version: VIEWER_SCHEMA_VERSION,
        format: VIEWER_FORMAT.to_string(),
        header: ViewerHeader {
            magic: inspected.header.magic.clone(),
            version: inspected.header.version,
            debug_level: inspected.header.debug_level,
            num_chunks: inspected.header.num_chunks,
        },
        chunks: inspected
            .chunks
            .iter()
            .enumerate()
            .map(|(idx, chunk)| ChunkView {
                chunk_id: format!("chunk:{idx}"),
                tag: chunk.tag.clone(),
                size: chunk.size,
                payload_offset: chunk.payload_offset as u32,
                padded_size: chunk.padded_size as u32,
            })
            .collect(),
        process_specs: inspected
            .bytecode
            .runtime_process_specs
            .entries
            .iter()
            .map(process_spec_view)
            .collect(),
        functions: inspected
            .bytecode
            .functions
            .iter()
            .map(|entry| function_view(entry, &inspected.bytecode, &source_lookup))
            .collect(),
        constants: inspected
            .bytecode
            .constants
            .iter()
            .enumerate()
            .map(|(idx, constant)| constant_view(idx as u32, constant))
            .collect(),
        opcodes: inspected
            .bytecode
            .opcodes
            .iter()
            .enumerate()
            .map(|(pc, opcode)| OpcodeRowView {
                pc: pc as u32,
                function_id: function_ranges
                    .iter()
                    .find(|(start, end, _)| (*start..=*end).contains(&(pc as u32)))
                    .map(|(_, _, function_id)| function_id.clone()),
                op: opcode_view(opcode),
                source_ref: inspected
                    .bytecode
                    .source_map
                    .as_ref()
                    .and_then(|source_map| {
                        source_map
                            .entries
                            .iter()
                            .find(|entry| entry.opcode_index == pc as u32)
                    })
                    .map(|entry| source_ref_view(entry, &source_lookup)),
                label: label_lookup.get(&(pc as u32)).cloned(),
            })
            .collect(),
        sources: inspected
            .bytecode
            .sources
            .iter()
            .map(source_file_view)
            .collect(),
        errors: inspected
            .bytecode
            .error_templates
            .iter()
            .map(|template| error_template_view(template, &source_lookup))
            .collect(),
    }
}

fn function_view(
    entry: &FunctionEntry,
    bytecode: &Bytecode,
    source_lookup: &HashMap<String, String>,
) -> FunctionView {
    let function_id = function_id(entry.fun_idx);
    let end_pc = if entry.end_pc == 0 {
        None
    } else {
        Some(entry.end_pc)
    };
    let opcode_pcs = end_pc
        .map(|end| (entry.entry_pc..=end).collect())
        .unwrap_or_default();
    let source_ref = bytecode
        .source_map
        .as_ref()
        .and_then(|source_map| {
            source_map
                .entries
                .iter()
                .find(|source| source.opcode_index == entry.entry_pc)
        })
        .map(|source| source_ref_view(source, source_lookup))
        .or_else(|| {
            if entry.span_end > entry.span_start {
                Some(SourceRefView {
                    source_id: first_source_id(&bytecode.sources),
                    span_start: entry.span_start,
                    span_end: entry.span_end,
                    line: 0,
                    column: 0,
                })
            } else {
                None
            }
        });

    FunctionView {
        function_id,
        fun_idx: entry.fun_idx,
        name: entry.qualified_name.clone(),
        entry_pc: entry.entry_pc,
        end_pc,
        arity: entry.arity,
        num_locals: entry.num_locals,
        chunk_id: "Func".to_string(),
        source_ref,
        opcode_pcs,
    }
}

fn constant_view(idx: u32, constant: &Constant) -> ConstantView {
    match constant {
        Constant::Int(value) => ConstantView::Int {
            idx,
            value: value.to_string(),
        },
        Constant::Tag(value) => ConstantView::Tag { idx, value: *value },
        Constant::Float(value) => ConstantView::Float { idx, value: *value },
        Constant::Str(value) => ConstantView::Str {
            idx,
            value: value.clone(),
        },
        Constant::Bool(value) => ConstantView::Bool { idx, value: *value },
        Constant::Unit => ConstantView::Unit { idx },
    }
}

fn opcode_view(opcode: &Opcode) -> OpcodeView {
    match opcode {
        Opcode::LoadConst(idx) => OpcodeView::LoadConst { const_idx: *idx },
        Opcode::LoadBuiltinRef(id) => OpcodeView::LoadBuiltinRef {
            builtin_id: *id,
            builtin: builtin_meta_by_id(*id)
                .map(|meta| meta.name.to_string())
                .unwrap_or_else(|| format!("builtin#{id}")),
        },
        Opcode::LoadFunctionRef(fun_idx) => OpcodeView::LoadFunctionRef { fun_idx: *fun_idx },
        Opcode::LoadLocal(local_idx) => OpcodeView::LoadLocal {
            local_idx: *local_idx,
        },
        Opcode::StoreLocal(local_idx) => OpcodeView::StoreLocal {
            local_idx: *local_idx,
        },
        Opcode::StoreConstLocal {
            const_idx,
            local_idx,
        } => OpcodeView::StoreConstLocal {
            const_idx: *const_idx,
            local_idx: *local_idx,
        },
        Opcode::CopyLocal {
            src_local_idx,
            dst_local_idx,
        } => OpcodeView::CopyLocal {
            src_local_idx: *src_local_idx,
            dst_local_idx: *dst_local_idx,
        },
        Opcode::AddInt => OpcodeView::AddInt,
        Opcode::SubInt => OpcodeView::SubInt,
        Opcode::MulInt => OpcodeView::MulInt,
        Opcode::BitNotInt => OpcodeView::BitNotInt,
        Opcode::BitAndInt => OpcodeView::BitAndInt,
        Opcode::BitOrInt => OpcodeView::BitOrInt,
        Opcode::BitXorInt => OpcodeView::BitXorInt,
        Opcode::AddFloat => OpcodeView::AddFloat,
        Opcode::SubFloat => OpcodeView::SubFloat,
        Opcode::MulFloat => OpcodeView::MulFloat,
        Opcode::EqInt => OpcodeView::EqInt,
        Opcode::NeqInt => OpcodeView::NeqInt,
        Opcode::LtInt => OpcodeView::LtInt,
        Opcode::GtInt => OpcodeView::GtInt,
        Opcode::LteInt => OpcodeView::LteInt,
        Opcode::GteInt => OpcodeView::GteInt,
        Opcode::EqFloat => OpcodeView::EqFloat,
        Opcode::NeqFloat => OpcodeView::NeqFloat,
        Opcode::LtFloat => OpcodeView::LtFloat,
        Opcode::GtFloat => OpcodeView::GtFloat,
        Opcode::LteFloat => OpcodeView::LteFloat,
        Opcode::GteFloat => OpcodeView::GteFloat,
        Opcode::EqStr => OpcodeView::EqStr,
        Opcode::NeqStr => OpcodeView::NeqStr,
        Opcode::EqBool => OpcodeView::EqBool,
        Opcode::NeqBool => OpcodeView::NeqBool,
        Opcode::ConcatStr => OpcodeView::ConcatStr,
        Opcode::StringIsEmpty => OpcodeView::StringIsEmpty,
        Opcode::StringHead => OpcodeView::StringHead,
        Opcode::StringTail => OpcodeView::StringTail,
        Opcode::NegInt => OpcodeView::NegInt,
        Opcode::NegFloat => OpcodeView::NegFloat,
        Opcode::NotBool => OpcodeView::NotBool,
        Opcode::ListNew { len } => OpcodeView::ListNew { len: *len },
        Opcode::ListEmpty => OpcodeView::ListEmpty,
        Opcode::ListNil => OpcodeView::ListNil,
        Opcode::ListCons => OpcodeView::ListCons,
        Opcode::ListIsEmpty => OpcodeView::ListIsEmpty,
        Opcode::ListHead => OpcodeView::ListHead,
        Opcode::ListTail => OpcodeView::ListTail,
        Opcode::ListFromItems { len } => OpcodeView::ListFromItems { len: *len },
        Opcode::TupleNew { len } => OpcodeView::TupleNew { len: *len },
        Opcode::GetTupleField { field_index } => OpcodeView::GetTupleField {
            field_index: *field_index,
        },
        Opcode::StructNew { field_count } => OpcodeView::StructNew {
            field_count: *field_count,
        },
        Opcode::GetField { field_index } => OpcodeView::GetField {
            field_index: *field_index,
        },
        Opcode::GetTag => OpcodeView::GetTag,
        Opcode::EqTag => OpcodeView::EqTag,
        Opcode::Dbg {
            template_id,
            arg_count,
        } => OpcodeView::Dbg {
            template_id: *template_id,
            arg_count: *arg_count,
        },
        Opcode::CallBuiltin {
            builtin_id,
            arity,
            span_start,
            span_end,
        } => OpcodeView::CallBuiltin {
            builtin_id: *builtin_id,
            builtin: builtin_meta_by_id(*builtin_id)
                .map(|meta| meta.name.to_string())
                .unwrap_or_else(|| format!("builtin#{builtin_id}")),
            arity: *arity,
            span_start: *span_start,
            span_end: *span_end,
        },
        Opcode::Call {
            fun_idx,
            arity,
            span_start,
            span_end,
        } => OpcodeView::Call {
            fun_idx: *fun_idx,
            arity: *arity,
            span_start: *span_start,
            span_end: *span_end,
        },
        Opcode::CaptureClosure(count) => OpcodeView::CaptureClosure {
            capture_count: *count,
        },
        Opcode::MakeError { template_id } => OpcodeView::MakeError {
            template_id: *template_id,
        },
        Opcode::MakeErrorLiteral {
            kind_const_idx,
            message_const_idx,
        } => OpcodeView::MakeErrorLiteral {
            kind_const_idx: *kind_const_idx,
            message_const_idx: *message_const_idx,
        },
        Opcode::CallClosure {
            arity,
            span_start,
            span_end,
        } => OpcodeView::CallClosure {
            arity: *arity,
            span_start: *span_start,
            span_end: *span_end,
        },
        Opcode::Jump(target_pc) => OpcodeView::Jump {
            target_pc: *target_pc,
        },
        Opcode::JumpIfFalse(target_pc) => OpcodeView::JumpIfFalse {
            target_pc: *target_pc,
        },
        Opcode::JumpIfTrue(target_pc) => OpcodeView::JumpIfTrue {
            target_pc: *target_pc,
        },
        Opcode::Pop => OpcodeView::Pop,
        Opcode::Return => OpcodeView::Return,
        Opcode::Halt => OpcodeView::Halt,
    }
}

fn source_file_view(source: &SourceFileEntry) -> SourceFileView {
    SourceFileView {
        source_id: source.source_id.to_string(),
        name: Some(source.path.clone()),
        normalized_path: source.normalized_path.clone(),
        content_hash: source.content_hash.clone(),
        text: source.text.clone(),
    }
}

fn error_template_view(
    template: &ErrTemplate,
    source_lookup: &HashMap<String, String>,
) -> ErrorTemplateView {
    ErrorTemplateView {
        template_id: template.id,
        kind: template.kind.clone(),
        format: template.format.clone(),
        num_params: template.num_params,
        source_ref: Some(SourceRefView {
            source_id: source_lookup
                .values()
                .next()
                .cloned()
                .unwrap_or_else(|| "0".to_string()),
            span_start: template.span_start,
            span_end: template.span_end,
            line: template.line,
            column: template.column,
        }),
    }
}

fn process_spec_view(spec: &RuntimeProcessSpec) -> ProcessSpecView {
    ProcessSpecView {
        process_id: spec.process_id,
        type_name: spec.type_name.clone(),
        kind: match spec.kind {
            RuntimeProcessKind::Agent => ProcessSpecKindView::Agent,
            RuntimeProcessKind::GenServer => ProcessSpecKindView::GenServer,
            RuntimeProcessKind::Supervisor => ProcessSpecKindView::Supervisor,
            RuntimeProcessKind::RuntimeSupervisor => ProcessSpecKindView::RuntimeSupervisor,
            RuntimeProcessKind::DynamicSupervisor => ProcessSpecKindView::DynamicSupervisor,
            RuntimeProcessKind::Task => ProcessSpecKindView::Task,
        },
        instance: match spec.instance {
            RuntimeProcessInstance::Singleton => ProcessSpecInstanceView::Singleton,
            RuntimeProcessInstance::Worker => ProcessSpecInstanceView::Worker,
        },
        init_fun_idx: spec.init.callable.fun_idx,
        init_policy: format!("{:?}", spec.init.policy),
        state_type: spec.state.state_type.name.clone(),
        handler_count: spec.handlers.len(),
        dependency_count: spec.dependencies.handlers.len(),
    }
}

fn source_lookup(sources: &[SourceFileEntry]) -> HashMap<String, String> {
    let mut lookup = HashMap::new();
    for source in sources {
        lookup.insert(source.path.clone(), source.source_id.to_string());
        if let Some(normalized) = &source.normalized_path {
            lookup.insert(normalized.clone(), source.source_id.to_string());
        }
    }
    lookup
}

fn source_ref_view(
    source: &OpcodeSource,
    source_lookup: &HashMap<String, String>,
) -> SourceRefView {
    let source_id = source
        .source_name
        .as_ref()
        .and_then(|name| source_lookup.get(name))
        .cloned()
        .unwrap_or_else(|| "0".to_string());
    SourceRefView {
        source_id,
        span_start: source.span_start,
        span_end: source.span_end,
        line: source.line,
        column: source.column,
    }
}

fn function_id(fun_idx: u32) -> String {
    format!("fn:{fun_idx}")
}

fn function_range(entry: &FunctionEntry) -> (u32, u32, String) {
    (
        entry.entry_pc,
        entry.end_pc.max(entry.entry_pc),
        function_id(entry.fun_idx),
    )
}

fn first_source_id(sources: &[SourceFileEntry]) -> String {
    sources
        .first()
        .map(|entry| entry.source_id.to_string())
        .unwrap_or_else(|| "0".to_string())
}

#[cfg(test)]
mod tests {
    use crate::builtin::builtin_id_by_name;
    use crate::ir::{
        Bytecode, CompileInfo, Constant, DocEntry, DocKind, EldrChunkInfo, EldrHeader, EldrInspect,
        ErrTemplate, FunctionEntry, FunctionFlags, LabelEntry, Opcode, OpcodeSource,
        RuntimeProcessInstance, RuntimeProcessKind, RuntimeProcessSpec, RuntimeProcessSpecTable,
        SourceFileEntry, SourceMap,
    };
    use crate::primitives::int;

    #[cfg(feature = "viewer-schema")]
    use super::viewer_schema;
    use super::{
        viewer_file_from_inspect, ProcessSpecInstanceView, ProcessSpecKindView, VIEWER_FORMAT,
        VIEWER_SCHEMA_VERSION,
    };

    #[test]
    fn viewer_model_contains_core_sections() {
        let print_id = builtin_id_by_name("print").expect("print builtin must exist");
        let bytecode = Bytecode {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::LoadBuiltinRef(print_id),
                Opcode::CallBuiltin {
                    builtin_id: print_id,
                    arity: 1,
                    span_start: 0,
                    span_end: 5,
                },
                Opcode::Halt,
            ],
            constants: vec![Constant::Int(int(42))],
            num_locals: 1,
            type_registry: Default::default(),
            error_templates: vec![ErrTemplate {
                id: 0,
                kind: "SampleError".into(),
                span_start: 0,
                span_end: 5,
                line: 1,
                column: 1,
                format: "sample".into(),
                num_params: 0,
            }],
            dbg_templates: Vec::new(),
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 0,
                num_locals: 1,
                arity: 0,
                qualified_name: Some("Main::entry".into()),
                signature: Some("entry() -> Unit".into()),
                end_pc: 3,
                span_start: 0,
                span_end: 5,
                flags: FunctionFlags::default(),
            }],
            source_map: Some(SourceMap {
                entries: vec![
                    OpcodeSource {
                        opcode_index: 0,
                        span_start: 0,
                        span_end: 2,
                        line: 1,
                        column: 1,
                        source_name: Some("sample.srt".into()),
                    },
                    OpcodeSource {
                        opcode_index: 1,
                        span_start: 0,
                        span_end: 5,
                        line: 1,
                        column: 1,
                        source_name: Some("sample.srt".into()),
                    },
                ],
            }),
            docs: vec![DocEntry {
                qualified_name: "Main::entry".into(),
                kind: DocKind::Function,
                module_path: "Main".into(),
                signature: Some("def entry() -> Unit".into()),
                doc: "sample".into(),
            }],
            compile_info: CompileInfo::default(),
            labels: vec![LabelEntry {
                name: "entry".into(),
                pc: 0,
            }],
            imports: Vec::new(),
            exports: Vec::new(),
            literals: Vec::new(),
            lines: Vec::new(),
            spans: Vec::new(),
            sources: vec![SourceFileEntry {
                source_id: 0,
                path: "sample.srt".into(),
                normalized_path: Some("sample.srt".into()),
                content_hash: None,
                text: Some("print(42)".into()),
            }],
            pc_spans: Vec::new(),
            runtime_process_specs: RuntimeProcessSpecTable {
                entries: vec![RuntimeProcessSpec {
                    process_id: 0,
                    type_name: "Counter".into(),
                    kind: RuntimeProcessKind::Agent,
                    instance: RuntimeProcessInstance::Worker,
                    state: crate::ir::RuntimeStateSpec {
                        state_type: crate::ir::RuntimeTypeRef { name: "Int".into() },
                    },
                    init: crate::ir::RuntimeInitSpec {
                        callable: crate::ir::RuntimeCallableRef { fun_idx: 0 },
                        policy: crate::ir::RuntimeInitPolicy::Eager,
                        result_shape: crate::ir::RuntimeInitResultShape::EagerState {
                            result_type: crate::ir::RuntimeTypeRef {
                                name: "Result<Int>".into(),
                            },
                        },
                        state_type: crate::ir::RuntimeTypeRef { name: "Int".into() },
                        init_route: None,
                    },
                    handlers: Vec::new(),
                    dependencies: Default::default(),
                    lifecycle: Default::default(),
                    supervision: Default::default(),
                }],
            },
            runtime_boot_plan: Default::default(),
        };
        let inspected = EldrInspect {
            header: EldrHeader {
                magic: "ELDR".into(),
                version: 1,
                debug_level: 2,
                num_chunks: 2,
            },
            chunks: vec![EldrChunkInfo {
                tag: "Code".into(),
                size: 12,
                payload_offset: 16,
                padded_size: 12,
            }],
            bytecode,
        };

        let viewer = viewer_file_from_inspect(&inspected);
        assert_eq!(viewer.schema_version, VIEWER_SCHEMA_VERSION);
        assert_eq!(viewer.format, VIEWER_FORMAT);
        assert_eq!(viewer.process_specs.len(), 1);
        assert_eq!(viewer.process_specs[0].kind, ProcessSpecKindView::Agent);
        assert_eq!(
            viewer.process_specs[0].instance,
            ProcessSpecInstanceView::Worker
        );
        assert_eq!(viewer.functions.len(), 1);
        assert_eq!(viewer.constants.len(), 1);
        assert_eq!(viewer.opcodes.len(), 4);
        assert_eq!(viewer.sources.len(), 1);
        assert_eq!(viewer.errors.len(), 1);
    }

    #[test]
    #[cfg(feature = "viewer-schema")]
    fn viewer_schema_is_buildable() {
        let schema = viewer_schema();
        assert_eq!(
            schema
                .schema
                .object
                .as_ref()
                .and_then(|obj| obj.properties.get("format"))
                .is_some(),
            true
        );
    }
}
