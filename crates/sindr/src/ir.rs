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
    ListNew(u32),
    ListEmpty,
    ListNil,
    ListCons,
    ListIsEmpty,
    ListHead,
    ListTail,
    ListFromItems(u32),

    // Struct / Tagged
    StructNew(u32),
    GetField(u32),
    GetTag,
    EqTag,

    // Built-in function call
    CallBuiltin(BuiltinId, u8, u32, u32),

    // User-defined function call
    Call(FunctionId, u8, u32, u32),
    CaptureClosure(u8),
    CapturePartial(u8),
    MakeError(u32),
    MakeErrorLiteral(u32, u32),
    CallClosure(u8, u32, u32),

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LegacyBytecode {
    opcodes: Vec<Opcode>,
    constants: Vec<Constant>,
    num_locals: usize,
    type_registry: TypeRegistry,
    error_templates: Vec<ErrTemplate>,
    functions: Vec<LegacyFunctionEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LegacyFunctionEntry {
    fun_idx: u32,
    entry_pc: u32,
    num_locals: u32,
    arity: u8,
}

impl From<LegacyFunctionEntry> for FunctionEntry {
    fn from(value: LegacyFunctionEntry) -> Self {
        Self {
            fun_idx: value.fun_idx,
            entry_pc: value.entry_pc,
            num_locals: value.num_locals,
            arity: value.arity,
            qualified_name: None,
        }
    }
}

impl From<LegacyBytecode> for Bytecode {
    fn from(value: LegacyBytecode) -> Self {
        Self {
            opcodes: value.opcodes,
            constants: value.constants,
            num_locals: value.num_locals,
            type_registry: value.type_registry,
            error_templates: value.error_templates,
            functions: value
                .functions
                .into_iter()
                .map(FunctionEntry::from)
                .collect(),
            source_map: None,
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

    /// Encode bytecode as `.eldr` bytes:
    /// Header(16 bytes) + Code chunk(tag+size+payload+padding).
    pub fn encode(&self) -> Result<Vec<u8>, BytecodeFormatError> {
        let payload = bincode::serialize(self)
            .map_err(|e| BytecodeFormatError::EncodeFailed(e.to_string()))?;
        let padded_payload_len = align4(payload.len());
        let total_len = Self::HEADER_LEN + Self::CHUNK_HEADER_LEN + padded_payload_len;
        let mut bytes = Vec::with_capacity(total_len);

        bytes.extend_from_slice(&Self::MAGIC);
        bytes.extend_from_slice(&Self::VERSION.to_le_bytes());
        bytes.extend_from_slice(&Self::DEBUG_LEVEL.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes()); // num_chunks
        bytes.extend_from_slice(&Self::CHUNK_CODE);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        Ok(bytes)
    }

    /// Inspect `.eldr` bytes and decode embedded bytecode.
    pub fn inspect(bytes: &[u8]) -> Result<EldrInspect, BytecodeFormatError> {
        let (header, chunks, payload) = parse_container(bytes)?;
        let bytecode = decode_payload(payload)?;
        Ok(EldrInspect {
            header,
            chunks,
            bytecode,
        })
    }

    /// Decode `.eldr` bytes into bytecode.
    pub fn decode(bytes: &[u8]) -> Result<Self, BytecodeFormatError> {
        Self::inspect(bytes).map(|inspected| inspected.bytecode)
    }
}

fn decode_payload(payload: &[u8]) -> Result<Bytecode, BytecodeFormatError> {
    match bincode::deserialize::<Bytecode>(payload) {
        Ok(bytecode) => Ok(bytecode),
        Err(err_new) => {
            // Backward-compatible path for payloads generated before `source_map` was added.
            match bincode::deserialize::<LegacyBytecode>(payload) {
                Ok(legacy) => Ok(legacy.into()),
                Err(err_legacy) => Err(BytecodeFormatError::DecodeFailed(format!(
                    "new-format error: {}; legacy-format error: {}",
                    err_new, err_legacy
                ))),
            }
        }
    }
}

fn parse_container(
    bytes: &[u8],
) -> Result<(EldrHeader, Vec<EldrChunkInfo>, &[u8]), BytecodeFormatError> {
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
    let mut offset = Bytecode::HEADER_LEN;
    let mut code_payload: Option<&[u8]> = None;
    let mut chunks = Vec::new();

    for _ in 0..num_chunks as usize {
        if offset + Bytecode::CHUNK_HEADER_LEN > bytes.len() {
            return Err(BytecodeFormatError::TruncatedChunkHeader);
        }

        let tag_bytes = [
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        offset += Bytecode::CHUNK_HEADER_LEN;

        if offset + size as usize > bytes.len() {
            return Err(BytecodeFormatError::TruncatedChunkData);
        }

        let payload_offset = offset;
        if tag_bytes == Bytecode::CHUNK_CODE {
            code_payload = Some(&bytes[payload_offset..payload_offset + size as usize]);
        }

        let padded_size = align4(size as usize);
        chunks.push(EldrChunkInfo {
            tag: String::from_utf8_lossy(&tag_bytes).to_string(),
            size,
            payload_offset,
            padded_size,
        });

        offset += size as usize;
        while offset % 4 != 0 && offset < bytes.len() {
            offset += 1;
        }
    }

    let payload = code_payload.ok_or(BytecodeFormatError::MissingCodeChunk)?;
    let header = EldrHeader {
        magic: String::from_utf8_lossy(&magic_bytes).to_string(),
        version,
        debug_level,
        num_chunks,
    };
    Ok((header, chunks, payload))
}

fn align4(len: usize) -> usize {
    (len + 3) & !3
}

#[cfg(test)]
mod tests {
    use super::{
        line_column_for_offset, populate_error_template_lines, Bytecode, BytecodeFormatError,
        Constant, ErrTemplate, FunctionEntry, LegacyBytecode, LegacyFunctionEntry, Opcode,
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
    fn decode_legacy_payload_without_source_map() {
        let legacy = LegacyBytecode {
            opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
            constants: vec![Constant::Int(int(7))],
            num_locals: 0,
            type_registry: sample_registry(),
            error_templates: Vec::new(),
            functions: vec![LegacyFunctionEntry {
                fun_idx: 0,
                entry_pc: 1,
                num_locals: 0,
                arity: 0,
            }],
        };

        let payload = bincode::serialize(&legacy).expect("legacy payload encode should succeed");

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"ELDR");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(b"Code");
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }

        let decoded = Bytecode::decode(&bytes).expect("legacy decode should succeed");
        assert_eq!(decoded.source_map, None);
        assert_eq!(decoded.constants, vec![Constant::Int(int(7))]);
        assert_eq!(decoded.functions[0].qualified_name, None);
    }

    #[test]
    fn line_column_for_offset_tracks_multiline_source() {
        let source = "deferror Boom {\n  \"boom\"\n}\n";
        assert_eq!(line_column_for_offset(source, 0), (1, 1));
        assert_eq!(line_column_for_offset(source, 16), (2, 1));
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
}
