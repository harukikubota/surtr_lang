use sindr::runtime::Value;

pub(crate) fn committed_chunk_value(execution: eldr::ChunkExecution) -> Value {
    execution.value
}
