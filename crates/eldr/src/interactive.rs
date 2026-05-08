use sindr::ir::{Bytecode, BytecodeChunk, FunctionEntry, RuntimeBootPlan};
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
        self.verify_append_only_chunk(&chunk)?;
        self.vm
            .push_atomic(chunk)
            .map(|value| ChunkExecution { value })
    }

    pub fn push_atomic_bootstrap(
        &mut self,
        chunk: BytecodeChunk,
    ) -> Result<ChunkExecution, RuntimeError> {
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

    fn verify_append_only_chunk(&self, chunk: &BytecodeChunk) -> Result<(), RuntimeError> {
        let current_function_len = self.vm.function_entries().len();
        if let Some(entry) = chunk
            .functions
            .iter()
            .find(|entry| (entry.fun_idx as usize) < current_function_len)
        {
            return Err(RuntimeError::new(format!(
                "InteractiveVm append-only function table violation: fun_idx {} would overwrite an existing function slot",
                entry.fun_idx
            )));
        }
        if !chunk.type_entries.is_empty() {
            return Err(RuntimeError::new(
                "InteractiveVm append-only REPL chunk cannot add type_entries",
            ));
        }
        if !chunk.runtime_process_specs.is_empty() {
            return Err(RuntimeError::new(
                "InteractiveVm append-only REPL chunk cannot add runtime_process_specs",
            ));
        }
        if chunk.runtime_boot_plan != RuntimeBootPlan::default() {
            return Err(RuntimeError::new(
                "InteractiveVm append-only REPL chunk cannot add runtime_boot_plan entries",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::InteractiveVm;
    use sindr::ir::{
        Bytecode, BytecodeChunk, FunctionEntry, Opcode, RuntimeBootPlan, RuntimeCallableRef,
        RuntimeInitPolicy, RuntimeInitResultShape, RuntimeInitSpec, RuntimeProcessInstance,
        RuntimeProcessKind, RuntimeProcessSpec, RuntimeStateSpec, RuntimeTypeRef,
        SingletonBootEntry,
    };
    use sindr::runtime::{TypeEntry, TypeKind, TypeRegistry};

    fn function_entry(fun_idx: u32, entry_pc: u32, qualified_name: &str) -> FunctionEntry {
        FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: 0,
            arity: 0,
            qualified_name: Some(qualified_name.to_string()),
            signature: None,
            end_pc: 0,
            span_start: 0,
            span_end: 0,
            flags: Default::default(),
        }
    }

    fn empty_chunk() -> BytecodeChunk {
        BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
        }
    }

    #[test]
    fn push_atomic_rejects_overwriting_existing_function_slot() {
        let mut bytecode = Bytecode {
            opcodes: vec![Opcode::Halt],
            functions: vec![function_entry(0, 0, "Main::old")],
            ..Bytecode::default()
        };
        bytecode.num_locals = 0;
        let mut vm = InteractiveVm::from_bytecode(bytecode);

        let err = vm
            .push_atomic(BytecodeChunk {
                opcodes: vec![Opcode::Halt, Opcode::Return],
                source_map: None,
                const_base: 0,
                constants: Vec::new(),
                new_locals: 0,
                type_entries: Vec::new(),
                error_template_base: 0,
                error_templates: Vec::new(),
                dbg_template_base: 0,
                dbg_templates: Vec::new(),
                functions: vec![function_entry(0, 1, "Main::new")],
                docs: Vec::new(),
                runtime_process_specs: Vec::new(),
                runtime_boot_plan: Default::default(),
            })
            .expect_err("interactive vm must reject function replacement");

        assert!(
            err.message.contains("append-only function table"),
            "{}",
            err.message
        );
        assert_eq!(
            vm.function_entries()[0].qualified_name.as_deref(),
            Some("Main::old")
        );
    }

    #[test]
    fn push_atomic_rejects_type_entries_in_repl_mode() {
        let mut vm = InteractiveVm::new(TypeRegistry::new());
        let mut chunk = empty_chunk();
        chunk.type_entries.push(TypeEntry {
            tag: 99,
            name: "Extra".into(),
            kind: TypeKind::Struct,
            field_names: Vec::new(),
            private_flags: Vec::new(),
        });

        let err = vm
            .push_atomic(chunk)
            .expect_err("repl mode must reject type entries");
        assert!(err.message.contains("type_entries"), "{}", err.message);
    }

    #[test]
    fn push_atomic_rejects_runtime_process_specs_in_repl_mode() {
        let mut vm = InteractiveVm::new(TypeRegistry::new());
        let mut chunk = empty_chunk();
        chunk.runtime_process_specs.push(RuntimeProcessSpec {
            process_id: 0,
            type_name: "Worker".into(),
            kind: RuntimeProcessKind::Agent,
            instance: RuntimeProcessInstance::Worker,
            state: RuntimeStateSpec {
                state_type: RuntimeTypeRef { name: "Int".into() },
                owner_process: None,
            },
            init: RuntimeInitSpec {
                callable: RuntimeCallableRef { fun_idx: 0 },
                policy: RuntimeInitPolicy::Eager,
                result_shape: RuntimeInitResultShape::EagerState {
                    result_type: RuntimeTypeRef { name: "Int".into() },
                },
                state_type: RuntimeTypeRef { name: "Int".into() },
                init_route: None,
            },
            handlers: Vec::new(),
            dependencies: Default::default(),
            lifecycle: Default::default(),
            supervision: Default::default(),
        });

        let err = vm
            .push_atomic(chunk)
            .expect_err("repl mode must reject runtime specs");
        assert!(
            err.message.contains("runtime_process_specs"),
            "{}",
            err.message
        );
    }

    #[test]
    fn push_atomic_rejects_runtime_boot_plan_in_repl_mode() {
        let mut vm = InteractiveVm::new(TypeRegistry::new());
        let mut chunk = empty_chunk();
        chunk.runtime_boot_plan = RuntimeBootPlan {
            singletons: vec![SingletonBootEntry {
                process_name: "Counter".into(),
                init_timeout_ms: 5000,
                source: sindr::ir::BootEntrySource::ExplicitConfig,
            }],
            ..RuntimeBootPlan::default()
        };

        let err = vm
            .push_atomic(chunk)
            .expect_err("repl mode must reject runtime boot plan");
        assert!(
            err.message.contains("runtime_boot_plan"),
            "{}",
            err.message
        );
    }

    #[test]
    fn push_atomic_bootstrap_allows_type_entries() {
        let mut vm = InteractiveVm::new(TypeRegistry::new());
        let mut chunk = empty_chunk();
        chunk.type_entries.push(TypeEntry {
            tag: 99,
            name: "Extra".into(),
            kind: TypeKind::Struct,
            field_names: Vec::new(),
            private_flags: Vec::new(),
        });

        vm.push_atomic_bootstrap(chunk)
            .expect("bootstrap mode should allow structural chunk");
        assert!(
            vm.type_registry().entries.iter().any(|entry| entry.tag == 99),
            "bootstrap type entry should be committed"
        );
    }
}
