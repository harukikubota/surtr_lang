pub use sindr::ir::{
    line_column_for_offset, populate_error_template_lines, stable_hash_hex, synthesize_source_map,
    Bytecode, BytecodeChunk, BytecodeFormatError, CompileInfo, Constant, EldrChunkInfo, EldrHeader,
    EldrInspect, ErrTemplate, ExportEntry, FunctionEntry, FunctionFlags, ImportEntry, ImportKind,
    LabelEntry, LineEntry, LiteralEntry, LiteralKind, OpcodeSource, PcSpanEntry, SourceFileEntry,
    SourceMap, SpanEntry,
};
