use sindr::ir::{Bytecode, BytecodeChunk, FunctionEntry};
use sindr::runtime::{Location, TypeRegistry, Value};
use std::time::Duration;

use crate::error::RuntimeError;
use crate::vm::VM;

/// Result of one committed interactive bytecode chunk.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkExecution {
    /// Final top-level value produced by the chunk. Empty stacks are normalized to `Unit`.
    pub value: Value,
}

/// REPL-facing VM wrapper for incremental bytecode execution.
///
/// `InteractiveVm` owns the append/rollback contract for `BytecodeChunk` while
/// keeping source-level REPL policy in Xldr.
#[derive(Clone)]
pub struct InteractiveVm {
    vm: VM,
}

impl InteractiveVm {
    /// Create an empty interactive VM seeded with the compiler's type registry.
    pub fn new(type_registry: TypeRegistry) -> Self {
        Self::from_vm(VM::new_interactive(type_registry))
    }

    /// Create an interactive VM from an existing bytecode image.
    pub fn from_bytecode(bytecode: Bytecode) -> Self {
        Self::from_vm(VM::new(bytecode))
    }

    fn from_vm(mut vm: VM) -> Self {
        vm.enable_repl_host_io_buffering();
        Self { vm }
    }

    pub fn with_source(mut self, source: String, file: String) -> Self {
        self.vm.set_source(source, file);
        self
    }

    pub fn as_vm(&self) -> &VM {
        &self.vm
    }

    pub fn bytecode(&self) -> &Bytecode {
        self.vm.bytecode()
    }

    pub fn type_registry(&self) -> &TypeRegistry {
        self.vm.type_registry()
    }

    pub fn snapshot_bytecode(&self) -> Bytecode {
        self.vm.snapshot_bytecode()
    }

    pub fn push_atomic(&mut self, chunk: BytecodeChunk) -> Result<ChunkExecution, RuntimeError> {
        self.vm
            .push_atomic(chunk)
            .map(|value| ChunkExecution { value })
    }

    pub fn last_result(&self) -> Option<&Value> {
        self.vm.last_value()
    }

    pub fn stack_depth(&self) -> usize {
        self.vm.stack_depth()
    }

    pub fn set_source(&mut self, source: String, file: String) {
        self.vm.set_source(source, file);
    }

    pub fn source(&self) -> Option<&str> {
        self.vm.source()
    }

    pub fn source_file(&self) -> Option<&str> {
        self.vm.source_file()
    }

    pub fn runtime_error_location(&self) -> Option<Location> {
        self.vm.runtime_error_location()
    }

    pub fn enable_repl_host_io_buffering(&mut self) {
        self.vm.enable_repl_host_io_buffering();
    }

    pub fn take_repl_host_stdout(&mut self) -> Vec<String> {
        self.vm.take_repl_host_stdout()
    }

    pub fn take_repl_host_stderr(&mut self) -> Vec<String> {
        self.vm.take_repl_host_stderr()
    }

    pub fn get_local(&self, slot: u32) -> Option<Value> {
        self.vm.get_local(slot)
    }

    pub fn function_entries(&self) -> &[FunctionEntry] {
        self.vm.function_entries()
    }

    pub fn has_pending_background_work(&self) -> bool {
        self.vm.has_pending_background_work()
    }

    pub fn next_background_deadline_delay(&self) -> Option<Duration> {
        self.vm.next_background_deadline_delay()
    }

    pub fn pump_background_ready(&mut self) -> Result<(), RuntimeError> {
        self.vm.pump_background_ready()
    }

    pub fn advance_background_time(&mut self, elapsed: Duration) -> Result<(), RuntimeError> {
        self.vm.advance_background_time(elapsed)
    }

    pub fn pump_background_to_next_deadline(&mut self) -> Result<bool, RuntimeError> {
        self.vm.pump_background_to_next_deadline()
    }
}
