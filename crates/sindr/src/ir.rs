use serde::{Deserialize, Serialize};

use crate::primitives::{BuiltinId, FunctionId, RuntimeTag, SurtrInt};
use crate::runtime::{TypeEntry, TypeRegistry};

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

    // Unary
    NegInt,
    NegFloat,
    NotBool,

    // List
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

    // Struct / Tagged
    StructNew {
        field_count: u32,
    },
    GetField {
        field_index: u32,
    },
    GetTag,
    EqTag,

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
    CapturePartial(u8),
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

    // Deprecated frame opcodes (kept for bytecode compatibility only).
    // New Forge codegen should not emit these.
    MakeFrame(u32),
    PopFrame,

    // Function return
    Return,

    // Program termination
    Halt,
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

/// A compiled Surtr program, ready for Eldr to execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bytecode {
    pub opcodes: Vec<Opcode>,
    pub constants: Vec<Constant>,
    pub num_locals: usize,
    pub type_registry: TypeRegistry,
    pub error_templates: Vec<ErrTemplate>,
    pub functions: Vec<FunctionEntry>,
    pub source_map: Option<SourceMap>,
    /// Symbol-level documentation carried from `@@doc` through `.eldr`.
    #[serde(default)]
    pub docs: Vec<DocEntry>,
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
    pub type_entries: Vec<TypeEntry>,
    /// Base offset of error templates in the VM-wide pool when this chunk is produced.
    pub error_template_base: u32,
    pub error_templates: Vec<ErrTemplate>,
    pub functions: Vec<FunctionEntry>,
}

/// Function table entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionEntry {
    pub fun_idx: FunctionId,
    pub entry_pc: u32,
    pub num_locals: u32,
    pub arity: u8,
    pub qualified_name: Option<String>,
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

    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

#[derive(Debug, Clone, PartialEq)]
pub enum BytecodeFormatError {
    HeaderTooShort,
    InvalidMagic([u8; 4]),
    UnsupportedVersion(u32),
    TruncatedChunkHeader,
    TruncatedChunkData,
    MissingCodeChunk,
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
            BytecodeFormatError::MissingCodeChunk => write!(f, "missing Code chunk"),
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
    code_payload: &'a [u8],
    docs_payload: Option<&'a [u8]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CodePayload {
    opcodes: Vec<Opcode>,
    constants: Vec<Constant>,
    num_locals: usize,
    type_registry: TypeRegistry,
    error_templates: Vec<ErrTemplate>,
    functions: Vec<FunctionEntry>,
    source_map: Option<SourceMap>,
}

impl From<CodePayload> for Bytecode {
    fn from(value: CodePayload) -> Self {
        Self {
            opcodes: value.opcodes,
            constants: value.constants,
            num_locals: value.num_locals,
            type_registry: value.type_registry,
            error_templates: value.error_templates,
            functions: value.functions,
            source_map: value.source_map,
            docs: Vec::new(),
        }
    }
}

impl Bytecode {
    const MAGIC: [u8; 4] = *b"ELDR";
    const VERSION: u32 = 1;
    const DEBUG_LEVEL: u32 = 0;
    const HEADER_LEN: usize = 16;
    const CHUNK_HEADER_LEN: usize = 8;
    const CHUNK_CODE: [u8; 4] = *b"Code";
    const CHUNK_DOCS: [u8; 4] = *b"Docs";

    /// Encode bytecode as `.eldr` bytes:
    /// Header(16 bytes) + chunk table + payloads.
    pub fn encode(&self) -> Result<Vec<u8>, BytecodeFormatError> {
        let code_payload = bincode::serialize(&CodePayload {
            opcodes: self.opcodes.clone(),
            constants: self.constants.clone(),
            num_locals: self.num_locals,
            type_registry: self.type_registry.clone(),
            error_templates: self.error_templates.clone(),
            functions: self.functions.clone(),
            source_map: self.source_map.clone(),
        })
        .map_err(|e| BytecodeFormatError::EncodeFailed(e.to_string()))?;
        let docs_payload = if self.docs.is_empty() {
            None
        } else {
            Some(
                bincode::serialize(&self.docs)
                    .map_err(|e| BytecodeFormatError::EncodeFailed(e.to_string()))?,
            )
        };
        let num_chunks = if docs_payload.is_some() { 2u32 } else { 1u32 };
        let padded_code_payload_len = align4(code_payload.len());
        let padded_docs_payload_len = docs_payload
            .as_ref()
            .map(|payload| align4(payload.len()))
            .unwrap_or(0);
        let code_payload_len = checked_payload_len(code_payload.len())?;
        let docs_payload_len = docs_payload
            .as_ref()
            .map(|payload| checked_payload_len(payload.len()))
            .transpose()?;
        let total_len = Self::HEADER_LEN
            + (Self::CHUNK_HEADER_LEN * num_chunks as usize)
            + padded_code_payload_len
            + padded_docs_payload_len;
        let mut bytes = Vec::with_capacity(total_len);

        bytes.extend_from_slice(&Self::MAGIC);
        bytes.extend_from_slice(&Self::VERSION.to_le_bytes());
        bytes.extend_from_slice(&Self::DEBUG_LEVEL.to_le_bytes());
        bytes.extend_from_slice(&num_chunks.to_le_bytes());
        bytes.extend_from_slice(&Self::CHUNK_CODE);
        bytes.extend_from_slice(&code_payload_len.to_le_bytes());
        if docs_payload.is_some() {
            bytes.extend_from_slice(&Self::CHUNK_DOCS);
            bytes.extend_from_slice(&docs_payload_len.unwrap_or(0).to_le_bytes());
        }
        bytes.extend_from_slice(&code_payload);
        bytes.resize(align4(bytes.len()), 0);
        if let Some(docs_payload) = docs_payload {
            bytes.extend_from_slice(&docs_payload);
            bytes.resize(align4(bytes.len()), 0);
        }
        Ok(bytes)
    }

    /// Inspect `.eldr` bytes and decode embedded bytecode.
    pub fn inspect(bytes: &[u8]) -> Result<EldrInspect, BytecodeFormatError> {
        let parsed = parse_container(bytes)?;
        let bytecode = decode_payload_with_docs(parsed.code_payload, parsed.docs_payload)?;
        Ok(EldrInspect {
            header: parsed.header,
            chunks: parsed.chunks,
            bytecode,
        })
    }

    /// Decode `.eldr` bytes into bytecode.
    pub fn decode(bytes: &[u8]) -> Result<Self, BytecodeFormatError> {
        let parsed = parse_container(bytes)?;
        decode_payload_with_docs(parsed.code_payload, parsed.docs_payload)
    }
}

fn decode_payload_with_docs(
    code_payload: &[u8],
    docs_payload: Option<&[u8]>,
) -> Result<Bytecode, BytecodeFormatError> {
    let mut bytecode = bincode::deserialize::<CodePayload>(code_payload)
        .map(Bytecode::from)
        .map_err(|e| BytecodeFormatError::DecodeFailed(e.to_string()))?;

    if let Some(payload) = docs_payload {
        bytecode.docs = bincode::deserialize::<Vec<DocEntry>>(payload)
            .map_err(|e| BytecodeFormatError::DecodeFailed(e.to_string()))?;
    }

    Ok(bytecode)
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
    if version != Bytecode::VERSION {
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
    let mut code_payload: Option<&[u8]> = None;
    let mut docs_payload: Option<&[u8]> = None;
    let mut chunks = Vec::with_capacity(raw_chunks.len());

    for (tag_bytes, size) in raw_chunks {
        if payload_offset + size as usize > bytes.len() {
            return Err(BytecodeFormatError::TruncatedChunkData);
        }

        let chunk_payload_offset = payload_offset;
        if tag_bytes == Bytecode::CHUNK_CODE {
            code_payload = Some(&bytes[chunk_payload_offset..chunk_payload_offset + size as usize]);
        } else if tag_bytes == Bytecode::CHUNK_DOCS {
            docs_payload = Some(&bytes[chunk_payload_offset..chunk_payload_offset + size as usize]);
        }

        let padded_size = align4(size as usize);
        chunks.push(EldrChunkInfo {
            tag: String::from_utf8_lossy(&tag_bytes).to_string(),
            size,
            payload_offset: chunk_payload_offset,
            padded_size,
        });

        payload_offset += padded_size;
    }

    let payload = code_payload.ok_or(BytecodeFormatError::MissingCodeChunk)?;
    let header = EldrHeader {
        magic: String::from_utf8_lossy(&magic_bytes).to_string(),
        version,
        debug_level,
        num_chunks,
    };
    Ok(ParsedContainer {
        header,
        chunks,
        code_payload: payload,
        docs_payload,
    })
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
        checked_payload_len, line_column_for_offset, populate_error_template_lines, Bytecode,
        BytecodeFormatError, Constant, DocEntry, DocKind, ErrTemplate, FunctionEntry, Opcode,
        OpcodeSource, SourceMap,
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
        });
        registry
    }

    fn sample_bytecode(source_map: Option<SourceMap>) -> Bytecode {
        Bytecode {
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
            functions: vec![FunctionEntry {
                fun_idx: 0,
                entry_pc: 1,
                num_locals: 0,
                arity: 0,
                qualified_name: Some("Main::entry".to_string()),
            }],
            source_map,
            docs: vec![DocEntry {
                qualified_name: "Bootstrap::Int".to_string(),
                kind: DocKind::Type,
                module_path: "Bootstrap".to_string(),
                signature: Some("type Int".to_string()),
                doc: "Builtin Int type.".to_string(),
            }],
        }
    }

    #[test]
    fn roundtrip_encode_decode_with_source_map_none() {
        let bytecode = sample_bytecode(None);
        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, bytecode);
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
    fn decode_rejects_invalid_magic() {
        let bytes = b"BAD!\x01\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::InvalidMagic(_)));
    }

    #[test]
    fn decode_rejects_missing_code_chunk() {
        let bytes = b"ELDR\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::MissingCodeChunk));
    }

    #[test]
    fn decode_rejects_unsupported_version() {
        let bytes = b"ELDR\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::UnsupportedVersion(2)));
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
        assert_eq!(inspected.header.version, 1);
        assert_eq!(inspected.chunks.len(), 2);
        assert_eq!(inspected.chunks[0].tag, "Code");
        assert_eq!(inspected.chunks[1].tag, "Docs");
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
        assert_eq!(line_column_for_offset(source, "あ".len()), (1, 2));
        assert_eq!(line_column_for_offset(source, "あい\nう".len()), (2, 2));
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
