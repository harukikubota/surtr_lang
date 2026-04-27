use sindr::builtin::builtin_meta_by_id;
use sindr::ir::{
    line_column_for_offset, Bytecode, BytecodeChunk, Constant, DocEntry, FunctionEntry, Opcode,
    SourceMap,
};
use sindr::primitives::SurtrInt;
use sindr::runtime::{
    Callable, CallableTarget, ListHandle, Location, RichError, TypeRegistry, Value,
};
use std::collections::{BTreeMap, BTreeSet};

use crate::builtin::call_builtin;
use crate::error::{RuntimeError, RuntimeErrorContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmTestEventKind {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmCapturedIo {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTestEvent {
    pub path: Vec<String>,
    pub detail: Option<String>,
    pub kind: VmTestEventKind,
    pub io: Option<VmCapturedIo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IoMode {
    #[default]
    Passthrough,
    Capture,
    Tee,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VmIoPolicy {
    pub stdout: IoMode,
    pub stderr: IoMode,
}

#[derive(Debug, Clone)]
struct CallFrame {
    return_pc: usize,
    stack_base: usize,
    call_site: Option<(u32, u32)>,
    locals: Vec<Value>,
}

#[derive(Debug, Clone)]
struct VmCheckpoint {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    pc: usize,
    exit_code: i32,
    last_result: Option<Value>,
    output_len: Option<usize>,
    error_output_len: Option<usize>,
    test_scope_len: usize,
    test_event_len: usize,
    test_stdout_cursor: usize,
    test_stderr_cursor: usize,
    opcode_len: usize,
    constant_len: usize,
    type_entry_len: usize,
    error_template_len: usize,
    function_len: usize,
    doc_len: usize,
    source_map_len: Option<usize>,
    overwritten_functions: Vec<(usize, FunctionEntry)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmObservationOptions {
    pub trace_opcodes: bool,
    pub trace_calls: bool,
    pub trace_limit: Option<usize>,
    pub trace_filter: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmStats {
    pub executed_opcodes: usize,
    pub per_opcode: BTreeMap<String, usize>,
    pub max_stack_depth: usize,
    pub max_frame_depth: usize,
    pub builtin_calls: usize,
    pub function_calls: usize,
    pub closure_calls: usize,
    pub return_count: usize,
    pub tail_calls_optimized: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmObservation {
    pub stats: VmStats,
    pub trace_lines: Vec<String>,
    pub dropped_trace_events: usize,
}

#[derive(Debug, Clone)]
struct VmObserver {
    options: VmObservationOptions,
    stats: VmStats,
    trace_lines: Vec<String>,
    dropped_trace_events: usize,
}

impl VmObserver {
    fn new(options: VmObservationOptions) -> Self {
        Self {
            options,
            stats: VmStats::default(),
            trace_lines: Vec::new(),
            dropped_trace_events: 0,
        }
    }

    fn snapshot(&self) -> VmObservation {
        VmObservation {
            stats: self.stats.clone(),
            trace_lines: self.trace_lines.clone(),
            dropped_trace_events: self.dropped_trace_events,
        }
    }

    fn record_depths(&mut self, stack_depth: usize, frame_depth: usize) {
        self.stats.max_stack_depth = self.stats.max_stack_depth.max(stack_depth);
        self.stats.max_frame_depth = self.stats.max_frame_depth.max(frame_depth);
    }

    fn trace_enabled_for(&self, kind: &str) -> bool {
        if self.options.trace_filter.is_empty() {
            return true;
        }
        self.options
            .trace_filter
            .contains(&kind.to_ascii_lowercase())
    }

    fn push_trace(&mut self, line: String) {
        if let Some(limit) = self.options.trace_limit {
            if self.trace_lines.len() >= limit {
                self.dropped_trace_events += 1;
                return;
            }
        }
        self.trace_lines.push(line);
    }

    fn record_opcode_step(
        &mut self,
        pc: usize,
        opcode: &Opcode,
        stack_depth: usize,
        frame_depth: usize,
    ) {
        let kind = opcode.kind_name();
        self.stats.executed_opcodes += 1;
        *self.stats.per_opcode.entry(kind.to_string()).or_default() += 1;
        self.record_depths(stack_depth, frame_depth);
        if self.options.trace_opcodes && self.trace_enabled_for(kind) {
            self.push_trace(format!(
                "op pc={} opcode={:?} stack_depth={} frame_depth={}",
                pc, opcode, stack_depth, frame_depth
            ));
        }
    }

    fn record_call_event(&mut self, kind: &str, line: String) {
        if self.options.trace_calls && self.trace_enabled_for(kind) {
            self.push_trace(line);
        }
    }
}

/// The Surtr virtual machine — executes bytecode produced by Forge.
#[derive(Clone)]
pub struct VM {
    bytecode: Bytecode,
    /// Operand stack
    stack: Vec<Value>,
    /// Call stack / locals frames
    frames: Vec<CallFrame>,
    /// Program counter (used by full-program `run`)
    pc: usize,
    /// Source code (for eprint / ariadne)
    source: Option<String>,
    /// Source file name
    source_file: Option<String>,
    /// Command-line arguments passed by the Rune `run` command.
    cli_args: Vec<String>,
    /// Captured stdout (for testing). `None` = print to real stdout.
    pub output: Option<Vec<String>>,
    /// Captured stderr (for testing). `None` = print to real stderr.
    pub error_output: Option<Vec<String>>,
    /// Runtime I/O policy for stdout/stderr.
    io_policy: VmIoPolicy,
    /// Process exit code requested by the running program.
    exit_code: i32,
    /// Last value produced by full-program or chunk execution.
    last_result: Option<Value>,
    /// Optional developer-facing execution observer.
    observer: Option<VmObserver>,
    /// Current nested test/describe scope names.
    test_scope: Vec<String>,
    /// Collected test events emitted by the test DSL runtime helpers.
    test_events: Vec<VmTestEvent>,
    /// Cursor tracking for test-event I/O slices.
    test_stdout_cursor: usize,
    test_stderr_cursor: usize,
}

impl VM {
    pub fn new(bytecode: Bytecode) -> Self {
        let num_locals = bytecode.num_locals;
        Self {
            bytecode,
            stack: Vec::new(),
            frames: vec![CallFrame {
                return_pc: 0,
                stack_base: 0,
                call_site: None,
                locals: vec![Value::Unit; num_locals],
            }],
            pc: 0,
            source: None,
            source_file: None,
            cli_args: Vec::new(),
            output: None,
            error_output: None,
            io_policy: VmIoPolicy::default(),
            exit_code: 0,
            last_result: None,
            observer: None,
            test_scope: Vec::new(),
            test_events: Vec::new(),
            test_stdout_cursor: 0,
            test_stderr_cursor: 0,
        }
    }

    /// Create an empty VM intended for REPL/incremental execution.
    pub fn new_interactive(type_registry: TypeRegistry) -> Self {
        Self::new(Bytecode {
            type_registry,
            ..Bytecode::default()
        })
    }

    /// Set source code for error reporting.
    pub fn with_source(mut self, source: String, file: String) -> Self {
        self.source = Some(source);
        self.source_file = Some(file);
        self
    }

    /// Replace source context for later runtime diagnostics.
    pub fn set_source(&mut self, source: String, file: String) {
        self.source = Some(source);
        self.source_file = Some(file);
    }

    pub fn with_cli_args(mut self, cli_args: Vec<String>) -> Self {
        self.cli_args = cli_args;
        self
    }

    pub fn set_cli_args(&mut self, cli_args: Vec<String>) {
        self.cli_args = cli_args;
    }

    pub fn cli_args(&self) -> &[String] {
        &self.cli_args
    }

    /// Access source text if attached.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Access source file name if attached.
    pub fn source_file(&self) -> Option<&str> {
        self.source_file.as_deref()
    }

    pub fn runtime_error_location(&self) -> Option<Location> {
        let (span_start, span_end) = self.current_frame().ok()?.call_site?;
        let file = self
            .source_file()
            .map(str::to_string)
            .unwrap_or_else(|| "<runtime>".to_string());
        let (line, column) = self
            .source()
            .map(|source| line_column_for_offset(source, span_start as usize))
            .unwrap_or((0, 0));
        Some(Location {
            file,
            func: "<runtime>".into(),
            line,
            column,
            span_start,
            span_end,
        })
    }

    fn runtime_error_context(&self, pc: usize, opcode: &Opcode) -> RuntimeErrorContext {
        let current_function = self
            .bytecode
            .functions
            .iter()
            .filter(|entry| entry.entry_pc as usize <= pc)
            .max_by_key(|entry| entry.entry_pc)
            .map(|entry| format!("fun#{}", entry.fun_idx));

        let mut details = vec![
            format!("stack_depth={}", self.stack.len()),
            format!("frame_depth={}", self.frames.len()),
        ];

        if let Some(frame) = self.frames.last() {
            details.push(format!("locals_len={}", frame.locals.len()));
            details.push(format!("stack_base={}", frame.stack_base));
        }
        if let Some(top) = self.stack.last() {
            details.push(format!("stack_top={:?}", top));
        }

        RuntimeErrorContext {
            pc: Some(pc),
            opcode: Some(format!("{:?}", opcode)),
            function: current_function,
            call_site: self.runtime_error_location(),
            details,
        }
    }

    fn enrich_runtime_error(&self, err: RuntimeError, pc: usize, opcode: &Opcode) -> RuntimeError {
        let RuntimeError {
            message,
            context: err_context,
        } = err;
        let mut context = self.runtime_error_context(pc, opcode);
        if err_context.pc.is_some() {
            context.pc = err_context.pc;
        }
        if err_context.opcode.is_some() {
            context.opcode = err_context.opcode;
        }
        if err_context.function.is_some() {
            context.function = err_context.function;
        }
        if err_context.call_site.is_some() {
            context.call_site = err_context.call_site;
        }
        context.details.extend(err_context.details);
        RuntimeError {
            message,
            context: Box::new(context),
        }
    }

    /// Enable stdout capture (for testing).
    pub fn with_output_capture(mut self) -> Self {
        self.io_policy.stdout = IoMode::Capture;
        self.configure_io_buffers();
        self
    }

    /// Enable stderr capture (for testing).
    pub fn with_error_capture(mut self) -> Self {
        self.io_policy.stderr = IoMode::Capture;
        self.configure_io_buffers();
        self
    }

    /// Replace stdout/stderr handling policy.
    pub fn with_io_policy(mut self, policy: VmIoPolicy) -> Self {
        self.io_policy = policy;
        self.configure_io_buffers();
        self
    }

    /// Update stdout/stderr handling policy.
    pub fn set_io_policy(&mut self, policy: VmIoPolicy) {
        self.io_policy = policy;
        self.configure_io_buffers();
    }

    pub fn io_policy(&self) -> VmIoPolicy {
        self.io_policy
    }

    pub fn stdout_mode(&self) -> IoMode {
        self.io_policy.stdout
    }

    pub fn stderr_mode(&self) -> IoMode {
        self.io_policy.stderr
    }

    pub fn is_stdout_captured(&self) -> bool {
        matches!(self.io_policy.stdout, IoMode::Capture | IoMode::Tee)
    }

    pub fn is_stderr_captured(&self) -> bool {
        matches!(self.io_policy.stderr, IoMode::Capture | IoMode::Tee)
    }

    pub fn is_any_io_captured(&self) -> bool {
        self.is_stdout_captured() || self.is_stderr_captured()
    }

    pub fn captured_stdout(&self) -> Option<&[String]> {
        self.output.as_deref()
    }

    pub fn captured_stderr(&self) -> Option<&[String]> {
        self.error_output.as_deref()
    }

    pub fn captured_io_snapshot(&self) -> VmCapturedIo {
        VmCapturedIo {
            stdout: self.output.clone().unwrap_or_default(),
            stderr: self.error_output.clone().unwrap_or_default(),
        }
    }

    /// Drain captured stdout lines and keep capture active.
    pub fn take_stdout(&mut self) -> Vec<String> {
        match self.output.as_mut() {
            Some(buffer) => std::mem::take(buffer),
            None => Vec::new(),
        }
    }

    /// Drain captured stderr lines and keep capture active.
    pub fn take_stderr(&mut self) -> Vec<String> {
        match self.error_output.as_mut() {
            Some(buffer) => std::mem::take(buffer),
            None => Vec::new(),
        }
    }

    pub fn reset_captured_io(&mut self) {
        if let Some(stdout) = self.output.as_mut() {
            stdout.clear();
        }
        if let Some(stderr) = self.error_output.as_mut() {
            stderr.clear();
        }
        self.test_stdout_cursor = 0;
        self.test_stderr_cursor = 0;
    }

    pub(crate) fn emit_stdout_line(&mut self, line: String) {
        match self.io_policy.stdout {
            IoMode::Passthrough => println!("{}", line),
            IoMode::Capture => {
                if let Some(buffer) = self.output.as_mut() {
                    buffer.push(line);
                } else {
                    println!("{}", line);
                }
            }
            IoMode::Tee => {
                println!("{}", line);
                if let Some(buffer) = self.output.as_mut() {
                    buffer.push(line);
                }
            }
        }
    }

    pub(crate) fn emit_stderr_line(&mut self, line: String) {
        match self.io_policy.stderr {
            IoMode::Passthrough => eprintln!("{}", line),
            IoMode::Capture => {
                if let Some(buffer) = self.error_output.as_mut() {
                    buffer.push(line);
                } else {
                    eprintln!("{}", line);
                }
            }
            IoMode::Tee => {
                eprintln!("{}", line);
                if let Some(buffer) = self.error_output.as_mut() {
                    buffer.push(line);
                }
            }
        }
    }

    fn configure_io_buffers(&mut self) {
        if matches!(self.io_policy.stdout, IoMode::Capture | IoMode::Tee) {
            if self.output.is_none() {
                self.output = Some(Vec::new());
            }
        } else {
            self.output = None;
        }

        if matches!(self.io_policy.stderr, IoMode::Capture | IoMode::Tee) {
            if self.error_output.is_none() {
                self.error_output = Some(Vec::new());
            }
        } else {
            self.error_output = None;
        }
    }

    fn current_output_len(&self) -> usize {
        self.output.as_ref().map(Vec::len).unwrap_or(0)
    }

    fn current_error_output_len(&self) -> usize {
        self.error_output.as_ref().map(Vec::len).unwrap_or(0)
    }

    fn next_test_event_io(&mut self) -> Option<VmCapturedIo> {
        if self.output.is_none() && self.error_output.is_none() {
            return None;
        }

        let stdout_len = self.current_output_len();
        let stderr_len = self.current_error_output_len();

        let stdout = self
            .output
            .as_ref()
            .map(|buffer| {
                let start = self.test_stdout_cursor.min(buffer.len());
                buffer[start..].to_vec()
            })
            .unwrap_or_default();
        let stderr = self
            .error_output
            .as_ref()
            .map(|buffer| {
                let start = self.test_stderr_cursor.min(buffer.len());
                buffer[start..].to_vec()
            })
            .unwrap_or_default();

        self.test_stdout_cursor = stdout_len;
        self.test_stderr_cursor = stderr_len;

        Some(VmCapturedIo { stdout, stderr })
    }

    /// Access the type registry (used by builtins).
    pub fn type_registry(&self) -> &TypeRegistry {
        &self.bytecode.type_registry
    }

    /// Read-only access to the accumulated bytecode.
    pub fn bytecode(&self) -> &Bytecode {
        &self.bytecode
    }

    /// Snapshot the current accumulated bytecode as a serialisable `Bytecode`.
    ///
    /// In interactive/REPL mode the VM grows its local frame dynamically, so
    /// `self.bytecode.num_locals` (initialised to 0) does not reflect the
    /// actual frame size.  This method returns a corrected clone suitable for
    /// `Bytecode::encode()`.
    pub fn snapshot_bytecode(&self) -> Bytecode {
        let actual_num_locals = self.frames.first().map(|f| f.locals.len()).unwrap_or(0);
        let mut bc = self.bytecode.clone();
        bc.num_locals = actual_num_locals;
        bc
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn pc(&self) -> usize {
        self.pc
    }

    pub fn stack_depth(&self) -> usize {
        self.stack.len()
    }

    pub fn frame_depth(&self) -> usize {
        self.frames.len()
    }

    pub fn set_exit_code(&mut self, exit_code: i32) {
        self.exit_code = exit_code;
    }

    pub fn last_value(&self) -> Option<&Value> {
        self.last_result.as_ref()
    }

    pub fn test_events(&self) -> &[VmTestEvent] {
        &self.test_events
    }

    pub(crate) fn push_test_scope(&mut self, _kind: &str, name: String) {
        if self.test_scope.is_empty() {
            self.test_stdout_cursor = self.current_output_len();
            self.test_stderr_cursor = self.current_error_output_len();
        }
        self.test_scope.push(name);
    }

    pub(crate) fn pop_test_scope(&mut self) -> Result<(), RuntimeError> {
        self.test_scope
            .pop()
            .map(|_| ())
            .ok_or_else(|| RuntimeError::new("test scope stack underflow"))
    }

    pub(crate) fn record_test_pass(&mut self, name: String) {
        let mut path = self.test_scope.clone();
        path.push(name);
        let io = self.next_test_event_io();
        self.test_events.push(VmTestEvent {
            path,
            detail: None,
            kind: VmTestEventKind::Passed,
            io,
        });
    }

    pub(crate) fn record_test_fail(&mut self, name: String, detail: String) {
        let mut path = self.test_scope.clone();
        path.push(name);
        let io = self.next_test_event_io();
        self.test_events.push(VmTestEvent {
            path,
            detail: Some(detail),
            kind: VmTestEventKind::Failed,
            io,
        });
    }

    pub(crate) fn record_current_scope_fail(&mut self, detail: String) {
        let io = self.next_test_event_io();
        self.test_events.push(VmTestEvent {
            path: self.test_scope.clone(),
            detail: Some(detail),
            kind: VmTestEventKind::Failed,
            io,
        });
    }

    pub fn enable_observation(&mut self, options: VmObservationOptions) {
        self.observer = Some(VmObserver::new(options));
    }

    pub fn observation(&self) -> Option<VmObservation> {
        self.observer.as_ref().map(VmObserver::snapshot)
    }

    /// Read a local slot value (used by REPL display logic).
    pub fn get_local(&self, slot: u32) -> Option<Value> {
        self.frames
            .last()
            .and_then(|frame| frame.locals.get(slot as usize).cloned())
    }

    /// Execute the loaded bytecode (`run` mode expects `Halt`).
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        self.verify_loaded_bytecode()?;
        self.last_result = None;
        self.test_scope.clear();
        self.test_events.clear();
        self.test_stdout_cursor = self.current_output_len();
        self.test_stderr_cursor = self.current_error_output_len();
        loop {
            if self.pc >= self.bytecode.opcodes.len() {
                return Err(RuntimeError::new("PC out of bounds"));
            }
            let op = self.bytecode.opcodes[self.pc].clone();
            self.observe_opcode_step(self.pc, &op);
            let mut next_pc = self.pc + 1;
            let halted = self
                .execute_opcode(op.clone(), &mut next_pc)
                .map_err(|err| self.enrich_runtime_error(err, self.pc, &op))?;
            self.pc = next_pc;
            self.observe_current_depths();

            if halted {
                self.last_result = Some(self.stack.last().cloned().unwrap_or(Value::Unit));
                return Ok(());
            }
        }
    }

    /// Execute an incremental bytecode chunk and return the final stack top.
    /// If the stack is empty at the end, returns `Unit`.
    ///
    /// Contract with Forge `codegen_chunk`:
    /// - chunk opcode indices are chunk-local (`LoadConst` / `MakeError`)
    /// - chunk `source_map` indices are chunk-local
    /// - this method relocates them using `const_base` / `error_template_base`
    /// - top-level execution starts at appended `code_base` and stops at first `Halt`
    fn execute_chunk(&mut self, chunk: BytecodeChunk) -> Result<Value, RuntimeError> {
        let BytecodeChunk {
            opcodes,
            source_map,
            const_base: chunk_const_base,
            constants,
            new_locals,
            type_entries,
            error_template_base: chunk_error_template_base,
            error_templates,
            functions,
            docs,
        } = chunk;
        let code_base = self.bytecode.opcodes.len();
        let const_base = self.bytecode.constants.len();
        let error_template_base = self.bytecode.error_templates.len();
        if chunk_const_base as usize != const_base {
            return Err(RuntimeError::new(format!(
                "Chunk constant base mismatch: chunk={}, vm={}",
                chunk_const_base, const_base
            )));
        }
        if chunk_error_template_base as usize != error_template_base {
            return Err(RuntimeError::new(format!(
                "Chunk error template base mismatch: chunk={}, vm={}",
                chunk_error_template_base, error_template_base
            )));
        }
        let mut chunk_opcodes = opcodes;
        Self::relocate_chunk_indices(
            &mut chunk_opcodes,
            code_base,
            const_base,
            error_template_base,
        )?;
        self.bytecode.constants.extend(constants);
        self.bytecode.type_registry.entries.extend(type_entries);
        self.bytecode.error_templates.extend(error_templates);
        self.extend_docs_unique(docs);
        self.bytecode.opcodes.extend(chunk_opcodes);
        self.relocate_and_extend_source_map(source_map, code_base)?;
        // Invariant: runtime uses O(1) lookup `functions[fun_idx as usize]`.
        // Chunk application may append a new slot or replace an existing slot, but never create holes.
        for mut entry in functions {
            entry.entry_pc += code_base as u32;
            let idx = entry.fun_idx as usize;
            if idx == self.bytecode.functions.len() {
                self.bytecode.functions.push(entry);
            } else if idx < self.bytecode.functions.len() {
                self.bytecode.functions[idx] = entry;
            } else {
                return Err(RuntimeError::new(format!(
                    "Function table invariant violated in chunk: fun_idx {} > len {}",
                    idx,
                    self.bytecode.functions.len()
                )));
            }
        }
        if let Some(frame) = self.frames.first_mut() {
            frame
                .locals
                .extend(std::iter::repeat_n(Value::Unit, new_locals));
        }

        let mut pc = code_base;
        while pc < self.bytecode.opcodes.len() {
            let current_pc = pc;
            let op = self.bytecode.opcodes[pc].clone();
            self.observe_opcode_step(current_pc, &op);
            pc += 1;
            let halted = self
                .execute_opcode(op.clone(), &mut pc)
                .map_err(|err| self.enrich_runtime_error(err, current_pc, &op))?;
            self.observe_current_depths();
            if halted {
                break;
            }
        }

        let result = self.stack.pop().unwrap_or(Value::Unit);
        self.last_result = Some(result.clone());
        self.stack.clear();
        Ok(result)
    }

    /// Execute a chunk atomically, preserving the existing VM state on failure.
    pub fn push_atomic(&mut self, chunk: BytecodeChunk) -> Result<Value, RuntimeError> {
        self.verify_chunk(&chunk)?;
        let checkpoint = self.checkpoint_for_chunk(&chunk);
        let result = match self.execute_chunk(chunk) {
            Ok(result) => result,
            Err(err) => {
                self.rollback_to_checkpoint(checkpoint);
                return Err(err);
            }
        };

        if let Err(err) = self.verify_loaded_bytecode() {
            self.rollback_to_checkpoint(checkpoint);
            return Err(err);
        }

        Ok(result)
    }

    fn checkpoint_for_chunk(&self, chunk: &BytecodeChunk) -> VmCheckpoint {
        let overwritten_functions = chunk
            .functions
            .iter()
            .filter_map(|entry| {
                let idx = entry.fun_idx as usize;
                self.bytecode
                    .functions
                    .get(idx)
                    .cloned()
                    .map(|existing| (idx, existing))
            })
            .collect();

        VmCheckpoint {
            stack: self.stack.clone(),
            frames: self.frames.clone(),
            pc: self.pc,
            exit_code: self.exit_code,
            last_result: self.last_result.clone(),
            output_len: self.output.as_ref().map(Vec::len),
            error_output_len: self.error_output.as_ref().map(Vec::len),
            test_scope_len: self.test_scope.len(),
            test_event_len: self.test_events.len(),
            test_stdout_cursor: self.test_stdout_cursor,
            test_stderr_cursor: self.test_stderr_cursor,
            opcode_len: self.bytecode.opcodes.len(),
            constant_len: self.bytecode.constants.len(),
            type_entry_len: self.bytecode.type_registry.entries.len(),
            error_template_len: self.bytecode.error_templates.len(),
            function_len: self.bytecode.functions.len(),
            doc_len: self.bytecode.docs.len(),
            source_map_len: self
                .bytecode
                .source_map
                .as_ref()
                .map(|map| map.entries.len()),
            overwritten_functions,
        }
    }

    fn rollback_to_checkpoint(&mut self, checkpoint: VmCheckpoint) {
        self.stack = checkpoint.stack;
        self.frames = checkpoint.frames;
        self.pc = checkpoint.pc;
        self.exit_code = checkpoint.exit_code;
        self.last_result = checkpoint.last_result;

        if let (Some(buf), Some(len)) = (self.output.as_mut(), checkpoint.output_len) {
            buf.truncate(len);
        }
        if let (Some(buf), Some(len)) = (self.error_output.as_mut(), checkpoint.error_output_len) {
            buf.truncate(len);
        }
        self.test_scope.truncate(checkpoint.test_scope_len);
        self.test_events.truncate(checkpoint.test_event_len);
        self.test_stdout_cursor = checkpoint.test_stdout_cursor;
        self.test_stderr_cursor = checkpoint.test_stderr_cursor;

        self.bytecode.opcodes.truncate(checkpoint.opcode_len);
        self.bytecode.constants.truncate(checkpoint.constant_len);
        self.bytecode
            .type_registry
            .entries
            .truncate(checkpoint.type_entry_len);
        self.bytecode
            .error_templates
            .truncate(checkpoint.error_template_len);
        self.bytecode.functions.truncate(checkpoint.function_len);
        self.bytecode.docs.truncate(checkpoint.doc_len);
        for (idx, entry) in checkpoint.overwritten_functions {
            if idx < self.bytecode.functions.len() {
                self.bytecode.functions[idx] = entry;
            }
        }

        match checkpoint.source_map_len {
            Some(len) => {
                if let Some(source_map) = self.bytecode.source_map.as_mut() {
                    source_map.entries.truncate(len);
                }
            }
            None => self.bytecode.source_map = None,
        }
    }

    fn verify_loaded_bytecode(&self) -> Result<(), RuntimeError> {
        Self::verify_program(&self.bytecode)
    }

    fn extend_docs_unique(&mut self, docs: Vec<DocEntry>) {
        for doc in docs {
            let exists = self.bytecode.docs.iter().any(|existing| existing == &doc);
            if !exists {
                self.bytecode.docs.push(doc);
            }
        }
    }

    fn observe_opcode_step(&mut self, pc: usize, opcode: &Opcode) {
        let stack_depth = self.stack.len();
        let frame_depth = self.frames.len();
        if let Some(observer) = self.observer.as_mut() {
            observer.record_opcode_step(pc, opcode, stack_depth, frame_depth);
        }
    }

    fn observe_current_depths(&mut self) {
        let stack_depth = self.stack.len();
        let frame_depth = self.frames.len();
        if let Some(observer) = self.observer.as_mut() {
            observer.record_depths(stack_depth, frame_depth);
        }
    }

    fn observe_call_event(&mut self, kind: &str, line: String) {
        if let Some(observer) = self.observer.as_mut() {
            match kind {
                "CallBuiltin" => observer.stats.builtin_calls += 1,
                "Call" => observer.stats.function_calls += 1,
                "CallClosure" => observer.stats.closure_calls += 1,
                "Return" => observer.stats.return_count += 1,
                _ => {}
            }
            observer.record_call_event(kind, line);
        }
    }

    fn observe_tail_call_optimized(&mut self) {
        if let Some(observer) = self.observer.as_mut() {
            observer.stats.tail_calls_optimized += 1;
        }
    }

    fn can_optimize_tail_call(&self, next_pc: usize) -> bool {
        self.frames.len() > 1 && matches!(self.bytecode.opcodes.get(next_pc), Some(Opcode::Return))
    }

    fn reuse_current_frame_for_call(
        &mut self,
        locals: Vec<Value>,
        call_site: Option<(u32, u32)>,
    ) -> Result<(), RuntimeError> {
        let frame = self.current_frame_mut()?;
        frame.locals = locals;
        frame.call_site = call_site;
        Ok(())
    }

    fn verify_program(bytecode: &Bytecode) -> Result<(), RuntimeError> {
        Self::verify_type_registry_entries(&bytecode.type_registry.entries, None)?;
        Self::verify_source_map_entries(bytecode.source_map.as_ref(), bytecode.opcodes.len(), "")?;

        let halt_pos = if let Some(pos) = bytecode
            .opcodes
            .iter()
            .position(|op| matches!(op, Opcode::Halt))
        {
            pos
        } else if bytecode
            .opcodes
            .iter()
            .any(|op| matches!(op, Opcode::Return))
        {
            return Err(RuntimeError::new("Return at top-level"));
        } else {
            return Err(RuntimeError::new("Bytecode verifier: missing Halt"));
        };

        for (idx, op) in bytecode.opcodes.iter().enumerate() {
            match op {
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    if *addr as usize >= bytecode.opcodes.len() {
                        return Err(RuntimeError::new(format!("Invalid jump target: {}", addr)));
                    }
                }
                Opcode::LoadConst(idx) => {
                    if *idx as usize >= bytecode.constants.len() {
                        return Err(RuntimeError::new(format!(
                            "LoadConst index out of bounds: {}",
                            idx
                        )));
                    }
                }
                Opcode::MakeError { template_id } => {
                    if *template_id as usize >= bytecode.error_templates.len() {
                        return Err(RuntimeError::new(format!(
                            "Unknown error template: {}",
                            template_id
                        )));
                    }
                }
                Opcode::Return if idx <= halt_pos => {
                    return Err(RuntimeError::new("Return at top-level"));
                }
                _ => {}
            }
        }

        for (idx, entry) in bytecode.functions.iter().enumerate() {
            if entry.fun_idx as usize != idx {
                return Err(RuntimeError::new(format!(
                    "Function table invariant violated: functions[{}].fun_idx = {}",
                    idx, entry.fun_idx
                )));
            }
            if entry.entry_pc as usize >= bytecode.opcodes.len() {
                return Err(RuntimeError::new(format!(
                    "Function {} entry_pc out of bounds: {}",
                    entry.fun_idx, entry.entry_pc
                )));
            }
            if entry.entry_pc as usize <= halt_pos {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: function {} entry_pc {} must be after top-level Halt {}",
                    entry.fun_idx, entry.entry_pc, halt_pos
                )));
            }
        }

        Ok(())
    }

    fn verify_chunk(&self, chunk: &BytecodeChunk) -> Result<(), RuntimeError> {
        Self::verify_type_registry_entries(
            &chunk.type_entries,
            Some(&self.bytecode.type_registry.entries),
        )?;
        Self::verify_source_map_entries(chunk.source_map.as_ref(), chunk.opcodes.len(), "chunk")?;

        let const_base = self.bytecode.constants.len();
        if chunk.const_base as usize != const_base {
            return Err(RuntimeError::new(format!(
                "Chunk constant base mismatch: chunk={}, vm={}",
                chunk.const_base, const_base
            )));
        }

        let error_template_base = self.bytecode.error_templates.len();
        if chunk.error_template_base as usize != error_template_base {
            return Err(RuntimeError::new(format!(
                "Chunk error template base mismatch: chunk={}, vm={}",
                chunk.error_template_base, error_template_base
            )));
        }

        let halt_pos = chunk
            .opcodes
            .iter()
            .position(|op| matches!(op, Opcode::Halt))
            .ok_or_else(|| RuntimeError::new("Bytecode verifier: chunk missing Halt"))?;

        for (idx, op) in chunk.opcodes.iter().enumerate() {
            match op {
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    if *addr as usize >= chunk.opcodes.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk jump target out of bounds: {}",
                            addr
                        )));
                    }
                }
                Opcode::LoadConst(idx) => {
                    if *idx as usize >= chunk.constants.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk LoadConst index out of bounds: {}",
                            idx
                        )));
                    }
                }
                Opcode::MakeError { template_id } => {
                    if *template_id as usize >= chunk.error_templates.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk error template out of bounds: {}",
                            template_id
                        )));
                    }
                }
                Opcode::Return if idx <= halt_pos => {
                    return Err(RuntimeError::new("Return at top-level"));
                }
                _ => {}
            }
        }

        let mut seen_fun_idxs = BTreeSet::new();
        let mut next_append_idx = self.bytecode.functions.len();
        let mut sorted_entries = chunk.functions.iter().collect::<Vec<_>>();
        sorted_entries.sort_by_key(|entry| entry.fun_idx);

        for entry in sorted_entries {
            let idx = entry.fun_idx as usize;
            if !seen_fun_idxs.insert(entry.fun_idx) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: duplicate function entry for fun_idx {}",
                    entry.fun_idx
                )));
            }
            if idx > next_append_idx {
                return Err(RuntimeError::new(format!(
                    "Function table invariant violated in chunk: fun_idx {} > len {}",
                    idx, next_append_idx
                )));
            }
            if idx == next_append_idx {
                next_append_idx += 1;
            }
            if entry.entry_pc as usize >= chunk.opcodes.len() {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: function {} entry_pc out of chunk bounds: {}",
                    entry.fun_idx, entry.entry_pc
                )));
            }
            if entry.entry_pc as usize <= halt_pos {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: function {} entry_pc {} must be after top-level Halt {}",
                    entry.fun_idx, entry.entry_pc, halt_pos
                )));
            }
        }

        Ok(())
    }

    fn verify_type_registry_entries(
        entries: &[sindr::runtime::TypeEntry],
        existing: Option<&[sindr::runtime::TypeEntry]>,
    ) -> Result<(), RuntimeError> {
        let mut seen_tags = BTreeSet::new();
        if let Some(existing) = existing {
            for entry in existing {
                seen_tags.insert(entry.tag);
            }
        }

        for entry in entries {
            if matches!(entry.tag, 0 | 1) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: reserved result tag reused in TypeRegistry: {}",
                    entry.tag
                )));
            }

            if !seen_tags.insert(entry.tag) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: duplicate type tag in TypeRegistry: {}",
                    entry.tag
                )));
            }
        }

        Ok(())
    }

    fn verify_source_map_entries(
        source_map: Option<&SourceMap>,
        opcode_len: usize,
        context: &str,
    ) -> Result<(), RuntimeError> {
        let Some(source_map) = source_map else {
            return Ok(());
        };
        let mut seen_opcode_indices = BTreeSet::new();
        for entry in &source_map.entries {
            if entry.opcode_index as usize >= opcode_len {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: {}source_map opcode_index out of bounds: {}",
                    if context.is_empty() { "" } else { "chunk " },
                    entry.opcode_index
                )));
            }

            if !seen_opcode_indices.insert(entry.opcode_index) {
                return Err(RuntimeError::new(format!(
                    "Bytecode verifier: {}duplicate source_map entry for opcode_index {}",
                    if context.is_empty() { "" } else { "chunk " },
                    entry.opcode_index
                )));
            }
        }

        Ok(())
    }

    fn relocate_and_extend_source_map(
        &mut self,
        source_map: Option<SourceMap>,
        code_base: usize,
    ) -> Result<(), RuntimeError> {
        let Some(mut source_map) = source_map else {
            return Ok(());
        };

        let code_base =
            u32::try_from(code_base).map_err(|_| RuntimeError::new("opcode count exceeds u32"))?;
        for entry in &mut source_map.entries {
            entry.opcode_index = entry
                .opcode_index
                .checked_add(code_base)
                .ok_or_else(|| RuntimeError::new("source_map opcode index overflow"))?;
        }

        match self.bytecode.source_map.as_mut() {
            Some(existing) => existing.entries.extend(source_map.entries),
            None => self.bytecode.source_map = Some(source_map),
        }

        Ok(())
    }

    fn relocate_chunk_indices(
        opcodes: &mut [Opcode],
        code_base: usize,
        const_base: usize,
        error_template_base: usize,
    ) -> Result<(), RuntimeError> {
        let code_base = u32::try_from(code_base).map_err(|_| {
            RuntimeError::new(format!(
                "Code base too large for jump relocation: {}",
                code_base
            ))
        })?;
        let const_base = u32::try_from(const_base).map_err(|_| {
            RuntimeError::new(format!(
                "Constant base too large for relocation: {}",
                const_base
            ))
        })?;
        let error_template_base = u32::try_from(error_template_base).map_err(|_| {
            RuntimeError::new(format!(
                "Error template base too large for relocation: {}",
                error_template_base
            ))
        })?;

        for op in opcodes.iter_mut() {
            match op {
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    *addr = addr.checked_add(code_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Jump relocation overflow: target {} + base {}",
                            *addr, code_base
                        ))
                    })?;
                }
                Opcode::LoadConst(idx) => {
                    *idx = idx.checked_add(const_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Const relocation overflow: index {} + base {}",
                            *idx, const_base
                        ))
                    })?;
                }
                Opcode::MakeError { template_id } => {
                    *template_id =
                        template_id
                            .checked_add(error_template_base)
                            .ok_or_else(|| {
                                RuntimeError::new(format!(
                                    "Error template relocation overflow: id {} + base {}",
                                    *template_id, error_template_base
                                ))
                            })?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn execute_opcode(&mut self, op: Opcode, pc: &mut usize) -> Result<bool, RuntimeError> {
        match op {
            Opcode::Halt => return Ok(true),

            Opcode::LoadConst(idx) => {
                let c = self.bytecode.constants.get(idx as usize).ok_or_else(|| {
                    RuntimeError::new(format!("LoadConst index out of bounds: {}", idx))
                })?;
                let val = match c {
                    Constant::Int(n) => Value::Int(n.clone()),
                    Constant::Tag(tag) => Value::Tag(*tag),
                    Constant::Float(f) => Value::Float(*f),
                    Constant::Str(s) => Value::Str(s.clone()),
                    Constant::Bool(b) => Value::Bool(*b),
                    Constant::Unit => Value::Unit,
                };
                self.stack.push(val);
            }

            Opcode::LoadBuiltinRef(builtin_id) => {
                self.stack.push(Value::Callable(Callable {
                    target: CallableTarget::Builtin(builtin_id),
                    lexical_captures: Vec::new(),
                    partial_args: Vec::new(),
                }));
            }

            Opcode::LoadFunctionRef(fun_idx) => {
                self.stack.push(Value::Callable(Callable {
                    target: CallableTarget::Function(fun_idx),
                    lexical_captures: Vec::new(),
                    partial_args: Vec::new(),
                }));
            }

            Opcode::LoadLocal(slot) => {
                let val = self
                    .current_frame()?
                    .locals
                    .get(slot as usize)
                    .cloned()
                    .ok_or_else(|| {
                        RuntimeError::new(format!("LoadLocal out of bounds: {}", slot))
                    })?;
                self.stack.push(val);
            }

            Opcode::StoreLocal(slot) => {
                let val = self.pop_stack()?;
                let target = self
                    .current_frame_mut()?
                    .locals
                    .get_mut(slot as usize)
                    .ok_or_else(|| {
                        RuntimeError::new(format!("StoreLocal out of bounds: {}", slot))
                    })?;
                *target = val;
            }

            Opcode::Pop => {
                self.pop_stack()?;
            }

            // Arithmetic (Int)
            Opcode::AddInt => self.int_binop(|a, b| Ok(Value::Int(a + b)))?,
            Opcode::SubInt => self.int_binop(|a, b| Ok(Value::Int(a - b)))?,
            Opcode::MulInt => self.int_binop(|a, b| Ok(Value::Int(a * b)))?,
            Opcode::BitNotInt => {
                let a = self.pop_int()?;
                self.stack.push(Value::Int(!a));
            }
            Opcode::BitAndInt => self.int_binop(|a, b| Ok(Value::Int(a & b)))?,
            Opcode::BitOrInt => self.int_binop(|a, b| Ok(Value::Int(a | b)))?,
            Opcode::BitXorInt => self.int_binop(|a, b| Ok(Value::Int(a ^ b)))?,

            // Arithmetic (Float)
            Opcode::AddFloat => self.float_binop(|a, b| Value::Float(a + b))?,
            Opcode::SubFloat => self.float_binop(|a, b| Value::Float(a - b))?,
            Opcode::MulFloat => self.float_binop(|a, b| Value::Float(a * b))?,

            // Comparison (Int)
            Opcode::EqInt => self.int_binop(|a, b| Ok(Value::Bool(a == b)))?,
            Opcode::NeqInt => self.int_binop(|a, b| Ok(Value::Bool(a != b)))?,
            Opcode::LtInt => self.int_binop(|a, b| Ok(Value::Bool(a < b)))?,
            Opcode::GtInt => self.int_binop(|a, b| Ok(Value::Bool(a > b)))?,
            Opcode::LteInt => self.int_binop(|a, b| Ok(Value::Bool(a <= b)))?,
            Opcode::GteInt => self.int_binop(|a, b| Ok(Value::Bool(a >= b)))?,

            // Comparison (Float)
            Opcode::EqFloat => self.float_binop(|a, b| Value::Bool(a == b))?,
            Opcode::NeqFloat => self.float_binop(|a, b| Value::Bool(a != b))?,
            Opcode::LtFloat => self.float_binop(|a, b| Value::Bool(a < b))?,
            Opcode::GtFloat => self.float_binop(|a, b| Value::Bool(a > b))?,
            Opcode::LteFloat => self.float_binop(|a, b| Value::Bool(a <= b))?,
            Opcode::GteFloat => self.float_binop(|a, b| Value::Bool(a >= b))?,

            // Comparison (String)
            Opcode::EqStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Bool(a == b));
            }
            Opcode::NeqStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Bool(a != b));
            }

            // Comparison (Bool)
            Opcode::EqBool => {
                let b = self.pop_bool()?;
                let a = self.pop_bool()?;
                self.stack.push(Value::Bool(a == b));
            }
            Opcode::NeqBool => {
                let b = self.pop_bool()?;
                let a = self.pop_bool()?;
                self.stack.push(Value::Bool(a != b));
            }

            // String
            Opcode::ConcatStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Str(a + &b));
            }
            Opcode::StringIsEmpty => {
                let value = self.pop_str()?;
                self.stack.push(Value::Bool(value.is_empty()));
            }
            Opcode::StringHead => {
                let value = self.pop_str()?;
                let mut chars = value.chars();
                let head = chars
                    .next()
                    .ok_or_else(|| RuntimeError::new("StringHead on empty string"))?;
                self.stack.push(Value::Str(head.to_string()));
            }
            Opcode::StringTail => {
                let value = self.pop_str()?;
                let mut chars = value.chars();
                chars
                    .next()
                    .ok_or_else(|| RuntimeError::new("StringTail on empty string"))?;
                self.stack.push(Value::Str(chars.collect()));
            }

            // Unary
            Opcode::NegInt => {
                let a = self.pop_int()?;
                self.stack.push(Value::Int(-a));
            }
            Opcode::NegFloat => {
                let a = self.pop_float()?;
                self.stack.push(Value::Float(-a));
            }
            Opcode::NotBool => {
                let a = self.pop_bool()?;
                self.stack.push(Value::Bool(!a));
            }

            // List
            Opcode::ListNew { len } => {
                let mut elems = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    elems.push(self.pop_stack()?);
                }
                elems.reverse();
                self.stack.push(Value::List(ListHandle::from_items(elems)));
            }
            Opcode::ListEmpty => {
                self.stack.push(Value::List(ListHandle::empty()));
            }
            Opcode::ListNil => {
                self.stack.push(Value::List(ListHandle::empty()));
            }
            Opcode::ListCons => {
                let tail = self.pop_stack()?;
                let head = self.pop_stack()?;
                match tail {
                    Value::List(handle) => {
                        self.stack
                            .push(Value::List(ListHandle::cons(head, &handle)));
                    }
                    other => {
                        return Err(RuntimeError::new(format!(
                            "ListCons expects list tail, got {:?}",
                            other
                        )));
                    }
                }
            }
            Opcode::ListIsEmpty => {
                let list = self.pop_stack()?;
                match list {
                    Value::List(handle) => self.stack.push(Value::Bool(handle.is_empty())),
                    other => {
                        return Err(RuntimeError::new(format!(
                            "ListIsEmpty expects List, got {:?}",
                            other
                        )));
                    }
                }
            }
            Opcode::ListHead => {
                let list = self.pop_stack()?;
                match list {
                    Value::List(handle) => {
                        let head = handle
                            .head_value()
                            .ok_or_else(|| RuntimeError::new("ListHead on empty list"))?;
                        self.stack.push(head);
                    }
                    other => {
                        return Err(RuntimeError::new(format!(
                            "ListHead expects List, got {:?}",
                            other
                        )));
                    }
                }
            }
            Opcode::ListTail => {
                let list = self.pop_stack()?;
                match list {
                    Value::List(handle) => {
                        let tail = handle
                            .tail_handle()
                            .ok_or_else(|| RuntimeError::new("ListTail on empty list"))?;
                        self.stack.push(Value::List(tail));
                    }
                    other => {
                        return Err(RuntimeError::new(format!(
                            "ListTail expects List, got {:?}",
                            other
                        )));
                    }
                }
            }
            Opcode::ListFromItems { len } => {
                let mut elems = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    elems.push(self.pop_stack()?);
                }
                elems.reverse();
                self.stack.push(Value::List(ListHandle::from_items(elems)));
            }

            // Tuple
            Opcode::TupleNew { len } => {
                let mut elems = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    elems.push(self.pop_stack()?);
                }
                elems.reverse();
                self.stack.push(Value::Tuple(elems));
            }
            Opcode::GetTupleField { field_index } => {
                let tuple = self.pop_stack()?;
                match tuple {
                    Value::Tuple(items) => {
                        let field = items.get(field_index as usize).cloned().ok_or_else(|| {
                            RuntimeError::new(format!("Tuple index {} out of bounds", field_index))
                        })?;
                        self.stack.push(field);
                    }
                    other => {
                        return Err(RuntimeError::new(format!(
                            "GetTupleField expects Tuple, got {:?}",
                            other
                        )));
                    }
                }
            }

            // Struct / Tagged
            Opcode::StructNew { field_count } => {
                let mut fields = Vec::with_capacity(field_count as usize);
                for _ in 0..field_count {
                    fields.push(self.pop_stack()?);
                }
                fields.reverse();
                let tag_val = self.pop_stack()?;
                let tag = match tag_val {
                    Value::Tag(tag) => tag,
                    other => {
                        return Err(RuntimeError::new(format!(
                            "StructNew: expected Tag, got {:?}",
                            other
                        )));
                    }
                };
                self.stack.push(Value::Tagged { tag, fields });
            }
            Opcode::GetField { field_index } => {
                let val = self.pop_stack()?;
                match val {
                    Value::Tagged { fields, .. } => {
                        let field = fields.get(field_index as usize).cloned().ok_or_else(|| {
                            RuntimeError::new(format!("Field index {} out of bounds", field_index))
                        })?;
                        self.stack.push(field);
                    }
                    _ => {
                        return Err(RuntimeError::new("GetField on non-tagged value"));
                    }
                }
            }
            Opcode::GetTag => {
                let val = self.pop_stack()?;
                match val {
                    Value::Tagged { tag, .. } => self.stack.push(Value::Tag(tag)),
                    _ => {
                        return Err(RuntimeError::new("GetTag on non-tagged value"));
                    }
                }
            }
            Opcode::EqTag => {
                let b = self.pop_tag()?;
                let a = self.pop_tag()?;
                self.stack.push(Value::Bool(a == b));
            }

            // Built-in function call
            Opcode::CallBuiltin {
                builtin_id,
                arity,
                span_start,
                span_end,
            } => {
                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();
                let builtin_name = builtin_meta_by_id(builtin_id)
                    .map(|meta| meta.name)
                    .unwrap_or("<unknown>");
                self.observe_call_event(
                    "CallBuiltin",
                    format!(
                        "call pc={} kind=CallBuiltin target={} arity={} stack_depth={} frame_depth={}",
                        (*pc).saturating_sub(1),
                        builtin_name,
                        arity,
                        self.stack.len(),
                        self.frames.len()
                    ),
                );
                let result = self.with_call_site(Some((span_start, span_end)), |vm| {
                    call_builtin(vm, builtin_id, args)
                })?;
                self.stack.push(result);
            }

            Opcode::Call {
                fun_idx,
                arity,
                span_start,
                span_end,
            } => {
                let entry = self.function_entry(fun_idx)?.clone();
                if entry.arity != arity {
                    return Err(RuntimeError::new(format!(
                        "Call arity mismatch for function {}: expected {}, got {}",
                        fun_idx, entry.arity, arity
                    )));
                }

                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();

                if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                    return Err(RuntimeError::new(format!(
                        "Function {} entry_pc out of bounds: {}",
                        fun_idx, entry.entry_pc
                    )));
                }

                let locals = Self::build_locals_for_call(&entry, args)?;
                let tail_call = self.can_optimize_tail_call(*pc);
                let frame_depth = if tail_call {
                    self.frames.len()
                } else {
                    self.frames.len() + 1
                };
                self.observe_call_event(
                    "Call",
                    format!(
                        "call pc={} kind=Call target=fun#{} arity={} stack_depth={} frame_depth={}",
                        (*pc).saturating_sub(1),
                        fun_idx,
                        arity,
                        self.stack.len(),
                        frame_depth
                    ),
                );
                if tail_call {
                    self.reuse_current_frame_for_call(locals, Some((span_start, span_end)))?;
                    self.observe_tail_call_optimized();
                } else {
                    let return_pc = *pc;
                    let stack_base = self.stack.len();
                    self.frames.push(CallFrame {
                        return_pc,
                        stack_base,
                        call_site: Some((span_start, span_end)),
                        locals,
                    });
                }
                *pc = entry.entry_pc as usize;
            }

            Opcode::MakeError { template_id } => {
                let message = match self.pop_stack()? {
                    Value::Str(s) => s,
                    other => {
                        return Err(RuntimeError::new(format!(
                            "MakeError expects String, got {:?}",
                            other
                        )));
                    }
                };
                let template = self
                    .bytecode
                    .error_templates
                    .get(template_id as usize)
                    .ok_or_else(|| {
                        RuntimeError::new(format!("Unknown error template: {}", template_id))
                    })?;
                let call_site = self.current_frame()?.call_site;
                let (span_start, span_end) =
                    call_site.unwrap_or((template.span_start, template.span_end));
                let (line, column) = match call_site {
                    Some((span_start, _)) => self
                        .source()
                        .map(|source| line_column_for_offset(source, span_start as usize))
                        .unwrap_or((template.line, template.column)),
                    None => (template.line, template.column),
                };
                let location = Location {
                    file: self
                        .source_file()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "<repl>".to_string()),
                    func: template.kind.clone(),
                    line,
                    column,
                    span_start,
                    span_end,
                };
                self.stack.push(Value::Error(Box::new(RichError::new(
                    template.kind.clone(),
                    message,
                    location,
                    None,
                ))));
            }
            Opcode::MakeErrorLiteral {
                kind_const_idx,
                message_const_idx,
            } => {
                let kind = match self.bytecode.constants.get(kind_const_idx as usize) {
                    Some(Constant::Str(s)) => s.clone(),
                    Some(other) => {
                        return Err(RuntimeError::new(format!(
                            "MakeErrorLiteral kind expects String constant, got {:?}",
                            other
                        )))
                    }
                    None => {
                        return Err(RuntimeError::new(format!(
                            "MakeErrorLiteral kind index out of bounds: {}",
                            kind_const_idx
                        )))
                    }
                };
                let message = match self.bytecode.constants.get(message_const_idx as usize) {
                    Some(Constant::Str(s)) => s.clone(),
                    Some(other) => {
                        return Err(RuntimeError::new(format!(
                            "MakeErrorLiteral message expects String constant, got {:?}",
                            other
                        )))
                    }
                    None => {
                        return Err(RuntimeError::new(format!(
                            "MakeErrorLiteral message index out of bounds: {}",
                            message_const_idx
                        )))
                    }
                };
                let (line, column) = self
                    .source()
                    .map(|source| line_column_for_offset(source, 0))
                    .unwrap_or((0, 0));
                self.stack.push(Value::Error(Box::new(RichError::new(
                    kind,
                    message,
                    Location {
                        file: self
                            .source_file()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "<repl>".to_string()),
                        func: "<pattern>".into(),
                        line,
                        column,
                        span_start: 0,
                        span_end: 0,
                    },
                    None,
                ))));
            }

            Opcode::CaptureClosure(num_captured) => {
                let mut lexical_captures = Vec::with_capacity(num_captured as usize);
                for _ in 0..num_captured {
                    lexical_captures.push(self.pop_stack()?);
                }
                lexical_captures.reverse();
                let target = self.pop_stack()?;
                let callable = match target {
                    Value::Callable(mut callable) => {
                        callable.lexical_captures.extend(lexical_captures);
                        callable
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "CaptureClosure expects a callable target",
                        ));
                    }
                };
                self.stack.push(Value::Callable(callable));
            }

            Opcode::CapturePartial(num_args) => {
                let mut partial_args = Vec::with_capacity(num_args as usize);
                for _ in 0..num_args {
                    partial_args.push(self.pop_stack()?);
                }
                partial_args.reverse();
                let target = self.pop_stack()?;
                let callable = match target {
                    Value::Callable(mut callable) => {
                        callable.partial_args.extend(partial_args);
                        callable
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "CapturePartial expects a callable target",
                        ));
                    }
                };
                self.stack.push(Value::Callable(callable));
            }

            Opcode::CallClosure {
                arity,
                span_start,
                span_end,
            } => {
                let mut args = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    args.push(self.pop_stack()?);
                }
                args.reverse();

                let callable = match self.pop_stack()? {
                    Value::Callable(callable) => callable,
                    _ => {
                        return Err(RuntimeError::new("CallClosure expects a callable value"));
                    }
                };

                let mut full_args = callable.lexical_captures;
                full_args.extend(callable.partial_args);
                full_args.extend(args);

                match callable.target {
                    CallableTarget::Builtin(builtin_id) => {
                        let builtin_name = builtin_meta_by_id(builtin_id)
                            .map(|meta| meta.name)
                            .unwrap_or("<unknown>");
                        self.observe_call_event(
                            "CallClosure",
                            format!(
                                "call pc={} kind=CallClosure target=builtin:{} arity={} stack_depth={} frame_depth={}",
                                (*pc).saturating_sub(1),
                                builtin_name,
                                full_args.len(),
                                self.stack.len(),
                                self.frames.len()
                            ),
                        );
                        let result = self.with_call_site(Some((span_start, span_end)), |vm| {
                            call_builtin(vm, builtin_id, full_args)
                        })?;
                        self.stack.push(result);
                    }
                    CallableTarget::Function(fun_idx) => {
                        let entry = self.function_entry(fun_idx)?.clone();
                        if entry.arity as usize != full_args.len() {
                            return Err(RuntimeError::new(format!(
                                "Call arity mismatch for function {}: expected {}, got {}",
                                fun_idx,
                                entry.arity,
                                full_args.len()
                            )));
                        }
                        if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                            return Err(RuntimeError::new(format!(
                                "Function {} entry_pc out of bounds: {}",
                                fun_idx, entry.entry_pc
                            )));
                        }

                        let locals = Self::build_locals_for_call(&entry, full_args)?;
                        let tail_call = self.can_optimize_tail_call(*pc);
                        let frame_depth = if tail_call {
                            self.frames.len()
                        } else {
                            self.frames.len() + 1
                        };
                        self.observe_call_event(
                            "CallClosure",
                            format!(
                                "call pc={} kind=CallClosure target=function:fun#{} arity={} stack_depth={} frame_depth={}",
                                (*pc).saturating_sub(1),
                                fun_idx,
                                entry.arity,
                                self.stack.len(),
                                frame_depth
                            ),
                        );
                        if tail_call {
                            self.reuse_current_frame_for_call(
                                locals,
                                Some((span_start, span_end)),
                            )?;
                            self.observe_tail_call_optimized();
                        } else {
                            let return_pc = *pc;
                            let stack_base = self.stack.len();
                            self.frames.push(CallFrame {
                                return_pc,
                                stack_base,
                                call_site: Some((span_start, span_end)),
                                locals,
                            });
                        }
                        *pc = entry.entry_pc as usize;
                    }
                }
            }

            // Control flow
            Opcode::Jump(addr) => {
                *pc = self.validate_jump_target(addr)?;
            }
            Opcode::JumpIfFalse(addr) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Bool(false) => {
                        *pc = self.validate_jump_target(addr)?;
                    }
                    Value::Bool(true) => {}
                    _ => {
                        return Err(RuntimeError::new("JumpIfFalse: expected Bool"));
                    }
                }
            }
            Opcode::JumpIfTrue(addr) => {
                let val = self.pop_stack()?;
                match val {
                    Value::Bool(true) => {
                        *pc = self.validate_jump_target(addr)?;
                    }
                    Value::Bool(false) => {}
                    _ => {
                        return Err(RuntimeError::new("JumpIfTrue: expected Bool"));
                    }
                }
            }

            // Return
            Opcode::Return => {
                if self.frames.len() == 1 {
                    return Err(RuntimeError::new("Return at top-level"));
                }

                let ret = self.pop_stack()?;
                let frame = self
                    .frames
                    .pop()
                    .ok_or_else(|| RuntimeError::new("Return with empty frame stack"))?;
                self.stack.truncate(frame.stack_base);
                self.stack.push(ret);
                *pc = frame.return_pc;
                self.observe_call_event(
                    "Return",
                    format!(
                        "return pc={} stack_depth={} frame_depth={}",
                        *pc,
                        self.stack.len(),
                        self.frames.len()
                    ),
                );
            }
        }

        Ok(false)
    }

    fn build_locals_for_call(
        entry: &FunctionEntry,
        args: Vec<Value>,
    ) -> Result<Vec<Value>, RuntimeError> {
        let num_locals = entry.num_locals as usize;
        if num_locals < args.len() {
            return Err(RuntimeError::new(format!(
                "Function {} requires at least {} local slots, got {}",
                entry.fun_idx,
                args.len(),
                num_locals
            )));
        }

        let mut locals = vec![Value::Unit; num_locals];
        for (idx, arg) in args.into_iter().enumerate() {
            locals[idx] = arg;
        }
        Ok(locals)
    }

    fn function_entry(&self, fun_idx: u32) -> Result<&FunctionEntry, RuntimeError> {
        let idx = fun_idx as usize;
        let entry = self
            .bytecode
            .functions
            .get(idx)
            .ok_or_else(|| RuntimeError::new(format!("Unknown function index: {}", fun_idx)))?;

        if entry.fun_idx != fun_idx {
            return Err(RuntimeError::new(format!(
                "Function table invariant violated: functions[{}].fun_idx = {}",
                idx, entry.fun_idx
            )));
        }

        Ok(entry)
    }

    fn validate_jump_target(&self, addr: u32) -> Result<usize, RuntimeError> {
        let target = addr as usize;
        if target >= self.bytecode.opcodes.len() {
            return Err(RuntimeError::new(format!("Invalid jump target: {}", addr)));
        }
        Ok(target)
    }

    // Stack helpers

    fn pop_stack(&mut self) -> Result<Value, RuntimeError> {
        self.stack
            .pop()
            .ok_or_else(|| RuntimeError::new("Stack underflow"))
    }

    fn current_frame(&self) -> Result<&CallFrame, RuntimeError> {
        self.frames
            .last()
            .ok_or_else(|| RuntimeError::new("Frame stack underflow"))
    }

    fn current_frame_mut(&mut self) -> Result<&mut CallFrame, RuntimeError> {
        self.frames
            .last_mut()
            .ok_or_else(|| RuntimeError::new("Frame stack underflow"))
    }

    fn with_call_site<T>(
        &mut self,
        call_site: Option<(u32, u32)>,
        f: impl FnOnce(&mut Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let frame_idx = self
            .frames
            .len()
            .checked_sub(1)
            .ok_or_else(|| RuntimeError::new("Frame stack underflow"))?;
        let previous = self.frames[frame_idx].call_site;
        self.frames[frame_idx].call_site = call_site;
        let result = f(self);
        let result = result.map_err(|mut err| {
            if err.context.call_site.is_none() {
                err.context.call_site = self.runtime_error_location();
            }
            err
        });
        self.frames[frame_idx].call_site = previous;
        result
    }

    fn pop_int(&mut self) -> Result<SurtrInt, RuntimeError> {
        match self.pop_stack()? {
            Value::Int(n) => Ok(n),
            other => Err(RuntimeError::new(format!("Expected Int, got {:?}", other))),
        }
    }

    fn pop_tag(&mut self) -> Result<u32, RuntimeError> {
        match self.pop_stack()? {
            Value::Tag(tag) => Ok(tag),
            other => Err(RuntimeError::new(format!("Expected Tag, got {:?}", other))),
        }
    }

    fn pop_float(&mut self) -> Result<f64, RuntimeError> {
        match self.pop_stack()? {
            Value::Float(f) => Ok(f),
            other => Err(RuntimeError::new(format!(
                "Expected Float, got {:?}",
                other
            ))),
        }
    }

    fn pop_str(&mut self) -> Result<String, RuntimeError> {
        match self.pop_stack()? {
            Value::Str(s) => Ok(s),
            other => Err(RuntimeError::new(format!("Expected Str, got {:?}", other))),
        }
    }

    fn pop_bool(&mut self) -> Result<bool, RuntimeError> {
        match self.pop_stack()? {
            Value::Bool(b) => Ok(b),
            other => Err(RuntimeError::new(format!("Expected Bool, got {:?}", other))),
        }
    }

    fn int_binop<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(SurtrInt, SurtrInt) -> Result<Value, RuntimeError>,
    {
        let b = self.pop_int()?;
        let a = self.pop_int()?;
        let result = f(a, b)?;
        self.stack.push(result);
        Ok(())
    }

    fn float_binop<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(f64, f64) -> Value,
    {
        let b = self.pop_float()?;
        let a = self.pop_float()?;
        self.stack.push(f(a, b));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{VmObservationOptions, VM};
    use sindr::ir::{
        Bytecode, BytecodeChunk, Constant, ErrTemplate, FunctionEntry, Opcode, OpcodeSource,
        SourceMap,
    };
    use sindr::primitives::int;
    use sindr::runtime::{TypeEntry, TypeKind, TypeRegistry, Value};

    fn base_bytecode(opcodes: Vec<Opcode>) -> Bytecode {
        Bytecode {
            opcodes,
            type_registry: TypeRegistry::new(),
            ..Bytecode::default()
        }
    }

    fn function_entry(
        fun_idx: u32,
        entry_pc: u32,
        num_locals: u32,
        arity: u8,
        qualified_name: Option<&str>,
    ) -> FunctionEntry {
        FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals,
            arity,
            qualified_name: qualified_name.map(str::to_string),
            signature: None,
            end_pc: 0,
            span_start: 0,
            span_end: 0,
            flags: Default::default(),
        }
    }

    #[test]
    fn top_level_return_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::Return]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("top-level"));
    }

    #[test]
    fn load_local_out_of_bounds_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Halt]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("LoadLocal out of bounds"));
    }

    #[test]
    fn invalid_jump_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::Jump(42), Opcode::Halt]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("Invalid jump target"));
    }

    #[test]
    fn runtime_error_captures_vm_context() {
        let bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::JumpIfFalse(0),
            Opcode::Halt,
        ]);
        let mut vm = VM::new(bytecode);
        vm.bytecode.constants = vec![Constant::Int(int(1))];

        let err = vm.run().expect_err("must fail");
        assert_eq!(err.context.pc, Some(1));
        assert_eq!(err.context.opcode.as_deref(), Some("JumpIfFalse(0)"));
        assert!(err
            .context
            .details
            .iter()
            .any(|detail| detail.starts_with("stack_depth=")));
    }

    #[test]
    fn unknown_function_index_is_runtime_error() {
        let bytecode = base_bytecode(vec![
            Opcode::Call {
                fun_idx: 1,
                arity: 0,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
        ]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("Unknown function index"));
    }

    #[test]
    fn call_initializes_locals_without_makeframe() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::Call {
                fun_idx: 0,
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Int(int(5))];
        bytecode.functions = vec![function_entry(0, 3, 1, 1, None)];

        VM::new(bytecode).run().expect("run should succeed");
    }

    #[test]
    fn frame_stack_underflow_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        vm.frames.clear();
        let err = vm.run().expect_err("must fail");
        assert!(err.message.contains("Frame stack underflow"));
    }

    #[test]
    fn function_table_mismatch_is_runtime_error() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Call {
                fun_idx: 0,
                arity: 0,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
        ]);
        bytecode.functions = vec![function_entry(1, 1, 0, 0, None)];

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("Function table invariant violated"));
    }

    #[test]
    fn push_relocates_jump_targets() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            // `Jump(2)` must be relocated to absolute pc=3 because code_base=1.
            opcodes: vec![
                Opcode::Jump(2),
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::Halt,
            ],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Int(int(99)), Constant::Int(int(7))],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let result = vm.push_atomic(chunk).expect("push should succeed");
        assert_eq!(result, Value::Int(int(7)));
    }

    #[test]
    fn push_relocates_const_indices() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.constants = vec![Constant::Int(int(10))];
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
            source_map: None,
            const_base: 1,
            constants: vec![Constant::Int(int(42))],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let result = vm.push_atomic(chunk).expect("push should succeed");
        assert_eq!(result, Value::Int(int(42)));
    }

    #[test]
    fn push_relocates_make_error_template_indices() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.error_templates = vec![ErrTemplate {
            id: 0,
            kind: "Old".into(),
            span_start: 0,
            span_end: 0,
            line: 1,
            column: 1,
            format: "{}".into(),
            num_params: 1,
        }];
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::MakeError { template_id: 0 },
                Opcode::Halt,
            ],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Str("new message".into())],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 1,
            error_templates: vec![ErrTemplate {
                id: 0,
                kind: "NewKind".into(),
                span_start: 10,
                span_end: 20,
                line: 2,
                column: 3,
                format: "{}".into(),
                num_params: 1,
            }],
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let result = vm.push_atomic(chunk).expect("push should succeed");
        match result {
            Value::Error(rich) => {
                assert_eq!(rich.kind, "NewKind");
                assert_eq!(rich.message, "new message");
                assert_eq!(rich.location.line, 2);
                assert_eq!(rich.location.column, 3);
            }
            other => panic!("expected Value::Error, got {:?}", other),
        }
    }

    #[test]
    fn make_error_prefers_call_site_line_and_column_when_source_is_available() {
        let source = "deferror Boom {\n  \"boom\"\n}\n\nBoom()\n".to_string();
        let mut vm =
            VM::new(base_bytecode(vec![Opcode::Halt])).with_source(source, "sample.srt".into());
        vm.frames[0].call_site = Some((30, 36));
        let result = vm
            .push_atomic(BytecodeChunk {
                opcodes: vec![
                    Opcode::LoadConst(0),
                    Opcode::MakeError { template_id: 0 },
                    Opcode::Halt,
                ],
                source_map: None,
                const_base: 0,
                constants: vec![Constant::Str("boom".into())],
                new_locals: 0,
                type_entries: Vec::new(),
                error_template_base: 0,
                error_templates: vec![ErrTemplate {
                    id: 0,
                    kind: "Boom".into(),
                    span_start: 0,
                    span_end: 5,
                    line: 1,
                    column: 1,
                    format: "{}".into(),
                    num_params: 1,
                }],
                functions: Vec::new(),
                docs: Vec::new(),
            })
            .expect("push should succeed");
        match result {
            Value::Error(rich) => {
                assert_eq!(rich.location.line, 5);
                assert_eq!(rich.location.column, 3);
                assert_eq!(rich.location.span_start, 30);
                assert_eq!(rich.location.span_end, 36);
            }
            other => panic!("expected Value::Error, got {:?}", other),
        }
    }

    #[test]
    fn builtin_result_error_uses_builtin_call_span_as_location() {
        let source = "safe_mod(10, 0)\n".to_string();
        let span_end = source.trim_end().len() as u32;
        let bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::CallBuiltin {
                builtin_id: 4,
                arity: 2,
                span_start: 0,
                span_end,
            },
            Opcode::Halt,
        ]);
        let mut vm = VM::new(bytecode).with_source(source, "REPL".into());
        vm.bytecode.constants = vec![Constant::Int(int(10)), Constant::Int(int(0))];

        vm.run().expect("run should succeed");
        match vm.last_value().cloned().expect("result should be recorded") {
            Value::Tagged { tag: 1, fields } => match fields.first() {
                Some(Value::Error(rich)) => {
                    assert_eq!(rich.kind, "ZeroDivisionError");
                    assert_eq!(rich.location.line, 1);
                    assert_eq!(rich.location.column, 1);
                    assert_eq!(rich.location.span_start, 0);
                    assert_eq!(rich.location.span_end, span_end);
                }
                other => panic!("expected Err(Value::Error), got {:?}", other),
            },
            other => panic!("expected Err result, got {:?}", other),
        }
    }

    #[test]
    fn push_fails_when_constant_base_mismatches() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 1,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("Chunk constant base mismatch"));
    }

    #[test]
    fn push_atomic_preserves_vm_on_failure() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let before = vm.clone();

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 1,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("Chunk constant base mismatch"));
        assert_eq!(vm.bytecode, before.bytecode);
        assert_eq!(vm.stack, before.stack);
        assert_eq!(vm.frames.len(), before.frames.len());
        assert_eq!(vm.pc, before.pc);
    }

    #[test]
    fn push_atomic_rolls_back_overwritten_function_entries() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.functions = vec![function_entry(0, 0, 0, 0, Some("old"))];
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::LoadLocal(9), Opcode::Halt, Opcode::Return],
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: vec![function_entry(0, 2, 1, 0, Some("new"))],
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("LoadLocal out of bounds"));
        assert_eq!(
            vm.bytecode.functions[0].qualified_name.as_deref(),
            Some("old")
        );
        assert_eq!(vm.bytecode.functions[0].entry_pc, 0);
    }

    #[test]
    fn push_atomic_rolls_back_captured_output() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode).with_output_capture();

        let chunk = BytecodeChunk {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::CallBuiltin {
                    builtin_id: 0,
                    arity: 1,
                    span_start: 0,
                    span_end: 0,
                },
                Opcode::LoadLocal(9),
                Opcode::Halt,
            ],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Str("hello".into())],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("LoadLocal out of bounds"));
        assert_eq!(vm.output.as_deref(), Some(&[][..]));
    }

    #[test]
    fn push_atomic_rejects_chunk_without_halt() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let before = vm.clone();

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::LoadConst(0)],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Int(int(1))],
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("chunk missing Halt"));
        assert_eq!(vm.bytecode, before.bytecode);
    }

    #[test]
    fn push_relocates_chunk_source_map_indices() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: Some(SourceMap {
                entries: vec![OpcodeSource {
                    opcode_index: 0,
                    span_start: 2,
                    span_end: 5,
                    line: 1,
                    column: 3,
                    source_name: Some("repl".into()),
                }],
            }),
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        vm.push_atomic(chunk).expect("push should succeed");
        let source_map = vm
            .bytecode
            .source_map
            .expect("source map should be present");
        assert_eq!(source_map.entries.len(), 1);
        assert_eq!(source_map.entries[0].opcode_index, 1);
        assert_eq!(source_map.entries[0].line, 1);
        assert_eq!(source_map.entries[0].column, 3);
        assert_eq!(source_map.entries[0].source_name.as_deref(), Some("repl"));
    }

    #[test]
    fn push_atomic_rejects_chunk_out_of_bounds_source_map_entry() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: Some(SourceMap {
                entries: vec![OpcodeSource {
                    opcode_index: 1,
                    span_start: 0,
                    span_end: 1,
                    line: 1,
                    column: 1,
                    source_name: Some("repl".into()),
                }],
            }),
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err
            .message
            .contains("chunk source_map opcode_index out of bounds"));
    }

    #[test]
    fn run_rejects_function_entry_before_top_level_halt() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 0, 0, 0, None)];

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("must be after top-level Halt"));
    }

    #[test]
    fn run_rejects_reserved_type_tag_in_registry() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 0,
            name: "Bad".into(),
            kind: TypeKind::Struct,
            field_names: vec![],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("reserved result tag"));
    }

    #[test]
    fn run_rejects_duplicate_type_tag_in_registry() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "A".into(),
            kind: TypeKind::Struct,
            field_names: vec![],
        });
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "B".into(),
            kind: TypeKind::Record,
            field_names: vec![],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("duplicate type tag"));
    }

    #[test]
    fn run_rejects_out_of_bounds_source_map_entry() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.source_map = Some(SourceMap {
            entries: vec![OpcodeSource {
                opcode_index: 1,
                span_start: 0,
                span_end: 1,
                line: 1,
                column: 1,
                source_name: Some("main.srt".into()),
            }],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err
            .message
            .contains("source_map opcode_index out of bounds"));
    }

    #[test]
    fn run_rejects_duplicate_source_map_opcode_index() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::Halt]);
        bytecode.source_map = Some(SourceMap {
            entries: vec![
                OpcodeSource {
                    opcode_index: 0,
                    span_start: 0,
                    span_end: 1,
                    line: 1,
                    column: 1,
                    source_name: Some("main.srt".into()),
                },
                OpcodeSource {
                    opcode_index: 0,
                    span_start: 2,
                    span_end: 3,
                    line: 1,
                    column: 3,
                    source_name: Some("main.srt".into()),
                },
            ],
        });

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("duplicate source_map entry"));
    }

    #[test]
    fn push_atomic_rejects_chunk_reserved_type_tag() {
        let bytecode = base_bytecode(vec![Opcode::Halt]);
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: vec![TypeEntry {
                tag: 1,
                name: "Bad".into(),
                kind: TypeKind::Struct,
                field_names: vec![],
            }],
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("reserved result tag"));
    }

    #[test]
    fn push_atomic_rejects_chunk_duplicate_type_tag_against_vm() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "Existing".into(),
            kind: TypeKind::Struct,
            field_names: vec![],
        });
        let mut vm = VM::new(bytecode);

        let chunk = BytecodeChunk {
            opcodes: vec![Opcode::Halt],
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_entries: vec![TypeEntry {
                tag: 10,
                name: "Duplicate".into(),
                kind: TypeKind::Record,
                field_names: vec![],
            }],
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
        };

        let err = vm.push_atomic(chunk).expect_err("must fail");
        assert!(err.message.contains("duplicate type tag"));
    }

    #[test]
    fn type_registry_getter_returns_shared_reference() {
        let vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let _: &TypeRegistry = vm.type_registry();
    }

    #[test]
    fn list_head_on_empty_list_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::ListEmpty, Opcode::ListHead, Opcode::Halt]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("ListHead on empty list"));
    }

    #[test]
    fn list_tail_on_empty_list_is_runtime_error() {
        let bytecode = base_bytecode(vec![Opcode::ListEmpty, Opcode::ListTail, Opcode::Halt]);
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("ListTail on empty list"));
    }

    #[test]
    fn string_head_and_tail_execute_successfully() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::StringHead,
            Opcode::LoadConst(0),
            Opcode::StringTail,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Str("あい".into())];

        let mut vm = VM::new(bytecode);
        vm.run().expect("must succeed");

        assert_eq!(
            vm.stack,
            vec![Value::Str("あ".into()), Value::Str("い".into())]
        );
    }

    #[test]
    fn string_head_on_empty_string_is_runtime_error() {
        let mut bytecode =
            base_bytecode(vec![Opcode::LoadConst(0), Opcode::StringHead, Opcode::Halt]);
        bytecode.constants = vec![Constant::Str(String::new())];
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("StringHead on empty string"));
    }

    #[test]
    fn string_tail_on_empty_string_is_runtime_error() {
        let mut bytecode =
            base_bytecode(vec![Opcode::LoadConst(0), Opcode::StringTail, Opcode::Halt]);
        bytecode.constants = vec![Constant::Str(String::new())];
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("StringTail on empty string"));
    }

    #[test]
    fn int_bitwise_opcodes_execute_successfully() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::BitNotInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::BitAndInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::BitOrInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::BitXorInt,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(6)), Constant::Int(int(3))];

        let mut vm = VM::new(bytecode);
        vm.run().expect("run should succeed");

        assert_eq!(vm.last_result, Some(Value::Int(int(5))));
        assert!(matches!(vm.stack.first(), Some(Value::Int(value)) if *value == int(-7)));
    }

    #[test]
    fn get_field_on_non_tagged_value_is_runtime_error() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::GetField { field_index: 0 },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(1))];
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("GetField on non-tagged value"));
    }

    #[test]
    fn get_tag_on_non_tagged_value_is_runtime_error() {
        let mut bytecode = base_bytecode(vec![Opcode::LoadConst(0), Opcode::GetTag, Opcode::Halt]);
        bytecode.constants = vec![Constant::Bool(true)];
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("GetTag on non-tagged value"));
    }

    #[test]
    fn capture_partial_requires_callable_target() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::CapturePartial(0),
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(1))];
        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err
            .message
            .contains("CapturePartial expects a callable target"));
    }

    #[test]
    fn observation_collects_opcode_stats() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::BitAndInt,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(6)), Constant::Int(int(3))];

        let mut vm = VM::new(bytecode);
        vm.enable_observation(VmObservationOptions::default());
        vm.run().expect("run should succeed");

        let observation = vm.observation().expect("observation should exist");
        assert_eq!(observation.stats.executed_opcodes, 4);
        assert_eq!(observation.stats.per_opcode.get("LoadConst"), Some(&2));
        assert_eq!(observation.stats.per_opcode.get("BitAndInt"), Some(&1));
        assert_eq!(observation.stats.per_opcode.get("Halt"), Some(&1));
        assert_eq!(observation.stats.max_stack_depth, 2);
        assert_eq!(observation.stats.max_frame_depth, 1);
    }

    #[test]
    fn observation_collects_call_trace_and_builtin_counts() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::CallBuiltin {
                builtin_id: 1,
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(42))];

        let mut vm = VM::new(bytecode);
        vm.enable_observation(VmObservationOptions {
            trace_calls: true,
            ..VmObservationOptions::default()
        });
        vm.run().expect("run should succeed");

        let observation = vm.observation().expect("observation should exist");
        assert_eq!(observation.stats.builtin_calls, 1);
        assert_eq!(observation.trace_lines.len(), 1);
        assert!(observation.trace_lines[0].contains("kind=CallBuiltin"));
        assert!(observation.trace_lines[0].contains("target=to_string"));
    }
}
