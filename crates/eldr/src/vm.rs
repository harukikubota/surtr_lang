use sindr::builtin::builtin_meta_by_id;
use sindr::ir::{
    line_column_for_offset, Bytecode, BytecodeChunk, Constant, DocEntry, FunctionEntry, Opcode,
    RuntimeProcessInstance, RuntimeProcessKind, RuntimeProcessSpec, RuntimeProcessSpecTable,
    SourceMap,
};
use sindr::primitives::SurtrInt;
use sindr::runtime::{
    Callable, CallableMetadata, CallableOrigin, CallableTarget, ListHandle, Location, PidHandle,
    RichError, TypeRegistry, Value,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::builtin::call_builtin;
use crate::dbg_display::{render_dbg_report, DbgRenderArg};
use crate::error::{RuntimeError, RuntimeErrorContext};
use std::io::{self, Write};
use std::time::Instant;

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
    pub diagnostic: Option<VmTestDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTestDiagnostic {
    pub kind: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub span_start: u32,
    pub span_end: u32,
}

impl VmTestDiagnostic {
    fn from_rich_error(error: &RichError) -> Self {
        Self {
            kind: error.kind.clone(),
            message: error.visible_message().to_string(),
            file: error.location.file.clone(),
            line: error.location.line,
            column: error.location.column,
            span_start: error.location.span_start,
            span_end: error.location.span_end,
        }
    }
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
    stdin_input_cursor: usize,
    process_runtime: ProcessRuntime,
    opcode_len: usize,
    constant_len: usize,
    type_entry_len: usize,
    error_template_len: usize,
    function_len: usize,
    doc_len: usize,
    process_spec_len: usize,
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
    pub process: VmProcessCounters,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmObservation {
    pub stats: VmStats,
    pub trace_lines: Vec<String>,
    pub dropped_trace_events: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmProcessCounters {
    pub process_spec_count: usize,
    pub singleton_slot_count: usize,
    pub process_count: usize,
    pub runnable_process_count: usize,
    pub waiting_process_count: usize,
    pub completed_process_count: usize,
    pub failed_process_count: usize,
    pub mailbox_message_count: usize,
    pub future_count: usize,
    pub running_future_count: usize,
    pub ready_future_count: usize,
    pub cancelled_future_count: usize,
    pub waiting_table_count: usize,
    pub reply_waiter_count: usize,
    pub deadline_queue_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmProcessSpecSnapshot {
    pub spec_id: u32,
    pub process_name: String,
    pub module_path: String,
    pub kind: String,
    pub instance: String,
    pub boot: bool,
    pub registry: bool,
    pub lazy: bool,
    pub init_fun_idx: u32,
    pub get_fun_idx: u32,
    pub set_fun_idx: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmExecutionContextSnapshot {
    pub pc: usize,
    pub stack_depth: usize,
    pub frame_depth: usize,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmProcessInstanceSnapshot {
    pub pid: u64,
    pub process_name: String,
    pub spec_id: u32,
    pub status: String,
    pub mailbox_len: usize,
    pub owner: Option<u64>,
    pub lazy_state_pending: bool,
    pub state_value: Option<String>,
    pub execution_context: Option<VmExecutionContextSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmFutureSnapshot {
    pub future_id: u64,
    pub owner: Option<u64>,
    pub state: String,
    pub value: Option<String>,
    pub deadline_tick: Option<u64>,
    pub waiter_count: usize,
    pub cancel_on_timeout: bool,
    pub correlation_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmDeadlineSnapshot {
    pub future_id: u64,
    pub deadline_tick: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmProcessRuntimeSnapshot {
    pub counters: VmProcessCounters,
    pub specs: Vec<VmProcessSpecSnapshot>,
    pub singleton_slots: BTreeMap<String, u64>,
    pub processes: Vec<VmProcessInstanceSnapshot>,
    pub waiting: BTreeMap<u64, String>,
    pub replies: BTreeMap<u64, u64>,
    pub deadlines: Vec<VmDeadlineSnapshot>,
    pub futures: Vec<VmFutureSnapshot>,
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

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
struct ProcessRuntime {
    next_pid: u64,
    next_future_id: FutureId,
    next_correlation_id: CorrelationId,
    specs_by_id: Vec<RuntimeProcessSpec>,
    spec_id_by_name: BTreeMap<String, u32>,
    specs_by_name: BTreeMap<String, RuntimeProcessSpec>,
    singleton_by_name: BTreeMap<String, u64>,
    processes: BTreeMap<u64, ProcessInstance>,
    futures: BTreeMap<FutureId, FutureRecord>,
    reply_table: BTreeMap<CorrelationId, FutureId>,
    waiting_table: BTreeMap<u64, ProcessWaitReason>,
    deadline_queue: VecDeque<DeadlineEntry>,
    root_supervisor: RootSupervisorState,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ProcessInstance {
    pid: u64,
    spec_id: u32,
    status: ProcessStatus,
    mailbox: VecDeque<ProcessMailboxMessage>,
    execution_context: Option<ProcessExecutionContext>,
    state_value: Option<Value>,
    owner: Option<u64>,
    lazy_state_pending: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum ProcessStatus {
    #[default]
    Runnable,
    Waiting(ProcessWaitReason),
    Completed,
    Failed,
    Restarting,
    Stopped,
}

type FutureId = u64;
type CorrelationId = u64;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProcessWaitReason {
    Future(FutureId),
    Reply(CorrelationId),
    Boot,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum ProcessMailboxMessage {
    Request { payload: Value },
    Reply { payload: Value },
    Cast(Value),
    SystemBoot,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ProcessExecutionContext {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    pc: usize,
    target: ExecutionTarget,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutionTarget {
    TopLevel,
    FrameDepth(usize),
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum StepOutcome {
    Continue,
    Halt(Value),
    Pending {
        future_id: FutureId,
        resume: ProcessExecutionContext,
    },
    RuntimeError(RuntimeError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpcodeControl {
    Continue,
    Halt,
    Pending(FutureId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskMode {
    Call,
    Async,
    Launch,
    Cast,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum FutureState {
    Running,
    Ready(Value),
    Cancelled(Value),
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
struct FutureRecord {
    id: FutureId,
    owner: Option<u64>,
    state: FutureState,
    deadline_tick: Option<u64>,
    waiters: Vec<u64>,
    cancel_on_timeout: bool,
    correlation_id: Option<CorrelationId>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeadlineEntry {
    deadline_tick: u64,
    future_id: FutureId,
}

#[derive(Debug, Clone, Default)]
struct RootSupervisorState {
    boot_completed: bool,
    boot_failures: BTreeMap<String, String>,
}

impl ProcessRuntime {
    fn counters(&self) -> VmProcessCounters {
        let mut counters = VmProcessCounters {
            process_spec_count: self.specs_by_id.len(),
            singleton_slot_count: self.singleton_by_name.len(),
            process_count: self.processes.len(),
            mailbox_message_count: self
                .processes
                .values()
                .map(|process| process.mailbox.len())
                .sum(),
            future_count: self.futures.len(),
            waiting_table_count: self.waiting_table.len(),
            reply_waiter_count: self.reply_table.len(),
            deadline_queue_count: self.deadline_queue.len(),
            ..VmProcessCounters::default()
        };

        for process in self.processes.values() {
            match process.status {
                ProcessStatus::Runnable => counters.runnable_process_count += 1,
                ProcessStatus::Waiting(_) => counters.waiting_process_count += 1,
                ProcessStatus::Completed => counters.completed_process_count += 1,
                ProcessStatus::Failed => counters.failed_process_count += 1,
                ProcessStatus::Restarting | ProcessStatus::Stopped => {}
            }
        }

        for future in self.futures.values() {
            match future.state {
                FutureState::Running => counters.running_future_count += 1,
                FutureState::Ready(_) => counters.ready_future_count += 1,
                FutureState::Cancelled(_) => counters.cancelled_future_count += 1,
            }
        }

        counters
    }
}

impl ProcessStatus {
    fn label(&self) -> &'static str {
        match self {
            ProcessStatus::Runnable => "runnable",
            ProcessStatus::Waiting(_) => "waiting",
            ProcessStatus::Completed => "completed",
            ProcessStatus::Failed => "failed",
            ProcessStatus::Restarting => "restarting",
            ProcessStatus::Stopped => "stopped",
        }
    }
}

impl ProcessWaitReason {
    fn label(&self) -> &'static str {
        match self {
            ProcessWaitReason::Future(_) => "future",
            ProcessWaitReason::Reply(_) => "reply",
            ProcessWaitReason::Boot => "boot",
        }
    }
}

impl ExecutionTarget {
    fn label(&self) -> String {
        match self {
            ExecutionTarget::TopLevel => "top_level".into(),
            ExecutionTarget::FrameDepth(depth) => format!("frame_depth:{depth}"),
        }
    }
}

impl FutureState {
    fn label(&self) -> &'static str {
        match self {
            FutureState::Running => "running",
            FutureState::Ready(_) => "ready",
            FutureState::Cancelled(_) => "cancelled",
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
    /// Optional stdin fixture used by Rust tests and non-interactive harnesses.
    stdin_input: Option<String>,
    stdin_input_cursor: usize,
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
    /// VM-owned process table for the initial actor/agent runtime.
    process_runtime: ProcessRuntime,
}

impl VM {
    pub fn new(bytecode: Bytecode) -> Self {
        let num_locals = bytecode.num_locals;
        let process_runtime = ProcessRuntime::from_spec_table(&bytecode.runtime_process_specs);
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
            stdin_input: None,
            stdin_input_cursor: 0,
            exit_code: 0,
            last_result: None,
            observer: None,
            test_scope: Vec::new(),
            test_events: Vec::new(),
            test_stdout_cursor: 0,
            test_stderr_cursor: 0,
            process_runtime,
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

    pub fn with_stdin_input(mut self, input: impl Into<String>) -> Self {
        self.set_stdin_input(input);
        self
    }

    pub fn set_stdin_input(&mut self, input: impl Into<String>) {
        self.stdin_input = Some(input.into());
        self.stdin_input_cursor = 0;
    }

    pub fn push_stdin_input(&mut self, input: impl AsRef<str>) {
        match self.stdin_input.as_mut() {
            Some(buffer) => buffer.push_str(input.as_ref()),
            None => {
                self.stdin_input = Some(input.as_ref().to_string());
                self.stdin_input_cursor = 0;
            }
        }
    }

    /// Access source text if attached.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Access source file name if attached.
    pub fn source_file(&self) -> Option<&str> {
        self.source_file.as_deref()
    }

    pub fn function_entries(&self) -> &[FunctionEntry] {
        &self.bytecode.functions
    }

    fn callable_metadata_for_builtin(&self, builtin_id: u16) -> CallableMetadata {
        let Some(meta) = builtin_meta_by_id(builtin_id) else {
            return CallableMetadata::default();
        };
        let doc = self.bytecode.docs.iter().rev().find(|doc| {
            doc.qualified_name.rsplit("::").next() == Some(meta.name)
                && matches!(doc.kind, sindr::ir::DocKind::Function)
        });

        CallableMetadata {
            origin: CallableOrigin::Capture,
            module: doc.map(|doc| doc.module_path.clone()),
            name: Some(meta.name.to_string()),
            full_signature: doc
                .and_then(|doc| doc.signature.clone())
                .or_else(|| Some(meta.sig_str.to_string())),
            applied_args: 0,
        }
    }

    fn callable_metadata_for_function(&self, fun_idx: u32) -> CallableMetadata {
        let Some(entry) = self.bytecode.functions.get(fun_idx as usize) else {
            return CallableMetadata::default();
        };
        if entry.flags.closure {
            if let (Some(qualified_name), Some(signature)) =
                (entry.qualified_name.as_deref(), entry.signature.clone())
            {
                let (module, name) = split_qualified_name_owned(qualified_name);
                return CallableMetadata {
                    origin: CallableOrigin::Capture,
                    module,
                    name,
                    full_signature: Some(signature),
                    applied_args: 0,
                };
            }
            return CallableMetadata {
                origin: CallableOrigin::Closure,
                full_signature: entry.signature.clone(),
                ..CallableMetadata::default()
            };
        }

        let qualified_name = entry.qualified_name.as_deref();
        let signature = entry.signature.clone().or_else(|| {
            qualified_name.and_then(|qualified_name| {
                self.bytecode
                    .docs
                    .iter()
                    .rev()
                    .find(|doc| {
                        matches!(doc.kind, sindr::ir::DocKind::Function)
                            && doc.qualified_name == qualified_name
                    })
                    .and_then(|doc| doc.signature.clone())
            })
        });
        let (module, name) = qualified_name
            .map(split_qualified_name_owned)
            .unwrap_or((None, None));

        CallableMetadata {
            origin: if signature.is_some() {
                CallableOrigin::Capture
            } else {
                CallableOrigin::Unknown
            },
            module,
            name,
            full_signature: signature,
            applied_args: 0,
        }
    }

    fn callable_for_function(&self, fun_idx: u32) -> Callable {
        Callable {
            target: CallableTarget::Function(fun_idx),
            lexical_captures: Vec::new(),
            metadata: self.callable_metadata_for_function(fun_idx),
        }
    }

    fn promote_partial_apply_metadata(
        &self,
        target: &Callable,
        lexical_captures: &[Value],
    ) -> CallableMetadata {
        let Some(Value::Callable(original)) = lexical_captures.first() else {
            return target.metadata.clone();
        };
        if original.metadata.origin != CallableOrigin::Capture {
            return target.metadata.clone();
        }

        let promoted = match target.target {
            CallableTarget::Function(fun_idx) => self
                .bytecode
                .functions
                .get(fun_idx as usize)
                .is_some_and(|entry| entry.flags.partial_apply_wrapper),
            _ => false,
        };
        if promoted
            || matches!(
                target.metadata.origin,
                CallableOrigin::Capture | CallableOrigin::Unknown
            )
        {
            let mut metadata = original.metadata.clone();
            metadata.applied_args += lexical_captures.len().saturating_sub(1);
            metadata
        } else {
            target.metadata.clone()
        }
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

    pub(crate) fn begin_test_case_io(&mut self) {
        self.reset_captured_io();
        self.stdin_input = None;
        self.stdin_input_cursor = 0;
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

    pub(crate) fn emit_stdout_text(&mut self, text: String) -> io::Result<()> {
        match self.io_policy.stdout {
            IoMode::Passthrough => {
                print!("{}", text);
                io::stdout().flush()
            }
            IoMode::Capture => {
                if let Some(buffer) = self.output.as_mut() {
                    if !text.is_empty() {
                        buffer.push(text);
                    }
                    Ok(())
                } else {
                    print!("{}", text);
                    io::stdout().flush()
                }
            }
            IoMode::Tee => {
                print!("{}", text);
                io::stdout().flush()?;
                if let Some(buffer) = self.output.as_mut() {
                    if !text.is_empty() {
                        buffer.push(text);
                    }
                }
                Ok(())
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

    pub(crate) fn has_injected_stdin(&self) -> bool {
        self.stdin_input.is_some()
    }

    pub(crate) fn read_injected_line(&mut self) -> Option<String> {
        let input = self.stdin_input.as_ref()?;
        if self.stdin_input_cursor >= input.len() {
            return None;
        }
        let remaining = &input[self.stdin_input_cursor..];
        let read_len = remaining
            .find('\n')
            .map(|idx| idx + '\n'.len_utf8())
            .unwrap_or(remaining.len());
        let line = remaining[..read_len].to_string();
        self.stdin_input_cursor += read_len;
        Some(line)
    }

    pub(crate) fn read_injected_char(&mut self) -> Option<String> {
        let input = self.stdin_input.as_ref()?;
        if self.stdin_input_cursor >= input.len() {
            return None;
        }
        let mut chars = input[self.stdin_input_cursor..].chars();
        let ch = chars.next()?;
        self.stdin_input_cursor += ch.len_utf8();
        Some(ch.to_string())
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

    fn boot_failure_error(&self, process_name: &str, detail: &str) -> RuntimeError {
        RuntimeError::new(format!("process `{process_name}` failed to boot: {detail}"))
    }

    fn ensure_root_supervisor_booted(&mut self) -> Result<(), RuntimeError> {
        if self.process_runtime.root_supervisor.boot_completed {
            return Ok(());
        }

        let boot_specs = self
            .process_runtime
            .specs_by_id
            .iter()
            .filter(|spec| {
                spec.instance == RuntimeProcessInstance::Singleton
                    && spec.boot
                    && !self
                        .process_runtime
                        .singleton_by_name
                        .contains_key(&spec.process_name)
            })
            .cloned()
            .collect::<Vec<_>>();

        let saved_runtime = self.process_runtime.clone();
        for spec in boot_specs {
            if let Err(err) = self.ensure_singleton_available(&spec.process_name) {
                let detail = self
                    .process_runtime
                    .root_supervisor
                    .boot_failures
                    .get(&spec.process_name)
                    .cloned()
                    .unwrap_or_else(|| err.message.clone());
                self.process_runtime = saved_runtime;
                self.process_runtime
                    .root_supervisor
                    .boot_failures
                    .insert(spec.process_name.clone(), detail.clone());
                return Err(self.boot_failure_error(&spec.process_name, &detail));
            }
        }

        self.process_runtime.root_supervisor.boot_completed = true;
        Ok(())
    }

    fn ensure_singleton_available(&mut self, process_name: &str) -> Result<u64, RuntimeError> {
        if let Some(pid) = self.process_runtime.singleton_by_name.get(process_name) {
            return Ok(*pid);
        }
        if let Some(detail) = self
            .process_runtime
            .root_supervisor
            .boot_failures
            .get(process_name)
            .cloned()
        {
            return Err(self.boot_failure_error(process_name, &detail));
        }

        let Some(spec) = self
            .process_runtime
            .specs_by_name
            .get(process_name)
            .cloned()
        else {
            return Err(RuntimeError::new(format!(
                "unknown singleton process `{process_name}`"
            )));
        };

        if spec.instance != RuntimeProcessInstance::Singleton {
            return Err(RuntimeError::new(format!(
                "process `{process_name}` is not a singleton"
            )));
        }

        if spec.kind == RuntimeProcessKind::ReadOnlyAgent && spec.lazy {
            let pid = self.allocate_process_state(process_name.to_string(), None)?;
            self.process_runtime
                .singleton_by_name
                .insert(process_name.to_string(), pid);
            return Ok(pid);
        }

        let init_result = self.invoke_callable_isolated_sync(
            self.callable_for_function(spec.init_fun_idx),
            Vec::new(),
        )?;
        let state = match decode_vm_result(init_result, "__root_boot", "init")? {
            Ok(state) => state,
            Err(err) => {
                let detail = err.visible_message().to_string();
                self.process_runtime
                    .root_supervisor
                    .boot_failures
                    .insert(process_name.to_string(), detail.clone());
                return Err(self.boot_failure_error(process_name, &detail));
            }
        };

        let pid = self.allocate_process_state(process_name.to_string(), Some(state))?;
        self.process_runtime
            .singleton_by_name
            .insert(process_name.to_string(), pid);
        Ok(pid)
    }

    fn materialize_lazy_process_state(&mut self, pid: u64) -> Result<Option<Value>, RuntimeError> {
        let Some(entry) = self.process_runtime.processes.get(&pid).cloned() else {
            return Err(RuntimeError::new(format!(
                "process {} disappeared while materializing state",
                pid
            )));
        };

        if !entry.lazy_state_pending {
            return Ok(entry.state_value);
        }

        let Some(spec) = self.process_runtime.spec_for_id(entry.spec_id).cloned() else {
            return Err(RuntimeError::new(format!(
                "process {} references unknown spec {}",
                pid, entry.spec_id
            )));
        };

        let init_result = self.invoke_callable_isolated_sync(
            self.callable_for_function(spec.init_fun_idx),
            Vec::new(),
        )?;
        let state = match decode_vm_result(init_result, "__process_state", "init")? {
            Ok(state) => state,
            Err(err) => return Ok(Some(err_vm_result(err))),
        };

        let Some(entry) = self.process_runtime.processes.get_mut(&pid) else {
            return Err(RuntimeError::new(format!(
                "process {} disappeared after lazy init",
                pid
            )));
        };
        entry.state_value = Some(state.clone());
        entry.lazy_state_pending = false;
        entry.status = ProcessStatus::Runnable;
        Ok(Some(ok_vm_result(state)))
    }

    pub(crate) fn process_singleton_pid(
        &mut self,
        process_name: String,
        _init: Callable,
    ) -> Result<Value, RuntimeError> {
        self.ensure_root_supervisor_booted()?;
        let pid = self.ensure_singleton_available(&process_name)?;
        Ok(Value::Pid(PidHandle {
            id: pid,
            process_name,
        }))
    }

    pub(crate) fn process_spawn(
        &mut self,
        process_name: String,
        init: Callable,
    ) -> Result<Value, RuntimeError> {
        let init_result = self.invoke_callable_sync(init, Vec::new())?;
        match decode_vm_result(init_result, "__process_spawn", "init")? {
            Ok(state) => {
                let pid = self.allocate_process_state(process_name.clone(), Some(state))?;
                Ok(ok_vm_result(Value::Pid(PidHandle {
                    id: pid,
                    process_name,
                })))
            }
            Err(err) => Ok(err_vm_result(err)),
        }
    }

    pub(crate) fn process_state(&mut self, pid: &PidHandle) -> Result<Value, RuntimeError> {
        let Some(entry) = self.process_runtime.processes.get(&pid.id) else {
            return Ok(err_vm_result(self.process_error(
                "InvalidPid",
                &format!("unknown pid {} for {}", pid.id, pid.process_name),
            )));
        };
        let Some(spec) = self.process_runtime.spec_for_id(entry.spec_id) else {
            return Err(RuntimeError::new(format!(
                "process {} references unknown spec {}",
                entry.pid, entry.spec_id
            )));
        };
        if spec.process_name != pid.process_name {
            let actual_name = spec.process_name.clone();
            return Ok(err_vm_result(self.process_error(
                "InvalidPid",
                &format!(
                    "pid {} belongs to {}, not {}",
                    pid.id, actual_name, pid.process_name
                ),
            )));
        }
        if let Some(state) = entry.state_value.clone() {
            return Ok(ok_vm_result(state));
        }
        if entry.lazy_state_pending {
            return self
                .materialize_lazy_process_state(pid.id)?
                .ok_or_else(|| RuntimeError::new(format!("lazy process {} lost state", pid.id)));
        }
        Ok(err_vm_result(self.process_error(
            "ProcessStateUnavailable",
            &format!("pid {} has no materialized state", pid.id),
        )))
    }

    pub(crate) fn process_store(
        &mut self,
        pid: &PidHandle,
        next_state: Value,
    ) -> Result<Value, RuntimeError> {
        let Some(spec_id) = self
            .process_runtime
            .processes
            .get(&pid.id)
            .map(|entry| entry.spec_id)
        else {
            return Ok(err_vm_result(self.process_error(
                "InvalidPid",
                &format!("unknown pid {} for {}", pid.id, pid.process_name),
            )));
        };
        let Some(spec) = self.process_runtime.spec_for_id(spec_id) else {
            return Err(RuntimeError::new(format!(
                "process {} references unknown spec {}",
                pid.id, spec_id
            )));
        };
        if spec.process_name != pid.process_name {
            let actual_name = spec.process_name.clone();
            return Ok(err_vm_result(self.process_error(
                "InvalidPid",
                &format!(
                    "pid {} belongs to {}, not {}",
                    pid.id, actual_name, pid.process_name
                ),
            )));
        }
        let Some(entry) = self.process_runtime.processes.get_mut(&pid.id) else {
            return Err(RuntimeError::new(format!(
                "process {} disappeared while storing state",
                pid.id
            )));
        };
        entry.state_value = Some(next_state);
        entry.lazy_state_pending = false;
        Ok(ok_vm_result(Value::Unit))
    }

    fn allocate_process_state(
        &mut self,
        name: String,
        state: Option<Value>,
    ) -> Result<u64, RuntimeError> {
        let Some(spec_id) = self.process_runtime.spec_id_by_name.get(&name).copied() else {
            return Err(RuntimeError::new(format!(
                "unknown process spec `{name}` during allocation"
            )));
        };
        let pid = self.process_runtime.next_pid;
        self.process_runtime.next_pid += 1;
        let lazy_state_pending = self
            .process_runtime
            .spec_for_id(spec_id)
            .is_some_and(|spec| spec.kind == RuntimeProcessKind::ReadOnlyAgent && spec.lazy)
            && state.is_none();
        self.process_runtime.processes.insert(
            pid,
            ProcessInstance {
                pid,
                spec_id,
                status: ProcessStatus::Runnable,
                mailbox: VecDeque::new(),
                execution_context: None,
                state_value: state,
                owner: None,
                lazy_state_pending,
            },
        );
        Ok(pid)
    }

    fn process_error(&self, kind: &str, message: &str) -> RichError {
        let location = self.runtime_error_location().unwrap_or_else(|| Location {
            file: self.source_file().unwrap_or("<runtime>").to_string(),
            func: "<process>".into(),
            line: 0,
            column: 0,
            span_start: 0,
            span_end: 0,
        });
        RichError::new(kind, message, location, None)
    }

    #[allow(dead_code)]
    fn process_err_value(&self, kind: &str, message: &str) -> Value {
        err_vm_result(self.process_error(kind, message))
    }

    #[allow(dead_code)]
    fn resolve_future_timeout(&mut self, future_id: FutureId) {
        let timeout_value =
            self.process_err_value("Timeout", &format!("future {} timed out", future_id));
        let _ = self
            .process_runtime
            .resolve_future(future_id, timeout_value);
    }

    #[allow(dead_code)]
    fn resolve_future_process_down(&mut self, future_id: FutureId, pid: u64) {
        let process_down = self.process_err_value(
            "ProcessDown",
            &format!("target process {} stopped before replying", pid),
        );
        let _ = self.process_runtime.resolve_future(future_id, process_down);
    }

    #[allow(dead_code)]
    fn expire_process_deadlines(&mut self, now_tick: u64) -> Vec<FutureId> {
        let expired = self.process_runtime.collect_expired_futures(now_tick);
        for future_id in &expired {
            self.resolve_future_timeout(*future_id);
        }
        expired
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
            diagnostic: None,
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
            diagnostic: None,
        });
    }

    pub(crate) fn record_test_fail_error(&mut self, name: String, error: &RichError) {
        let mut path = self.test_scope.clone();
        path.push(name);
        let io = self.next_test_event_io();
        self.test_events.push(VmTestEvent {
            path,
            detail: Some(error.to_display_string()),
            kind: VmTestEventKind::Failed,
            io,
            diagnostic: Some(VmTestDiagnostic::from_rich_error(error)),
        });
    }

    pub(crate) fn record_current_scope_fail(&mut self, detail: String) {
        let io = self.next_test_event_io();
        self.test_events.push(VmTestEvent {
            path: self.test_scope.clone(),
            detail: Some(detail),
            kind: VmTestEventKind::Failed,
            io,
            diagnostic: None,
        });
    }

    pub fn enable_observation(&mut self, options: VmObservationOptions) {
        self.observer = Some(VmObserver::new(options));
    }

    pub fn observation(&self) -> Option<VmObservation> {
        self.observer.as_ref().map(|observer| {
            let mut snapshot = observer.snapshot();
            snapshot.stats.process = self.process_runtime.counters();
            snapshot
        })
    }

    pub fn process_runtime_snapshot(&self) -> VmProcessRuntimeSnapshot {
        let specs = self
            .process_runtime
            .specs_by_id
            .iter()
            .enumerate()
            .map(|(idx, spec)| VmProcessSpecSnapshot {
                spec_id: idx as u32,
                process_name: spec.process_name.clone(),
                module_path: spec.module_path.clone(),
                kind: format!("{:?}", spec.kind),
                instance: format!("{:?}", spec.instance),
                boot: spec.boot,
                registry: spec.registry,
                lazy: spec.lazy,
                init_fun_idx: spec.init_fun_idx,
                get_fun_idx: spec.get_fun_idx,
                set_fun_idx: spec.set_fun_idx,
            })
            .collect();
        let processes =
            self.process_runtime
                .processes
                .values()
                .map(|process| {
                    let process_name = self
                        .process_runtime
                        .spec_for_id(process.spec_id)
                        .map(|spec| spec.process_name.clone())
                        .unwrap_or_else(|| format!("<unknown:{}>", process.spec_id));
                    let execution_context = process.execution_context.as_ref().map(|context| {
                        VmExecutionContextSnapshot {
                            pc: context.pc,
                            stack_depth: context.stack.len(),
                            frame_depth: context.frames.len(),
                            target: context.target.label(),
                        }
                    });
                    VmProcessInstanceSnapshot {
                        pid: process.pid,
                        process_name,
                        spec_id: process.spec_id,
                        status: process.status.label().into(),
                        mailbox_len: process.mailbox.len(),
                        owner: process.owner,
                        lazy_state_pending: process.lazy_state_pending,
                        state_value: process
                            .state_value
                            .as_ref()
                            .map(|value| crate::builtin::inspect_value(self, value)),
                        execution_context,
                    }
                })
                .collect();
        let waiting = self
            .process_runtime
            .waiting_table
            .iter()
            .map(|(pid, reason)| (*pid, reason.label().to_string()))
            .collect();
        let replies = self
            .process_runtime
            .reply_table
            .iter()
            .map(|(correlation_id, future_id)| (*correlation_id, *future_id))
            .collect();
        let deadlines = self
            .process_runtime
            .deadline_queue
            .iter()
            .map(|entry| VmDeadlineSnapshot {
                future_id: entry.future_id,
                deadline_tick: entry.deadline_tick,
            })
            .collect();
        let futures = self
            .process_runtime
            .futures
            .values()
            .map(|future| VmFutureSnapshot {
                future_id: future.id,
                owner: future.owner,
                state: future.state.label().into(),
                value: match &future.state {
                    FutureState::Ready(value) | FutureState::Cancelled(value) => {
                        Some(crate::builtin::inspect_value(self, value))
                    }
                    FutureState::Running => None,
                },
                deadline_tick: future.deadline_tick,
                waiter_count: future.waiters.len(),
                cancel_on_timeout: future.cancel_on_timeout,
                correlation_id: future.correlation_id,
            })
            .collect();

        VmProcessRuntimeSnapshot {
            counters: self.process_runtime.counters(),
            specs,
            singleton_slots: self.process_runtime.singleton_by_name.clone(),
            processes,
            waiting,
            replies,
            deadlines,
            futures,
        }
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
        self.ensure_root_supervisor_booted()?;
        self.last_result = None;
        self.test_scope.clear();
        self.test_events.clear();
        self.test_stdout_cursor = self.current_output_len();
        self.test_stderr_cursor = self.current_error_output_len();
        match self.run_until_outcome(self.pc, ExecutionTarget::TopLevel) {
            StepOutcome::Halt(_) => {
                self.last_result = Some(self.stack.last().cloned().unwrap_or(Value::Unit));
                Ok(())
            }
            StepOutcome::Pending { .. } => Err(RuntimeError::new(
                "top-level execution suspended without scheduler support",
            )),
            StepOutcome::RuntimeError(err) => Err(err),
            StepOutcome::Continue => Err(RuntimeError::new("top-level execution did not finish")),
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
            dbg_template_base: chunk_dbg_template_base,
            dbg_templates,
            functions,
            docs,
            runtime_process_specs,
        } = chunk;
        self.ensure_root_supervisor_booted()?;
        let code_base = self.bytecode.opcodes.len();
        let const_base = self.bytecode.constants.len();
        let error_template_base = self.bytecode.error_templates.len();
        let dbg_template_base = self.bytecode.dbg_templates.len();
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
        if chunk_dbg_template_base as usize != dbg_template_base {
            return Err(RuntimeError::new(format!(
                "Chunk dbg template base mismatch: chunk={}, vm={}",
                chunk_dbg_template_base, dbg_template_base
            )));
        }
        let mut chunk_opcodes = opcodes;
        Self::relocate_chunk_indices(
            &mut chunk_opcodes,
            code_base,
            const_base,
            error_template_base,
            dbg_template_base,
        )?;
        self.bytecode.constants.extend(constants);
        self.bytecode.type_registry.entries.extend(type_entries);
        self.bytecode.error_templates.extend(error_templates);
        self.bytecode.dbg_templates.extend(dbg_templates);
        self.extend_docs_unique(docs);
        self.bytecode
            .runtime_process_specs
            .entries
            .extend(runtime_process_specs);
        self.process_runtime
            .register_spec_table(&self.bytecode.runtime_process_specs);
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

        self.process_runtime.root_supervisor.boot_completed = false;
        for spec in &self.bytecode.runtime_process_specs.entries {
            self.process_runtime
                .root_supervisor
                .boot_failures
                .remove(&spec.process_name);
        }
        self.ensure_root_supervisor_booted()?;

        match self.run_until_outcome(code_base, ExecutionTarget::TopLevel) {
            StepOutcome::Halt(_) => {
                let result = self.stack.pop().unwrap_or(Value::Unit);
                self.last_result = Some(result.clone());
                self.stack.clear();
                Ok(result)
            }
            StepOutcome::Pending { .. } => Err(RuntimeError::new(
                "chunk execution suspended without scheduler support",
            )),
            StepOutcome::RuntimeError(err) => Err(err),
            StepOutcome::Continue => Err(RuntimeError::new("chunk execution did not finish")),
        }
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
            stdin_input_cursor: self.stdin_input_cursor,
            process_runtime: self.process_runtime.clone(),
            opcode_len: self.bytecode.opcodes.len(),
            constant_len: self.bytecode.constants.len(),
            type_entry_len: self.bytecode.type_registry.entries.len(),
            error_template_len: self.bytecode.error_templates.len(),
            function_len: self.bytecode.functions.len(),
            doc_len: self.bytecode.docs.len(),
            process_spec_len: self.bytecode.runtime_process_specs.entries.len(),
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
        self.stdin_input_cursor = checkpoint.stdin_input_cursor;
        self.process_runtime = checkpoint.process_runtime;

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
        self.bytecode
            .runtime_process_specs
            .entries
            .truncate(checkpoint.process_spec_len);
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

    fn capture_execution_context(
        &self,
        pc: usize,
        target: ExecutionTarget,
    ) -> ProcessExecutionContext {
        ProcessExecutionContext {
            stack: self.stack.clone(),
            frames: self.frames.clone(),
            pc,
            target,
        }
    }

    #[allow(dead_code)]
    fn restore_execution_context(&mut self, context: ProcessExecutionContext) {
        self.stack = context.stack;
        self.frames = context.frames;
        self.pc = context.pc;
    }

    fn invoke_callable_isolated_sync(
        &mut self,
        callable: Callable,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let saved = self.capture_execution_context(self.pc, ExecutionTarget::TopLevel);
        let result = self.invoke_callable_sync(callable, args);
        self.restore_execution_context(saved);
        result
    }

    fn load_local_or_pending(&mut self, slot: u32) -> Result<OpcodeControl, RuntimeError> {
        let slot_index = slot as usize;
        let value = self
            .current_frame()?
            .locals
            .get(slot_index)
            .cloned()
            .ok_or_else(|| RuntimeError::new(format!("LoadLocal out of bounds: {}", slot)))?;

        match value {
            Value::PendingFuture(future_id) => {
                let resolved = self
                    .process_runtime
                    .futures
                    .get(&future_id)
                    .and_then(|future| match &future.state {
                        FutureState::Ready(value) | FutureState::Cancelled(value) => {
                            Some(value.clone())
                        }
                        FutureState::Running => None,
                    });

                if let Some(value) = resolved {
                    self.current_frame_mut()?.locals[slot_index] = value.clone();
                    self.stack.push(value);
                    Ok(OpcodeControl::Continue)
                } else {
                    Ok(OpcodeControl::Pending(future_id))
                }
            }
            value => {
                self.stack.push(value);
                Ok(OpcodeControl::Continue)
            }
        }
    }

    fn complete_execution_target(
        &mut self,
        target: &ExecutionTarget,
    ) -> Result<Option<Value>, RuntimeError> {
        match target {
            ExecutionTarget::TopLevel => Ok(None),
            ExecutionTarget::FrameDepth(frame_depth) => {
                if self.frames.len() == *frame_depth {
                    Ok(Some(self.pop_stack()?))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn run_until_outcome(&mut self, mut pc: usize, target: ExecutionTarget) -> StepOutcome {
        loop {
            if pc >= self.bytecode.opcodes.len() {
                return StepOutcome::RuntimeError(RuntimeError::new("PC out of bounds"));
            }
            self.pc = pc;
            let current_pc = pc;
            let op = self.bytecode.opcodes[current_pc].clone();
            self.observe_opcode_step(current_pc, &op);
            let mut next_pc = current_pc + 1;
            let control = match self.execute_opcode(op.clone(), &mut next_pc) {
                Ok(control) => control,
                Err(err) => {
                    return StepOutcome::RuntimeError(
                        self.enrich_runtime_error(err, current_pc, &op),
                    );
                }
            };
            self.observe_current_depths();

            match control {
                OpcodeControl::Continue => {
                    pc = next_pc;
                    self.pc = pc;
                    match self.complete_execution_target(&target) {
                        Ok(Some(value)) => return StepOutcome::Halt(value),
                        Ok(None) => {}
                        Err(err) => {
                            return StepOutcome::RuntimeError(
                                self.enrich_runtime_error(err, current_pc, &op),
                            );
                        }
                    }
                }
                OpcodeControl::Halt => {
                    self.pc = next_pc;
                    return StepOutcome::Halt(self.stack.last().cloned().unwrap_or(Value::Unit));
                }
                OpcodeControl::Pending(future_id) => {
                    return StepOutcome::Pending {
                        future_id,
                        resume: self.capture_execution_context(current_pc, target),
                    };
                }
            }
        }
    }

    #[allow(dead_code)]
    fn resume_execution(&mut self, context: ProcessExecutionContext) -> StepOutcome {
        let pc = context.pc;
        let target = context.target.clone();
        self.restore_execution_context(context);
        self.run_until_outcome(pc, target)
    }

    fn invoke_callable_step(&mut self, callable: Callable, args: Vec<Value>) -> StepOutcome {
        let mut full_args = callable.lexical_captures;
        full_args.extend(args);

        match callable.target {
            CallableTarget::Builtin(builtin_id) => {
                match call_builtin(self, builtin_id, full_args) {
                    Ok(value) => StepOutcome::Halt(value),
                    Err(err) => StepOutcome::RuntimeError(err),
                }
            }
            CallableTarget::Function(fun_idx) => {
                let entry = match self.function_entry(fun_idx) {
                    Ok(entry) => entry.clone(),
                    Err(err) => return StepOutcome::RuntimeError(err),
                };
                if entry.arity as usize != full_args.len() {
                    return StepOutcome::RuntimeError(RuntimeError::new(format!(
                        "Call arity mismatch for function {}: expected {}, got {}",
                        fun_idx,
                        entry.arity,
                        full_args.len()
                    )));
                }
                if entry.entry_pc as usize >= self.bytecode.opcodes.len() {
                    return StepOutcome::RuntimeError(RuntimeError::new(format!(
                        "Function {} entry_pc out of bounds: {}",
                        fun_idx, entry.entry_pc
                    )));
                }

                let frame_depth = self.frames.len();
                let locals = match Self::build_locals_for_call(&entry, full_args) {
                    Ok(locals) => locals,
                    Err(err) => return StepOutcome::RuntimeError(err),
                };
                let stack_base = self.stack.len();
                self.frames.push(CallFrame {
                    return_pc: usize::MAX,
                    stack_base,
                    call_site: self.current_frame().ok().and_then(|frame| frame.call_site),
                    locals,
                });

                self.run_until_outcome(
                    entry.entry_pc as usize,
                    ExecutionTarget::FrameDepth(frame_depth),
                )
            }
        }
    }

    pub(crate) fn invoke_callable_sync(
        &mut self,
        callable: Callable,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        match self.invoke_callable_step(callable, args) {
            StepOutcome::Halt(value) => Ok(value),
            StepOutcome::Pending { .. } => Err(RuntimeError::new(
                "callable suspended without scheduler support",
            )),
            StepOutcome::RuntimeError(err) => Err(err),
            StepOutcome::Continue => Err(RuntimeError::new("callable execution did not finish")),
        }
    }

    fn ready_future_value(&self, future_id: FutureId) -> Option<Value> {
        self.process_runtime
            .futures
            .get(&future_id)
            .and_then(|future| match &future.state {
                FutureState::Ready(value) | FutureState::Cancelled(value) => Some(value.clone()),
                FutureState::Running => None,
            })
    }

    fn await_task_completion(
        &mut self,
        future_id: FutureId,
        mut outcome: StepOutcome,
    ) -> Result<Value, RuntimeError> {
        loop {
            match outcome {
                StepOutcome::Halt(value) => {
                    self.process_runtime
                        .resolve_future(future_id, value.clone());
                    return self.ready_future_value(future_id).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "task completion future {} did not resolve",
                            future_id
                        ))
                    });
                }
                StepOutcome::Pending {
                    future_id: awaited_future,
                    resume,
                } => {
                    if self.ready_future_value(awaited_future).is_none() {
                        return Err(RuntimeError::new(format!(
                            "task suspended on unresolved future {}",
                            awaited_future
                        )));
                    }
                    outcome = self.resume_execution(resume);
                }
                StepOutcome::RuntimeError(err) => return Err(err),
                StepOutcome::Continue => {
                    return Err(RuntimeError::new("task execution did not finish"));
                }
            }
        }
    }

    pub(crate) fn invoke_task(
        &mut self,
        callable: Callable,
        mode: TaskMode,
    ) -> Result<Value, RuntimeError> {
        self.invoke_task_with_timeout(callable, mode, None)
    }

    pub(crate) fn invoke_task_with_timeout(
        &mut self,
        callable: Callable,
        mode: TaskMode,
        timeout_ms: Option<u64>,
    ) -> Result<Value, RuntimeError> {
        let started = Instant::now();
        match mode {
            TaskMode::Call | TaskMode::Async => {
                let completion_future = self.process_runtime.allocate_future(None, None, false);
                let outcome = self.invoke_callable_step(callable, Vec::new());
                let result = self.await_task_completion(completion_future, outcome)?;
                Ok(self.apply_task_timeout(timeout_ms, started, result))
            }
            TaskMode::Launch => {
                let _ = self.invoke_callable_sync(callable, Vec::new())?;
                Ok(self.apply_task_timeout(timeout_ms, started, ok_vm_result(Value::Unit)))
            }
            TaskMode::Cast => {
                let _ = self.invoke_callable_sync(callable, Vec::new())?;
                Ok(self.apply_task_timeout(timeout_ms, started, ok_vm_result(Value::Unit)))
            }
        }
    }

    fn apply_task_timeout(&self, timeout_ms: Option<u64>, started: Instant, value: Value) -> Value {
        let Some(timeout_ms) = timeout_ms else {
            return value;
        };
        if started.elapsed().as_millis() > u128::from(timeout_ms) {
            err_vm_result(
                self.process_error("Timeout", &format!("task timed out after {}ms", timeout_ms)),
            )
        } else {
            value
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
        dbg_template_base: usize,
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
        let dbg_template_base = u32::try_from(dbg_template_base).map_err(|_| {
            RuntimeError::new(format!(
                "Dbg template base too large for relocation: {}",
                dbg_template_base
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
                Opcode::Dbg { template_id, .. } => {
                    *template_id = template_id.checked_add(dbg_template_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Dbg template relocation overflow: id {} + base {}",
                            *template_id, dbg_template_base
                        ))
                    })?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn render_dbg_output(
        &self,
        template_id: u32,
        values: &[Value],
    ) -> Result<String, RuntimeError> {
        let template = self
            .bytecode
            .dbg_templates
            .iter()
            .find(|template| template.id == template_id)
            .ok_or_else(|| RuntimeError::new(format!("dbg template {} not found", template_id)))?;

        if template.args.len() != values.len() {
            return Err(RuntimeError::new(format!(
                "dbg arg count mismatch: template has {}, runtime has {}",
                template.args.len(),
                values.len()
            )));
        }

        let file = template
            .source_name
            .clone()
            .or_else(|| self.source_file.clone())
            .unwrap_or_else(|| "<unknown>".into());
        let source = self.source.as_deref().unwrap_or_default();
        let args = template
            .args
            .iter()
            .zip(values)
            .map(|(arg, value)| DbgRenderArg {
                span_start: arg.span_start,
                span_end: arg.span_end,
                label: format!(
                    "{}: {}",
                    arg.ty_name,
                    crate::builtin::inspect_value(self, value)
                ),
            })
            .collect::<Vec<_>>();

        Ok(render_dbg_report(&file, source, template, &args))
    }

    fn execute_opcode(
        &mut self,
        op: Opcode,
        pc: &mut usize,
    ) -> Result<OpcodeControl, RuntimeError> {
        match op {
            Opcode::Halt => return Ok(OpcodeControl::Halt),

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
                    metadata: self.callable_metadata_for_builtin(builtin_id),
                }));
            }

            Opcode::LoadFunctionRef(fun_idx) => {
                self.stack.push(Value::Callable(Callable {
                    target: CallableTarget::Function(fun_idx),
                    lexical_captures: Vec::new(),
                    metadata: self.callable_metadata_for_function(fun_idx),
                }));
            }

            Opcode::LoadLocal(slot) => {
                return self.load_local_or_pending(slot);
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
            Opcode::Dbg {
                template_id,
                arg_count,
            } => {
                let mut values = Vec::with_capacity(arg_count as usize);
                for _ in 0..arg_count {
                    values.push(self.pop_stack()?);
                }
                values.reverse();
                let rendered = self.render_dbg_output(template_id, &values)?;
                for line in rendered.lines() {
                    self.emit_stderr_line(line.to_string());
                }
                self.stack.push(Value::Unit);
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
                        callable.metadata = self
                            .promote_partial_apply_metadata(&callable, &callable.lexical_captures);
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

        Ok(OpcodeControl::Continue)
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

#[allow(dead_code)]
impl ProcessRuntime {
    fn from_spec_table(spec_table: &RuntimeProcessSpecTable) -> Self {
        let mut runtime = Self::default();
        runtime.register_spec_table(spec_table);
        runtime
    }

    fn register_spec_table(&mut self, spec_table: &RuntimeProcessSpecTable) {
        self.specs_by_id = spec_table.entries.clone();
        self.spec_id_by_name = spec_table
            .entries
            .iter()
            .enumerate()
            .map(|(idx, spec)| (spec.process_name.clone(), idx as u32))
            .collect();
        self.specs_by_name = spec_table
            .entries
            .iter()
            .cloned()
            .map(|spec| (spec.process_name.clone(), spec))
            .collect();
    }

    fn spec_for_id(&self, spec_id: u32) -> Option<&RuntimeProcessSpec> {
        self.specs_by_id.get(spec_id as usize)
    }

    fn allocate_future(
        &mut self,
        owner: Option<u64>,
        deadline_tick: Option<u64>,
        cancel_on_timeout: bool,
    ) -> FutureId {
        let future_id = self.next_future_id;
        self.next_future_id += 1;
        if let Some(deadline_tick) = deadline_tick {
            self.deadline_queue.push_back(DeadlineEntry {
                deadline_tick,
                future_id,
            });
        }
        self.futures.insert(
            future_id,
            FutureRecord {
                id: future_id,
                owner,
                state: FutureState::Running,
                deadline_tick,
                waiters: Vec::new(),
                cancel_on_timeout,
                correlation_id: None,
            },
        );
        future_id
    }

    fn allocate_correlation_id(&mut self) -> CorrelationId {
        let correlation_id = self.next_correlation_id;
        self.next_correlation_id += 1;
        correlation_id
    }

    fn register_reply_waiter(&mut self, correlation_id: CorrelationId, future_id: FutureId) {
        self.reply_table.insert(correlation_id, future_id);
        if let Some(future) = self.futures.get_mut(&future_id) {
            future.correlation_id = Some(correlation_id);
        }
    }

    fn mark_process_waiting(&mut self, pid: u64, reason: ProcessWaitReason) {
        if let Some(process) = self.processes.get_mut(&pid) {
            process.status = ProcessStatus::Waiting(reason.clone());
        }
        if let ProcessWaitReason::Future(future_id) = reason.clone() {
            if let Some(future) = self.futures.get_mut(&future_id) {
                if !future.waiters.contains(&pid) {
                    future.waiters.push(pid);
                }
            }
        }
        self.waiting_table.insert(pid, reason);
    }

    fn resolve_future(&mut self, future_id: FutureId, value: Value) -> Vec<u64> {
        let Some(future) = self.futures.get_mut(&future_id) else {
            return Vec::new();
        };
        future.state = FutureState::Ready(value);
        if let Some(correlation_id) = future.correlation_id.take() {
            self.reply_table.remove(&correlation_id);
        }
        let waiters = std::mem::take(&mut future.waiters);
        for waiter in &waiters {
            self.waiting_table.remove(waiter);
            if let Some(process) = self.processes.get_mut(waiter) {
                process.status = ProcessStatus::Runnable;
            }
        }
        waiters
    }

    fn resolve_reply(&mut self, correlation_id: CorrelationId, value: Value) -> Vec<u64> {
        let Some(future_id) = self.reply_table.remove(&correlation_id) else {
            return Vec::new();
        };
        self.resolve_future(future_id, value)
    }

    fn collect_expired_futures(&mut self, now_tick: u64) -> Vec<FutureId> {
        let mut expired = Vec::new();
        let mut retained = VecDeque::with_capacity(self.deadline_queue.len());
        while let Some(entry) = self.deadline_queue.pop_front() {
            if entry.deadline_tick <= now_tick {
                let is_running = self
                    .futures
                    .get(&entry.future_id)
                    .is_some_and(|future| matches!(future.state, FutureState::Running));
                if is_running {
                    if let Some(future) = self.futures.get_mut(&entry.future_id) {
                        if let Some(correlation_id) = future.correlation_id.take() {
                            self.reply_table.remove(&correlation_id);
                        }
                    }
                    expired.push(entry.future_id);
                }
            } else {
                retained.push_back(entry);
            }
        }
        self.deadline_queue = retained;
        expired
    }
}

fn ok_vm_result(value: Value) -> Value {
    Value::Tagged {
        tag: 0,
        fields: vec![value],
    }
}

fn err_vm_result(rich: RichError) -> Value {
    Value::Tagged {
        tag: 1,
        fields: vec![Value::Error(Box::new(rich))],
    }
}

fn decode_vm_result(
    value: Value,
    builtin_name: &str,
    arg_name: &str,
) -> Result<Result<Value, RichError>, RuntimeError> {
    match value {
        Value::Tagged { tag: 0, fields } => match fields.as_slice() {
            [inner] => Ok(Ok(inner.clone())),
            other => Err(RuntimeError::new(format!(
                "{builtin_name} expects Ok with exactly one field for {arg_name}, got {}",
                other.len()
            ))),
        },
        Value::Tagged { tag: 1, fields } => match fields.as_slice() {
            [Value::Error(rich)] => Ok(Err((**rich).clone())),
            [other] => Err(RuntimeError::new(format!(
                "{builtin_name} expects Err(Error) for {arg_name}, got Err({:?})",
                other
            ))),
            other => Err(RuntimeError::new(format!(
                "{builtin_name} expects Err with exactly one field for {arg_name}, got {}",
                other.len()
            ))),
        },
        other => Err(RuntimeError::new(format!(
            "{builtin_name} expects Result as {arg_name}, got {:?}",
            other
        ))),
    }
}

fn split_qualified_name_owned(qualified_name: &str) -> (Option<String>, Option<String>) {
    match qualified_name.rsplit_once("::") {
        Some((module, name)) if !module.is_empty() => {
            (Some(module.to_string()), Some(name.to_string()))
        }
        _ if qualified_name.is_empty() => (None, None),
        _ => (
            Some("<local>".to_string()),
            Some(qualified_name.to_string()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcessWaitReason, StepOutcome, TaskMode, VmObservationOptions, VM};
    use sindr::ir::{
        Bytecode, BytecodeChunk, Constant, ErrTemplate, FunctionEntry, Opcode, OpcodeSource,
        RuntimeProcessInstance, RuntimeProcessKind, RuntimeProcessSpec, RuntimeProcessSpecTable,
        SourceMap,
    };
    use sindr::primitives::int;
    use sindr::runtime::{
        Callable, CallableMetadata, CallableTarget, PidHandle, TypeEntry, TypeKind, TypeRegistry,
        Value,
    };

    fn base_bytecode(opcodes: Vec<Opcode>) -> Bytecode {
        Bytecode {
            opcodes,
            type_registry: TypeRegistry::new(),
            ..Bytecode::default()
        }
    }

    fn singleton_boot_bytecode(
        process_name: &str,
        kind: RuntimeProcessKind,
        lazy: bool,
        boot: bool,
        init_opcodes: Vec<Opcode>,
        constants: Vec<Constant>,
    ) -> Bytecode {
        let mut opcodes = vec![Opcode::Halt];
        opcodes.extend(init_opcodes);
        let mut bytecode = base_bytecode(opcodes);
        bytecode.constants = constants;
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Agents::__agent_init"))];
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![RuntimeProcessSpec {
                process_name: process_name.into(),
                module_path: "Agents".into(),
                kind,
                instance: RuntimeProcessInstance::Singleton,
                boot,
                registry: true,
                lazy,
                init_fun_idx: 0,
                get_fun_idx: 1,
                set_fun_idx: None,
            }],
        };
        bytecode
    }

    #[test]
    fn vm_new_registers_runtime_process_specs_from_bytecode() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![RuntimeProcessSpec {
                process_name: "Counter".into(),
                module_path: "Counter".into(),
                kind: RuntimeProcessKind::StateAgent,
                instance: RuntimeProcessInstance::Singleton,
                boot: true,
                registry: true,
                lazy: false,
                init_fun_idx: 0,
                get_fun_idx: 1,
                set_fun_idx: Some(2),
            }],
        };

        let vm = VM::new(bytecode);
        let spec = vm
            .process_runtime
            .specs_by_name
            .get("Counter")
            .expect("process spec should be registered");
        assert_eq!(spec.module_path, "Counter");
        assert_eq!(spec.init_fun_idx, 0);
        assert_eq!(spec.get_fun_idx, 1);
        assert_eq!(spec.set_fun_idx, Some(2));
        assert_eq!(vm.process_runtime.spec_id_by_name.get("Counter"), Some(&0));
        assert_eq!(
            vm.process_runtime
                .spec_for_id(0)
                .expect("spec id 0 should resolve")
                .process_name,
            "Counter"
        );
    }

    #[test]
    fn allocate_process_creates_process_instance_with_runtime_shape() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![RuntimeProcessSpec {
                process_name: "Counter".into(),
                module_path: "Counter".into(),
                kind: RuntimeProcessKind::StateAgent,
                instance: RuntimeProcessInstance::Singleton,
                boot: true,
                registry: true,
                lazy: false,
                init_fun_idx: 0,
                get_fun_idx: 1,
                set_fun_idx: Some(2),
            }],
        };

        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Counter".into(), Some(Value::Int(int(41))))
            .expect("process allocation should succeed");
        let instance = vm
            .process_runtime
            .processes
            .get(&pid)
            .expect("process instance should be stored");

        assert_eq!(instance.pid, pid);
        assert_eq!(instance.spec_id, 0);
        assert_eq!(instance.status, super::ProcessStatus::Runnable);
        assert!(instance.mailbox.is_empty());
        assert!(instance.execution_context.is_none());
        assert_eq!(instance.state_value, Some(Value::Int(int(41))));
        assert_eq!(instance.owner, None);
        assert!(!instance.lazy_state_pending);
    }

    #[test]
    fn process_state_and_store_validate_pid_against_registered_spec() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                RuntimeProcessSpec {
                    process_name: "Counter".into(),
                    module_path: "Counter".into(),
                    kind: RuntimeProcessKind::StateAgent,
                    instance: RuntimeProcessInstance::Singleton,
                    boot: true,
                    registry: true,
                    lazy: false,
                    init_fun_idx: 0,
                    get_fun_idx: 1,
                    set_fun_idx: Some(2),
                },
                RuntimeProcessSpec {
                    process_name: "Clock".into(),
                    module_path: "Clock".into(),
                    kind: RuntimeProcessKind::StateAgent,
                    instance: RuntimeProcessInstance::Singleton,
                    boot: true,
                    registry: true,
                    lazy: false,
                    init_fun_idx: 3,
                    get_fun_idx: 4,
                    set_fun_idx: Some(5),
                },
            ],
        };

        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Counter".into(), Some(Value::Int(int(41))))
            .expect("process allocation should succeed");

        let ok_pid = PidHandle {
            id: pid,
            process_name: "Counter".into(),
        };
        let wrong_pid = PidHandle {
            id: pid,
            process_name: "Clock".into(),
        };

        assert_eq!(
            vm.process_state(&ok_pid)
                .expect("state lookup should succeed"),
            super::ok_vm_result(Value::Int(int(41)))
        );
        let mismatch = vm
            .process_state(&wrong_pid)
            .expect("mismatch should still return Err(Result)");
        assert!(matches!(mismatch, Value::Tagged { tag: 1, .. }));

        assert_eq!(
            vm.process_store(&ok_pid, Value::Int(int(99)))
                .expect("store should succeed"),
            super::ok_vm_result(Value::Unit)
        );
        assert_eq!(
            vm.process_state(&ok_pid)
                .expect("updated state should succeed"),
            super::ok_vm_result(Value::Int(int(99)))
        );
    }

    #[test]
    fn vm_new_initializes_empty_future_runtime_tables() {
        let vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        assert!(vm.process_runtime.futures.is_empty());
        assert!(vm.process_runtime.reply_table.is_empty());
        assert!(vm.process_runtime.waiting_table.is_empty());
        assert!(vm.process_runtime.deadline_queue.is_empty());
    }

    #[test]
    fn allocate_future_records_owner_deadline_and_flags() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let future_id = vm.process_runtime.allocate_future(Some(7), Some(42), true);
        let future = vm
            .process_runtime
            .futures
            .get(&future_id)
            .expect("future should be recorded");
        assert_eq!(future.id, future_id);
        assert_eq!(future.owner, Some(7));
        assert_eq!(future.deadline_tick, Some(42));
        assert!(future.cancel_on_timeout);
        assert!(matches!(future.state, super::FutureState::Running));
        assert_eq!(
            vm.process_runtime.deadline_queue.front(),
            Some(&super::DeadlineEntry {
                deadline_tick: 42,
                future_id,
            })
        );
    }

    #[test]
    fn register_reply_future_and_resolve_reply_marks_future_ready() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let future_id = vm.process_runtime.allocate_future(None, None, false);
        let correlation_id = vm.process_runtime.allocate_correlation_id();
        vm.process_runtime
            .register_reply_waiter(correlation_id, future_id);

        assert_eq!(
            vm.process_runtime.reply_table.get(&correlation_id),
            Some(&future_id)
        );

        let resumed = vm
            .process_runtime
            .resolve_reply(correlation_id, super::ok_vm_result(Value::Int(int(99))));
        assert!(resumed.is_empty());
        assert!(!vm.process_runtime.reply_table.contains_key(&correlation_id));
        assert!(matches!(
            vm.process_runtime
                .futures
                .get(&future_id)
                .expect("future should remain tracked")
                .state,
            super::FutureState::Ready(Value::Tagged { tag: 0, .. })
        ));
    }

    #[test]
    fn attach_waiter_updates_waiting_table_and_future_record() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![RuntimeProcessSpec {
                process_name: "Counter".into(),
                module_path: "Counter".into(),
                kind: RuntimeProcessKind::StateAgent,
                instance: RuntimeProcessInstance::Singleton,
                boot: true,
                registry: true,
                lazy: false,
                init_fun_idx: 0,
                get_fun_idx: 1,
                set_fun_idx: Some(2),
            }],
        };
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Counter".into(), Some(Value::Int(int(41))))
            .expect("process allocation should succeed");
        let future_id = vm.process_runtime.allocate_future(Some(pid), None, false);
        vm.process_runtime
            .mark_process_waiting(pid, super::ProcessWaitReason::Future(future_id));

        assert_eq!(
            vm.process_runtime.waiting_table.get(&pid),
            Some(&super::ProcessWaitReason::Future(future_id))
        );
        assert_eq!(
            vm.process_runtime
                .futures
                .get(&future_id)
                .expect("future should exist")
                .waiters,
            vec![pid]
        );
        assert!(matches!(
            vm.process_runtime
                .processes
                .get(&pid)
                .expect("process should exist")
                .status,
            super::ProcessStatus::Waiting(super::ProcessWaitReason::Future(id)) if id == future_id
        ));
    }

    #[test]
    fn expire_deadlines_marks_timeout_err_and_clears_reply_mapping() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![RuntimeProcessSpec {
                process_name: "Counter".into(),
                module_path: "Counter".into(),
                kind: RuntimeProcessKind::StateAgent,
                instance: RuntimeProcessInstance::Singleton,
                boot: true,
                registry: true,
                lazy: false,
                init_fun_idx: 0,
                get_fun_idx: 1,
                set_fun_idx: Some(2),
            }],
        };
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Counter".into(), Some(Value::Int(int(41))))
            .expect("process allocation should succeed");
        let future_id = vm.process_runtime.allocate_future(Some(pid), Some(3), true);
        let correlation_id = vm.process_runtime.allocate_correlation_id();
        vm.process_runtime
            .register_reply_waiter(correlation_id, future_id);
        vm.process_runtime
            .mark_process_waiting(pid, super::ProcessWaitReason::Future(future_id));

        let expired = vm.expire_process_deadlines(3);
        assert_eq!(expired, vec![future_id]);
        assert!(!vm.process_runtime.reply_table.contains_key(&correlation_id));
        assert_eq!(vm.process_runtime.waiting_table.get(&pid), None);
        assert!(matches!(
            vm.process_runtime
                .futures
                .get(&future_id)
                .expect("future should exist")
                .state,
            super::FutureState::Ready(Value::Tagged { tag: 1, .. })
        ));
    }

    #[test]
    fn process_runtime_future_tables_rollback_with_vm_checkpoint() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let checkpoint = vm.checkpoint_for_chunk(&BytecodeChunk {
            opcodes: Vec::new(),
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
        });
        let future_id = vm.process_runtime.allocate_future(None, Some(9), true);
        let correlation_id = vm.process_runtime.allocate_correlation_id();
        vm.process_runtime
            .register_reply_waiter(correlation_id, future_id);

        assert!(vm.process_runtime.futures.contains_key(&future_id));
        assert!(vm.process_runtime.reply_table.contains_key(&correlation_id));

        vm.rollback_to_checkpoint(checkpoint);
        assert!(vm.process_runtime.futures.is_empty());
        assert!(vm.process_runtime.reply_table.is_empty());
        assert!(vm.process_runtime.deadline_queue.is_empty());
    }

    #[test]
    fn root_supervisor_eagerly_boots_singleton_before_run() {
        let bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::StateAgent,
            false,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Int(int(41))],
        );
        let mut vm = VM::new(bytecode);

        vm.ensure_root_supervisor_booted()
            .expect("boot should succeed");

        let pid = vm
            .process_runtime
            .singleton_by_name
            .get("Counter")
            .copied()
            .expect("singleton slot should be registered");
        let instance = vm
            .process_runtime
            .processes
            .get(&pid)
            .expect("booted singleton process should exist");
        assert_eq!(instance.state_value, Some(Value::Int(int(41))));
        assert!(!instance.lazy_state_pending);
    }

    #[test]
    fn lazy_readonly_singleton_materializes_state_on_first_access() {
        let bytecode = singleton_boot_bytecode(
            "Env",
            RuntimeProcessKind::ReadOnlyAgent,
            true,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Str("ready".into())],
        );
        let mut vm = VM::new(bytecode);

        vm.ensure_root_supervisor_booted()
            .expect("boot should register lazy singleton");

        let pid = vm
            .process_runtime
            .singleton_by_name
            .get("Env")
            .copied()
            .expect("singleton slot should be registered");
        let instance = vm
            .process_runtime
            .processes
            .get(&pid)
            .expect("lazy singleton process should exist");
        assert_eq!(instance.state_value, None);
        assert!(instance.lazy_state_pending);

        let value = vm
            .process_state(&PidHandle {
                id: pid,
                process_name: "Env".into(),
            })
            .expect("lazy state access should succeed");
        assert_eq!(value, super::ok_vm_result(Value::Str("ready".into())));
        let instance = vm
            .process_runtime
            .processes
            .get(&pid)
            .expect("lazy singleton process should remain registered");
        assert_eq!(instance.state_value, Some(Value::Str("ready".into())));
        assert!(!instance.lazy_state_pending);
    }

    #[test]
    fn boot_failure_keeps_singleton_unpublished() {
        let bytecode = singleton_boot_bytecode(
            "Broken",
            RuntimeProcessKind::StateAgent,
            false,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::MakeErrorLiteral {
                    kind_const_idx: 1,
                    message_const_idx: 2,
                },
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![
                Constant::Tag(1),
                Constant::Str("BootFailure".into()),
                Constant::Str("bad boot".into()),
            ],
        );
        let mut vm = VM::new(bytecode);

        let err = vm
            .ensure_root_supervisor_booted()
            .expect_err("boot should fail");
        assert!(err.message.contains("Broken"));
        assert!(err.message.contains("bad boot"));
        assert!(!vm.process_runtime.singleton_by_name.contains_key("Broken"));
        assert_eq!(
            vm.process_runtime
                .root_supervisor
                .boot_failures
                .get("Broken")
                .map(String::as_str),
            Some("bad boot")
        );
    }

    #[test]
    fn root_supervisor_boot_preserves_execution_context() {
        let bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::StateAgent,
            false,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Int(int(41))],
        );
        let mut vm = VM::new(bytecode);
        vm.stack.push(Value::Int(int(7)));
        vm.pc = 0;

        vm.ensure_root_supervisor_booted()
            .expect("boot should succeed");

        assert_eq!(vm.pc, 0);
        assert_eq!(vm.frames.len(), 1);
        assert_eq!(vm.stack, vec![Value::Int(int(7))]);
    }

    #[test]
    fn root_supervisor_rolls_back_partial_singleton_publication_on_failure() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
            Opcode::LoadConst(2),
            Opcode::MakeErrorLiteral {
                kind_const_idx: 3,
                message_const_idx: 4,
            },
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![
            Constant::Tag(0),
            Constant::Int(int(1)),
            Constant::Tag(1),
            Constant::Str("BootFailure".into()),
            Constant::Str("bad boot".into()),
        ];
        bytecode.functions = vec![
            function_entry(0, 1, 0, 0, Some("Agents::good_init")),
            function_entry(1, 5, 0, 0, Some("Agents::broken_init")),
        ];
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                RuntimeProcessSpec {
                    process_name: "Good".into(),
                    module_path: "Agents".into(),
                    kind: RuntimeProcessKind::StateAgent,
                    instance: RuntimeProcessInstance::Singleton,
                    boot: true,
                    registry: true,
                    lazy: false,
                    init_fun_idx: 0,
                    get_fun_idx: 2,
                    set_fun_idx: None,
                },
                RuntimeProcessSpec {
                    process_name: "Broken".into(),
                    module_path: "Agents".into(),
                    kind: RuntimeProcessKind::StateAgent,
                    instance: RuntimeProcessInstance::Singleton,
                    boot: true,
                    registry: true,
                    lazy: false,
                    init_fun_idx: 1,
                    get_fun_idx: 3,
                    set_fun_idx: None,
                },
            ],
        };
        let mut vm = VM::new(bytecode);

        let err = vm
            .ensure_root_supervisor_booted()
            .expect_err("boot should fail");

        assert!(err.message.contains("Broken"));
        assert!(vm.process_runtime.singleton_by_name.is_empty());
        assert!(vm.process_runtime.processes.is_empty());
        assert_eq!(
            vm.process_runtime
                .root_supervisor
                .boot_failures
                .get("Broken")
                .map(String::as_str),
            Some("bad boot")
        );
    }

    #[test]
    fn invoke_callable_step_returns_pending_with_resume_context() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 1, 1, 1, Some("Main::await_value"))];
        let mut vm = VM::new(bytecode);
        let future_id = vm.process_runtime.allocate_future(None, None, false);
        let callable = Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        };

        let outcome = vm.invoke_callable_step(callable, vec![Value::PendingFuture(future_id)]);
        match outcome {
            StepOutcome::Pending {
                future_id: pending_id,
                resume,
            } => {
                assert_eq!(pending_id, future_id);
                assert_eq!(resume.pc, 1);
                assert!(resume.stack.is_empty());
                assert_eq!(resume.frames.len(), 2);
                assert!(matches!(
                    resume.frames.last().and_then(|frame| frame.locals.first()),
                    Some(Value::PendingFuture(id)) if *id == future_id
                ));
            }
            other => panic!("expected pending outcome, got {other:?}"),
        }
    }

    #[test]
    fn resume_execution_reloads_ready_future_value_and_returns_result() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 1, 1, 1, Some("Main::await_value"))];
        let mut vm = VM::new(bytecode);
        let future_id = vm.process_runtime.allocate_future(None, None, false);
        let callable = Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        };

        let resume = match vm.invoke_callable_step(callable, vec![Value::PendingFuture(future_id)])
        {
            StepOutcome::Pending { resume, .. } => resume,
            other => panic!("expected pending outcome, got {other:?}"),
        };

        let resumed = vm
            .process_runtime
            .resolve_future(future_id, Value::Int(int(41)));
        assert!(resumed.is_empty());

        match vm.resume_execution(resume) {
            StepOutcome::Halt(Value::Int(value)) => assert_eq!(value, int(41)),
            other => panic!("expected resumed halt value, got {other:?}"),
        }
        assert_eq!(vm.frames.len(), 1);
    }

    #[test]
    fn pending_local_preserves_left_to_right_evaluation_until_resume() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::AddInt,
            Opcode::Return,
        ]);
        bytecode.functions = vec![function_entry(0, 1, 2, 2, Some("Main::await_add"))];
        let mut vm = VM::new(bytecode);
        let future_id = vm.process_runtime.allocate_future(None, None, false);
        let callable = Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        };

        let outcome = vm.invoke_callable_step(
            callable,
            vec![Value::PendingFuture(future_id), Value::Int(int(1))],
        );
        let resume = match outcome {
            StepOutcome::Pending { resume, .. } => {
                assert!(resume.stack.is_empty());
                assert_eq!(resume.pc, 1);
                assert!(matches!(
                    resume.frames.last().map(|frame| frame.locals.as_slice()),
                    Some([Value::PendingFuture(id), Value::Int(value)]) if *id == future_id && *value == int(1)
                ));
                resume
            }
            other => panic!("expected pending outcome, got {other:?}"),
        };

        let resumed = vm
            .process_runtime
            .resolve_future(future_id, Value::Int(int(41)));
        assert!(resumed.is_empty());

        match vm.resume_execution(resume) {
            StepOutcome::Halt(Value::Int(value)) => assert_eq!(value, int(42)),
            other => panic!("expected resumed halt value, got {other:?}"),
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
    fn dbg_opcode_writes_to_stderr_and_returns_unit() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::Dbg {
                template_id: 0,
                arg_count: 1,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(42))];
        bytecode.dbg_templates = vec![sindr::ir::DbgTemplate {
            id: 0,
            span_start: 0,
            span_end: 7,
            source_name: Some("sample.srt".into()),
            args: vec![sindr::ir::DbgArgTemplate {
                span_start: 5,
                span_end: 6,
                ty_name: "Int".into(),
            }],
        }];

        let mut vm = VM::new(bytecode)
            .with_source("dbg!(42)".into(), "sample.srt".into())
            .with_error_capture();
        vm.run().expect("run should succeed");
        assert_eq!(vm.last_value(), Some(&Value::Unit));
        let stderr = vm.take_stderr().join("\n");
        assert!(stderr.contains("Debug:"), "{stderr}");
        assert!(stderr.contains("inspect values."), "{stderr}");
        assert!(stderr.contains("sample.srt:1:1"), "{stderr}");
        assert!(stderr.contains("Int: 42"));
    }

    #[test]
    fn dbg_opcode_renders_argument_labels_left_to_right() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::Dbg {
                template_id: 0,
                arg_count: 2,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(2)), Constant::Str("hoge".into())];
        bytecode.dbg_templates = vec![sindr::ir::DbgTemplate {
            id: 0,
            span_start: 0,
            span_end: 15,
            source_name: Some("sample.srt".into()),
            args: vec![
                sindr::ir::DbgArgTemplate {
                    span_start: 5,
                    span_end: 8,
                    ty_name: "Int".into(),
                },
                sindr::ir::DbgArgTemplate {
                    span_start: 10,
                    span_end: 14,
                    ty_name: "String".into(),
                },
            ],
        }];

        let mut vm = VM::new(bytecode)
            .with_source("dbg!(num, term)".into(), "sample.srt".into())
            .with_error_capture();
        vm.run().expect("run should succeed");
        let stderr = vm.take_stderr().join("\n");
        let int_idx = stderr.find("Int: 2").expect("Int label should render");
        let string_idx = stderr
            .find("String: \"hoge\"")
            .expect("String label should render");
        assert!(int_idx < string_idx, "{stderr}");
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
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
            runtime_process_specs: Vec::new(),
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
                dbg_template_base: 0,
                dbg_templates: Vec::new(),
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
                runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: vec![function_entry(0, 2, 1, 0, Some("new"))],
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            private_flags: vec![],
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
            private_flags: vec![],
        });
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "B".into(),
            kind: TypeKind::Record,
            field_names: vec![],
            private_flags: vec![],
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
                private_flags: vec![],
            }],
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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
            private_flags: vec![],
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
                private_flags: vec![],
            }],
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            runtime_process_specs: Vec::new(),
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

    #[test]
    fn observation_includes_process_runtime_counters() {
        let bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::StateAgent,
            false,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Int(int(41))],
        );
        let mut vm = VM::new(bytecode);
        vm.enable_observation(VmObservationOptions::default());
        vm.ensure_root_supervisor_booted()
            .expect("boot should succeed");

        let pid = vm
            .process_runtime
            .singleton_by_name
            .get("Counter")
            .copied()
            .expect("singleton pid should exist");
        let future_id = vm.process_runtime.allocate_future(Some(pid), Some(9), true);
        let correlation_id = vm.process_runtime.allocate_correlation_id();
        vm.process_runtime
            .register_reply_waiter(correlation_id, future_id);
        vm.process_runtime
            .mark_process_waiting(pid, ProcessWaitReason::Reply(correlation_id));

        let observation = vm.observation().expect("observation should exist");
        assert_eq!(observation.stats.process.process_spec_count, 1);
        assert_eq!(observation.stats.process.singleton_slot_count, 1);
        assert_eq!(observation.stats.process.process_count, 1);
        assert_eq!(observation.stats.process.waiting_process_count, 1);
        assert_eq!(observation.stats.process.future_count, 1);
        assert_eq!(observation.stats.process.running_future_count, 1);
        assert_eq!(observation.stats.process.waiting_table_count, 1);
        assert_eq!(observation.stats.process.reply_waiter_count, 1);
        assert_eq!(observation.stats.process.deadline_queue_count, 1);
    }

    #[test]
    fn process_runtime_snapshot_includes_runtime_tables() {
        let bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::StateAgent,
            false,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Int(int(41))],
        );
        let mut vm = VM::new(bytecode);
        vm.ensure_root_supervisor_booted()
            .expect("boot should succeed");

        let pid = vm
            .process_runtime
            .singleton_by_name
            .get("Counter")
            .copied()
            .expect("singleton pid should exist");
        let future_id = vm.process_runtime.allocate_future(Some(pid), Some(9), true);
        let correlation_id = vm.process_runtime.allocate_correlation_id();
        vm.process_runtime
            .register_reply_waiter(correlation_id, future_id);
        vm.process_runtime
            .mark_process_waiting(pid, ProcessWaitReason::Reply(correlation_id));

        let snapshot = vm.process_runtime_snapshot();
        assert_eq!(snapshot.specs.len(), 1);
        assert_eq!(snapshot.specs[0].process_name, "Counter");
        assert_eq!(snapshot.singleton_slots.get("Counter"), Some(&pid));
        assert_eq!(snapshot.processes.len(), 1);
        assert_eq!(snapshot.processes[0].process_name, "Counter");
        assert_eq!(snapshot.processes[0].status, "waiting");
        assert_eq!(snapshot.processes[0].state_value.as_deref(), Some("41"));
        assert_eq!(
            snapshot.waiting.get(&pid).map(String::as_str),
            Some("reply")
        );
        assert_eq!(snapshot.replies.get(&correlation_id), Some(&future_id));
        assert_eq!(snapshot.deadlines.len(), 1);
        assert_eq!(snapshot.deadlines[0].future_id, future_id);
        assert_eq!(snapshot.futures.len(), 1);
        assert_eq!(snapshot.futures[0].state, "running");
        assert_eq!(snapshot.futures[0].owner, Some(pid));
    }

    #[test]
    fn stress_process_runtime_snapshot_handles_many_singletons_spawns_and_tasks() {
        let singleton_count = 48u32;
        let spawn_count = 96usize;
        let task_count = 48usize;

        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
            Opcode::LoadConst(0),
            Opcode::LoadConst(2),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
            Opcode::LoadConst(0),
            Opcode::LoadConst(3),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![
            Constant::Tag(0),
            Constant::Int(int(1)),
            Constant::Int(int(2)),
            Constant::Unit,
        ];
        bytecode.functions = vec![
            function_entry(0, 1, 0, 0, Some("Agents::singleton_init")),
            function_entry(1, 5, 0, 0, Some("Worker::__agent_init")),
            function_entry(2, 9, 0, 0, Some("Task::body")),
        ];

        let mut specs = (0..singleton_count)
            .map(|idx| RuntimeProcessSpec {
                process_name: format!("Singleton{idx}"),
                module_path: "Agents".into(),
                kind: RuntimeProcessKind::StateAgent,
                instance: RuntimeProcessInstance::Singleton,
                boot: true,
                registry: true,
                lazy: false,
                init_fun_idx: 0,
                get_fun_idx: 0,
                set_fun_idx: None,
            })
            .collect::<Vec<_>>();
        specs.push(RuntimeProcessSpec {
            process_name: "Worker".into(),
            module_path: "Worker".into(),
            kind: RuntimeProcessKind::StateAgent,
            instance: RuntimeProcessInstance::Multi,
            boot: false,
            registry: true,
            lazy: false,
            init_fun_idx: 1,
            get_fun_idx: 1,
            set_fun_idx: None,
        });
        bytecode.runtime_process_specs = RuntimeProcessSpecTable { entries: specs };

        let mut vm = VM::new(bytecode);
        vm.enable_observation(VmObservationOptions::default());
        vm.ensure_root_supervisor_booted()
            .expect("singleton boot stress should succeed");

        let worker_init = vm.callable_for_function(1);
        let task_body = vm.callable_for_function(2);
        for _ in 0..spawn_count {
            let value = vm
                .process_spawn("Worker".into(), worker_init.clone())
                .expect("spawn should succeed");
            assert!(matches!(
                value,
                Value::Tagged { tag: 0, fields } if matches!(fields.first(), Some(Value::Pid(_)))
            ));
        }
        for _ in 0..task_count {
            vm.invoke_task(task_body.clone(), TaskMode::Call)
                .expect("task call should succeed");
            vm.invoke_task(task_body.clone(), TaskMode::Async)
                .expect("task async should succeed");
            vm.invoke_task(task_body.clone(), TaskMode::Launch)
                .expect("task launch should succeed");
            vm.invoke_task(task_body.clone(), TaskMode::Cast)
                .expect("task cast should succeed");
        }

        let snapshot = vm.process_runtime_snapshot();
        assert_eq!(
            snapshot.counters.process_spec_count,
            singleton_count as usize + 1
        );
        assert_eq!(
            snapshot.counters.singleton_slot_count,
            singleton_count as usize
        );
        assert_eq!(
            snapshot.counters.process_count,
            singleton_count as usize + spawn_count
        );
        assert_eq!(snapshot.counters.future_count, task_count * 2);
        assert_eq!(snapshot.counters.ready_future_count, task_count * 2);
        assert_eq!(snapshot.specs.len(), singleton_count as usize + 1);
        assert_eq!(
            snapshot.processes.len(),
            singleton_count as usize + spawn_count
        );
        assert_eq!(snapshot.futures.len(), task_count * 2);

        let observation = vm.observation().expect("observation should exist");
        assert_eq!(
            observation.stats.process.process_count,
            singleton_count as usize + spawn_count
        );
        assert_eq!(observation.stats.process.future_count, task_count * 2);
    }
}
