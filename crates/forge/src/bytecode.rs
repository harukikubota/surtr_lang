use crate::opcode::Opcode;
use crate::registry::{TypeEntry, TypeRegistry};
use serde::{Deserialize, Serialize};

/// A compiled Surtr program, ready for Eldr to execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bytecode {
    pub opcodes: Vec<Opcode>,
    pub constants: Vec<Constant>,
    pub num_locals: usize,
    pub type_registry: TypeRegistry,
    pub error_templates: Vec<ErrTemplate>,
    pub functions: Vec<FunctionEntry>,
}

/// Incremental bytecode payload for REPL execution.
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeChunk {
    pub opcodes: Vec<Opcode>,
    pub constants: Vec<Constant>,
    pub new_locals: usize,
    pub type_entries: Vec<TypeEntry>,
    pub error_templates: Vec<ErrTemplate>,
    pub functions: Vec<FunctionEntry>,
}

/// Function table entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionEntry {
    pub fun_idx: u32,
    pub entry_pc: u32,
    pub num_locals: u32,
    pub arity: u8,
}

/// Constant pool entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Int(i64),
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
        let bytecode = bincode::deserialize(payload)
            .map_err(|e| BytecodeFormatError::DecodeFailed(e.to_string()))?;
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
    let mut chunks: Vec<EldrChunkInfo> = Vec::new();

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
    use super::{Bytecode, BytecodeFormatError, Constant, ErrTemplate, FunctionEntry};
    use crate::opcode::Opcode;
    use crate::registry::{TypeEntry, TypeKind, TypeRegistry};

    #[test]
    fn roundtrip_encode_decode() {
        let mut registry = TypeRegistry::new();
        registry.register(TypeEntry {
            tag: 10,
            name: "User".to_string(),
            kind: TypeKind::Struct,
            field_names: vec!["name".to_string(), "age".to_string()],
        });

        let bytecode = Bytecode {
            opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
            constants: vec![Constant::Int(42)],
            num_locals: 1,
            type_registry: registry,
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
            }],
        };

        let bytes = bytecode.encode().expect("encode should succeed");
        let decoded = Bytecode::decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, bytecode);

        let inspected = Bytecode::inspect(&bytes).expect("inspect should succeed");
        assert_eq!(inspected.header.magic, "ELDR");
        assert_eq!(inspected.header.version, 1);
        assert_eq!(inspected.chunks.len(), 1);
        assert_eq!(inspected.chunks[0].tag, "Code");
    }

    #[test]
    fn decode_rejects_invalid_magic() {
        let bytes = b"BAD!\x01\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::InvalidMagic(_)));
    }

    #[test]
    fn decode_rejects_missing_code_chunk() {
        // Header only, num_chunks = 0
        let bytes = b"ELDR\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let err = Bytecode::decode(bytes).expect_err("decode must fail");
        assert!(matches!(err, BytecodeFormatError::MissingCodeChunk));
    }
}
